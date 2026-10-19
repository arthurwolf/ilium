//! `ilium chat init`: the initial file (chatroom-B05), the ignore lines (chatroom-B06), the refusals,
//! the call to Agent setup and the absence of any agent file written by this plugin (F-062, F-064).

use crate::common::*;

const GITIGNORE: &str = "project:.gitignore";

fn init(host: &mut ilium_plugin_sdk::testing::FakeHost) -> Run {
    chat(host, &["init"])
}

/// REQ: chatroom-B05, F-062
#[test]
fn init_b05_creates_the_exact_initial_file() {
    let mut host = server_host();
    let run = init(&mut host);
    assert_eq!(run.code, Some(0), "stderr: {}", run.err);
    assert_eq!(room_text(&mut host), initial_room());
}

/// REQ: chatroom-B05
#[test]
fn init_b05_file_starts_with_the_title_and_the_marker() {
    let mut host = server_host();
    init(&mut host);
    let text = room_text(&mut host);
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("# ILIUM CHATROOM"));
    assert_eq!(lines.next(), Some("<!-- ilium-chatroom: v1 -->"));
    assert_eq!(lines.next(), Some(""));
}

/// REQ: chatroom-B05
#[test]
fn init_b05_file_holds_the_guidance_paragraph_of_the_prompt_template_byte_for_byte() {
    let mut host = server_host();
    init(&mut host);
    assert!(room_text(&mut host).contains(&guidance()));
    assert!(guidance().starts_with("Use the chatroom sparingly."));
}

/// REQ: chatroom-B05
#[test]
fn init_b05_file_ends_with_the_messages_heading_and_no_record() {
    let mut host = server_host();
    init(&mut host);
    let text = room_text(&mut host);
    assert!(text.ends_with("\n\n## Messages\n"), "{text:?}");
    assert!(room_records(&mut host).is_empty());
}

/// REQ: chatroom-B05, PORT:ilium-client/src/chatroom.rs::initialization_creates_an_ignored_room_and_hooks_without_editing_guidance
#[test]
fn init_b05_twice_keeps_the_room_and_leaves_existing_agent_guidance_alone() {
    let mut host = server_host();
    host.fs().write("project:AGENTS.md", b"# Existing\n");
    init(&mut host);
    init(&mut host);
    assert_eq!(room_text(&mut host), initial_room());
    assert_eq!(
        host.fs().read("project:AGENTS.md").expect("kept"),
        b"# Existing\n"
    );
    assert!(room_text(&mut host).contains("Do not post routine progress narration"));
}

/// REQ: chatroom-B05, PORT:ilium-client/src/app.rs::project_chatroom_setup_refresh_is_idempotent_and_preserves_room_data
#[test]
fn init_b05_never_overwrites_an_existing_file_and_keeps_its_records() {
    let mut host = server_host();
    let custom = room_with(&[numbered_line(1), numbered_line(2)]);
    host.fs().write(ROOM, custom.as_bytes());
    let run = init(&mut host);
    assert_eq!(run.code, Some(0), "stderr: {}", run.err);
    assert_eq!(room_text(&mut host), custom);
}

/// REQ: chatroom-B05
#[test]
fn init_b05_never_overwrites_a_hand_written_file_without_the_marker() {
    let mut host = server_host();
    host.fs().write(ROOM, b"my own notes\n");
    init(&mut host);
    assert_eq!(room_text(&mut host), "my own notes\n");
}

/// REQ: chatroom-B05, PORT:ilium-client/src/chatroom.rs::initialization_rejects_a_directory_at_the_chatroom_path_before_writing_integrations
#[test]
fn init_b05_a_directory_at_the_room_path_is_refused_and_nothing_else_is_written() {
    let mut host = server_host();
    host.fs().mkdir(ROOM);
    let run = init(&mut host);
    assert_eq!(run.code, Some(1));
    assert!(run.err.contains("not a regular file"), "{}", run.err);
    assert!(host.fs().read(GITIGNORE).is_none());
    assert!(host.service_calls().is_empty());
}

/// REQ: chatroom-B05, PORT:ilium-client/src/chatroom.rs::initialization_rejects_a_dangling_chatroom_symlink
#[test]
fn init_b05_a_dangling_symlink_at_the_room_path_is_refused() {
    let mut host = server_host();
    host.fs().symlink(ROOM, "project:missing-target");
    let run = init(&mut host);
    assert_eq!(run.code, Some(1));
    assert!(run.err.contains("symlinked chatroom"), "{}", run.err);
}

/// REQ: chatroom-B05
#[test]
fn init_b05_a_symlink_to_a_regular_file_is_refused_too() {
    let mut host = server_host();
    host.fs().write("project:elsewhere.md", b"x\n");
    host.fs().symlink(ROOM, "project:elsewhere.md");
    let run = init(&mut host);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        host.fs().read("project:elsewhere.md").expect("kept"),
        b"x\n"
    );
}

/// REQ: chatroom-B05, chatroom-B11
#[test]
fn init_b05_send_refuses_a_symlinked_room() {
    let mut host = server_host();
    host.fs().write("project:elsewhere.md", b"x\n");
    host.fs().symlink(ROOM, "project:elsewhere.md");
    let run = send(&mut host, "hello");
    assert_eq!(run.code, Some(1));
    assert_eq!(
        host.fs().read("project:elsewhere.md").expect("kept"),
        b"x\n"
    );
}

/// REQ: chatroom-B06
#[test]
fn ignore_b06_init_creates_the_gitignore_with_both_lines() {
    let mut host = server_host();
    init(&mut host);
    let text =
        String::from_utf8(host.fs().read(GITIGNORE).expect(".gitignore created")).expect("utf-8");
    assert_eq!(text, "/CHATROOM.md\n/CHATROOM.archive.md\n");
}

/// REQ: chatroom-B06
#[test]
fn ignore_b06_existing_lines_of_the_gitignore_are_kept_and_the_two_lines_appended() {
    let mut host = server_host();
    host.fs().write(GITIGNORE, b"target/\n");
    init(&mut host);
    let text = String::from_utf8(host.fs().read(GITIGNORE).expect("kept")).expect("utf-8");
    assert_eq!(text, "target/\n/CHATROOM.md\n/CHATROOM.archive.md\n");
}

/// REQ: chatroom-B06
#[test]
fn ignore_b06_a_missing_final_newline_is_added_before_the_new_lines() {
    let mut host = server_host();
    host.fs().write(GITIGNORE, b"target/");
    init(&mut host);
    let text = String::from_utf8(host.fs().read(GITIGNORE).expect("kept")).expect("utf-8");
    assert_eq!(text, "target/\n/CHATROOM.md\n/CHATROOM.archive.md\n");
}

/// REQ: chatroom-B06
#[test]
fn ignore_b06_only_the_missing_line_is_added() {
    let mut host = server_host();
    host.fs().write(GITIGNORE, b"/CHATROOM.md\n");
    init(&mut host);
    let text = String::from_utf8(host.fs().read(GITIGNORE).expect("kept")).expect("utf-8");
    assert_eq!(text, "/CHATROOM.md\n/CHATROOM.archive.md\n");
}

/// REQ: chatroom-B06
#[test]
fn ignore_b06_running_init_twice_adds_no_duplicate_line() {
    let mut host = server_host();
    init(&mut host);
    init(&mut host);
    let text = String::from_utf8(host.fs().read(GITIGNORE).expect("kept")).expect("utf-8");
    assert_eq!(text.matches("/CHATROOM.md\n").count(), 1);
    assert_eq!(text.matches("/CHATROOM.archive.md\n").count(), 1);
}

/// REQ: chatroom-B06
#[test]
fn ignore_b06_a_line_with_surrounding_spaces_still_counts_as_present() {
    let mut host = server_host();
    host.fs()
        .write(GITIGNORE, b"  /CHATROOM.md  \n/CHATROOM.archive.md\n");
    init(&mut host);
    let text = String::from_utf8(host.fs().read(GITIGNORE).expect("kept")).expect("utf-8");
    assert_eq!(text.matches("CHATROOM.md").count(), 1);
}

/// REQ: F-062, F-064, chatroom-B12
#[test]
fn init_f062_asks_agent_setup_to_install_the_chatroom_block_and_hooks() {
    let mut host = server_host();
    init(&mut host);
    let calls = host.service_calls();
    let call = calls
        .iter()
        .find(|(service, method, _)| service == "ilium.core.agent-setup" && method == "install")
        .expect("a call to ilium.core.agent-setup/install");
    let request: serde_json::Value = serde_json::from_str(&call.2).expect("request is JSON");
    assert_eq!(request["feature"], "chatroom");
    assert!(request["project"].is_string());
}

/// REQ: F-062, F-064
#[test]
fn init_f062_the_plugin_writes_no_agent_file_itself() {
    let mut host = server_host();
    init(&mut host);
    for path in [
        "project:.claude/settings.local.json",
        "project:.codex/hooks.json",
        "project:AGENTS.md",
        "project:CLAUDE.md",
    ] {
        assert!(
            host.fs().read(path).is_none(),
            "{path} must belong to Agent setup"
        );
    }
}

/// REQ: F-062
#[test]
fn init_f062_a_failing_agent_setup_call_still_leaves_the_room_created() {
    let mut host = server_host();
    host.inject_failure(
        "services.call",
        ilium_plugin_sdk::types::PluginError::new(
            ilium_plugin_sdk::types::ErrorCode::NotFound,
            "agent-setup is not installed",
        ),
    );
    let run = init(&mut host);
    assert_eq!(room_text(&mut host), initial_room());
    assert!(run.code.is_some());
}

/// REQ: F-062, PORT:ilium-client/src/chatroom.rs::boot_repair_leaves_projects_without_a_room_untouched
#[test]
fn init_f062_starting_without_a_room_creates_nothing_and_calls_no_service() {
    let host = server_host();
    assert!(host.service_calls().is_empty());
    let mut host = host;
    assert!(host.fs().read(ROOM).is_none());
    assert!(host.fs().read(GITIGNORE).is_none());
}

/// REQ: F-062
#[test]
fn init_f062_starting_with_a_room_does_not_rewrite_it() {
    let mut host = server_host();
    let custom = room_with(&[numbered_line(1)]);
    host.fs().write(ROOM, custom.as_bytes());
    host.settle();
    assert_eq!(room_text(&mut host), custom);
}
