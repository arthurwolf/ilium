//! F-071 Include files and setup: `.worktreeinclude` copy with limits, then the setup command via
//! `sh` before the agent pane is created.

use ilium_plugin_sdk::testing::FakeHost;
use proptest::prelude::*;
use serde_json::json;

use crate::common::*;

const ROOT: &str = "/project";

fn seeded() -> FakeHost {
    let mut host = server_host();
    host.fs().seed(
        format!("{ROOT}/.worktreeinclude"),
        b".env\n# comment\n\nconfig/*.toml\n",
    );
    host.fs().seed(format!("{ROOT}/.env"), b"SECRET=1");
    host.fs().seed(format!("{ROOT}/config/a.toml"), b"a=1");
    host
}

fn dest(host: &mut FakeHost, rel: &str) -> Option<Vec<u8>> {
    let add = worktree_add_calls(&host.process().calls());
    let path = add.first()?.arguments[4].clone();
    host.fs().read(format!("{path}/{rel}"))
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::copies_selected_files_and_tracks_created_paths
#[test]
fn port_071_listed_files_are_copied_into_the_new_worktree_and_comments_are_skipped() {
    let mut host = seeded();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert_eq!(dest(&mut host, ".env"), Some(b"SECRET=1".to_vec()));
    assert_eq!(dest(&mut host, "config/a.toml"), Some(b"a=1".to_vec()));
    assert_eq!(dest(&mut host, "# comment"), None);
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::missing_include_file_copies_nothing
#[test]
fn port_071_without_a_worktreeinclude_file_nothing_is_copied_and_creation_succeeds() {
    let mut host = server_host();
    host.fs().seed(format!("{ROOT}/.env"), b"x");
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert_eq!(dest(&mut host, ".env"), None);
    assert_eq!(create_panes(&submitted).len(), 1);
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::existing_destination_is_preserved_and_error_lists_prior_writes
#[test]
fn port_071_an_existing_destination_file_is_never_overwritten() {
    let mut host = seeded();
    host.fs().seed("/project.worktrees/fix-auth/.env", b"KEEP");
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert_ne!(dest(&mut host, ".env"), Some(b"SECRET=1".to_vec()));
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::selected_symlink_and_symlinked_destination_parent_are_rejected
#[test]
fn port_071_a_selected_symlink_is_rejected_and_no_pane_is_created() {
    let mut host = seeded();
    host.fs().symlink(format!("{ROOT}/link"), "/etc/passwd");
    host.fs()
        .seed(format!("{ROOT}/.worktreeinclude"), b"link\n");
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::rollback_refuses_a_replaced_copied_file
#[test]
fn port_071_a_failed_creation_removes_only_the_files_it_copied() {
    let mut host = seeded();
    host.fs().seed(
        format!("{ROOT}/.worktreeinclude"),
        b".env\n/etc/shadow\n../escape\n",
    );
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
    assert_eq!(dest(&mut host, ".env"), None);
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::cancellation_before_discovery_writes_nothing
#[test]
fn port_071_a_cancelled_creation_copies_nothing() {
    let mut host = seeded();
    host.deliver_one(new_agent_message(&json!({"branch": "fix-auth"})));
    host.cancel_pending();
    host.settle();
    assert_eq!(dest(&mut host, ".env"), None);
}

/// REQ: F-071
#[test]
fn feature_071_the_include_file_of_64_kib_is_read_and_one_byte_more_is_refused() {
    for (size, expect_pane) in [
        (64 * 1024 - 1, true),
        (64 * 1024, true),
        (64 * 1024 + 1, false),
    ] {
        let mut host = seeded();
        let mut body = b".env\n".to_vec();
        body.resize(size, b'#');
        host.fs().seed(format!("{ROOT}/.worktreeinclude"), &body);
        let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
        assert_eq!(!create_panes(&submitted).is_empty(), expect_pane, "{size}");
    }
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::byte_limit_rejects_selection_before_writing
#[test]
fn port_071_the_copy_limit_is_64_mib_and_larger_selections_write_nothing() {
    for (size, expect_pane) in [
        (64u64 * 1024 * 1024 - 1, true),
        (64 * 1024 * 1024, true),
        (64 * 1024 * 1024 + 1, false),
    ] {
        let mut host = seeded();
        host.fs().seed_sparse(format!("{ROOT}/big.bin"), size);
        host.fs()
            .seed(format!("{ROOT}/.worktreeinclude"), b"big.bin\n");
        let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
        assert_eq!(!create_panes(&submitted).is_empty(), expect_pane, "{size}");
        if !expect_pane {
            assert_eq!(dest(&mut host, "big.bin"), None);
        }
    }
}

/// REQ: F-071
#[test]
fn feature_071_the_copy_limit_is_1000_files() {
    for (count, expect_pane) in [(999usize, true), (1000, true), (1001, false)] {
        let mut host = server_host();
        for index in 0..count {
            host.fs().seed(format!("{ROOT}/inc/f{index}"), b"x");
        }
        host.fs()
            .seed(format!("{ROOT}/.worktreeinclude"), b"inc/\n");
        let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
        assert_eq!(!create_panes(&submitted).is_empty(), expect_pane, "{count}");
    }
}

/// REQ: F-071, PORT:ilium-server/src/worktree_include.rs::discovery_refuses_excess_source_entries_before_copying
#[test]
fn port_071_more_than_10000_visited_source_entries_are_refused_before_copying() {
    let mut host = server_host();
    for index in 0..10_001 {
        host.fs().seed(format!("{ROOT}/many/f{index}"), b"");
    }
    host.fs()
        .seed(format!("{ROOT}/.worktreeinclude"), b"many/\n");
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
}

fn sh_calls(host: &mut FakeHost) -> Vec<ilium_plugin_sdk::host::process::ProcessSpec> {
    host.process()
        .calls()
        .into_iter()
        .filter(|c| c.program == "sh")
        .collect()
}

/// REQ: F-071
#[test]
fn port_071_the_setup_command_runs_with_sh_dash_c_in_the_worktree_for_120_seconds() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("bun install")).expect("valid");
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let calls = sh_calls(&mut host);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments, ["-c", "bun install"]);
    assert_eq!(calls[0].timeout_millis, 120_000);
    assert!(calls[0]
        .cwd
        .as_ref()
        .map(|c| format!("{c:?}"))
        .unwrap_or_default()
        .contains("fix-auth"));
}

/// REQ: F-071
#[test]
fn port_071_the_command_text_reaches_sh_unchanged() {
    let mut host = server_host();
    let command = "  echo  \"a  b\" && ls\t";
    set_git(&mut host, "setup_command", json!(command)).expect("valid");
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert_eq!(sh_calls(&mut host)[0].arguments[1], command);
}

/// REQ: F-071
#[test]
fn feature_071_no_setup_command_means_no_sh_call() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(sh_calls(&mut host).is_empty());
}

/// REQ: F-071
#[test]
fn feature_071_setup_runs_after_the_worktree_exists_and_before_the_agent_pane() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("true")).expect("valid");
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let programs: Vec<String> = host
        .process()
        .calls()
        .into_iter()
        .map(|c| c.program)
        .collect();
    let git = programs.iter().position(|p| p == "git").expect("git");
    let sh = programs.iter().position(|p| p == "sh").expect("sh");
    assert!(git < sh);
    assert!(host
        .order_of_calls_and_submissions()
        .process_before_submission("sh", "create-pane"));
}

/// REQ: F-071
#[test]
fn port_071_a_failing_setup_keeps_the_worktree_creates_no_pane_and_hides_the_output() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("exit 3")).expect("valid");
    host.process().on_program(
        "sh",
        ilium_plugin_sdk::host::process::ProcessExit {
            status: Some(3),
            signal: None,
            stdout: b"TOKEN=abc".to_vec(),
            stderr: b"TOKEN=abc".to_vec(),
        },
    );
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
    assert!(!host.last_notice().unwrap_or_default().contains("TOKEN=abc"));
    assert!(!host.logs().iter().any(|l| l.contains("TOKEN=abc")));
}

/// REQ: F-071
#[test]
fn port_071_a_setup_timeout_keeps_the_directory_and_creates_no_pane() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("sleep 999")).expect("valid");
    host.process().on_program_timeout("sh");
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: F-071
#[test]
fn port_071_a_cancelled_creation_never_creates_the_pane_after_setup() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("true")).expect("valid");
    host.deliver_one(new_agent_message(&json!({"branch": "fix-auth"})));
    host.cancel_pending();
    host.settle();
    assert!(create_panes(&host.submitted()).is_empty());
}

/// REQ: F-071
#[test]
fn port_071_large_setup_output_is_not_carried_into_any_submission() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("yes | head -c 10000000")).expect("valid");
    host.process().on_program(
        "sh",
        ilium_plugin_sdk::host::process::ProcessExit {
            status: Some(0),
            signal: None,
            stdout: vec![b'y'; 1 << 20],
            stderr: Vec::new(),
        },
    );
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(serde_json::to_string(&submitted).expect("serialises").len() < 64 * 1024);
}

/// REQ: F-071, F-072
#[test]
fn feature_071_setup_is_refused_on_a_platform_other_than_linux() {
    let mut host = server_host();
    host.set_platform("windows");
    set_git(&mut host, "setup_command", json!("true")).expect("valid");
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(sh_calls(&mut host).is_empty());
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: EXT-050, F-071
#[test]
fn feature_071_post_create_steps_entries_run_in_priority_order_after_the_setup_command() {
    let mut host = server_host();
    host.collect_point(POST_CREATE_STEPS)
        .add("b", 60, json!({"title": "B", "command": "echo b"}));
    host.collect_point(POST_CREATE_STEPS)
        .add("a", 50, json!({"title": "A", "command": "echo a"}));
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let commands: Vec<String> = sh_calls(&mut host)
        .iter()
        .map(|c| c.arguments[1].clone())
        .collect();
    assert_eq!(commands, ["echo a", "echo b"]);
}

proptest! {
    // REQ: F-071
    #[test]
    fn feature_071_include_lines_that_are_comments_or_blank_copy_nothing(lines in prop::collection::vec("(#[ -~]{0,20}| *)", 0..8)) {
        let mut host = server_host();
        host.fs().seed(format!("{ROOT}/.worktreeinclude"), lines.join("\n").as_bytes());
        host.fs().seed(format!("{ROOT}/.env"), b"x");
        let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
        prop_assert_eq!(create_panes(&submitted).len(), 1);
        prop_assert_eq!(dest(&mut host, ".env"), None);
    }
}
