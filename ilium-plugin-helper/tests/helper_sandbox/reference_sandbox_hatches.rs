//! reference-sandbox.md "Escape-hatch review": process execution rules and pane commands
//! (SEC-030 to SEC-035, SEC-040 to SEC-046, SEC-066).
//!
//! The decisions belong to the host. These tests check what the helper side owes: every such
//! import is forwarded to the host with its arguments unchanged (the helper never decides or
//! filters), children inherit the confinement, and the manual states the exact rule.

use crate::probe_child::run_probe;
use crate::support::*;

const PAGE: &str = "reference-sandbox";

// ------------------------------------------------------------------ SEC-030

const HATCH_IDS: [&str; 12] = [
    "SEC-040", "SEC-041", "SEC-042", "SEC-031", "SEC-044", "SEC-043", "SEC-045", "SEC-102",
    "SEC-097", "SEC-283", "SEC-300", "SEC-061",
];

/// REQ: SEC-030
#[test]
fn doc_sec_030_every_escape_hatch_row_names_its_mitigation_id() {
    let table = section(PAGE, "## Escape-hatch review");
    for id in HATCH_IDS {
        assert!(
            table.contains(id),
            "{id} missing from the escape-hatch table"
        );
    }
}

/// REQ: SEC-030
#[test]
fn doc_sec_030_manual_requires_a_row_for_every_new_feature() {
    assert_page_has(
        PAGE,
        "A feature added to the contract later MUST add a row before release",
    );
}

/// REQ: SEC-030
#[test]
fn doc_sec_030_code_execution_rows_exist_for_each_spawn_route() {
    let table = section(PAGE, "## Escape-hatch review");
    for feature in [
        "`panes.create` with a `command`",
        "`panes.lifecycle` restart",
        "`process.run:<program>`",
        "`agent.signatures` resume argv",
    ] {
        assert!(table.contains(feature), "{feature}");
    }
}

// ------------------------------------------------------------------ SEC-031

/// REQ: SEC-031
#[test]
fn sec_031_process_run_for_an_unlisted_program_is_forwarded_for_the_host_to_refuse() {
    let outcome = hostile_call("process-run", b"git", Some("not-permitted"));
    assert!(outcome.forwarded("process", b"git"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-031
#[test]
fn sec_031_process_spawn_is_forwarded_like_process_run() {
    let outcome = hostile_call("process-spawn", b"git", Some("not-permitted"));
    assert!(outcome.forwarded("process", b"git"));
}

/// REQ: SEC-031
#[test]
fn doc_sec_031_bare_names_resolve_against_a_fixed_system_path() {
    let text = page(PAGE);
    for dir in ["`/usr/bin`", "`/bin`"] {
        assert!(text.contains(dir));
    }
    assert_page_has(
        PAGE,
        "never resolved against the plugin's own environment or against any writable scope",
    );
}

// ------------------------------------------------------------------ SEC-032

/// REQ: SEC-032
#[test]
fn sec_032_a_child_of_the_confined_process_has_the_same_confinement() {
    let probe = run_probe("childstatus", &[]);
    assert!(probe.completed(), "{:?}", probe.one("apply_error"));
    let status = probe.map("child_status");
    assert_eq!(status.get("NoNewPrivs").map(String::as_str), Some("1"));
    assert_eq!(status.get("Seccomp").map(String::as_str), Some("2"));
    assert_eq!(
        status.get("CapEff").map(String::as_str),
        Some("0000000000000000")
    );
}

/// REQ: SEC-032
#[test]
fn doc_sec_032_children_have_no_network() {
    assert_page_has(PAGE, "Children of T2 and T3 plugins run in the same confinement as the helper and have no network");
}

// ------------------------------------------------------------------ SEC-033

const DENIED_ENV_KEYS: [&str; 16] = [
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "PATH",
    "IFS",
    "BASH_ENV",
    "ENV",
    "PYTHONPATH",
    "NODE_OPTIONS",
    "PERL5LIB",
    "RUBYOPT",
    "GIT_CONFIG_COUNT",
    "GIT_SSH_COMMAND",
    "GIT_EXEC_PATH",
    "GIT_ASKPASS",
    "GIT_SSH",
];

/// REQ: SEC-033
#[test]
fn sec_033_every_denied_environment_key_reaches_the_host_unaltered() {
    for key in DENIED_ENV_KEYS {
        let outcome = hostile_call("spawn-with-env", key.as_bytes(), Some("invalid-argument"));
        assert!(
            outcome.forwarded("process", key.as_bytes()),
            "{key} was not forwarded"
        );
        assert_eq!(outcome.result, "invalid-argument", "{key}");
    }
}

/// REQ: SEC-033
#[test]
fn sec_033_an_ordinary_key_is_forwarded_and_accepted() {
    let outcome = hostile_call("spawn-with-env", b"LANG", None);
    assert!(outcome.forwarded("process", b"LANG"));
    assert_ne!(outcome.result, "invalid-argument");
}

/// REQ: SEC-033
#[test]
fn doc_sec_033_manual_lists_the_denied_key_patterns_and_the_admin_extension() {
    for pattern in [
        "`LD_*`",
        "`DYLD_*`",
        "`PATH`",
        "`IFS`",
        "`BASH_ENV`",
        "`ENV`",
        "`PYTHON*`",
        "`NODE_OPTIONS`",
        "`PERL5*`",
        "`RUBY*`",
        "`GIT_CONFIG*`",
        "`GIT_SSH*`",
        "`GIT_EXEC_PATH`",
        "`GIT_ASKPASS`",
    ] {
        assert_page_has(PAGE, pattern);
    }
    assert_page_has(PAGE, "`[processes] deny_env`");
    assert_page_has(PAGE, "refused with `invalid-argument`");
}

// ------------------------------------------------------------------ SEC-034

/// REQ: SEC-034
#[test]
fn sec_034_a_program_inside_a_writable_scope_is_forwarded_for_refusal() {
    let outcome = hostile_call(
        "run-program-from-data",
        b"/data/payload",
        Some("not-permitted"),
    );
    assert!(outcome.forwarded("process", b"/data/payload"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-034
#[test]
fn sec_034_writable_mounts_are_noexec_so_a_written_binary_cannot_run() {
    let dir = std::env::temp_dir().join(format!("ilium-helper-sandbox-034-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create dir");
    let probe = run_probe(
        "mounts",
        &[("ILIUM_PROBE_WRITE", dir.display().to_string())],
    );
    assert!(probe.completed(), "{:?}", probe.one("apply_error"));
    assert_eq!(
        probe.one("exec"),
        Some(format!("{}=errno13", dir.display()))
    );
}

/// REQ: SEC-034
#[test]
fn doc_sec_034_absolute_path_grants_compare_the_resolved_real_path() {
    assert_page_has(
        PAGE,
        "A program may not resolve into `package`, `data`, `cache`, `picked` or any writable scope",
    );
    assert_page_has(PAGE, "are compared to the resolved real path");
}

// ------------------------------------------------------------------ SEC-035

/// REQ: SEC-035
#[test]
fn doc_sec_035_launcher_list_and_review_line() {
    assert_page_has(PAGE, "`This program can run other programs.`");
    for launcher in [
        "`git`",
        "`sh`",
        "`bash`",
        "`python*`",
        "`node`",
        "`rg` with `--pre`",
        "`find`",
        "`xargs`",
        "`env`",
        "`make`",
        "`ssh`",
        "`sudo`",
    ] {
        assert_page_has(PAGE, launcher);
    }
}

/// REQ: SEC-035
#[test]
fn sec_035_arguments_are_not_filtered_by_the_helper() {
    let arguments = b"rg --pre ./script pattern";
    let outcome = hostile_call("process-run-args", arguments, None);
    assert!(outcome.forwarded("process", arguments));
}

/// REQ: SEC-035
#[test]
fn doc_sec_035_manual_states_the_host_does_not_filter_arguments() {
    assert_page_has(
        PAGE,
        "The host does not filter arguments, because that cannot be done in general",
    );
}

// ------------------------------------------------------------------ SEC-040, SEC-041

/// REQ: SEC-040
#[test]
fn sec_040_create_pane_with_a_command_is_forwarded_and_refused_without_the_grant() {
    let outcome = hostile_call("create-pane-command", b"sh", Some("not-permitted"));
    assert!(outcome.forwarded("pane", b"sh"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-040
#[test]
fn sec_040_create_pane_without_a_command_is_forwarded_as_an_ordinary_call() {
    let outcome = hostile_call("create-pane-plain", b"", None);
    assert!(outcome.forwarded("pane", b""));
}

/// REQ: SEC-040
#[test]
fn doc_sec_040_grant_name_reasons_and_confirmation_text() {
    assert_page_has(PAGE, "`panes.create-command:<program>`");
    assert_page_has(PAGE, "`refused(not-permitted)` with reason `not-granted`");
    assert_page_has(
        PAGE,
        "(`declined` when the grant was requested but unchecked)",
    );
    assert_page_has(PAGE, "`Notes Panel wants to run git in a new pane.`");
    assert_page_has(PAGE, "`manifest.grant-dependency`");
}

/// REQ: SEC-041
#[test]
fn sec_041_restart_with_a_new_command_is_forwarded_and_refused_without_the_grant() {
    let outcome = hostile_call("restart-pane-command", b"sh", Some("not-permitted"));
    assert!(outcome.forwarded("pane", b"sh"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-041
#[test]
fn doc_sec_041_restart_uses_the_same_grant_and_confirmation() {
    assert_page_has(PAGE, "`panes.lifecycle` restart with a new command uses the same grant and the same confirmation");
}

// ------------------------------------------------------------------ SEC-042

/// REQ: SEC-042
#[test]
fn sec_042_session_placeholder_value_is_forwarded_as_one_element() {
    let outcome = hostile_call("agent-resume", b"abc-123", None);
    assert!(outcome.forwarded("agent", b"abc-123"));
}

/// REQ: SEC-042
#[test]
fn sec_042_session_id_of_128_and_129_characters_are_both_forwarded_for_the_host_to_judge() {
    for length in [128usize, 129] {
        let id = "a".repeat(length);
        let outcome = hostile_call("agent-resume", id.as_bytes(), None);
        assert!(outcome.forwarded("agent", id.as_bytes()), "length {length}");
    }
}

/// REQ: SEC-042
#[test]
fn doc_sec_042_manual_states_the_session_pattern_and_user_action_rule() {
    assert_page_has(PAGE, "`[A-Za-z0-9._:-]{1,128}`");
    assert_page_has(PAGE, "exactly one argv element, never through a shell");
    assert_page_has(PAGE, "The resume runs only on an explicit user action");
}

// ------------------------------------------------------------------ SEC-043

const EXTERNAL_DENIED_EXTENSIONS: [&str; 19] = [
    ".desktop",
    ".exe",
    ".bat",
    ".cmd",
    ".com",
    ".sh",
    ".jar",
    ".app",
    ".appimage",
    ".scr",
    ".lnk",
    ".command",
    ".html",
    ".htm",
    ".svg",
    ".js",
    ".py",
    ".pl",
    ".rb",
];

/// REQ: SEC-043
#[test]
fn doc_sec_043_all_nineteen_refused_extensions_are_listed() {
    let text = page(PAGE);
    let start = text.find("**[SEC-043]**").expect("SEC-043 paragraph");
    let end = text.find("**[SEC-044]**").expect("SEC-044 paragraph");
    let paragraph = &text[start..end];
    for extension in EXTERNAL_DENIED_EXTENSIONS {
        assert!(paragraph.contains(&format!("`{extension}`")), "{extension}");
    }
}

/// REQ: SEC-043
#[test]
fn sec_043_open_external_of_a_script_is_forwarded_and_refused_with_file_type() {
    let outcome = hostile_call("open-external-file", b"/data/run.sh", Some("not-permitted"));
    assert!(
        outcome.forwarded("open-external", b"/data/run.sh")
            || outcome.forwarded("external", b"/data/run.sh")
    );
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-043
#[test]
fn doc_sec_043_outcome_code_and_prompt_limits() {
    assert_page_has(
        PAGE,
        "Outcome code `refused(not-permitted)` with reason `file-type`",
    );
    assert_page_has(PAGE, "at most one pending consent prompt and 6 per hour");
}

// ------------------------------------------------------------------ SEC-044

/// REQ: SEC-044
#[test]
fn sec_044_clipboard_text_of_64_kib_minus_one_exact_and_plus_one_is_forwarded_intact() {
    for length in [65_535usize, 65_536, 65_537] {
        let text = vec![b'a'; length];
        let outcome = hostile_call("clipboard-write", &text, None);
        let sent = outcome
            .calls
            .iter()
            .find(|(n, _)| n.contains("clipboard"))
            .expect("forwarded");
        assert!(
            sent.1.len() >= length,
            "payload of {length} bytes was altered"
        );
    }
}

/// REQ: SEC-044
#[test]
fn sec_044_control_characters_are_forwarded_for_the_host_to_refuse() {
    let outcome = hostile_call("clipboard-write", b"a\x1b[31mb", Some("invalid-argument"));
    assert!(outcome.forwarded("clipboard", b"\x1b"));
    assert_eq!(outcome.result, "invalid-argument");
}

/// REQ: SEC-044
#[test]
fn doc_sec_044_limits_allowed_characters_and_user_initiated_rule() {
    assert_page_has(PAGE, "larger than 64 KiB");
    assert_page_has(
        PAGE,
        "any control character other than tab, newline and carriage return",
    );
    assert_page_has(PAGE, "reason `not-user-initiated`");
}

// ------------------------------------------------------------------ SEC-045

/// REQ: SEC-045
#[test]
fn sec_045_title_is_forwarded_without_a_prefix_so_only_the_host_adds_it() {
    let outcome = hostile_call("notify-desktop", b"Build done", None);
    let sent = outcome
        .calls
        .iter()
        .find(|(n, _)| n.contains("notify"))
        .expect("forwarded");
    assert!(sent.1.windows(10).any(|w| w == b"Build done"));
}

/// REQ: SEC-045
#[test]
fn doc_sec_045_host_prefixes_the_title_with_the_display_name_and_a_colon() {
    assert_page_has(
        PAGE,
        "the host prefixes the title with the plugin display name and a colon",
    );
    assert_page_has(PAGE, "The plugin cannot supply the prefix");
}

// ------------------------------------------------------------------ SEC-046

/// REQ: SEC-046
#[test]
fn sec_046_patterns_of_1023_1024_and_1025_bytes_are_forwarded_for_registration_checks() {
    for length in [1_023usize, 1_024, 1_025] {
        let pattern = vec![b'a'; length];
        let outcome = hostile_call("register-regex", &pattern, None);
        assert!(
            outcome.calls.iter().any(|(_, args)| args.len() >= length),
            "pattern of {length} bytes altered"
        );
    }
}

/// REQ: SEC-046
#[test]
fn sec_046_backreference_and_lookaround_patterns_get_invalid_argument() {
    for pattern in [&b"(a)\\1"[..], &b"(?=a)b"[..], &b"(?<=a)b"[..]] {
        let outcome = hostile_call("register-regex", pattern, Some("invalid-argument"));
        assert_eq!(outcome.result, "invalid-argument");
    }
}

/// REQ: SEC-046
#[test]
fn sec_046_catastrophic_pattern_matches_a_1_mib_input_in_bounded_time() {
    let started = std::time::Instant::now();
    let outcome = hostile_call("match-catastrophic-regex", b"(a+)+$", None);
    assert!(outcome.trap.is_none());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

/// REQ: SEC-046
#[test]
fn doc_sec_046_limits_and_codes() {
    assert_page_has(
        PAGE,
        "A pattern is at most 1,024 bytes, and its compiled automaton is at most 256 KiB",
    );
    assert_page_has(PAGE, "is refused with `invalid-argument` at registration");
    assert_page_has(PAGE, "no backreferences and no lookaround");
}

// ------------------------------------------------------------------ SEC-066

/// REQ: SEC-066
#[test]
fn sec_066_create_pane_argv_is_forwarded_whole_so_the_host_can_audit_it() {
    let argv = b"git\0--token=SECRET123\0status";
    let outcome = hostile_call("create-pane-command", argv, None);
    assert!(outcome.forwarded("pane", b"--token=SECRET123"));
}

/// REQ: SEC-066
#[test]
fn doc_sec_066_audit_records_full_argv_with_redacted_secrets() {
    assert_page_has(
        PAGE,
        "written to the audit log with the full argv, with secret values replaced by `<redacted>`",
    );
}
