//! The five statuses of a block in a target file (page "Setup tab: statuses, rows and files" and the sample
//! "Read the status of a target"), reported by the service method `status` with the words
//! set-up, update-required, newer, detected and not-set-up.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::*;

fn host_with(text: &str) -> FakeHost {
    let mut host = server_host();
    put(&mut host, claude_file(), text);
    host
}

fn progress_status(text: &str) -> String {
    status_of(&mut host_with(text), "progress", CLAUDE_GLOBAL)
}

fn chatroom_status(text: &str) -> String {
    status_of(&mut host_with(text), "chatroom", CLAUDE_GLOBAL)
}

/// REQ: F-063
#[test]
fn status_063_a_missing_file_is_not_set_up_for_both_blocks() {
    let mut host = server_host();
    assert_eq!(status_of(&mut host, "chatroom", CLAUDE_GLOBAL), "not-set-up");
    assert_eq!(status_of(&mut host, "progress", CLAUDE_GLOBAL), "not-set-up");
}

/// REQ: F-063
#[test]
fn status_063_an_empty_file_is_not_set_up() {
    assert_eq!(progress_status(""), "not-set-up");
    assert_eq!(chatroom_status(""), "not-set-up");
}

/// REQ: F-063
#[test]
fn status_063_the_current_chatroom_block_is_set_up() {
    let mut host = host_with("# Existing\n");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    assert_eq!(status_of(&mut host, "chatroom", CLAUDE_GLOBAL), "set-up");
}

/// REQ: F-063
#[test]
fn status_063_the_current_progress_block_is_set_up() {
    let mut host = host_with("# Existing\n");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert_eq!(status_of(&mut host, "progress", CLAUDE_GLOBAL), "set-up");
}

/// REQ: F-063
#[test]
fn status_063_a_version_twelve_progress_block_is_update_required_with_installed_and_current_versions() {
    let mut host = host_with(&old_progress_block(12));
    let rows = status_rows(&mut host);
    let row = rows.iter().find(|row| row["block"] == "progress" && row["target"] == CLAUDE_GLOBAL).expect("row");
    assert_eq!(row["status"], "update-required");
    assert_eq!(row["installed"], 12);
    assert_eq!(row["current"], 13);
}

/// REQ: F-063
#[test]
fn status_063_progress_versions_above_thirteen_are_newer() {
    assert_eq!(progress_status(&old_progress_block(14)), "newer");
    assert_eq!(progress_status(&old_progress_block(99)), "newer");
}

/// REQ: F-063
#[test]
fn status_063_a_hand_edited_version_thirteen_block_is_update_required() {
    assert_eq!(progress_status(&old_progress_block(13)), "update-required");
}

/// REQ: F-063
#[test]
fn status_063_a_progress_block_without_a_version_is_update_required() {
    let text = "<!-- ilium-agent-feature: progress -->\nUse ilium progress as a progress monitor.\n<!-- /ilium-agent-feature: progress -->\n";
    assert_eq!(progress_status(text), "update-required");
}

/// REQ: F-063
#[test]
fn status_063_an_empty_managed_block_is_update_required() {
    let text = "<!-- ilium-agent-feature: progress -->\n<!-- /ilium-agent-feature: progress -->\n";
    assert_eq!(progress_status(text), "update-required");
}

/// REQ: F-063
#[test]
fn status_063_look_alike_progress_text_without_markers_is_detected() {
    assert_eq!(progress_status(PROGRESS_LOOK_ALIKE), "detected");
}

/// REQ: F-063
#[test]
fn status_063_look_alike_chatroom_text_without_markers_is_detected() {
    assert_eq!(chatroom_status(CHATROOM_LOOK_ALIKE), "detected");
}

/// REQ: F-063
#[test]
fn status_063_detection_does_not_depend_on_case() {
    assert_eq!(progress_status(&PROGRESS_LOOK_ALIKE.to_uppercase()), "detected");
}

/// REQ: F-063
#[test]
fn status_063_detection_does_not_depend_on_line_order() {
    let reversed: String = PROGRESS_LOOK_ALIKE.lines().rev().map(|line| format!("{line}\n")).collect();
    assert_eq!(progress_status(&reversed), "detected");
}

/// REQ: F-063
#[test]
fn status_063_detection_needs_every_progress_signal() {
    for missing in ["sole recurring poller", "at least three minutes", "MUST NOT poll", "ilium progress check"] {
        let reduced = PROGRESS_LOOK_ALIKE.replace(missing, "xxxx");
        assert_eq!(progress_status(&reduced), "not-set-up", "without {missing:?}");
    }
}

/// REQ: F-063
#[test]
fn status_063_detection_needs_both_chatroom_signals() {
    assert_eq!(chatroom_status("Read CHATROOM.md before you start.\n"), "not-set-up");
    assert_eq!(chatroom_status("Use ilium chat send for claims.\n"), "not-set-up");
}

/// REQ: F-063
#[test]
fn status_063_an_obsolete_unmanaged_instruction_is_not_set_up() {
    let text = "Use ilium progress with an absolute-path command and clear it when finished.\n";
    assert_eq!(progress_status(text), "not-set-up");
}

/// REQ: F-063
#[test]
fn status_063_a_legacy_chatroom_guidance_block_counts_as_set_up() {
    let text = "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n";
    assert_eq!(chatroom_status(text), "set-up");
}

/// REQ: F-063
#[test]
fn status_063_look_alike_text_is_reported_but_the_file_is_not_touched() {
    let mut host = host_with(PROGRESS_LOOK_ALIKE);
    let _ = status_rows(&mut host);
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some(PROGRESS_LOOK_ALIKE));
}

/// REQ: F-063
#[test]
fn status_063_the_two_blocks_have_independent_statuses_in_one_file() {
    let mut host = host_with(&old_progress_block(12));
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    assert_eq!(status_of(&mut host, "chatroom", CLAUDE_GLOBAL), "set-up");
    assert_eq!(status_of(&mut host, "progress", CLAUDE_GLOBAL), "update-required");
}

/// REQ: F-063, F-037
#[test]
fn status_063_each_global_file_has_its_own_status() {
    let mut host = host_with("");
    install(&mut host, "progress", CODEX_GLOBAL).expect("install");
    assert_eq!(status_of(&mut host, "progress", CLAUDE_GLOBAL), "not-set-up");
    assert_eq!(status_of(&mut host, "progress", CODEX_GLOBAL), "set-up");
}

/// REQ: F-063, F-037
#[test]
fn status_063_rows_carry_block_target_scope_path_and_status() {
    let mut host = host_with("");
    let rows = status_rows(&mut host);
    let row = rows.iter().find(|row| row["block"] == "chatroom" && row["target"] == CLAUDE_GLOBAL).expect("row");
    assert_eq!(row["scope"], "global");
    assert_eq!(row["path"], "~/.claude/CLAUDE.md");
    assert_eq!(row["status"], "not-set-up");
    let codex = rows.iter().find(|row| row["block"] == "progress" && row["target"] == CODEX_GLOBAL).expect("row");
    assert_eq!(codex["path"], "~/.codex/AGENTS.md");
}

/// REQ: F-063, F-037
#[test]
fn status_063_without_an_open_project_the_status_lists_four_global_rows() {
    let mut host = server_host();
    assert_eq!(status_rows(&mut host).len(), 4);
}

/// REQ: F-063, F-037
#[test]
fn status_063_an_open_project_adds_a_claude_and_an_agents_row_per_block() {
    let mut host = project_host();
    let rows = status_rows(&mut host);
    assert_eq!(rows.len(), 8);
    for target in [CLAUDE_PROJECT, CODEX_PROJECT] {
        for block in ["chatroom", "progress"] {
            let row = rows.iter().find(|row| row["block"] == block && row["target"] == target).expect("project row");
            assert_eq!(row["scope"], "project");
            assert_eq!(row["status"], "not-set-up");
        }
    }
}

/// REQ: F-037
#[test]
fn status_037_project_rows_name_the_files_in_the_project_root() {
    let mut host = project_host();
    let rows = status_rows(&mut host);
    let claude = rows.iter().find(|row| row["target"] == CLAUDE_PROJECT).expect("claude row");
    let agents = rows.iter().find(|row| row["target"] == CODEX_PROJECT).expect("agents row");
    assert!(claude["path"].as_str().expect("path").ends_with("CLAUDE.md"));
    assert!(agents["path"].as_str().expect("path").ends_with("AGENTS.md"));
}

/// REQ: F-063
#[test]
fn status_063_a_malformed_marker_makes_the_status_fail_with_invalid_argument() {
    let mut host = host_with("<!-- ilium-agent-feature: progress version=abc -->\nx\n<!-- /ilium-agent-feature: progress -->\n");
    let error = host.call_service(SERVICE, "status", "{}").expect_err("status fails");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-063
#[test]
fn status_063_the_status_words_are_exactly_the_five_documented_ones() {
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(progress_status(""));
    seen.insert(progress_status(&old_progress_block(12)));
    seen.insert(progress_status(&old_progress_block(99)));
    seen.insert(progress_status(PROGRESS_LOOK_ALIKE));
    let mut host = host_with("");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    seen.insert(status_of(&mut host, "progress", CLAUDE_GLOBAL));
    let words: Vec<&str> = seen.iter().map(String::as_str).collect();
    assert_eq!(words, ["detected", "newer", "not-set-up", "set-up", "update-required"]);
}

/// REQ: F-063
#[test]
fn status_063_install_reply_carries_the_status_word_of_the_result() {
    let mut host = host_with("");
    let reply = install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert_eq!(reply, json!({ "status": "set-up" }));
}

/// REQ: F-063
#[test]
fn status_063_remove_reply_carries_not_set_up() {
    let mut host = host_with("");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let reply = call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect("remove");
    assert_eq!(reply, json!({ "status": "not-set-up" }));
}
