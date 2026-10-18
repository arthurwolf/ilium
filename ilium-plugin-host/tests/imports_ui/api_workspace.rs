//! `ilium:host/workspace` (api-workspace.md).

use ilium_plugin_api::generated::host::workspace::{HostWorkspace, WorkspaceSpec};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &[
    "workspace.manage",
    "process.run:git",
    "fs.write:worktrees",
    "fs.read:project",
];

fn spec(path: &str, branch: &str) -> WorkspaceSpec {
    WorkspaceSpec {
        repository: FsPath {
            scope: "project".to_owned(),
            path: ".".to_owned(),
        },
        path: FsPath {
            scope: "worktrees".to_owned(),
            path: path.to_owned(),
        },
        branch: branch.to_owned(),
        base: None,
        owner: None,
    }
}

fn seeded() -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.set_git_repository("project", ".");
    rig
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_returns_an_id_and_lists_it() {
    let rig = seeded();
    let id = rig
        .host
        .workspace()
        .register(spec("w1", "feature/a"))
        .expect("registered");
    let listed = rig.host.workspace().list();
    assert!(listed.iter().any(|w| w.id == id && w.branch == "feature/a"));
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_empty_branch_invalid_argument() {
    let rig = seeded();
    assert_eq!(
        code_of(rig.host.workspace().register(spec("w1", ""))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_invalid_branch_name_invalid_argument() {
    let rig = seeded();
    assert_eq!(
        code_of(rig.host.workspace().register(spec("w1", "bad..name"))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_duplicate_path_conflict() {
    let rig = seeded();
    rig.host
        .workspace()
        .register(spec("w1", "a"))
        .expect("first");
    assert_eq!(
        code_of(rig.host.workspace().register(spec("w1", "b"))),
        ErrorCode::Conflict
    );
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_not_a_git_repository_not_found() {
    let rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.workspace().register(spec("w1", "a"))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_missing_owner_node_not_found() {
    let rig = seeded();
    let mut with_owner = spec("w1", "a");
    with_owner.owner = Some(MISSING_PANE);
    assert_eq!(
        code_of(rig.host.workspace().register(with_owner)),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/workspace.register
#[test]
fn wit_workspace_register_declined_grant_not_permitted() {
    let rig = declined_rig(&["workspace.manage"]);
    assert_eq!(
        code_of(rig.host.workspace().register(spec("w1", "a"))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/workspace.release
#[test]
fn wit_workspace_release_removes_the_workspace() {
    let rig = seeded();
    let id = rig
        .host
        .workspace()
        .register(spec("w1", "a"))
        .expect("registered");
    rig.host.workspace().release(id).expect("released");
    assert!(rig.host.workspace().list().is_empty());
}

/// REQ: WIT:ilium:host/workspace.release
#[test]
fn wit_workspace_release_twice_not_found() {
    let rig = seeded();
    let id = rig
        .host
        .workspace()
        .register(spec("w1", "a"))
        .expect("registered");
    rig.host.workspace().release(id).expect("first");
    assert_eq!(
        code_of(rig.host.workspace().release(id)),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/workspace.release
#[test]
fn wit_workspace_release_in_use_conflict() {
    let rig = seeded();
    let id = rig
        .host
        .workspace()
        .register(spec("w1", "a"))
        .expect("registered");
    rig.kernel.set_workspace_in_use(id, true);
    assert_eq!(
        code_of(rig.host.workspace().release(id)),
        ErrorCode::Conflict
    );
}

/// REQ: WIT:ilium:host/workspace.list
#[test]
fn wit_workspace_list_is_empty_before_any_registration() {
    let rig = seeded();
    assert!(rig.host.workspace().list().is_empty());
}

/// REQ: WIT:ilium:host/workspace.list
#[test]
fn wit_workspace_list_reports_in_use_flag() {
    let rig = seeded();
    let id = rig
        .host
        .workspace()
        .register(spec("w1", "a"))
        .expect("registered");
    rig.kernel.set_workspace_in_use(id, true);
    assert!(rig
        .host
        .workspace()
        .list()
        .iter()
        .any(|w| w.id == id && w.in_use));
}

/// REQ: WIT:ilium:host/workspace.list
#[test]
fn wit_workspace_list_declined_grant_is_empty() {
    let rig = declined_rig(&["workspace.manage"]);
    assert!(rig.host.workspace().list().is_empty());
}
