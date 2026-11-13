//! Shared helpers of the parity lane: a booted `ServerKernel` world (gap constructors, see
//! API_GAPS.md), intent builders and the run-in-both-modes comparison.

use std::fmt::Debug;
use std::path::{Path, PathBuf};

use ilium_plugin_api::generated::types::{
    CreateFolderSpec, CreateGroupSpec, CreatePaneSpec, CreateProjectSpec, CreateSplitSpec,
    Delivery, ErrorCode, FsPath, Intent, MoveNodeSpec, NodeClass, NodeId, Outcome, Placement,
    RenameNodeSpec, SendInputSpec, SplitDirection, TitleSource,
};
use ilium_plugin_api::kernel::{KernelFacade, Origin, TreeNode};
use ilium_plugin_host::lifecycle::StartMode;
use ilium_server::plugin_host::ServerKernel;

/// The two builds every baseline scenario must agree on.
pub const MODES: [StartMode; 2] = [StartMode::Normal, StartMode::NoPlugins];

/// An empty, booted kernel in the given start mode with an empty config file.
pub fn world(mode: StartMode) -> ServerKernel {
    ServerKernel::for_parity_world(mode, "")
}

/// An empty, booted kernel whose `config.toml` holds `config_toml`.
pub fn world_with_config(mode: StartMode, config_toml: &str) -> ServerKernel {
    ServerKernel::for_parity_world(mode, config_toml)
}

/// Runs a scenario with plugins and kernel only; both results must be equal (COR-001).
pub fn in_both_modes<T: PartialEq + Debug>(scenario: impl Fn(&ServerKernel) -> T) -> T {
    let with_plugins = scenario(&world(StartMode::Normal));
    let kernel_only = scenario(&world(StartMode::NoPlugins));
    assert_eq!(
        with_plugins, kernel_only,
        "plugin build differs from the kernel-only baseline"
    );
    with_plugins
}

pub fn apply(kernel: &ServerKernel, intent: Intent) -> Outcome {
    kernel.apply_intent(&Origin::User, intent)
}

/// The node id of an `ok-node` outcome; panics with the outcome otherwise.
pub fn created(outcome: Outcome) -> NodeId {
    match outcome {
        Outcome::OkNode(id) => id,
        other => panic!("expected ok-node, got {other:?}"),
    }
}

pub fn is_refused(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Refused(_))
}

pub fn refusal_code(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Refused(error) => Some(error.code),
        _ => None,
    }
}

pub fn project(kernel: &ServerKernel, title: &str) -> NodeId {
    created(apply(
        kernel,
        Intent::CreateProject(CreateProjectSpec {
            title: title.to_owned(),
            root: FsPath {
                scope: "project".to_owned(),
                path: ".".to_owned(),
            },
        }),
    ))
}

pub fn group(kernel: &ServerKernel, parent: Option<NodeId>, title: &str) -> NodeId {
    created(apply(
        kernel,
        Intent::CreateGroup(CreateGroupSpec {
            parent,
            title: title.to_owned(),
            placement: Placement::InsideLast,
            children: Vec::new(),
        }),
    ))
}

pub fn folder(kernel: &ServerKernel, parent: Option<NodeId>, title: &str) -> NodeId {
    created(apply(
        kernel,
        Intent::CreateFolder(CreateFolderSpec {
            parent,
            title: title.to_owned(),
            path: None,
        }),
    ))
}

/// A terminal pane running `command` under `parent`.
pub fn terminal(
    kernel: &ServerKernel,
    parent: Option<NodeId>,
    title: &str,
    command: &[&str],
) -> NodeId {
    created(apply(
        kernel,
        Intent::CreatePane(CreatePaneSpec {
            parent,
            title: title.to_owned(),
            content_kind: "ilium.terminal".to_owned(),
            cwd: None,
            command: Some(command.iter().map(|part| (*part).to_owned()).collect()),
            initial_input: None,
        }),
    ))
}

pub fn split(kernel: &ServerKernel, panes: &[NodeId], direction: SplitDirection) -> Outcome {
    apply(
        kernel,
        Intent::CreateSplit(CreateSplitSpec {
            panes: panes.to_vec(),
            direction,
        }),
    )
}

pub fn rename(kernel: &ServerKernel, node: NodeId, title: &str) -> Outcome {
    apply(
        kernel,
        Intent::RenameNode(RenameNodeSpec {
            node,
            title: title.to_owned(),
            source: TitleSource::Plugin,
            expected_revision: None,
        }),
    )
}

pub fn move_node(
    kernel: &ServerKernel,
    node: NodeId,
    target: NodeId,
    placement: Placement,
) -> Outcome {
    apply(
        kernel,
        Intent::MoveNode(MoveNodeSpec {
            node,
            target,
            placement,
        }),
    )
}

pub fn close(kernel: &ServerKernel, node: NodeId) -> Outcome {
    apply(kernel, Intent::CloseNode(node))
}

/// Typed input, as the interface sends it.
pub fn type_text(kernel: &ServerKernel, pane: NodeId, text: &str) -> Outcome {
    apply(
        kernel,
        Intent::SendInput(SendInputSpec {
            pane,
            text: text.to_owned(),
            delivery: Delivery::Typed,
            prompt_epoch: None,
            stages: Vec::new(),
            raw: None,
            mouse: None,
            if_current: None,
            lock_user_input: false,
            journal: false,
        }),
    )
}

/// Titles of every node in pre-order.
pub fn titles(kernel: &ServerKernel) -> Vec<String> {
    kernel
        .tree()
        .nodes()
        .into_iter()
        .map(|node| node.title)
        .collect()
}

pub fn node(kernel: &ServerKernel, id: NodeId) -> TreeNode {
    kernel
        .tree()
        .node(id)
        .unwrap_or_else(|| panic!("node {id:?} is not in the tree"))
}

pub fn class_of(kernel: &ServerKernel, id: NodeId) -> NodeClass {
    node(kernel, id).class
}

/// The workspace root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The text of one plugin-manual page.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The text of one user-documentation page (`src/docs`).
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// One row of the feature table in reference-feature-traceability.md, cells trimmed.
pub fn feature_row(id: &str) -> Vec<String> {
    let page = manual_page("reference-feature-traceability");
    let prefix = format!("| {id} |");
    let line = page
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no row {id} in reference-feature-traceability.md"));
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// Runs a scenario on the plugin build only (for behaviour a core plugin completes, such as the
/// route the fleet plugin registers on the kernel's HTTP listener).
pub fn on_plugin_build<T>(scenario: impl Fn(&ServerKernel) -> T) -> T {
    scenario(&world(StartMode::Normal))
}
