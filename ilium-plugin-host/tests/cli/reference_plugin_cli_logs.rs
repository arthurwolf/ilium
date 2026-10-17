//! reference-plugin-cli.md: `logs`, `log` (singular) and `log-level` (DEV-190..195).

use ilium_plugin_host::cli::{DevEvent, MemorySession};

use crate::common::*;
use crate::common_p2::*;

fn logged_session() -> std::sync::Arc<MemorySession> {
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    session.add_log_line("acme.demo", "server", "info", "first");
    session.add_log_line("acme.demo", "host", "warn", "second");
    session.add_log_line("acme.demo", "client", "error", "third");
    session
}

fn messages(output: &ilium_plugin_host::cli::CliOutput) -> Vec<String> {
    recs(output, "log")
        .iter()
        .map(|record| record["message"].as_str().expect("message").to_owned())
        .collect()
}

/// REQ: DEV-190
#[test]
fn dev_190_prints_the_oldest_entry_first() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["logs", "acme.demo", "--json"]);
    assert_eq!(messages(&output), vec!["first", "second", "third"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-190
#[test]
fn dev_190_text_lines_use_the_fixed_log_format() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let text = run(&env, &["logs", "acme.demo"]).stdout_text;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(
        lines[0].ends_with("info  server acme.demo first"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].ends_with("warn  host   acme.demo second"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].ends_with("error client acme.demo third"),
        "{}",
        lines[2]
    );
    let stamp: Vec<char> = lines[0].chars().take(19).collect();
    assert_eq!(
        (stamp[4], stamp[7], stamp[10], stamp[13], stamp[16]),
        ('-', '-', ' ', ':', ':')
    );
}

/// REQ: DEV-190
#[test]
fn dev_190_follow_prints_the_file_entries_then_new_ones_and_exits_130() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session).with_dev_script(vec![
        DevEvent::Log {
            at_ms: 500,
            side: "server".to_owned(),
            level: "info".to_owned(),
            message: "fourth".to_owned(),
        },
        DevEvent::Log {
            at_ms: 900,
            side: "server".to_owned(),
            level: "info".to_owned(),
            message: "fifth".to_owned(),
        },
        DevEvent::Interrupt { at_ms: 1000 },
    ]);
    let output = run(&env, &["logs", "acme.demo", "--follow", "--json"]);
    assert_eq!(
        messages(&output),
        vec!["first", "second", "third", "fourth", "fifth"]
    );
    assert_eq!(output.exit_code, 130);
    assert_eq!(result(&output)["exit_code"], 130);
}

/// REQ: DEV-190
#[test]
fn dev_190_without_follow_it_ends_by_itself_with_exit_zero() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session).with_dev_script(vec![DevEvent::Log {
        at_ms: 500,
        side: "server".to_owned(),
        level: "info".to_owned(),
        message: "late".to_owned(),
    }]);
    let output = run(&env, &["logs", "acme.demo", "--json"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(messages(&output), vec!["first", "second", "third"]);
}

/// REQ: DEV-190
#[test]
fn dev_190_follow_with_no_entry_yet_still_waits_for_the_interrupt() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session)
        .with_dev_script(vec![DevEvent::Interrupt { at_ms: 1000 }]);
    let output = run(&env, &["logs", "acme.demo", "--follow", "--json"]);
    assert!(messages(&output).is_empty());
    assert_eq!(output.exit_code, 130);
}

/// REQ: DEV-191
#[test]
fn dev_191_an_unknown_id_without_a_kept_log_fails_not_found_with_exit_five() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["logs", "acme.nothing"]);
    assert_eq!(code(&output).as_deref(), Some("plugin.not-found"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: DEV-191
#[test]
fn dev_191_a_kept_log_of_an_uninstalled_plugin_can_still_be_read() {
    let root = scratch();
    let session = MemorySession::new();
    session.keep_log("acme.gone", &[("server", "info", "last words")]);
    let env = env_session(root.path(), &session);
    let output = run(&env, &["logs", "acme.gone", "--json"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(messages(&output), vec!["last words"]);
}

/// REQ: DEV-191
#[test]
fn dev_191_an_installed_plugin_with_no_entries_is_not_an_error() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let output = run(&env, &["logs", "acme.demo", "--json"]);
    assert_eq!(output.exit_code, 0);
    assert!(messages(&output).is_empty());
}

/// REQ: DEV-191
#[test]
fn dev_191_the_json_form_ends_with_summary_and_result() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["logs", "acme.demo", "--json"]);
    let kinds = types(&output);
    assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"]);
}

/// REQ: DEV-192
#[test]
fn dev_192_the_singular_log_is_not_a_command() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["log", "acme.demo"]);
    assert_eq!(output.exit_code, 2);
    assert!(recs(&output, "log").is_empty());
    assert_eq!(result(&output)["ok"], false);
}

/// REQ: DEV-192
#[test]
fn doc_dev_192_no_manual_page_shows_ilium_plugin_log_as_a_command() {
    let pages =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages");
    let entries = std::fs::read_dir(&pages).expect("manual pages directory");
    let mut offenders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read page");
        for (number, line) in text.lines().enumerate() {
            if line.contains("singular")
                || line.contains("not a command")
                || line.contains("is wrong")
            {
                continue;
            }
            let mut rest = line;
            while let Some(position) = rest.find("ilium plugin log") {
                let after = &rest[position + "ilium plugin log".len()..];
                let next = after.chars().next();
                if next != Some('s') && next != Some('-') {
                    offenders.push(format!("{}:{}", path.display(), number + 1));
                }
                rest = after;
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "pages show the singular command: {offenders:?}"
    );
}

/// REQ: DEV-195
#[test]
fn dev_195_without_an_argument_it_prints_the_current_threshold() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["log-level", "acme.demo"]);
    assert_eq!(output.exit_code, 0);
    let all = format!(
        "{}{}{}",
        output.stdout_raw, output.stdout_text, output.stderr
    );
    assert!(all.contains("info"), "{all}");
}

/// REQ: DEV-195
#[test]
fn dev_195_a_level_sets_the_threshold_for_the_plugin() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    assert_eq!(run(&env, &["log-level", "acme.demo", "debug"]).exit_code, 0);
    assert_eq!(session.log_level("acme.demo"), "debug");
    let all = {
        let output = run(&env, &["log-level", "acme.demo"]);
        format!(
            "{}{}{}",
            output.stdout_raw, output.stdout_text, output.stderr
        )
    };
    assert!(all.contains("debug"), "{all}");
}

/// REQ: DEV-195
#[test]
fn dev_195_every_level_is_accepted() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    for level in ["trace", "debug", "info", "warn", "error"] {
        assert_eq!(
            run(&env, &["log-level", "acme.demo", level]).exit_code,
            0,
            "{level}"
        );
        assert_eq!(session.log_level("acme.demo"), level);
    }
}

/// REQ: DEV-195
#[test]
fn dev_195_reset_returns_to_the_session_default() {
    let root = scratch();
    let session = logged_session();
    session.set_default_log_level("warn");
    let env = env_session(root.path(), &session);
    run(&env, &["log-level", "acme.demo", "trace"]);
    let output = run(&env, &["log-level", "acme.demo", "--reset"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(session.log_level("acme.demo"), "warn");
}

/// REQ: DEV-195
#[test]
fn dev_195_the_threshold_of_one_plugin_does_not_change_another() {
    let root = scratch();
    let session = logged_session();
    session.add_plugin("acme.other", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    run(&env, &["log-level", "acme.demo", "error"]);
    assert_eq!(session.log_level("acme.other"), "info");
}

/// REQ: DEV-195
#[test]
fn dev_195_the_setting_lives_only_as_long_as_the_session() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    run(&env, &["log-level", "acme.demo", "trace"]);
    let fresh = logged_session();
    assert_eq!(fresh.log_level("acme.demo"), "info");
}

/// REQ: DEV-195
#[test]
fn dev_195_an_unknown_level_is_a_usage_error() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["log-level", "acme.demo", "loud"]);
    assert_eq!(code(&output).as_deref(), Some("usage.invalid-value"));
    assert_eq!(output.exit_code, 2);
    assert_eq!(session.log_level("acme.demo"), "info");
}

/// REQ: DEV-195
#[test]
fn dev_195_a_level_together_with_reset_is_a_conflict() {
    let root = scratch();
    let session = logged_session();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["log-level", "acme.demo", "debug", "--reset"]);
    assert_eq!(code(&output).as_deref(), Some("usage.conflict"));
    assert_eq!(output.exit_code, 2);
}
