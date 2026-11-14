//! Vector drawing on the Braille dot raster: antialiased thin lines,
//! outlines, fills and a tiny dot font, all writing both a tone for the
//! host's dither and the colour of the strongest contribution to each cell.

use crate::raster::Raster;

/// How a stroke looks: half width in dots, tone for the dither, colour.
#[derive(Debug, Clone, Copy)]
pub struct Stroke {
    pub width: f32,
    pub tone: f32,
    pub color: [u8; 3],
}

/// Draws into the frame's raster and per-cell colours.
pub struct Canvas<'a> {
    raster: &'a mut Raster,
    cell_colors: &'a mut [[u8; 3]],
    /// Strongest tone seen per cell, so the brightest shape owns the colour.
    strongest: &'a mut [f32],
    columns: usize,
}

impl<'a> Canvas<'a> {
    pub fn new(
        raster: &'a mut Raster,
        cell_colors: &'a mut [[u8; 3]],
        strongest: &'a mut [f32],
        columns: usize,
    ) -> Self {
        Self {
            raster,
            cell_colors,
            strongest,
            columns,
        }
    }

    pub fn width(&self) -> usize {
        self.raster.width
    }

    pub fn height(&self) -> usize {
        self.raster.height
    }

    /// Light one dot to at least `tone`; the cell takes `color` when this is
    /// its strongest contribution so far.
    #[inline]
    pub fn plot(&mut self, x: i32, y: i32, tone: f32, color: [u8; 3]) {
        if tone <= 0.0 || x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.raster.width || y >= self.raster.height {
            return;
        }
        let index = y * self.raster.width + x;
        if tone > self.raster.dots[index] {
            self.raster.dots[index] = tone;
        }
        let cell = (y / 4) * self.columns + x / 2;
        if let (Some(best), Some(slot)) =
            (self.strongest.get_mut(cell), self.cell_colors.get_mut(cell))
        {
            if tone >= *best {
                *best = tone;
                *slot = color;
            }
        }
    }

    /// A soft line `half_width` dots to each side of the segment.
    pub fn line(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        half_width: f32,
        tone: f32,
        color: [u8; 3],
    ) {
        let outer = half_width + 0.6;
        let squared_outer = outer * outer;
        let left = (from.0.min(to.0) - outer).floor().max(0.0) as i32;
        let top = (from.1.min(to.1) - outer).floor().max(0.0) as i32;
        let right = ((from.0.max(to.0) + outer).ceil() as i32).min(self.raster.width as i32 - 1);
        let bottom = ((from.1.max(to.1) + outer).ceil() as i32).min(self.raster.height as i32 - 1);
        let delta = (to.0 - from.0, to.1 - from.1);
        let squared_length = delta.0 * delta.0 + delta.1 * delta.1;
        for y in top..=bottom {
            for x in left..=right {
                let px = x as f32 + 0.5 - from.0;
                let py = y as f32 + 0.5 - from.1;
                let fraction = if squared_length > 1e-4 {
                    ((px * delta.0 + py * delta.1) / squared_length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let ox = px - fraction * delta.0;
                let oy = py - fraction * delta.1;
                let squared = ox * ox + oy * oy;
                if squared >= squared_outer {
                    continue;
                }
                let coverage = (outer - squared.sqrt()).clamp(0.0, 1.0);
                self.plot(x, y, coverage * tone, color);
            }
        }
    }

    pub fn polyline(
        &mut self,
        points: &[(f32, f32)],
        closed: bool,
        half_width: f32,
        tone: f32,
        color: [u8; 3],
    ) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], half_width, tone, color);
        }
        if closed && points.len() > 2 {
            self.line(points[points.len() - 1], points[0], half_width, tone, color);
        }
    }

    /// Fill a polygon (even-odd) with a flat tone.
    pub fn fill_polygon(&mut self, points: &[(f32, f32)], tone: f32, color: [u8; 3]) {
        if points.len() < 3 || tone <= 0.0 {
            return;
        }
        let min_x = points.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let max_x = points.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let min_y = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let max_y = points.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let top = min_y.floor().max(0.0) as i32;
        let bottom = (max_y.ceil() as i32).min(self.raster.height as i32 - 1);
        let left = min_x.floor().max(0.0) as i32;
        let right = (max_x.ceil() as i32).min(self.raster.width as i32 - 1);
        for y in top..=bottom {
            let scan = y as f32 + 0.5;
            // Crossings of this scanline with the polygon edges.
            let mut crossings: [f32; 16] = [0.0; 16];
            let mut count = 0;
            for index in 0..points.len() {
                let a = points[index];
                let b = points[(index + 1) % points.len()];
                if (a.1 <= scan) != (b.1 <= scan) && count < crossings.len() {
                    crossings[count] = a.0 + (scan - a.1) / (b.1 - a.1) * (b.0 - a.0);
                    count += 1;
                }
            }
            crossings[..count].sort_by(|a, b| a.total_cmp(b));
            for pair in crossings[..count].chunks_exact(2) {
                let from = (pair[0] - 0.5).ceil().max(left as f32) as i32;
                let to = ((pair[1] - 0.5).floor() as i32).min(right);
                for x in from..=to {
                    self.plot(x, y, tone, color);
                }
            }
        }
    }

    pub fn disk(&mut self, center: (f32, f32), radius: f32, tone: f32, color: [u8; 3]) {
        let outer = radius + 0.5;
        let left = (center.0 - outer).floor().max(0.0) as i32;
        let top = (center.1 - outer).floor().max(0.0) as i32;
        let right = ((center.0 + outer).ceil() as i32).min(self.raster.width as i32 - 1);
        let bottom = ((center.1 + outer).ceil() as i32).min(self.raster.height as i32 - 1);
        for y in top..=bottom {
            for x in left..=right {
                let dx = x as f32 + 0.5 - center.0;
                let dy = y as f32 + 0.5 - center.1;
                let distance = dx.hypot(dy);
                if distance < outer {
                    self.plot(x, y, (outer - distance).clamp(0.0, 1.0) * tone, color);
                }
            }
        }
    }

    pub fn circle(&mut self, center: (f32, f32), radius: f32, stroke: Stroke) {
        self.arc(center, radius, 0.0, std::f32::consts::TAU, stroke);
    }

    /// An arc from `start` to `start + sweep` radians.
    pub fn arc(&mut self, center: (f32, f32), radius: f32, start: f32, sweep: f32, stroke: Stroke) {
        let segments = ((radius * sweep.abs() / 2.2).ceil() as usize).clamp(6, 96);
        let mut previous = (
            center.0 + start.cos() * radius,
            center.1 + start.sin() * radius,
        );
        for step in 1..=segments {
            let angle = start + sweep * step as f32 / segments as f32;
            let next = (
                center.0 + angle.cos() * radius,
                center.1 + angle.sin() * radius,
            );
            self.line(previous, next, stroke.width, stroke.tone, stroke.color);
            previous = next;
        }
    }

    /// A circle drawn as `count` dashes with gaps, rotated by `phase`.
    pub fn dashed_circle(
        &mut self,
        center: (f32, f32),
        radius: f32,
        (count, phase): (usize, f32),
        stroke: Stroke,
    ) {
        let step = std::f32::consts::TAU / count as f32;
        for index in 0..count {
            self.arc(
                center,
                radius,
                phase + index as f32 * step,
                step * 0.5,
                stroke,
            );
        }
    }

    /// Zero a rectangle of dots (and darken its cells) so text stays legible.
    pub fn clear_rect(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        let (left, top) = (
            left.floor().max(0.0) as usize,
            top.floor().max(0.0) as usize,
        );
        let right = (right.ceil().max(0.0) as usize).min(self.raster.width);
        let bottom = (bottom.ceil().max(0.0) as usize).min(self.raster.height);
        for y in top..bottom {
            for x in left..right {
                self.raster.dots[y * self.raster.width + x] = 0.0;
            }
        }
        for cell_y in top / 4..bottom.div_ceil(4) {
            for cell_x in left / 2..right.div_ceil(2) {
                if let Some(best) = self.strongest.get_mut(cell_y * self.columns + cell_x) {
                    *best = 0.0;
                }
            }
        }
    }

    /// Draw `text` in the 3x5 font with its top-left at `(x, y)` and every
    /// font pixel `scale` dots square. Returns the width drawn in dots.
    pub fn text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        scale: i32,
        tone: f32,
        color: [u8; 3],
    ) -> f32 {
        let mut cursor = x as i32;
        for character in text.chars() {
            let rows = glyph(character);
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..3 {
                    if bits & (0b100 >> column) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.plot(
                                    cursor + column * scale + dx,
                                    y as i32 + row as i32 * scale + dy,
                                    tone,
                                    color,
                                );
                            }
                        }
                    }
                }
            }
            cursor += 4 * scale;
        }
        (cursor - x as i32) as f32
    }
}

/// Width in dots of `text` at `scale` (no trailing gap).
pub fn text_width(text: &str, scale: i32) -> f32 {
    ((text.chars().count() as i32 * 4 - 1).max(0) * scale) as f32
}

/// 3x5 glyph rows, top to bottom, bit 2 on the left.
fn glyph(character: char) -> [u8; 5] {
    match character.to_ascii_uppercase() {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '$' => [0b011, 0b110, 0b010, 0b011, 0b110],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        _ => [0; 5],
    }
}
