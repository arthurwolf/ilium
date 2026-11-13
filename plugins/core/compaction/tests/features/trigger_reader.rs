//! F-083 Agent auto-compact trigger reader: the plugin reads the trigger each agent uses (never writes it) and the Remote compaction tab
//! shows it next to the CLI default and warns when the agent would compact first (user doc Settings, Remote compaction).
//!
//! Assumed windows are the defaults of the existing implementation: Claude Code 1 000 000 tokens, Codex 258 400 tokens.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{Event, ViewTarget, ViewVisibilityEvent};
use proptest::prelude::*;

use crate::common::*;

const CLAUDE_FILE: &str = "agent-homes:.claude/settings.json";
const CODEX_FILE: &str = "agent-homes:.codex/config.toml";
const WIDE: u16 = 250;

fn tab() -> ViewTarget {
    ViewTarget::SettingsPage("remote-compaction".to_string())
}

fn session(files: &[(&str, &str)]) -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("compaction").start();
    for (path, content) in files {
        session
            .server(PLUGIN_ID)
            .fs()
            .write(*path, content.as_bytes());
    }
    session.pump().expect("session settles");
    session
}

fn screen_text(session: &mut FakeSession) -> String {
    session.client(PLUGIN_ID).render(tab(), WIDE, 80).text()
}

fn codex_with(limit: u64) -> String {
    format!("model_auto_compact_token_limit = {limit}\n")
}

/// REQ: F-083
#[test]
fn trigger_083_the_tab_has_a_native_compaction_section_headline() {
    let mut session = session(&[]);
    assert!(screen_text(&mut session).contains("NATIVE COMPACTION OF THE AGENTS"));
}

/// REQ: F-083
#[test]
fn trigger_083_without_files_both_agents_show_not_configured_and_the_cli_default() {
    let mut session = session(&[]);
    let text = screen_text(&mut session);
    assert!(text.contains("Configured: not configured. CLI default: 967k tokens (97%)."));
    assert!(text.contains("Configured: not configured. CLI default: 232560 tokens (90%)."));
}

/// REQ: F-083
#[test]
fn trigger_083_the_unset_claude_trigger_fires_33k_below_the_window() {
    let mut session = session(&[]);
    let text = screen_text(&mut session);
    assert!(text.contains("compacts at 967k tokens (97% of an assumed 1000k tokens window)"));
}

/// REQ: F-083
#[test]
fn trigger_083_the_unset_codex_trigger_is_ninety_percent_of_the_window() {
    let mut session = session(&[]);
    let text = screen_text(&mut session);
    assert!(text.contains("compacts at 232560 tokens (90% of an assumed 258400 tokens window)"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_claude_auto_compact_window_in_the_user_file_is_read_and_its_source_named() {
    let mut session = session(&[(CLAUDE_FILE, "{\"autoCompactWindow\": 250000}")]);
    let text = screen_text(&mut session);
    assert!(text.contains("Configured: 250000 tokens from user file"));
    assert!(text.contains("compacts at 217k tokens"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_codex_limit_in_the_user_file_is_read_and_its_source_named() {
    let mut session = session(&[(CODEX_FILE, "model_auto_compact_token_limit = 200000\n")]);
    let text = screen_text(&mut session);
    assert!(text.contains("Configured: 200000 tokens from user file"));
    assert!(text.contains("compacts at 200k tokens"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_codex_limit_above_ninety_percent_is_reduced_to_ninety_percent() {
    let mut session = session(&[(CODEX_FILE, "model_auto_compact_token_limit = 999999\n")]);
    assert!(screen_text(&mut session).contains("compacts at 232560 tokens"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_low_claude_setting_shadows_the_remote_threshold_with_advice() {
    let mut session = session(&[(CLAUDE_FILE, "{\"autoCompactWindow\": 130000}")]);
    let text = screen_text(&mut session);
    assert!(text.contains("compacts at 97k tokens"));
    assert!(text.contains("Warning: Claude Code compacts itself at 97k tokens before the 65% remote threshold (650k tokens) is reached"));
    assert!(text.contains("Set the threshold to 8% or lower, or raise the agent's own limit."));
}

/// REQ: F-083
#[test]
fn trigger_083_the_warning_follows_the_threshold_setting() {
    let mut session = session(&[(CLAUDE_FILE, "{\"autoCompactWindow\": 130000}")]);
    session
        .client(PLUGIN_ID)
        .settings()
        .set("remote_compaction.threshold_percent", serde_json::json!(88))
        .expect("setting accepted");
    assert!(screen_text(&mut session).contains("before the 88% remote threshold (880k tokens)"));
}

/// REQ: F-083
#[test]
fn trigger_083_no_warning_when_both_agents_use_their_defaults() {
    let mut session = session(&[]);
    assert!(!screen_text(&mut session).contains("Warning:"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_codex_limit_one_token_above_the_remote_trigger_gives_no_warning() {
    // 65 % of 258 400 is 167 960 tokens.
    let mut session = session(&[(CODEX_FILE, &codex_with(167_961))]);
    assert!(!screen_text(&mut session).contains("Warning: Codex"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_codex_limit_equal_to_the_remote_trigger_counts_as_shadowed() {
    let mut session = session(&[(CODEX_FILE, &codex_with(167_960))]);
    assert!(screen_text(&mut session).contains("Warning: Codex compacts itself at 167960 tokens before the 65% remote threshold (167960 tokens)"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_codex_limit_one_token_below_the_remote_trigger_is_shadowed() {
    let mut session = session(&[(CODEX_FILE, &codex_with(167_959))]);
    assert!(screen_text(&mut session).contains("Warning: Codex compacts itself at 167959 tokens"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_codex_profile_override_is_listed_as_a_note_not_resolved() {
    let config = "model_auto_compact_token_limit = 200000\n\n[profiles.fast]\nmodel_auto_compact_token_limit = 100000\n";
    let mut session = session(&[(CODEX_FILE, config)]);
    let text = screen_text(&mut session);
    assert!(text
        .contains("profile \"fast\" sets 100000; it applies only when that profile is selected"));
    assert!(text.contains("Configured: 200000 tokens from user file"));
}

/// REQ: F-083
#[test]
fn trigger_083_a_malformed_claude_file_reads_as_not_configured() {
    let mut session = session(&[(CLAUDE_FILE, "{ this is not json")]);
    assert!(screen_text(&mut session)
        .contains("Configured: not configured. CLI default: 967k tokens (97%)."));
}

/// REQ: F-083
#[test]
fn trigger_083_a_missing_agent_home_file_reads_as_not_configured_without_creating_it() {
    let mut session = session(&[]);
    let _ = screen_text(&mut session);
    assert!(session.server(PLUGIN_ID).fs().read(CLAUDE_FILE).is_none());
    assert!(session.server(PLUGIN_ID).fs().read(CODEX_FILE).is_none());
}

/// REQ: F-083
#[test]
fn trigger_083_reading_never_changes_the_agent_files() {
    let claude = "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\": 130000\n}\n";
    let codex = "# my settings\nmodel_auto_compact_token_limit = 123456\n";
    let mut session = session(&[(CLAUDE_FILE, claude), (CODEX_FILE, codex)]);
    let _ = screen_text(&mut session);
    assert_eq!(
        session.server(PLUGIN_ID).fs().read(CLAUDE_FILE),
        Some(claude.as_bytes().to_vec())
    );
    assert_eq!(
        session.server(PLUGIN_ID).fs().read(CODEX_FILE),
        Some(codex.as_bytes().to_vec())
    );
}

/// REQ: F-083
#[test]
fn trigger_083_the_values_are_read_again_each_time_the_tab_becomes_visible() {
    let mut session = session(&[(CODEX_FILE, &codex_with(200_000))]);
    assert!(screen_text(&mut session).contains("Configured: 200000 tokens"));
    session
        .server(PLUGIN_ID)
        .fs()
        .write(CODEX_FILE, codex_with(150_000).as_bytes());
    session
        .client(PLUGIN_ID)
        .deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
            view: tab(),
            visible: false,
        }));
    session
        .client(PLUGIN_ID)
        .deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
            view: tab(),
            visible: true,
        }));
    session.pump().expect("session settles");
    assert!(screen_text(&mut session).contains("Configured: 150000 tokens"));
}

/// REQ: F-083
#[test]
fn trigger_083_the_reader_needs_the_agent_homes_read_grant_on_the_server_side() {
    let manifest = manifest_value();
    let server: Vec<&str> = manifest["capabilities"]["server"]
        .as_array()
        .expect("grants")
        .iter()
        .filter_map(|grant| grant.as_str())
        .collect();
    assert!(server.contains(&"fs.read:agent-homes"));
}

proptest! {
    // REQ: F-083
    #[test]
    fn trigger_083_a_codex_limit_is_shown_exactly_up_to_ninety_percent_of_the_window(limit in 1_000u64..232_560) {
        let mut session = session(&[(CODEX_FILE, &codex_with(limit))]);
        let text = screen_text(&mut session);
        let expected = if limit % 1_000 == 0 { format!("compacts at {}k tokens", limit / 1_000) } else { format!("compacts at {limit} tokens") };
        prop_assert!(text.contains(&expected));
    }
}
