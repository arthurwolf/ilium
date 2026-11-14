//! Unit tests for public ilium-client items named by the coverage inventory.
//! Each test drives one item through its public signature only.

use ilium_client::config::TerminalSettings;
use ilium_client::cost_history::default_cache_path;
use ilium_client::cost_model::format_quota;
use ilium_client::naming::extract_icon_field;
use ilium_client::session_stats::{ProgressOutcome, SessionStats, TokenTotals, WorkKind};
use ilium_client::session_stats_timeline::{RibbonCell, WorkRibbon};
use ilium_client::value_config::{
    editor_autosave_spec, set_terminal_engine_memory, set_terminal_scrollback,
    terminal_engine_memory_spec, terminal_scrollback_spec,
};
use ilium_client::value_number::NumberSpec;

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
