use ilium_git::{
    branch_facts, branch_tip, create_worktree, default_base_ref, delete_branch_if_merged,
    delete_branch_if_tip, discover, head_paths, head_probe, is_worktree_pristine_including_ignored,
    list_branches, list_worktrees, remove_worktree, resolve_commit, status, GitError,
};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .expect("git is a test dependency");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository() -> TempDir {
    let temporary = TempDir::new().expect("temp repo");
    git(temporary.path(), &["init", "-q", "-b", "main"]);
    git(temporary.path(), &["config", "user.name", "Ilium Test"]);
    git(
        temporary.path(),
        &["config", "user.email", "ilium@example.invalid"],
    );
    git(
        temporary.path(),
        &["commit", "-q", "--allow-empty", "-m", "initial"],
    );
    temporary
}

#[tokio::test]
async fn discovers_same_common_directory_from_nested_and_linked_worktrees() {
    let temporary = repository();
    let main = temporary.path();
    let nested = main.join("src");
    std::fs::create_dir(&nested).expect("nested dir");
    let linked = main.parent().expect("parent").join(format!(
        "{}-linked",
        main.file_name().expect("name").to_string_lossy()
    ));
    create_worktree(main, &linked, "agent/test", "main")
        .await
        .expect("create linked worktree");

    let root = discover(main).await.expect("discover root");
    let nested_facts = discover(&nested).await.expect("discover nested");
    let linked_facts = discover(&linked).await.expect("discover linked");
    assert_eq!(root.common_dir, nested_facts.common_dir);
    assert_eq!(root.common_dir, linked_facts.common_dir);
    assert_eq!(nested_facts.project_subpath, Path::new("src"));
    assert_eq!(linked_facts.worktree_root, linked);
    assert_eq!(linked_facts.project_subpath, Path::new(""));
    assert!(!root.is_bare);

    let worktrees = list_worktrees(main).await.expect("list worktrees");
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[1].branch.as_deref(), Some("agent/test"));
    let branches = list_branches(main).await.expect("list branches");
    assert!(branches.contains(&"agent/test".to_owned()));
    assert_eq!(default_base_ref(main).await.expect("default base"), "main");
    assert_eq!(
        head_probe(&linked).await.expect("head").branch.as_deref(),
        Some("agent/test")
    );
    assert_eq!(
        branch_facts(&linked)
            .await
            .expect("facts")
            .last_commit_subject,
        "initial"
    );
    let metadata = head_paths(&linked).await.expect("worktree metadata");
    assert!(metadata.head.is_file());
    assert!(metadata.head_log.is_file());
    assert!(metadata.head.starts_with(&root.common_dir));

    remove_worktree(main, &linked, false)
        .await
        .expect("remove linked worktree");
    delete_branch_if_merged(main, "agent/test")
        .await
        .expect("delete merged branch");
}

#[tokio::test]
async fn preserves_dirty_linked_worktree_and_rejects_main_checkout_removal() {
    let temporary = repository();
    let main = temporary.path();
    let linked = main.parent().expect("parent").join(format!(
        "{}-dirty",
        main.file_name().expect("name").to_string_lossy()
    ));
    create_worktree(main, &linked, "agent/dirty", "main")
        .await
        .expect("create linked worktree");
    std::fs::write(linked.join("new file"), "untracked").expect("write untracked file");
    let state = status(&linked).await.expect("status");
    assert_eq!(state.branch.as_deref(), Some("agent/dirty"));
    assert_eq!(state.untracked, 1);
    assert!(!state.is_clean());
    assert!(matches!(
        remove_worktree(main, main, false).await,
        Err(GitError::InvalidInput(_))
    ));
    assert!(remove_worktree(main, &linked, false).await.is_err());
    assert!(linked.join("new file").exists());
    remove_worktree(main, &linked, true)
        .await
        .expect("explicit force removal");
}

#[tokio::test]
async fn rollback_pristine_check_detects_ignored_hook_output() {
    let temporary = repository();
    let main = temporary.path();
    std::fs::write(main.join(".gitignore"), "secret\n").expect("ignore rule");
    git(main, &["add", ".gitignore"]);
    git(main, &["commit", "-q", "-m", "ignore secret"]);
    let linked = main.parent().expect("parent").join(format!(
        "{}-ignored",
        main.file_name().expect("name").to_string_lossy()
    ));
    create_worktree(main, &linked, "agent/ignored", "main")
        .await
        .expect("create linked worktree");
    assert!(is_worktree_pristine_including_ignored(&linked)
        .await
        .expect("clean checkout"));
    std::fs::write(linked.join("secret"), "hook-owned data").expect("ignored data");
    assert!(status(&linked).await.expect("ordinary status").is_clean());
    assert!(!is_worktree_pristine_including_ignored(&linked)
        .await
        .expect("ignored data detected"));
    // Git's non-force removal itself deletes ignored content. The server must
    // enforce the stronger pristine gate before it ever reaches this call.
    remove_worktree(main, &linked, false)
        .await
        .expect("Git silently removes ignored fixture data");
    assert!(!linked.join("secret").exists());
}

#[tokio::test]
async fn removal_clears_a_custom_file_in_registered_worktree_metadata() {
    let temporary = repository();
    let main = temporary.path();
    let linked = main.parent().expect("parent").join(format!(
        "{}-metadata",
        main.file_name().expect("name").to_string_lossy()
    ));
    create_worktree(main, &linked, "agent/metadata", "main")
        .await
        .expect("create linked worktree");
    let head = head_paths(&linked).await.expect("worktree metadata").head;
    let metadata = head.parent().expect("HEAD parent").to_path_buf();
    let marker = metadata.join("ilium-workspace-owner.json");
    std::fs::write(&marker, b"owned metadata").expect("write custom metadata");

    remove_worktree(main, &linked, false)
        .await
        .expect("remove linked worktree with custom metadata");
    assert!(!linked.exists());
    assert!(!metadata.exists());
    assert!(!list_worktrees(main)
        .await
        .expect("list after removal")
        .iter()
        .any(|worktree| worktree.path == linked));
}

#[tokio::test]
async fn rejects_invalid_branch_without_creating_worktree() {
    let temporary = repository();
    let target = temporary.path().join("outside");
    assert!(
        create_worktree(temporary.path(), &target, "bad..name", "main")
            .await
            .is_err()
    );
    assert!(!target.exists());
}

#[tokio::test]
async fn resolves_selected_base_and_compare_deletes_only_the_expected_tip() {
    let temporary = repository();
    let main = temporary.path();
    let initial_commit = resolve_commit(main, "main").await.expect("main commit");
    git(main, &["branch", "agent/rollback", "main"]);
    assert_eq!(
        branch_tip(main, "agent/rollback")
            .await
            .expect("branch tip"),
        Some(initial_commit.clone())
    );

    let wrong_tip = "b".repeat(initial_commit.len());
    assert!(delete_branch_if_tip(main, "agent/rollback", &wrong_tip)
        .await
        .is_err());
    assert_eq!(
        branch_tip(main, "agent/rollback")
            .await
            .expect("branch preserved"),
        Some(initial_commit.clone())
    );

    git(main, &["commit", "-q", "--allow-empty", "-m", "later"]);
    assert_ne!(resolve_commit(main, "main").await.unwrap(), initial_commit);
    assert_eq!(
        resolve_commit(main, "agent/rollback").await.unwrap(),
        initial_commit
    );
    delete_branch_if_tip(main, "agent/rollback", &initial_commit)
        .await
        .expect("compare-delete owned branch");
    assert_eq!(branch_tip(main, "agent/rollback").await.unwrap(), None);
}
