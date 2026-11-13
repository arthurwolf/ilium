//! Tests of the pure room rules moved into the plugin (`src/messages.rs`, `src/root.rs`,
//! `src/router.rs`). They need no host: the room file is a string, the file system a map.
//! The kernel tests they extend are in the port map as `adapt-to-plugin-api` rows and stay
//! covered by the FakeHost twins of the sibling files.

use std::collections::{BTreeMap, BTreeSet};

use ilium_plugin_core_chatroom::messages::{
    capped_context, flatten_for_hook_output, format_post, gitignore_with_room, message_fingerprint,
    new_room_text, parse_message_line, parse_messages, reader_cursor_file_name, render_template,
    sanitize_field, tail_messages, unescape_field, unread_context, ChatMessage, PostError,
    UnreadReader, HOOK_COMMAND, HOOK_COMMAND_PREFIX,
};
use ilium_plugin_core_chatroom::root::{
    ancestors, join, main_checkout_of_linked_worktree, resolve_project_root, RoomFileSystem,
};
use ilium_plugin_core_chatroom::router::{
    cursor_after_delivery, notification_lines, parse_batch, parse_record, referenced_names,
    same_agent_name, start_offset, BatchError, FileCursor, MAX_BATCH_BYTES, MAX_BATCH_RECORDS,
    MAX_RECORD_BYTES,
};

const STAMP: &str = "2026-11-13 10:00:00 +01:00";

fn room_with(records: &[(&str, &str)]) -> String {
    let mut text = new_room_text();
    for (author, content) in records {
        text.push_str(&format_post(&text, STAMP, author, content).expect("valid post"));
    }
    text
}

fn reader(key: &str, is_session_start: bool) -> UnreadReader<'_> {
    UnreadReader {
        key,
        is_session_start,
    }
}

// ---- messages.rs ----------------------------------------------------------------------------

#[test]
fn a_new_room_has_title_marker_guidance_and_messages_heading() {
    let text = new_room_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "# ILIUM CHATROOM");
    assert_eq!(lines[1], "<!-- ilium-chatroom: v1 -->");
    assert_eq!(lines[2], "");
    assert!(
        lines[3].starts_with("Agents and the user coordinate here. Use the chatroom sparingly.")
    );
    assert!(lines[3].ends_with("do not rewrite prior records."));
    assert_eq!(lines[4], "");
    assert_eq!(lines[5], "## Messages");
    assert!(text.ends_with("## Messages\n"));
    assert!(parse_messages(&text).is_empty());
}

#[test]
fn fields_round_trip_through_sanitise_and_unescape() {
    for value in [
        "plain",
        "a | b",
        "back\\slash",
        "two\nlines",
        "tab\there",
        "\\|\\n",
    ] {
        assert_eq!(unescape_field(&sanitize_field(value)), value, "{value:?}");
    }
    assert_eq!(sanitize_field("  padded  "), "padded");
    assert_eq!(sanitize_field("a\u{1b}[31mb"), "a[31mb");
}

#[test]
fn multiline_content_is_one_record_line() {
    let room = room_with(&[("agent", "first\nsecond | still second")]);
    assert_eq!(
        room.lines().filter(|line| line.starts_with("- ")).count(),
        1
    );
    let messages = parse_messages(&room);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "first\nsecond | still second");
    assert_eq!(messages[0].timestamp, STAMP);
}

#[test]
fn a_post_is_refused_when_empty_or_too_long() {
    assert_eq!(format_post("", STAMP, "", "x"), Err(PostError::Empty));
    assert_eq!(format_post("", STAMP, "a", " \n "), Err(PostError::Empty));
    assert!(format_post("", STAMP, "a", &"x".repeat(4000)).is_ok());
    assert_eq!(
        format_post("", STAMP, "a", &"x".repeat(4001)),
        Err(PostError::TooLong)
    );
    assert_eq!(
        PostError::TooLong.to_string(),
        "chatroom messages are limited to 4000 characters"
    );
}

#[test]
fn a_missing_trailing_line_end_is_repaired_before_the_record() {
    let appended = format_post("# room\n- old", STAMP, "a", "b").unwrap();
    assert!(appended.starts_with('\n'));
    let appended_clean = format_post("# room\n", STAMP, "a", "b").unwrap();
    assert!(appended_clean.starts_with("- "));
}

#[test]
fn hand_edited_records_cannot_smuggle_raw_terminal_escapes() {
    let line = "- 2026\u{1b}[2J | ag\u{7}ent | hi\u{1b}]0;title\u{7}";
    let message = parse_message_line(line).expect("record shape holds");
    assert_eq!(message.timestamp, "2026[2J");
    assert_eq!(message.author, "agent");
    assert_eq!(message.content, "hi]0;title");
    assert_eq!(flatten_for_hook_output("a\u{1b}b\nc\td"), "ab\\nc\\td");
}

#[test]
fn lines_outside_the_record_shape_are_ignored() {
    let text = "# ILIUM CHATROOM\nnot a record\n- only | two\n- t | a | ok\n";
    let messages = parse_messages(text);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "ok");
}

#[test]
fn tail_keeps_the_newest_records_and_zero_keeps_none() {
    let room = room_with(&[("a", "1"), ("a", "2"), ("a", "3")]);
    let tail = tail_messages(&room, 2);
    assert_eq!(
        tail.iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        ["2", "3"]
    );
    assert!(tail_messages(&room, 0).is_empty());
}

#[test]
fn unread_context_prints_only_records_the_reader_has_not_seen() {
    let mut room = room_with(&[("agent", "first claim"), ("agent", "second claim")]);

    let first = unread_context(&room, 40, 2048, &reader("pane-1", false), None);
    assert!(first.output.contains("first claim") && first.output.contains("second claim"));
    assert!(first.output.contains("Ilium chatroom is enabled"));
    let cursor = first.new_cursor.clone().expect("the room has records");

    let unchanged = unread_context(&room, 40, 2048, &reader("pane-1", false), Some(&cursor));
    assert_eq!(unchanged.output, "");
    assert_eq!(unchanged.new_cursor.as_deref(), Some(cursor.as_str()));

    room.push_str(&format_post(&room, STAMP, "agent", "third claim").unwrap());
    let next = unread_context(&room, 40, 2048, &reader("pane-1", false), Some(&cursor));
    assert!(next.output.starts_with("New ilium chatroom messages"));
    assert!(next.output.contains("third claim"));
    assert!(!next.output.contains("second claim"));
    assert_ne!(next.new_cursor.as_deref(), Some(cursor.as_str()));

    let other = unread_context(&room, 40, 2048, &reader("pane-2", false), None);
    assert!(other.output.contains("first claim") && other.output.contains("third claim"));

    let restart = unread_context(&room, 40, 2048, &reader("pane-1", true), Some(&cursor));
    assert!(restart.output.contains("first claim") && restart.output.contains("third claim"));
}

#[test]
fn a_cursor_that_rotated_out_of_the_room_gets_the_full_header() {
    let room = room_with(&[("agent", "only record")]);
    let outcome = unread_context(&room, 40, 2048, &reader("pane-1", false), Some("gone"));
    assert!(outcome.output.contains("Ilium chatroom is enabled"));
    assert!(outcome.output.contains("only record"));
}

#[test]
fn an_empty_room_prints_the_empty_header_and_stores_no_cursor() {
    let outcome = unread_context(&new_room_text(), 40, 2048, &reader("p", false), None);
    assert!(outcome.output.contains("No messages have been posted yet."));
    assert_eq!(outcome.new_cursor, None);
}

#[test]
fn capped_context_keeps_newest_records_and_counts_the_rest() {
    let mut room = new_room_text();
    for index in 0..30 {
        let post = format_post(
            &room,
            STAMP,
            "agent",
            &format!("record {index} {}", "x".repeat(200)),
        )
        .unwrap();
        room.push_str(&post);
    }
    let output = capped_context(&room, 40, 2048);
    assert!(output.len() <= 2048, "{} bytes", output.len());
    assert!(output.contains("record 29 "));
    assert!(!output.contains("record 0 "));
    assert!(output.contains("earlier unread messages omitted"));

    let post = format_post(&room, STAMP, "agent", &"y".repeat(3900)).unwrap();
    room.push_str(&post);
    let oversized = capped_context(&room, 40, 2048);
    assert!(oversized.len() <= 2048, "{} bytes", oversized.len());
    assert!(oversized.contains("yyyy"));
}

#[test]
fn truncation_never_splits_a_character() {
    let mut room = new_room_text();
    let post = format_post(&room, STAMP, "agent", &"é".repeat(2000)).unwrap();
    room.push_str(&post);
    let output = capped_context(&room, 40, 2048);
    assert!(output.is_char_boundary(output.len()));
    assert!(output.contains('…'));
}

#[test]
fn fingerprints_are_stable_and_separate_the_fields() {
    let first = ChatMessage {
        timestamp: "t".into(),
        author: "ab".into(),
        content: "c".into(),
    };
    let shifted = ChatMessage {
        timestamp: "t".into(),
        author: "a".into(),
        content: "bc".into(),
    };
    assert_eq!(
        message_fingerprint(&first),
        message_fingerprint(&first.clone())
    );
    assert_ne!(message_fingerprint(&first), message_fingerprint(&shifted));
    assert_eq!(message_fingerprint(&first).len(), 32);
}

#[test]
fn reader_cursor_file_names_are_path_safe() {
    assert_eq!(reader_cursor_file_name("pane-1_a"), "pane-1_a.cursor");
    assert_eq!(
        reader_cursor_file_name("../../etc/x y"),
        "______etc_x_y.cursor"
    );
    assert!(reader_cursor_file_name(&"k".repeat(500)).len() <= 96 + ".cursor".len());
}

#[test]
fn the_hook_command_matches_its_upgrade_prefix() {
    assert!(HOOK_COMMAND.starts_with(HOOK_COMMAND_PREFIX));
    assert!(HOOK_COMMAND.contains("--since-last-read"));
}

#[test]
fn gitignore_gets_both_room_entries_once() {
    let updated = gitignore_with_room("target/").expect("entries are missing");
    assert_eq!(updated, "target/\n/CHATROOM.md\n/CHATROOM.archive.md\n");
    assert_eq!(gitignore_with_room(&updated), None);
    let partial = gitignore_with_room("/CHATROOM.md\n").unwrap();
    assert_eq!(partial, "/CHATROOM.md\n/CHATROOM.archive.md\n");
    assert_eq!(
        gitignore_with_room(""),
        Some("/CHATROOM.md\n/CHATROOM.archive.md\n".into())
    );
}

#[test]
fn templates_substitute_without_escaping() {
    assert_eq!(
        render_template("{{v0}} & {{v1}} {{v0}}", &["<a>", "b"]),
        "<a> & b <a>"
    );
}

// ---- root.rs --------------------------------------------------------------------------------

/// A map file system laid out the way `git worktree add` writes a main checkout and a worktree.
#[derive(Default)]
struct MapFileSystem {
    rooms: BTreeSet<String>,
    dot_git_dirs: BTreeSet<String>,
    files: BTreeMap<String, String>,
}

impl RoomFileSystem for MapFileSystem {
    fn has_room(&self, directory: &str) -> bool {
        self.rooms.contains(directory)
    }
    fn dot_git_is_file(&self, directory: &str) -> Option<bool> {
        let key = join(directory, ".git");
        if self.files.contains_key(&key) {
            Some(true)
        } else if self.dot_git_dirs.contains(directory) {
            Some(false)
        } else {
            None
        }
    }
    fn read_dot_git_file(&self, directory: &str) -> Option<String> {
        self.files.get(&join(directory, ".git")).cloned()
    }
    fn read_commondir(&self, gitdir: &str) -> Option<String> {
        self.files.get(&join(gitdir, "commondir")).cloned()
    }
    fn canonicalize(&self, path: &str) -> Option<String> {
        let mut parts: Vec<&str> = Vec::new();
        for part in path
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
        {
            if part == ".." {
                parts.pop()?;
            } else {
                parts.push(part);
            }
        }
        Some(format!("/{}", parts.join("/")))
    }
}

fn repository_with_room() -> MapFileSystem {
    let mut file_system = MapFileSystem::default();
    file_system.rooms.insert("/base/main".into());
    file_system.dot_git_dirs.insert("/base/main".into());
    file_system.files.insert(
        "/base/agent-task/.git".into(),
        "gitdir: /base/main/.git/worktrees/agent-task\n".into(),
    );
    file_system.files.insert(
        "/base/main/.git/worktrees/agent-task/commondir".into(),
        "../..\n".into(),
    );
    file_system
}

#[test]
fn ancestors_walk_up_to_the_root() {
    assert_eq!(ancestors("/a/b/c"), ["/a/b/c", "/a/b", "/a", "/"]);
    assert_eq!(ancestors("/a/b/"), ["/a/b", "/a", "/"]);
    assert_eq!(join("/", "x"), "/x");
    assert_eq!(join("/a", "../b"), "/a/../b");
    assert_eq!(join("/a", "/b"), "/b");
}

#[test]
fn nearest_parent_room_wins() {
    let file_system = repository_with_room();
    assert_eq!(
        resolve_project_root(&file_system, "/base/main/src/deep", None),
        "/base/main"
    );
}

#[test]
fn linked_worktree_resolves_to_the_main_checkout_room() {
    let file_system = repository_with_room();
    assert_eq!(
        resolve_project_root(&file_system, "/base/agent-task/src/deep", None),
        "/base/main"
    );
    assert_eq!(
        resolve_project_root(&file_system, "/base/agent-task", None),
        "/base/main"
    );
    assert_eq!(
        main_checkout_of_linked_worktree(&file_system, "/base/agent-task/x").as_deref(),
        Some("/base/main")
    );
}

#[test]
fn a_room_in_the_worktree_itself_takes_precedence() {
    let mut file_system = repository_with_room();
    file_system.rooms.insert("/base/agent-task".into());
    assert_eq!(
        resolve_project_root(&file_system, "/base/agent-task", None),
        "/base/agent-task"
    );
}

#[test]
fn pane_worktree_covers_a_shell_that_left_its_worktree() {
    let file_system = repository_with_room();
    assert_eq!(
        resolve_project_root(&file_system, "/scratch", Some("/base/agent-task")),
        "/base/main"
    );
    assert_eq!(
        resolve_project_root(&file_system, "/scratch", None),
        "/scratch"
    );
}

#[test]
fn worktree_of_a_repository_without_a_room_falls_back_to_the_cwd() {
    let mut file_system = repository_with_room();
    file_system.rooms.clear();
    assert_eq!(
        resolve_project_root(&file_system, "/base/agent-task", None),
        "/base/agent-task"
    );
}

#[test]
fn malformed_or_submodule_dot_git_files_fall_back_to_the_cwd() {
    let mut file_system = MapFileSystem::default();
    file_system
        .files
        .insert("/root/.git".into(), "not a gitdir pointer\n".into());
    assert_eq!(resolve_project_root(&file_system, "/root", None), "/root");

    file_system
        .files
        .insert("/root/.git".into(), "gitdir: /root/modules/lib\n".into());
    assert_eq!(resolve_project_root(&file_system, "/root", None), "/root");
}

#[test]
fn a_commondir_that_is_not_a_dot_git_directory_is_refused() {
    let mut file_system = repository_with_room();
    file_system.files.insert(
        "/base/main/.git/worktrees/agent-task/commondir".into(),
        "../../..\n".into(),
    );
    assert_eq!(
        main_checkout_of_linked_worktree(&file_system, "/base/agent-task"),
        None
    );
}

#[test]
fn a_parent_room_beats_the_worktree_mapping() {
    let mut file_system = repository_with_room();
    file_system.rooms.clear();
    file_system.rooms.insert("/base".into());
    assert_eq!(
        resolve_project_root(&file_system, "/base/agent-task", None),
        "/base"
    );
}

#[test]
fn a_main_checkout_dot_git_directory_stops_the_worktree_mapping() {
    let mut file_system = MapFileSystem::default();
    file_system.dot_git_dirs.insert("/repo".into());
    assert_eq!(
        main_checkout_of_linked_worktree(&file_system, "/repo/src"),
        None
    );
}

// ---- router.rs ------------------------------------------------------------------------------

const GENERATION: (u64, u64) = (7, 42);

fn record_lines(count: usize) -> String {
    (0..count)
        .map(|index| format!("- t | sender | record-{index}\n"))
        .collect()
}

#[test]
fn parses_escaped_chat_record_and_multiple_references() {
    let record = parse_record("- t | sender | hello\\n@compiler @reviewer").unwrap();
    assert_eq!(record.content, "hello\n@compiler @reviewer");
    assert_eq!(referenced_names(&record.content), ["compiler", "reviewer"]);
}

#[test]
fn references_strip_edge_punctuation_and_skip_bare_at_signs() {
    assert_eq!(
        referenced_names("ping @a-b, @c_d) @ @!! mail@x.org"),
        ["a-b", "c_d"]
    );
    assert!(referenced_names("nobody here").is_empty());
}

#[test]
fn notification_lines_preserve_each_enter_boundary() {
    let lines: Vec<&str> = notification_lines("header\n/goal\n").collect();
    assert_eq!(lines, ["header", "/goal", ""]);
}

#[test]
fn author_names_compare_without_the_leading_at_sign() {
    assert!(same_agent_name("@compiler", "compiler"));
    assert!(same_agent_name("compiler", "@compiler"));
    assert!(!same_agent_name("@compiler", "reviewer"));
}

#[test]
fn append_reader_returns_only_one_ordered_record_batch() {
    let contents = record_lines(70);
    let bytes = &contents.as_bytes()[..MAX_BATCH_BYTES.min(contents.len())];
    let batch = parse_batch(bytes, 0, GENERATION).unwrap();
    assert_eq!(batch.records.len(), MAX_BATCH_RECORDS);
    assert_eq!(batch.records[0].content, "record-0");
    assert_eq!(batch.records[63].content, "record-63");
    let consumed: usize = contents.lines().take(64).map(|line| line.len() + 1).sum();
    assert_eq!(batch.next_cursor.byte_offset, consumed as u64);

    let rest = parse_batch(
        &contents.as_bytes()[consumed..],
        consumed as u64,
        GENERATION,
    )
    .unwrap();
    assert_eq!(rest.records.len(), 6);
    assert_eq!(rest.records[0].content, "record-64");
    assert_eq!(rest.next_cursor.byte_offset, contents.len() as u64);
}

#[test]
fn append_reader_keeps_an_unterminated_record_for_the_next_batch() {
    let contents = "- t | a | whole\n- t | a | half";
    let batch = parse_batch(contents.as_bytes(), 100, GENERATION).unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(
        batch.next_cursor.byte_offset,
        100 + "- t | a | whole\n".len() as u64
    );
}

#[test]
fn append_reader_refuses_a_record_beyond_its_byte_limit_without_advancing() {
    let long = format!("- t | a | {}\n", "x".repeat(MAX_RECORD_BYTES + 1));
    assert_eq!(
        parse_batch(long.as_bytes(), 0, GENERATION),
        Err(BatchError::OversizedRecord)
    );
    let unterminated = format!("- t | a | {}", "x".repeat(MAX_RECORD_BYTES + 1));
    assert_eq!(
        parse_batch(unterminated.as_bytes(), 0, GENERATION),
        Err(BatchError::OversizedRecord)
    );
}

#[test]
fn append_reader_accepts_the_writer_limit_of_four_byte_unicode_messages() {
    let message = "😀".repeat(4000);
    let line = format!("- 2026-11-13 10:00:00 +01:00 | agent | {message}\n");
    assert!(line.len() <= MAX_RECORD_BYTES);
    let batch = parse_batch(line.as_bytes(), 0, GENERATION).unwrap();
    assert_eq!(batch.records[0].content.chars().count(), 4000);
}

#[test]
fn carriage_returns_and_foreign_lines_do_not_make_records() {
    let contents = "# heading\r\n- t | a | with cr\r\nplain\n";
    let batch = parse_batch(contents.as_bytes(), 0, GENERATION).unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].content, "with cr");
    assert_eq!(batch.next_cursor.byte_offset, contents.len() as u64);
}

#[test]
fn offsets_that_would_overflow_are_refused() {
    assert_eq!(
        parse_batch(b"- t | a | b\n", u64::MAX - 2, GENERATION),
        Err(BatchError::OffsetOverflow)
    );
}

#[test]
fn a_replaced_or_truncated_log_restarts_at_the_top() {
    let cursor = FileCursor {
        generation: Some(GENERATION),
        byte_offset: 50,
    };
    assert_eq!(start_offset(cursor, GENERATION, 100), 50);
    assert_eq!(
        start_offset(cursor, (7, 43), 100),
        0,
        "equal-length atomic replacement"
    );
    assert_eq!(
        start_offset(cursor, GENERATION, 49),
        0,
        "truncated below the cursor"
    );
    assert_eq!(
        start_offset(FileCursor::new(50), GENERATION, 100),
        0,
        "first poll"
    );
}

#[test]
fn the_cursor_follows_only_what_was_delivered() {
    let contents = record_lines(3);
    let batch = parse_batch(contents.as_bytes(), 0, GENERATION).unwrap();
    let previous = Some(FileCursor::new(5));
    assert_eq!(
        cursor_after_delivery(previous, &batch, 3, true),
        Some(batch.next_cursor)
    );
    assert_eq!(cursor_after_delivery(previous, &batch, 0, false), previous);
    assert_eq!(
        cursor_after_delivery(previous, &batch, 2, false),
        Some(batch.records[1].next_cursor)
    );
    assert_eq!(cursor_after_delivery(None, &batch, 0, false), None);
}
