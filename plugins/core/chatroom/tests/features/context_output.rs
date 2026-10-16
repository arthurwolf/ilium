//! `ilium chat context`: the text from the prompt templates (chatroom-B13), the byte cap (chatroom-B14),
//! the hook command (chatroom-B12) and the sanitising of hook output (F-064).

use crate::common::*;

fn row(timestamp: &str, author: &str, content: &str) -> String {
    render("chatroom-context-row", &[timestamp, author, content])
}

fn header_with_messages() -> String {
    render("chatroom-context", &[&guidance()])
}

/// REQ: chatroom-B13, F-064
#[test]
fn context_b13_with_messages_is_the_context_template_followed_by_one_row_each() {
    let mut host = host_with_records(3);
    let run = chat(&mut host, &["context"]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.err);
    let mut expected = header_with_messages();
    for n in 0..3 {
        expected.push_str(&row(&stamp(n), "agent", &format!("record {n}")));
    }
    assert_eq!(run.out, expected);
}

/// REQ: chatroom-B13
#[test]
fn context_b13_first_read_starts_with_ilium_chatroom_is_enabled() {
    let mut host = host_with_records(1);
    assert!(chat(&mut host, &["context"]).out.starts_with("Ilium chatroom is enabled"));
}

/// REQ: chatroom-B13
#[test]
fn context_b13_without_messages_the_empty_template_is_used() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    let run = chat(&mut host, &["context"]);
    assert_eq!(run.out, render("chatroom-empty-context", &[&guidance()]));
    assert!(run.out.starts_with("Ilium chatroom is enabled. No messages have been posted yet."));
}

/// REQ: chatroom-B13
#[test]
fn context_b13_without_a_room_file_the_empty_template_is_used_too() {
    let mut host = server_host();
    let run = chat(&mut host, &["context"]);
    assert_eq!(run.out, render("chatroom-empty-context", &[&guidance()]));
    assert_eq!(run.code, Some(0));
}

/// REQ: chatroom-B13
#[test]
fn context_b13_default_limit_keeps_the_newest_records_only() {
    let mut host = host_with_records(60);
    let run = chat(&mut host, &["context", "--limit", "2"]);
    let mut expected = header_with_messages();
    expected.push_str(&row(&stamp(58), "agent", "record 58"));
    expected.push_str(&row(&stamp(59), "agent", "record 59"));
    assert_eq!(run.out, expected);
}

/// REQ: chatroom-B13
#[test]
fn context_b13_limit_zero_selects_no_record_and_uses_the_empty_text() {
    let mut host = host_with_records(3);
    let run = chat(&mut host, &["context", "--limit", "0"]);
    assert_eq!(run.out, render("chatroom-empty-context", &[&guidance()]));
}

/// REQ: chatroom-B13
#[test]
fn context_b13_rows_flatten_newlines_tabs_and_control_characters() {
    let mut host = server_host();
    let mut text = initial_room();
    text.push_str("- 2026-10-12 10:00:00 +02:00 | agent | a\\nb\\tc\u{1b}[31m\n");
    host.fs().write(ROOM, text.as_bytes());
    host.settle();
    let run = chat(&mut host, &["context"]);
    assert!(!run.out.contains('\u{1b}'));
    assert!(run.out.contains("a\\nb\\tc[31m"), "{}", run.out);
}

/// REQ: chatroom-B13, chatroom-B14
#[test]
fn context_b14_output_within_the_budget_has_no_omitted_note() {
    let mut host = host_with_records(3);
    let run = chat(&mut host, &["context", "--max-bytes", "2048"]);
    assert!(!run.out.contains("earlier unread messages omitted"));
    assert!(run.out.len() <= 2048);
}

/// REQ: chatroom-B14
#[test]
fn context_b14_output_never_exceeds_max_bytes_and_counts_the_omitted_records() {
    let mut host = server_host();
    let lines: Vec<String> =
        (0..30).map(|n| record_line(&stamp(n), "agent", &format!("record {n} {}", "x".repeat(200)))).collect();
    seed_room(&mut host, &lines);
    let run = chat(&mut host, &["context", "--limit", "40", "--max-bytes", "2048"]);
    assert!(run.out.len() <= 2048, "{} bytes", run.out.len());
    assert!(run.out.contains("record 29 "));
    assert!(!run.out.contains("record 0 "));
    assert!(run.out.contains("earlier unread messages omitted to keep this short"));
    let omitted: usize = run.out.split(" earlier unread messages omitted").next().expect("note").rsplit('(').next().expect("count").parse().expect("number");
    assert!(omitted > 0 && omitted < 30);
}

/// REQ: chatroom-B14
#[test]
fn context_b14_the_omitted_note_follows_the_omitted_template() {
    let mut host = server_host();
    let lines: Vec<String> =
        (0..30).map(|n| record_line(&stamp(n), "agent", &format!("record {n} {}", "x".repeat(200)))).collect();
    seed_room(&mut host, &lines);
    let run = chat(&mut host, &["context", "--limit", "40", "--max-bytes", "2048"]);
    let header = header_with_messages();
    let after_header = &run.out[header.len()..];
    let note_end = after_header.find('\n').map(|end| end + 1).expect("note line");
    let note = &after_header[..note_end];
    let shown = run.out[header.len() + note_end..].matches("record ").count();
    let omitted = 30 - shown;
    assert_eq!(note, render("chatroom-omitted", &[&omitted.to_string(), "30"]));
}

/// REQ: chatroom-B14, PORT:ilium-client/src/chatroom.rs::capped_context_keeps_newest_records_and_counts_the_rest
#[test]
fn context_b14_a_single_oversized_newest_record_is_cut_with_the_marker() {
    let mut host = server_host();
    seed_room(&mut host, &[record_line(&stamp(1), "agent", &"y".repeat(3900))]);
    let run = chat(&mut host, &["context", "--limit", "40", "--max-bytes", "2048"]);
    assert!(run.out.len() <= 2048, "{} bytes", run.out.len());
    assert!(run.out.contains("yyyy"));
    assert!(run.out.ends_with("\u{2026}\n"), "{:?}", &run.out[run.out.len().saturating_sub(20)..]);
}

/// REQ: chatroom-B14
#[test]
fn context_b14_a_cut_never_splits_a_multibyte_character() {
    let mut host = server_host();
    seed_room(&mut host, &[record_line(&stamp(1), "agent", &"\u{1F9ED}".repeat(1500))]);
    let run = chat(&mut host, &["context", "--limit", "40", "--max-bytes", "2048"]);
    assert!(run.out.len() <= 2048);
    assert!(run.out.ends_with("\u{2026}\n"));
}

/// REQ: chatroom-B14
#[test]
fn context_b14_a_budget_smaller_than_the_header_still_prints_the_header_and_the_marker_row() {
    let mut host = host_with_records(2);
    let run = chat(&mut host, &["context", "--max-bytes", "10"]);
    assert!(run.out.starts_with("Ilium chatroom is enabled"));
    assert_eq!(run.code, Some(0));
}

/// REQ: chatroom-B14, chatroom-B15
#[test]
fn context_b14_a_session_start_hook_gets_four_times_the_budget() {
    let mut host = server_host();
    let lines: Vec<String> =
        (0..30).map(|n| record_line(&stamp(n), "agent", &format!("record {n} {}", "x".repeat(200)))).collect();
    seed_room(&mut host, &lines);
    let normal = chat(&mut host, &["context", "--limit", "40", "--since-last-read", "--reader", "a", "--max-bytes", "2048"]);
    let start = chat_stdin(
        &mut host,
        &["context", "--limit", "40", "--since-last-read", "--reader", "b", "--max-bytes", "2048"],
        br#"{"hook_event_name":"SessionStart"}"#,
    );
    assert!(normal.out.len() <= 4 * 2048 && start.out.len() <= 4 * 2048);
    assert!(start.out.len() > 2048, "{} bytes", start.out.len());
    assert!(start.out.matches("record ").count() >= normal.out.matches("record ").count());
}

/// REQ: chatroom-B14
#[test]
fn context_b14_the_reserve_of_160_bytes_keeps_room_for_the_note() {
    // A budget of exactly header + 160 leaves no room for any row: every record is omitted or cut.
    let mut host = host_with_records(5);
    let header = header_with_messages().len();
    let run = chat(&mut host, &["context", "--max-bytes", &(header + 160).to_string()]);
    assert!(run.out.len() <= header + 160, "{} bytes", run.out.len());
}

/// REQ: chatroom-B12, F-064
#[test]
fn hook_b12_the_documented_hook_command_runs_and_answers_within_the_budget() {
    let mut host = host_with_records(3);
    let run = chat_stdin(
        &mut host,
        &["context", "--limit", "40", "--since-last-read", "--max-bytes", "2048"],
        br#"{"session_id":"s9","hook_event_name":"UserPromptSubmit"}"#,
    );
    assert_eq!(run.code, Some(0));
    assert!(run.out.len() <= 2048 * 4);
}

/// REQ: chatroom-B12
#[test]
fn hook_b12_the_page_states_the_exact_hook_command_timeout_and_status_message() {
    let page = page_text();
    assert!(page.contains("`ilium chat context --limit 40 --since-last-read --max-bytes 2048`, timeout 10 s, status message \"Checking ilium chatroom\""));
}

/// REQ: chatroom-B12
#[test]
fn hook_b12_the_hook_is_installed_by_agent_setup_not_by_this_plugin() {
    let manifest = std::fs::read_to_string(manifest_path()).expect("manifest");
    assert!(!manifest.contains("agent-config"));
    assert!(manifest.contains("services.use:ilium.core.agent-setup"));
}

/// REQ: chatroom-B13, chatroom-B14, chatroom-B15
#[test]
fn context_b13_an_unchanged_since_last_read_prints_nothing_and_exits_0() {
    let mut host = host_with_records(2);
    chat(&mut host, &["context", "--since-last-read", "--reader", "r1"]);
    let again = chat(&mut host, &["context", "--since-last-read", "--reader", "r1"]);
    assert_eq!(again.out, "");
    assert_eq!(again.code, Some(0));
}

/// REQ: chatroom-B13
#[test]
fn context_b13_the_unread_form_starts_with_the_unread_template() {
    let mut host = host_with_records(2);
    chat(&mut host, &["context", "--since-last-read", "--reader", "r1"]);
    send(&mut host, "fresh");
    let run = chat(&mut host, &["context", "--since-last-read", "--reader", "r1"]);
    let unread_header = template("chatroom-unread-context");
    assert!(run.out.starts_with(&unread_header), "{}", run.out);
    assert!(run.out.ends_with(&row_text_of(&run.out)), "last row is a context row");
}

fn row_text_of(output: &str) -> String {
    output.lines().last().map(|line| format!("{line}\n")).unwrap_or_default()
}
