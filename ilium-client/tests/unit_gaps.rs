//! Unit tests for public ilium-client items named by the coverage inventory.
//! Each test drives one item through its public signature only.

use ilium_client::agent_config_writer::{
    read_overrides, AgentConfigTarget, ConfigPaths, FileFingerprint,
};
use ilium_client::attention::queue::AttentionQueue;
use ilium_client::attention::runtime::TurboWorkers;
use ilium_client::board::checkbox_occurrences;
use ilium_client::config::TerminalSettings;
use ilium_client::cost_history::default_cache_path;
use ilium_client::cost_model::format_quota;
use ilium_client::naming::extract_icon_field;
use ilium_client::remote_compaction_native::{
    agent_kind, default_window_tokens, describe_limit, NativeEvaluation, NativeLimit,
};
use ilium_client::session_stats::{ProgressOutcome, SessionStats, TokenTotals, WorkKind};
use ilium_client::session_stats_timeline::{RibbonCell, WorkRibbon};
use ilium_client::value_config::{
    editor_autosave_spec, set_terminal_engine_memory, set_terminal_scrollback,
    terminal_engine_memory_spec, terminal_scrollback_spec,
};
use ilium_client::value_number::NumberSpec;
use ilium_compaction_analysis::AgentKind;

// --- cost_model::format_quota ---------------------------------------------

#[test]
fn format_quota_zero_keeps_one_decimal() {
    assert_eq!(format_quota(0.0), "0.0%", "zero quota keeps one decimal");
}

#[test]
fn format_quota_below_ten_keeps_one_decimal() {
    assert_eq!(format_quota(0.4), "0.4%");
    assert_eq!(format_quota(3.2), "3.2%");
    assert_eq!(format_quota(9.9), "9.9%");
}

#[test]
fn format_quota_at_ten_drops_the_decimal() {
    assert_eq!(
        format_quota(10.0),
        "10%",
        "the one-decimal form ends below 10"
    );
    assert_eq!(format_quota(12.0), "12%");
}

#[test]
fn format_quota_at_and_above_one_hundred_percent() {
    assert_eq!(format_quota(100.0), "100%");
    assert_eq!(format_quota(104.0), "104%", "quota may exceed 100 percent");
}

#[test]
fn format_quota_very_large_value_has_no_unit_suffix() {
    // Quota is a percentage of the plan window, so a huge value is only digits and "%".
    assert_eq!(format_quota(1_000_000_000.0), "1000000000%");
}

// --- cost_history::default_cache_path ------------------------------------

#[test]
fn default_cache_path_names_the_cost_history_file() {
    let path = default_cache_path();
    let file_name = path
        .as_deref()
        .and_then(|candidate| candidate.file_name())
        .and_then(|name| name.to_str());
    assert_eq!(file_name, Some("cost-history-v1.json"));
    assert!(
        path.is_some_and(|candidate| candidate.is_absolute()),
        "cache path must be absolute"
    );
}

// --- session_stats::TokenTotals::prompt_side ------------------------------

#[test]
fn prompt_side_sums_input_and_cache_counts_only() {
    let tokens = TokenTotals {
        input: 1,
        cache_read: 2,
        cache_write: 3,
        output: 100,
        reasoning: 50,
    };
    assert_eq!(
        tokens.prompt_side(),
        6,
        "output and reasoning are not prompt-side"
    );
}

#[test]
fn prompt_side_is_zero_without_prompt_tokens() {
    assert_eq!(TokenTotals::default().prompt_side(), 0);
}

#[test]
fn prompt_side_large_counts_do_not_overflow() {
    let quarter = u64::MAX / 4;
    let tokens = TokenTotals {
        input: quarter,
        cache_read: quarter,
        cache_write: quarter,
        ..TokenTotals::default()
    };
    assert_eq!(tokens.prompt_side(), quarter * 3);
}

// --- session_stats::SessionStats::context_fill ----------------------------

fn stats_with_context(used: Option<u64>, window: Option<u64>) -> SessionStats {
    let mut stats = SessionStats::default();
    stats.context_tokens = used;
    stats.context_window = window;
    stats
}

#[test]
fn context_fill_zero_window_is_unknown_not_a_division() {
    let stats = stats_with_context(Some(50), Some(0));
    assert_eq!(
        stats.context_fill(),
        None,
        "a zero window yields no fraction"
    );
}

#[test]
fn context_fill_above_window_clamps_to_full() {
    let stats = stats_with_context(Some(300), Some(200));
    assert_eq!(stats.context_fill(), Some(1.0));
}

#[test]
fn context_fill_reports_the_partial_fraction() {
    let stats = stats_with_context(Some(50), Some(200));
    assert_eq!(stats.context_fill(), Some(0.25));
}

#[test]
fn context_fill_missing_token_count_is_unknown() {
    let stats = stats_with_context(None, Some(200));
    assert_eq!(stats.context_fill(), None);
}

#[test]
fn context_fill_missing_window_is_unknown() {
    let stats = stats_with_context(Some(50), None);
    assert_eq!(stats.context_fill(), None);
}

// --- session_stats::WorkKind::for_tool ------------------------------------

#[test]
fn for_tool_maps_shell_tools_case_insensitively() {
    let names = [
        "bash",
        "shell",
        "exec_command",
        "local_shell",
        "shell_command",
        "write_stdin",
        "BashOutput",
        "KillShell",
    ];
    for name in names {
        assert_eq!(WorkKind::for_tool(name), WorkKind::Shell, "tool {name}");
    }
}

#[test]
fn for_tool_maps_edit_read_agent_and_web_tools() {
    for name in ["Edit", "write", "MultiEdit", "NotebookEdit", "apply_patch"] {
        assert_eq!(WorkKind::for_tool(name), WorkKind::Edit, "tool {name}");
    }
    for name in ["read", "grep", "glob", "LS", "view_image"] {
        assert_eq!(WorkKind::for_tool(name), WorkKind::Read, "tool {name}");
    }
    for name in ["task", "agent", "spawn_agent", "wait_agent", "send_input"] {
        assert_eq!(WorkKind::for_tool(name), WorkKind::Agent, "tool {name}");
    }
    for name in ["WebFetch", "websearch", "web_search"] {
        assert_eq!(WorkKind::for_tool(name), WorkKind::Web, "tool {name}");
    }
}

#[test]
fn for_tool_unknown_name_falls_back_to_other() {
    assert_eq!(WorkKind::for_tool("frobnicate"), WorkKind::Other);
}

#[test]
fn for_tool_empty_name_falls_back_to_other() {
    assert_eq!(WorkKind::for_tool(""), WorkKind::Other);
}

// --- session_stats_timeline::WorkRibbon::has_outcome ----------------------

fn ribbon_with_outcomes(outcomes: Vec<Option<ProgressOutcome>>) -> WorkRibbon {
    WorkRibbon {
        cells: Vec::new(),
        outcomes,
    }
}

#[test]
fn has_outcome_on_empty_ribbon_is_false() {
    let ribbon = WorkRibbon::default();
    assert!(!ribbon.has_outcome(ProgressOutcome::Success));
}

#[test]
fn has_outcome_with_only_pending_columns_is_false() {
    let ribbon = ribbon_with_outcomes(vec![None, None]);
    assert!(!ribbon.has_outcome(ProgressOutcome::Success));
    assert!(!ribbon.has_outcome(ProgressOutcome::Failure));
}

#[test]
fn has_outcome_finds_a_completed_outcome() {
    let ribbon = ribbon_with_outcomes(vec![None, Some(ProgressOutcome::Success), None]);
    assert!(ribbon.has_outcome(ProgressOutcome::Success));
    assert!(!ribbon.has_outcome(ProgressOutcome::Failure));
}

#[test]
fn has_outcome_matches_each_terminal_kind_in_a_mixed_ribbon() {
    let ribbon = ribbon_with_outcomes(vec![
        Some(ProgressOutcome::Failure),
        Some(ProgressOutcome::Unknown),
    ]);
    assert!(ribbon.has_outcome(ProgressOutcome::Failure));
    assert!(ribbon.has_outcome(ProgressOutcome::Unknown));
    assert!(!ribbon.has_outcome(ProgressOutcome::Success));
}

#[test]
fn has_outcome_ignores_activity_cells() {
    let ribbon = WorkRibbon {
        cells: vec![Some(RibbonCell {
            kind: WorkKind::Edit,
            progress_running: true,
        })],
        outcomes: vec![None],
    };
    assert!(!ribbon.has_outcome(ProgressOutcome::Success));
}

// --- remote_compaction_native ---------------------------------------------

fn evaluation(
    window_tokens: u64,
    default_trigger_tokens: u64,
    effective: Option<u64>,
) -> NativeEvaluation {
    NativeEvaluation {
        window_tokens,
        default_trigger_tokens,
        effective_trigger_tokens: effective,
        remote_trigger_tokens: window_tokens / 2,
        is_remote_shadowed: false,
    }
}

#[test]
fn agent_kind_maps_claude_target_to_claude_code() {
    assert_eq!(
        agent_kind(AgentConfigTarget::ClaudeAutoCompactWindow),
        AgentKind::ClaudeCode
    );
}

#[test]
fn agent_kind_maps_codex_target_to_codex() {
    assert_eq!(
        agent_kind(AgentConfigTarget::CodexAutoCompactTokenLimit),
        AgentKind::Codex
    );
}

#[test]
fn default_window_tokens_is_positive_for_each_target() {
    assert!(default_window_tokens(AgentConfigTarget::ClaudeAutoCompactWindow) > 0);
    assert!(default_window_tokens(AgentConfigTarget::CodexAutoCompactTokenLimit) > 0);
}

#[test]
fn describe_limit_default_says_not_set() {
    assert_eq!(describe_limit(NativeLimit::Default), "not set");
}

#[test]
fn describe_limit_token_setting_reports_tokens() {
    assert_eq!(
        describe_limit(NativeLimit::Setting(180_000)),
        "180000 tokens"
    );
}

#[test]
fn describe_limit_zero_and_maximum_token_values() {
    assert_eq!(describe_limit(NativeLimit::Setting(0)), "0 tokens");
    assert_eq!(
        describe_limit(NativeLimit::Setting(u64::MAX)),
        format!("{} tokens", u64::MAX)
    );
}

#[test]
fn describe_limit_percent_reports_share_of_window() {
    assert_eq!(
        describe_limit(NativeLimit::PercentOfWindow(80)),
        "80% of the window"
    );
}

#[test]
fn describe_limit_disabled_says_compaction_is_off() {
    assert_eq!(
        describe_limit(NativeLimit::Disabled),
        "automatic compaction disabled"
    );
}

#[test]
fn effective_percent_is_none_while_native_compaction_is_off() {
    assert_eq!(evaluation(200_000, 33_000, None).effective_percent(), None);
}

#[test]
fn effective_percent_is_share_of_window() {
    let percent = evaluation(200_000, 33_000, Some(50_000)).effective_percent();
    assert_eq!(percent, Some(25.0));
}

#[test]
fn effective_percent_zero_tokens_is_zero() {
    let percent = evaluation(200_000, 33_000, Some(0)).effective_percent();
    assert_eq!(percent, Some(0.0));
}

#[test]
fn default_percent_is_share_of_window_for_cli_default_trigger() {
    assert_eq!(evaluation(200_000, 33_000, None).default_percent(), 16.5);
}

// --- naming::extract_icon_field -------------------------------------------

#[test]
fn extract_icon_field_returns_present_icon() {
    let parsed = serde_json::json!({"icon": "🔧"});
    let icon = extract_icon_field(&parsed, "icon", "tab").expect("present icon accepted");
    assert_eq!(icon, "🔧");
}

#[test]
fn extract_icon_field_trims_surrounding_whitespace() {
    let parsed = serde_json::json!({"icon": " ✨ "});
    let icon = extract_icon_field(&parsed, "icon", "tab").expect("trimmed icon accepted");
    assert_eq!(icon, "✨");
}

#[test]
fn extract_icon_field_missing_field_is_an_error() {
    let parsed = serde_json::json!({"other": "🔧"});
    assert!(extract_icon_field(&parsed, "icon", "tab").is_err());
}

#[test]
fn extract_icon_field_non_string_value_is_an_error() {
    let parsed = serde_json::json!({"icon": 5});
    assert!(extract_icon_field(&parsed, "icon", "tab").is_err());
}

#[test]
fn extract_icon_field_prose_is_rejected() {
    let parsed = serde_json::json!({"icon": "hello world"});
    assert!(extract_icon_field(&parsed, "icon", "tab").is_err());
}

#[test]
fn extract_icon_field_control_character_is_rejected() {
    let parsed = serde_json::json!({"icon": "\u{7}"});
    assert!(extract_icon_field(&parsed, "icon", "tab").is_err());
}

#[test]
fn extract_icon_field_empty_value_is_rejected() {
    let parsed = serde_json::json!({"icon": ""});
    assert!(extract_icon_field(&parsed, "icon", "tab").is_err());
}

#[test]
fn extract_icon_field_accepts_sixteen_characters_and_rejects_seventeen() {
    let sixteen = serde_json::json!({"icon": "a".repeat(16)});
    assert!(extract_icon_field(&sixteen, "icon", "tab").is_ok());
    let seventeen = serde_json::json!({"icon": "a".repeat(17)});
    assert!(extract_icon_field(&seventeen, "icon", "tab").is_err());
}

#[test]
fn extract_icon_field_keeps_quote_characters_inside_the_value() {
    let parsed = serde_json::json!({"icon": "\"x\""});
    let icon =
        extract_icon_field(&parsed, "icon", "tab").expect("quotes are not control characters");
    assert_eq!(icon, "\"x\"");
}

// --- attention::runtime::TurboWorkers (no worker running) -----------------

#[test]
fn turbo_workers_default_has_no_running_worker() {
    let workers = TurboWorkers::default();
    assert!(!workers.is_running());
    assert_eq!(workers.current_generation(), None);
}

#[test]
fn turbo_cancel_current_without_worker_is_a_no_op() {
    let mut workers = TurboWorkers::default();
    workers.cancel_current();
    assert!(!workers.is_running());
}

#[test]
fn turbo_finish_current_without_worker_is_a_no_op() {
    let mut workers = TurboWorkers::default();
    workers.finish_current();
    assert!(!workers.is_running());
    assert_eq!(workers.reap(), 0);
}

#[test]
fn turbo_reap_with_nothing_retiring_reports_zero() {
    let mut workers = TurboWorkers::default();
    assert_eq!(workers.reap(), 0);
}

#[test]
fn turbo_cancel_all_without_workers_is_a_no_op() {
    let mut workers = TurboWorkers::default();
    workers.cancel_all();
    assert!(!workers.is_running());
    assert_eq!(workers.reap(), 0);
}

// --- attention::queue::AttentionQueue::analysed_fingerprints --------------

#[test]
fn analysed_fingerprints_of_new_queue_is_empty() {
    let queue = AttentionQueue::new();
    assert!(queue.analysed_fingerprints().is_empty());
}

#[test]
fn analysed_fingerprints_is_stable_across_calls() {
    let queue = AttentionQueue::new();
    let first = queue.analysed_fingerprints().len();
    let second = queue.analysed_fingerprints().len();
    assert_eq!(first, second, "repeated reads must agree");
}

// --- agent_config_writer::read_overrides / FileFingerprint::same_content ---

#[test]
fn read_overrides_with_missing_files_yields_no_warnings() {
    let root = tempfile::tempdir().expect("temporary directory");
    let paths = ConfigPaths::with_roots(
        root.path().join("claude"),
        root.path().join("codex"),
        root.path().join("locks"),
    );
    for target in [
        AgentConfigTarget::ClaudeAutoCompactWindow,
        AgentConfigTarget::CodexAutoCompactTokenLimit,
    ] {
        assert!(
            read_overrides(target, &paths).is_empty(),
            "missing user file must produce no override warnings for {target:?}"
        );
    }
}

fn fingerprint(size: u64, sha256: &str) -> FileFingerprint {
    FileFingerprint {
        size,
        modified: None,
        sha256: sha256.to_owned(),
    }
}

#[test]
fn same_content_true_for_identical_fingerprints() {
    let left = fingerprint(12, "abc123");
    assert!(left.same_content(&fingerprint(12, "abc123")));
}

#[test]
fn same_content_false_when_hash_differs_at_same_size() {
    assert!(!fingerprint(12, "abc123").same_content(&fingerprint(12, "def456")));
}

#[test]
fn same_content_false_for_a_trailing_newline_difference() {
    assert!(!fingerprint(5, "aaa").same_content(&fingerprint(6, "bbb")));
}

#[test]
fn same_content_true_for_two_empty_files() {
    let empty_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert!(fingerprint(0, empty_sha256).same_content(&fingerprint(0, empty_sha256)));
}

#[test]
fn same_content_ignores_modification_time() {
    let touched = FileFingerprint {
        size: 3,
        modified: Some(std::time::SystemTime::UNIX_EPOCH),
        sha256: "same".to_owned(),
    };
    assert!(touched.same_content(&fingerprint(3, "same")));
}

// --- board::checkbox_occurrences ------------------------------------------

#[test]
fn checkbox_occurrences_empty_title_has_none() {
    assert!(checkbox_occurrences("").is_empty());
}

#[test]
fn checkbox_occurrences_detects_open_and_checked_markers_with_byte_index() {
    assert_eq!(
        checkbox_occurrences("[ ] a [x] b [X] c"),
        vec![(0, false), (6, true), (12, true)]
    );
}

#[test]
fn checkbox_occurrences_keep_byte_indices_after_multibyte_text() {
    // "é" is two bytes, so the marker starts at byte 2.
    assert_eq!(checkbox_occurrences("é[ ]"), vec![(2, false)]);
}

#[test]
fn checkbox_occurrences_ignores_partial_marker_at_end() {
    assert!(checkbox_occurrences("a [x").is_empty());
    assert!(checkbox_occurrences("a [ ").is_empty());
}

// --- value_config specs and setters ---------------------------------------

#[test]
fn editor_autosave_spec_is_250_to_5000_milliseconds() {
    assert_eq!(
        editor_autosave_spec(),
        NumberSpec::Integer {
            minimum: 250,
            maximum: 5000,
        }
    );
}

#[test]
fn terminal_scrollback_spec_matches_settings_limits() {
    assert_eq!(
        terminal_scrollback_spec(),
        NumberSpec::Integer {
            minimum: i128::from(TerminalSettings::MIN_SCROLLBACK_BUDGET_MIB),
            maximum: i128::from(TerminalSettings::MAX_SCROLLBACK_BUDGET_MIB),
        }
    );
}

#[test]
fn terminal_engine_memory_spec_runs_from_zero_to_engine_maximum() {
    assert_eq!(
        terminal_engine_memory_spec(),
        NumberSpec::Integer {
            minimum: 0,
            maximum: i128::from(TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB),
        }
    );
}

#[test]
fn set_terminal_scrollback_accepts_both_limits() {
    let mut settings = TerminalSettings::default();
    assert_eq!(set_terminal_scrollback(&mut settings, "4"), Ok(()));
    assert_eq!(settings.scrollback_budget_mib, 4);
    assert_eq!(set_terminal_scrollback(&mut settings, "512"), Ok(()));
    assert_eq!(settings.scrollback_budget_mib, 512);
}

#[test]
fn set_terminal_scrollback_rejects_out_of_range_and_non_numeric_text() {
    let mut settings = TerminalSettings::default();
    let before = settings.scrollback_budget_mib;
    assert!(set_terminal_scrollback(&mut settings, "3").is_err());
    assert!(set_terminal_scrollback(&mut settings, "513").is_err());
    assert!(set_terminal_scrollback(&mut settings, "abc").is_err());
    assert_eq!(
        settings.scrollback_budget_mib, before,
        "rejected input must leave the setting unchanged"
    );
}

#[test]
fn set_terminal_engine_memory_accepts_the_maximum() {
    let mut settings = TerminalSettings::default();
    let maximum = TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB;
    assert_eq!(
        set_terminal_engine_memory(&mut settings, &maximum.to_string()),
        Ok(())
    );
    assert_eq!(settings.engine_memory_budget_mib, maximum);
}

#[test]
fn set_terminal_engine_memory_rejects_above_maximum_and_decimals() {
    let mut settings = TerminalSettings::default();
    let before = settings.engine_memory_budget_mib;
    let above = TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB + 1;
    assert!(set_terminal_engine_memory(&mut settings, &above.to_string()).is_err());
    assert!(set_terminal_engine_memory(&mut settings, "2.5").is_err());
    assert_eq!(settings.engine_memory_budget_mib, before);
}
