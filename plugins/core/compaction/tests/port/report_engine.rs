//! Twins of ilium-client/src/compaction_report/tests.rs.
//! The plugin exposes the report builder through the gap method
//! `FakeHost::build_report(spec) -> Result<Value, PluginError>`; the JSON keeps the
//! original struct field names (see ../API_GAPS_port.md).

use serde_json::{json, Value};

use crate::common::*;

fn build(spec: Value) -> Value {
    let mut host = server_host();
    host.build_report(spec).expect("the report builds")
}

fn spec(agent: &str, sessions: usize) -> Value {
    json!({"agent": agent, "sessions": sessions})
}

fn with(mut base: Value, key: &str, value: Value) -> Value {
    base[key] = value;
    base
}

fn process(agent: &str, sessions: usize, process: Value) -> Value {
    with(spec(agent, sessions), "process", process)
}

fn card(report: &Value) -> &Value {
    assert!(
        report["recommendation"].is_object(),
        "a recommendation: {report}"
    );
    &report["recommendation"]
}

fn triggers(report: &Value) -> Vec<u64> {
    report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["trigger_tokens"].as_u64().unwrap())
        .collect()
}

fn warnings(report: &Value) -> Vec<String> {
    report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect()
}

fn has_warning(report: &Value, needle: &str) -> bool {
    warnings(report).iter().any(|w| w.contains(needle))
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_claude_corpus_yields_a_complete_report
#[test]
fn report_a_claude_corpus_yields_a_complete_report() {
    let report = build(with(
        spec("claude", 8),
        "scan",
        json!({"files_listed": 8, "files_parsed": 8, "bytes_total": 4_000_000}),
    ));
    assert_eq!(report["status"], "Complete");
    assert!(report["status_message"].is_null());
    assert_eq!(report["corpus"]["main_sessions"], 8);
    assert_eq!(report["corpus"]["files_listed"], 8);
    assert!(report["compactions"]["total"].as_u64().unwrap() >= 8);
    assert_eq!(
        report["compactions"]["auto"],
        report["compactions"]["total"]
    );
    assert!(report["compactions"]["pre_tokens"].is_object());
    assert!(
        report["fixed_prefix"]["first_request_tokens"].is_object()
            || report["fixed_prefix"]["first_request_tokens"].is_number()
    );
    assert!(text(&report["fixed_prefix"]["rule_of_thumb"]).contains("10-15k"));
    assert!(report["cost_mix"]["cache_read_share"].as_f64().unwrap() > 0.0);
    assert!(report["regimes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["is_most_recent"] == true));
    let t = triggers(&report);
    assert!(t.windows(2).all(|p| p[0] < p[1]));
    assert!(t.contains(&217_000) && t.contains(&120_000) && t.contains(&967_000));
    let rows = report["simulation"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|r| r["is_best"] == true).count(), 1);
    assert!(rows
        .iter()
        .any(|r| r["is_cli_default"] == true && r["trigger_tokens"] == 967_000));
    let best = rows.iter().find(|r| r["is_best"] == true).unwrap();
    assert!(best["within_2_percent"] == true && best["within_5_percent"] == true);
    let pick = card(&report);
    assert_eq!(pick["setting_key"], "autoCompactWindow");
    assert_eq!(
        pick["setting_value"].as_u64(),
        Some(pick["trigger_tokens"].as_u64().unwrap() + 33_000)
    );
    assert!(text(&pick["headline_text"]).starts_with("Optimal: autoCompactWindow "));
    assert!(rows
        .iter()
        .any(|r| r["is_pick"] == true && r["trigger_tokens"] == pick["trigger_tokens"]));
    assert_eq!(
        pick["extrapolated"] == true,
        pick["observed_support_fraction"].as_f64().unwrap() < 0.01
    );
    assert!(!report["per_model"].as_array().unwrap().is_empty());
    assert!(text(&report["per_model_note"]).contains("cannot be applied per model"));
    let multipliers: Vec<f64> = report["rework"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["multiplier"].as_f64().unwrap())
        .collect();
    assert_eq!(multipliers, vec![0.5, 1.0, 2.0]);
    assert!(text(&report["rework_note"]).contains("research corpus"));
    assert!(report["bootstrap"]["resamples"].as_u64().unwrap() >= 200);
    let comparison = report["comparison"].as_array().unwrap();
    assert_eq!(comparison.len(), 2);
    assert_eq!(comparison[0]["kind"], "CliDefault");
    assert_eq!(comparison[0]["trigger_tokens"], 967_000);
    assert_eq!(comparison[1]["kind"], "Recommended");
    assert!(pick["current_vs_recommended"].is_null());
    assert!(
        has_warning(&report, "API-price equivalents") && has_warning(&report, "rework penalty")
    );
    let mut unique = warnings(&report);
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), warnings(&report).len());
    let plain = text(&report["plain_text"]);
    for needle in [
        "== Recommendation ==",
        "== Corpus ==",
        "== Compactions ==",
        "== Cost mix ==",
        "== Simulation ==",
        "== Warnings ==",
        "cannot be applied per model",
    ] {
        assert!(plain.contains(needle), "missing {needle}");
    }
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::the_current_setting_becomes_a_third_comparison_row
#[test]
fn report_the_current_setting_becomes_a_third_comparison_row() {
    let report = build(with(
        spec("claude", 8),
        "settings",
        json!({"current_setting_value": 283_000}),
    ));
    let kinds: Vec<&str> = report["comparison"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["CliDefault", "CurrentSetting", "Recommended"]);
    assert_eq!(report["comparison"][1]["trigger_tokens"], 250_000);
    assert_eq!(report["comparison"][1]["setting_value"], 283_000);
    assert!(report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["is_current"] == true));
    assert!(
        card(&report)["current_vs_recommended"].is_object()
            || card(&report)["current_vs_recommended"].is_number()
    );
    assert!(
        report["comparison"][2]["relative_to_recommended"]
            .as_f64()
            .unwrap()
            .abs()
            < 1e-9
    );
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_codex_corpus_uses_the_codex_semantics
#[test]
fn report_a_codex_corpus_uses_the_codex_semantics() {
    let report = build(spec("codex", 8));
    assert_eq!(report["status"], "Complete");
    assert_eq!(
        report["semantics"]["setting_key"],
        "model_auto_compact_token_limit"
    );
    assert_eq!(report["semantics"]["allowed_setting_range"][1], 232_560);
    assert_eq!(report["semantics"]["cli_default_trigger_tokens"], 232_560);
    assert!(text(&report["semantics"]["mapping_note"]).contains("clamped to 90%"));
    assert_eq!(report["compactions"]["auto"], 0);
    assert_eq!(
        report["compactions"]["unknown_trigger"],
        report["compactions"]["total"]
    );
    assert!(text(&report["plain_text"]).contains("of unstated trigger"));
    assert!(card(&report)["setting_value"].as_u64().unwrap() <= 232_560);
    assert!(has_warning(&report, "cached input at 10%"));
    let t = triggers(&report);
    assert!(t.contains(&100_000) && t.contains(&232_560));
    assert!(t.iter().all(|&x| x <= 232_560));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::an_empty_corpus_is_a_clear_no_sessions_result
#[test]
fn report_an_empty_corpus_is_a_clear_no_sessions_result() {
    for agent in ["claude", "codex"] {
        let report = build(spec(agent, 0));
        assert_eq!(report["status"], "NoSessionsFound");
        assert!(report["recommendation"].is_null());
        assert!(report["simulation"].as_array().unwrap().is_empty());
        assert!(report["comparison"].as_array().unwrap().is_empty());
        assert!(text(&report["status_message"]).starts_with("No sessions found"));
        assert!(text(&report["plain_text"]).contains("No sessions found"));
        assert!(
            !warnings(&report).is_empty(),
            "the price caveat still shows"
        );
    }
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::small_sessions_report_statistics_but_nothing_to_replay
#[test]
fn report_small_sessions_report_statistics_but_nothing_to_replay() {
    let report = build(process(
        "claude",
        3,
        json!({"turns": 20, "trigger": 10_000_000}),
    ));
    assert_eq!(report["status"], "NothingToReplay");
    assert_eq!(report["corpus"]["main_sessions"], 3);
    assert!(text(&report["status_message"]).contains("No session grew beyond 120k"));
    assert!(report["recommendation"].is_null());
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::scan_warnings_truncation_and_subagents_reach_the_report
#[test]
fn report_scan_warnings_truncation_and_subagents_reach_the_report() {
    let report = build(json!({
        "agent": "claude", "sessions": 6, "subagent_sessions": 1,
        "scan": {"sessions_dropped_for_cap": 12, "warnings": ["Skipped /synthetic/a.jsonl: gone"]}
    }));
    assert_eq!(report["corpus"]["subagent_sessions"], 1);
    assert!(report["corpus"]["subagent_request_share"].as_f64().unwrap() > 0.0);
    assert_eq!(report["corpus"]["sessions_dropped_for_cap"], 12);
    assert!(has_warning(&report, "larger than the retained-result cap"));
    assert!(has_warning(&report, "a.jsonl"));
    assert!(has_warning(
        &report,
        "subagent sessions are reported separately"
    ));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::unpriced_models_are_named_in_a_warning
#[test]
fn report_unpriced_models_are_named_in_a_warning() {
    let report = build(with(
        spec("codex", 6),
        "model_override",
        json!("gpt-mystery-model"),
    ));
    assert!(has_warning(&report, "No price for gpt-mystery-model"));
    assert!(report["cost_mix"]["total_usd"].is_null());
    assert!(report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["cost_usd"].is_null()));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::priced_models_produce_dollar_figures
#[test]
fn report_priced_models_produce_dollar_figures() {
    let report = build(spec("claude", 6));
    assert!(report["cost_mix"]["total_usd"].is_number());
    assert!(report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["cost_usd"].is_number()));
    assert!(!has_warning(&report, "No price for"));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::price_weights_map_the_table_rates
#[test]
fn report_price_weights_map_the_table_rates() {
    let host = server_host();
    let weights = host
        .price_weights("claude-sonnet-5-5")
        .expect("a priced model");
    assert_eq!(weights["input"], 2.0);
    assert_eq!(weights["output"], 10.0);
    assert_eq!(weights["cache_read"], 0.20);
    assert_eq!(weights["cache_write_5m"], 2.5);
    assert_eq!(weights["cache_write_1h"], 4.0);
    assert!(host.price_weights("unknown-model").is_none());
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::grids_cover_the_documented_ranges
#[test]
fn report_grids_cover_the_documented_ranges() {
    let host = server_host();
    let claude = host.default_grid("claude");
    assert_eq!(claude.first(), Some(&120_000));
    assert_eq!(claude.last(), Some(&600_000));
    assert!(claude.contains(&567_000) && claude.contains(&217_000) && claude.contains(&125_000));
    let codex = host.default_grid("codex");
    assert_eq!(codex.first(), Some(&100_000));
    assert_eq!(codex.last(), Some(&232_560));
    assert!(codex.contains(&225_000));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::stated_codex_windows_set_the_cap
#[test]
fn report_stated_codex_windows_set_the_cap() {
    let report = build(with(
        spec("codex", 3),
        "context_window_tokens",
        json!(272_000),
    ));
    assert_eq!(report["semantics"]["allowed_setting_range"][1], 244_800);
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::formatting_helpers_are_compact_and_stable
#[test]
fn report_formatting_helpers_are_compact_and_stable() {
    let host = server_host();
    let cases: [(&str, f64, &str); 21] = [
        ("humanize_tokens", 0.0, "0"),
        ("humanize_tokens", 950.0, "950"),
        ("humanize_tokens", 217_000.0, "217k"),
        ("humanize_tokens", 217_500.0, "217.5k"),
        ("humanize_tokens", 999_700.0, "1M"),
        ("humanize_tokens", 1_250_000.0, "1.25M"),
        ("group_thousands", 0.0, "0"),
        ("group_thousands", 250_000.0, "250,000"),
        ("group_thousands", 1_234_567.0, "1,234,567"),
        ("format_usd", 0.4, "$0.40"),
        ("format_usd", 1_234.6, "$1,235"),
        ("format_percent", 0.123, "12.3%"),
        ("format_percent", 0.0, "0.0%"),
        ("format_percent", 0.00001, "<0.1%"),
        ("format_signed_percent", 0.032, "+3.2%"),
        ("format_signed_percent", -0.004, "-0.4%"),
        ("format_signed_percent", 0.0, "0.0%"),
        ("format_bytes", 812.0, "812 B"),
        ("format_bytes", 3_400_000.0, "3.4 MB"),
        ("format_elapsed", 5.0, "5s"),
        ("format_elapsed", 185.0, "3m 05s"),
    ];
    for (helper, input, expected) in cases {
        assert_eq!(
            host.format_value(helper, input),
            expected,
            "{helper}({input})"
        );
    }
    assert_eq!(host.format_value("humanize_tokens", f64::NAN), "0");
    assert_eq!(host.format_value("format_elapsed", 3_725.0), "1h 02m");
    assert_eq!(
        host.format_token_quantiles(12, 120_000.0, 217_000.0, 567_000.0),
        "p10 120k | p50 217k | p90 567k (n=12)"
    );
}

fn measured(report: &Value) -> bool {
    text(&report["semantics"]["mapping_note"]).contains("(measured on these logs)")
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_plausible_current_setting_re_measures_the_claude_offset
#[test]
fn report_a_plausible_current_setting_re_measures_the_claude_offset() {
    let report = build(with(
        spec("claude", 8),
        "settings",
        json!({"current_setting_value": 183_000}),
    ));
    assert!(measured(&report));
    let pick = card(&report);
    let offset = pick["setting_value"].as_i64().unwrap() - pick["trigger_tokens"].as_i64().unwrap();
    assert!((29_000..=33_000).contains(&offset), "{offset}");
    assert!(!has_warning(&report, "uses the researched default"));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::an_implausible_current_setting_keeps_the_default_mapping
#[test]
fn report_an_implausible_current_setting_keeps_the_default_mapping() {
    let report = build(with(
        spec("claude", 8),
        "settings",
        json!({"current_setting_value": 900_000}),
    ));
    assert!(!measured(&report));
    let pick = card(&report);
    assert_eq!(
        pick["setting_value"].as_u64(),
        Some(pick["trigger_tokens"].as_u64().unwrap() + 33_000)
    );
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_plausible_codex_limit_re_measures_the_realized_ratio
#[test]
fn report_a_plausible_codex_limit_re_measures_the_realized_ratio() {
    let report = build(with(
        spec("codex", 8),
        "settings",
        json!({"current_setting_value": 150_000}),
    ));
    assert!(measured(&report));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::plain_argmin_is_extrapolated_when_no_candidate_has_support
#[test]
fn report_plain_argmin_is_extrapolated_when_no_candidate_has_support() {
    let report = build(process(
        "codex",
        8,
        json!({"trigger": 240_000, "turns": 200}),
    ));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "PlainArgmin");
    assert_eq!(pick["extrapolated"], true);
    assert_eq!(pick["pick_rule_text"], "plain optimum, extrapolated");
    assert!(text(&pick["headline_text"]).contains("extrapolated"));
    assert!(text(&pick["observed_support_text"]).starts_with("extrapolated: "));
    assert!(
        pick["unsupported_argmin_tokens"].is_null()
            && pick["unsupported_relative_saving"].is_null()
    );
    assert!(report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["observed_support"] == 0.0));
    assert!(text(&report["plain_text"]).contains("Pick rule: plain optimum, extrapolated"));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::compactions_inside_the_two_percent_band_give_the_band_rule
#[test]
fn report_compactions_inside_the_two_percent_band_give_the_band_rule() {
    let report = build(process("codex", 8, json!({"trigger": 150_000})));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "BandWithSupport");
    assert_eq!(pick["extrapolated"], false);
    assert_eq!(
        pick["pick_rule_text"],
        "lowest candidate within 2% of best with observed support"
    );
    assert!(pick["unsupported_argmin_tokens"].is_null());
    assert!(pick["observed_support_fraction"].as_f64().unwrap() >= 0.01);
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_supported_optimum_reports_the_cheaper_unsupported_argmin
#[test]
fn report_a_supported_optimum_reports_the_cheaper_unsupported_argmin() {
    let report = build(process(
        "codex",
        8,
        json!({"trigger": 195_000, "turns": 200}),
    ));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "SupportedOptimum");
    assert_eq!(pick["extrapolated"], false);
    assert!(pick["trigger_tokens"].as_u64().unwrap() >= 195_000);
    let cheaper = pick["unsupported_argmin_tokens"]
        .as_u64()
        .expect("a cheaper trigger");
    assert!(cheaper < pick["trigger_tokens"].as_u64().unwrap());
    let saving = pick["unsupported_relative_saving"]
        .as_f64()
        .expect("its saving");
    assert!(saving > 0.0 && saving < 1.0);
    let rule = text(&pick["pick_rule_text"]);
    assert!(rule.starts_with("cheapest candidate with observed support; cost keeps falling below "));
    assert!(
        rule.contains("cheaper in simulation")
            && rule.ends_with("but no compactions observed there")
    );
    assert_ne!(pick["confidence"], "High");
    let plain = text(&report["plain_text"]);
    assert!(
        plain.contains("Pick rule: cheapest candidate with observed support")
            && plain.contains("Cheaper but unsupported: ")
    );
    assert!(
        has_warning(&report, "no observed compactions there")
            || has_warning(&report, "but no observed compactions")
    );
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_claude_corpus_compacting_at_567k_has_support_only_far_above_the_cheap_region
#[test]
fn report_a_claude_corpus_compacting_at_567k_has_support_only_far_above_the_cheap_region() {
    let report = build(process(
        "claude",
        6,
        json!({"turns": 400, "trigger": 567_000}),
    ));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "SupportedOptimum");
    assert!(pick["trigger_tokens"].as_u64().unwrap() >= 567_000);
    assert!(pick["unsupported_argmin_tokens"].as_u64().unwrap() < 300_000);
    assert!(pick["unsupported_relative_saving"].as_f64().unwrap() > 0.2);
    assert_eq!(pick["setting_key"], "autoCompactWindow");
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::support_counts_main_sessions_only
#[test]
fn report_support_counts_main_sessions_only() {
    let report = build(json!({
        "agent": "codex", "sessions": 8, "process": {"trigger": 240_000, "turns": 200},
        "subagent_sessions": 4, "subagent_process": {"trigger": 150_000}
    }));
    assert_eq!(card(&report)["pick_rule"], "PlainArgmin");
    assert!(report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["observed_support"] == 0.0));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::an_optimum_at_the_grid_floor_widens_the_grid_once
#[test]
fn report_an_optimum_at_the_grid_floor_widens_the_grid_once() {
    let report = build(process(
        "codex",
        8,
        json!({"trigger": 150_000, "post_fresh": 2_000, "post_cache_read": 15_000, "growth": 3_000}),
    ));
    let pick = card(&report);
    assert_eq!(pick["grid_widened"], true);
    assert_eq!(pick["grid_floor_limited"], false);
    let t = triggers(&report);
    assert_eq!(t.first(), Some(&60_000));
    assert!(t.windows(2).all(|p| p[0] < p[1]));
    let best = report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["is_best"] == true)
        .unwrap();
    let best_trigger = best["trigger_tokens"].as_u64().unwrap();
    assert!(best_trigger > 60_000 && best_trigger < 100_000);
    assert!(has_warning(&report, "grid was widened to 60k-"));
    assert!(text(&report["plain_text"]).contains("The simulated grid was widened automatically."));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::claude_widening_stops_at_the_lowest_setting_the_agent_accepts
#[test]
fn report_claude_widening_stops_at_the_lowest_setting_the_agent_accepts() {
    let report = build(process(
        "claude",
        8,
        json!({"trigger": 150_000, "post_fresh": 2_000, "post_cache_read": 15_000}),
    ));
    assert_eq!(card(&report)["grid_widened"], true);
    assert_eq!(triggers(&report).first(), Some(&67_000));
    assert!(report["simulation"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["setting_value"].as_u64().unwrap() >= 100_000));
    assert!(card(&report)["setting_value"].as_u64().unwrap() >= 100_000);
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_normal_optimum_does_not_widen_the_grid
#[test]
fn report_a_normal_optimum_does_not_widen_the_grid() {
    let report = build(spec("codex", 8));
    let pick = card(&report);
    assert!(pick["grid_widened"] == false && pick["grid_floor_limited"] == false);
    assert_eq!(triggers(&report).first(), Some(&100_000));
    assert!(!has_warning(&report, "widened"));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::widening_triggers_cover_floor_ceiling_and_limits
#[test]
fn report_widening_triggers_cover_floor_ceiling_and_limits() {
    let host = server_host();
    let claude_grid = host.default_grid("claude");
    assert_eq!(
        host.widening_triggers("claude", &claude_grid, 120_000),
        vec![67_000, 80_000, 93_000, 106_000]
    );
    let ceiling = host.widening_triggers("claude", &claude_grid, 600_000);
    assert_eq!(ceiling.last(), Some(&967_000));
    assert!(ceiling.iter().all(|&p| p > 600_000));
    let wide = vec![67_000, 120_000, 967_000];
    assert!(host.widening_triggers("claude", &wide, 967_000).is_empty());
    assert!(host.widening_triggers("claude", &wide, 67_000).is_empty());
    assert!(host
        .widening_triggers("claude", &claude_grid, 250_000)
        .is_empty());
    let codex_grid = host.default_grid("codex");
    assert_eq!(
        host.widening_triggers("codex", &codex_grid, 100_000),
        vec![60_000, 70_000, 80_000, 90_000]
    );
    assert!(host
        .widening_triggers("codex", &codex_grid, 232_560)
        .is_empty());
    assert!(host.widening_triggers("codex", &[], 0).is_empty());
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::the_claude_567k_corpus_offers_an_extrapolated_alternative
#[test]
fn report_the_claude_567k_corpus_offers_an_extrapolated_alternative() {
    let report = build(process(
        "claude",
        6,
        json!({"turns": 400, "trigger": 567_000}),
    ));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "SupportedOptimum");
    assert_eq!(pick["trigger_tokens"], 575_000);
    let alt = &pick["extrapolated_alternative"];
    assert!(alt.is_object());
    let alt_trigger = alt["trigger_tokens"].as_u64().unwrap();
    assert!((175_000..=225_000).contains(&alt_trigger));
    assert!(alt["relative_saving"].as_f64().unwrap() > 0.30);
    assert_eq!(alt["trigger_tokens"], pick["unsupported_argmin_tokens"]);
    assert_eq!(alt["relative_saving"], pick["unsupported_relative_saving"]);
    assert_eq!(alt["setting_key"], "autoCompactWindow");
    assert_eq!(alt["setting_value"].as_u64(), Some(alt_trigger + 33_000));
    assert_eq!(alt["setting_clamped"], false);
    assert!(text(&alt["observed_support_text"]).starts_with("extrapolated: 0.0% of observed"));
    let headline = text(&alt["headline_text"]);
    assert!(headline.starts_with("Simulated optimum: autoCompactWindow "));
    assert!(headline.contains("cheaper than the supported pick"));
    assert!(headline.ends_with("EXTRAPOLATED: no observed compactions at or below this level)"));
    assert_eq!(pick["extrapolated"], false);
    assert!(text(&report["plain_text"]).contains(&format!("Alternative: {headline}")));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_codex_supported_optimum_maps_its_alternative_through_the_semantics
#[test]
fn report_a_codex_supported_optimum_maps_its_alternative_through_the_semantics() {
    let report = build(process(
        "codex",
        8,
        json!({"trigger": 195_000, "turns": 200}),
    ));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "SupportedOptimum");
    assert_eq!(pick["trigger_tokens"], 200_000);
    let alt = &pick["extrapolated_alternative"];
    assert!(alt.is_object());
    assert_eq!(alt["trigger_tokens"], pick["unsupported_argmin_tokens"]);
    assert!(alt["trigger_tokens"].as_u64() < pick["trigger_tokens"].as_u64());
    assert_eq!(alt["setting_key"], "model_auto_compact_token_limit");
    assert_eq!(alt["setting_value"], alt["trigger_tokens"]);
    assert!(alt["relative_saving"].as_f64().unwrap() > 0.0);
    assert!(text(&alt["headline_text"]).contains("model_auto_compact_token_limit"));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::the_band_pick_without_a_cheaper_unsupported_trigger_has_no_alternative
#[test]
fn report_the_band_pick_without_a_cheaper_unsupported_trigger_has_no_alternative() {
    let report = build(process("codex", 8, json!({"trigger": 150_000})));
    let pick = card(&report);
    assert_eq!(pick["pick_rule"], "BandWithSupport");
    assert!(
        pick["unsupported_argmin_tokens"].is_null() && pick["extrapolated_alternative"].is_null()
    );
    assert!(!text(&report["plain_text"]).contains("Alternative:"));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::the_plain_argmin_pick_has_no_alternative
#[test]
fn report_the_plain_argmin_pick_has_no_alternative() {
    let report = build(process(
        "codex",
        8,
        json!({"trigger": 240_000, "turns": 200}),
    ));
    assert_eq!(card(&report)["pick_rule"], "PlainArgmin");
    assert!(card(&report)["extrapolated_alternative"].is_null());
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_small_corpus_reports_the_research_prior_as_its_rework_model
#[test]
fn report_a_small_corpus_reports_the_research_prior_as_its_rework_model() {
    let report = build(spec("claude", 6));
    let model = text(&report["rework_model_text"]);
    assert!(model.contains("research prior") && model.contains("not measured on these logs"));
    assert!(text(&report["plain_text"]).contains(&format!("Rework model: {model}")));
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::a_corpus_with_enough_compactions_measures_its_rework
#[test]
fn report_a_corpus_with_enough_compactions_measures_its_rework() {
    let report = build(spec("claude", 60));
    assert!(report["compactions"]["total"].as_u64().unwrap() >= 30);
    assert!(text(&report["rework_model_text"]).contains("measured from"));
    assert!(!text(&report["rework_model_text"]).contains("research prior"));
    assert!(text(&report["rework_note"]).contains("measured from"));
    assert!(report["recommendation"].is_object());
}

/// REQ: PORT:ilium-client/src/compaction_report/tests.rs::the_model_text_is_set_even_when_nothing_can_be_replayed
#[test]
fn report_the_model_text_is_set_even_when_nothing_can_be_replayed() {
    let report = build(spec("codex", 0));
    assert!(report["recommendation"].is_null());
    assert!(text(&report["rework_model_text"]).contains("research prior"));
}
