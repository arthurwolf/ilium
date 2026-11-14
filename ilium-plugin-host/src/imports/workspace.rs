//! Host implementation of `ilium:host/workspace`: the kernel custody records of Git worktrees.
//!
//! The kernel never runs Git. `register` only validates the spec against the kernel's view of the
//! world (repository folders, tree nodes, busy panes) and records custody; `release` removes the
//! record only (manual page `api-workspace`).
//!
//! The view of the world is a [`WorkspaceWorldSource`]. The runtime links [`KernelWorkspaceWorld`],
//! which reads it from the kernel on every call: the tree for owner nodes, the detected agents for
//! busy panes and the project folder for repositories. Tests and the harness use a fixed
//! [`WorkspaceWorld`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ilium_plugin_api::generated::host::workspace::{HostWorkspace, WorkspaceInfo, WorkspaceSpec};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::kernel::KernelFacade;

use super::env::grants_allow;

/// Scope that worktree folders must live in (`fs.write:worktrees`).
const WORKTREES_SCOPE: &str = "worktrees";
/// The scope of the project folder, the only repository scope the kernel resolves itself.
const PROJECT_SCOPE: &str = "project";
/// Maximum characters of an error message (the ABI caps it at 256).
const MESSAGE_LIMIT: usize = 256;

/// What the kernel knows about the outside world when it validates a worktree spec.
pub trait WorkspaceWorldSource: Send + Sync {
    /// Whether `folder` is a Git repository (holds a `.git` entry).
    fn is_repository(&self, folder: &FsPath) -> bool;
    /// Whether the tree node exists.
    fn node_exists(&self, node: u64) -> bool;
    /// Whether a process runs in the worktree: its owner pane runs one, or a pane that runs one
    /// works inside the worktree folder.
    fn in_use(&self, owner: Option<NodeId>, worktree: &FsPath) -> bool;
}

/// A fixed view of the world.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceWorld {
    /// Repository folders that hold a `.git` entry.
    pub repositories: BTreeSet<FsPathKey>,
    /// Tree nodes that exist.
    pub nodes: BTreeSet<u64>,
    /// Owner panes that currently run a process.
    pub busy_owners: BTreeSet<u64>,
}

/// Ordered (scope, path) key of a folder.
pub type FsPathKey = (String, String);

impl WorkspaceWorldSource for WorkspaceWorld {
    fn is_repository(&self, folder: &FsPath) -> bool {
        self.repositories
            .contains(&(folder.scope.clone(), folder.path.clone()))
    }

    fn node_exists(&self, node: u64) -> bool {
        self.nodes.contains(&node)
    }

    fn in_use(&self, owner: Option<NodeId>, _worktree: &FsPath) -> bool {
        owner.is_some_and(|owner| self.busy_owners.contains(&owner.0))
    }
}

/// The view of the world the kernel has right now.
///
/// - A repository is a folder of the `project` scope that holds `.git`; the project folder is the
///   one the kernel reports (`WorkspaceAccess::project_dir`). Other scopes are not resolved here.
/// - A pane runs a process while the kernel detects an agent in it. A shell at its prompt is no
///   agent, so an idle shell does not hold a worktree, as the manual asks.
/// - A worktree folder is under the default `worktrees` root, `<project name>.worktrees/` next
///   to the project folder; a pane whose working folder lies inside it holds it too.
pub struct KernelWorkspaceWorld {
    kernel: Arc<dyn KernelFacade>,
}

impl KernelWorkspaceWorld {
    /// The world of `kernel`.
    pub fn new(kernel: Arc<dyn KernelFacade>) -> Self {
        Self { kernel }
    }

    fn project_dir(&self) -> Option<PathBuf> {
        self.kernel
            .workspace()?
            .project_dir()
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    }

    /// The host folder of an `FsPath` in the `project` or `worktrees` scope.
    fn host_path(&self, path: &FsPath) -> Option<PathBuf> {
        let relative = relative_path(&path.path)?;
        let project = self.project_dir()?;
        match path.scope.as_str() {
            PROJECT_SCOPE => Some(project.join(relative)),
            WORKTREES_SCOPE => Some(default_worktree_root(&project)?.join(relative)),
            _ => None,
        }
    }
}

/// `<parent>/<project name>.worktrees`, the default of `workspace.worktree_root`.
fn default_worktree_root(project: &Path) -> Option<PathBuf> {
    let name = project.file_name()?.to_str()?;
    Some(project.parent()?.join(format!("{name}.worktrees")))
}

/// The path of a scope-relative `FsPath` part, or `None` when it climbs out of the scope (an
/// absolute path or a `..` component).
fn relative_path(path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        .then(|| candidate.to_path_buf())
}

impl WorkspaceWorldSource for KernelWorkspaceWorld {
    fn is_repository(&self, folder: &FsPath) -> bool {
        folder.scope == PROJECT_SCOPE
            && self
                .host_path(folder)
                .is_some_and(|host| host.join(".git").exists())
    }

    fn node_exists(&self, node: u64) -> bool {
        self.kernel.tree().node(NodeId(node)).is_some()
    }

    fn in_use(&self, owner: Option<NodeId>, worktree: &FsPath) -> bool {
        let worktree_folder = self.host_path(worktree);
        self.kernel.tree().nodes().iter().any(|node| {
            if node.agent.is_none() {
                return false;
            }
            let owns = owner == Some(node.id);
            let works_inside = match (&worktree_folder, &node.cwd) {
                (Some(folder), Some(cwd)) => Path::new(cwd).starts_with(folder),
                _ => false,
            };
            owns || works_inside
        })
    }
}

/// Host side of the `workspace` interface.
pub struct WorkspaceImport {
    world: Arc<dyn WorkspaceWorldSource>,
    /// The grants of the instance; `None` checks no scope grant (host-internal use and tests).
    granted: Option<Vec<String>>,
    records: BTreeMap<u64, WorkspaceInfo>,
    next_id: u64,
}

impl std::fmt::Debug for WorkspaceImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceImport")
            .field("records", &self.records)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl Default for WorkspaceImport {
    fn default() -> Self {
        Self::with_world(WorkspaceWorld::default())
    }
}

impl WorkspaceImport {
    /// Creates an import that validates against `world`.
    pub fn with_world(world: WorkspaceWorld) -> Self {
        Self::with_source(Arc::new(world), None)
    }

    /// The import of one plugin instance holding `granted`, validating against `world`.
    pub fn with_source(world: Arc<dyn WorkspaceWorldSource>, granted: Option<Vec<String>>) -> Self {
        Self {
            world,
            granted,
            records: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// The import the runtime links: the kernel's world, the instance's grants.
    pub fn over_kernel(kernel: Arc<dyn KernelFacade>, granted: &[String]) -> Self {
        Self::with_source(
            Arc::new(KernelWorkspaceWorld::new(kernel)),
            Some(granted.to_vec()),
        )
    }

    /// Replaces the kernel's view of the world (for example when panes open or close).
    pub fn set_world(&mut self, world: WorkspaceWorld) {
        self.world = Arc::new(world);
    }

    /// Clears the owner of every record owned by `node`, as the kernel does when the pane closes.
    pub fn owner_closed(&mut self, node: NodeId) {
        for record in self.records.values_mut() {
            if record.owner == Some(node) {
                record.owner = None;
            }
        }
    }

    /// Whether the instance holds `<family>:<scope>`; always true without a grant list.
    fn holds_scope(&self, family: &str, scope: &str) -> bool {
        self.granted
            .as_ref()
            .is_none_or(|granted| grants_allow(granted, &format!("{family}:{scope}")))
    }
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.chars().take(MESSAGE_LIMIT).collect(),
        retry_after_ms: None,
    }
}

/// Git ref-name rules (`git check-ref-format --branch`), without running Git.
fn is_valid_branch(branch: &str) -> bool {
    if branch.is_empty()
        || branch == "@"
        || branch.starts_with('-')
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.ends_with('.')
        || branch.contains("..")
        || branch.contains("//")
        || branch.contains("@{")
    {
        return false;
    }
    if branch
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
    {
        return false;
    }
    branch
        .split('/')
        .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

impl HostWorkspace for WorkspaceImport {
    fn register(&mut self, spec: WorkspaceSpec) -> Result<u64, PluginError> {
        if spec.path.scope != WORKTREES_SCOPE || !self.holds_scope("fs.write", WORKTREES_SCOPE) {
            return Err(error(
                ErrorCode::NotPermitted,
                "the worktree path must be in the fs.write:worktrees scope",
            ));
        }
        if !self.holds_scope("fs.read", &spec.repository.scope) {
            return Err(error(
                ErrorCode::NotPermitted,
                "the repository folder lies outside the read grants",
            ));
        }
        if !is_valid_branch(&spec.branch) || spec.path.path.is_empty() {
            return Err(error(
                ErrorCode::InvalidArgument,
                "the branch name or the worktree path is not valid",
            ));
        }
        if !self.world.is_repository(&spec.repository) {
            return Err(error(
                ErrorCode::NotFound,
                "the repository folder has no .git entry",
            ));
        }
        if let Some(owner) = spec.owner {
            if !self.world.node_exists(owner.0) {
                return Err(error(ErrorCode::NotFound, "the owner node does not exist"));
            }
        }
        if self.records.values().any(|record| record.path == spec.path) {
            return Err(error(
                ErrorCode::Conflict,
                "a worktree is already registered at this path",
            ));
        }
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        self.records.insert(
            id,
            WorkspaceInfo {
                id,
                path: spec.path,
                branch: spec.branch,
                owner: spec.owner,
                in_use: false,
            },
        );
        Ok(id)
    }

    fn release(&mut self, workspace: u64) -> Result<(), PluginError> {
        let Some(record) = self.records.get(&workspace) else {
            return Err(error(
                ErrorCode::NotFound,
                "no worktree record with this id",
            ));
        };
        if self.world.in_use(record.owner, &record.path) {
            return Err(error(
                ErrorCode::Conflict,
                "the worktree is in use; try again after the process ends",
            ));
        }
        self.records.remove(&workspace);
        Ok(())
    }

    fn list(&mut self) -> Vec<WorkspaceInfo> {
        self.records
            .values()
            .map(|record| {
                let mut info = record.clone();
                info.in_use = self.world.in_use(record.owner, &record.path);
                info
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceImport, WorkspaceWorld};
    use ilium_plugin_api::generated::host::workspace::{HostWorkspace, WorkspaceSpec};
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::paths::FsPath;

    /// Builds an import whose world knows the `project/repo` repository and owner panes 14 and 15.
    fn test_import() -> WorkspaceImport {
        let mut world = WorkspaceWorld::default();
        world
            .repositories
            .insert(("project".to_string(), "repo".to_string()));
        world.nodes.insert(OWNER_PANE);
        world.nodes.insert(OWNER_PANE + 1);
        WorkspaceImport::with_world(world)
    }

    /// Node id used as an owner pane in these tests.
    const OWNER_PANE: u64 = 14;
    /// Id that no record can have; used for unknown-id checks.
    const UNKNOWN_WORKSPACE_ID: u64 = u64::MAX;

    /// Builds a spec with the repository in the `project` scope and the worktree in the `worktrees` scope.
    fn spec_for_branch(branch: &str, owner: Option<NodeId>) -> WorkspaceSpec {
        WorkspaceSpec {
            repository: FsPath {
                scope: "project".to_string(),
                path: "repo".to_string(),
            },
            path: FsPath {
                scope: "worktrees".to_string(),
                path: format!("repo-{}", branch.replace('/', "-")),
            },
            branch: branch.to_string(),
            base: Some("HEAD".to_string()),
            owner,
        }
    }

    /// REQ: API-001
    #[test]
    fn register_valid_spec_returns_an_id() {
        let mut import = test_import();
        let spec = spec_for_branch("feature/agent-one", Some(NodeId(OWNER_PANE)));

        let id = import.register(spec).expect("a valid spec must register");

        assert!(id > 0, "a registered worktree must get a non-zero id");
    }

    /// REQ: API-001
    #[test]
    fn register_accepts_absent_base_and_owner() {
        let mut import = test_import();
        let spec = WorkspaceSpec {
            base: None,
            owner: None,
            ..spec_for_branch("feature/no-owner", None)
        };

        let id = import.register(spec).expect("base and owner are optional");

        assert!(id > 0, "a registered worktree must get a non-zero id");
    }

    /// REQ: API-002
    #[test]
    fn register_empty_branch_is_invalid_argument() {
        let mut import = test_import();
        let spec = spec_for_branch("", None);

        let error = import
            .register(spec)
            .expect_err("an empty branch name must be refused");

        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn register_branch_with_invalid_git_syntax_is_invalid_argument() {
        let mut import = test_import();
        let spec = spec_for_branch("feature..bad name", None);

        let error = import
            .register(spec)
            .expect_err("a branch name with invalid Git syntax must be refused");

        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-003
    #[test]
    fn register_invalid_argument_carries_no_retry_after() {
        let mut import = test_import();
        let spec = spec_for_branch("", None);

        let error = import
            .register(spec)
            .expect_err("an empty branch name must be refused");

        assert_eq!(
            error.retry_after_ms, None,
            "retry-after-ms is only set for rate-limited and CPU-budget quota errors"
        );
    }

    /// REQ: CAP-013
    #[test]
    fn register_path_outside_worktrees_scope_is_not_permitted() {
        let mut import = test_import();
        let spec = WorkspaceSpec {
            path: FsPath {
                scope: "project".to_string(),
                path: "repo-feature-outside".to_string(),
            },
            ..spec_for_branch("feature/outside", None)
        };

        let error = import
            .register(spec)
            .expect_err("a worktree path outside fs.write:worktrees must be refused");

        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    /// REQ: CAP-013
    #[test]
    fn register_default_empty_spec_is_not_permitted() {
        let mut import = test_import();

        let error = import
            .register(WorkspaceSpec::default())
            .expect_err("an empty spec has no granted scope and must be refused");

        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    /// REQ: API-002
    #[test]
    fn register_missing_repository_folder_is_not_found() {
        let mut import = test_import();
        let spec = WorkspaceSpec {
            repository: FsPath {
                scope: "project".to_string(),
                path: "no-such-repository-folder".to_string(),
            },
            ..spec_for_branch("feature/no-repo", None)
        };

        let error = import
            .register(spec)
            .expect_err("a repository folder without a .git entry must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn register_unknown_owner_node_is_not_found() {
        let mut import = test_import();
        let spec = spec_for_branch("feature/ghost-owner", Some(NodeId(UNKNOWN_WORKSPACE_ID)));

        let error = import
            .register(spec)
            .expect_err("an owner node that does not exist must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn register_same_path_twice_is_conflict() {
        let mut import = test_import();
        let first = spec_for_branch("feature/duplicate", Some(NodeId(OWNER_PANE)));
        let second = first.clone();
        import
            .register(first)
            .expect("the first registration must succeed");

        let error = import
            .register(second)
            .expect_err("a path that is already registered must be refused");

        assert_eq!(error.code, ErrorCode::Conflict);
    }

    /// REQ: API-001
    #[test]
    fn list_on_empty_registry_returns_no_records() {
        let mut import = test_import();

        let records = import.list();

        assert!(
            records.is_empty(),
            "a fresh registry must hold no worktree records"
        );
    }

    /// REQ: API-001
    #[test]
    fn list_returns_registered_records_in_ascending_id_order_with_spec_fields() {
        let mut import = test_import();
        let first_spec = spec_for_branch("feature/first", Some(NodeId(OWNER_PANE)));
        let second_spec = spec_for_branch("feature/second", Some(NodeId(OWNER_PANE + 1)));
        let first_id = import
            .register(first_spec.clone())
            .expect("first registration must succeed");
        let second_id = import
            .register(second_spec.clone())
            .expect("second registration must succeed");

        let records = import.list();

        assert_eq!(records.len(), 2, "list must return every registered record");
        assert!(
            first_id < second_id,
            "ids must increase with registration order"
        );
        assert_eq!(records[0].id, first_id);
        assert_eq!(records[1].id, second_id);
        assert_eq!(records[0].path, first_spec.path);
        assert_eq!(records[0].branch, first_spec.branch);
        assert_eq!(records[0].owner, first_spec.owner);
        assert_eq!(records[1].path, second_spec.path);
        assert_eq!(records[1].branch, second_spec.branch);
        assert_eq!(records[1].owner, second_spec.owner);
    }

    /// REQ: API-001
    #[test]
    fn list_reports_absent_owner_as_none() {
        let mut import = test_import();
        let spec = spec_for_branch("feature/unowned", None);
        let id = import
            .register(spec)
            .expect("registration without an owner must succeed");

        let record = import
            .list()
            .into_iter()
            .find(|candidate| candidate.id == id)
            .expect("the registered record must be listed");

        assert_eq!(record.owner, None);
    }

    /// REQ: API-001
    #[test]
    fn list_reports_idle_freshly_registered_worktree_as_not_in_use() {
        let mut import = test_import();
        let spec = spec_for_branch("feature/idle", Some(NodeId(OWNER_PANE)));
        let id = import.register(spec).expect("registration must succeed");

        let record = import
            .list()
            .into_iter()
            .find(|candidate| candidate.id == id)
            .expect("the registered record must be listed");

        assert!(
            !record.in_use,
            "a worktree whose owner pane runs no process is not in use"
        );
    }

    /// REQ: API-001
    #[test]
    fn release_registered_record_succeeds_and_removes_it_from_list() {
        let mut import = test_import();
        let spec = spec_for_branch("feature/release-me", Some(NodeId(OWNER_PANE)));
        let id = import.register(spec).expect("registration must succeed");

        import
            .release(id)
            .expect("releasing a registered, unused record must succeed");

        assert!(
            import.list().iter().all(|candidate| candidate.id != id),
            "a released record must no longer be listed"
        );
    }

    /// REQ: API-002
    #[test]
    fn release_unknown_id_is_not_found() {
        let mut import = test_import();

        let error = import
            .release(UNKNOWN_WORKSPACE_ID)
            .expect_err("releasing an id that was never registered must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn release_same_id_twice_is_not_found_on_second_call() {
        let mut import = test_import();
        let spec = spec_for_branch("feature/release-twice", None);
        let id = import.register(spec).expect("registration must succeed");
        import.release(id).expect("the first release must succeed");

        let error = import
            .release(id)
            .expect_err("a second release of the same id must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    use super::{KernelWorkspaceWorld, WorkspaceWorldSource};
    use ilium_plugin_api::generated::types::tree::{AgentActivity, AgentView, NodeClass};
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead, WorkspaceAccess,
    };
    use ilium_plugin_api::ProcessKind;
    use std::sync::{Arc, Mutex};

    /// A kernel with a project folder and a tree, nothing else.
    struct ProjectKernel {
        project: String,
        nodes: Mutex<Vec<TreeNode>>,
    }

    impl TreeRead for ProjectKernel {
        fn nodes(&self) -> Vec<TreeNode> {
            self.nodes.lock().expect("nodes").clone()
        }

        fn node(&self, id: NodeId) -> Option<TreeNode> {
            self.nodes().into_iter().find(|node| node.id == id)
        }

        fn epoch(&self) -> u64 {
            1
        }
    }

    impl PaneRead for ProjectKernel {
        fn screen_text(&self, _pane: NodeId) -> Option<String> {
            None
        }

        fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
            None
        }

        fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
            None
        }
    }

    impl WorkspaceAccess for ProjectKernel {
        fn project_dir(&self) -> Option<String> {
            Some(self.project.clone())
        }

        fn session_name(&self) -> Option<String> {
            Some("default".to_owned())
        }
    }

    impl KernelFacade for ProjectKernel {
        fn apply_intent(
            &self,
            _origin: &Origin,
            _intent: ilium_plugin_api::generated::types::Intent,
        ) -> IntentResult {
            IntentResult::Ok
        }

        fn tree(&self) -> &dyn TreeRead {
            self
        }

        fn panes(&self) -> &dyn PaneRead {
            self
        }

        fn process_kind(&self) -> ProcessKind {
            ProcessKind::Server
        }

        fn workspace(&self) -> Option<&dyn WorkspaceAccess> {
            Some(self)
        }
    }

    fn pane(id: u64, cwd: &str, agent: bool) -> TreeNode {
        TreeNode {
            id: NodeId(id),
            parent: None,
            class: NodeClass::Pane,
            content_kind: Some("ilium.terminal".to_owned()),
            title: format!("pane {id}"),
            cwd: Some(cwd.to_owned()),
            agent: agent.then(|| AgentView {
                signature: "claude".to_owned(),
                activity: AgentActivity::Working,
                objective: None,
                goal: None,
            }),
            locked: false,
            revision: 1,
            container: None,
        }
    }

    /// REQ: API-001
    #[test]
    fn the_kernel_world_finds_repositories_nodes_and_busy_panes() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let project = scratch.path().join("app");
        std::fs::create_dir_all(project.join("repo").join(".git")).expect("repository");
        std::fs::create_dir_all(project.join("plain")).expect("plain folder");
        let worktree_cwd = scratch
            .path()
            .join("app.worktrees")
            .join("repo-feature")
            .join("src");
        let kernel = Arc::new(ProjectKernel {
            project: project.to_string_lossy().into_owned(),
            nodes: Mutex::new(vec![
                pane(OWNER_PANE, "/elsewhere", false),
                pane(OWNER_PANE + 1, &worktree_cwd.to_string_lossy(), true),
            ]),
        });
        let world = KernelWorkspaceWorld::new(kernel.clone());
        let folder = |scope: &str, path: &str| FsPath {
            scope: scope.to_owned(),
            path: path.to_owned(),
        };
        assert!(world.is_repository(&folder("project", "repo")));
        assert!(!world.is_repository(&folder("project", "plain")));
        assert!(
            !world.is_repository(&folder("project", "../app/repo")),
            "no climbing out"
        );
        assert!(
            !world.is_repository(&folder("data", "repo")),
            "only the project scope"
        );
        assert!(world.node_exists(OWNER_PANE));
        assert!(!world.node_exists(999));
        let worktree = folder("worktrees", "repo-feature");
        assert!(
            world.in_use(None, &worktree),
            "an agent pane works inside the worktree folder"
        );
        assert!(
            !world.in_use(Some(NodeId(OWNER_PANE)), &folder("worktrees", "other")),
            "an owner pane with only a shell holds nothing"
        );

        let mut import = WorkspaceImport::over_kernel(
            kernel.clone(),
            &[
                "workspace.manage".to_owned(),
                "fs.read:project".to_owned(),
                "fs.write:worktrees".to_owned(),
            ],
        );
        let id = import
            .register(WorkspaceSpec {
                repository: folder("project", "repo"),
                path: worktree.clone(),
                branch: "feature".to_owned(),
                base: None,
                owner: Some(NodeId(OWNER_PANE)),
            })
            .expect("a repository of the project registers");
        assert!(import.list()[0].in_use);
        let refused = import.release(id).expect_err("in use");
        assert_eq!(refused.code, ErrorCode::Conflict);
        kernel.nodes.lock().expect("nodes").pop();
        import
            .release(id)
            .expect("released once the agent pane is gone");
    }

    /// REQ: API-002
    #[test]
    fn register_without_the_scope_grants_is_not_permitted() {
        let kernel = Arc::new(ProjectKernel {
            project: "/nonexistent/app".to_owned(),
            nodes: Mutex::new(Vec::new()),
        });
        let mut import = WorkspaceImport::over_kernel(kernel, &["workspace.manage".to_owned()]);
        let error = import
            .register(spec_for_branch("feature/x", None))
            .expect_err("no fs grants");
        assert_eq!(error.code, ErrorCode::NotPermitted);
    }
}
