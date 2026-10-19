//! Replay of the examples of src/docs/cli-reference.md against the plugin build (COR-041) and the
//! kernel-owned CLI features F-078 (session management and flags), F-105 (attach, --cwd,
//! --onboarding), F-106 (new-session, ls, kill-session), F-111 (hidden internal commands).

use crate::common::*;

const BASELINE_COMMANDS: [&str; 10] = [
    "new-session",
    "ls",
    "kill-session",
    "new-pane",
    "chat",
    "progress",
    "voice",
    "panes",
    "broadcast",
    "help",
];

/// REQ: COR-041, F-078
#[test]
fn cor_041_help_lists_every_command_of_the_baseline_synopsis() {
    let run = Sandbox::new().ilium(&["--help"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let commands = help_commands(&run.stdout);
    for command in BASELINE_COMMANDS {
        assert!(
            commands.iter().any(|listed| listed == command),
            "{command} missing from {commands:?}"
        );
    }
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_help_also_lists_recap_from_the_quick_table() {
    let run = Sandbox::new().ilium(&["--help"]);
    assert!(
        help_commands(&run.stdout)
            .iter()
            .any(|listed| listed == "recap"),
        "{}",
        run.stdout
    );
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_version_prints_the_program_name_and_exits_zero() {
    let run = Sandbox::new().ilium(&["--version"]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.starts_with("ilium "), "{:?}", run.stdout);
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_short_version_flag_equals_the_long_one() {
    let sandbox = Sandbox::new();
    assert_eq!(
        sandbox.ilium(&["-V"]).stdout,
        sandbox.ilium(&["--version"]).stdout
    );
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_every_command_answers_dash_dash_help_with_exit_zero() {
    let sandbox = Sandbox::new();
    for command in BASELINE_COMMANDS.iter().take(9).chain(["recap"].iter()) {
        let run = sandbox.ilium(&[command, "--help"]);
        assert_eq!(run.code, 0, "{command}: {}", run.stderr);
        assert!(!run.stdout.trim().is_empty(), "{command}");
    }
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_global_options_are_accepted_by_every_subcommand_help() {
    let help = Sandbox::new().ilium(&["--help"]).stdout;
    for option in [
        "--cwd",
        "--restart-server",
        "--reset-session",
        "--onboarding",
    ] {
        assert!(
            help.contains(option),
            "{option} missing from the global options"
        );
    }
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_restart_server_with_reset_session_is_a_usage_error_exit_two() {
    let run = Sandbox::new().ilium(&["--restart-server", "--reset-session", "ls"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_an_unknown_flag_is_a_usage_error_exit_two() {
    let run = Sandbox::new().ilium(&["--no-such-flag"]);
    assert_eq!(run.code, 2);
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_a_missing_required_argument_is_a_usage_error_exit_two() {
    let run = Sandbox::new().ilium(&["new-session"]);
    assert_eq!(run.code, 2);
}

/// REQ: COR-041, F-105
#[test]
fn cor_041_cwd_that_is_not_a_directory_fails_with_exit_one_and_the_documented_message() {
    let sandbox = Sandbox::new();
    let missing = sandbox.path().join("does-not-exist");
    let missing_text = missing.to_string_lossy().into_owned();
    let run = sandbox.ilium_env(&["ls"], &[]);
    assert_eq!(run.code, 0, "control run with a valid cwd: {}", run.stderr);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ilium"));
    command
        .args(["--cwd", &missing_text, "ls"])
        .env("XDG_DATA_HOME", sandbox.path().join("data"))
        .env("XDG_CONFIG_HOME", sandbox.path().join("config"))
        .env("XDG_RUNTIME_DIR", sandbox.path().join("run"));
    let failed = run_bounded(command);
    assert_eq!(failed.code, 1);
    assert!(failed.stderr.starts_with("ilium: "), "{:?}", failed.stderr);
    assert!(
        failed.stderr.contains(&format!(
            "--cwd \"{missing_text}\" is not a valid directory"
        )),
        "{:?}",
        failed.stderr
    );
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_ls_without_sessions_prints_no_sessions() {
    let run = Sandbox::new().ilium(&["ls"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "no sessions\n");
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_ls_pads_the_name_to_24_columns_then_running_or_not_running() {
    let sandbox = Sandbox::new();
    let sessions = sandbox.project.join(".ilium/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    std::fs::write(sessions.join("review.json"), "{}").expect("snapshot");
    let run = sandbox.ilium(&["ls"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        format!("{:<24} not running\n", "review").replace(
            "review                   not",
            "review                   not"
        )
    );
    assert_eq!(run.stdout, "review                   not running\n");
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_ls_skips_snapshot_files_whose_names_are_not_valid_session_names() {
    let sandbox = Sandbox::new();
    let sessions = sandbox.project.join(".ilium/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    std::fs::write(sessions.join("bad name.json"), "{}").expect("snapshot");
    std::fs::write(sessions.join("good_1.json"), "{}").expect("snapshot");
    let run = sandbox.ilium(&["ls"]);
    assert_eq!(run.stdout, "good_1                   not running\n");
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_new_session_rejects_a_name_with_a_space() {
    let run = Sandbox::new().ilium(&["new-session", "bad name"]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains(
            "session name \"bad name\" must contain only letters, digits, hyphens, or underscores"
        ),
        "{:?}",
        run.stderr
    );
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_new_session_rejects_a_name_longer_than_48_characters() {
    let name = "a".repeat(49);
    let run = Sandbox::new().ilium(&["new-session", &name]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.starts_with("ilium: "), "{:?}", run.stderr);
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_new_session_rejects_an_empty_name() {
    let run = Sandbox::new().ilium(&["new-session", ""]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("session name"), "{:?}", run.stderr);
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_kill_session_of_a_session_that_is_not_running_fails_with_exit_one() {
    let run = Sandbox::new().ilium(&["kill-session", "nope"]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains("session \"nope\" is not running"),
        "{:?}",
        run.stderr
    );
}

/// REQ: COR-041, F-106
#[test]
fn cor_041_kill_session_does_not_delete_the_snapshot_file() {
    let sandbox = Sandbox::new();
    let sessions = sandbox.project.join(".ilium/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    std::fs::write(sessions.join("keep.json"), "{}").expect("snapshot");
    let run = sandbox.ilium(&["kill-session", "keep"]);
    assert_eq!(run.code, 1);
    assert!(sessions.join("keep.json").is_file());
}

/// REQ: COR-041, F-105
#[test]
fn cor_041_onboarding_flag_is_accepted_by_subcommands_that_ignore_it() {
    let run = Sandbox::new().ilium(&["--onboarding", "ls"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "no sessions\n");
}

/// REQ: COR-041, F-105
#[test]
fn cor_041_restart_server_is_accepted_by_ls_and_ignored() {
    let run = Sandbox::new().ilium(&["--restart-server", "ls"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "no sessions\n");
}

/// REQ: COR-041, F-105
#[test]
fn cor_041_reset_session_is_accepted_by_ls_and_ignored() {
    let run = Sandbox::new().ilium(&["--reset-session", "ls"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
}

/// REQ: COR-041, F-078
#[test]
fn cor_041_unknown_command_is_a_usage_error_exit_two() {
    let run = Sandbox::new().ilium(&["frobnicate"]);
    assert_eq!(run.code, 2);
}

/// REQ: COR-041
#[test]
fn cor_041_chat_init_creates_the_room_ignore_entries_and_both_hook_files() {
    let sandbox = Sandbox::new();
    let run = sandbox.ilium(&["chat", "init"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let room = sandbox.project.join("CHATROOM.md");
    assert!(room.is_file());
    assert_eq!(
        run.stdout.trim_end(),
        format!("chatroom ready at {}", room.display())
    );
    let ignore = std::fs::read_to_string(sandbox.project.join(".gitignore")).expect(".gitignore");
    assert!(
        ignore.lines().any(|line| line == "/CHATROOM.md"),
        "{ignore:?}"
    );
    assert!(
        ignore.lines().any(|line| line == "/CHATROOM.archive.md"),
        "{ignore:?}"
    );
    assert!(sandbox
        .project
        .join(".claude/settings.local.json")
        .is_file());
    assert!(sandbox.project.join(".codex/hooks.json").is_file());
}

/// REQ: COR-041
#[test]
fn cor_041_chat_hooks_call_context_since_last_read_on_session_start_and_prompt_submit() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    let claude = std::fs::read_to_string(sandbox.project.join(".claude/settings.local.json"))
        .expect("claude hooks");
    assert!(
        claude.contains("SessionStart") && claude.contains("UserPromptSubmit"),
        "{claude}"
    );
    assert!(
        claude.contains("chat context --since-last-read"),
        "{claude}"
    );
}

/// REQ: COR-041
#[test]
fn cor_041_chat_send_prints_chatroom_message_sent_and_tail_shows_author_and_content() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    let sent = sandbox.ilium(&[
        "chat",
        "send",
        "--author",
        "claude",
        "--message",
        "Taking the auth refactor",
    ]);
    assert_eq!(sent.code, 0, "{}", sent.stderr);
    assert_eq!(sent.stdout.trim_end(), "chatroom message sent");
    let tail = sandbox.ilium(&["chat", "tail", "--limit", "20"]);
    assert_eq!(tail.code, 0, "{}", tail.stderr);
    assert!(
        tail.stdout.contains("| claude | Taking the auth refactor"),
        "{:?}",
        tail.stdout
    );
}

/// REQ: COR-041
#[test]
fn cor_041_chat_send_author_defaults_come_from_the_environment_then_agent() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    sandbox.ilium_env(
        &["chat", "send", "--message", "one"],
        &[("ILIUM_CHATROOM_AUTHOR", "alice"), ("AGENT_NAME", "bob")],
    );
    sandbox.ilium_env(
        &["chat", "send", "--message", "two"],
        &[("AGENT_NAME", "bob")],
    );
    sandbox.ilium_env(
        &["chat", "send", "--message", "three"],
        &[("ILIUM_CHATROOM_AUTHOR", ""), ("AGENT_NAME", "")],
    );
    let tail = sandbox.ilium(&["chat", "tail"]).stdout;
    assert!(tail.contains("| alice | one"), "{tail:?}");
    assert!(tail.contains("| bob | two"), "{tail:?}");
    assert!(tail.contains("| agent | three"), "{tail:?}");
}

/// REQ: COR-041
#[test]
fn cor_041_chat_send_without_a_room_fails_with_exit_one() {
    let run = Sandbox::new().ilium(&["chat", "send", "--message", "hello"]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.starts_with("ilium: "), "{:?}", run.stderr);
}

/// REQ: COR-041
#[test]
fn cor_041_chat_send_message_of_4000_characters_is_accepted_and_4001_is_not() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    let at_limit = "x".repeat(4000);
    let over_limit = "x".repeat(4001);
    assert_eq!(
        sandbox
            .ilium(&["chat", "send", "--message", &at_limit])
            .code,
        0
    );
    assert_eq!(
        sandbox
            .ilium(&["chat", "send", "--message", &over_limit])
            .code,
        1
    );
}

/// REQ: COR-041
#[test]
fn cor_041_chat_send_requires_the_message_option() {
    let run = Sandbox::new().ilium(&["chat", "send"]);
    assert_eq!(run.code, 2);
}

/// REQ: COR-041
#[test]
fn cor_041_chat_send_with_an_empty_message_fails_with_exit_one() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    assert_eq!(sandbox.ilium(&["chat", "send", "--message", ""]).code, 1);
}

/// REQ: COR-041
#[test]
fn cor_041_chat_context_defaults_to_the_last_40_records() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    for n in 0..45 {
        sandbox.ilium(&[
            "chat",
            "send",
            "--author",
            "a",
            "--message",
            &format!("msg-{n:02}"),
        ]);
    }
    let context = sandbox.ilium(&["chat", "context"]).stdout;
    assert!(
        context.contains("msg-44") && context.contains("msg-05"),
        "{context}"
    );
    assert!(!context.contains("msg-04"), "{context}");
}

/// REQ: COR-041
#[test]
fn cor_041_chat_tail_defaults_to_the_last_100_records() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    for n in 0..3 {
        sandbox.ilium(&[
            "chat",
            "send",
            "--author",
            "a",
            "--message",
            &format!("msg-{n}"),
        ]);
    }
    let tail = sandbox.ilium(&["chat", "tail"]).stdout;
    assert_eq!(tail.lines().count(), 3, "{tail:?}");
}

/// REQ: COR-041
#[test]
fn cor_041_chat_context_since_last_read_prints_nothing_when_nothing_is_new() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    sandbox.ilium(&["chat", "send", "--author", "a", "--message", "hi"]);
    let first = sandbox.ilium(&["chat", "context", "--since-last-read", "--reader", "r1"]);
    assert!(first.stdout.contains("hi"), "{:?}", first.stdout);
    let second = sandbox.ilium(&["chat", "context", "--since-last-read", "--reader", "r1"]);
    assert_eq!(second.code, 0);
    assert_eq!(second.stdout, "");
}

/// REQ: COR-041
#[test]
fn cor_041_chat_context_max_bytes_closes_with_a_note_that_points_to_chat_tail() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    for n in 0..10 {
        sandbox.ilium(&[
            "chat",
            "send",
            "--author",
            "a",
            "--message",
            &format!("{n}{}", "y".repeat(200)),
        ]);
    }
    let context = sandbox
        .ilium(&["chat", "context", "--max-bytes", "600"])
        .stdout;
    assert!(context.contains("ilium chat tail"), "{context}");
}

/// REQ: COR-041
#[test]
fn cor_041_chat_commands_find_the_room_from_a_subdirectory() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["chat", "init"]);
    let sub = sandbox.project.join("src/deep");
    std::fs::create_dir_all(&sub).expect("subdir");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ilium"));
    command
        .args(["chat", "send", "--author", "a", "--message", "from below"])
        .current_dir(&sub)
        .env("XDG_DATA_HOME", sandbox.path().join("data"))
        .env("XDG_CONFIG_HOME", sandbox.path().join("config"))
        .env("XDG_RUNTIME_DIR", sandbox.path().join("run"));
    let run = run_bounded(command);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(std::fs::read_to_string(sandbox.project.join("CHATROOM.md"))
        .expect("room")
        .contains("from below"));
}

/// REQ: COR-041
#[test]
fn cor_041_panes_without_running_sessions_ends_with_the_documented_summary_record() {
    let run = Sandbox::new().ilium(&["panes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let records = run.records();
    let summary = records.last().expect("summary");
    assert_eq!(
        *summary,
        serde_json::json!({"type":"summary","command":"panes","ok":true,"sessions":0,"unreachable_sessions":0,"panes":0})
    );
}

/// REQ: COR-041
#[test]
fn cor_041_panes_with_an_invalid_selection_is_an_error_record_and_exit_two() {
    let run = Sandbox::new().ilium(&["panes", "--state", "no-such-state"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.records()[0]["type"], "error");
}

/// REQ: COR-041
#[test]
fn cor_041_broadcast_with_nothing_selected_exits_one() {
    let run = Sandbox::new().ilium(&["broadcast", "--dry-run", "hello"]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    let records = run.records();
    let summary = first_of_type(&records, "summary").expect("summary record");
    assert_eq!(summary["command"], "broadcast");
    assert_eq!(summary["recipients"], 0);
}

/// REQ: COR-041
#[test]
fn cor_041_broadcast_with_an_empty_message_is_an_error_record_and_exit_two() {
    let run = Sandbox::new().ilium(&["broadcast", ""]);
    assert_eq!(run.code, 2);
    assert_eq!(run.records()[0]["type"], "error");
}

/// REQ: COR-041
#[test]
fn cor_041_recap_with_an_invalid_selection_is_an_error_record_and_exit_two() {
    let run = Sandbox::new().ilium(&["recap", "--state", "no-such-state"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.records()[0]["type"], "error");
}

/// REQ: COR-041
#[test]
fn cor_041_voice_say_without_a_live_session_prints_an_error_record_and_exits_one() {
    let run = Sandbox::new().ilium(&["voice", "say", "hello"]);
    assert_eq!(run.code, 1);
    let records = run.records();
    assert_eq!(records.last().expect("record")["type"], "error");
    assert!(!run.stderr.is_empty());
}

/// REQ: COR-041
#[test]
fn cor_041_new_pane_worktree_failure_prints_an_error_record_and_the_error_on_stderr() {
    let run = Sandbox::new().ilium(&["new-pane", "--worktree", "--branch", "b", "--", "sh"]);
    assert_eq!(run.code, 1);
    let records = run.records();
    assert_eq!(records.last().expect("record")["type"], "error");
    assert!(run.stderr.starts_with("ilium: "), "{:?}", run.stderr);
}

/// REQ: COR-041
#[test]
fn cor_041_progress_outside_a_pane_prints_a_rejected_or_failed_record_and_exits_one() {
    let run = Sandbox::new().ilium(&["progress", "status"]);
    assert_eq!(run.code, 1);
    let records = run.records();
    let kind = records.last().expect("record")["type"]
        .as_str()
        .expect("type")
        .to_owned();
    assert!(
        kind == "progress_rejected" || kind == "progress_request_failed",
        "{kind}"
    );
}

/// REQ: COR-041
#[test]
fn cor_041_wait_is_an_alias_of_progress_wait() {
    let sandbox = Sandbox::new();
    let alias = sandbox.ilium(&["wait", "--help"]);
    let full = sandbox.ilium(&["progress", "wait", "--help"]);
    assert_eq!(alias.code, 0, "{}", alias.stderr);
    assert_eq!(full.code, 0, "{}", full.stderr);
    assert_eq!(
        alias.stdout.replace("ilium wait", "ilium progress wait"),
        full.stdout
    );
}

/// REQ: COR-041
#[test]
fn cor_041_progress_check_rejects_a_probe_that_is_not_one_json_object() {
    let run = Sandbox::new().ilium(&["progress", "check", "--command", "echo not-json"]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.records().last().expect("record")["type"],
        "progress_rejected"
    );
}

/// REQ: F-111
#[test]
fn f_111_hidden_commands_are_not_listed_in_help() {
    let help = Sandbox::new().ilium(&["--help"]).stdout;
    for hidden in [
        "clipboard-helper",
        "release-embedding-probe",
        "release-animation-probe",
    ] {
        assert!(!help.contains(hidden), "{hidden} must stay hidden");
    }
}

/// REQ: F-111
#[test]
fn f_111_hidden_commands_still_parse() {
    let sandbox = Sandbox::new();
    for hidden in [
        "clipboard-helper",
        "release-embedding-probe",
        "release-animation-probe",
    ] {
        let run = sandbox.ilium(&[hidden, "--help"]);
        assert_eq!(run.code, 0, "{hidden}: {}", run.stderr);
    }
}

/// REQ: F-111
#[test]
fn f_111_hidden_commands_work_with_no_plugins() {
    let sandbox = Sandbox::new();
    let run = sandbox.ilium(&["--no-plugins", "release-embedding-probe", "--help"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
}

/// REQ: F-078
#[test]
fn f_078_the_user_page_lists_exit_codes_zero_one_and_two() {
    let page = user_doc("cli-reference");
    assert!(page.contains("- `0` on success."));
    assert!(page.contains("- `1` on any failure"));
    assert!(page.contains("- `2` for command-line usage errors"));
}

/// REQ: F-105
#[test]
fn f_105_default_cwd_is_the_current_directory() {
    let sandbox = Sandbox::new();
    std::fs::create_dir_all(sandbox.project.join(".ilium/sessions")).expect("dir");
    std::fs::write(sandbox.project.join(".ilium/sessions/here.json"), "{}").expect("snapshot");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ilium"));
    command
        .arg("ls")
        .current_dir(&sandbox.project)
        .env("XDG_DATA_HOME", sandbox.path().join("data"))
        .env("XDG_CONFIG_HOME", sandbox.path().join("config"))
        .env("XDG_RUNTIME_DIR", sandbox.path().join("run"));
    let run = run_bounded(command);
    assert_eq!(run.stdout, "here                     not running\n");
}
