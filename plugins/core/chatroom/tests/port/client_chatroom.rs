//! Twins of the tests of `ilium-client/src/chatroom.rs` (storage, hooks setup, context, unread cursor).

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

const GITIGNORE: &str = "project:.gitignore";

fn install_calls(host: &FakeHost) -> usize {
    host.service_calls().iter().filter(|(service, method, _)| service == "ilium.core.agent-setup" && method == "install").count()
}

fn unread(host: &mut FakeHost, reader: &str) -> Run {
    chat(host, &["context", "--since-last-read", "--reader", reader, "--limit", "40", "--max-bytes", "2048"])
}

/// REQ: PORT:ilium-client/src/chatroom.rs::unread_context_prints_only_records_the_reader_has_not_seen
#[test]
fn port_unread_context_prints_only_records_the_reader_has_not_seen() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    send(&mut host, "first claim");
    send(&mut host, "second claim");

    let first = unread(&mut host, "pane-1");
    assert!(first.out.contains("first claim") && first.out.contains("second claim"), "{}", first.out);
    assert!(first.out.contains("Ilium chatroom is enabled"));

    let unchanged = unread(&mut host, "pane-1");
    assert_eq!(unchanged.out, "");
    assert_eq!(unchanged.code, Some(0));

    send(&mut host, "third claim");
    let next = unread(&mut host, "pane-1");
    assert!(next.out.starts_with("New ilium chatroom messages"), "{}", next.out);
    assert!(next.out.contains("third claim"));
    assert!(!next.out.contains("second claim"));

    let other = unread(&mut host, "pane-2");
    assert!(other.out.contains("first claim") && other.out.contains("third claim"), "{}", other.out);
}

/// REQ: PORT:ilium-client/src/chatroom.rs::unread_context_prints_only_records_the_reader_has_not_seen
#[test]
fn port_unread_context_a_session_start_hook_prints_the_whole_room_again() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    send(&mut host, "first claim");
    unread(&mut host, "pane-1");
    send(&mut host, "third claim");
    unread(&mut host, "pane-1");
    let restart = chat_stdin(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1", "--limit", "40", "--max-bytes", "2048"],
        br#"{"hook_event_name":"SessionStart"}"#,
    );
    assert!(restart.out.contains("first claim") && restart.out.contains("third claim"), "{}", restart.out);
    assert!(restart.out.starts_with("Ilium chatroom is enabled"), "{}", restart.out);
}

/// REQ: PORT:ilium-client/src/chatroom.rs::capped_context_keeps_newest_records_and_counts_the_rest
#[test]
fn port_capped_context_keeps_newest_records_and_counts_the_rest() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    for index in 0..30 {
        send(&mut host, &format!("record {index} {}", "x".repeat(200)));
    }
    let run = chat(&mut host, &["context", "--limit", "40", "--max-bytes", "2048"]);
    assert!(run.out.len() <= 2048, "{} bytes", run.out.len());
    assert!(run.out.contains("record 29 "));
    assert!(!run.out.contains("record 0 "));
    assert!(run.out.contains("earlier unread messages omitted"), "{}", run.out);
}

/// REQ: PORT:ilium-client/src/chatroom.rs::capped_context_keeps_newest_records_and_counts_the_rest
#[test]
fn port_capped_context_cuts_an_oversized_newest_record_inside_the_budget() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    for index in 0..30 {
        send(&mut host, &format!("record {index} {}", "x".repeat(200)));
    }
    send(&mut host, &"y".repeat(3900));
    let run = chat(&mut host, &["context", "--limit", "40", "--max-bytes", "2048"]);
    assert!(run.out.len() <= 2048, "{} bytes", run.out.len());
    assert!(run.out.contains("yyyy"));
}

/// REQ: PORT:ilium-client/src/chatroom.rs::integration_repair_upgrades_an_older_hook_command_in_place
#[test]
fn port_integration_repair_asks_agent_setup_for_the_current_hook_when_the_room_exists() {
    let mut host = server_host();
    host.fs().write(ROOM, initial_room().as_bytes());
    host.settle();
    assert!(install_calls(&host) >= 1, "{:?}", host.service_calls());
    assert!(host.fs().read("project:.claude/settings.local.json").is_none(), "this plugin writes no agent file");
}

/// REQ: PORT:ilium-client/src/chatroom.rs::integration_repair_upgrades_an_older_hook_command_in_place
#[test]
fn port_integration_repair_hook_command_is_the_one_the_page_states() {
    let page = page_text();
    assert!(page.contains("ilium chat context --limit 40 --since-last-read --max-bytes 2048"));
    let mut host = server_host();
    host.fs().write(ROOM, initial_room().as_bytes());
    host.settle();
    let requests: Vec<String> = host.service_calls().into_iter().filter(|(service, _, _)| service == "ilium.core.agent-setup").map(|(_, _, request)| request).collect();
    assert!(requests.iter().any(|request| request.contains("chatroom")), "{requests:?}");
}

/// REQ: PORT:ilium-client/src/chatroom.rs::integration_repair_upgrades_an_older_hook_command_in_place
#[test]
fn port_integration_repair_adds_the_archive_ignore_line_to_an_existing_gitignore() {
    let mut host = server_host();
    host.fs().write(GITIGNORE, b"/CHATROOM.md\n");
    chat(&mut host, &["init"]);
    let ignore = String::from_utf8(host.fs().read(GITIGNORE).expect("gitignore")).expect("utf-8");
    assert!(ignore.contains("/CHATROOM.archive.md"), "{ignore}");
    assert_eq!(ignore.matches("/CHATROOM.md").count(), 1, "{ignore}");
}

/// REQ: PORT:ilium-client/src/chatroom.rs::initialization_creates_an_ignored_room_and_hooks_without_editing_guidance
#[test]
fn port_initialization_creates_an_ignored_room_and_leaves_guidance_alone() {
    let mut host = server_host();
    host.fs().write("project:AGENTS.md", b"# Existing\n");
    let first = chat(&mut host, &["init"]);
    let second = chat(&mut host, &["init"]);
    assert_eq!((first.code, second.code), (Some(0), Some(0)), "{} {}", first.err, second.err);

    assert_eq!(room_text(&mut host), initial_room());
    let ignore = String::from_utf8(host.fs().read(GITIGNORE).expect("gitignore")).expect("utf-8");
    assert!(ignore.contains("/CHATROOM.md"));
    assert_eq!(host.fs().read("project:AGENTS.md").expect("kept"), b"# Existing\n");
    assert!(room_text(&mut host).contains("Do not post routine progress narration"));
}

/// REQ: PORT:ilium-client/src/chatroom.rs::initialization_creates_an_ignored_room_and_hooks_without_editing_guidance
#[test]
fn port_initialization_installs_the_hooks_through_agent_setup_only() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    assert!(install_calls(&host) >= 1, "{:?}", host.service_calls());
    assert!(host.fs().read("project:.codex/hooks.json").is_none());
    assert!(host.fs().read("project:.claude/settings.local.json").is_none());
}

/// REQ: PORT:ilium-client/src/chatroom.rs::append_and_read_preserve_multiline_message_content
#[test]
fn port_append_and_read_preserve_multiline_message_content() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    chat(&mut host, &["send", "--message", "claiming | work\nthen testing", "--author", "agent:codex"]);
    let records = room_records(&mut host);
    assert_eq!(records.len(), 1);
    let (timestamp, author, content) = split_record(&records[0]);
    assert!(is_record_timestamp(&timestamp), "{timestamp}");
    assert_eq!(author, "agent:codex");
    assert_eq!(content, "claiming | work\\nthen testing");

    let context = chat(&mut host, &["context", "--limit", "40"]);
    assert!(context.out.contains("claiming | work"), "{}", context.out);
    assert!(context.out.contains("Use the chatroom sparingly"));
    assert!(context.out.contains("Do not post routine progress narration"));
}

/// REQ: PORT:ilium-client/src/chatroom.rs::initialization_rejects_a_directory_at_the_chatroom_path_before_writing_integrations
#[test]
fn port_initialization_rejects_a_directory_at_the_chatroom_path_before_writing_integrations() {
    let mut host = server_host();
    host.fs().mkdir(ROOM);
    let run = chat(&mut host, &["init"]);
    assert_eq!(run.code, Some(1));
    assert!(run.err.contains("not a regular file"), "{}", run.err);
    assert!(host.fs().read(GITIGNORE).is_none());
    assert!(host.fs().read("project:.codex/hooks.json").is_none());
    assert!(host.fs().read("project:.claude/settings.local.json").is_none());
    assert_eq!(install_calls(&host), 0);
}

/// REQ: PORT:ilium-client/src/chatroom.rs::initialization_rejects_a_dangling_chatroom_symlink
#[test]
fn port_initialization_rejects_a_dangling_chatroom_symlink() {
    let mut host = server_host();
    host.fs().symlink(ROOM, "project:missing-target");
    let run = chat(&mut host, &["init"]);
    assert_eq!(run.code, Some(1));
    assert!(run.err.contains("symlinked chatroom"), "{}", run.err);
    assert!(host.fs().read("project:missing-target").is_none(), "nothing is created through the link");
}

/// REQ: PORT:ilium-client/src/chatroom.rs::append_repairs_a_missing_trailing_newline_left_by_a_human_edit
#[test]
fn port_append_repairs_a_missing_trailing_newline_left_by_a_human_edit() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    let mut text = room_text(&mut host);
    assert!(text.ends_with('\n'));
    while text.ends_with('\n') {
        text.pop();
    }
    host.fs().write(ROOM, text.as_bytes());
    host.settle();

    let run = chat(&mut host, &["send", "--message", "hello", "--author", "agent:codex"]);
    assert_eq!(run.code, Some(0), "{}", run.err);
    let records = room_records(&mut host);
    assert_eq!(records.len(), 1);
    let (_, author, content) = split_record(&records[0]);
    assert_eq!((author.as_str(), content.as_str()), ("agent:codex", "hello"));
    assert!(room_text(&mut host).contains("## Messages\n- "), "the record starts on its own line");
}

/// REQ: PORT:ilium-client/src/chatroom.rs::hand_edited_records_cannot_smuggle_raw_terminal_escapes
#[test]
fn port_hand_edited_records_cannot_smuggle_raw_terminal_escapes() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    let mut text = room_text(&mut host);
    text.push_str("- 2026-08-19 10:00:00 +02:00 | human | red \u{1b}[31mtext\u{1b}[0m\tok\n");
    host.fs().write(ROOM, text.as_bytes());
    host.settle();

    let tail = chat(&mut host, &["tail"]);
    assert_eq!(tail.code, Some(0), "{}", tail.err);
    assert!(!tail.out.contains('\u{1b}'), "{:?}", tail.out);
    assert!(tail.out.contains(" | human | red [31mtext[0m"), "{:?}", tail.out);
    let context = chat(&mut host, &["context", "--limit", "40"]);
    assert!(!context.out.contains('\u{1b}'), "{:?}", context.out);
}

/// REQ: PORT:ilium-client/src/chatroom.rs::boot_repair_leaves_projects_without_a_room_untouched
#[test]
fn port_boot_repair_leaves_projects_without_a_room_untouched() {
    let mut host = server_host();
    host.settle();
    assert_eq!(install_calls(&host), 0, "{:?}", host.service_calls());
    assert!(host.fs().read(ROOM).is_none());
    assert!(host.fs().read(GITIGNORE).is_none());
    assert!(host.fs().read("project:.codex/hooks.json").is_none());
}

/// REQ: PORT:ilium-client/src/chatroom.rs::boot_repair_leaves_projects_without_a_room_untouched
#[test]
fn port_boot_repair_send_without_a_room_creates_nothing_and_says_so() {
    let mut host = server_host();
    let run = send(&mut host, "hello");
    assert_eq!(run.code, Some(1));
    assert!(run.err.contains("this project does not have a chatroom"), "{}", run.err);
    assert!(host.fs().read(ROOM).is_none());
}
