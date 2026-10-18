//! The `ilium chat` command: the default author (chatroom-B04), the output texts (chatroom-B10), the send
//! errors (chatroom-B11), `tail`, the nearest-room lookup and the command contribution (F-062).

use crate::common::*;

fn host_with_room() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    host
}

fn author_of_only_record(host: &mut ilium_plugin_sdk::testing::FakeHost) -> String {
    let records = room_records(host);
    assert_eq!(records.len(), 1, "one record expected");
    split_record(&records[0]).1
}

/// REQ: chatroom-B04
#[test]
fn author_b04_falls_back_to_agent_without_options_or_environment() {
    let mut host = host_with_room();
    send(&mut host, "hello");
    assert_eq!(author_of_only_record(&mut host), "agent");
}

/// REQ: chatroom-B04
#[test]
fn author_b04_agent_name_is_used_when_the_chatroom_author_is_unset() {
    let mut host = host_with_room();
    host.cli().set_env("AGENT_NAME", "review");
    send(&mut host, "hello");
    assert_eq!(author_of_only_record(&mut host), "review");
}

/// REQ: chatroom-B04
#[test]
fn author_b04_ilium_chatroom_author_wins_over_agent_name() {
    let mut host = host_with_room();
    host.cli().set_env("AGENT_NAME", "review");
    host.cli().set_env("ILIUM_CHATROOM_AUTHOR", "fix-auth");
    send(&mut host, "hello");
    assert_eq!(author_of_only_record(&mut host), "fix-auth");
}

/// REQ: chatroom-B04
#[test]
fn author_b04_the_author_option_wins_over_both_variables() {
    let mut host = host_with_room();
    host.cli().set_env("AGENT_NAME", "review");
    host.cli().set_env("ILIUM_CHATROOM_AUTHOR", "fix-auth");
    chat(
        &mut host,
        &["send", "--message", "hello", "--author", "explicit"],
    );
    assert_eq!(author_of_only_record(&mut host), "explicit");
}

/// REQ: chatroom-B10
#[test]
fn output_b10_send_prints_chatroom_message_sent_and_exits_0() {
    let mut host = host_with_room();
    let run = send(&mut host, "hello");
    assert_eq!(run.out, "chatroom message sent\n");
    assert_eq!(run.err, "");
    assert_eq!(run.code, Some(0));
}

/// REQ: chatroom-B10
#[test]
fn output_b10_init_prints_chatroom_ready_at_the_path_of_the_room() {
    let mut host = server_host();
    let run = chat(&mut host, &["init"]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.err);
    let line = run.out.trim_end_matches('\n');
    assert!(line.starts_with("chatroom ready at "), "{line}");
    assert!(line.ends_with("CHATROOM.md"), "{line}");
    assert_eq!(run.out.matches('\n').count(), 1);
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_prints_timestamp_author_content_lines() {
    let mut host = host_with_room();
    send(&mut host, "one");
    chat(
        &mut host,
        &["send", "--message", "two", "--author", "review"],
    );
    let run = chat(&mut host, &["tail"]);
    assert_eq!(run.code, Some(0));
    let lines: Vec<&str> = run.out.lines().collect();
    assert_eq!(lines.len(), 2);
    let first: Vec<&str> = lines[0].splitn(3, " | ").collect();
    assert_eq!(first.len(), 3);
    assert!(is_record_timestamp(first[0]), "{}", first[0]);
    assert_eq!((first[1], first[2]), ("agent", "one"));
    assert!(lines[1].ends_with(" | review | two"), "{}", lines[1]);
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_defaults_to_100_records() {
    let mut host = host_with_records(101);
    let run = chat(&mut host, &["tail"]);
    let lines: Vec<&str> = run.out.lines().collect();
    assert_eq!(lines.len(), 100);
    assert!(lines[0].ends_with(" | agent | record 1"), "{}", lines[0]);
    assert!(
        lines[99].ends_with(" | agent | record 100"),
        "{}",
        lines[99]
    );
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_with_exactly_100_records_prints_all_of_them() {
    let mut host = host_with_records(100);
    assert_eq!(chat(&mut host, &["tail"]).out.lines().count(), 100);
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_with_99_records_prints_99() {
    let mut host = host_with_records(99);
    assert_eq!(chat(&mut host, &["tail"]).out.lines().count(), 99);
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_limit_selects_the_newest_records() {
    let mut host = host_with_records(10);
    let run = chat(&mut host, &["tail", "--limit", "3"]);
    let contents: Vec<String> = run
        .out
        .lines()
        .map(|line| line.rsplit(" | ").next().unwrap_or("").to_string())
        .collect();
    assert_eq!(contents, ["record 7", "record 8", "record 9"]);
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_limit_zero_prints_nothing() {
    let mut host = host_with_records(3);
    let run = chat(&mut host, &["tail", "--limit", "0"]);
    assert_eq!(run.out, "");
    assert_eq!(run.code, Some(0));
}

/// REQ: chatroom-B10
#[test]
fn output_b10_tail_skips_lines_that_are_not_records() {
    let mut host = host_with_room();
    let mut text = room_text(&mut host);
    text.push_str("a hand written note\n");
    text.push_str(&format!("{}\n", numbered_line(1)));
    host.fs().write(ROOM, text.as_bytes());
    host.settle();
    assert_eq!(chat(&mut host, &["tail"]).out.lines().count(), 1);
}

/// REQ: chatroom-B10, chatroom-B11
#[test]
fn output_b10_tail_without_a_room_prints_nothing_and_exits_0() {
    let mut host = server_host();
    let run = chat(&mut host, &["tail"]);
    assert_eq!(run.out, "");
    assert_eq!(run.code, Some(0));
}

/// REQ: chatroom-B11
#[test]
fn errors_b11_send_without_a_room_says_the_project_has_no_chatroom() {
    let mut host = server_host();
    let run = send(&mut host, "hello");
    assert_eq!(run.code, Some(1));
    assert_eq!(run.err, "ilium: this project does not have a chatroom\n");
    assert_eq!(run.out, "");
}

/// REQ: chatroom-B11
#[test]
fn errors_b11_a_failed_send_creates_no_file() {
    let mut host = server_host();
    send(&mut host, "hello");
    assert!(host.fs().read(ROOM).is_none());
}

/// REQ: chatroom-B11
#[test]
fn errors_b11_errors_go_to_stderr_not_stdout() {
    let mut host = host_with_room();
    let run = send(&mut host, "");
    assert_eq!(run.out, "");
    assert!(run.err.starts_with("ilium: "));
}

/// REQ: F-062
#[test]
fn command_f062_send_finds_the_nearest_room_from_a_subdirectory() {
    let mut host = host_with_room();
    host.cli().set_cwd("project:src/deep");
    let run = send(&mut host, "from a subdirectory");
    assert_eq!(run.code, Some(0), "stderr: {}", run.err);
    assert_eq!(room_records(&mut host).len(), 1);
}

/// REQ: F-062
#[test]
fn command_f062_send_without_any_room_in_the_parents_fails_in_the_current_directory() {
    let mut host = server_host();
    host.cli().set_cwd("project:src/deep");
    let run = send(&mut host, "nobody listens");
    assert_eq!(run.code, Some(1));
    assert_eq!(run.err, "ilium: this project does not have a chatroom\n");
}

/// REQ: F-062
#[test]
fn command_f062_help_lists_the_four_subcommands_in_the_summary() {
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest"))
            .expect("json");
    assert_eq!(
        manifest["contributes"]["cli"][0]["summary"],
        "Project chatroom: init, send, context, tail"
    );
}

/// REQ: F-062
#[test]
fn command_f062_send_while_the_server_side_is_the_only_one_running_needs_no_client() {
    // requires_server is false in the manifest: the command works on the file alone.
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest"))
            .expect("json");
    assert_eq!(manifest["contributes"]["cli"][0]["requires_server"], false);
    let mut host = host_with_room();
    assert_eq!(send(&mut host, "no client").code, Some(0));
}

/// REQ: F-062
#[test]
fn command_f062_an_unknown_subcommand_exits_nonzero_and_writes_nothing() {
    let mut host = host_with_room();
    let before = room_text(&mut host);
    let run = chat(&mut host, &["frobnicate"]);
    assert_ne!(run.code, Some(0));
    assert_eq!(room_text(&mut host), before);
}
