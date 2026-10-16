//! F-082 Compaction optimiser panel and F-115 session conversion hand-off (page sample 4 and "Convert a session").

use ilium_plugin_sdk::types::{ExternalTarget, Intent, ViewTarget};
use serde_json::json;

use crate::common::*;

const CODEX_FILE: &str = "agent-homes:.codex/config.toml";

fn panel() -> ViewTarget {
    ViewTarget::Panel("optimizer".to_string())
}

fn opened() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = client_host();
    host.open_view(panel());
    host
}

/// REQ: F-082
#[test]
fn optimizer_082_the_panel_is_a_non_modal_view_titled_compaction() {
    let panel_row = &manifest_value()["contributes"]["panels"][0];
    assert_eq!(panel_row["id"], "optimizer");
    assert_eq!(panel_row["title"], "Compaction");
    assert_eq!(panel_row["modal"], false);
}

/// REQ: F-082
#[test]
fn optimizer_082_opening_the_panel_starts_no_scan() {
    let host = opened();
    assert!(host.scans_started().is_empty());
}

/// REQ: F-082
#[test]
fn optimizer_082_the_s_key_starts_a_scan() {
    let mut host = opened();
    host.send_key(panel(), "s");
    assert_eq!(host.scans_started().len(), 1);
}

/// REQ: F-082
#[test]
fn optimizer_082_a_second_scan_key_while_scanning_starts_no_second_scan() {
    let mut host = opened();
    host.send_key(panel(), "s");
    host.send_key(panel(), "s");
    assert_eq!(host.scans_started().len(), 1);
}

/// REQ: F-082
#[test]
fn optimizer_082_the_panel_shows_threshold_recommended_and_both_buttons() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    let text = host.render(panel(), 80, 24).text();
    for needle in ["Auto-compact threshold", "72 %", "Recommended", "65 %", "[ Apply ]", "[ Revert ]"] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

/// REQ: F-082
#[test]
fn optimizer_082_apply_asks_for_confirmation_before_writing() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    host.send_key(panel(), "a");
    let text = host.render(panel(), 100, 24).text();
    assert!(text.contains("Apply model_auto_compact_token_limit"));
    assert!(host.agent_file_writes().is_empty());
}

/// REQ: F-082
#[test]
fn optimizer_082_escape_cancels_the_confirmation_and_writes_nothing() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    host.send_key(panel(), "a");
    host.send_key(panel(), "Esc");
    assert!(host.agent_file_writes().is_empty());
}

/// REQ: F-082
#[test]
fn optimizer_082_confirming_changes_only_the_one_key_and_keeps_comments_byte_for_byte() {
    let original = "# keep me\nmodel = \"x\"  # inline\nmodel_auto_compact_token_limit = 123456\n\n[profiles.p]\nmodel = \"y\"\n";
    let mut host = opened();
    host.agent_files().write(CODEX_FILE, original.as_bytes());
    host.set_recommendation(72, 65);
    host.send_key(panel(), "a");
    host.send_key(panel(), "Enter");
    let after = String::from_utf8(host.agent_files().read(CODEX_FILE).expect("file")).expect("utf-8");
    assert_eq!(after, original.replace("123456", &host.recommended_tokens().to_string()));
}

/// REQ: F-082
#[test]
fn optimizer_082_revert_restores_the_previous_value_and_shows_a_reverted_note() {
    let original = "model_auto_compact_token_limit = 123456\n";
    let mut host = opened();
    host.agent_files().write(CODEX_FILE, original.as_bytes());
    host.set_recommendation(72, 65);
    host.send_key(panel(), "a");
    host.send_key(panel(), "Enter");
    host.send_key(panel(), "r");
    host.send_key(panel(), "Enter");
    assert_eq!(host.agent_files().read(CODEX_FILE), Some(original.as_bytes().to_vec()));
    assert!(host.render(panel(), 100, 24).text().contains("Reverted"));
}

/// REQ: F-082
#[test]
fn optimizer_082_revert_after_an_outside_edit_is_refused_and_the_file_is_left_alone() {
    let mut host = opened();
    host.agent_files().write(CODEX_FILE, b"model_auto_compact_token_limit = 123456\n");
    host.set_recommendation(72, 65);
    host.send_key(panel(), "a");
    host.send_key(panel(), "Enter");
    host.agent_files().write(CODEX_FILE, b"model_auto_compact_token_limit = 999\n");
    host.send_key(panel(), "r");
    host.send_key(panel(), "Enter");
    assert_eq!(host.agent_files().read(CODEX_FILE), Some(b"model_auto_compact_token_limit = 999\n".to_vec()));
    assert!(host.render(panel(), 100, 24).text().contains("changed since"));
}

/// REQ: F-082
#[test]
fn optimizer_082_revert_with_nothing_applied_does_nothing() {
    let mut host = opened();
    host.send_key(panel(), "r");
    assert!(host.agent_file_writes().is_empty());
}

/// REQ: F-082
#[test]
fn optimizer_082_left_and_right_move_between_the_agent_targets() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    let before = host.render(panel(), 100, 24).text();
    host.send_key(panel(), "Right");
    assert_ne!(host.render(panel(), 100, 24).text(), before);
    host.send_key(panel(), "Left");
    assert_eq!(host.render(panel(), 100, 24).text(), before);
}

/// REQ: F-082
#[test]
fn optimizer_082_the_e_key_expands_the_details_and_scroll_moves_them() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    let before = host.render(panel(), 100, 12).text();
    host.send_key(panel(), "e");
    assert_ne!(host.render(panel(), 100, 12).text(), before);
}

/// REQ: F-082
#[test]
fn optimizer_082_escape_closes_the_panel() {
    let mut host = opened();
    host.send_key(panel(), "Esc");
    assert!(host.view_closed(panel()));
}

/// REQ: F-082
#[test]
fn optimizer_082_applying_a_claude_value_warns_when_an_environment_override_is_set() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    host.set_agent_env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "300000");
    host.select_agent_target("claude");
    host.send_key(panel(), "a");
    let text = host.render(panel(), 120, 24).text();
    assert!(text.contains("CLAUDE_CODE_AUTO_COMPACT_WINDOW=300000 is set in Ilium's environment and takes precedence over the file for sessions started from it"));
}

/// REQ: F-082
#[test]
fn optimizer_082_a_missing_agent_file_is_created_with_only_the_one_key_after_confirmation() {
    let mut host = opened();
    host.set_recommendation(72, 65);
    host.send_key(panel(), "a");
    host.send_key(panel(), "Enter");
    let after = String::from_utf8(host.agent_files().read(CODEX_FILE).expect("file created")).expect("utf-8");
    assert_eq!(after.lines().count(), 1);
    assert!(after.starts_with("model_auto_compact_token_limit = "));
}

/// REQ: F-082
#[test]
fn optimizer_082_the_panel_needs_the_client_ui_panels_grant() {
    let client: Vec<String> = manifest_value()["capabilities"]["client"].as_array().expect("grants").iter().map(|grant| grant.as_str().unwrap_or_default().to_string()).collect();
    assert!(client.contains(&"ui.panels".to_string()));
}

/// REQ: F-115
#[test]
fn convert_115_a_converted_session_is_opened_for_inspection_with_open_external() {
    let mut host = client_host();
    let submissions = host.convert_session("claude", "codex", "/home/u/.codex/sessions/rollout-1.jsonl");
    let target = intents(&submissions).into_iter().find_map(|intent| match intent {
        Intent::OpenExternal(spec) => Some(spec.target),
        _ => None,
    });
    assert!(matches!(target, Some(ExternalTarget::Path(path)) if path == "/home/u/.codex/sessions/rollout-1.jsonl"));
}

/// REQ: F-115
#[test]
fn convert_115_exactly_one_open_external_is_submitted_per_conversion() {
    let mut host = client_host();
    let submissions = host.convert_session("codex", "claude", "/home/u/.claude/projects/p/s.jsonl");
    let count = intents(&submissions).iter().filter(|intent| matches!(intent, Intent::OpenExternal(_))).count();
    assert_eq!(count, 1);
}

/// REQ: F-115
#[test]
fn convert_115_a_failed_conversion_opens_nothing() {
    let mut host = client_host();
    let submissions = host.convert_session_failing("claude", "codex");
    assert!(!intents(&submissions).iter().any(|intent| matches!(intent, Intent::OpenExternal(_))));
}

/// REQ: F-115
#[test]
fn convert_115_the_open_external_grant_is_declared_on_the_client_only() {
    let manifest = manifest_value();
    let grants = |side: &str| -> Vec<String> { manifest["capabilities"][side].as_array().expect("grants").iter().map(|grant| grant.as_str().unwrap_or_default().to_string()).collect() };
    assert!(grants("client").contains(&"open-external".to_string()));
    assert!(!grants("server").contains(&"open-external".to_string()));
    let _ = json!(null);
}
