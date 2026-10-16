//! Safety rules (page "What it covers"): text outside the markers is kept, a file over 512 KiB is not changed,
//! a symbolic link stays a symbolic link, ambiguous markers are refused, a concurrent change is a conflict,
//! and the file grants bound what can be written.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, PluginError};

use crate::common::*;

const LIMIT: usize = 512 * 1024;

fn file_of_size(size: usize) -> String {
    let line = "x".repeat(79) + "\n";
    let mut text = line.repeat(size / 80);
    text.push_str(&"y".repeat(size - text.len()));
    assert_eq!(text.len(), size);
    text
}

fn refused_without_change(text: &str) {
    let mut host = server_host();
    put(&mut host, claude_file(), text);
    for method in ["install", "remove"] {
        let error = call(&mut host, method, "progress", CLAUDE_GLOBAL).expect_err("the marker structure is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{method}");
    }
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some(text), "the file is untouched");
}

/// REQ: F-063, F-037
#[test]
fn safety_063_a_file_of_exactly_512_kib_is_changed() {
    let mut host = server_host();
    put(&mut host, claude_file(), &file_of_size(LIMIT));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("512 KiB is within the limit");
    assert!(get(&mut host, claude_file()).expect("file").contains(PROGRESS_OPEN));
}

/// REQ: F-063, F-037
#[test]
fn safety_063_a_file_one_byte_under_512_kib_is_changed() {
    let mut host = server_host();
    put(&mut host, claude_file(), &file_of_size(LIMIT - 1));
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("under the limit");
}

/// REQ: F-063, F-037
#[test]
fn safety_063_a_file_one_byte_over_512_kib_is_refused_with_invalid_argument_and_not_changed() {
    let mut host = server_host();
    let text = file_of_size(LIMIT + 1);
    put(&mut host, claude_file(), &text);
    let error = install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("over the limit");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.message.contains("512 KiB"), "{}", error.message);
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some(text.as_str()));
}

/// REQ: F-063
#[test]
fn safety_063_remove_is_also_refused_over_512_kib() {
    let mut host = server_host();
    let text = file_of_size(LIMIT + 1);
    put(&mut host, claude_file(), &text);
    let error = call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect_err("over the limit");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some(text.as_str()));
}

/// REQ: F-063
#[test]
fn safety_063_the_status_of_a_file_over_512_kib_is_an_invalid_argument_error() {
    let mut host = server_host();
    put(&mut host, claude_file(), &file_of_size(LIMIT + 1));
    let error = host.call_service(SERVICE, "status", "{}").expect_err("status fails");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-063
#[test]
fn safety_063_a_file_that_is_not_utf_8_is_refused_and_not_changed() {
    let mut host = server_host();
    host.fs().write(claude_file(), &[0xff, 0xfe, b'a']);
    let error = install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("not text");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.fs().read(claude_file()), Some(vec![0xff, 0xfe, b'a']));
}

/// REQ: F-063
#[test]
fn safety_063_an_opening_marker_with_a_non_numeric_version_is_refused() {
    refused_without_change(
        "# Notes\n<!-- ilium-agent-feature: progress version=abc -->\nx\n<!-- /ilium-agent-feature: progress -->\n",
    );
}

/// REQ: F-063
#[test]
fn safety_063_an_opening_marker_with_unsupported_version_syntax_is_refused() {
    refused_without_change(
        "<!-- ilium-agent-feature: progress version=13x -->\nx\n<!-- /ilium-agent-feature: progress -->\n",
    );
}

/// REQ: F-063
#[test]
fn safety_063_an_opening_marker_without_a_closing_marker_is_refused() {
    refused_without_change("# Notes\n<!-- ilium-agent-feature: progress version=13 -->\nKeep this text.\n");
}

/// REQ: F-063
#[test]
fn safety_063_a_closing_marker_without_an_opener_is_refused() {
    refused_without_change("# Notes\n<!-- /ilium-agent-feature: progress -->\n");
}

/// REQ: F-063
#[test]
fn safety_063_two_opening_markers_before_one_closer_are_refused() {
    refused_without_change(
        "<!-- ilium-agent-feature: progress version=13 -->\na\n<!-- ilium-agent-feature: progress version=13 -->\nb\n<!-- /ilium-agent-feature: progress -->\n",
    );
}

/// REQ: F-063
#[test]
fn safety_063_a_chatroom_marker_inside_a_progress_block_is_refused() {
    refused_without_change(
        "<!-- ilium-agent-feature: progress version=13 -->\n<!-- ilium-agent-feature: chatroom -->\nx\n<!-- /ilium-agent-feature: chatroom -->\n<!-- /ilium-agent-feature: progress -->\n",
    );
}

/// REQ: F-063
#[test]
fn safety_063_an_unterminated_opening_marker_is_refused() {
    refused_without_change("# Notes\n<!-- ilium-agent-feature: progress version=13\nKeep this text.\n");
}

/// REQ: F-063
#[test]
fn safety_063_an_unrecognised_marker_form_is_refused() {
    refused_without_change("<!-- ilium-agent-feature: progress extra -->\nx\n<!-- /ilium-agent-feature: progress -->\n");
}

/// REQ: F-063
#[test]
fn safety_063_a_concurrent_change_ends_as_a_conflict_and_the_other_change_is_kept() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Mine\n");
    host.inject_failure(
        "fs.write-atomic",
        PluginError { code: ErrorCode::Conflict, message: "revision changed".to_string(), retry_after_ms: None },
    );
    let error = install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("conflict");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some("# Mine\n"));
}

/// REQ: F-063
#[test]
fn safety_063_a_failed_write_leaves_no_partial_block_and_a_retry_succeeds() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Mine\n");
    host.inject_failure(
        "fs.write-atomic",
        PluginError { code: ErrorCode::Io, message: "disk full".to_string(), retry_after_ms: None },
    );
    install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("io failure");
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some("# Mine\n"));
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("retry");
    assert!(get(&mut host, claude_file()).expect("file").contains(PROGRESS_OPEN));
}

/// REQ: F-063, F-037
#[test]
fn safety_063_a_symbolic_link_stays_a_symbolic_link_and_the_target_gets_the_block() {
    let mut host = project_host();
    put(&mut host, project_claude_file(), "# Shared\n");
    host.fs().symlink(project_agents_file(), project_claude_file());
    install(&mut host, "progress", CODEX_PROJECT).expect("install through the link");
    assert!(host.fs().is_symlink(project_agents_file()), "the link is still a link");
    assert!(get(&mut host, project_claude_file()).expect("target").contains(PROGRESS_OPEN));
    assert_eq!(get(&mut host, project_agents_file()), get(&mut host, project_claude_file()));
}

/// REQ: F-063
#[test]
fn safety_063_without_the_claude_write_grant_the_claude_file_is_not_permitted_and_unchanged() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Mine\n");
    host.revoke("fs.write:agent-config:claude");
    let error = install(&mut host, "progress", CLAUDE_GLOBAL).expect_err("no grant");
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some("# Mine\n"));
}

/// REQ: F-063, F-037
#[test]
fn safety_063_revoking_the_claude_grant_leaves_the_codex_file_writable() {
    let mut host = server_host();
    host.revoke("fs.write:agent-config:claude");
    install(&mut host, "progress", CODEX_GLOBAL).expect("codex still works");
}

/// REQ: F-063
#[test]
fn safety_063_without_the_project_write_grant_a_project_file_is_not_permitted() {
    let mut host = project_host();
    host.revoke("fs.write:project");
    let error = install(&mut host, "chatroom", CLAUDE_PROJECT).expect_err("no grant");
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(get(&mut host, project_claude_file()), None);
}

/// REQ: F-063
#[test]
fn safety_063_without_the_read_grant_the_status_cannot_read_the_file() {
    let mut host = server_host();
    put(&mut host, claude_file(), "# Mine\n");
    host.revoke("fs.read:agent-config:claude");
    let error = host.call_service(SERVICE, "status", "{}").expect_err("no grant");
    assert_eq!(error.code, ErrorCode::NotPermitted);
}

/// REQ: F-063
#[test]
fn safety_063_text_outside_the_markers_survives_install_and_remove_of_both_blocks() {
    let mut host: FakeHost = server_host();
    let original = "# Rules\n\nAlways run the tests.\n\n## Style\n\nShort lines.\n";
    put(&mut host, claude_file(), original);
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("chatroom");
    install(&mut host, "progress", CLAUDE_GLOBAL).expect("progress");
    call(&mut host, "remove", "chatroom", CLAUDE_GLOBAL).expect("remove chatroom");
    call(&mut host, "remove", "progress", CLAUDE_GLOBAL).expect("remove progress");
    assert_eq!(get(&mut host, claude_file()).as_deref(), Some(original));
}
