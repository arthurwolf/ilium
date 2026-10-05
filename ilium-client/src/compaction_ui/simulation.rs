//! The simulation section of the report: the candidate table (pick, best,
//! default and current markers) and the cost chart under it.

use ratatui::style::Style;
use ratatui::text::Span;

use super::report::{bold, tokens, warning_bold};
use super::text::{table_lines, Column, TableRow};
use super::{accent, dim, warning, Builder, INSET};
use crate::ascii_chart::{self, CellKind, ChartConfig, Downsample};
use crate::compaction_report::{
    format_percent, format_signed_percent, format_usd, group_thousands, humanize_tokens,
    CompactionReport, SimulationRow,
};

/// The chart's y axis stops here (percent above the best candidate); a row
/// above it is flattened so the interesting range stays readable.
const CHART_CAP_PERCENT: f64 = 50.0;
/// Content width from which the simulation markers are spelled out.
const WIDE_TABLE_WIDTH: usize = 88;

// ---------------------------------------------------------- simulation

fn marks(row: &SimulationRow, is_wide: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if is_wide {
        if row.is_pick {
            parts.push("pick");
        }
        if row.is_best {
            parts.push("best");
        }
        if row.is_cli_default {
            parts.push("default");
        }
        if row.is_current {
            parts.push("current");
        }
    } else {
        if row.is_pick {
            parts.push("P");
        }
        if row.is_best {
            parts.push("B");
        }
        if row.is_cli_default {
            parts.push("D");
        }
        if row.is_current {
            parts.push("C");
        }
    }
    parts.join(" ")
}

pub(super) fn simulation(builder: &mut Builder, report: &CompactionReport) {
    if report.simulation.is_empty() {
        return;
    }
    let is_wide = builder.table_width() >= WIDE_TABLE_WIDTH;
    builder.heading("SIMULATION BY TRIGGER", "OPT-10");
    let columns = [
        Column::right("Trigger", 7, 0),
        Column::right("Setting", 9, 0),
        Column::right("vs best", 8, 0),
        Column::left("Band", 4, 1),
        Column::right("Support", 8, 2),
        Column::right("Compactions", 11, 4),
        Column::right("Cost", 8, 3),
        Column::right("USD", 9, 5),
        Column::left("Marks", if is_wide { 25 } else { 7 }, 0),
    ];
    let rows: Vec<TableRow> = report
        .simulation
        .iter()
        .map(|row| {
            let band = if row.within_2_percent {
                "2%"
            } else if row.within_5_percent {
                "5%"
            } else {
                ""
            };
            TableRow {
                cells: vec![
                    tokens(row.trigger_tokens),
                    group_thousands(u64::from(row.setting_value)),
                    format_signed_percent(row.relative_to_best),
                    band.to_owned(),
                    format_percent(row.observed_support),
                    format!("{:.1}", row.compactions),
                    humanize_tokens(row.cost_weighted_tokens),
                    row.cost_usd.map_or_else(|| "n/a".to_owned(), format_usd),
                    marks(row, is_wide),
                ],
                style: if row.is_pick {
                    accent()
                } else if row.is_best {
                    bold()
                } else {
                    Style::new()
                },
            }
        })
        .collect();
    builder.push_lines(table_lines(
        &columns,
        &rows,
        builder.table_width(),
        INSET,
        dim(),
    ));
    let legend = if is_wide {
        "pick = recommendation, best = lowest simulated cost, default = CLI default, current = your setting; Band = within 2% / 5% of the best cost; Support = share of observed compactions at or below the trigger."
    } else {
        "P pick (recommendation) · B best · D CLI default · C current · Band: within 2% / 5% of the best cost."
    };
    builder.paragraph(legend, dim());
    builder.blank();
    chart(builder, &report.simulation);
    if let Some(bootstrap) = &report.bootstrap {
        builder.field(
            "Bootstrap",
            &format!(
                "{} resamples of the sessions put the optimum at {}-{} (median {}); the pick stayed within 2% of the best in {} of them",
                group_thousands(bootstrap.resamples),
                tokens(bootstrap.argmin_p2_5),
                tokens(bootstrap.argmin_p97_5),
                tokens(bootstrap.argmin_median),
                format_percent(bootstrap.pick_within_band_share)
            ),
        );
    }
    builder.blank();
}

fn percent_axis_label(value: f64) -> String {
    format!("{value:.0}%")
}

fn run_spans(cells: Vec<(char, Style)>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style = Style::new();
    for (character, style) in cells {
        if !run.is_empty() && style != run_style {
            spans.push(Span::styled(std::mem::take(&mut run), run_style));
        }
        run_style = style;
        run.push(character);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, run_style));
    }
    spans
}

/// Cost over the best candidate, per trigger: an asciichart line with one
/// marker row (P pick, D default, C current, B best) under its axis.
fn chart(builder: &mut Builder, rows: &[SimulationRow]) {
    if rows.len() < 3 {
        return;
    }
    let available = builder.table_width().saturating_sub(INSET);
    let height = if available >= WIDE_TABLE_WIDTH { 10 } else { 6 };
    let series = vec![rows
        .iter()
        .map(|row| (row.relative_to_best * 100.0).clamp(0.0, CHART_CAP_PERCENT))
        .collect::<Vec<f64>>()];
    let plot = |columns: usize| {
        ascii_chart::plot(
            &series,
            &ChartConfig {
                width: columns,
                height,
                offset: 0,
                downsample: Downsample::Max,
                include_zero: true,
                label_formatter: Some(percent_axis_label),
            },
        )
    };
    let probe = plot(available.saturating_sub(12).max(8));
    if probe.rows.is_empty() {
        return;
    }
    let mut data_columns = available.saturating_sub(probe.gutter + 1).max(8);
    let mut rendered = plot(data_columns);
    for _ in 0..3 {
        let excess = rendered.width().saturating_sub(available);
        if excess == 0 || data_columns <= 8 {
            break;
        }
        data_columns = data_columns.saturating_sub(excess).max(8);
        rendered = plot(data_columns);
    }
    let gutter = rendered.gutter;
    let data_width = rendered.width().saturating_sub(gutter).max(1);

    builder.paragraph(
        &format!(
            "Extra cost over the best trigger (y, capped at {CHART_CAP_PERCENT:.0}%) by trigger size (x)"
        ),
        dim(),
    );
    for row in &rendered.rows {
        let mut cells: Vec<(char, Style)> = vec![(' ', Style::new()); INSET];
        for cell in row {
            let style = match cell.kind {
                CellKind::Blank => Style::new(),
                CellKind::Label | CellKind::Axis => dim(),
                CellKind::Line(_) => accent(),
            };
            cells.push((cell.ch, style));
        }
        builder.line(run_spans(cells));
    }

    // Marker row: the series is stretched so that candidate `i` sits at
    // `i * (columns - 1) / (count - 1)`.
    let last = rows.len() - 1;
    let mut markers: Vec<(char, Style)> = vec![(' ', Style::new()); INSET + gutter + data_width];
    let mut place = |index: usize, character: char, style: Style| {
        let column = INSET + gutter + index * data_width.saturating_sub(1) / last;
        if let Some(slot) = markers.get_mut(column) {
            *slot = (character, style);
        }
    };
    // Lowest priority first: later marks overwrite earlier ones.
    for (index, row) in rows.iter().enumerate() {
        if row.is_best {
            place(index, 'B', dim());
        }
    }
    for (index, row) in rows.iter().enumerate() {
        if row.is_cli_default {
            place(index, 'D', warning());
        }
    }
    for (index, row) in rows.iter().enumerate() {
        if row.is_current {
            place(index, 'C', warning_bold());
        }
    }
    for (index, row) in rows.iter().enumerate() {
        if row.is_pick {
            place(index, 'P', accent());
        }
    }
    builder.line(run_spans(markers));

    let first_label = tokens(rows[0].trigger_tokens);
    let last_label = tokens(rows[last].trigger_tokens);
    let gap = data_width.saturating_sub(first_label.chars().count() + last_label.chars().count());
    builder.line(vec![Span::styled(
        format!(
            "{}{first_label}{}{last_label}",
            " ".repeat(INSET + gutter),
            " ".repeat(gap.max(1))
        ),
        dim(),
    )]);
    builder.paragraph("P pick · B best · D CLI default · C current", dim());
    builder.blank();
}
