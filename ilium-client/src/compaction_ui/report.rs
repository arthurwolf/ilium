//! The report page of the Optimization tab: recommendation card on top, then
//! the comparison, statistics, cost mix, fixed prefix, simulation (table and
//! chart), per-model optima, rework sensitivity, regimes and warnings.
//! Everything is laid out from the plain-data [`CompactionReport`]; widths
//! adapt (columns drop, text wraps) from a 48-column content area, which is
//! what an 80-column terminal leaves next to the tab list, up to wide screens.

use ilium_compaction_analysis::stats::Quantiles;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::text::{bar_parts, table_lines, Column, TableRow};
use super::{
    accent, dim, push_apply_note, push_current_setting, scan_note, warning, Action, Builder,
    Screen, INSET,
};
use crate::compaction_app::agent_label;
use crate::compaction_report::{
    format_bytes, format_elapsed, format_percent, format_signed_percent, format_token_quantiles,
    format_usd, group_thousands, humanize_tokens, CompactionReport, ComparisonKind, ComparisonRow,
};
use ilium_compaction_analysis::optimize::PointQuality;

pub(super) fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub(super) fn warning_bold() -> Style {
    warning().add_modifier(Modifier::BOLD)
}

/// Lays out the whole report page.
pub(super) fn ready(builder: &mut Builder, screen: &Screen<'_>, report: &CompactionReport) {
    if let Some(note) = scan_note(screen) {
        builder.paragraph(&note, warning());
    }
    push_apply_note(builder, screen.panel.note.as_ref());
    recommendation(builder, screen, report);
    comparison(builder, screen, report);
    corpus(builder, report);
    cost_mix(builder, report);
    fixed_prefix(builder, report);
    super::simulation::simulation(builder, report);
    per_model(builder, report);
    rework(builder, report);
    regimes(builder, report);
    warnings(builder, report);
    footer(builder, report);
}

fn timestamp(unix_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

pub(super) fn tokens(value: u32) -> String {
    humanize_tokens(f64::from(value))
}

fn range(low: u32, high: u32) -> String {
    if low == high {
        tokens(low)
    } else {
        format!("{}-{}", tokens(low), tokens(high))
    }
}

fn count_quantiles(quantiles: &Quantiles, unit: &str) -> String {
    format!(
        "p10 {:.0} | p50 {:.0} | p90 {:.0} {unit}(n={})",
        quantiles.p10,
        quantiles.p50,
        quantiles.p90,
        group_thousands(quantiles.n as u64)
    )
}

// ------------------------------------------------------- recommendation

fn recommendation(builder: &mut Builder, screen: &Screen<'_>, report: &CompactionReport) {
    builder.heading("RECOMMENDATION", "OPT-04");
    match &report.recommendation {
        Some(card) => {
            builder.button_block(
                Action::Apply,
                &format!(
                    "{} - Apply to {}",
                    card.headline_text,
                    agent_label(screen.agent)
                ),
                super::primary_button_style(),
            );
            builder.field("Confidence", card.confidence_label());
            if card.extrapolated {
                builder.field_styled("Extrapolated", &card.observed_support_text, warning_bold());
            } else {
                builder.field("Support", &card.observed_support_text);
            }
            builder.field("Pick rule", &card.pick_rule_text);
            builder.paragraph(&card.basis, dim());
            builder.field(
                "Flat band",
                &format!(
                    "within 2%: trigger {} · within 5%: trigger {}",
                    range(card.band_2_percent_tokens.0, card.band_2_percent_tokens.1),
                    range(card.band_5_percent_tokens.0, card.band_5_percent_tokens.1)
                ),
            );
            if card.setting_clamped {
                builder.paragraph(
                    "The value was clamped into the range the agent accepts.",
                    warning(),
                );
            }
            if let Some(alternative) = &card.extrapolated_alternative {
                builder.blank();
                builder.button_block(
                    Action::ApplyExtrapolated,
                    &format!(
                        "Apply simulated optimum (extrapolated): {} - {} cheaper",
                        group_thousands(alternative.setting_value),
                        format_percent(alternative.relative_saving)
                    ),
                    super::secondary_button_style(),
                );
                builder.field("Extrapolated support", &alternative.observed_support_text);
                builder.paragraph(
                    "Warning: the simulation extrapolates beyond your observed compactions here. No compaction was observed at or below this level, so the saving is a model result, not a measurement.",
                    warning(),
                );
                builder.blank();
            }
            match card.current_vs_recommended {
                Some(delta) if delta.abs() < 0.0005 => builder.field(
                    "Your current setting",
                    "costs the same as the recommendation",
                ),
                Some(delta) if delta > 0.0 => builder.field_styled(
                    "Your current setting",
                    &format!(
                        "costs {} more than the recommendation",
                        format_percent(delta)
                    ),
                    warning(),
                ),
                Some(delta) => builder.field(
                    "Your current setting",
                    &format!(
                        "costs {} less than the recommendation (the pick favours observed support)",
                        format_percent(-delta)
                    ),
                ),
                None => builder.field(
                    "Your current setting",
                    &format!(
                        "is not set: the CLI default (trigger ~{}) applies",
                        tokens(report.semantics.cli_default_trigger_tokens)
                    ),
                ),
            }
        }
        None => {
            builder.paragraph(
                report
                    .status_message
                    .as_deref()
                    .unwrap_or("No recommendation could be computed."),
                warning(),
            );
            push_current_setting(builder, screen);
        }
    }
    builder.blank();
    let mut buttons = vec![(
        Action::Scan,
        "Re-scan".to_owned(),
        super::secondary_button_style(),
    )];
    if let Some(record) = &screen.panel.record {
        buttons.push((
            Action::Revert,
            super::revert_label(record, builder.text_width()),
            super::secondary_button_style(),
        ));
    }
    builder.mark("OPT-05");
    builder.button_row(&buttons);
    builder.blank();
}

// ----------------------------------------------------------- comparison

fn comparison(builder: &mut Builder, screen: &Screen<'_>, report: &CompactionReport) {
    if report.comparison.is_empty() {
        return;
    }
    builder.heading("CLI DEFAULT | CURRENT | RECOMMENDED", "OPT-06");
    let columns = [
        Column::left("", 11, 0),
        Column::right("Setting", 9, 0),
        Column::right("Trigger", 8, 1),
        Column::right("vs default", 10, 0),
        Column::right("Cost", 8, 2),
        Column::right("USD", 9, 3),
        Column::right("Compactions", 11, 4),
    ];
    let mut rows: Vec<TableRow> = Vec::new();
    let mut has_current = false;
    for row in &report.comparison {
        has_current |= row.kind == ComparisonKind::CurrentSetting;
        rows.push(comparison_row(row));
    }
    if !has_current {
        let not_set = match screen.panel.current {
            crate::compaction_app::CurrentSetting::NoFile => "no file",
            _ => "not set",
        };
        let row = TableRow {
            cells: vec![
                "Current".to_owned(),
                not_set.to_owned(),
                "-".to_owned(),
                "-".to_owned(),
                "-".to_owned(),
                "-".to_owned(),
                "-".to_owned(),
            ],
            style: dim(),
        };
        rows.insert(rows.len().saturating_sub(1).min(1), row);
    }
    let lines = table_lines(&columns, &rows, builder.table_width(), INSET, dim());
    builder.push_lines(lines);
    for row in &report.comparison {
        if row.quality != PointQuality::Exact {
            builder.paragraph(&format!("{}: {}.", row.label, row.quality_note), dim());
        }
    }
    builder.paragraph(
        "Cost is in weighted input-token equivalents with the rework of each compaction included; USD is an API-price equivalent.",
        dim(),
    );
    builder.blank();
}

fn comparison_row(row: &ComparisonRow) -> TableRow {
    let (label, style) = match row.kind {
        ComparisonKind::CliDefault => ("CLI default", Style::new()),
        ComparisonKind::CurrentSetting => ("Current", Style::new()),
        ComparisonKind::Recommended => ("Recommended", accent()),
    };
    TableRow {
        cells: vec![
            label.to_owned(),
            row.setting_value
                .map_or_else(|| "-".to_owned(), |value| group_thousands(u64::from(value))),
            tokens(row.trigger_tokens),
            format_signed_percent(row.relative_to_default),
            humanize_tokens(row.cost_weighted_tokens),
            row.cost_usd.map_or_else(|| "n/a".to_owned(), format_usd),
            format!("{:.1}", row.compactions),
        ],
        style,
    }
}

// ------------------------------------------------------------- corpus

fn corpus(builder: &mut Builder, report: &CompactionReport) {
    let corpus = &report.corpus;
    let stats = &report.compactions;
    builder.heading("CORPUS AND COMPACTIONS", "OPT-07");
    builder.field(
        "Transcripts",
        &format!(
            "{} files ({}) over {:.1} days: {} main sessions, {} subagent sessions",
            group_thousands(corpus.files_listed),
            format_bytes(corpus.bytes_total),
            corpus.span_days,
            group_thousands(corpus.main_sessions),
            group_thousands(corpus.subagent_sessions)
        ),
    );
    builder.field(
        "Requests",
        &format!(
            "{} main, {} subagent ({} subagent)",
            group_thousands(corpus.main_requests),
            group_thousands(corpus.subagent_requests),
            format_percent(corpus.subagent_request_share)
        ),
    );
    builder.field(
        "Scan",
        &format!(
            "{} parsed, {} from cache, {} skipped, {}; {} duplicate requests removed",
            group_thousands(corpus.files_parsed),
            group_thousands(corpus.files_from_cache),
            group_thousands(corpus.files_skipped),
            format_elapsed(corpus.scan_seconds),
            group_thousands(corpus.duplicate_turns_removed)
        ),
    );
    if corpus.sessions_dropped_for_cap > 0 {
        builder.field_styled(
            "Dropped",
            &format!(
                "{} older sessions did not fit the retained-result cap",
                group_thousands(corpus.sessions_dropped_for_cap)
            ),
            warning(),
        );
    }
    let unknown = if stats.unknown_trigger > 0 {
        format!(", {} of unstated trigger", stats.unknown_trigger)
    } else {
        String::new()
    };
    builder.field(
        "Compactions",
        &format!(
            "{} in {} sessions ({} auto, {} manual{unknown}; {} measured)",
            group_thousands(stats.total),
            group_thousands(stats.sessions_with_compaction),
            group_thousands(stats.auto),
            group_thousands(stats.manual),
            group_thousands(stats.measured)
        ),
    );
    if let Some(quantiles) = &stats.pre_tokens {
        builder.field("Fired at", &format_token_quantiles(quantiles));
    }
    if let Some(quantiles) = &stats.first_post_request_tokens {
        builder.field("First request after", &format_token_quantiles(quantiles));
    }
    if let Some(quantiles) = &stats.first_post_cache_read_share {
        builder.field(
            "First request cache hit",
            &format!(
                "p10 {} | p50 {} | p90 {}",
                format_percent(quantiles.p10),
                format_percent(quantiles.p50),
                format_percent(quantiles.p90)
            ),
        );
    }
    if let Some(quantiles) = &stats.summary_tokens {
        builder.field("Summary size", &format_token_quantiles(quantiles));
    }
    if let Some(quantiles) = &stats.cycle_requests {
        builder.field("Cycle length", &count_quantiles(quantiles, "requests "));
    }
    if let Some(quantiles) = &stats.cycle_minutes {
        builder.field("Cycle duration", &count_quantiles(quantiles, "minutes "));
    }
    builder.field(
        "Cycles",
        &format!(
            "{}: {} of at most 3 requests (refill thrash), {} of at most 10, {} of at most 30",
            group_thousands(stats.cycles),
            group_thousands(stats.thrash_cycles),
            group_thousands(stats.cycles_at_most_10),
            group_thousands(stats.cycles_at_most_30)
        ),
    );
    builder.blank();
}

// ------------------------------------------------------------ cost mix

fn share_line(builder: &mut Builder, label: &str, share: f64) {
    const LABEL_WIDTH: usize = 12;
    let bar_width = builder
        .text_width()
        .saturating_sub(LABEL_WIDTH + 9)
        .clamp(6, 40);
    let (filled, empty) = bar_parts(share, bar_width);
    builder.line(vec![
        Span::raw(" ".repeat(INSET)),
        Span::styled(super::text::pad_right(label, LABEL_WIDTH), dim()),
        Span::styled(filled, accent()),
        Span::styled(empty, dim()),
        Span::raw(format!(" {:>6}", format_percent(share))),
    ]);
}

fn cost_mix(builder: &mut Builder, report: &CompactionReport) {
    let mix = &report.cost_mix;
    let cold = &report.cold_cache;
    builder.heading("COST MIX AND COLD CACHE", "OPT-08");
    share_line(builder, "cache read", mix.cache_read_share);
    share_line(builder, "cache write", mix.cache_write_share);
    share_line(builder, "uncached in", mix.input_share);
    share_line(builder, "output", mix.output_share);
    builder.field(
        "Total",
        &format!(
            "{} weighted input-token equivalents{}",
            humanize_tokens(mix.total_weighted_tokens),
            mix.total_usd
                .map_or_else(String::new, |usd| format!(", {}", format_usd(usd)))
        ),
    );
    builder.field(
        "Cold-cache requests",
        &format!(
            "{} of {} requests ({} after an idle gap); rewriting prefixes costs {} of the total",
            format_percent(cold.cache_cold_share),
            group_thousands(cold.requests_considered),
            format_percent(cold.gap_cold_share),
            format_percent(cold.avoidable_rewrite_cost_share)
        ),
    );
    for reach in &mix.reach {
        builder.paragraph(
            &format!(
                "{} of the cost is spent above {} of context ({} sessions get there).",
                format_percent(reach.cost_share_above),
                tokens(reach.threshold_tokens),
                group_thousands(reach.sessions_reaching)
            ),
            dim(),
        );
    }
    if !mix.models.is_empty() {
        builder.blank();
        let columns = [
            Column::left("Model", 24, 0),
            Column::right("Share", 7, 0),
            Column::right("USD", 9, 1),
            Column::right("Requests", 9, 2),
        ];
        let rows: Vec<TableRow> = mix
            .models
            .iter()
            .map(|model| TableRow {
                cells: vec![
                    model.model.clone(),
                    format_percent(model.weighted_cost_share),
                    model.usd.map_or_else(|| "n/a".to_owned(), format_usd),
                    group_thousands(model.requests),
                ],
                style: Style::new(),
            })
            .collect();
        builder.push_lines(table_lines(
            &columns,
            &rows,
            builder.table_width(),
            INSET,
            dim(),
        ));
    }
    builder.blank();
}

// -------------------------------------------------------- fixed prefix

fn fixed_prefix(builder: &mut Builder, report: &CompactionReport) {
    let prefix = &report.fixed_prefix;
    builder.heading("FIXED PREFIX (C0)", "OPT-09");
    match &prefix.first_request_tokens {
        Some(quantiles) => builder.field(
            "First request of a session",
            &format_token_quantiles(quantiles),
        ),
        None => builder.field("First request of a session", "no data"),
    }
    if let Some(quantiles) = &prefix.post_compaction_request_tokens {
        builder.field(
            "First request after a compaction",
            &format_token_quantiles(quantiles),
        );
    }
    builder.paragraph(&prefix.rule_of_thumb, dim());
    builder.blank();
}

// ------------------------------------------------------------ per model

fn per_model(builder: &mut Builder, report: &CompactionReport) {
    if report.per_model.is_empty() && report.per_model_note.is_empty() {
        return;
    }
    builder.heading("PER-MODEL OPTIMA", "OPT-11");
    if !report.per_model_note.is_empty() {
        builder.paragraph(&report.per_model_note, warning_bold());
    }
    if report.per_model.is_empty() {
        builder.paragraph("No model family had enough replayable sessions.", dim());
    } else {
        let columns = [
            Column::left("Model family", 18, 0),
            Column::right("Optimum", 8, 0),
            Column::right("Setting", 9, 1),
            Column::right("Flat band", 13, 0),
            Column::right("Sessions", 8, 2),
            Column::right("95% interval", 13, 3),
        ];
        let rows: Vec<TableRow> = report
            .per_model
            .iter()
            .map(|row| TableRow {
                cells: vec![
                    row.family.clone(),
                    tokens(row.optimum_trigger_tokens),
                    group_thousands(u64::from(row.optimum_setting_value)),
                    range(row.band_low_tokens, row.band_high_tokens),
                    group_thousands(row.sessions),
                    range(row.interval_tokens.0, row.interval_tokens.1),
                ],
                style: Style::new(),
            })
            .collect();
        builder.push_lines(table_lines(
            &columns,
            &rows,
            builder.table_width(),
            INSET,
            dim(),
        ));
    }
    builder.blank();
}

// --------------------------------------------------------------- rework

fn rework(builder: &mut Builder, report: &CompactionReport) {
    if report.rework.is_empty() {
        return;
    }
    builder.heading("REWORK SENSITIVITY", "OPT-12");
    builder.paragraph(&report.rework_note, dim());
    let columns = [
        Column::left("Rework", 8, 0),
        Column::right("Optimum", 8, 0),
        Column::right("Flat band", 13, 0),
        Column::right("Pick overhead", 13, 1),
    ];
    let rows: Vec<TableRow> = report
        .rework
        .iter()
        .map(|row| TableRow {
            cells: vec![
                format!("{}x", row.multiplier),
                tokens(row.optimum_trigger_tokens),
                range(row.band_low_tokens, row.band_high_tokens),
                format_signed_percent(row.pick_overhead),
            ],
            style: if (row.multiplier - 1.0).abs() < f64::EPSILON {
                bold()
            } else {
                Style::new()
            },
        })
        .collect();
    builder.push_lines(table_lines(
        &columns,
        &rows,
        builder.table_width(),
        INSET,
        dim(),
    ));
    builder.blank();
}

// -------------------------------------------------------------- regimes

fn regimes(builder: &mut Builder, report: &CompactionReport) {
    builder.heading("COMPACTION REGIMES AND MAPPING", "OPT-13");
    if report.regimes.is_empty() {
        builder.paragraph(
            "No compaction was observed, so there is no regime to report.",
            dim(),
        );
    }
    for regime in &report.regimes {
        builder.paragraph(
            &format!(
                "{} compactions around {} ({}){}",
                group_thousands(regime.count),
                tokens(regime.center_tokens),
                range(regime.min_tokens, regime.max_tokens),
                if regime.is_most_recent {
                    ", most recent regime"
                } else {
                    ""
                }
            ),
            Style::new(),
        );
    }
    builder.paragraph(&report.semantics.mapping_note, dim());
    builder.paragraph(
        &format!(
            "CLI default: trigger ~{} ({}).",
            tokens(report.semantics.cli_default_trigger_tokens),
            report.semantics.cli_default_note
        ),
        dim(),
    );
    builder.blank();
}

// ------------------------------------------------------------- warnings

fn warnings(builder: &mut Builder, report: &CompactionReport) {
    let card = report.recommendation.as_ref();
    let grid_limited = card.is_some_and(|card| card.grid_floor_limited);
    let grid_widened = card.is_some_and(|card| card.grid_widened && !card.grid_floor_limited);
    if report.warnings.is_empty() && !grid_limited && !grid_widened {
        return;
    }
    builder.heading("WARNINGS", "OPT-14");
    if grid_limited {
        builder.bullet(
            "The optimum still sits at the edge of the simulated range, so the true optimum may lie outside it.",
            warning(),
        );
    }
    if grid_widened {
        builder.bullet(
            "The simulated range was widened once because the first optimum sat at its edge.",
            dim(),
        );
    }
    for text in &report.warnings {
        builder.bullet(text, warning());
    }
    builder.blank();
}

fn footer(builder: &mut Builder, report: &CompactionReport) {
    builder.paragraph(
        &format!(
            "Report of {} · {} transcripts · the optimizer never changes anything on its own.",
            timestamp(report.generated_at_unix_ms),
            group_thousands(report.corpus.files_listed)
        ),
        dim(),
    );
}
