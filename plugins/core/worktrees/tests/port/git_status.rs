//! Twins of ilium-server/src/git_status.rs: the probe schedule and the status the server side
//! publishes on the `changed` topic (the originals tested the private probe state).

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::common::*;
use crate::support::*;

const STATUS: [&str; 2] = ["status", "--porcelain"];

fn host_with_worktree() -> FakeHost {
    let mut host = server_host_at("/projects/acme");
    create_worktree(&mut host, &request("agent/original"));
    host.deliver_one(pane_created(14, "agent/original"));
    host.settle();
    host
}

fn git_calls(host: &mut FakeHost) -> usize {
    host.process().calls().len()
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::retry_delay_doubles_and_caps
#[test]
fn port_069_after_failed_probes_the_delay_doubles_from_10_seconds_and_caps_at_300() {
    let mut host = host_with_worktree();
    host.process().default_exit(failed_exit(128));
    host.advance_clock(10_000);
    host.settle();
    for delay_seconds in [10u64, 20, 40, 80, 160, 300, 300, 300] {
        let before = git_calls(&mut host);
        host.advance_clock(delay_seconds * 1000 - 1);
        host.settle();
        assert_eq!(
            git_calls(&mut host),
            before,
            "no retry before {delay_seconds} s"
        );
        host.advance_clock(1);
        host.settle();
        assert!(
            git_calls(&mut host) > before,
            "a retry at {delay_seconds} s"
        );
    }
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::cheap_head_refresh_preserves_last_full_dirty_counts, PORT:ilium-server/src/git_status.rs::fingerprint_detects_reflog_change_and_missing_head
#[test]
fn port_069_a_head_change_publishes_the_new_branch_and_keeps_the_dirty_count() {
    let mut host = host_with_worktree();
    host.process()
        .on_args(&STATUS, stdout_exit(" M a\n M b\n M c\n"));
    host.process().on_args(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        stdout_exit("agent/original\n"),
    );
    host.advance_clock(10_000);
    host.settle();
    assert_eq!(last_changed(&mut host)["modified"], json!(3));
    host.process().on_args(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        stdout_exit("agent/new\n"),
    );
    host.advance_clock(10_000);
    host.settle();
    let published = last_changed(&mut host);
    assert_eq!(published["branch"], json!("agent/new"));
    assert_eq!(published["modified"], json!(3));
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::initial_cheap_status_is_not_claimed_as_full
#[test]
fn port_069_the_first_publication_before_any_probe_is_not_claimed_as_fully_checked() {
    let mut host = host_with_worktree();
    let published = last_changed(&mut host);
    assert!(
        published
            .get("full_checked_at_unix_millis")
            .is_none_or(|value| value.is_null()),
        "{published}"
    );
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::initial_cheap_status_is_not_claimed_as_full
#[test]
fn port_069_a_completed_probe_claims_a_full_check() {
    let mut host = host_with_worktree();
    host.advance_clock(10_000);
    host.settle();
    let published = last_changed(&mut host);
    assert!(
        published["full_checked_at_unix_millis"].is_u64(),
        "{published}"
    );
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::first_cheap_baseline_keeps_full_commit_subject_until_head_changes
#[test]
fn port_069_the_commit_subject_is_kept_while_head_is_unchanged_and_not_reused_after_a_head_change()
{
    let mut host = host_with_worktree();
    host.process().on_args(
        &["log", "-1", "--format=%s"],
        stdout_exit("current commit\n"),
    );
    host.process().on_args(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        stdout_exit("agent/original\n"),
    );
    host.advance_clock(10_000);
    host.settle();
    assert_eq!(
        last_changed(&mut host)["last_commit_subject"],
        json!("current commit")
    );
    host.advance_clock(10_000);
    host.settle();
    assert_eq!(
        last_changed(&mut host)["last_commit_subject"],
        json!("current commit")
    );
    host.process()
        .on_args(&["log", "-1", "--format=%s"], stdout_exit("newer commit\n"));
    host.process().on_args(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        stdout_exit("agent/other\n"),
    );
    host.advance_clock(10_000);
    host.settle();
    assert_ne!(
        last_changed(&mut host)["last_commit_subject"],
        json!("current commit")
    );
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::disappearance_and_return_do_not_reuse_old_full_counts
#[test]
fn port_069_a_missing_worktree_is_published_with_zero_counts_and_no_subject() {
    let mut host = host_with_worktree();
    host.process().on_args(
        &STATUS,
        stdout_exit(" M a\n M b\n M c\n M d\n M e\n M f\n M g\n"),
    );
    host.process()
        .on_args(&["log", "-1", "--format=%s"], stdout_exit("old commit\n"));
    host.advance_clock(10_000);
    host.settle();
    assert_eq!(last_changed(&mut host)["modified"], json!(7));
    host.process().default_exit(failed_exit(128));
    host.advance_clock(10_000);
    host.settle();
    let published = last_changed(&mut host);
    assert_eq!(published["missing"], json!(true));
    assert_eq!(published["modified"], json!(0));
    assert!(published["last_commit_subject"].is_null(), "{published}");
}

/// REQ: F-069, PORT:ilium-server/src/git_status.rs::disappearance_and_return_do_not_reuse_old_full_counts
#[test]
fn port_069_a_worktree_that_returns_does_not_reuse_the_old_counts() {
    let mut host = host_with_worktree();
    host.process().on_args(
        &STATUS,
        stdout_exit(" M a\n M b\n M c\n M d\n M e\n M f\n M g\n"),
    );
    host.advance_clock(10_000);
    host.settle();
    host.process().default_exit(failed_exit(128));
    host.advance_clock(10_000);
    host.settle();
    host.process().default_exit(ok_exit());
    host.process().on_args(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        stdout_exit("agent/returned\n"),
    );
    host.advance_clock(300_000);
    host.settle();
    let published = last_changed(&mut host);
    assert_eq!(published["missing"], json!(false));
    assert_eq!(published["branch"], json!("agent/returned"));
    assert_ne!(published["modified"], json!(7));
    assert!(
        published["last_commit_subject"].is_null()
            || published["last_commit_subject"] != json!("old commit")
    );
}
