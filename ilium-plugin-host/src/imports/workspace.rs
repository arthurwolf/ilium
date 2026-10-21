//! Host implementation of `ilium:host/workspace` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostWorkspace` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::workspace::{HostWorkspace, WorkspaceInfo, WorkspaceSpec};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `workspace` interface.
#[derive(Debug)]
pub struct WorkspaceImport;

impl HostWorkspace for WorkspaceImport {
    fn register(&mut self, _spec: WorkspaceSpec) -> Result<u64, PluginError> {
        unimplemented!("host::imports::workspace::register")
    }

    fn release(&mut self, _workspace: u64) -> Result<(), PluginError> {
        unimplemented!("host::imports::workspace::release")
    }

    fn list(&mut self) -> Vec<WorkspaceInfo> {
        unimplemented!("host::imports::workspace::list")
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceImport;
    use ilium_plugin_api::generated::host::workspace::{HostWorkspace, WorkspaceSpec};
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::paths::FsPath;

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
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature/agent-one", Some(NodeId(OWNER_PANE)));

        let id = import.register(spec).expect("a valid spec must register");

        assert!(id > 0, "a registered worktree must get a non-zero id");
    }

    /// REQ: API-001
    #[test]
    fn register_accepts_absent_base_and_owner() {
        let mut import = WorkspaceImport;
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
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("", None);

        let error = import.register(spec).expect_err("an empty branch name must be refused");

        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn register_branch_with_invalid_git_syntax_is_invalid_argument() {
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature..bad name", None);

        let error = import.register(spec).expect_err("a branch name with invalid Git syntax must be refused");

        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: API-003
    #[test]
    fn register_invalid_argument_carries_no_retry_after() {
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("", None);

        let error = import.register(spec).expect_err("an empty branch name must be refused");

        assert_eq!(error.retry_after_ms, None, "retry-after-ms is only set for rate-limited and CPU-budget quota errors");
    }

    /// REQ: CAP-013
    #[test]
    fn register_path_outside_worktrees_scope_is_not_permitted() {
        let mut import = WorkspaceImport;
        let spec = WorkspaceSpec {
            path: FsPath {
                scope: "project".to_string(),
                path: "repo-feature-outside".to_string(),
            },
            ..spec_for_branch("feature/outside", None)
        };

        let error = import.register(spec).expect_err("a worktree path outside fs.write:worktrees must be refused");

        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    /// REQ: CAP-013
    #[test]
    fn register_default_empty_spec_is_not_permitted() {
        let mut import = WorkspaceImport;

        let error = import
            .register(WorkspaceSpec::default())
            .expect_err("an empty spec has no granted scope and must be refused");

        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    /// REQ: API-002
    #[test]
    fn register_missing_repository_folder_is_not_found() {
        let mut import = WorkspaceImport;
        let spec = WorkspaceSpec {
            repository: FsPath {
                scope: "project".to_string(),
                path: "no-such-repository-folder".to_string(),
            },
            ..spec_for_branch("feature/no-repo", None)
        };

        let error = import.register(spec).expect_err("a repository folder without a .git entry must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn register_unknown_owner_node_is_not_found() {
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature/ghost-owner", Some(NodeId(UNKNOWN_WORKSPACE_ID)));

        let error = import.register(spec).expect_err("an owner node that does not exist must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn register_same_path_twice_is_conflict() {
        let mut import = WorkspaceImport;
        let first = spec_for_branch("feature/duplicate", Some(NodeId(OWNER_PANE)));
        let second = first.clone();
        import.register(first).expect("the first registration must succeed");

        let error = import.register(second).expect_err("a path that is already registered must be refused");

        assert_eq!(error.code, ErrorCode::Conflict);
    }

    /// REQ: API-001
    #[test]
    fn list_on_empty_registry_returns_no_records() {
        let mut import = WorkspaceImport;

        let records = import.list();

        assert!(records.is_empty(), "a fresh registry must hold no worktree records");
    }

    /// REQ: API-001
    #[test]
    fn list_returns_registered_records_in_ascending_id_order_with_spec_fields() {
        let mut import = WorkspaceImport;
        let first_spec = spec_for_branch("feature/first", Some(NodeId(OWNER_PANE)));
        let second_spec = spec_for_branch("feature/second", Some(NodeId(OWNER_PANE + 1)));
        let first_id = import.register(first_spec.clone()).expect("first registration must succeed");
        let second_id = import.register(second_spec.clone()).expect("second registration must succeed");

        let records = import.list();

        assert_eq!(records.len(), 2, "list must return every registered record");
        assert!(first_id < second_id, "ids must increase with registration order");
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
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature/unowned", None);
        let id = import.register(spec).expect("registration without an owner must succeed");

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
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature/idle", Some(NodeId(OWNER_PANE)));
        let id = import.register(spec).expect("registration must succeed");

        let record = import
            .list()
            .into_iter()
            .find(|candidate| candidate.id == id)
            .expect("the registered record must be listed");

        assert!(!record.in_use, "a worktree whose owner pane runs no process is not in use");
    }

    /// REQ: API-001
    #[test]
    fn release_registered_record_succeeds_and_removes_it_from_list() {
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature/release-me", Some(NodeId(OWNER_PANE)));
        let id = import.register(spec).expect("registration must succeed");

        import.release(id).expect("releasing a registered, unused record must succeed");

        assert!(
            import.list().iter().all(|candidate| candidate.id != id),
            "a released record must no longer be listed"
        );
    }

    /// REQ: API-002
    #[test]
    fn release_unknown_id_is_not_found() {
        let mut import = WorkspaceImport;

        let error = import
            .release(UNKNOWN_WORKSPACE_ID)
            .expect_err("releasing an id that was never registered must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn release_same_id_twice_is_not_found_on_second_call() {
        let mut import = WorkspaceImport;
        let spec = spec_for_branch("feature/release-twice", None);
        let id = import.register(spec).expect("registration must succeed");
        import.release(id).expect("the first release must succeed");

        let error = import.release(id).expect_err("a second release of the same id must be refused");

        assert_eq!(error.code, ErrorCode::NotFound);
    }
}
