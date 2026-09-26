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

#[cfg(test)]
mod tests {
    use super::*;

    /// Row text with trailing spaces removed, since the reference trims the
    /// final character of its output and trailing padding is not meaningful.
    fn text(series: &[Vec<f64>], config: &ChartConfig) -> Vec<String> {
        plot(series, config)
            .to_lines()
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }

    fn expected(lines: &[&str]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }

    fn one(values: &[f64]) -> Vec<Vec<f64>> {
        vec![values.to_vec()]
    }

    #[test]
    fn flat_series_draw_a_single_row() {
        assert_eq!(
            text(&one(&[1.0; 5]), &ChartConfig::default()),
            expected(&[" 1.00 ┼────"])
        );
        assert_eq!(
            text(&one(&[0.0; 5]), &ChartConfig::default()),
            expected(&[" 0.00 ┼────"])
        );
    }

    #[test]
    fn matches_the_reference_for_a_mixed_series() {
        let series = one(&[2., 1., 1., 2., -2., 5., 7., 11., 3., 7., 1.]);
        assert_eq!(
            text(&series, &ChartConfig::default()),
            expected(&[
                " 11.00 ┤      ╭╮   ",
                " 10.00 ┤      ││   ",
                "  9.00 ┤      ││   ",
                "  8.00 ┤      ││   ",
                "  7.00 ┤     ╭╯│╭╮ ",
                "  6.00 ┤     │ │││ ",
                "  5.00 ┤    ╭╯ │││ ",
                "  4.00 ┤    │  │││ ",
                "  3.00 ┤    │  ╰╯│ ",
                "  2.00 ┼╮ ╭╮│    │ ",
                "  1.00 ┤╰─╯││    ╰ ",
                "  0.00 ┤   ││      ",
                " -1.00 ┤   ││      ",
                " -2.00 ┤   ╰╯     ",
            ])
        );
    }

    #[test]
    fn a_custom_height_rescales_the_rows() {
        let series = one(&[2., 1., 1., 2., -2., 5., 7., 11., 3., 7., 1.]);
        let config = ChartConfig {
            height: 4,
            offset: 3,
            ..ChartConfig::default()
        };
        assert_eq!(
            text(&series, &config),
            expected(&[
                " 11.00 ┤      ╭╮   ",
                "  7.75 ┤    ╭─╯│╭╮ ",
                "  4.50 ┼╮ ╭╮│  ╰╯│ ",
                "  1.25 ┤╰─╯││    ╰ ",
                " -2.00 ┤   ╰╯     ",
            ])
        );
    }

    #[test]
    fn fractional_ranges_use_more_decimals() {
        let series = one(&[0.2, 0.1, 0.2, 2., -0.9, 0.7, 0.91, 0.3, 0.7, 0.4, 0.5]);
        assert_eq!(
            text(&series, &ChartConfig::default()),
            expected(&[
                "  2.00 ┤  ╭╮ ╭╮    ",
                "  0.55 ┼──╯│╭╯╰─── ",
                " -0.90 ┤   ╰╯     ",
            ])
        );
        let small = one(&[0.01, 0.004, 0.003, 0.0042, 0.0083, 0.0033, 0.0079]);
        assert_eq!(
            text(&small, &ChartConfig::default()),
            expected(&[
                " 0.010 ┼╮      ",
                " 0.009 ┤│      ",
                " 0.008 ┤│  ╭╮╭ ",
                " 0.007 ┤│  │││ ",
                " 0.006 ┤│  │││ ",
                " 0.005 ┤│  │││ ",
                " 0.004 ┤╰╮╭╯││ ",
                " 0.003 ┤ ╰╯ ╰╯",
            ])
        );
    }

    #[test]
    fn large_values_drop_the_decimals() {
        let series = one(&[
            192., 431., 112., 449., -122., 375., 782., 123., 911., 1711., 172.,
        ]);
        let config = ChartConfig {
            height: 10,
            ..ChartConfig::default()
        };
        assert_eq!(
            text(&series, &config),
            expected(&[
                " 1711 ┤        ╭╮ ",
                " 1528 ┤        ││ ",
                " 1344 ┤        ││ ",
                " 1161 ┤        ││ ",
                "  978 ┤       ╭╯│ ",
                "  794 ┤     ╭╮│ │ ",
                "  611 ┤     │││ │ ",
                "  428 ┤╭╮╭╮╭╯││ │ ",
                "  245 ┼╯╰╯││ ╰╯ ╰ ",
                "   61 ┤   ││      ",
                " -122 ┤   ╰╯     ",
            ])
        );
    }

    #[test]
    fn negative_only_series_keep_their_sign() {
        let series = one(&[
            -5., -2., -3., -4., 0., -5., -6., -7., -8., 0., -9., -3., -5., -2., -9., -3., -1.,
        ]);
        assert_eq!(
            text(&series, &ChartConfig::default()),
            expected(&[
                "  0.00 ┤   ╭╮   ╭╮       ",
                " -1.00 ┤   ││   ││     ╭ ",
                " -2.00 ┤╭╮ ││   ││  ╭╮ │ ",
                " -3.00 ┤│╰╮││   ││╭╮││╭╯ ",
                " -4.00 ┤│ ╰╯│   │││││││  ",
                " -5.00 ┼╯   ╰╮  │││╰╯││  ",
                " -6.00 ┤     ╰╮ │││  ││  ",
                " -7.00 ┤      ╰╮│││  ││  ",
                " -8.00 ┤       ╰╯││  ││  ",
                " -9.00 ┤         ╰╯  ╰╯ ",
            ])
        );
    }

    #[test]
    fn a_target_width_interpolates_like_the_reference() {
        let series = one(&[
            0.3189989805,
            0.149949026,
            0.30142492354,
            0.195129182935,
            0.3142492354,
            0.1674974513,
            0.3142492354,
            0.1474974513,
            0.3047974513,
        ]);
        let config = ChartConfig {
            width: 30,
            height: 5,
            ..ChartConfig::default()
        };
        assert_eq!(
            text(&series, &config),
            expected(&[
                " 0.32 ┼╮            ╭─╮     ╭╮     ╭ ",
                " 0.29 ┤╰╮    ╭─╮   ╭╯ │    ╭╯│     │ ",
                " 0.26 ┤ │   ╭╯ ╰╮ ╭╯  ╰╮  ╭╯ ╰╮   ╭╯ ",
                " 0.23 ┤ ╰╮ ╭╯   ╰╮│    ╰╮╭╯   ╰╮ ╭╯  ",
                " 0.20 ┤  ╰╮│     ╰╯     ╰╯     │╭╯   ",
                " 0.16 ┤   ╰╯                   ╰╯   ",
            ])
        );
    }

    #[test]
    fn several_series_share_one_grid_and_report_their_index() {
        let series = vec![
            vec![0., 1., 2., 3., 3., 3., 2., 0.],
            vec![5., 4., 2., 1., 4., 6., 6.],
        ];
        let chart = plot(&series, &ChartConfig::default());
        assert_eq!(
            chart
                .to_lines()
                .iter()
                .map(|line| line.trim_end().to_string())
                .collect::<Vec<_>>(),
            expected(&[
                " 6.00 ┤    ╭─  ",
                " 5.00 ┼╮   │   ",
                " 4.00 ┤╰╮ ╭╯   ",
                " 3.00 ┤ │╭│─╮  ",
                " 2.00 ┤ ╰╮│ ╰╮ ",
                " 1.00 ┤╭╯╰╯  │ ",
                " 0.00 ┼╯     ╰",
            ])
        );
        let kinds: Vec<CellKind> = chart.rows.iter().flatten().map(|cell| cell.kind).collect();
        assert!(kinds.contains(&CellKind::Line(0)));
        assert!(kinds.contains(&CellKind::Line(1)));
        assert!(kinds.contains(&CellKind::Axis));
        assert!(kinds.contains(&CellKind::Label));
    }

    #[test]
    fn nan_values_leave_gaps() {
        let series = vec![vec![
            0.1,
            0.2,
            0.3,
            f64::NAN,
            0.5,
            0.6,
            0.7,
            f64::NAN,
            f64::NAN,
            0.9,
            1.0,
        ]];
        assert_eq!(
            text(&series, &ChartConfig::default()),
            expected(&[
                " 1.00 ┤         ╭ ",
                " 0.90 ┤        ─╯ ",
                " 0.80 ┤           ",
                " 0.70 ┤     ╭─    ",
                " 0.60 ┤    ╭╯     ",
                " 0.50 ┤   ─╯      ",
                " 0.40 ┤           ",
                " 0.30 ┤ ╭─        ",
                " 0.20 ┤╭╯         ",
                " 0.10 ┼╯         ",
            ])
        );
    }

    #[test]
    fn short_series_are_padded_with_gaps_when_a_width_is_given() {
        let series = vec![
            vec![0., 0., 2., 2., f64::NAN],
            vec![1., 1., 1., 1., 1., 1., 1.],
            vec![f64::NAN, f64::NAN, f64::NAN, 0., 0., 2., 2.],
        ];
        assert_eq!(
            text(&series, &ChartConfig::default()),
            expected(&[" 2.00 ┤ ╭──╭─ ", " 1.00 ┼────│─ ", " 0.00 ┼─╯──╯ ",])
        );
    }

    #[test]
    fn max_downsampling_keeps_a_spike_that_interpolation_can_miss() {
        let mut values = vec![0.0; 100];
        values[51] = 50.0;
        let interpolated = resample(&values, 10, Downsample::Interpolate);
        let maxed = resample(&values, 10, Downsample::Max);
        assert!(maxed.iter().copied().fold(0.0, f64::max) >= 50.0);
        assert!(interpolated.iter().copied().fold(0.0, f64::max) < 50.0);
        assert_eq!(maxed.len(), 10);
        let mean = resample(&[2.0, 4.0, 6.0, 8.0], 2, Downsample::Mean);
        assert_eq!(mean, vec![3.0, 7.0]);
    }

    #[test]
    fn stretching_keeps_the_endpoints() {
        let stretched = resample(&[1.0, 5.0], 5, Downsample::Interpolate);
        assert_eq!(stretched, vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(resample(&[7.0], 3, Downsample::Max), vec![7.0; 3]);
    }

    #[test]
    fn include_zero_extends_the_axis_to_zero() {
        let series = one(&[100., 110., 120.]);
        let config = ChartConfig {
            height: 4,
            include_zero: true,
            ..ChartConfig::default()
        };
        let lines = plot(&series, &config).to_lines();
        assert!(lines.last().unwrap().contains("0"), "{lines:?}");
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn empty_or_all_nan_input_draws_nothing() {
        assert!(plot(&[], &ChartConfig::default()).rows.is_empty());
        assert!(plot(&[vec![]], &ChartConfig::default()).rows.is_empty());
        assert!(plot(&[vec![f64::NAN; 4]], &ChartConfig::default())
            .rows
            .is_empty());
    }

    #[test]
    fn gutter_counts_the_label_and_axis_columns() {
        let chart = plot(&one(&[1., 2., 3.]), &ChartConfig::default());
        // " 3.00 ┤" is 7 display columns; the first data column follows it.
        assert_eq!(chart.gutter, 7);
        assert_eq!(chart.rows[0][chart.gutter - 1].ch, '┤');
    }

    #[test]
    fn a_label_formatter_replaces_the_number_format() {
        let config = ChartConfig {
            height: 2,
            label_formatter: Some(|value| format!("{:.0}k", value / 1000.0)),
            ..ChartConfig::default()
        };
        let lines = plot(&one(&[1000., 3000., 5000.]), &config).to_lines();
        assert!(lines[0].contains("5k"), "{lines:?}");
        assert!(lines[2].contains("1k"), "{lines:?}");
    }
}
