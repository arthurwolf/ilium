//! Tests of the Optimization tab layout: pure functions over a hand-built
//! report, so every figure in the golden lines is a chosen number.

use std::collections::BTreeSet;
use std::time::Duration;

use ilium_compaction_analysis::optimize::PointQuality;
use ilium_compaction_analysis::stats::Quantiles;
use ilium_compaction_analysis::AgentKind;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::Terminal;
use unicode_width::UnicodeWidthStr;

use super::text::{bar_parts, kept_columns, truncate_to, wrap_text, Column};
use super::*;
use crate::agent_config_writer::{AgentConfigTarget, ApplyRecord};
use crate::compaction_app::{AgentPanel, CurrentSetting, Note, NoteTone};
use crate::compaction_report::{
    BootstrapRow, ColdCacheSummary, CompactionSummary, ComparisonKind, ComparisonRow, Confidence,
    CorpusSummary, CostMixSummary, FixedPrefixSummary, ModelUsageRow, PerModelRow, PickRule,
    ReachRow, RecommendationCard, RegimeRow, ReportStatus, ReworkRow, SemanticsSummary,
    SimulationRow,
};

fn quantiles(n: usize, p10: f64, p50: f64, p90: f64) -> Quantiles {
    Quantiles {
        n,
        p10,
        p50,
        p90,
        mean: p50,
    }
}

/// (trigger, weighted cost in millions, observed support)
const CANDIDATES: [(u32, f64, f64); 9] = [
    (100_000, 13.1, 0.0),
    (120_000, 12.4, 0.01),
    (140_000, 12.1, 0.02),
    (150_000, 12.0, 0.03),
    (160_000, 12.05, 0.04),
    (180_000, 12.3, 0.06),
    (200_000, 12.9, 0.10),
    (225_000, 14.0, 0.30),
    (232_000, 14.4, 0.40),
];

fn simulation_rows() -> Vec<SimulationRow> {
    CANDIDATES
        .iter()
        .map(|&(trigger, cost, support)| {
            let relative = cost / 12.0 - 1.0;
            SimulationRow {
                trigger_tokens: trigger,
                setting_value: trigger,
                cost_weighted_tokens: cost * 1_000_000.0,
                cost_usd: Some(cost * 4.0),
                compactions: 120.0 - f64::from(trigger) / 2_000.0,
                relative_to_best: relative,
                observed_support: support,
                within_2_percent: relative <= 0.02,
                within_5_percent: relative <= 0.05,
                is_best: trigger == 150_000,
                is_pick: trigger == 150_000,
                is_cli_default: trigger == 232_000,
                is_current: trigger == 200_000,
            }
        })
        .collect()
}

fn comparison_row(
    kind: ComparisonKind,
    label: &str,
    trigger: u32,
    cost: f64,
    quality: PointQuality,
) -> ComparisonRow {
    ComparisonRow {
        kind,
        label: label.to_owned(),
        trigger_tokens: trigger,
        setting_value: Some(trigger),
        cost_weighted_tokens: cost * 1_000_000.0,
        cost_usd: Some(cost * 4.0),
        compactions: 60.0,
        relative_to_default: cost / 14.4 - 1.0,
        relative_to_recommended: cost / 12.0 - 1.0,
        quality,
        quality_note: match quality {
            PointQuality::Exact => "simulated point",
            PointQuality::Interpolated => "interpolated between two simulated points",
            PointQuality::OutsideGrid => "outside the simulated range; nearest point shown",
        }
        .to_owned(),
    }
}

/// A complete Codex report with chosen numbers.
pub(crate) fn fixture_report() -> CompactionReport {
    let card = RecommendationCard {
        trigger_tokens: 150_000,
        setting_key: "model_auto_compact_token_limit".to_owned(),
        setting_value: 150_000,
        setting_clamped: false,
        headline_text:
            "Optimal: model_auto_compact_token_limit 150,000 (trigger ~150k, the simulated best, extrapolated)"
                .to_owned(),
        extrapolated: true,
        observed_support_text:
            "extrapolated: 3.0% of observed compactions at or below this level".to_owned(),
        observed_support_fraction: 0.03,
        confidence: Confidence::Medium,
        basis: "Lowest cost of the simulated candidates; no observed compaction supports it, so the pick is the plain argmin.".to_owned(),
        pick_rule: PickRule::PlainArgmin,
        pick_rule_text: "plain optimum, extrapolated".to_owned(),
        unsupported_argmin_tokens: None,
        unsupported_relative_saving: None,
        grid_floor_limited: false,
        grid_widened: false,
        extrapolated_alternative: None,
        band_2_percent_tokens: (140_000, 160_000),
        band_5_percent_tokens: (120_000, 180_000),
        current_vs_recommended: Some(0.075),
        warnings: vec!["Dollar figures are API-price equivalents.".to_owned()],
    };
    CompactionReport {
        agent: AgentKind::Codex,
        generated_at_unix_ms: 1_791_000_000_000,
        status: ReportStatus::Complete,
        status_message: None,
        corpus: CorpusSummary {
            files_listed: 5_678,
            files_parsed: 4_000,
            files_from_cache: 1_678,
            files_skipped: 2,
            bytes_total: 29_300_000_000,
            main_sessions: 1_200,
            subagent_sessions: 0,
            main_requests: 150_000,
            subagent_requests: 0,
            subagent_request_share: 0.0,
            first_timestamp_ms: 1_770_000_000_000,
            last_timestamp_ms: 1_788_000_000_000,
            span_days: 212.0,
            lines_skipped: 3,
            duplicate_turns_removed: 1_234,
            sessions_dropped_for_cap: 0,
            scan_seconds: 125.0,
        },
        compactions: CompactionSummary {
            total: 340,
            auto: 300,
            manual: 40,
            unknown_trigger: 0,
            measured: 330,
            sessions_with_compaction: 120,
            pre_tokens: Some(quantiles(340, 190_000.0, 224_000.0, 232_000.0)),
            pre_tokens_auto: None,
            first_post_request_tokens: Some(quantiles(330, 30_000.0, 41_000.0, 62_000.0)),
            first_post_cache_read_share: Some(quantiles(330, 0.0, 0.5, 0.9)),
            summary_tokens: Some(quantiles(330, 2_000.0, 4_000.0, 8_000.0)),
            duration_seconds: None,
            cycles: 220,
            cycle_requests: Some(quantiles(220, 12.0, 40.0, 120.0)),
            cycle_minutes: Some(quantiles(220, 5.0, 25.0, 90.0)),
            thrash_cycles: 6,
            cycles_at_most_10: 30,
            cycles_at_most_30: 90,
            requests_to_first_compaction: None,
        },
        cost_mix: CostMixSummary {
            total_weighted_tokens: 1_200_000_000.0,
            total_usd: Some(456.78),
            input_share: 0.104,
            output_share: 0.114,
            cache_read_share: 0.612,
            cache_write_share: 0.17,
            models: vec![
                ModelUsageRow {
                    model: "gpt-5.5".to_owned(),
                    family: "gpt-5.5".to_owned(),
                    requests: 120_000,
                    weighted_cost_share: 0.8,
                    usd: Some(400.0),
                },
                ModelUsageRow {
                    model: "gpt-5.5-mini".to_owned(),
                    family: "gpt-5.5-mini".to_owned(),
                    requests: 30_000,
                    weighted_cost_share: 0.2,
                    usd: None,
                },
            ],
            reach: vec![
                ReachRow {
                    threshold_tokens: 100_000,
                    sessions_reaching: 900,
                    cost_share_above: 0.7,
                },
                ReachRow {
                    threshold_tokens: 200_000,
                    sessions_reaching: 300,
                    cost_share_above: 0.3,
                },
            ],
        },
        cold_cache: ColdCacheSummary {
            requests_considered: 150_000,
            cache_cold_share: 0.12,
            gap_cold_share: 0.08,
            avoidable_rewrite_cost_share: 0.05,
        },
        fixed_prefix: FixedPrefixSummary {
            first_request_tokens: Some(quantiles(1_200, 18_000.0, 22_000.0, 31_000.0)),
            post_compaction_request_tokens: Some(quantiles(330, 30_000.0, 41_000.0, 62_000.0)),
            rule_of_thumb: "Rule of thumb (first order): every ~10k tokens removed from the fixed prefix lowers the optimal trigger by about 10-15k tokens.".to_owned(),
        },
        semantics: SemanticsSummary {
            setting_key: "model_auto_compact_token_limit".to_owned(),
            window_tokens: 258_000,
            allowed_setting_range: (1_000, 232_000),
            cli_default_trigger_tokens: 232_000,
            cli_default_note: "90% of the 258k window".to_owned(),
            mapping_note: "model_auto_compact_token_limit is clamped to 90% of the 258,000-token window; realized trigger = limit x 1.00 (researched default, not re-measured on these logs).".to_owned(),
        },
        regimes: vec![
            RegimeRow {
                center_tokens: 224_000,
                min_tokens: 210_000,
                max_tokens: 232_000,
                count: 300,
                is_most_recent: false,
            },
            RegimeRow {
                center_tokens: 160_000,
                min_tokens: 150_000,
                max_tokens: 170_000,
                count: 40,
                is_most_recent: true,
            },
        ],
        recommendation: Some(card),
        simulation: simulation_rows(),
        bootstrap: Some(BootstrapRow {
            resamples: 500,
            argmin_p2_5: 130_000,
            argmin_median: 150_000,
            argmin_p97_5: 180_000,
            pick_within_band_share: 0.9,
        }),
        per_model: vec![
            PerModelRow {
                family: "gpt-5.5".to_owned(),
                sessions: 900,
                optimum_trigger_tokens: 160_000,
                optimum_setting_value: 160_000,
                band_low_tokens: 150_000,
                band_high_tokens: 180_000,
                interval_tokens: (140_000, 200_000),
            },
            PerModelRow {
                family: "gpt-5.5-mini".to_owned(),
                sessions: 300,
                optimum_trigger_tokens: 130_000,
                optimum_setting_value: 130_000,
                band_low_tokens: 120_000,
                band_high_tokens: 150_000,
                interval_tokens: (100_000, 180_000),
            },
        ],
        per_model_note: "The setting (model_auto_compact_token_limit) is one global value and cannot be applied per model. Apply uses the cost-weighted optimum of all models; the per-model optima only show the spread.".to_owned(),
        rework: vec![
            ReworkRow {
                multiplier: 0.5,
                optimum_trigger_tokens: 130_000,
                band_low_tokens: 120_000,
                band_high_tokens: 150_000,
                pick_overhead: 0.004,
            },
            ReworkRow {
                multiplier: 1.0,
                optimum_trigger_tokens: 150_000,
                band_low_tokens: 140_000,
                band_high_tokens: 160_000,
                pick_overhead: 0.0,
            },
            ReworkRow {
                multiplier: 2.0,
                optimum_trigger_tokens: 180_000,
                band_low_tokens: 160_000,
                band_high_tokens: 200_000,
                pick_overhead: 0.021,
            },
        ],
        rework_note: "Rework per compaction: 40k weighted tokens, a default measured on one research corpus, not on these logs. Rows show the optimum at 0.5x, 1x and 2x of it.".to_owned(),
        rework_model_text: "rework is a research prior (40000 weighted tokens), not measured on these logs".to_owned(),
        comparison: vec![
            comparison_row(
                ComparisonKind::CliDefault,
                "CLI default",
                232_000,
                14.4,
                PointQuality::Exact,
            ),
            comparison_row(
                ComparisonKind::CurrentSetting,
                "Current setting",
                200_000,
                12.9,
                PointQuality::Exact,
            ),
            comparison_row(
                ComparisonKind::Recommended,
                "Recommended",
                150_000,
                12.0,
                PointQuality::Interpolated,
            ),
        ],
        warnings: vec![
            "Dollar figures are API-price equivalents.".to_owned(),
            "No price for gpt-5.5-mini: relative research weights are used for them.".to_owned(),
        ],
    }
}

fn plain(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn body_lines(screen: &Screen<'_>, width: u16) -> Vec<String> {
    plain(&build_body(screen, width).lines)
}

fn panel_with(current: CurrentSetting) -> AgentPanel {
    AgentPanel {
        current,
        config_path: Some("/home/test/.codex/config.toml".into()),
        ..AgentPanel::default()
    }
}

fn screen<'a>(
    view: ScanView<'a>,
    report: Option<&'a CompactionReport>,
    panel: &'a AgentPanel,
) -> Screen<'a> {
    Screen {
        agent: AgentKind::Codex,
        view,
        report,
        panel,
    }
}

fn progress(phase: ScanPhase) -> ScanProgress {
    ScanProgress {
        phase,
        files_done: 1_204,
        files_total: 5_678,
        bytes_done: 12_400_000_000,
        bytes_total: 29_300_000_000,
        elapsed: Duration::from_secs(65),
        current_file_name: "rollout-2026-10-05T10-00-00-abcdef.jsonl".to_owned(),
    }
}

fn contains(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|line| line.contains(needle))
}

/// The lines joined with single spaces: for text that wraps differently at
/// different widths.
fn squeezed(lines: &[String]) -> String {
    lines
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_fits(lines: &[String], width: u16) {
    for line in lines {
        assert!(
            UnicodeWidthStr::width(line.as_str()) <= usize::from(width),
            "line wider than {width}: {line:?}"
        );
    }
}

/// Topics reachable on the idle, scanning and report pages (used by the
/// Settings help test: every OPT topic needs a visible source row).
pub(crate) fn reachable_topics() -> BTreeSet<String> {
    let report = fixture_report();
    let panel = AgentPanel {
        record: Some(ApplyRecord {
            target: AgentConfigTarget::CodexAutoCompactTokenLimit,
            path: "/home/test/.codex/config.toml".into(),
            previous_value: Some(200_000),
            written_value: 150_000,
            applied_at: "2026-10-05 12:00:00".to_owned(),
        }),
        ..panel_with(CurrentSetting::Value(200_000))
    };
    let mut topics = BTreeSet::new();
    topics.insert("OPT-01".to_owned());
    let progress = progress(ScanPhase::Reading);
    for screen in [
        screen(ScanView::Idle, None, &panel),
        screen(ScanView::Scanning(progress), None, &panel),
        screen(ScanView::Ready(&report), Some(&report), &panel),
    ] {
        for mark in build_body(&screen, 100).marks {
            topics.insert(mark.topic.to_owned());
        }
    }
    topics
}

// ------------------------------------------------------------ text helpers

#[test]
fn wrap_text_wraps_by_cells_and_splits_long_words() {
    assert_eq!(
        wrap_text("one two three four", 9),
        vec!["one two", "three", "four"]
    );
    let long = wrap_text("abcdefghijklmnopqrstuvwxyz", 10);
    assert_eq!(long, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
    assert!(wrap_text("", 10).is_empty());
}

#[test]
fn truncate_to_marks_a_cut_with_an_ellipsis() {
    assert_eq!(truncate_to("short", 10), "short");
    assert_eq!(truncate_to("abcdefghij", 6), "abcde…");
    assert_eq!(truncate_to("abc", 0), "");
}

#[test]
fn bars_fill_in_proportion_and_clamp() {
    assert_eq!(bar_parts(0.5, 10), ("█████".to_owned(), "░░░░░".to_owned()));
    assert_eq!(bar_parts(2.0, 4), ("████".to_owned(), String::new()));
    assert_eq!(bar_parts(f64::NAN, 4), (String::new(), "░░░░".to_owned()));
}

#[test]
fn table_columns_drop_least_important_first_and_keep_rank_zero() {
    let columns = [
        Column::left("A", 10, 0),
        Column::right("B", 10, 2),
        Column::right("C", 10, 1),
        Column::right("D", 10, 3),
    ];
    assert_eq!(kept_columns(&columns, 100), vec!["A", "B", "C", "D"]);
    assert_eq!(kept_columns(&columns, 36), vec!["A", "B", "C"]);
    assert_eq!(kept_columns(&columns, 25), vec!["A", "C"]);
    assert_eq!(kept_columns(&columns, 5), vec!["A"]);
}

// ----------------------------------------------------------- idle / scan

#[test]
fn the_idle_page_only_offers_a_scan_and_shows_the_current_setting() {
    let panel = panel_with(CurrentSetting::Value(200_000));
    let idle = screen(ScanView::Idle, None, &panel);
    let lines = body_lines(&idle, 80);
    assert!(contains(&lines, "COMPACTION OPTIMIZER - CODEX"));
    assert!(contains(&lines, "Scan sessions"));
    assert!(squeezed(&lines).contains(
        "Current model_auto_compact_token_limit: 200,000 (in /home/test/.codex/config.toml)"
    ));
    assert!(squeezed(&lines).contains("Nothing is scanned until you press the button"));
    let body = build_body(&idle, 80);
    assert_eq!(
        body.buttons
            .iter()
            .map(|button| button.action)
            .collect::<Vec<_>>(),
        vec![Action::Scan]
    );
    assert!(body.marks.iter().any(|mark| mark.topic == "OPT-02"));

    for (current, expected) in [
        (
            CurrentSetting::NotSet,
            "not set in /home/test/.codex/config.toml: the CLI default applies",
        ),
        (CurrentSetting::NoFile, "does not exist"),
        (
            CurrentSetting::Unreadable("bad toml".into()),
            "unreadable: bad toml",
        ),
        (CurrentSetting::Unknown, "not read yet"),
    ] {
        let panel = panel_with(current);
        let lines = body_lines(&screen(ScanView::Idle, None, &panel), 80);
        assert!(squeezed(&lines).contains(expected), "{expected}: {lines:?}");
    }
}

#[test]
fn the_progress_page_shows_phase_bar_counts_elapsed_file_and_cancel_at_every_width() {
    let panel = panel_with(CurrentSetting::NotSet);
    for width in [48_u16, 60, 80, 128] {
        let view = ScanView::Scanning(progress(ScanPhase::Reading));
        let lines = body_lines(&screen(view, None, &panel), width);
        assert!(contains(&lines, "SCANNING CODEX SESSIONS"), "{width}");
        assert!(contains(&lines, "Phase: Reading transcripts"), "{width}");
        assert!(contains(&lines, "42.3%"), "{width}: {lines:?}");
        assert!(contains(&lines, "Files: 1,204 of 5,678"), "{width}");
        assert!(contains(&lines, "Data: 12.4 GB of 29.3 GB"), "{width}");
        assert!(contains(&lines, "Elapsed: 1m 05s"), "{width}");
        assert!(
            squeezed(&lines).contains("Current file: rollout-2026-10-05"),
            "{width}"
        );
        assert!(contains(&lines, "[ Cancel ]"), "{width}");
        assert!(lines
            .iter()
            .any(|line| line.contains('█') && line.contains('░')));
        assert_fits(&lines, width);
    }
    let analyzing = ScanView::Scanning(progress(ScanPhase::Analyzing));
    assert!(contains(
        &body_lines(&screen(analyzing, None, &panel), 80),
        "Phase: Analyzing"
    ));
    let listing = ScanView::Listing { files_found: 812 };
    let lines = body_lines(&screen(listing, None, &panel), 80);
    assert!(contains(&lines, "Phase: Listing files"));
    assert!(contains(&lines, "Transcript files found: 812"));
    assert!(contains(&lines, "[ Cancel ]"));
}

#[test]
fn the_bar_follows_the_byte_fraction() {
    let panel = panel_with(CurrentSetting::NotSet);
    let mut early = progress(ScanPhase::Reading);
    early.bytes_done = 0;
    let lines = body_lines(&screen(ScanView::Scanning(early), None, &panel), 80);
    let bar = lines.iter().find(|line| line.contains('░')).unwrap();
    assert!(!bar.contains('█'), "{bar}");
    let mut done = progress(ScanPhase::Reading);
    done.bytes_done = done.bytes_total;
    let lines = body_lines(&screen(ScanView::Scanning(done), None, &panel), 80);
    let bar = lines.iter().find(|line| line.contains('█')).unwrap();
    assert!(!bar.contains('░'), "{bar}");
    assert!(bar.contains("100.0%"));
}

// ----------------------------------------------------------------- report

fn heading_line(lines: &[String], title: &str) -> usize {
    lines
        .iter()
        .position(|line| line.trim() == title)
        .unwrap_or_else(|| panic!("heading {title}: {lines:?}"))
}

#[test]
fn the_report_starts_with_the_recommendation_card_and_its_apply_button() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let body = build_body(&ready, 128);
    let lines = plain(&body.lines);
    assert_eq!(lines[0], "");
    assert_eq!(lines[1], "  RECOMMENDATION");
    assert_eq!(
        lines[2].trim_end(),
        "   Optimal: model_auto_compact_token_limit 150,000 (trigger ~150k, the simulated best, extrapolated) - Apply to Codex"
    );
    assert_eq!(lines[3], "  Confidence: medium");
    assert_eq!(
        lines[4],
        "  Extrapolated: extrapolated: 3.0% of observed compactions at or below this level"
    );
    assert!(contains(
        &lines,
        "Your current setting: costs 7.5% more than the recommendation"
    ));
    assert!(contains(&lines, "Pick rule: plain optimum, extrapolated"));
    let apply = body
        .buttons
        .iter()
        .find(|button| button.action == Action::Apply)
        .expect("an Apply button");
    assert_eq!((apply.first_line, apply.last_line), (2, 2));
    assert!(body
        .buttons
        .iter()
        .any(|button| button.action == Action::Scan));
    assert!(contains(&lines, "[ Re-scan ]"));
    assert!(!contains(&lines, "Revert"));
}

#[test]
fn the_card_explains_the_pick_rule_and_a_grid_limited_optimum() {
    let mut report = fixture_report();
    {
        let card = report.recommendation.as_mut().unwrap();
        card.pick_rule = PickRule::SupportedOptimum;
        card.pick_rule_text = "cheapest candidate with observed support; cost keeps falling below 120k (3.0% cheaper in simulation) but no compactions observed there".to_owned();
        card.unsupported_argmin_tokens = Some(120_000);
        card.unsupported_relative_saving = Some(0.03);
        card.grid_floor_limited = true;
    }
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    for width in [48_u16, 128] {
        let lines = body_lines(&ready, width);
        let text = squeezed(&lines);
        assert!(
            text.contains(
                "Pick rule: cheapest candidate with observed support; cost keeps falling below 120k"
            ),
            "{width}"
        );
        assert!(
            text.contains("still sits at the edge of the simulated range"),
            "{width}"
        );
        assert_fits(&lines, width);
    }
    report.recommendation.as_mut().unwrap().grid_floor_limited = false;
    report.recommendation.as_mut().unwrap().grid_widened = true;
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    assert!(squeezed(&body_lines(&ready, 128)).contains("widened once"));
}

#[test]
fn the_extrapolated_alternative_is_a_secondary_button_with_support_and_warning() {
    use crate::compaction_report::ExtrapolatedAlternative;
    let mut report = fixture_report();
    {
        let card = report.recommendation.as_mut().unwrap();
        card.extrapolated_alternative = Some(ExtrapolatedAlternative {
            trigger_tokens: 100_000,
            setting_key: "model_auto_compact_token_limit".to_owned(),
            setting_value: 100_000,
            setting_clamped: false,
            relative_saving: 0.397,
            observed_support_text:
                "extrapolated: 0.0% of observed compactions at or below this level".to_owned(),
            headline_text: "Simulated optimum".to_owned(),
        });
        card.grid_floor_limited = true;
    }
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    for width in [48_u16, 128] {
        let body = build_body(&ready, width);
        let lines = plain(&body.lines);
        let text = squeezed(&lines);
        assert!(
            text.contains("Apply simulated optimum (extrapolated): 100,000 - 39.7% cheaper"),
            "{width}"
        );
        assert!(
            text.contains("Extrapolated support: extrapolated: 0.0% of observed compactions"),
            "{width}"
        );
        assert!(
            text.contains("Warning: the simulation extrapolates beyond your observed compactions"),
            "{width}"
        );
        let primary = body
            .buttons
            .iter()
            .find(|b| b.action == Action::Apply)
            .unwrap();
        let secondary = body
            .buttons
            .iter()
            .find(|b| b.action == Action::ApplyExtrapolated)
            .unwrap();
        assert!(
            secondary.first_line > primary.last_line,
            "secondary comes after the primary"
        );
        assert_fits(&lines, width);
        // The grid note moved to the warnings area.
        let warnings = heading_line(&lines, "WARNINGS");
        assert!(
            squeezed(&lines[warnings..]).contains("still sits at the edge of the simulated range")
        );
    }
    let area = content();
    let button = *build_body(&ready, area.width)
        .buttons
        .iter()
        .find(|b| b.action == Action::ApplyExtrapolated)
        .unwrap();
    let position = Position::new(
        area.x + button.x_start,
        body_area(area).y + button.first_line,
    );
    assert_eq!(
        hit_in(&ready, area, 0, position),
        Some(Action::ApplyExtrapolated)
    );
    // Without the alternative there is no such button.
    let plain_report = fixture_report();
    let ready = screen(ScanView::Ready(&plain_report), Some(&plain_report), &panel);
    assert!(!build_body(&ready, 128)
        .buttons
        .iter()
        .any(|b| b.action == Action::ApplyExtrapolated));
}

#[test]
fn a_long_apply_button_wraps_into_one_clickable_block() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let body = build_body(&ready, 48);
    let apply = body
        .buttons
        .iter()
        .find(|button| button.action == Action::Apply)
        .unwrap();
    assert!(apply.last_line > apply.first_line, "wrapped over lines");
    let lines = plain(&body.lines);
    let block: Vec<&String> = lines[usize::from(apply.first_line)..=usize::from(apply.last_line)]
        .iter()
        .collect();
    let widths: BTreeSet<usize> = block
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .collect();
    assert_eq!(widths.len(), 1, "padded to one width: {block:?}");
    assert!(block.last().unwrap().contains("Apply to Codex"));
    assert!(block.first().unwrap().contains("Optimal:"));
}

#[test]
fn every_section_is_present_in_order() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    for width in [48_u16, 80, 128, 200] {
        let lines = body_lines(&ready, width);
        let order = [
            "RECOMMENDATION",
            "CLI DEFAULT | CURRENT | RECOMMENDED",
            "CORPUS AND COMPACTIONS",
            "COST MIX AND COLD CACHE",
            "FIXED PREFIX (C0)",
            "SIMULATION BY TRIGGER",
            "PER-MODEL OPTIMA",
            "REWORK SENSITIVITY",
            "COMPACTION REGIMES AND MAPPING",
            "WARNINGS",
        ];
        let positions: Vec<usize> = order
            .iter()
            .map(|title| heading_line(&lines, title))
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{width}: {positions:?}"
        );
        assert_fits(&lines, width);
    }
}

#[test]
fn the_comparison_shows_default_current_and_recommended() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let words =
        |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_owned).collect() };
    let wide = body_lines(&ready, 128);
    let start = heading_line(&wide, "CLI DEFAULT | CURRENT | RECOMMENDED");
    assert_eq!(
        words(&wide[start + 1]),
        [
            "Setting",
            "Trigger",
            "vs",
            "default",
            "Cost",
            "USD",
            "Compactions"
        ]
    );
    assert_eq!(
        words(&wide[start + 2]),
        ["CLI", "default", "232,000", "232k", "0.0%", "14.4M", "$57.60", "60.0"]
    );
    assert_eq!(
        words(&wide[start + 3]),
        ["Current", "200,000", "200k", "-10.4%", "12.9M", "$51.60", "60.0"]
    );
    assert_eq!(
        words(&wide[start + 4]),
        [
            "Recommended",
            "150,000",
            "150k",
            "-16.7%",
            "12M",
            "$48.00",
            "60.0"
        ]
    );
    assert!(contains(
        &wide,
        "Recommended: interpolated between two simulated points."
    ));

    // 48 columns keep the name, the setting, the trigger and vs default.
    let narrow = body_lines(&ready, 48);
    let start = heading_line(&narrow, "CLI DEFAULT | CURRENT | RECOMMENDED");
    assert_eq!(
        words(&narrow[start + 1]),
        ["Setting", "Trigger", "vs", "default"]
    );
    assert_eq!(
        words(&narrow[start + 4]),
        ["Recommended", "150,000", "150k", "-16.7%"]
    );
    assert_fits(&narrow, 48);
}

#[test]
fn a_missing_current_setting_is_shown_as_not_set_in_the_comparison() {
    let mut report = fixture_report();
    report
        .comparison
        .retain(|row| row.kind != ComparisonKind::CurrentSetting);
    let panel = panel_with(CurrentSetting::NotSet);
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let lines = body_lines(&ready, 128);
    let start = heading_line(&lines, "CLI DEFAULT | CURRENT | RECOMMENDED");
    assert!(
        lines[start + 3].contains("Current"),
        "{:?}",
        lines[start + 3]
    );
    assert!(lines[start + 3].contains("not set"));
    report
        .recommendation
        .as_mut()
        .unwrap()
        .current_vs_recommended = None;
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    assert!(contains(
        &body_lines(&ready, 128),
        "Your current setting: is not set: the CLI default (trigger ~232k) applies"
    ));
}

#[test]
fn statistics_cost_mix_fixed_prefix_regimes_and_warnings_have_their_figures() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let lines = body_lines(&ready, 128);
    let text = squeezed(&lines);
    for expected in [
        "Transcripts: 5,678 files (29.3 GB) over 212.0 days: 1,200 main sessions, 0 subagent sessions",
        "Compactions: 340 in 120 sessions (300 auto, 40 manual; 330 measured)",
        "Fired at: p10 190k | p50 224k | p90 232k (n=340)",
        "First request after: p10 30k | p50 41k | p90 62k (n=330)",
        "Cycles: 220: 6 of at most 3 requests (refill thrash), 30 of at most 10, 90 of at most 30",
        "cache read",
        "61.2%",
        "Total: 1.2G weighted input-token equivalents, $456.78",
        "Cold-cache requests: 12.0% of 150,000 requests (8.0% after an idle gap); rewriting prefixes costs 5.0% of the total",
        "First request of a session: p10 18k | p50 22k | p90 31k (n=1,200)",
        "every ~10k tokens removed from the fixed prefix lowers the optimal trigger by about 10-15k tokens",
        "300 compactions around 224k (210k-232k)",
        "40 compactions around 160k (150k-170k), most recent regime",
        "- Dollar figures are API-price equivalents.",
        "- No price for gpt-5.5-mini",
        "Bootstrap: 500 resamples",
    ] {
        assert!(text.contains(expected), "missing {expected:?}");
    }
}

#[test]
fn the_per_model_table_carries_the_visible_cannot_apply_per_model_message() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    for width in [48_u16, 128] {
        let lines = body_lines(
            &screen(ScanView::Ready(&report), Some(&report), &panel),
            width,
        );
        let joined = lines.join(" ");
        let squeezed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            squeezed.contains("is one global value and cannot be applied per model"),
            "{width}"
        );
        assert!(contains(&lines, "gpt-5.5-mini"));
    }
}

#[test]
fn the_simulation_table_marks_pick_best_default_and_current() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);

    let wide = body_lines(&ready, 128);
    let start = heading_line(&wide, "SIMULATION BY TRIGGER");
    assert!(wide[start + 1].contains("Trigger") && wide[start + 1].contains("Marks"));
    let pick = wide.iter().find(|line| line.contains("pick best")).unwrap();
    assert!(pick.trim_start().starts_with("150k"), "{pick}");
    assert!(pick.contains("0.0%") && pick.contains("2%"));
    assert!(wide.iter().any(|line| line.trim_end().ends_with("current")));
    assert!(wide.iter().any(|line| line.trim_end().ends_with("default")));

    let narrow = body_lines(&ready, 48);
    let start = heading_line(&narrow, "SIMULATION BY TRIGGER");
    assert!(!narrow[start + 1].contains("Marks Support"));
    let pick = narrow
        .iter()
        .skip(start)
        .find(|line| line.trim_end().ends_with("P B"))
        .expect("letter markers at narrow width");
    assert!(pick.contains("150,000"));
    assert!(contains(&narrow, "P pick (recommendation)"));
}

#[test]
fn the_simulation_chart_plots_the_cost_curve_with_markers() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    for width in [48_u16, 128] {
        let lines = body_lines(&ready, width);
        assert!(
            contains(&lines, "Extra cost over the best trigger"),
            "{width}"
        );
        let axis_rows = lines
            .iter()
            .filter(|line| line.contains('┤') || line.contains('┼'))
            .count();
        assert!(axis_rows >= 5, "{width}: chart rows {axis_rows}");
        let chart_start = lines
            .iter()
            .position(|line| line.contains("Extra cost over the best trigger"))
            .unwrap();
        let marker_row = lines[chart_start..]
            .iter()
            .find(|line| {
                !line.trim().is_empty()
                    && line
                        .trim()
                        .chars()
                        .all(|c| matches!(c, 'P' | 'B' | 'C' | 'D' | ' '))
            })
            .unwrap_or_else(|| panic!("{width}: marker row"));
        // The pick sits on the best candidate, so P covers B.
        let marks: String = marker_row.chars().filter(|c| *c != ' ').collect();
        assert_eq!(marks.len(), 3, "{width}: {marker_row:?}");
        for mark in ['P', 'C', 'D'] {
            assert!(marks.contains(mark), "{width}: {marker_row:?}");
        }
        assert!(
            marker_row.find('P').unwrap() < marker_row.find('C').unwrap()
                && marker_row.find('C').unwrap() < marker_row.find('D').unwrap(),
            "{width}: markers follow the trigger order: {marker_row:?}"
        );
        let labels = lines
            .iter()
            .find(|line| line.contains("100k") && line.contains("232k"));
        assert!(labels.is_some(), "{width}: x axis labels");
        assert_fits(&lines, width);
    }
}

#[test]
fn rework_sensitivity_lists_the_three_multipliers() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let lines = body_lines(
        &screen(ScanView::Ready(&report), Some(&report), &panel),
        128,
    );
    let start = heading_line(&lines, "REWORK SENSITIVITY");
    assert!(squeezed(&lines[start + 1..start + 4])
        .contains("a default measured on one research corpus"));
    let header = lines
        .iter()
        .position(|line| line.contains("Rework") && line.contains("Optimum"))
        .expect("table header");
    assert!(header > start);
    let rows = &lines[header..header + 4];
    assert!(rows[0].contains("Rework") && rows[0].contains("Optimum"));
    assert!(rows[1].trim_start().starts_with("0.5x") && rows[1].contains("130k"));
    assert!(rows[2].trim_start().starts_with("1x") && rows[2].contains("150k"));
    assert!(rows[3].trim_start().starts_with("2x") && rows[3].contains("180k"));
}

#[test]
fn a_failed_or_cancelled_scan_keeps_the_previous_report_with_a_note() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let failed = screen(ScanView::Failed("worker failed"), Some(&report), &panel);
    let lines = body_lines(&failed, 80);
    assert_eq!(
        lines[1],
        "  The last scan failed: worker failed. The report below is from the previous"
    );
    assert!(contains(&lines, "scan."));
    assert!(contains(&lines, "RECOMMENDATION"));
    assert!(build_body(&failed, 80)
        .buttons
        .iter()
        .any(|button| button.action == Action::Apply));

    let cancelled = screen(ScanView::Cancelled, Some(&report), &panel);
    let lines = body_lines(&cancelled, 128);
    assert!(contains(
        &lines,
        "The last scan was cancelled. The report below is from the previous scan."
    ));
    assert!(contains(&lines, "RECOMMENDATION"));

    // Without an earlier report the idle page carries the note.
    let lines = body_lines(&screen(ScanView::Cancelled, None, &panel), 80);
    assert!(contains(&lines, "The last scan was cancelled."));
    assert!(contains(&lines, "Scan sessions"));
    let lines = body_lines(&screen(ScanView::Failed("no admission"), None, &panel), 80);
    assert!(contains(&lines, "The last scan failed: no admission."));
}

#[test]
fn a_report_without_a_recommendation_offers_rescan_but_no_apply() {
    let mut report = fixture_report();
    report.recommendation = None;
    report.comparison.clear();
    report.simulation.clear();
    report.status = ReportStatus::NothingToReplay;
    report.status_message = Some("No session grew beyond 100k tokens of context.".to_owned());
    let panel = panel_with(CurrentSetting::NotSet);
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let body = build_body(&ready, 80);
    let lines = plain(&body.lines);
    assert!(contains(
        &lines,
        "No session grew beyond 100k tokens of context."
    ));
    assert!(!body
        .buttons
        .iter()
        .any(|button| button.action == Action::Apply));
    assert!(body
        .buttons
        .iter()
        .any(|button| button.action == Action::Scan));
    assert!(!contains(&lines, "SIMULATION BY TRIGGER"));
}

#[test]
fn notes_and_the_revert_button_follow_the_panel() {
    let report = fixture_report();
    let mut panel = panel_with(CurrentSetting::Value(150_000));
    panel.note = Some(Note {
        tone: NoteTone::Success,
        text: "Applied model_auto_compact_token_limit = 150,000.".to_owned(),
    });
    panel.record = Some(ApplyRecord {
        target: AgentConfigTarget::CodexAutoCompactTokenLimit,
        path: "/home/test/.codex/config.toml".into(),
        previous_value: None,
        written_value: 150_000,
        applied_at: "2026-10-05 12:00:00".to_owned(),
    });
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let body = build_body(&ready, 128);
    let lines = plain(&body.lines);
    assert!(contains(
        &lines,
        "Applied model_auto_compact_token_limit = 150,000."
    ));
    assert!(contains(
        &lines,
        "[ Revert model_auto_compact_token_limit to remove the key (applied 2026-10-05 12:00:00) ]"
    ));
    assert!(body
        .buttons
        .iter()
        .any(|button| button.action == Action::Revert));
    // Too narrow for the long label: the short one is used and still fits.
    let narrow = build_body(&ready, 48);
    let narrow_lines = plain(&narrow.lines);
    assert!(contains(&narrow_lines, "[ Revert to remove the key ]"));
    assert_fits(&narrow_lines, 48);
    // The idle page shows it too.
    let idle = build_body(&screen(ScanView::Idle, None, &panel), 80);
    assert!(idle
        .buttons
        .iter()
        .any(|button| button.action == Action::Revert));
}

// ---------------------------------------------------------- hit testing

fn content() -> Rect {
    Rect::new(30, 3, 110, 30)
}

#[test]
fn header_buttons_select_the_agent_and_the_selected_one_is_filled() {
    let header = header(AgentKind::ClaudeCode, 110);
    let lines = plain(&header.lines);
    assert!(lines[0].contains(" Codex ") && lines[0].contains(" Claude Code "));
    assert!(lines[1].contains("←/→ agent"));
    assert_eq!(
        header
            .buttons
            .iter()
            .map(|button| button.action)
            .collect::<Vec<_>>(),
        vec![
            Action::SelectAgent(AgentKind::Codex),
            Action::SelectAgent(AgentKind::ClaudeCode)
        ]
    );
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::NotSet);
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let area = content();
    for button in &header.buttons {
        let position = Position::new(area.x + button.x_start, area.y);
        assert_eq!(hit_in(&ready, area, 0, position), Some(button.action));
        let after = Position::new(area.x + button.x_end, area.y);
        assert_ne!(hit_in(&ready, area, 0, after), Some(button.action));
    }
}

#[test]
fn body_clicks_follow_the_scroll_offset_and_ignore_text() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let area = content();
    let body = build_body(&ready, area.width);
    let apply = *body
        .buttons
        .iter()
        .find(|button| button.action == Action::Apply)
        .unwrap();
    let top = body_area(area).y;
    let at = |scroll: u16, line: u16, x: u16| {
        let row = top + line.saturating_sub(scroll);
        hit_in(&ready, area, scroll, Position::new(area.x + x, row))
    };
    assert_eq!(at(0, apply.first_line, apply.x_start), Some(Action::Apply));
    assert_eq!(
        at(0, apply.first_line, apply.x_end - 1),
        Some(Action::Apply)
    );
    assert_eq!(at(0, apply.first_line, apply.x_end), None);
    assert_eq!(at(1, apply.first_line, apply.x_start), Some(Action::Apply));
    // The Apply row is not where it used to be after scrolling by two.
    assert_ne!(
        hit_in(
            &ready,
            area,
            2,
            Position::new(area.x + apply.x_start, top + apply.first_line)
        ),
        Some(Action::Apply)
    );
    // A heading is plain text.
    assert_eq!(at(0, 1, 3), None);
    // Outside the content area.
    assert_eq!(
        hit_in(&ready, area, 0, Position::new(area.x - 1, top + 2)),
        None
    );
    assert_eq!(
        hit_in(&ready, area, 0, Position::new(area.x + 2, area.bottom())),
        None
    );
}

#[test]
fn scan_buttons_are_reachable_by_click_on_the_idle_and_progress_pages() {
    let panel = panel_with(CurrentSetting::NotSet);
    let area = content();
    let idle = screen(ScanView::Idle, None, &panel);
    let scan = *build_body(&idle, area.width)
        .buttons
        .iter()
        .find(|button| button.action == Action::Scan)
        .unwrap();
    let position = Position::new(
        area.x + scan.x_start + 1,
        body_area(area).y + scan.first_line,
    );
    assert_eq!(hit_in(&idle, area, 0, position), Some(Action::Scan));

    let running = screen(
        ScanView::Scanning(progress(ScanPhase::Reading)),
        None,
        &panel,
    );
    let cancel = *build_body(&running, area.width)
        .buttons
        .iter()
        .find(|button| button.action == Action::Cancel)
        .unwrap();
    let position = Position::new(
        area.x + cancel.x_start + 2,
        body_area(area).y + cancel.first_line,
    );
    assert_eq!(hit_in(&running, area, 0, position), Some(Action::Cancel));
}

#[test]
fn scroll_bounds_and_help_anchors_follow_the_page() {
    let report = fixture_report();
    let panel = panel_with(CurrentSetting::Value(200_000));
    let ready = screen(ScanView::Ready(&report), Some(&report), &panel);
    let area = Rect::new(0, 0, 100, 24);
    let total = build_body(&ready, area.width).lines.len() as u16;
    assert_eq!(max_scroll_in(&ready, area), total - body_area(area).height);
    assert_eq!(page_height(area), body_area(area).height - 1);
    assert_eq!(body_area(area).height, 24 - HEADER_ROWS);

    let top = help_anchors_in(&ready, area, 0);
    assert_eq!(top[0], ("OPT-01", 0));
    assert!(top
        .iter()
        .any(|(topic, row)| *topic == "OPT-04" && *row == HEADER_ROWS + 1));
    assert!(top.iter().all(|(_, row)| *row < area.height));
    let scrolled = help_anchors_in(&ready, area, max_scroll_in(&ready, area));
    assert!(scrolled.iter().any(|(topic, _)| *topic == "OPT-14"));
    assert!(!scrolled.iter().any(|(topic, _)| *topic == "OPT-04"));
    assert!(scrolled.iter().all(|(_, row)| *row < area.height));

    // A page shorter than the area never scrolls.
    let panel = panel_with(CurrentSetting::NotSet);
    let idle = screen(ScanView::Idle, None, &panel);
    assert_eq!(max_scroll_in(&idle, Rect::new(0, 0, 100, 60)), 0);
}

#[test]
fn every_help_topic_of_the_tab_exists_and_the_set_is_complete() {
    let topics = reachable_topics();
    let expected: BTreeSet<String> = (1..=14).map(|index| format!("OPT-{index:02}")).collect();
    assert_eq!(topics, expected);
    for topic in &topics {
        assert!(
            crate::settings_help::catalog::by_id(topic).is_some(),
            "{topic} is in the catalog"
        );
    }
}

// ----------------------------------------------------------------- render

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_tab_renders_in_the_settings_screen_at_80_and_160_columns() {
    use crate::app::{App, Mode, SettingsState, SettingsTab};
    for (width, height) in [(80_u16, 30_u16), (160, 40)] {
        let mut app = App::new("optimization-render".to_owned(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, width, height));
        let state = SettingsState {
            tab: SettingsTab::Optimization,
            ..SettingsState::default()
        };
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Optimization,
            ..SettingsState::default()
        });
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
            .unwrap();
        let text = rendered(&terminal);
        assert!(text.contains("Optimization"), "{width}: tab list entry");
        assert!(text.contains(" Codex "), "{width}");
        assert!(text.contains(" Claude Code "), "{width}");
        assert!(text.contains("Scan sessions"), "{width}");
        assert!(text.contains("COMPACTION OPTIMIZER - CODEX"), "{width}");
    }
}
