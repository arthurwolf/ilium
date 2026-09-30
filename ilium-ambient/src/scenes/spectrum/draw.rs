//! Drawing of the eight spectrum styles into the Braille dot raster, and the
//! optional per-cell colouring. Pure functions of their inputs: no clock, no
//! I/O. Intensities use the full 0..1 range so the host's dither shows bar
//! bodies as fine halftone and tips as solid dots.

use super::{ColorMode, Orientation, SpectrumSettings, SpectrumStyle};
use crate::raster::Raster;
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

/// Rolling spectrogram columns, newest last. One column holds the normalized
/// value of every band at one instant.
#[derive(Debug, Default)]
pub struct SpectrogramHistory {
    columns: VecDeque<Vec<f32>>,
    capacity: usize,
}

impl SpectrogramHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            columns: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, column: &[f32]) {
        if self.columns.len() >= self.capacity {
            self.columns.pop_front();
        }
        self.columns.push_back(column.to_vec());
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Column `age` steps back from the newest (0 = newest).
    pub fn column(&self, age: usize) -> Option<&[f32]> {
        let index = self.columns.len().checked_sub(age + 1)?;
        self.columns.get(index).map(Vec::as_slice)
    }
}

/// Smoothed energy of the bass, mid and treble regions, each 0..1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EnergyLevels {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
}

/// An expanding ring launched by a bass hit. `age` runs 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ripple {
    pub age: f32,
    pub strength: f32,
}

pub struct DrawInput<'a> {
    pub values: &'a [f32],
    pub peaks: &'a [f32],
    /// Oscilloscope samples already scaled to about -1..1.
    pub waveform: &'a [f32],
    pub history: &'a SpectrogramHistory,
    pub levels: EnergyLevels,
    pub ripples: &'a [Ripple],
}

/// Size of the virtual (band axis x height axis) canvas for an orientation.
pub fn virtual_size(
    orientation: Orientation,
    raster_width: usize,
    raster_height: usize,
) -> (usize, usize) {
    match orientation {
        Orientation::BottomUp | Orientation::TopDown => (raster_width, raster_height),
        Orientation::LeftToRight | Orientation::RightToLeft => (raster_height, raster_width),
    }
}

/// Virtual (vx along the band axis, vy = height above the baseline) to raster.
pub fn to_raster(
    orientation: Orientation,
    raster_width: usize,
    raster_height: usize,
    vx: usize,
    vy: usize,
) -> (usize, usize) {
    match orientation {
        Orientation::BottomUp => (vx, raster_height - 1 - vy),
        Orientation::TopDown => (vx, vy),
        Orientation::LeftToRight => (vy, raster_height - 1 - vx),
        Orientation::RightToLeft => (raster_width - 1 - vy, raster_height - 1 - vx),
    }
}

/// Inverse of `to_raster`.
pub fn from_raster(
    orientation: Orientation,
    raster_width: usize,
    raster_height: usize,
    x: usize,
    y: usize,
) -> (usize, usize) {
    match orientation {
        Orientation::BottomUp => (x, raster_height - 1 - y),
        Orientation::TopDown => (x, y),
        Orientation::LeftToRight => (raster_height - 1 - y, x),
        Orientation::RightToLeft => (raster_height - 1 - y, raster_width - 1 - x),
    }
}

/// A drawing surface in oriented coordinates: bands run along x, magnitude
/// grows along y from the baseline.
struct Canvas<'a> {
    raster: &'a mut Raster,
    orientation: Orientation,
    width: usize,
    height: usize,
}

impl<'a> Canvas<'a> {
    fn new(raster: &'a mut Raster, orientation: Orientation) -> Self {
        let (width, height) = virtual_size(orientation, raster.width, raster.height);
        Self {
            raster,
            orientation,
            width,
            height,
        }
    }

    fn plot(&mut self, vx: isize, vy: isize, intensity: f32) {
        if vx < 0 || vy < 0 || vx as usize >= self.width || vy as usize >= self.height {
            return;
        }
        let (x, y) = to_raster(
            self.orientation,
            self.raster.width,
            self.raster.height,
            vx as usize,
            vy as usize,
        );
        let index = y * self.raster.width + x;
        self.raster.dots[index] = self.raster.dots[index].max(intensity);
    }

    /// Anti-aliased vertical run at column `vx` covering `from..=to`
    /// (fractional dot heights) with the given thickness in dots.
    fn plot_span(&mut self, vx: isize, from: f32, to: f32, thickness: f32) {
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        let reach = thickness / 2.0 + 0.5;
        let first = (low - reach).floor().max(0.0) as isize;
        let last = ((high + reach).ceil() as isize).min(self.height as isize - 1);
        for row in first..=last {
            let centre = row as f32 + 0.5;
            let distance = if centre < low + 0.5 {
                low + 0.5 - centre
            } else if centre > high + 0.5 {
                centre - high - 0.5
            } else {
                0.0
            };
            let coverage = (reach - distance).clamp(0.0, 1.0);
            if coverage > 0.02 {
                self.plot(vx, row, coverage);
            }
        }
    }
}

/// Catmull-Rom interpolation of `values` at a fractional index.
pub fn cubic_sample(values: &[f32], position: f32) -> f32 {
    match values.len() {
        0 => return 0.0,
        1 => return values[0],
        _ => {}
    }
    let position = position.clamp(0.0, (values.len() - 1) as f32);
    let index = position.floor() as usize;
    let fraction = position - index as f32;
    let at = |offset: isize| {
        values[(index as isize + offset).clamp(0, values.len() as isize - 1) as usize]
    };
    let (p0, p1, p2, p3) = (at(-1), at(0), at(1), at(2));
    let value = 0.5
        * (2.0 * p1
            + (-p0 + p2) * fraction
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * fraction * fraction
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * fraction * fraction * fraction);
    value.clamp(0.0, 1.0)
}

/// Value of a bar covering `low..=high` (0..1 of the band axis): the
/// loudest band it spans, or an interpolation when it spans less than one.
pub fn sample_span(values: &[f32], low: f32, high: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let last = (values.len() - 1) as f32;
    let (low, high) = (low.clamp(0.0, 1.0) * last, high.clamp(0.0, 1.0) * last);
    if high - low >= 1.0 {
        let first = (low - 1e-3).ceil().max(0.0) as usize;
        let end = ((high + 1e-3).floor() as usize).min(values.len() - 1);
        values[first..=end.max(first)]
            .iter()
            .copied()
            .fold(0.0, f32::max)
    } else {
        cubic_sample(values, (low + high) / 2.0)
    }
}

/// Folds a 0..1 position about the centre when mirroring (bass in the middle).
fn fold(position: f32, mirror: bool) -> f32 {
    if mirror {
        (2.0 * position - 1.0).abs()
    } else {
        position
    }
}

/// Horizontal placement of bars. Auto width spreads the bars over the whole
/// canvas with a fractional pitch; a fixed width centres whole bars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarLayout {
    pub count: usize,
    pitch: f32,
    gap: usize,
    offset: f32,
}

impl BarLayout {
    /// `bar_width` 0 = auto: one bar per band, as wide as fits.
    pub fn new(canvas_width: usize, bands: usize, bar_width: usize, gap: usize) -> Self {
        let canvas_width = canvas_width.max(1);
        if bar_width == 0 {
            let count = bands.clamp(1, ((canvas_width + gap) / (1 + gap)).max(1));
            return Self {
                count,
                pitch: (canvas_width + gap) as f32 / count as f32,
                gap,
                offset: 0.0,
            };
        }
        let count = ((canvas_width + gap) / (bar_width + gap)).max(1);
        let pitch = (bar_width + gap) as f32;
        let total = count as f32 * pitch - gap as f32;
        Self {
            count,
            pitch,
            gap,
            offset: ((canvas_width as f32 - total) / 2.0).floor().max(0.0),
        }
    }

    /// First dot column and width in dots of bar `index`.
    pub fn span(&self, index: usize) -> (usize, usize) {
        let start = (self.offset + index as f32 * self.pitch).round();
        let end = (self.offset + (index + 1) as f32 * self.pitch).round() - self.gap as f32;
        (start as usize, ((end - start) as usize).max(1))
    }
}

fn body_intensity(height_fraction: f32) -> f32 {
    0.55 + 0.4 * height_fraction.clamp(0.0, 1.0)
}

/// Draw the spectrum described by `input` in the chosen style.
pub fn draw(raster: &mut Raster, settings: &SpectrumSettings, input: &DrawInput<'_>) {
    if raster.width < 2 || raster.height < 2 {
        return;
    }
    match settings.style {
        SpectrumStyle::Bars => draw_bars(raster, settings, input, false),
        SpectrumStyle::MirroredBars => draw_bars(raster, settings, input, true),
        SpectrumStyle::Line => draw_curve(raster, settings, input, false),
        SpectrumStyle::Area => draw_curve(raster, settings, input, true),
        SpectrumStyle::Waveform => draw_waveform(raster, settings, input),
        SpectrumStyle::Radial => draw_radial(raster, settings, input),
        SpectrumStyle::Spectrogram => draw_spectrogram(raster, settings, input),
        SpectrumStyle::PulseRings => draw_rings(raster, input),
    }
}

fn draw_bars(raster: &mut Raster, settings: &SpectrumSettings, input: &DrawInput<'_>, split: bool) {
    let mut canvas = Canvas::new(raster, settings.orientation);
    let layout = BarLayout::new(
        canvas.width,
        input.values.len(),
        settings.bar_width as usize,
        settings.bar_gap as usize,
    );
    let show_peaks = settings.peak_hold;
    for bar in 0..layout.count {
        let (low, high) = (
            bar as f32 / layout.count as f32,
            (bar + 1) as f32 / layout.count as f32,
        );
        let (low, high) = if settings.mirror {
            let centre = fold((low + high) / 2.0, true);
            let half = 1.0 / layout.count as f32;
            (centre - half, centre + half)
        } else {
            (low, high)
        };
        let value = sample_span(input.values, low, high);
        let peak = sample_span(input.peaks, low, high);
        let (x_start, bar_width) = layout.span(bar);
        for x in x_start..x_start + bar_width {
            draw_bar_column(&mut canvas, x as isize, value, peak, show_peaks, split);
        }
    }
}

fn draw_bar_column(
    canvas: &mut Canvas<'_>,
    x: isize,
    value: f32,
    peak: f32,
    show_peak: bool,
    split: bool,
) {
    let total_height = canvas.height as f32;
    let (scale, centre) = if split {
        (total_height / 2.0, canvas.height / 2)
    } else {
        (total_height, 0)
    };
    let extent = value.clamp(0.0, 1.0) * scale;
    let full = extent.floor() as isize;
    let fraction = extent - full as f32;
    for row in 0..full {
        // The outermost dots are solid so the silhouette stays crisp.
        let intensity = if row >= full - 2 {
            1.0
        } else {
            body_intensity(row as f32 / scale)
        };
        plot_pair(canvas, x, centre, row, intensity, split);
    }
    if fraction > 0.02 {
        plot_pair(canvas, x, centre, full, fraction, split);
    }
    let peak_extent = peak.clamp(0.0, 1.0) * scale;
    if show_peak && peak_extent >= extent + 2.0 {
        plot_pair(canvas, x, centre, peak_extent.floor() as isize, 1.0, split);
    }
}

/// Plots `row` above the centre and, when `split`, its mirror below it.
fn plot_pair(
    canvas: &mut Canvas<'_>,
    x: isize,
    centre: usize,
    row: isize,
    intensity: f32,
    split: bool,
) {
    canvas.plot(x, centre as isize + row, intensity);
    if split {
        canvas.plot(x, centre as isize - 1 - row, intensity);
    }
}

fn draw_curve(
    raster: &mut Raster,
    settings: &SpectrumSettings,
    input: &DrawInput<'_>,
    filled: bool,
) {
    let mut canvas = Canvas::new(raster, settings.orientation);
    let width = canvas.width;
    let top = (canvas.height - 1) as f32;
    let heights: Vec<f32> = (0..width)
        .map(|x| {
            let position = fold(x as f32 / (width - 1).max(1) as f32, settings.mirror);
            cubic_sample(
                input.values,
                position * (input.values.len().max(1) - 1) as f32,
            ) * top
        })
        .collect();
    for (x, &height) in heights.iter().enumerate() {
        let previous = if x > 0 { heights[x - 1] } else { height };
        if filled {
            for row in 0..height.floor() as isize {
                canvas.plot(x as isize, row, 0.2 + 0.5 * (row as f32 / top.max(1.0)));
            }
        }
        canvas.plot_span(x as isize, previous, height, 1.4);
        if settings.peak_hold {
            let position = fold(x as f32 / (width - 1).max(1) as f32, settings.mirror);
            let peak = cubic_sample(
                input.peaks,
                position * (input.peaks.len().max(1) - 1) as f32,
            ) * top;
            if peak >= height + 2.0 {
                canvas.plot(x as isize, peak.round() as isize, 0.9);
            }
        }
    }
}

fn draw_waveform(raster: &mut Raster, settings: &SpectrumSettings, input: &DrawInput<'_>) {
    let mut canvas = Canvas::new(raster, settings.orientation);
    let samples = input.waveform;
    if samples.len() < 8 {
        return;
    }
    let width = canvas.width;
    let centre = (canvas.height - 1) as f32 / 2.0;
    // Trigger on a rising zero crossing so a steady tone holds still.
    let search = samples.len() / 3;
    let start = (1..search)
        .find(|&index| samples[index - 1] < 0.0 && samples[index] >= 0.0)
        .unwrap_or(0);
    let span = (samples.len() - start).min(640) as f32;
    for x in (0..width).step_by(4) {
        // Faint centre line as a dotted reference.
        canvas.plot(x as isize, centre.round() as isize, 0.3);
    }
    let sample_at = |x: usize| {
        let position = start as f32 + x as f32 / (width - 1).max(1) as f32 * (span - 1.0);
        let index = position.floor() as usize;
        let fraction = position - index as f32;
        let next = (index + 1).min(samples.len() - 1);
        samples[index] * (1.0 - fraction) + samples[next] * fraction
    };
    let mut previous = centre + sample_at(0).clamp(-1.0, 1.0) * centre * 0.95;
    for x in 0..width {
        let height = centre + sample_at(x).clamp(-1.0, 1.0) * centre * 0.95;
        canvas.plot_span(x as isize, previous, height, 1.3);
        previous = height;
    }
}

fn draw_radial(raster: &mut Raster, settings: &SpectrumSettings, input: &DrawInput<'_>) {
    let (width, height) = (raster.width as f32, raster.height as f32);
    let (centre_x, centre_y) = (width / 2.0, height / 2.0);
    let outer = (width.min(height) / 2.0 - 1.0).max(2.0);
    // The inner ring gently swells with the bass.
    let inner = outer * (0.28 + 0.05 * input.levels.bass);
    let count = input.values.len().max(1);
    let spokes = if settings.mirror { count * 2 } else { count };
    // A ring at the base of the spokes anchors the shape even in silence.
    raster.curve(((TAU * inner) as usize / 2).max(24), 0.5, 0.45, |t| {
        let angle = t * TAU;
        (
            (centre_x + inner * angle.cos()) / width,
            (centre_y + inner * angle.sin()) / height,
        )
    });
    let spoke_radius = ((TAU * inner / spokes as f32) * 0.28).clamp(0.4, 1.6);
    for spoke in 0..spokes {
        let band = if settings.mirror && spoke >= count {
            spokes - 1 - spoke
        } else {
            spoke
        };
        let angle = -PI / 2.0 + TAU * (spoke as f32 + 0.5) / spokes as f32;
        let (sin, cos) = angle.sin_cos();
        let point = |radius: f32| {
            (
                (centre_x + radius * cos) / width,
                (centre_y + radius * sin) / height,
            )
        };
        let length = input
            .values
            .get(band)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let tip = inner + 1.0 + length * (outer - inner - 1.0);
        raster.line(point(inner + 1.0), point(tip), spoke_radius, 0.85);
        raster.line(point((tip - 2.0).max(inner)), point(tip), spoke_radius, 1.0);
        if settings.peak_hold {
            let peak = input
                .peaks
                .get(band)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let peak_radius = inner + 1.0 + peak * (outer - inner - 1.0);
            if peak_radius > tip + 2.5 {
                raster.line(
                    point(peak_radius),
                    point(peak_radius + 1.0),
                    spoke_radius,
                    0.9,
                );
            }
        }
    }
}

fn draw_circle(raster: &mut Raster, radius: f32, thickness: f32, intensity: f32) {
    let (width, height) = (raster.width as f32, raster.height as f32);
    let (centre_x, centre_y) = (width / 2.0, height / 2.0);
    let steps = ((TAU * radius) as usize / 2).clamp(16, 400);
    raster.curve(steps, thickness, intensity, |t| {
        let angle = t * TAU;
        (
            (centre_x + radius * angle.cos()) / width,
            (centre_y + radius * angle.sin()) / height,
        )
    });
}

fn draw_rings(raster: &mut Raster, input: &DrawInput<'_>) {
    let outer = (raster.width.min(raster.height) as f32 / 2.0 - 1.0).max(4.0);
    let rings = [
        (0.16, 0.22, input.levels.bass),
        (0.42, 0.14, input.levels.mid),
        (0.66, 0.12, input.levels.treble),
    ];
    for (base, swing, level) in rings {
        let level = level.clamp(0.0, 1.0);
        let radius = outer * (base + swing * level);
        draw_circle(raster, radius, 0.7 + 1.3 * level, 0.6 + 0.4 * level);
    }
    for ripple in input.ripples {
        let age = ripple.age.clamp(0.0, 1.0);
        let radius = outer * (0.25 + 0.75 * age);
        let intensity = ripple.strength * (1.0 - age).powf(1.5);
        if intensity > 0.05 {
            draw_circle(raster, radius, 0.5, intensity.min(1.0));
        }
    }
}

fn draw_spectrogram(raster: &mut Raster, settings: &SpectrumSettings, input: &DrawInput<'_>) {
    let mut canvas = Canvas::new(raster, settings.orientation);
    let (width, height) = (canvas.width, canvas.height);
    // Per-row interpolation table: frequency runs up the vertical axis.
    let rows: Vec<(usize, usize, f32)> = (0..height)
        .map(|row| {
            let position = fold(row as f32 / (height - 1).max(1) as f32, settings.mirror);
            let last = input.values.len().saturating_sub(1);
            let scaled = position * last as f32;
            let low = (scaled.floor() as usize).min(last);
            (low, (low + 1).min(last), scaled - low as f32)
        })
        .collect();
    for x in 0..width {
        let Some(column) = input.history.column(width - 1 - x) else {
            continue;
        };
        for (row, &(low, high, fraction)) in rows.iter().enumerate() {
            let (Some(a), Some(b)) = (column.get(low), column.get(high)) else {
                continue;
            };
            let value = a * (1.0 - fraction) + b * fraction;
            let intensity = value.clamp(0.0, 1.0).powf(1.15);
            if intensity > 0.02 {
                canvas.plot(x as isize, row as isize, intensity);
            }
        }
    }
}

fn hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let sector = hue.floor();
    let fraction = hue - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    let (r, g, b) = match sector as u32 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    ]
}

fn gradient(stops: &[(f32, [u8; 3])], position: f32) -> [u8; 3] {
    let position = position.clamp(0.0, 1.0);
    for pair in stops.windows(2) {
        let ((start, from), (end, to)) = (pair[0], pair[1]);
        if position <= end {
            let fraction = ((position - start) / (end - start).max(1e-6)).clamp(0.0, 1.0);
            let mix = |a: u8, b: u8| {
                (f32::from(a) + (f32::from(b) - f32::from(a)) * fraction).round() as u8
            };
            return [
                mix(from[0], to[0]),
                mix(from[1], to[1]),
                mix(from[2], to[2]),
            ];
        }
    }
    stops.last().map_or([255, 255, 255], |stop| stop.1)
}

/// Colour for a cell given its position along the frequency axis (`along`)
/// and its level or height (`level`), both 0..1.
pub fn color_for(mode: ColorMode, along: f32, level: f32) -> [u8; 3] {
    match mode {
        ColorMode::Mono => [255, 255, 255],
        ColorMode::Rainbow => hsv(
            0.83 * along.clamp(0.0, 1.0),
            0.85,
            0.75 + 0.25 * level.clamp(0.0, 1.0),
        ),
        ColorMode::Heat => gradient(
            &[
                (0.0, [110, 10, 40]),
                (0.35, [230, 50, 20]),
                (0.65, [255, 190, 30]),
                (1.0, [255, 250, 225]),
            ],
            level,
        ),
        ColorMode::Ice => gradient(
            &[
                (0.0, [30, 70, 200]),
                (0.5, [70, 190, 255]),
                (1.0, [235, 250, 255]),
            ],
            level,
        ),
    }
}

/// Fill one colour per terminal cell (`width * height`, row-major) for the
/// non-mono colour modes, from the geometry of the style just drawn.
pub fn fill_cell_colors(
    cells: &mut [[u8; 3]],
    width: u16,
    height: u16,
    raster: &Raster,
    settings: &SpectrumSettings,
) {
    let (cell_columns, cell_rows) = (usize::from(width), usize::from(height));
    if raster.width < cell_columns * 2
        || raster.height < cell_rows * 4
        || cells.len() < cell_columns * cell_rows
    {
        return;
    }
    let (raster_width, raster_height) = (raster.width, raster.height);
    let (virtual_width, virtual_height) =
        virtual_size(settings.orientation, raster_width, raster_height);
    let centre = (raster_width as f32 / 2.0, raster_height as f32 / 2.0);
    let outer = (raster_width.min(raster_height) as f32 / 2.0 - 1.0).max(2.0);
    for cell_y in 0..cell_rows {
        for cell_x in 0..cell_columns {
            let (x, y) = (cell_x * 2 + 1, cell_y * 4 + 2);
            let brightness = (0..4)
                .flat_map(|dy| (0..2).map(move |dx| (dx, dy)))
                .map(|(dx, dy)| raster.dots[(cell_y * 4 + dy) * raster_width + cell_x * 2 + dx])
                .fold(0.0f32, f32::max);
            let (along, level) = match settings.style {
                SpectrumStyle::Radial | SpectrumStyle::PulseRings => {
                    let (dx, dy) = (x as f32 - centre.0, y as f32 - centre.1);
                    let radius = ((dx * dx + dy * dy).sqrt() / outer).clamp(0.0, 1.0);
                    if settings.style == SpectrumStyle::PulseRings {
                        (radius, radius)
                    } else {
                        let turn = (dx.atan2(-dy) / TAU).rem_euclid(1.0);
                        (fold(turn, settings.mirror), radius)
                    }
                }
                style => {
                    let (vx, vy) =
                        from_raster(settings.orientation, raster_width, raster_height, x, y);
                    let across = vx as f32 / virtual_width.max(1) as f32;
                    let up = vy as f32 / virtual_height.max(1) as f32;
                    match style {
                        SpectrumStyle::Spectrogram => (fold(up, settings.mirror), brightness),
                        SpectrumStyle::MirroredBars => {
                            (fold(across, settings.mirror), (2.0 * up - 1.0).abs())
                        }
                        _ => (fold(across, settings.mirror), up),
                    }
                }
            };
            cells[cell_y * cell_columns + cell_x] = color_for(settings.color_mode, along, level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::{threshold, DitherMode};

    fn synthetic_values(count: usize) -> Vec<f32> {
        (0..count)
            .map(|index| {
                let u = index as f32 / (count - 1) as f32;
                (0.85 * (-(u * 3.0)).exp() + 0.35 * (u * 14.0).sin().abs() * (1.0 - u * 0.6))
                    .clamp(0.02, 1.0)
            })
            .collect()
    }

    fn draw_style(style: SpectrumStyle, width: usize, height: usize) -> Raster {
        let settings = SpectrumSettings {
            style,
            ..SpectrumSettings::default()
        };
        let values = synthetic_values(48);
        let peaks: Vec<f32> = values.iter().map(|v| (v + 0.15).min(1.0)).collect();
        let waveform: Vec<f32> = (0..1024).map(|n| 0.7 * (n as f32 * 0.09).sin()).collect();
        let mut history = SpectrogramHistory::new(400);
        for step in 0..400 {
            let shifted: Vec<f32> = (0..48)
                .map(|band| {
                    ((band as f32 * 0.25 - step as f32 * 0.05).sin() * 0.5 + 0.5) * values[band]
                })
                .collect();
            history.push(&shifted);
        }
        let ripples = [
            Ripple {
                age: 0.3,
                strength: 0.9,
            },
            Ripple {
                age: 0.7,
                strength: 0.6,
            },
        ];
        let input = DrawInput {
            values: &values,
            peaks: &peaks,
            waveform: &waveform,
            history: &history,
            levels: EnergyLevels {
                bass: 0.8,
                mid: 0.5,
                treble: 0.3,
            },
            ripples: &ripples,
        };
        let mut raster = Raster::default();
        raster.resize(width, height);
        draw(&mut raster, &settings, &input);
        raster
    }

    fn lit(raster: &Raster) -> usize {
        raster.dots.iter().filter(|dot| **dot > 0.5).count()
    }

    #[test]
    fn every_style_draws_something_and_no_two_styles_match() {
        let rasters: Vec<(SpectrumStyle, Raster)> = SpectrumStyle::ALL
            .iter()
            .map(|style| (*style, draw_style(*style, 120, 48)))
            .collect();
        for (style, raster) in &rasters {
            assert!(lit(raster) > 40, "{style:?} lit only {}", lit(raster));
            assert!(
                raster.dots.iter().all(|dot| (0.0..=1.0).contains(dot)),
                "{style:?} range"
            );
            assert!(
                lit(raster) < raster.dots.len() * 9 / 10,
                "{style:?} is not a solid fill"
            );
        }
        for (i, (first, a)) in rasters.iter().enumerate() {
            for (second, b) in &rasters[i + 1..] {
                let differing = a
                    .dots
                    .iter()
                    .zip(&b.dots)
                    .filter(|(x, y)| (**x - **y).abs() > 0.3)
                    .count();
                assert!(
                    differing > 100,
                    "{first:?} vs {second:?} differ in only {differing} dots"
                );
            }
        }
    }

    #[test]
    fn drawing_is_deterministic() {
        for style in SpectrumStyle::ALL {
            assert_eq!(
                draw_style(*style, 90, 40).dots,
                draw_style(*style, 90, 40).dots,
                "{style:?}"
            );
        }
    }

    #[test]
    fn tiny_rasters_do_not_panic() {
        for style in SpectrumStyle::ALL {
            for (width, height) in [(0, 0), (1, 1), (2, 4), (3, 5), (10, 4)] {
                let _ = draw_style(*style, width, height);
            }
        }
    }

    #[test]
    fn bars_grow_with_their_value_and_start_at_the_baseline() {
        let settings = SpectrumSettings {
            style: SpectrumStyle::Bars,
            bar_width: 3,
            bar_gap: 1,
            peak_hold: false,
            ..SpectrumSettings::default()
        };
        let values = [0.25, 0.5, 1.0, 0.0];
        let history = SpectrogramHistory::new(1);
        let input = DrawInput {
            values: &values,
            peaks: &values,
            waveform: &[],
            history: &history,
            levels: EnergyLevels::default(),
            ripples: &[],
        };
        let mut raster = Raster::default();
        raster.resize(40, 40);
        draw(&mut raster, &settings, &input);
        let layout = BarLayout::new(40, 4, 3, 1);
        // 40 dots / 4 per bar = 10 bars sampled from 4 bands; measure bar columns directly.
        let column_height = |bar: usize| {
            let x = layout.span(bar).0 + 1;
            (0..40).filter(|y| raster.dots[y * 40 + x] > 0.3).count()
        };
        assert_eq!(layout.count, 10);
        let heights: Vec<usize> = (0..10).map(column_height).collect();
        assert!(
            heights.windows(2).take(6).all(|pair| pair[1] >= pair[0]),
            "{heights:?}"
        );
        assert!(heights[6] >= 38, "bar 6 near full height: {heights:?}");
        // Grows upward: the bottom row of the tallest bar is lit, the top row is at the top.
        let x = layout.span(6).0 + 1;
        assert!(raster.dots[39 * 40 + x] > 0.3);
        // Gap columns stay dark.
        assert!((0..40).all(|y| raster.dots[y * 40 + layout.span(0).0 + 3] == 0.0));
    }

    #[test]
    fn bar_layout_covers_auto_and_fixed_widths() {
        // Auto: bars tile the whole canvas, separated by the gap, never overlapping.
        for (canvas, bands, gap) in [(120usize, 32usize, 1usize), (100, 48, 2), (37, 20, 0)] {
            let auto = BarLayout::new(canvas, bands, 0, gap);
            assert_eq!(auto.count, bands.min((canvas + gap) / (1 + gap)));
            let (first, _) = auto.span(0);
            let (last_start, last_width) = auto.span(auto.count - 1);
            assert_eq!(first, 0);
            assert!(
                last_start + last_width <= canvas && last_start + last_width + 1 >= canvas,
                "{canvas}: fills width"
            );
            for index in 1..auto.count {
                let (previous_start, previous_width) = auto.span(index - 1);
                assert!(
                    auto.span(index).0 >= previous_start + previous_width + gap,
                    "no overlap"
                );
            }
        }
        // More bands than fit: one dot bars, count limited by the canvas.
        let crowded = BarLayout::new(20, 128, 0, 1);
        assert_eq!(crowded.span(0).1, 1);
        assert!(crowded.count <= 10);
        let fixed = BarLayout::new(100, 48, 4, 2);
        assert_eq!(fixed.count, 17);
        assert!((0..17).all(|index| fixed.span(index).1 == 4));
        assert_eq!(fixed.span(1).0 - fixed.span(0).0, 6);
        assert_eq!(BarLayout::new(1, 8, 0, 0).count, 1);
    }

    #[test]
    fn orientation_moves_the_baseline_to_each_edge() {
        let values = [1.0; 16];
        let history = SpectrogramHistory::new(1);
        let input = DrawInput {
            values: &values,
            peaks: &values,
            waveform: &[],
            history: &history,
            levels: EnergyLevels::default(),
            ripples: &[],
        };
        let mean_position = |orientation: Orientation| {
            let settings = SpectrumSettings {
                style: SpectrumStyle::Bars,
                orientation,
                peak_hold: false,
                ..SpectrumSettings::default()
            };
            let values_half = [0.5; 16];
            let input = DrawInput {
                values: &values_half,
                peaks: &values_half,
                ..input_copy(&input)
            };
            let mut raster = Raster::default();
            raster.resize(60, 40);
            draw(&mut raster, &settings, &input);
            let (mut sum_x, mut sum_y, mut count) = (0.0, 0.0, 0.0);
            for y in 0..40 {
                for x in 0..60 {
                    if raster.dots[y * 60 + x] > 0.3 {
                        sum_x += x as f32;
                        sum_y += y as f32;
                        count += 1.0;
                    }
                }
            }
            (sum_x / count, sum_y / count)
        };
        let (_, bottom_up_y) = mean_position(Orientation::BottomUp);
        let (_, top_down_y) = mean_position(Orientation::TopDown);
        let (left_x, _) = mean_position(Orientation::LeftToRight);
        let (right_x, _) = mean_position(Orientation::RightToLeft);
        assert!(
            bottom_up_y > 25.0 && top_down_y < 15.0,
            "{bottom_up_y} {top_down_y}"
        );
        assert!(left_x < 20.0 && right_x > 40.0, "{left_x} {right_x}");
    }

    fn input_copy<'a>(input: &DrawInput<'a>) -> DrawInput<'a> {
        DrawInput {
            values: input.values,
            peaks: input.peaks,
            waveform: input.waveform,
            history: input.history,
            levels: input.levels,
            ripples: input.ripples,
        }
    }

    #[test]
    fn coordinate_mappings_are_inverse_of_each_other() {
        for orientation in Orientation::ALL {
            let (raster_width, raster_height) = (37, 23);
            let (virtual_width, virtual_height) =
                virtual_size(*orientation, raster_width, raster_height);
            for vx in [0, 5, virtual_width - 1] {
                for vy in [0, 7, virtual_height - 1] {
                    let (x, y) = to_raster(*orientation, raster_width, raster_height, vx, vy);
                    assert!(x < raster_width && y < raster_height);
                    assert_eq!(
                        from_raster(*orientation, raster_width, raster_height, x, y),
                        (vx, vy)
                    );
                }
            }
        }
    }

    #[test]
    fn mirror_makes_bars_symmetric_with_bass_in_the_middle() {
        let settings = SpectrumSettings {
            style: SpectrumStyle::Bars,
            mirror: true,
            peak_hold: false,
            bar_width: 2,
            bar_gap: 0,
            ..SpectrumSettings::default()
        };
        let values = synthetic_values(32);
        let history = SpectrogramHistory::new(1);
        let input = DrawInput {
            values: &values,
            peaks: &values,
            waveform: &[],
            history: &history,
            levels: EnergyLevels::default(),
            ripples: &[],
        };
        let mut raster = Raster::default();
        raster.resize(64, 40);
        draw(&mut raster, &settings, &input);
        for y in 0..40 {
            for x in 0..32 {
                let left = raster.dots[y * 64 + x];
                let right = raster.dots[y * 64 + 63 - x];
                assert!((left - right).abs() < 1e-6, "asymmetric at ({x},{y})");
            }
        }
    }

    #[test]
    fn mirrored_bars_are_symmetric_top_to_bottom() {
        let raster = draw_style(SpectrumStyle::MirroredBars, 80, 40);
        for y in 0..20 {
            for x in 0..80 {
                assert!((raster.dots[y * 80 + x] - raster.dots[(39 - y) * 80 + x]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn cubic_and_span_sampling() {
        let values = [0.0, 1.0, 0.0, 1.0];
        assert_eq!(cubic_sample(&values, 1.0), 1.0);
        assert!((cubic_sample(&values, 0.5) - 0.5).abs() < 0.3);
        assert_eq!(cubic_sample(&[], 0.3), 0.0);
        assert_eq!(cubic_sample(&[0.7], 5.0), 0.7);
        // Wide span picks the loudest band; narrow span interpolates.
        assert_eq!(sample_span(&[0.1, 0.9, 0.2, 0.3], 0.0, 1.0), 0.9);
        let narrow = sample_span(&[0.0, 1.0], 0.4, 0.5);
        assert!(narrow > 0.3 && narrow < 0.6);
    }

    #[test]
    fn spectrogram_history_is_a_bounded_ring() {
        let mut history = SpectrogramHistory::new(3);
        assert!(history.is_empty());
        for step in 0..5 {
            history.push(&[step as f32]);
        }
        assert_eq!(history.len(), 3);
        assert_eq!(history.column(0), Some(&[4.0][..]));
        assert_eq!(history.column(2), Some(&[2.0][..]));
        assert_eq!(history.column(3), None);
    }

    #[test]
    fn spectrogram_scrolls_newest_column_to_the_right_edge() {
        let settings = SpectrumSettings {
            style: SpectrumStyle::Spectrogram,
            ..SpectrumSettings::default()
        };
        let mut history = SpectrogramHistory::new(100);
        for _ in 0..10 {
            history.push(&[0.0; 8]);
        }
        history.push(&[1.0; 8]);
        let input = DrawInput {
            values: &[0.0; 8],
            peaks: &[0.0; 8],
            waveform: &[],
            history: &history,
            levels: EnergyLevels::default(),
            ripples: &[],
        };
        let mut raster = Raster::default();
        raster.resize(30, 20);
        draw(&mut raster, &settings, &input);
        assert!(
            (0..20).all(|y| raster.dots[y * 30 + 29] > 0.9),
            "newest column full height on the right"
        );
        assert!((0..20).all(|y| raster.dots[y * 30 + 28] == 0.0));
    }

    #[test]
    fn colour_modes_produce_distinct_gradients() {
        assert_ne!(
            color_for(ColorMode::Rainbow, 0.0, 1.0),
            color_for(ColorMode::Rainbow, 0.7, 1.0)
        );
        let cold = color_for(ColorMode::Heat, 0.0, 0.0);
        let hot = color_for(ColorMode::Heat, 0.0, 1.0);
        assert!(hot[0] > 240 && hot[2] > cold[2] && hot[1] > cold[1]);
        let deep = color_for(ColorMode::Ice, 0.0, 0.0);
        let pale = color_for(ColorMode::Ice, 0.0, 1.0);
        assert!(deep[2] > deep[0] && pale[0] > deep[0]);
        assert_eq!(color_for(ColorMode::Mono, 0.3, 0.3), [255, 255, 255]);
    }

    #[test]
    fn cell_colours_follow_frequency_and_height() {
        let mut settings = SpectrumSettings {
            style: SpectrumStyle::Bars,
            color_mode: ColorMode::Rainbow,
            ..SpectrumSettings::default()
        };
        let raster = draw_style(SpectrumStyle::Bars, 120, 48);
        let mut cells = vec![[0u8; 3]; 60 * 12];
        fill_cell_colors(&mut cells, 60, 12, &raster, &settings);
        assert_ne!(cells[0], cells[59], "hue moves along the frequency axis");
        let dominant = |color: [u8; 3]| {
            color
                .iter()
                .enumerate()
                .max_by_key(|(_, channel)| **channel)
                .map(|(index, _)| index)
        };
        assert_eq!(
            dominant(cells[0]),
            dominant(cells[11 * 60]),
            "same column, same hue family"
        );
        settings.color_mode = ColorMode::Heat;
        let mut heat = vec![[0u8; 3]; 60 * 12];
        fill_cell_colors(&mut heat, 60, 12, &raster, &settings);
        // Top row is hotter than the bottom row in the same column.
        assert!(heat[0][1] > heat[11 * 60][1]);
        // Undersized buffers are ignored rather than panicking.
        fill_cell_colors(&mut heat[..5], 60, 12, &raster, &settings);
    }

    /// Prints every style so the aesthetics can be judged by eye:
    /// `cargo test -p ilium-ambient show_all_styles -- --ignored --nocapture`.
    #[test]
    #[ignore = "prints Braille for visual inspection"]
    fn show_all_styles() {
        for style in SpectrumStyle::ALL {
            let raster = draw_style(*style, 120, 48);
            println!("--- {style:?}");
            for row in braille(&raster, 12) {
                println!("{row}");
            }
        }
    }

    pub fn braille(raster: &Raster, height_cells: usize) -> Vec<String> {
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        (0..height_cells)
            .map(|cy| {
                (0..raster.width / 2)
                    .map(|cx| {
                        let mut cell = 0u8;
                        for (dy, row) in BITS.iter().enumerate() {
                            for (dx, bit) in row.iter().enumerate() {
                                let (x, y) = (cx * 2 + dx, cy * 4 + dy);
                                if raster.dots[y * raster.width + x] * 0.7
                                    > threshold(x, y, DitherMode::Ordered)
                                {
                                    cell |= bit;
                                }
                            }
                        }
                        char::from_u32(0x2800 + u32::from(cell)).unwrap_or(' ')
                    })
                    .collect()
            })
            .collect()
    }
}
