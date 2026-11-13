//! The two blocks and their markers (page "What it covers"): text outside the markers is kept, a block is
//! replaced in place, an older block is upgraded, duplicates are repaired, removal only takes the marked text.

use proptest::prelude::*;

use crate::common::*;

fn seeded(text: &str) -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = server_host();
    put(&mut host, claude_file(), text);
    host
}

fn claude_text(host: &mut ilium_plugin_sdk::testing::FakeHost) -> String {
    get(host, claude_file()).expect("the Claude file exists")
}

/// REQ: F-063
#[test]
fn markers_063_chatroom_install_into_an_empty_file_starts_with_the_opening_marker() {
    let mut host = seeded("");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(text.trim_start().starts_with(CHATROOM_OPEN), "{text}");
    assert!(text.ends_with(&format!("{CHATROOM_CLOSE}\n")), "{text}");
}

/// REQ: F-063
#[test]
fn markers_063_the_chatroom_marker_carries_no_version() {
    let mut host = seeded("");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert_eq!(
        text.matches("<!-- ilium-agent-feature: chatroom").count(),
        1
    );
    assert_eq!(
        text.matches(CHATROOM_OPEN).count(),
        1,
        "the opening marker is exactly the page marker"
    );
}

/// REQ: F-063
#[test]
fn markers_063_the_progress_marker_carries_version_thirteen() {
    let mut host = seeded("");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert_eq!(text.matches(PROGRESS_OPEN).count(), 1, "{text}");
    assert_eq!(text.matches(PROGRESS_CLOSE).count(), 1, "{text}");
}

/// REQ: F-063, F-037
#[test]
fn markers_063_install_creates_a_missing_file() {
    let mut host = server_host();
    assert_eq!(get(&mut host, claude_file()), None);
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    assert!(claude_text(&mut host).contains(CHATROOM_OPEN));
}

/// REQ: F-063
#[test]
fn markers_063_text_before_the_block_is_kept_and_separated_by_a_blank_line() {
    let mut host = seeded("# Existing\n");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(
        text.starts_with(&format!("# Existing\n\n{PROGRESS_OPEN}")),
        "{text}"
    );
}

/// REQ: F-063
#[test]
fn markers_063_a_file_without_a_final_newline_gets_a_blank_line_before_the_block() {
    let mut host = seeded("# Existing");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(
        text.starts_with(&format!("# Existing\n\n{PROGRESS_OPEN}")),
        "{text}"
    );
}

/// REQ: F-063
#[test]
fn markers_063_installing_twice_gives_the_same_bytes() {
    let mut host = seeded("# Existing\n");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("first install");
    let first = claude_text(&mut host);
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("second install");
    assert_eq!(claude_text(&mut host), first);
    assert_eq!(first.matches(PROGRESS_OPEN).count(), 1);
}

/// REQ: F-063
#[test]
fn markers_063_replacing_a_block_keeps_all_text_outside_the_markers_once_and_in_order() {
    let before = "# Before\n\nKeep A.\n";
    let after = "# After\n\nKeep B.\n";
    let stale = old_progress_block(13).replace("Old wording.", "Edited by hand.");
    let mut host = seeded(&format!("{before}\n{stale}\n{after}"));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert_eq!(text.matches("Keep A.").count(), 1, "{text}");
    assert_eq!(text.matches("Keep B.").count(), 1, "{text}");
    assert!(text.find("Keep A.") < text.find("Keep B."), "{text}");
    assert!(!text.contains("Edited by hand."));
    assert_eq!(text.matches(PROGRESS_OPEN).count(), 1);
}

/// REQ: F-063
#[test]
fn markers_063_an_older_progress_block_is_upgraded_to_version_thirteen() {
    let mut host = seeded(&format!("# Top\n\n{}", old_progress_block(12)));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(text.contains("version=13"));
    assert!(!text.contains("version=12"));
    assert!(!text.contains("Old wording."));
    assert!(text.starts_with("# Top\n"));
}

/// REQ: F-063
#[test]
fn markers_063_a_version_zero_progress_block_is_upgraded() {
    let mut host = seeded(&old_progress_block(0));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(text.contains(PROGRESS_OPEN) && !text.contains("version=0 "));
}

/// REQ: F-063
#[test]
fn markers_063_a_progress_block_without_a_version_is_upgraded() {
    let mut host = seeded("<!-- ilium-agent-feature: progress -->\nOld guidance.\n<!-- /ilium-agent-feature: progress -->\n");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(text.contains(PROGRESS_OPEN));
    assert!(!text.contains("Old guidance."));
}

/// REQ: F-063
#[test]
fn markers_063_duplicate_blocks_are_repaired_to_one_current_copy() {
    let block = old_progress_block(12);
    let mut host = seeded(&format!("{block}\n{block}"));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert_eq!(
        text.matches("<!-- ilium-agent-feature: progress").count(),
        1,
        "{text}"
    );
    assert_eq!(text.matches(PROGRESS_CLOSE).count(), 1, "{text}");
    assert!(!text.contains("Old wording."));
}

/// REQ: F-063
#[test]
fn markers_063_an_explicit_install_replaces_a_newer_block_with_the_current_one() {
    let mut host = seeded(
        "# User policy\n\n<!-- ilium-agent-feature: progress version=99 -->\nFuture wording.\n<!-- /ilium-agent-feature: progress -->\n",
    );
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(text.contains("# User policy"));
    assert!(text.contains("version=13") && !text.contains("version=99"));
}

/// REQ: F-063
#[test]
fn markers_063_remove_takes_only_the_marked_text() {
    let mut host = seeded("Keep this.\n");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    call(&mut host, "remove", "chatroom", CLAUDE_GLOBAL).expect("remove");
    assert_eq!(claude_text(&mut host), "Keep this.\n");
}

/// REQ: F-063
#[test]
fn markers_063_remove_of_a_block_that_is_not_there_changes_nothing() {
    let mut host = seeded("Keep this.\n");
    call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect("remove");
    assert_eq!(claude_text(&mut host), "Keep this.\n");
}

/// REQ: F-063
#[test]
fn markers_063_remove_never_creates_a_missing_file() {
    let mut host = server_host();
    call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect("remove");
    assert_eq!(get(&mut host, claude_file()), None);
}

/// REQ: F-063
#[test]
fn markers_063_both_blocks_live_side_by_side_and_remove_takes_only_one() {
    let mut host = seeded("Top.\n");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("chatroom");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("progress");
    let both = claude_text(&mut host);
    assert!(both.contains(CHATROOM_OPEN) && both.contains(PROGRESS_OPEN));
    call(&mut host, "remove", "chatroom", CLAUDE_GLOBAL).expect("remove");
    let rest = claude_text(&mut host);
    assert!(!rest.contains("ilium-agent-feature: chatroom"));
    assert!(rest.contains(PROGRESS_OPEN) && rest.starts_with("Top.\n"));
}

/// REQ: F-063
#[test]
fn markers_063_a_marker_of_another_feature_name_is_plain_text() {
    let other =
        "<!-- ilium-agent-feature: other -->\nSomething.\n<!-- /ilium-agent-feature: other -->\n";
    let mut host = seeded(other);
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    assert!(claude_text(&mut host).starts_with(other));
}

/// REQ: F-063
#[test]
fn markers_063_the_progress_block_teaches_the_registration_and_result_handling_rules() {
    let mut host = seeded("");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let block = block_of(&claude_text(&mut host), "progress");
    for required in [
        "ilium progress",
        "at least three minutes",
        "MUST use",
        "MUST NOT poll",
        "sole recurring poller",
        "ilium progress check",
        "positive JSONL registration acknowledgement",
        "send you a message when the task reaches",
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
        assert!(
            block.contains(required),
            "progress block must contain {required:?}"
        );
    }
}

/// REQ: F-063
#[test]
fn markers_063_the_progress_block_never_mentions_goals_pausing_or_resuming() {
    let mut host = seeded("");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
    let block = block_of(&claude_text(&mut host), "progress").to_lowercase();
    for forbidden in [
        "/goal",
        "goal",
        "pause",
        "resume",
        "stop hook",
        "arm-goal-resume",
        "end the turn",
    ] {
        assert!(
            !block.contains(forbidden),
            "progress block must not mention {forbidden:?}"
        );
    }
    assert!(block.contains("must not poll"));
}

/// REQ: F-063
#[test]
fn markers_063_the_chatroom_block_names_the_chatroom_file_and_the_chat_commands() {
    let mut host = seeded("");
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let block = block_of(&claude_text(&mut host), "chatroom");
    assert!(block.contains("CHATROOM.md"));
    assert!(block.contains("ilium chat context"));
    assert!(block.contains("ilium chat send"));
}

/// REQ: F-063, F-037
#[test]
fn markers_063_every_target_file_gets_the_same_progress_text() {
    let mut host = server_host();
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("claude");
    install(&mut host, "progress", CODEX_GLOBAL).expect("codex");
    assert_eq!(get(&mut host, claude_file()), get(&mut host, codex_file()));
}

/// REQ: F-063
#[test]
fn markers_063_a_legacy_chatroom_guidance_block_is_replaced_and_user_text_after_it_is_kept() {
    let mut host = seeded(
        "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n\n## Keep\n\nUser text.\n",
    );
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(!text.contains("ilium-chatroom-guidance"));
    assert!(text.contains(CHATROOM_OPEN));
    assert!(text.contains("## Keep\n\nUser text."));
}

/// REQ: F-063
#[test]
fn markers_063_replacing_a_legacy_chatroom_block_keeps_an_unheaded_user_note_after_it() {
    let mut host = seeded(
        "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n\nKeep this unheaded user note.\n",
    );
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("install");
    let text = claude_text(&mut host);
    assert!(!text.contains("Post concise coordination"));
    assert!(text.contains("Keep this unheaded user note."));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-063
    #[test]
    fn markers_063_install_then_remove_restores_any_marker_free_text(raw in "[a-c .\n#]{0,120}") {
        let mut text = raw;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        let mut host = seeded(&text);
        install(&mut host, "progress", CLAUDE_GLOBAL).expect("install");
        let installed = claude_text(&mut host);
        prop_assert!(installed.starts_with(&text));
        call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect("remove");
        let restored = get(&mut host, claude_file()).unwrap_or_default();
        prop_assert_eq!(restored, text);
    }

    // REQ: F-063
    #[test]
    fn markers_063_install_is_idempotent_for_any_marker_free_text(raw in "[a-c .\n#]{0,120}") {
        let mut host = seeded(&raw);
        install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("first");
        let first = claude_text(&mut host);
        install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("second");
        prop_assert_eq!(claude_text(&mut host), first);
    }
}
