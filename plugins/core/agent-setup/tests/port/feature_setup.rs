//! Twins of the tests of ilium-client/src/agent_feature_setup.rs (F-037, F-063): install, remove and status of
//! the two managed instruction blocks through the install service of the plugin.

use ilium_plugin_sdk::types::ErrorCode;

use crate::common::*;

const PROGRESS_OPEN: &str = "<!-- ilium-agent-feature: progress version=13 -->";
const CHATROOM_OPEN: &str = "<!-- ilium-agent-feature: chatroom";

fn claude_text(host: &mut ilium_plugin_sdk::testing::FakeHost) -> String {
    text_of(host, claude_file())
}

fn refused(host: &mut ilium_plugin_sdk::testing::FakeHost, block: &str, target: &str) {
    assert_eq!(status_of(host, block, target).expect_err("status refuses").code, ErrorCode::InvalidArgument);
    assert_eq!(install(host, block, target).expect_err("install refuses").code, ErrorCode::InvalidArgument);
    assert_eq!(remove(host, block, target).expect_err("remove refuses").code, ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::install_reports_managed_and_replaces_old_copy
#[test]
fn port_fs_01_install_reports_managed_and_replaces_old_copy() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Existing\n");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert_eq!(status_ok(&mut host, "progress", CLAUDE_GLOBAL), "set-up");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install again");
    let text = claude_text(&mut host);
    assert_eq!(text.matches("ilium-agent-feature: progress").count(), 2);
    assert!(text.contains("# Existing"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::install_repairs_duplicate_managed_blocks_to_one_current_copy
#[test]
fn port_fs_02_install_repairs_duplicate_managed_blocks_to_one_current_copy() {
    let mut host = server_host();
    let block = "<!-- ilium-agent-feature: progress -->\nold\n<!-- /ilium-agent-feature: progress -->\n";
    put(&mut host, claude_file(), &format!("{block}\n{block}"));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert_eq!(text.matches("ilium-agent-feature: progress").count(), 2);
    assert!(!text.contains("\nold\n"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::stale_managed_progress_copy_is_detected_and_replaced_in_place
#[test]
fn port_fs_03_stale_managed_progress_copy_is_detected_and_replaced_in_place() {
    let mut host = server_host();
    put(
        &mut host,
        codex_file(),
        "<!-- ilium-agent-feature: progress -->\nUse ilium progress as a progress monitor and clear it when finished.\n<!-- /ilium-agent-feature: progress -->\n",
    );
    assert_eq!(status_ok(&mut host, "progress", CODEX_GLOBAL), "update-required");
    install(&mut host, "progress", CODEX_GLOBAL).expect("install");
    let text = text_of(&mut host, codex_file());
    assert!(!text.contains("as a progress monitor"));
    for needle in [
        PROGRESS_OPEN,
        "you MUST block with `ilium progress wait <monitor_id>`",
        "--wait",
        "at least three minutes",
        "MUST NOT poll",
        "sole recurring poller",
        "ilium progress check --command",
        "positive JSONL registration acknowledgement",
        "`job_id`",
        "ilium progress set --command",
        "ilium progress clear",
        "send you a message when the task reaches `done` or `error`",
    ] {
        assert!(text.contains(needle), "{needle:?} missing");
    }
    assert_eq!(status_ok(&mut host, "progress", CODEX_GLOBAL), "set-up");
}

fn installed_progress_text() -> String {
    let mut host = server_host();
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    block_of(&claude_text(&mut host), "progress")
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::progress_instruction_never_mentions_goals_pausing_or_resuming
#[test]
fn port_fs_04_progress_instruction_never_mentions_goals_pausing_or_resuming() {
    let instruction = installed_progress_text().to_lowercase();
    for forbidden in ["/goal", "goal", "pause", "resume", "stop hook", "arm-goal-resume", "end the turn"] {
        assert!(!instruction.contains(forbidden), "progress instruction must not mention {forbidden:?}");
    }
    assert!(instruction.contains("must not poll"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::progress_instruction_teaches_result_handling
#[test]
fn port_fs_05_progress_instruction_teaches_result_handling() {
    let instruction = installed_progress_text();
    for required in [
        "never through `ilium new-pane`",
        "Read `percent` from a structured source",
        "never the outcome of the user's request",
        "on `done`, verify the result",
        "on `error`, inspect that attempt's retained evidence once",
        "while `running`, do every step that does not need its result",
        "when the outcome is unknown",
        "A pasted monitor report is an event inside the current request",
        "go straight back to `ilium progress wait <monitor_id>`",
        "Make the probe notice a vanished task",
        "ends every earlier wait",
        "find your monitor by `job_id`",
    ] {
        assert!(instruction.contains(required), "progress instruction must contain {required:?}");
    }
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::every_provider_target_gets_the_same_progress_text
#[test]
fn port_fs_06_every_provider_target_gets_the_same_progress_text() {
    let mut host = server_host();
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("claude");
    install(&mut host, "progress", CODEX_GLOBAL).expect("codex");
    assert_eq!(claude_text(&mut host), text_of(&mut host, codex_file()));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::an_agents_file_symlinked_to_claude_stays_one_stable_shared_file
#[test]
fn port_fs_07_an_agents_file_symlinked_to_claude_stays_one_stable_shared_file() {
    let mut host = project_host();
    put(&mut host, project_claude_file(), "Keep this.\n");
    host.fs().symlink(project_agents_file(), project_claude_file());
    install(&mut host, "progress", CLAUDE_PROJECT).expect("install");
    let text = text_of(&mut host, project_claude_file());
    assert!(text.starts_with("Keep this.\n"));
    for target in [CLAUDE_PROJECT, CODEX_PROJECT] {
        assert_eq!(status_ok(&mut host, "progress", target), "set-up", "{target}");
    }
    install(&mut host, "progress", CODEX_PROJECT).expect("install through the link");
    assert_eq!(text_of(&mut host, project_claude_file()), text);
    assert!(host.fs().is_symlink(project_agents_file()));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::uninstall_preserves_unmanaged_text
#[test]
fn port_fs_08_uninstall_preserves_unmanaged_text() {
    let mut host = server_host();
    put(&mut host, claude_file(), "Keep this.\n");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    remove(&mut host, "chatroom", CLAUDE_GLOBAL).expect("remove");
    assert_eq!(claude_text(&mut host), "Keep this.\n");
    assert_eq!(status_ok(&mut host, "chatroom", CLAUDE_GLOBAL), "not-set-up");
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::obsolete_unmanaged_instruction_does_not_satisfy_the_current_contract
#[test]
fn port_fs_09_obsolete_unmanaged_instruction_does_not_satisfy_the_current_contract() {
    let mut host = server_host();
    put(&mut host, claude_file(), "Use ilium progress with an absolute-path command and clear it when finished.\n");
    assert_eq!(status_ok(&mut host, "progress", CLAUDE_GLOBAL), "not-set-up");
    remove(&mut host, "progress", CLAUDE_GLOBAL).expect("remove is a no-op");
    assert!(claude_text(&mut host).contains("Use ilium progress"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::detects_complete_current_unmanaged_instruction_without_claiming_ownership
#[test]
fn port_fs_10_detects_complete_current_unmanaged_instruction_without_claiming_ownership() {
    let mut host = server_host();
    put(
        &mut host,
        codex_file(),
        concat!(
            "For every task taking at least three minutes, you MUST use ilium progress. ",
            "Start the task and verify its process is alive. ",
            "Run ilium progress check, then wait for a positive JSONL registration acknowledgement. ",
            "The report has job_id, status, and percent. The agent MUST NOT poll; ",
            "Ilium is the sole recurring poller and will send you a message when the task ",
            "reaches done or error.\n",
        ),
    );
    assert_eq!(status_ok(&mut host, "progress", CODEX_GLOBAL), "detected");
    remove(&mut host, "progress", CODEX_GLOBAL).expect("remove leaves unmanaged text");
    assert!(text_of(&mut host, codex_file()).contains("The agent MUST NOT poll"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::detection_is_order_independent_but_requires_two_feature_specific_signals
#[test]
fn port_fs_11_detection_is_order_independent_but_requires_two_feature_specific_signals() {
    let mut host = server_host();
    put(
        &mut host,
        claude_file(),
        "Avoid routine progress narration. Send with `ilium chat send`; consult CHATROOM.md first.\n",
    );
    assert_eq!(status_ok(&mut host, "chatroom", CLAUDE_GLOBAL), "detected");
    put(&mut host, claude_file(), "The project happens to contain CHATROOM.md.\n");
    assert_eq!(status_ok(&mut host, "chatroom", CLAUDE_GLOBAL), "not-set-up");
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::legacy_ilium_chatroom_block_is_managed_and_replaced
#[test]
fn port_fs_12_legacy_ilium_chatroom_block_is_managed_and_replaced() {
    let mut host = server_host();
    put(
        &mut host,
        claude_file(),
        "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n\n## Keep\n\nUser text.\n",
    );
    assert_eq!(status_ok(&mut host, "chatroom", CLAUDE_GLOBAL), "set-up");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(!text.contains("ilium-chatroom-guidance"));
    assert!(text.contains(CHATROOM_OPEN));
    assert!(text.contains("## Keep\n\nUser text."));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::replacing_legacy_chatroom_block_preserves_unheaded_user_text_after_it
#[test]
fn port_fs_13_replacing_legacy_chatroom_block_preserves_unheaded_user_text_after_it() {
    let mut host = server_host();
    put(
        &mut host,
        claude_file(),
        "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n\nKeep this unheaded user note.\n",
    );
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(!text.contains("Post concise coordination"));
    assert!(text.contains("Keep this unheaded user note."));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::malformed_managed_marker_is_refused_without_changing_user_text
#[test]
fn port_fs_14_malformed_managed_marker_is_refused_as_invalid_argument_without_changing_user_text() {
    let mut host = server_host();
    let original = "<!-- ilium-agent-feature: progress -->\nOld guidance.\n\n# Personal notes\nKeep this text.\n";
    put(&mut host, claude_file(), original);
    refused(&mut host, "progress", CLAUDE_GLOBAL);
    assert_eq!(claude_text(&mut host), original);
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::empty_managed_block_is_stale_and_repairable
#[test]
fn port_fs_15_empty_managed_block_is_stale_and_repairable() {
    let mut host = server_host();
    put(
        &mut host,
        claude_file(),
        "<!-- ilium-agent-feature: progress -->\n<!-- /ilium-agent-feature: progress -->\n",
    );
    assert_eq!(status_ok(&mut host, "progress", CLAUDE_GLOBAL), "update-required");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert_eq!(status_ok(&mut host, "progress", CLAUDE_GLOBAL), "set-up");
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::future_managed_version_is_preserved_until_explicit_refresh
#[test]
fn port_fs_16_future_managed_version_is_preserved_until_explicit_refresh() {
    let mut host = server_host();
    let original = concat!(
        "# User policy\n\n",
        "<!-- ilium-agent-feature: progress version=99 -->\n",
        "Future Ilium-owned wording.\n",
        "<!-- /ilium-agent-feature: progress -->\n",
    );
    put(&mut host, codex_file(), original);
    assert_eq!(status_ok(&mut host, "progress", CODEX_GLOBAL), "newer");
    assert_eq!(text_of(&mut host, codex_file()), original);
    install(&mut host, "progress", CODEX_GLOBAL).expect("explicit refresh");
    let updated = text_of(&mut host, codex_file());
    assert!(updated.contains("# User policy"));
    assert!(updated.contains("version=13"));
    assert!(!updated.contains("version=99"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::unsupported_version_syntax_is_refused_without_changing_the_file
#[test]
fn port_fs_17_unsupported_version_syntax_is_refused_as_invalid_argument_without_changing_the_file() {
    let mut host = server_host();
    let original = concat!(
        "Keep this.\n",
        "<!-- ilium-agent-feature: progress version=next -->\n",
        "Owned-looking but ambiguous.\n",
        "<!-- /ilium-agent-feature: progress -->\n",
    );
    put(&mut host, codex_file(), original);
    assert_eq!(status_of(&mut host, "progress", CODEX_GLOBAL).expect_err("status").code, ErrorCode::InvalidArgument);
    assert_eq!(install(&mut host, "progress", CODEX_GLOBAL).expect_err("install").code, ErrorCode::InvalidArgument);
    assert_eq!(text_of(&mut host, codex_file()), original);
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::atomic_publish_refuses_a_concurrent_content_change
#[test]
fn port_fs_18_atomic_publish_refuses_a_concurrent_content_change_with_conflict() {
    let mut host = server_host();
    put(&mut host, codex_file(), "old contents\n");
    host.fs().race_next_write(codex_file(), b"new user contents\n");
    let error = install(&mut host, "progress", CODEX_GLOBAL).expect_err("conflict");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(text_of(&mut host, codex_file()), "new user contents\n");
    let leftovers = host.fs().all_paths().iter().filter(|path| path.path.contains("ilium-tmp")).count();
    assert_eq!(leftovers, 0);
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::overlapping_feature_markers_are_refused_without_changing_user_text
#[test]
fn port_fs_19_overlapping_feature_markers_are_refused_as_invalid_argument_without_changing_user_text() {
    let mut host = server_host();
    let original = concat!(
        "<!-- ilium-agent-feature: chatroom -->\n",
        "Read CHATROOM.md and use `ilium chat send`.\n",
        "<!-- ilium-agent-feature: progress -->\n",
        "Use `ilium progress set` and `ilium progress clear`.\n",
        "<!-- /ilium-agent-feature: chatroom -->\n",
        "<!-- /ilium-agent-feature: progress -->\n",
        "Keep this user text.\n",
    );
    put(&mut host, claude_file(), original);
    assert_eq!(status_of(&mut host, "chatroom", CLAUDE_GLOBAL).expect_err("status").code, ErrorCode::InvalidArgument);
    assert_eq!(install(&mut host, "chatroom", CLAUDE_GLOBAL).expect_err("install").code, ErrorCode::InvalidArgument);
    assert_eq!(remove(&mut host, "progress", CLAUDE_GLOBAL).expect_err("remove").code, ErrorCode::InvalidArgument);
    assert_eq!(claude_text(&mut host), original);
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::installing_through_a_symlink_preserves_the_symlink
#[test]
fn port_fs_20_installing_through_a_symlink_preserves_the_symlink() {
    let mut host = project_host();
    let actual = ilium_plugin_sdk::types::FsPath { scope: "project".to_string(), path: "actual.md".to_string() };
    put(&mut host, actual.clone(), "Keep this.\n");
    host.fs().symlink(project_claude_file(), actual.clone());
    install(&mut host, "progress", CLAUDE_PROJECT).expect("install");
    assert!(host.fs().is_symlink(project_claude_file()));
    assert!(text_of(&mut host, actual).contains("ilium-agent-feature: progress"));
}

/// REQ: PORT:ilium-client/src/agent_feature_setup.rs::atomic_replacement_preserves_existing_file_permissions
#[test]
fn port_fs_21_atomic_replacement_preserves_existing_file_permissions() {
    let mut host = server_host();
    put(&mut host, claude_file(), "Keep this.\n");
    host.fs().set_mode(claude_file(), 0o640);
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert_eq!(host.fs().mode(claude_file()), Some(0o640));
}
