//! Twins of `ilium-git/src/parse.rs` and `ilium-git/tests/real_repo.rs`.
//!
//! The parser tests are pure and move unchanged. The real-repository tests spawned `git`; their
//! plugin twins check the same behaviour over captured output in `tests/fixtures/git/` and over the
//! argument vectors the plugin hands to the `process` import (running them is gap G10).

use ilium_plugin_core_worktrees::git::commands::{self, RemovalFacts};
use ilium_plugin_core_worktrees::git::parse::{git_version, status, worktrees, worktrees_lines};
use ilium_plugin_core_worktrees::git::{parse, GitError, GitVersion, Worktree};

const TWO_WORKTREES: &[u8] = include_bytes!("../fixtures/git/worktree-list-two.z.txt");
const UNTRACKED_STATUS: &[u8] = include_bytes!("../fixtures/git/status-untracked.z.txt");
const IGNORED_STATUS: &[u8] = include_bytes!("../fixtures/git/status-ignored.z.txt");
const BRANCHES: &[u8] = include_bytes!("../fixtures/git/branches.txt");
const VERSION: &[u8] = include_bytes!("../fixtures/git/version.txt");
const TIP: &str = "1111111111111111111111111111111111111111";

fn listed() -> Vec<Worktree> {
    worktrees(TWO_WORKTREES).expect("fixture parses")
}

/// REQ: F-067, PORT:ilium-git/src/parse.rs::parses_worktree_porcelain_with_detached_and_locked_entries
#[test]
fn parses_worktree_porcelain_with_detached_and_locked_entries() {
    let output = b"worktree /tmp/main\0HEAD abc\0branch refs/heads/main\0\0worktree /tmp/linked\0HEAD def\0detached\0locked reason\0\0";
    let parsed = worktrees(output).expect("parse worktrees");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].branch.as_deref(), Some("main"));
    assert!(parsed[1].is_detached);
    assert!(parsed[1].is_locked);
}

/// REQ: F-067, PORT:ilium-git/src/parse.rs::parses_status_with_rename_extra_field_and_counts
#[test]
fn parses_status_with_rename_extra_field_and_counts() {
    let output = b"# branch.oid abc\0# branch.head feature/x\0# branch.upstream origin/x\0# branch.ab +3 -2\0\
                   1 .M N... 100644 100644 100644 a b file\0";
    let mut extended = output.to_vec();
    extended.extend_from_slice(b"2 R. N... 100644 100644 100644 a b R100 new\0old\0u UU N... 100644 100644 100644 100644 a b c conflict\0? new file\0");
    let parsed = status(&extended).expect("parse status");
    assert_eq!(parsed.branch.as_deref(), Some("feature/x"));
    assert_eq!((parsed.ahead, parsed.behind), (3, 2));
    assert_eq!(
        (
            parsed.staged,
            parsed.modified,
            parsed.untracked,
            parsed.conflicted
        ),
        (1, 1, 1, 1)
    );
}

/// REQ: F-067, PORT:ilium-git/src/parse.rs::malformed_records_are_rejected
#[test]
fn malformed_records_are_rejected() {
    assert!(worktrees(b"branch refs/heads/main\0").is_err());
    assert!(status(b"2 R. metadata new\0").is_err());
    assert!(status(b"? untracked").is_err());
    assert!(worktrees_lines(b"worktree /tmp/a\nunexpected newline segment\nHEAD abc\n\n").is_err());
    assert_eq!(
        worktrees_lines(b"worktree /tmp/a\nHEAD abc\nbranch refs/heads/main\n\n")
            .expect("old porcelain")[0]
            .branch
            .as_deref(),
        Some("main")
    );
}

/// REQ: F-067, PORT:ilium-git/src/parse.rs::status_counts_non_utf8_filenames_without_decoding_paths
#[test]
fn status_counts_non_utf8_filenames_without_decoding_paths() {
    let parsed = status(b"# branch.head main\0? file-\xff\0").expect("count raw filename");
    assert_eq!(parsed.untracked, 1);
}

/// REQ: F-067, PORT:ilium-git/src/parse.rs::git_version_accepts_distribution_suffix
#[test]
fn git_version_accepts_distribution_suffix() {
    assert_eq!(
        git_version(b"git version 2.39.5.windows.1\n").expect("version"),
        GitVersion {
            major: 2,
            minor: 39,
            patch: 5
        }
    );
}

/// REQ: F-067, PORT:ilium-git/tests/real_repo.rs::discovers_same_common_directory_from_nested_and_linked_worktrees
#[test]
fn discovers_same_common_directory_from_nested_and_linked_worktrees() {
    // The twin covers the listing half over captured output; common-directory discovery needs
    // `rev-parse --git-common-dir` through the process import (G10).
    let version = git_version(VERSION).expect("fixture version");
    assert_eq!(
        commands::list_worktrees(version),
        ["worktree", "list", "--porcelain", "-z"]
    );
    let worktrees = listed();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[1].branch.as_deref(), Some("agent/test"));
    let branches = parse::branches(BRANCHES).expect("branches");
    assert!(branches.contains(&"agent/test".to_owned()));
    let old = GitVersion {
        major: 2,
        minor: 30,
        patch: 0,
    };
    assert_eq!(
        commands::list_worktrees(old),
        ["worktree", "list", "--porcelain"]
    );
    assert!(commands::supports_worktrees(version));
    assert!(!commands::supports_worktrees(GitVersion {
        major: 2,
        minor: 16,
        patch: 9
    }));
}

/// REQ: F-067, PORT:ilium-git/tests/real_repo.rs::preserves_dirty_linked_worktree_and_rejects_main_checkout_removal
#[test]
fn preserves_dirty_linked_worktree_and_rejects_main_checkout_removal() {
    let state = status(UNTRACKED_STATUS).expect("status");
    assert_eq!(state.branch.as_deref(), Some("agent/dirty"));
    assert_eq!(state.untracked, 1);
    assert!(!state.is_clean());
    let listed = listed();
    assert!(matches!(
        commands::remove_worktree(&listed, "/repo/main", "/repo/main", false),
        Err(GitError::InvalidInput(_))
    ));
    assert_eq!(
        commands::remove_worktree(&listed, "/repo/main-linked", "/repo/main", false)
            .expect("linked removal"),
        ["worktree", "remove", "--", "/repo/main-linked"]
    );
    assert_eq!(
        commands::remove_worktree(&listed, "/repo/main-linked", "/repo/main", true)
            .expect("forced removal"),
        ["worktree", "remove", "--force", "--", "/repo/main-linked"]
    );
    assert!(commands::remove_worktree(&listed, "/elsewhere", "/repo/main", true).is_err());
}

/// REQ: F-067, PORT:ilium-git/tests/real_repo.rs::rollback_pristine_check_detects_ignored_hook_output
#[test]
fn rollback_pristine_check_detects_ignored_hook_output() {
    assert!(commands::status_including_ignored().contains(&"--ignored".to_owned()));
    assert!(commands::is_pristine_including_ignored(b""));
    assert!(!commands::is_pristine_including_ignored(IGNORED_STATUS));
    // The ordinary status hides ignored files, which is why rollback uses the second probe.
    assert!(!commands::status().contains(&"--ignored".to_owned()));
}

/// REQ: F-067, PORT:ilium-git/tests/real_repo.rs::removal_clears_a_custom_file_in_registered_worktree_metadata
#[test]
fn removal_clears_a_custom_file_in_registered_worktree_metadata() {
    // Removal is one `worktree remove` call and Git deletes its own metadata directory, so the
    // plugin must issue the exact command and see the path vanish from the next listing. The
    // filesystem half needs a real repository (G10).
    let listed = listed();
    let arguments = commands::remove_worktree(&listed, "/repo/main-linked", "/repo/main", false)
        .expect("removal");
    assert_eq!(arguments[..2], ["worktree", "remove"]);
    let after = worktrees(
        b"worktree /repo/main\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0",
    )
    .expect("listing after removal");
    assert!(!after
        .iter()
        .any(|worktree| worktree.path == "/repo/main-linked"));
}

/// REQ: F-067, PORT:ilium-git/tests/real_repo.rs::rejects_invalid_branch_without_creating_worktree
#[test]
fn rejects_invalid_branch_without_creating_worktree() {
    for branch in ["", "-bad", "--force"] {
        assert!(commands::create_worktree("/repo/outside", branch, "main").is_err());
        assert!(commands::check_branch_name(branch).is_err());
    }
    assert!(commands::create_worktree("relative/path", "agent/x", "main").is_err());
    assert!(commands::create_worktree("/repo/outside", "agent/x", "").is_err());
    assert!(commands::create_worktree("/repo/outside", "agent/x", "-main").is_err());
    // `bad..name` is refused by `git check-ref-format --branch`, which the plugin runs first.
    assert_eq!(
        commands::check_branch_name("bad..name").expect("validation command"),
        ["check-ref-format", "--branch", "bad..name"]
    );
    assert_eq!(
        commands::create_worktree("/repo/linked", "agent/x", "main").expect("valid"),
        [
            "worktree",
            "add",
            "-b",
            "agent/x",
            "--",
            "/repo/linked",
            "main"
        ]
    );
}

/// REQ: F-067, PORT:ilium-git/tests/real_repo.rs::resolves_selected_base_and_compare_deletes_only_the_expected_tip
#[test]
fn resolves_selected_base_and_compare_deletes_only_the_expected_tip() {
    assert_eq!(
        commands::resolve_commit("main").expect("resolve"),
        ["rev-parse", "--verify", "main^{commit}"]
    );
    assert!(commands::resolve_commit("-x").is_err());
    assert!(commands::delete_branch_if_tip("agent/rollback", "bbbbb").is_err());
    assert_eq!(
        commands::delete_branch_if_tip("agent/rollback", TIP).expect("compare-delete"),
        ["update-ref", "-d", "refs/heads/agent/rollback", TIP]
    );
    // A moved tip fails the removal gate and the branch stays.
    let moved = RemovalFacts {
        tip: "2".repeat(40),
        expected_tip: TIP.to_owned(),
        creation_commit: TIP.to_owned(),
        upstream_tip: None,
        contained_in_upstream: false,
        merged_into_base: true,
    };
    assert!(commands::verify_removal(&moved, false).is_err());
    let unmoved = RemovalFacts {
        tip: TIP.to_owned(),
        ..moved
    };
    assert!(commands::verify_removal(&unmoved, false).is_ok());
}
