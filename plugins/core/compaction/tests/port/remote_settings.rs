//! Twins of the inline tests in ilium-client/src/remote_compaction_settings.rs (the settings
//! model: defaults, clamping, adjust semantics, technique routing, options mapping).
//! Gaps: `remote_settings_defaults`, `remote_settings_sanitize`, `remote_settings_adjust`,
//! `remote_settings_technique_for`, `remote_settings_options_for`, `remote_settings_row_ids`,
//! `technique_ids`, `technique_description`, `technique_fidelity`,
//! `custom_prompt_max_chars`, `custom_prompt_max_serialized_bytes`,
//! `settings_serialized_bound_ok` (see ../API_GAPS_port.md).

use serde_json::{json, Value};

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

fn host() -> FakeHost {
    client_host()
}

fn sanitize(host: &FakeHost, patch: Value) -> Value {
    host.remote_settings_sanitize(patch)
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::defaults_match_the_specification
#[test]
fn rset_defaults_match_the_specification() {
    let d = host().remote_settings_defaults();
    assert_eq!(d["enabled"], false);
    assert_eq!(d["automatic"], false);
    assert_eq!(d["threshold_percent"], 65);
    assert_eq!(d["claude_technique"], "claude-code");
    assert_eq!(d["codex_technique"], "codex");
    assert_eq!(d["other_technique"], "claude-code");
    assert_eq!(d["tail_tokens"], 20_000);
    assert_eq!(d["protected_recent_tool_tokens"], 40_000);
    assert_eq!(d["tool_result_chars"], 2_000);
    assert_eq!(d["redact_secrets"], true);
    assert_eq!(d["summarizer_context_tokens"], 120_000);
    assert_eq!(d["pause_timeout_seconds"], 120);
    assert_eq!(d["cooldown_minutes"], 10);
    assert_eq!(d["keep_backups"], 3);
    assert_eq!(d["privacy_banner_dismissed"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::sanitize_clamps_every_range_and_bounds_the_prompt
#[test]
fn rset_sanitize_clamps_every_range_and_bounds_the_prompt() {
    let host = host();
    let max = host.custom_prompt_max_chars();
    let high = sanitize(
        &host,
        json!({
            "threshold_percent": 99, "tail_tokens": u64::MAX, "protected_recent_tool_tokens": u64::MAX,
            "tool_result_chars": u64::MAX, "summarizer_context_tokens": u64::MAX,
            "pause_timeout_seconds": u64::MAX, "cooldown_minutes": u64::MAX, "keep_backups": u64::MAX,
            "custom_prompt": "x".repeat(max + 50),
        }),
    );
    assert_eq!(high["threshold_percent"], 88);
    assert_eq!(high["tail_tokens"], 100_000);
    assert_eq!(high["protected_recent_tool_tokens"], 200_000);
    assert_eq!(high["tool_result_chars"], 20_000);
    assert_eq!(high["summarizer_context_tokens"], 900_000);
    assert_eq!(high["pause_timeout_seconds"], 1_800);
    assert_eq!(high["cooldown_minutes"], 240);
    assert_eq!(high["keep_backups"], 20);
    assert_eq!(high["custom_prompt"].as_str().unwrap().chars().count(), max);
    let low = sanitize(
        &host,
        json!({
            "threshold_percent": 0, "tail_tokens": 0, "protected_recent_tool_tokens": 0,
            "tool_result_chars": 0, "summarizer_context_tokens": 0, "pause_timeout_seconds": 0,
            "cooldown_minutes": 0, "keep_backups": 0,
        }),
    );
    assert_eq!(low["threshold_percent"], 30);
    assert_eq!(low["tail_tokens"], 2_000);
    assert_eq!(low["protected_recent_tool_tokens"], 0);
    assert_eq!(low["tool_result_chars"], 200);
    assert_eq!(low["summarizer_context_tokens"], 8_000);
    assert_eq!(low["pause_timeout_seconds"], 10);
    assert_eq!(low["cooldown_minutes"], 1);
    assert_eq!(low["keep_backups"], 1);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::a_multibyte_prompt_is_cut_on_a_character_boundary_inside_the_transport_bound
#[test]
fn rset_a_multibyte_prompt_is_cut_on_a_character_boundary_inside_the_transport_bound() {
    let host = host();
    let cleaned = sanitize(
        &host,
        json!({"custom_prompt": "\u{1F980}".repeat(host.custom_prompt_max_chars())}),
    );
    let prompt = cleaned["custom_prompt"].as_str().unwrap();
    assert!(prompt.len() <= host.custom_prompt_max_serialized_bytes());
    assert!(prompt.chars().all(|c| c == '\u{1F980}'));
    assert!(!prompt.is_empty());
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::an_escape_heavy_prompt_still_fits_the_configuration_transport
#[test]
fn rset_an_escape_heavy_prompt_still_fits_the_configuration_transport() {
    let host = host();
    let max = host.custom_prompt_max_chars();
    let cleaned = sanitize(&host, json!({"custom_prompt": "\u{1}".repeat(max)}));
    assert!(host.settings_serialized_bound_ok(&cleaned));
    assert!(cleaned["custom_prompt"].as_str().unwrap().len() < max);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::toggles_flip_and_report_the_change
#[test]
fn rset_toggles_flip_and_report_the_change() {
    let mut host = host();
    for (row, field, start) in [
        ("enabled", "enabled", false),
        ("automatic", "automatic", false),
        ("redact_secrets", "redact_secrets", true),
    ] {
        assert_eq!(host.remote_settings_defaults()[field], start);
        assert!(
            host.remote_settings_adjust(row, 0),
            "{row} reports a change"
        );
        assert_eq!(host.remote_settings_current()[field], !start);
        assert!(host.remote_settings_adjust(row, -1), "{row} flips back");
        assert_eq!(host.remote_settings_current()[field], start);
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::threshold_steps_by_one_and_stops_at_its_limits
#[test]
fn rset_threshold_steps_by_one_and_stops_at_its_limits() {
    let mut host = host();
    assert!(host.remote_settings_adjust("threshold", 1));
    assert_eq!(host.remote_settings_current()["threshold_percent"], 66);
    assert!(host.remote_settings_adjust("threshold", -1));
    assert_eq!(host.remote_settings_current()["threshold_percent"], 65);
    for _ in 0..100 {
        host.remote_settings_adjust("threshold", 1);
    }
    assert_eq!(host.remote_settings_current()["threshold_percent"], 88);
    assert!(
        !host.remote_settings_adjust("threshold", 1),
        "no change at the top"
    );
    for _ in 0..100 {
        host.remote_settings_adjust("threshold", -1);
    }
    assert_eq!(host.remote_settings_current()["threshold_percent"], 30);
    assert!(
        !host.remote_settings_adjust("threshold", -1),
        "no change at the bottom"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::numeric_rows_walk_their_ladders_and_stop_at_the_ends
#[test]
fn rset_numeric_rows_walk_their_ladders_and_stop_at_the_ends() {
    let mut host = host();
    for (row, field, low, high) in [
        ("pause_timeout", "pause_timeout_seconds", 10u64, 1_800u64),
        ("cooldown", "cooldown_minutes", 1, 240),
        ("tail_tokens", "tail_tokens", 2_000, 100_000),
        (
            "protected_tool_tokens",
            "protected_recent_tool_tokens",
            0,
            200_000,
        ),
        ("tool_result_chars", "tool_result_chars", 200, 20_000),
        (
            "summarizer_context_tokens",
            "summarizer_context_tokens",
            8_000,
            900_000,
        ),
        ("keep_backups", "keep_backups", 1, 20),
    ] {
        let start = host.remote_settings_current()[field].as_u64().unwrap();
        assert!(host.remote_settings_adjust(row, 1), "{row} grows");
        let up = host.remote_settings_current()[field].as_u64().unwrap();
        assert!(up > start, "{row}");
        assert!(host.remote_settings_adjust(row, -1), "{row} shrinks");
        assert_eq!(
            host.remote_settings_current()[field].as_u64().unwrap(),
            start,
            "{row}"
        );
        for _ in 0..200 {
            host.remote_settings_adjust(row, 1);
        }
        assert_eq!(
            host.remote_settings_current()[field].as_u64().unwrap(),
            high,
            "{row} top"
        );
        assert!(
            !host.remote_settings_adjust(row, 1),
            "{row} stops at the top"
        );
        for _ in 0..200 {
            host.remote_settings_adjust(row, -1);
        }
        assert_eq!(
            host.remote_settings_current()[field].as_u64().unwrap(),
            low,
            "{row} bottom"
        );
        assert!(
            !host.remote_settings_adjust(row, -1),
            "{row} stops at the bottom"
        );
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::an_off_ladder_value_snaps_in_the_requested_direction
#[test]
fn rset_an_off_ladder_value_snaps_in_the_requested_direction() {
    let mut host = host();
    host.remote_settings_set(json!({"tail_tokens": 23_456}));
    assert!(host.remote_settings_adjust("tail_tokens", 1));
    assert_eq!(host.remote_settings_current()["tail_tokens"], 30_000);
    host.remote_settings_set(json!({"tail_tokens": 23_456}));
    assert!(host.remote_settings_adjust("tail_tokens", -1));
    assert_eq!(host.remote_settings_current()["tail_tokens"], 20_000);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::technique_rows_cycle_through_every_technique_independently
#[test]
fn rset_technique_rows_cycle_through_every_technique_independently() {
    let mut host = host();
    let all = host.technique_ids();
    for (row, field) in [
        ("technique:claude", "claude_technique"),
        ("technique:codex", "codex_technique"),
        ("technique:other", "other_technique"),
    ] {
        let others: Vec<(String, Value)> =
            ["claude_technique", "codex_technique", "other_technique"]
                .iter()
                .filter(|name| **name != field)
                .map(|name| {
                    (
                        name.to_string(),
                        host.remote_settings_current()[*name].clone(),
                    )
                })
                .collect();
        let start = host.remote_settings_current()[field]
            .as_str()
            .unwrap()
            .to_string();
        let start_index = all
            .iter()
            .position(|id| *id == start)
            .expect("known technique");
        for step in 1..=all.len() {
            assert!(host.remote_settings_adjust(row, 1));
            let expected = &all[(start_index + step) % all.len()];
            assert_eq!(
                host.remote_settings_current()[field],
                json!(expected),
                "{row} step {step}"
            );
        }
        assert!(host.remote_settings_adjust(row, -1));
        assert_eq!(
            host.remote_settings_current()[field],
            json!(all[(start_index + all.len() - 1) % all.len()])
        );
        for (name, value) in others {
            assert_eq!(
                host.remote_settings_current()[&name],
                value,
                "{row} left {name} alone"
            );
        }
        host.remote_settings_set(json!({ field: start }));
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::technique_for_routes_each_agent_class_to_its_row
#[test]
fn rset_technique_for_routes_each_agent_class_to_its_row() {
    let host = host();
    let settings = json!({"claude_technique": "opencode", "codex_technique": "custom", "other_technique": "ilium"});
    assert_eq!(
        host.remote_settings_technique_for(&settings, "claude"),
        "opencode"
    );
    assert_eq!(
        host.remote_settings_technique_for(&settings, "codex"),
        "custom"
    );
    assert_eq!(
        host.remote_settings_technique_for(&settings, "antigravity"),
        "ilium"
    );
    assert_eq!(
        host.remote_settings_technique_for(&settings, "other"),
        "ilium"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::the_banner_row_dismisses_once_and_never_returns
#[test]
fn rset_the_banner_row_dismisses_once_and_never_returns() {
    let mut host = host();
    assert_eq!(
        host.remote_settings_row_ids().first().map(String::as_str),
        Some("privacy_banner")
    );
    assert!(
        host.remote_settings_adjust("privacy_banner", 0),
        "dismissing reports a change"
    );
    assert_eq!(
        host.remote_settings_current()["privacy_banner_dismissed"],
        true
    );
    assert_eq!(
        host.remote_settings_row_ids().first().map(String::as_str),
        Some("enabled")
    );
    assert!(!host
        .remote_settings_row_ids()
        .iter()
        .any(|id| id == "privacy_banner"));
    assert!(
        !host.remote_settings_adjust("privacy_banner", 0),
        "never returns"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::rows_cover_every_help_topic_exactly_once
#[test]
fn rset_rows_cover_every_help_topic_exactly_once() {
    let mut host = host();
    host.remote_settings_adjust("privacy_banner", 0);
    let ids = host.remote_settings_help_ids();
    assert_eq!(ids.len(), host.remote_settings_help_topic_count());
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(*id, format!("RCOMP-{:02}", index + 1));
        assert_eq!(ids.iter().filter(|other| *other == id).count(), 1, "{id}");
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::display_only_rows_and_the_prompt_row_do_not_change_settings
#[test]
fn rset_display_only_rows_and_the_prompt_row_do_not_change_settings() {
    let mut host = host();
    host.remote_settings_adjust("privacy_banner", 0);
    let before = host.remote_settings_current();
    for row in ["model", "custom_prompt"] {
        for delta in [-1, 0, 1] {
            assert!(!host.remote_settings_adjust(row, delta), "{row} {delta}");
        }
    }
    assert_eq!(host.remote_settings_current(), before);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::options_for_maps_every_field_per_agent
#[test]
fn rset_options_for_maps_every_field_per_agent() {
    let host = host();
    let settings = json!({
        "claude_technique": "opencode", "codex_technique": "custom", "custom_prompt": "Summarise tersely.",
        "tail_tokens": 30000, "protected_recent_tool_tokens": 10000, "tool_result_chars": 500,
        "redact_secrets": false, "summarizer_context_tokens": 64000, "keep_backups": 7,
    });
    let claude = host.remote_settings_options_for(&settings, "claude");
    assert_eq!(claude["technique"], "opencode");
    assert_eq!(claude["custom_prompt"], "Summarise tersely.");
    assert_eq!(claude["tail_tokens"], 30000);
    assert_eq!(claude["protected_recent_tool_tokens"], 10000);
    assert_eq!(claude["tool_result_chars"], 500);
    assert_eq!(claude["redact_secrets"], false);
    assert_eq!(claude["summarizer_context_tokens"], 64000);
    assert_eq!(
        claude["summarizer_max_output_tokens"],
        host.default_summarizer_max_output_tokens()
    );
    assert_eq!(claude["keep_backups"], 7);
    assert_eq!(
        host.remote_settings_options_for(&settings, "codex")["technique"],
        "custom"
    );
    let defaults = host.remote_settings_options_for(&host.remote_settings_defaults(), "claude");
    assert!(defaults["custom_prompt"].is_null());
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::every_technique_has_a_description_and_provenance
#[test]
fn rset_every_technique_has_a_description_and_provenance() {
    let host = host();
    for technique in host.technique_ids() {
        assert!(
            !host.technique_description(&technique).is_empty(),
            "{technique}"
        );
        assert!(
            host.technique_fidelity(&technique).starts_with("Prompt:"),
            "{technique}"
        );
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::defaults_match_the_specification
#[test]
fn rset_defaults_survive_sanitizing_unchanged() {
    let host = host();
    let defaults = host.remote_settings_defaults();
    assert_eq!(sanitize(&host, defaults.clone()), defaults);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::sanitize_clamps_every_range_and_bounds_the_prompt
#[test]
fn rset_sanitize_keeps_a_prompt_inside_the_bound_unchanged() {
    let host = host();
    let cleaned = sanitize(&host, json!({"custom_prompt": "Keep the plan."}));
    assert_eq!(cleaned["custom_prompt"], "Keep the plan.");
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::a_multibyte_prompt_is_cut_on_a_character_boundary_inside_the_transport_bound
#[test]
fn rset_short_multibyte_prompt_is_kept_whole() {
    let host = host();
    let prompt = "\u{e9}t\u{e9} \u{1F980}";
    let cleaned = sanitize(&host, json!({"custom_prompt": prompt}));
    assert_eq!(cleaned["custom_prompt"], prompt);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::an_escape_heavy_prompt_still_fits_the_configuration_transport
#[test]
fn rset_control_characters_never_reach_the_stored_prompt() {
    let host = host();
    let cleaned = sanitize(&host, json!({"custom_prompt": "a\u{1}b"}));
    assert!(!cleaned["custom_prompt"]
        .as_str()
        .unwrap()
        .contains('\u{1}'));
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::toggles_flip_and_report_the_change
#[test]
fn rset_toggle_leaves_the_other_flags_alone() {
    let mut host = host();
    assert!(host.remote_settings_adjust("enabled", 0));
    assert_eq!(host.remote_settings_current()["automatic"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::threshold_steps_by_one_and_stops_at_its_limits
#[test]
fn rset_threshold_five_steps_down_from_the_default() {
    let mut host = host();
    for _ in 0..5 {
        assert!(host.remote_settings_adjust("threshold", -1));
    }
    assert_eq!(host.remote_settings_current()["threshold_percent"], 60);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::numeric_rows_walk_their_ladders_and_stop_at_the_ends
#[test]
fn rset_zero_delta_changes_no_numeric_row() {
    let mut host = host();
    let before = host.remote_settings_current();
    assert!(!host.remote_settings_adjust("cooldown", 0));
    assert_eq!(host.remote_settings_current(), before);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::an_off_ladder_value_snaps_in_the_requested_direction
#[test]
fn rset_off_ladder_value_below_the_ladder_snaps_to_its_bottom() {
    let mut host = host();
    host.remote_settings_set(json!({"tail_tokens": 2_500}));
    assert!(host.remote_settings_adjust("tail_tokens", -1));
    assert_eq!(host.remote_settings_current()["tail_tokens"], 2_000);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::technique_rows_cycle_through_every_technique_independently
#[test]
fn rset_technique_row_step_leaves_other_agents_untouched() {
    let mut host = host();
    let before = host.remote_settings_current();
    assert!(host.remote_settings_adjust("technique:codex", 1));
    let after = host.remote_settings_current();
    assert_eq!(after["claude_technique"], before["claude_technique"]);
    assert_ne!(after["codex_technique"], before["codex_technique"]);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::technique_for_routes_each_agent_class_to_its_row
#[test]
fn rset_technique_for_uses_the_claude_default() {
    let host = host();
    let defaults = host.remote_settings_defaults();
    assert_eq!(
        host.remote_settings_technique_for(&defaults, "claude"),
        "claude-code"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::the_banner_row_dismisses_once_and_never_returns
#[test]
fn rset_dismissing_the_banner_removes_exactly_one_row() {
    let mut host = host();
    let before = host.remote_settings_row_ids().len();
    host.remote_settings_adjust("privacy_banner", 0);
    assert_eq!(host.remote_settings_row_ids().len(), before - 1);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::rows_cover_every_help_topic_exactly_once
#[test]
fn rset_every_help_id_uses_the_rcomp_prefix() {
    let mut host = host();
    host.remote_settings_adjust("privacy_banner", 0);
    assert!(host
        .remote_settings_help_ids()
        .iter()
        .all(|id| id.starts_with("RCOMP-")));
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::display_only_rows_and_the_prompt_row_do_not_change_settings
#[test]
fn rset_display_rows_are_listed_in_the_settings_model() {
    let host = host();
    let rows = host.remote_settings_row_ids();
    assert!(rows.iter().any(|row| row == "model"));
    assert!(rows.iter().any(|row| row == "custom_prompt"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::options_for_maps_every_field_per_agent
#[test]
fn rset_options_for_other_agents_use_the_other_row() {
    let host = host();
    let defaults = host.remote_settings_defaults();
    assert_eq!(
        host.remote_settings_options_for(&defaults, "other")["technique"],
        "claude-code"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings.rs::every_technique_has_a_description_and_provenance
#[test]
fn rset_technique_ids_are_unique() {
    let host = host();
    let mut ids = host.technique_ids();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count);
}
