//! Settings `[agent_setup]` and the Global file path field (saved after 600 ms, R restores the default).

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::*;

/// REQ: F-037
#[test]
fn settings_037_defaults_are_the_claude_file_never_ask_false_and_no_projects() {
    let mut host = server_host();
    assert_eq!(
        host.settings().get("agent_setup.global_file"),
        Some(json!("~/.claude/CLAUDE.md"))
    );
    assert_eq!(
        host.settings().get("agent_setup.never_ask_global"),
        Some(json!(false))
    );
    assert_eq!(
        host.settings().get("agent_setup.never_ask_projects"),
        Some(json!([]))
    );
}

/// REQ: F-037
#[test]
fn settings_037_per_block_path_keys_default_to_the_global_file() {
    let mut host = server_host();
    assert_eq!(
        host.settings().get("agent_setup.chatroom_global_file"),
        Some(json!("~/.claude/CLAUDE.md"))
    );
    assert_eq!(
        host.settings().get("agent_setup.progress_global_file"),
        Some(json!("~/.claude/CLAUDE.md"))
    );
}

/// REQ: F-037
#[test]
fn settings_037_a_progress_path_applies_only_to_the_progress_block() {
    let mut host = server_host();
    host.settings()
        .set(
            "agent_setup.progress_global_file",
            json!("~/.claude/ALT.md"),
        )
        .expect("set");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("progress");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("chatroom");
    assert!(get(&mut host, claude_alt_file())
        .expect("alt")
        .contains(PROGRESS_OPEN));
    assert!(get(&mut host, claude_file())
        .expect("main")
        .contains(CHATROOM_OPEN));
}

/// REQ: F-037
#[test]
fn settings_037_a_path_edit_is_not_saved_after_599_ms() {
    let mut host = server_host();
    host.type_into_field("setup", "global-file", "~/.claude/ALT.md");
    host.advance_clock(599);
    host.settle();
    assert_eq!(
        host.settings().get("agent_setup.global_file"),
        Some(json!("~/.claude/CLAUDE.md"))
    );
}

/// REQ: F-037
#[test]
fn settings_037_a_path_edit_is_saved_after_600_ms() {
    let mut host = server_host();
    host.type_into_field("setup", "global-file", "~/.claude/ALT.md");
    host.advance_clock(600);
    host.settle();
    assert_eq!(
        host.settings().get("agent_setup.global_file"),
        Some(json!("~/.claude/ALT.md"))
    );
}

/// REQ: F-037
#[test]
fn settings_037_pressing_r_on_the_path_field_restores_the_default() {
    let mut host = server_host();
    host.settings()
        .set("agent_setup.global_file", json!("~/.claude/ALT.md"))
        .expect("set");
    host.press_field_key("setup", "global-file", 'r');
    host.settle();
    assert_eq!(
        host.settings().get("agent_setup.global_file"),
        Some(json!("~/.claude/CLAUDE.md"))
    );
}

/// REQ: F-037
#[test]
fn settings_037_a_path_that_is_not_a_string_is_rejected_with_invalid_argument() {
    let mut host = server_host();
    let error = host
        .settings()
        .set("agent_setup.global_file", json!(5))
        .expect_err("type");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-037
#[test]
fn settings_037_an_empty_path_is_rejected_with_invalid_argument() {
    let mut host = server_host();
    let error = host
        .settings()
        .set("agent_setup.global_file", json!(""))
        .expect_err("empty");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        host.settings().get("agent_setup.global_file"),
        Some(json!("~/.claude/CLAUDE.md"))
    );
}

/// REQ: F-037
#[test]
fn settings_037_never_ask_projects_must_be_a_list_of_strings() {
    let mut host = server_host();
    let error = host
        .settings()
        .set("agent_setup.never_ask_projects", json!([1]))
        .expect_err("type");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-037
#[test]
fn settings_037_an_unknown_key_is_not_found() {
    let mut host = server_host();
    let error = host
        .settings()
        .set("agent_setup.nope", json!(1))
        .expect_err("unknown");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-037
#[test]
fn settings_037_a_saved_path_is_kept_after_the_plugin_is_disabled_and_enabled() {
    let mut host = server_host();
    host.settings()
        .set("agent_setup.global_file", json!("~/.claude/ALT.md"))
        .expect("set");
    host.disable_plugin();
    host.enable_plugin();
    assert_eq!(
        host.settings().get("agent_setup.global_file"),
        Some(json!("~/.claude/ALT.md"))
    );
}
