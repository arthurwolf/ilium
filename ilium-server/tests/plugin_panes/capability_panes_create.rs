//! create-pane errors and defaults (capability-panes-create.md): PAN-030, PAN-031.

use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::{CreatePaneSpec, ErrorCode, Intent, NodeId, Outcome};

use crate::common::*;

const CREATE: &[&str] = &["panes.create", "fs.scope.project"];

/// `scope/path` names a scope and a relative path, the form the create-pane cwd takes.
fn fs_path(text: &str) -> FsPath {
    let (scope, path) = text.split_once('/').expect("cwd names a scope and a path");
    FsPath { scope: scope.to_owned(), path: path.to_owned() }
}

fn create(cwd: Option<&str>, command: Option<Vec<&str>>) -> Intent {
    Intent::CreatePane(CreatePaneSpec {
        parent: Some(NodeId(1)),
        title: "new".into(),
        content_kind: "ilium.terminal".into(),
        cwd: cwd.map(fs_path),
        command: command.map(|parts| parts.into_iter().map(str::to_owned).collect()),
        initial_input: None,
    })
}

/// REQ: PAN-030
#[test]
fn pan_030_missing_cwd_folder_refused_not_found() {
    let kernel = world(CREATE);
    assert_eq!(
        refusal(&submit(&kernel, create(Some("project/missing"), None))),
        Some(ErrorCode::NotFound)
    );
}

/// REQ: PAN-030
#[test]
fn pan_030_cwd_scope_not_granted_refused_not_permitted() {
    let kernel = world(&["panes.create"]);
    assert_eq!(
        refusal(&submit(&kernel, create(Some("project/src"), None))),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: PAN-030
#[test]
fn pan_030_existing_cwd_accepted() {
    let kernel = world(CREATE);
    assert!(matches!(
        submit(&kernel, create(Some("project/src"), None)),
        Outcome::OkNode(_)
    ));
}

/// REQ: PAN-030
#[test]
fn pan_030_unstartable_program_completes_failed_io_after_the_pane_exists() {
    let kernel = world(CREATE);
    let outcome = submit(&kernel, create(None, Some(vec!["/nonexistent/nope"])));
    assert_eq!(failure(&outcome), Some(ErrorCode::Io), "{outcome:?}");
}

/// REQ: PAN-031
#[test]
fn pan_031_no_command_runs_the_login_shell_from_shell() {
    let kernel = world(CREATE);
    kernel.set_environment("SHELL", Some("/bin/zsh"));
    submit(&kernel, create(None, None));
    assert_eq!(kernel.last_spawned_program().as_deref(), Some("/bin/zsh"));
}

/// REQ: PAN-031
#[test]
fn pan_031_shell_unset_runs_bin_sh() {
    let kernel = world(CREATE);
    kernel.set_environment("SHELL", None);
    submit(&kernel, create(None, None));
    assert_eq!(kernel.last_spawned_program().as_deref(), Some("/bin/sh"));
}
