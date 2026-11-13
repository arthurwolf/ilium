//! Pure-rule tests for the worktrees plugin modules (`location`, `git::commands`). They carry no
//! FakeHost: the rules take plain strings and return plain data, so they run without a host.

use ilium_plugin_core_worktrees::git::commands;
use ilium_plugin_core_worktrees::location::{
    default_worktree_path, expand_worktree_path_template, is_within, native_separators,
    slugify_branch, split_parent, unique_branch, validate_branch_name, validate_new_path,
    BranchNameError,
};

const DEFAULT_TEMPLATE: &str = "{repo_parent}/{repo_name}.worktrees/{branch_slug}";

#[test]
fn slugs_are_lowercase_dashed_and_bounded() {
    assert_eq!(slugify_branch("  Fix Login / OAuth!  "), "fix-login-oauth");
    assert_eq!(slugify_branch("---"), "task");
    assert_eq!(slugify_branch(""), "task");
    assert!(slugify_branch(&"abc ".repeat(40)).len() <= 48);
    assert!(!slugify_branch(&"abc ".repeat(40)).ends_with('-'));
}

#[test]
fn branch_names_follow_the_ref_rules() {
    assert_eq!(validate_branch_name(""), Err(BranchNameError::Empty));
    assert_eq!(
        validate_branch_name("a b"),
        Err(BranchNameError::ForbiddenCharacter)
    );
    assert_eq!(
        validate_branch_name("a..b"),
        Err(BranchNameError::InvalidSequence)
    );
    assert_eq!(
        validate_branch_name("a//b"),
        Err(BranchNameError::InvalidSequence)
    );
    assert_eq!(
        validate_branch_name("a@{u}"),
        Err(BranchNameError::InvalidSequence)
    );
    assert_eq!(
        validate_branch_name("-a"),
        Err(BranchNameError::InvalidComponent)
    );
    assert_eq!(
        validate_branch_name("a/"),
        Err(BranchNameError::InvalidComponent)
    );
    assert_eq!(
        validate_branch_name("a/.b"),
        Err(BranchNameError::InvalidComponent)
    );
    assert_eq!(
        validate_branch_name("a.lock"),
        Err(BranchNameError::InvalidComponent)
    );
    assert_eq!(validate_branch_name("agent/task-2"), Ok(()));
}

#[test]
fn template_placeholders_expand_once_and_inserted_literals_stay_literal() {
    assert_eq!(
        expand_worktree_path_template(DEFAULT_TEMPLATE, "/p/acme", "/p", "acme", "task"),
        "/p/acme.worktrees/task"
    );
    // A repository literally named `{branch_slug}-repo` must not be expanded a second time.
    assert_eq!(
        expand_worktree_path_template(
            DEFAULT_TEMPLATE,
            "/p/{branch_slug}-repo",
            "/p",
            "{branch_slug}-repo",
            "agent-task-2"
        ),
        "/p/{branch_slug}-repo.worktrees/agent-task-2"
    );
    assert_eq!(
        expand_worktree_path_template("{project}/x/{unknown}", "/p/acme", "/p", "acme", "s"),
        "/p/acme/x/{unknown}"
    );
}

#[test]
fn template_slashes_become_the_platform_separator_without_touching_other_text() {
    assert_eq!(native_separators("a/b/c", '/'), "a/b/c");
    assert_eq!(
        native_separators("C:\\repos/app/x", '\\'),
        "C:\\repos\\app\\x"
    );
}

#[test]
fn parents_and_names_split_on_the_last_component() {
    assert_eq!(split_parent("/p/acme"), ("/p", "acme"));
    assert_eq!(split_parent("/p/acme/"), ("/p", "acme"));
    assert_eq!(split_parent("/acme"), ("/", "acme"));
    assert_eq!(split_parent("acme"), ("", "acme"));
}

#[test]
fn default_path_lives_next_to_the_main_checkout() {
    assert_eq!(
        default_worktree_path(DEFAULT_TEMPLATE, "/p/acme", "agent/task-2", '/'),
        "/p/acme.worktrees/agent-task-2"
    );
    // A relative template falls back to the project's own `.ilium/worktrees`.
    assert_eq!(
        default_worktree_path("wt/{branch_slug}", "/p/acme", "agent/x", '/'),
        "/p/acme/.ilium/worktrees/agent-x"
    );
}

#[test]
fn branch_collisions_get_the_first_free_numeric_suffix() {
    let taken = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(unique_branch("agent/task", &taken(&["main"])), "agent/task");
    assert_eq!(
        unique_branch("agent/task", &taken(&["agent/task", "agent/task-2"])),
        "agent/task-3"
    );
}

#[test]
fn path_overlap_and_occupied_existing_are_blocked() {
    let existing = ["/p/acme", "/p/acme.worktrees/one"];
    assert!(validate_new_path("/p/acme.worktrees/two", "/p/acme/.git", &existing).is_ok());
    assert!(validate_new_path("relative", "/p/acme/.git", &existing).is_err());
    assert!(validate_new_path("/p/acme.worktrees/../x", "/p/acme/.git", &existing).is_err());
    assert!(validate_new_path("/p/acme/.git/wt", "/p/acme/.git", &existing).is_err());
    assert!(validate_new_path("/p/acme/inside", "/p/acme/.git", &existing).is_err());
    assert!(validate_new_path("/p", "/p/acme/.git", &existing).is_err());
    assert!(validate_new_path("/p/acme.worktrees/one", "/p/acme/.git", &existing).is_err());
    assert!(is_within("/a/b", "/a"));
    assert!(!is_within("/ab", "/a"));
}

#[test]
fn object_ids_and_removal_refs_are_strict() {
    assert!(commands::is_full_object_id(&"a".repeat(40)));
    assert!(commands::is_full_object_id(&"F".repeat(64)));
    assert!(!commands::is_full_object_id(&"a".repeat(39)));
    assert!(!commands::is_full_object_id(&"g".repeat(40)));
    assert!(commands::is_shared_removal_ref("refs/heads/main"));
    assert!(commands::is_shared_removal_ref("refs/remotes/origin/main"));
    assert!(!commands::is_shared_removal_ref("HEAD"));
    assert!(!commands::is_shared_removal_ref("refs/heads/a^b"));
    assert!(!commands::is_shared_removal_ref("refs/heads/a@{u}"));
    assert!(!commands::is_shared_removal_ref("refs/tags/v1"));
}

#[test]
fn direct_reference_output_must_be_exact() {
    let id = "1".repeat(40);
    let reference = "refs/heads/main";
    let good = format!("{reference}\0{id}\0\0refs/remotes/origin/main\n");
    assert_eq!(
        commands::parse_direct_reference(reference, &good).expect("parse"),
        (id.clone(), Some("refs/remotes/origin/main".to_owned()))
    );
    let no_upstream = format!("{reference}\0{id}\0\0\n");
    assert_eq!(
        commands::parse_direct_reference(reference, &no_upstream)
            .expect("parse")
            .1,
        None
    );
    let symbolic = format!("{reference}\0{id}\0refs/heads/other\0\n");
    assert!(commands::parse_direct_reference(reference, &symbolic).is_err());
    assert!(commands::parse_direct_reference(reference, "").is_err());
    assert!(commands::parse_direct_reference("refs/heads/x", &good).is_err());
}

#[test]
fn removal_gate_requires_publication_or_an_untouched_branch() {
    let id = "1".repeat(40);
    let base = commands::RemovalFacts {
        tip: id.clone(),
        expected_tip: id.clone(),
        creation_commit: id.clone(),
        upstream_tip: None,
        contained_in_upstream: false,
        merged_into_base: true,
    };
    assert!(commands::verify_removal(&base, false).is_ok());
    let committed = commands::RemovalFacts {
        tip: "2".repeat(40),
        expected_tip: "2".repeat(40),
        ..base.clone()
    };
    assert!(commands::verify_removal(&committed, false).is_err());
    let published = commands::RemovalFacts {
        contained_in_upstream: true,
        upstream_tip: Some("2".repeat(40)),
        ..committed.clone()
    };
    assert!(commands::verify_removal(&published, true).is_ok());
    let unpublished = commands::RemovalFacts {
        contained_in_upstream: false,
        ..published.clone()
    };
    assert!(commands::verify_removal(&unpublished, true).is_err());
    let unmerged = commands::RemovalFacts {
        merged_into_base: false,
        ..published
    };
    assert!(commands::verify_removal(&unmerged, true).is_err());
}

#[test]
fn index_checks_refuse_hidden_edits_and_submodules() {
    assert!(commands::verify_index_flags(b"H a.rs\0H b.rs\0").is_ok());
    assert!(commands::verify_index_flags(b"H a.rs\0S b.rs\0").is_err());
    assert!(commands::verify_index_flags(b"h a.rs\0").is_err());
    assert!(commands::verify_index_stages(b"100644 abc 0\tfile\0").is_ok());
    assert!(commands::verify_index_stages(b"160000 abc 0\tsub\0").is_err());
}

#[test]
fn deleting_branches_validates_the_name_first() {
    assert!(commands::delete_branch_if_merged("-x").is_err());
    assert!(commands::delete_branch_force("").is_err());
    assert_eq!(
        commands::delete_branch_if_merged("agent/x").expect("safe"),
        ["branch", "-d", "--", "agent/x"]
    );
    assert_eq!(
        commands::delete_branch_force("agent/x").expect("forced"),
        ["branch", "-D", "--", "agent/x"]
    );
    assert!(commands::commit_is_ancestor("abc", &"1".repeat(40)).is_err());
}

mod include_and_wrap_up {
    use ilium_plugin_core_worktrees::include::{
        is_safe_relative_path, parent_directories, plan_selection, roots_overlap,
        validate_include_file, SourceEntry, MAX_COPIED_BYTES, MAX_COPIED_FILES,
        MAX_INCLUDE_FILE_BYTES,
    };
    use ilium_plugin_core_worktrees::wrap_up;

    fn file(path: &str, bytes: u64) -> SourceEntry {
        SourceEntry {
            relative_path: path.to_owned(),
            is_dir: false,
            is_symlink: false,
            bytes,
        }
    }

    fn select_env(path: &str, _is_dir: bool) -> bool {
        path.starts_with(".env")
    }

    #[test]
    fn selection_is_sorted_and_skips_git_directories_and_unmatched_files() {
        let entries = [
            file("src/main.rs", 10),
            file(".env.local", 5),
            file(".env", 3),
            file(".git/.env", 1),
        ];
        let selected = plan_selection(&entries, select_env, || false).expect("selection");
        let names: Vec<_> = selected
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect();
        assert_eq!(names, [".env", ".env.local"]);
    }

    #[test]
    fn byte_limit_rejects_selection_before_writing() {
        let entries = [file(".env", MAX_COPIED_BYTES), file(".env2", 1)];
        assert!(plan_selection(&entries, select_env, || false).is_err());
        let overflow = [file(".env", u64::MAX), file(".env2", 1)];
        assert!(plan_selection(&overflow, select_env, || false).is_err());
    }

    #[test]
    fn count_limit_and_entry_limit_are_enforced() {
        let many: Vec<_> = (0..=MAX_COPIED_FILES)
            .map(|index| file(&format!(".env{index}"), 1))
            .collect();
        assert!(plan_selection(&many, select_env, || false).is_err());
        let wide: Vec<_> = (0..10_001)
            .map(|index| file(&format!("f{index}"), 1))
            .collect();
        assert!(plan_selection(&wide, select_env, || false).is_err());
    }

    #[test]
    fn cancellation_before_discovery_writes_nothing() {
        let entries = [file(".env", 1)];
        assert!(plan_selection(&entries, select_env, || true).is_err());
    }

    #[test]
    fn selected_symlinks_and_escaping_paths_are_rejected() {
        let link = SourceEntry {
            is_symlink: true,
            ..file(".env", 1)
        };
        assert!(plan_selection(&[link], select_env, || false).is_err());
        assert!(plan_selection(&[file(".env/../x", 1)], select_env, || false).is_err());
    }

    #[test]
    fn include_file_is_bounded_utf8_without_a_byte_order_mark() {
        assert_eq!(
            validate_include_file("\u{feff}.env\n".as_bytes()).expect("text"),
            ".env\n"
        );
        assert!(validate_include_file(b"\xff").is_err());
        assert!(validate_include_file(&vec![b'a'; MAX_INCLUDE_FILE_BYTES as usize + 1]).is_err());
    }

    #[test]
    fn relative_paths_and_overlapping_roots_are_classified() {
        assert!(is_safe_relative_path("a/b.txt"));
        assert!(!is_safe_relative_path("/a"));
        assert!(!is_safe_relative_path("a/../b"));
        assert!(!is_safe_relative_path("a//b"));
        assert!(!is_safe_relative_path(""));
        assert!(roots_overlap("/p/a", "/p/a"));
        assert!(roots_overlap("/p/a", "/p/a/b"));
        assert!(roots_overlap("/p/a/b", "/p/a"));
        assert!(!roots_overlap("/p/a", "/p/ab"));
        assert_eq!(parent_directories("a/b/c.txt"), ["a", "a/b"]);
        assert!(parent_directories("c.txt").is_empty());
    }

    #[test]
    fn wrap_up_text_names_every_close_pane_mode() {
        for mode in wrap_up::CLOSE_PANE_MODES {
            assert!(
                wrap_up::INSTRUCTIONS.contains(&format!("--worktree {mode}")),
                "missing {mode}"
            );
        }
        assert!(wrap_up::INSTRUCTIONS.starts_with("Wrap-up of this Git worktree agent"));
    }
}
