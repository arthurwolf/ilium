//! Text line charts in the style of [asciichart](https://www.npmjs.com/package/asciichart).
//!
//! This is a port of the plotting algorithm shared by asciichart (JavaScript)
//! and its Rust port rasciigraph: a numeric y-axis drawn with `┤`/`┼`, series
//! drawn as connected box-drawing glyphs (`─ ╭ ╮ ╰ ╯ │`), several series
//! overlaid on one grid, and `NaN` values leaving gaps. Two things differ from
//! the originals, both because ilium draws into a ratatui buffer rather than
//! printing a string:
//!
//! * the result is a grid of [`ChartCell`]s tagged with what they are (label,
//!   axis, or series `n`), so the caller colours them itself; and
//! * resampling to a target width can take the per-column maximum or mean
//!   (as text-graph.js offers) instead of only linear interpolation, so a
//!   downsampled spike does not disappear.
//!
//! The glyph choice, label precision rules, and row arithmetic follow the
//! reference implementation exactly; the unit tests below reuse its published
//! expected outputs.

/// How a series longer than the requested width is reduced to that width. A
/// series shorter than the width is always stretched by linear interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Downsample {
    /// The reference behaviour: sample the line at evenly spaced points.
    #[default]
    Interpolate,
    /// Keep each column's largest value, so spikes survive.
    Max,
    /// Average each column's values.
    Mean,
}

/// Formats a y-axis value. `None` uses the reference number formatting.
pub type LabelFormatter = fn(f64) -> String;

#[derive(Debug, Clone)]
pub struct ChartConfig {
    /// Number of data columns; `0` keeps the longest series as it is.
    pub width: usize,
    /// Rows the value range is divided into; `0` derives it from the range
    /// the way the reference does (only sensible for small ranges).
    pub height: usize,
    /// Columns reserved left of the data for the axis; `0` means the default 3.
    pub offset: usize,
    pub downsample: Downsample,
    /// Draw the axis from zero even when every value is larger.
    pub include_zero: bool,
    pub label_formatter: Option<LabelFormatter>,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            offset: 0,
            downsample: Downsample::Interpolate,
            include_zero: false,
            label_formatter: None,
        }
    }
}

/// What a cell of the rendered grid is, so the caller can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Blank,
    /// A digit or sign of a y-axis label.
    Label,
    /// A `┤` or `┼` axis glyph.
    Axis,
    /// A glyph of series `n` (index into the slice passed to [`plot`]).
    Line(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChartCell {
    pub ch: char,
    pub kind: CellKind,
}

/// A rendered chart: `rows` from the top (largest value) down.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chart {
    pub rows: Vec<Vec<ChartCell>>,
    /// Display columns left of the first data column (label and axis glyph).
    pub gutter: usize,
}

impl Chart {
    /// Display width of the widest row.
    pub fn width(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Plain text of each row, for tests and non-coloured output.
    pub fn to_lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.ch).collect())
            .collect()
    }
}

/// Plots `series` (each a list of values, `NaN` for a gap) onto one grid.
///
/// Returns an empty chart when there is nothing finite to draw.
pub fn plot(series: &[Vec<f64>], config: &ChartConfig) -> Chart {
    let mut series: Vec<Vec<f64>> = series.iter().filter(|s| !s.is_empty()).cloned().collect();
    if series.is_empty() {
        return Chart::default();
    }

    let mut len_max = series.iter().map(Vec::len).max().unwrap_or(0);
    if config.width > 0 {
        for values in &mut series {
            if values.len() < len_max {
                values.resize(len_max, f64::NAN);
            }
            *values = resample(values, config.width, config.downsample);
        }
        len_max = config.width;
    }

    let (mut min, max) = series
        .iter()
        .map(|values| min_max(values))
        .fold((f64::MAX, f64::MIN), |(lo, hi), (next_lo, next_hi)| {
            (lo.min(next_lo), hi.max(next_hi))
        });
    if min > max {
        // Only NaN values: nothing to scale against.
        return Chart::default();
    }
    let mut max = max;
    if config.include_zero {
        min = min.min(0.0);
        max = max.max(0.0);
    }

    let interval = (max - min).abs();
    let height = if config.height == 0 {
        if interval == 0.0 {
            3
        } else if interval <= 1.0 {
            (interval * 10_f64.powi((-interval.log10()).ceil() as i32)) as usize
        } else {
            interval as usize
        }
    } else {
        config.height
    };
    let offset = if config.offset == 0 { 3 } else { config.offset };

    let ratio = if interval != 0.0 {
        height as f64 / interval
    } else {
        1.0
    };
    let min2 = (min * ratio).round();
    let max2 = (max * ratio).round();
    let (int_min2, int_max2) = (min2 as i32, max2 as i32);
    let rows = (int_max2 - int_min2).abs();
    let cell_width = len_max + offset;

    // Each grid entry is one *cell*; a label occupies a single entry that
    // expands to several display columns when flattened, exactly as in the
    // reference implementation.
    let mut grid: Vec<Vec<(String, CellKind)>> = (0..=rows)
        .map(|_| vec![(" ".to_string(), CellKind::Blank); cell_width])
        .collect();

    let precision = label_precision(min, max);
    let format_label = |value: f64| -> String {
        match config.label_formatter {
            Some(format) => format(value),
            None => format!("{value:.precision$}"),
        }
    };
    let label_width = format_label(max)
        .chars()
        .count()
        .max(format_label(min).chars().count());

    for y in int_min2..=int_max2 {
        let magnitude = if rows > 0 {
            max - f64::from(y - int_min2) * interval / f64::from(rows)
        } else if config.label_formatter.is_some() || config.include_zero {
            min
        } else {
            f64::from(y)
        };
        let label = format!(
            "{:>width$}",
            format_label(magnitude),
            width = label_width + 1
        );
        let row = (y - int_min2) as usize;
        let start = offset.saturating_sub(label.chars().count());
        grid[row][start] = (label, CellKind::Label);
        grid[row][offset - 1] = ("┤".to_string(), CellKind::Axis);
    }

    let y_index = |value: f64| -> i32 { (value * ratio).round() as i32 - int_min2 };
    let set = |grid: &mut Vec<Vec<(String, CellKind)>>, y: i32, x: usize, glyph: &str, n: usize| {
        grid[(rows - y) as usize][x + offset] = (glyph.to_string(), CellKind::Line(n));
    };

    for (n, values) in series.iter().enumerate() {
        if !values[0].is_nan() {
            let y0 = y_index(values[0]);
            grid[(rows - y0) as usize][offset - 1] = ("┼".to_string(), CellKind::Axis);
        }
        for x in 0..values.len().saturating_sub(1) {
            let (a, b) = (values[x], values[x + 1]);
            if a.is_nan() && b.is_nan() {
                continue;
            }
            if b.is_nan() {
                set(&mut grid, y_index(a), x, "─", n);
                continue;
            }
            if a.is_nan() {
                set(&mut grid, y_index(b), x, "─", n);
                continue;
            }
            let (y0, y1) = (y_index(a), y_index(b));
            if y0 == y1 {
                set(&mut grid, y0, x, "─", n);
                continue;
            }
            if y0 > y1 {
                set(&mut grid, y1, x, "╰", n);
                set(&mut grid, y0, x, "╮", n);
            } else {
                set(&mut grid, y1, x, "╭", n);
                set(&mut grid, y0, x, "╯", n);
            }
            for y in (y0.min(y1) + 1)..y0.max(y1) {
                set(&mut grid, y, x, "│", n);
            }
        }
    }

    let mut gutter = 0;
    let rows: Vec<Vec<ChartCell>> = grid
        .into_iter()
        .map(|row| {
            let mut flat = Vec::with_capacity(cell_width + label_width);
            for (index, (text, kind)) in row.into_iter().enumerate() {
                if index == offset {
                    gutter = gutter.max(flat.len());
                }
                for ch in text.chars() {
                    flat.push(ChartCell { ch, kind });
                }
            }
            flat
        })
        .collect();
    Chart { rows, gutter }
}

/// Decimal places for y-axis labels, per the reference: two by default, more
/// for values below 1 so small ranges stay distinguishable, none above 1000.
fn label_precision(min: f64, max: f64) -> usize {
    let mut precision: i32 = 2;
    let log_maximum = if min == 0.0 && max == 0.0 {
        -1.0
    } else {
        max.abs().max(min.abs()).log10()
    };
    if log_maximum < 0.0 {
        if log_maximum % 1.0 != 0.0 {
            precision += log_maximum.abs() as i32;
        } else {
            precision += (log_maximum.abs() - 1.0) as i32;
        }
    } else if log_maximum > 2.0 {
        precision = 0;
    }
    precision.max(0) as usize
}

fn min_max(values: &[f64]) -> (f64, f64) {
    values
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &value| {
            (
                if value < lo { value } else { lo },
                if value > hi { value } else { hi },
            )
        })
}

/// Reduces or stretches `values` to exactly `count` points.
pub fn resample(values: &[f64], count: usize, downsample: Downsample) -> Vec<f64> {
    if count == 0 || values.is_empty() {
        return Vec::new();
    }
    if values.len() == 1 {
        return vec![values[0]; count];
    }
    if count == 1 {
        return vec![values[values.len() - 1]];
    }
    if values.len() > count && downsample != Downsample::Interpolate {
        return (0..count)
            .map(|column| {
                let start = column * values.len() / count;
                let end = ((column + 1) * values.len() / count).clamp(start + 1, values.len());
                let bucket = values[start..end].iter().copied().filter(|v| !v.is_nan());
                match downsample {
                    Downsample::Max => {
                        bucket.fold(
                            f64::NAN,
                            |acc, v| if acc.is_nan() || v > acc { v } else { acc },
                        )
                    }
                    _ => {
                        let (sum, n) = bucket.fold((0.0, 0), |(s, n), v| (s + v, n + 1));
                        if n == 0 {
                            f64::NAN
                        } else {
                            sum / f64::from(n)
                        }
                    }
                }
            })
            .collect();
    }
    interpolate(values, count)
}

/// Evenly spaced samples of the polyline through `values`, first and last
/// values kept exactly.
fn interpolate(values: &[f64], count: usize) -> Vec<f64> {
    let spring_factor = (values.len() - 1) as f64 / (count - 1) as f64;
    let mut result = Vec::with_capacity(count);
    result.push(values[0]);
    for index in 1..count - 1 {
        let spring = index as f64 * spring_factor;
        let (before, after) = (spring.floor(), spring.ceil());
        let at_point = spring - before;
        let (low, high) = (values[before as usize], values[after as usize]);
        result.push(low + (high - low) * at_point);
    }
    result.push(values[values.len() - 1]);
    result
}

