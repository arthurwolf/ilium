//! The service `ilium.core.agent-setup/install` (methods install, remove, status): parameter validation,
//! project targets, and the Chatroom side effects (`CHATROOM.md` and the hooks that deliver messages).

use ilium_plugin_sdk::types::ErrorCode;

use crate::common::*;

fn raw(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    method: &str,
    payload: &str,
) -> Result<String, ilium_plugin_sdk::types::PluginError> {
    host.call_service(SERVICE, method, payload)
}

/// REQ: F-063
#[test]
fn service_063_install_without_a_block_is_invalid_argument() {
    let mut host = server_host();
    let error =
        raw(&mut host, "install", r#"{"target":"claude-global"}"#).expect_err("block is required");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-063
#[test]
fn service_063_install_without_a_target_is_invalid_argument() {
    let mut host = server_host();
    let error =
        raw(&mut host, "install", r#"{"block":"progress"}"#).expect_err("target is required");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-063
#[test]
fn service_063_remove_without_a_block_or_target_is_invalid_argument() {
    let mut host = server_host();
    assert_eq!(
        raw(&mut host, "remove", "{}").expect_err("params").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-063
#[test]
fn service_063_a_block_that_is_not_a_string_is_invalid_argument() {
    let mut host = server_host();
    let error = raw(
        &mut host,
        "install",
        r#"{"block":7,"target":"claude-global"}"#,
    )
    .expect_err("type");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-063
#[test]
fn service_063_a_payload_that_is_not_json_is_invalid_argument() {
    let mut host = server_host();
    assert_eq!(
        raw(&mut host, "install", "not json")
            .expect_err("json")
            .code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-063
#[test]
fn service_063_a_payload_that_is_not_an_object_is_invalid_argument() {
    let mut host = server_host();
    assert_eq!(
        raw(&mut host, "status", "[1]").expect_err("object").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-063
#[test]
fn service_063_an_unknown_block_is_not_found_and_writes_nothing() {
    let mut host = server_host();
    let error = install(&mut host, "review-rules", CLAUDE_GLOBAL).expect_err("unknown block");
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(get(&mut host, claude_file()), None);
}

/// REQ: F-063
#[test]
fn service_063_an_unknown_target_is_not_found_and_writes_nothing() {
    let mut host = server_host();
    let error = install(&mut host, "progress", "nowhere").expect_err("unknown target");
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(get(&mut host, claude_file()), None);
}

/// REQ: F-063, F-037
#[test]
fn service_063_a_project_target_without_an_open_project_is_not_found() {
    let mut host = server_host();
    let error = install(&mut host, "progress", CLAUDE_PROJECT).expect_err("no project");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-063
#[test]
fn service_063_status_accepts_an_empty_object_and_returns_the_targets_array() {
    let mut host = server_host();
    let text = raw(&mut host, "status", "{}").expect("status");
    let reply: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert!(reply["targets"].is_array());
}

/// REQ: F-063, F-037
#[test]
fn service_063_install_into_the_project_claude_file_creates_it_with_the_block() {
    let mut host = project_host();
    install(&mut host, "progress", CLAUDE_PROJECT).expect("install");
    assert!(get(&mut host, project_claude_file())
        .expect("file")
        .contains(PROGRESS_OPEN));
    assert_eq!(get(&mut host, project_agents_file()), None);
}

/// REQ: F-063, F-037
#[test]
fn service_063_install_into_the_project_agents_file_leaves_the_claude_file_alone() {
    let mut host = project_host();
    install(&mut host, "progress", CODEX_PROJECT).expect("install");
    assert!(get(&mut host, project_agents_file())
        .expect("file")
        .contains(PROGRESS_OPEN));
    assert_eq!(get(&mut host, project_claude_file()), None);
}

/// REQ: F-063, F-037
#[test]
fn service_063_remove_from_a_project_file_keeps_the_project_text() {
    let mut host = project_host();
    put(&mut host, project_claude_file(), "# Project\n");
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("install");
    call(&mut host, "remove", "chatroom", CLAUDE_PROJECT).expect("remove");
    assert_eq!(
        get(&mut host, project_claude_file()).as_deref(),
        Some("# Project\n")
    );
}

/// REQ: F-063
#[test]
fn service_063_chatroom_setup_in_a_project_also_creates_chatroom_md() {
    let mut host = project_host();
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("install");
    assert!(
        get(&mut host, chatroom_file()).is_some(),
        "CHATROOM.md exists"
    );
}

/// REQ: F-063
#[test]
fn service_063_progress_setup_does_not_create_chatroom_md() {
    let mut host = project_host();
    install(&mut host, "progress", CLAUDE_PROJECT).expect("install");
    assert_eq!(get(&mut host, chatroom_file()), None);
}

/// REQ: F-063
#[test]
fn service_063_chatroom_setup_keeps_an_existing_chatroom_md_byte_for_byte() {
    let mut host = project_host();
    put(
        &mut host,
        chatroom_file(),
        "2026-10-13 10:00:00 user hello\n",
    );
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("install");
    assert_eq!(
        get(&mut host, chatroom_file()).as_deref(),
        Some("2026-10-13 10:00:00 user hello\n")
    );
}

/// REQ: F-063
#[test]
fn service_063_chatroom_setup_installs_the_hook_command_that_delivers_messages() {
    let mut host = project_host();
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("install");
    let paths = host.fs().all_paths();
    let mut found = false;
    for path in paths {
        if let Some(text) = get(&mut host, path) {
            found |=
                text.contains("ilium chat context --limit 40 --since-last-read --max-bytes 2048");
        }
    }
    assert!(found, "no file holds the chatroom hook command");
}

/// REQ: F-063
#[test]
fn service_063_the_chatroom_hook_has_a_ten_second_timeout_and_the_status_message() {
    let mut host = project_host();
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("install");
    let mut hook_text = String::new();
    for path in host.fs().all_paths() {
        if let Some(text) = get(&mut host, path) {
            if text.contains("ilium chat context") && text.contains("Checking ilium chatroom") {
                hook_text = text;
            }
        }
    }
    let value: serde_json::Value = serde_json::from_str(&hook_text).expect("the hook file is JSON");
    assert!(value.to_string().contains("\"timeout\":10"), "{hook_text}");
    assert!(hook_text.contains("Checking ilium chatroom"));
}

/// REQ: F-063
#[test]
fn service_063_installing_the_chatroom_hook_twice_does_not_duplicate_it() {
    let mut host = project_host();
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("first");
    install(&mut host, "chatroom", CLAUDE_PROJECT).expect("second");
    let mut count = 0;
    for path in host.fs().all_paths() {
        if let Some(text) = get(&mut host, path) {
            count += text.matches("--since-last-read --max-bytes 2048").count();
        }
    }
    assert_eq!(count, 1);
}

/// REQ: F-063
#[test]
fn service_063_the_install_reply_is_a_json_object_with_only_the_status_key() {
    let mut host = server_host();
    let reply = install(&mut host, "chatroom", CODEX_GLOBAL).expect("install");
    assert_eq!(
        reply
            .as_object()
            .expect("object")
            .keys()
            .collect::<Vec<_>>(),
        ["status"]
    );
}
