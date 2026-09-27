use std::path::{Path, PathBuf};

use ilium_core::{PaneContentKind, PaneWorkspace, Tree};
use ilium_ipc::ServerEvent;

#[test]
fn tree_snapshot_wire_preserves_launch_directory_and_workspace_creation_facts() {
    let mut tree = Tree::new();
    let project = tree.add_project(PathBuf::from("/tmp/repo")).unwrap();
    let group = tree.add_group(project, "agents").unwrap();
    let pane = tree
        .add_pane(group, "codex", PaneContentKind::Terminal)
        .unwrap();
    let cwd = PathBuf::from("/tmp/repo.worktrees/agent-fix/src");
    let workspace = PaneWorkspace {
        workspace_id: Some("test-workspace".to_owned()),
        repo_common_dir: PathBuf::from("/tmp/repo/.git"),
        worktree_root: PathBuf::from("/tmp/repo.worktrees/agent-fix"),
        branch: "agent/fix".to_owned(),
        base_ref: "main".to_owned(),
        base_commit: "a".repeat(40),
        created_by_ilium: true,
        created_at_unix: 1_790_380_800,
    };
    tree.set_pane_launch_cwd(pane, cwd.clone()).unwrap();
    tree.set_pane_workspace(pane, Some(workspace.clone()))
        .unwrap();

    let encoded = bincode::serialize(&ServerEvent::TreeSnapshot(tree)).unwrap();
    let decoded: ServerEvent = bincode::deserialize(&encoded).unwrap();
    let ServerEvent::TreeSnapshot(decoded_tree) = decoded else {
        panic!("wrong event variant");
    };
    assert_eq!(decoded_tree.pane_cwd(pane), Some(cwd.as_path()));
    assert_eq!(decoded_tree.pane_workspace(pane), Some(&workspace));
    assert_ne!(decoded_tree.pane_cwd(pane), Some(Path::new("/tmp/repo")));
}
