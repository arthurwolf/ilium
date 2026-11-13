//! The three extension points: `instruction-blocks` (collect), `targets` (collect) and `before-install`
//! (veto hook, high risk). Other plugins add blocks and targets; a policy plugin can refuse a write.

use ilium_plugin_sdk::exports::HookReply;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, PluginError, PluginId, TopicPublishedEvent};
use serde_json::{json, Value};

use crate::common::*;

const BLOCKS: &str = "ilium.core.agent-setup/instruction-blocks";
const TARGETS: &str = "ilium.core.agent-setup/targets";
const BEFORE_INSTALL: &str = "ilium.core.agent-setup/before-install";

fn extensions_changed(host: &mut FakeHost) {
    host.deliver_one(Event::TopicPublished(TopicPublishedEvent {
        topic: "ilium.extensions/changed".to_string(),
        from: PluginId("ilium.kernel".to_string()),
        payload_json: "{}".to_string(),
    }));
    host.settle();
}

fn review_block(host: &mut FakeHost) {
    host.provide_help_file(
        "help/review-rules.md",
        "Review every change before it is merged.\n",
    );
    host.contribute_extension(
        BLOCKS,
        "review-rules",
        50,
        json!({ "id": "review-rules", "title": "Review rules", "marker": "review-rules", "file": "help/review-rules.md" }),
    );
    extensions_changed(host);
}

fn gemini_target(host: &mut FakeHost) {
    host.contribute_extension(
        TARGETS,
        "gemini-project",
        50,
        json!({ "id": "gemini-project", "title": "Gemini", "path_template": "{project}/GEMINI.md", "scope": "project" }),
    );
    extensions_changed(host);
}

fn gemini_file() -> ilium_plugin_sdk::types::FsPath {
    ilium_plugin_sdk::types::FsPath {
        scope: "project".to_string(),
        path: "GEMINI.md".to_string(),
    }
}

fn blocks_listed(host: &mut FakeHost) -> Vec<String> {
    let mut names: Vec<String> = status_rows(host)
        .iter()
        .map(|row| row["block"].as_str().expect("block").to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// REQ: F-063
#[test]
fn points_063_without_contributions_only_the_chatroom_and_progress_blocks_are_listed() {
    let mut host = server_host();
    assert_eq!(blocks_listed(&mut host), ["chatroom", "progress"]);
}

/// REQ: F-063
#[test]
fn points_063_a_contributed_block_appears_next_to_chatroom_and_progress() {
    let mut host = server_host();
    review_block(&mut host);
    assert_eq!(
        blocks_listed(&mut host),
        ["chatroom", "progress", "review-rules"]
    );
}

/// REQ: F-063
#[test]
fn points_063_a_contributed_block_is_installed_between_its_own_markers_with_the_text_of_its_file() {
    let mut host = server_host();
    review_block(&mut host);
    install(&mut host, "review-rules", CLAUDE_GLOBAL).expect("install");
    let text = get(&mut host, claude_file()).expect("file");
    assert!(
        text.contains("<!-- ilium-agent-feature: review-rules -->"),
        "{text}"
    );
    assert!(
        text.contains("<!-- /ilium-agent-feature: review-rules -->"),
        "{text}"
    );
    assert!(block_of(&text, "review-rules").contains("Review every change before it is merged."));
}

/// REQ: F-063
#[test]
fn points_063_a_contributed_block_does_not_disturb_the_other_blocks_of_the_file() {
    let mut host = server_host();
    review_block(&mut host);
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("progress");
    let before = block_of(&get(&mut host, claude_file()).expect("file"), "progress");
    install(&mut host, "review-rules", CLAUDE_GLOBAL).expect("review");
    assert_eq!(
        block_of(&get(&mut host, claude_file()).expect("file"), "progress"),
        before
    );
    assert_eq!(
        status_of(&mut host, "review-rules", CLAUDE_GLOBAL),
        "set-up"
    );
}

/// REQ: F-063
#[test]
fn points_063_a_contributed_block_can_be_removed_without_touching_the_others() {
    let mut host = server_host();
    review_block(&mut host);
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("chatroom");
    install(&mut host, "review-rules", CLAUDE_GLOBAL).expect("review");
    call(&mut host, "remove", "review-rules", CLAUDE_GLOBAL).expect("remove");
    let text = get(&mut host, claude_file()).expect("file");
    assert!(!text.contains("review-rules"));
    assert!(text.contains(CHATROOM_OPEN));
}

/// REQ: F-063
#[test]
fn points_063_the_block_list_is_read_again_when_the_kernel_publishes_extensions_changed() {
    let mut host = server_host();
    review_block(&mut host);
    assert!(blocks_listed(&mut host).contains(&"review-rules".to_string()));
    host.withdraw_extension(BLOCKS, "review-rules");
    extensions_changed(&mut host);
    assert_eq!(blocks_listed(&mut host), ["chatroom", "progress"]);
}

/// REQ: F-063
#[test]
fn points_063_a_block_entry_without_a_marker_is_ignored() {
    let mut host = server_host();
    host.contribute_extension(
        BLOCKS,
        "broken",
        50,
        json!({ "id": "broken", "title": "Broken", "file": "help/x.md" }),
    );
    extensions_changed(&mut host);
    assert_eq!(blocks_listed(&mut host), ["chatroom", "progress"]);
}

/// REQ: F-063
#[test]
fn points_063_a_broken_entry_does_not_hide_the_good_ones() {
    let mut host = server_host();
    host.contribute_extension(BLOCKS, "broken", 10, json!("not an object"));
    review_block(&mut host);
    assert_eq!(
        blocks_listed(&mut host),
        ["chatroom", "progress", "review-rules"]
    );
}

/// REQ: F-063
#[test]
fn points_063_at_most_eight_blocks_are_listed() {
    let mut host = server_host();
    for index in 0..9 {
        host.contribute_extension(
            BLOCKS,
            &format!("extra-{index}"),
            50,
            json!({ "id": format!("extra-{index}"), "title": "Extra", "marker": format!("extra-{index}"), "file": "help/x.md" }),
        );
    }
    extensions_changed(&mut host);
    assert!(blocks_listed(&mut host).len() <= 8);
}

/// REQ: F-063, F-037
#[test]
fn points_063_a_contributed_project_target_adds_a_row_per_block() {
    let mut host = project_host();
    gemini_target(&mut host);
    let rows = status_rows(&mut host);
    let gemini: Vec<&Value> = rows
        .iter()
        .filter(|row| row["target"] == "gemini-project")
        .collect();
    assert_eq!(gemini.len(), 2);
    assert!(gemini
        .iter()
        .all(|row| row["scope"] == "project" && row["status"] == "not-set-up"));
    assert!(gemini[0]["path"]
        .as_str()
        .expect("path")
        .ends_with("GEMINI.md"));
}

/// REQ: F-063, F-037
#[test]
fn points_063_a_block_installed_into_a_contributed_target_lands_in_the_templated_file() {
    let mut host = project_host();
    gemini_target(&mut host);
    install(&mut host, "progress", "gemini-project").expect("install");
    assert!(get(&mut host, gemini_file())
        .expect("GEMINI.md")
        .contains(PROGRESS_OPEN));
}

/// REQ: F-063
#[test]
fn points_063_a_target_with_a_scope_outside_global_and_project_is_ignored() {
    let mut host = project_host();
    host.contribute_extension(
        TARGETS,
        "odd",
        50,
        json!({ "id": "odd", "title": "Odd", "path_template": "{project}/ODD.md", "scope": "everywhere" }),
    );
    extensions_changed(&mut host);
    assert!(status_rows(&mut host)
        .iter()
        .all(|row| row["target"] != "odd"));
}

/// REQ: F-063
#[test]
fn points_063_at_most_sixteen_targets_are_listed() {
    let mut host = project_host();
    for index in 0..17 {
        host.contribute_extension(
            TARGETS,
            &format!("t-{index}"),
            50,
            json!({ "id": format!("t-{index}"), "title": "T", "path_template": format!("{{project}}/T{index}.md"), "scope": "project" }),
        );
    }
    extensions_changed(&mut host);
    let mut targets: Vec<String> = status_rows(&mut host)
        .iter()
        .map(|row| row["target"].as_str().expect("t").to_string())
        .collect();
    targets.sort();
    targets.dedup();
    assert!(targets.len() <= 16 + 4, "{targets:?}");
    assert!(targets.iter().filter(|name| name.starts_with("t-")).count() <= 16);
}

/// REQ: F-063
#[test]
fn points_063_the_veto_hook_runs_once_before_a_write_with_target_block_and_path() {
    let mut host = server_host();
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let calls = host.hook_payloads(BEFORE_INSTALL);
    assert_eq!(calls.len(), 1);
    let payload: Value = serde_json::from_str(&calls[0]).expect("payload is JSON");
    assert_eq!(
        payload,
        json!({ "target": CLAUDE_GLOBAL, "block": "progress", "path": "~/.claude/CLAUDE.md" })
    );
}

/// REQ: F-063
#[test]
fn points_063_the_veto_hook_also_runs_before_a_removal() {
    let mut host = server_host();
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect("remove");
    assert_eq!(host.hook_payloads(BEFORE_INSTALL).len(), 2);
}

/// REQ: F-063
#[test]
fn points_063_the_veto_hook_does_not_run_for_a_status_call() {
    let mut host = server_host();
    status_rows(&mut host);
    assert!(host.hook_payloads(BEFORE_INSTALL).is_empty());
}

/// REQ: F-063
#[test]
fn points_063_a_refusal_writes_nothing_and_the_call_fails_with_the_reason() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Team file\n");
    host.set_hook_reply(
        BEFORE_INSTALL,
        HookReply::Refuse("shared team file".to_string()),
    );
    let error = install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("vetoed");
    assert!(
        error.message.contains("shared team file"),
        "{}",
        error.message
    );
    assert_eq!(
        get(&mut host, claude_file()).as_deref(),
        Some("# Team file\n")
    );
}

/// REQ: F-063
#[test]
fn points_063_a_refusal_shows_the_notice_install_stopped_with_the_reason() {
    let mut host = server_host();
    host.set_hook_reply(
        BEFORE_INSTALL,
        HookReply::Refuse("shared team file".to_string()),
    );
    let _ = install(&mut host, "progress", CLAUDE_GLOBAL);
    host.settle();
    let submitted = format!("{:?}", host.submitted());
    assert!(submitted.contains("Install stopped: "), "{submitted}");
    assert!(submitted.contains("shared team file"), "{submitted}");
}

/// REQ: F-063
#[test]
fn points_063_a_pass_reply_lets_the_write_happen() {
    let mut host = server_host();
    host.set_hook_reply(BEFORE_INSTALL, HookReply::Pass);
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert!(get(&mut host, claude_file())
        .expect("file")
        .contains(PROGRESS_OPEN));
}

/// REQ: F-063
#[test]
fn points_063_a_refusal_for_one_target_does_not_block_another_target() {
    let mut host = server_host();
    host.set_hook_reply_when(
        BEFORE_INSTALL,
        json!({ "target": CLAUDE_GLOBAL }),
        HookReply::Refuse("no".to_string()),
    );
    install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("vetoed");
    install(&mut host, "progress", CODEX_GLOBAL).expect("allowed");
    assert_eq!(get(&mut host, claude_file()), None);
    assert!(get(&mut host, codex_file()).is_some());
}

/// REQ: F-063
#[test]
fn points_063_a_check_that_does_not_finish_stops_the_install_with_its_own_notice() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Mine\n");
    host.inject_failure(
        "extensions.run-hook",
        PluginError {
            code: ErrorCode::TimedOut,
            message: "deadline".to_string(),
            retry_after_ms: None,
        },
    );
    let _ = install(&mut host, "progress", CLAUDE_GLOBAL);
    host.settle();
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some("# Mine\n"));
    assert!(format!("{:?}", host.submitted()).contains("Install stopped: the check did not finish"));
}

/// REQ: F-063
#[test]
fn points_063_the_automatic_refresh_also_asks_the_veto_hook_and_respects_a_refusal() {
    let mut host = server_host();
    host.set_hook_reply(BEFORE_INSTALL, HookReply::Refuse("frozen".to_string()));
    host.deliver_one(Event::ClientAttached(ilium_plugin_sdk::types::ClientId(1)));
    host.advance_clock(2_000);
    host.settle();
    assert_eq!(get(&mut host, claude_file()), None);
    assert_eq!(get(&mut host, codex_file()), None);
}
