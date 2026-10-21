//! Host implementation of `ilium:host/fs` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostFs` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::fs::{
    ConfigChange, DirEntry, FsMetadata, HostFs, PickSpec, ProposalResult, RevisionToken,
    Revisioned, ScannedEntry, ScopeRoot, TailChunk, WriteMode,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{LockId, RequestId, WatchId};
use ilium_plugin_api::generated::types::paths::FsPath;

/// Host side of the `fs` interface.
#[derive(Debug)]
pub struct FsImport;

impl HostFs for FsImport {
    fn roots(&mut self) -> Vec<ScopeRoot> {
        unimplemented!("host::imports::fs::roots")
    }

    fn metadata(&mut self, _path: FsPath) -> Result<FsMetadata, PluginError> {
        unimplemented!("host::imports::fs::metadata")
    }

    fn list_dir(&mut self, _path: FsPath, _max_entries: u32) -> Result<Vec<DirEntry>, PluginError> {
        unimplemented!("host::imports::fs::list_dir")
    }

    fn read(
        &mut self,
        _path: FsPath,
        _offset: u64,
        _max_bytes: u32,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::fs::read")
    }

    fn read_all(&mut self, _path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::read_all")
    }

    fn scan(
        &mut self,
        _path: FsPath,
        _glob: String,
        _max_entries: u32,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::scan")
    }

    fn write(
        &mut self,
        _path: FsPath,
        _data: Vec<u8>,
        _mode: WriteMode,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::write")
    }

    fn create_dir(&mut self, _path: FsPath) -> Result<(), PluginError> {
        unimplemented!("host::imports::fs::create_dir")
    }

    fn remove(&mut self, _path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::remove")
    }

    fn rename(&mut self, _source: FsPath, _target: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::rename")
    }

    fn watch(&mut self, _path: FsPath, _recursive: bool) -> Result<WatchId, PluginError> {
        unimplemented!("host::imports::fs::watch")
    }

    fn unwatch(&mut self, _watch: WatchId) {
        unimplemented!("host::imports::fs::unwatch")
    }

    fn lock(
        &mut self,
        _path: FsPath,
        _exclusive: bool,
        _timeout_millis: u32,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::lock")
    }

    fn unlock(&mut self, _lock: LockId) {
        unimplemented!("host::imports::fs::unlock")
    }

    fn pick(&mut self, _spec: PickSpec) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::pick")
    }

    fn read_with_revision(&mut self, _path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::read_with_revision")
    }

    fn write_atomic(
        &mut self,
        _path: FsPath,
        _data: Vec<u8>,
        _expected: Option<RevisionToken>,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::write_atomic")
    }

    fn tail(
        &mut self,
        _path: FsPath,
        _cursor: u64,
        _max_bytes: u32,
    ) -> Result<TailChunk, PluginError> {
        unimplemented!("host::imports::fs::tail")
    }

    fn take_bytes(&mut self, _request: RequestId) -> Option<Vec<u8>> {
        unimplemented!("host::imports::fs::take_bytes")
    }

    fn take_entries(&mut self, _request: RequestId) -> Option<Vec<ScannedEntry>> {
        unimplemented!("host::imports::fs::take_entries")
    }

    fn take_lock(&mut self, _request: RequestId) -> Option<LockId> {
        unimplemented!("host::imports::fs::take_lock")
    }

    fn take_picked(&mut self, _request: RequestId) -> Option<FsPath> {
        unimplemented!("host::imports::fs::take_picked")
    }

    fn take_revisioned(&mut self, _request: RequestId) -> Option<Revisioned> {
        unimplemented!("host::imports::fs::take_revisioned")
    }

    fn take_written(&mut self, _request: RequestId) -> Option<RevisionToken> {
        unimplemented!("host::imports::fs::take_written")
    }

    fn propose_agent_config(
        &mut self,
        _agent: String,
        _title: String,
        _changes: Vec<ConfigChange>,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::propose_agent_config")
    }

    fn take_proposal(&mut self, _request: RequestId) -> Option<ProposalResult> {
        unimplemented!("host::imports::fs::take_proposal")
    }
}

#[cfg(test)]
mod tests {
    use super::FsImport;
    use ilium_plugin_api::generated::host::fs::{HostFs, PickKind, PickSpec, WriteMode};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::paths::FsPath;

    /// Largest `max_bytes` a synchronous `read` or `tail` accepts (1 MiB, fixed cap).
    const ONE_MIB: u32 = 1024 * 1024;
    /// Largest `max_entries` for `list_dir` (fixed cap).
    const LIST_DIR_MAX_ENTRIES: u32 = 4_096;
    /// Largest `max_entries` for `scan` (fixed cap).
    const SCAN_MAX_ENTRIES: u32 = 65_536;
    /// Largest payload of one `write` (`fs_write_mib`, 16 MiB by default).
    const WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;

    fn fs_path(scope: &str, path: &str) -> FsPath {
        FsPath {
            scope: scope.to_owned(),
            path: path.to_owned(),
        }
    }

    fn error_code_of<T>(result: Result<T, PluginError>) -> Option<ErrorCode> {
        result.err().map(|error| error.code)
    }

    /// REQ: CAP-011
    #[test]
    fn roots_lists_the_always_granted_scopes() {
        let mut fs_import = FsImport;
        let roots = fs_import.roots();
        let scope_names: Vec<&str> = roots.iter().map(|root| root.scope.as_str()).collect();
        assert!(scope_names.contains(&"package"), "package scope missing: {scope_names:?}");
        assert!(scope_names.contains(&"data"), "data scope missing: {scope_names:?}");
        assert!(scope_names.contains(&"cache"), "cache scope missing: {scope_names:?}");
    }

    /// REQ: CAP-011
    #[test]
    fn roots_marks_package_read_only_and_data_writable() {
        let mut fs_import = FsImport;
        let roots = fs_import.roots();
        let package = roots.iter().find(|root| root.scope == "package");
        let data = roots.iter().find(|root| root.scope == "data");
        assert!(matches!(package, Some(root) if !root.writable), "package must be read only");
        assert!(matches!(data, Some(root) if root.writable), "data must be writable");
    }

    /// REQ: API-002
    #[test]
    fn metadata_of_absent_entry_is_not_found() {
        let mut fs_import = FsImport;
        let result = fs_import.metadata(fs_path("data", "no-such-entry.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-001
    #[test]
    fn metadata_error_carries_a_message_within_the_limit() {
        let mut fs_import = FsImport;
        let result = fs_import.metadata(fs_path("data", "no-such-entry.txt"));
        match result {
            Ok(_) => panic!("metadata of an absent entry must fail"),
            Err(error) => {
                assert!(!error.message.is_empty(), "error message must not be empty");
                assert!(error.message.chars().count() <= 256, "message over 256 characters");
            }
        }
    }

    /// REQ: SEC-058
    #[test]
    fn metadata_path_escaping_the_scope_root_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.metadata(fs_path("data", "../escape.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn metadata_in_an_ungranted_scope_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.metadata(fs_path("project", "README.md"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-062
    #[test]
    fn list_dir_above_the_entry_cap_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.list_dir(fs_path("data", ""), LIST_DIR_MAX_ENTRIES + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-062
    #[test]
    fn list_dir_at_the_entry_cap_is_accepted() {
        let mut fs_import = FsImport;
        let result = fs_import.list_dir(fs_path("data", ""), LIST_DIR_MAX_ENTRIES);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-002
    #[test]
    fn list_dir_of_absent_folder_is_not_found() {
        let mut fs_import = FsImport;
        let result = fs_import.list_dir(fs_path("data", "no-such-folder"), 10);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-062
    #[test]
    fn read_above_one_mib_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.read(fs_path("data", "blob.bin"), 0, ONE_MIB + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-062
    #[test]
    fn read_at_exactly_one_mib_is_accepted() {
        let mut fs_import = FsImport;
        let result = fs_import.read(fs_path("data", "blob.bin"), 0, ONE_MIB);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn read_outside_the_scope_fails_at_once() {
        let mut fs_import = FsImport;
        let result = fs_import.read(fs_path("data", "../../etc/passwd"), 0, 16);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-002
    #[test]
    fn read_of_absent_file_is_not_found() {
        let mut fs_import = FsImport;
        let result = fs_import.read(fs_path("data", "no-such-file.bin"), 0, 16);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-005
    #[test]
    fn read_all_outside_the_scope_fails_in_the_call() {
        let mut fs_import = FsImport;
        let result = fs_import.read_all(fs_path("data", "../secret.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn read_all_in_an_ungranted_scope_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.read_all(fs_path("picked", "0"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-061
    #[test]
    fn scan_above_the_entry_cap_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.scan(fs_path("data", ""), "*.md".to_owned(), SCAN_MAX_ENTRIES + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-061
    #[test]
    fn scan_at_the_entry_cap_is_accepted() {
        let mut fs_import = FsImport;
        let result = fs_import.scan(fs_path("data", ""), "*.md".to_owned(), SCAN_MAX_ENTRIES);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn scan_with_a_malformed_glob_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.scan(fs_path("data", ""), "[unclosed".to_owned(), 10);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn write_above_the_payload_cap_is_invalid_argument() {
        let mut fs_import = FsImport;
        let data = vec![0_u8; WRITE_MAX_BYTES + 1];
        let result = fs_import.write(fs_path("data", "large.bin"), data, WriteMode::Replace);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn write_at_the_payload_cap_is_accepted() {
        let mut fs_import = FsImport;
        let data = vec![0_u8; WRITE_MAX_BYTES];
        let result = fs_import.write(fs_path("data", "large.bin"), data, WriteMode::Replace);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn empty_write_is_not_refused_by_the_call_checks() {
        let mut fs_import = FsImport;
        let result = fs_import.write(fs_path("data", "empty.txt"), Vec::new(), WriteMode::Append);
        assert_ne!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn write_to_the_read_only_package_scope_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.write(
            fs_path("package", "assets/file.txt"),
            b"new".to_vec(),
            WriteMode::Replace,
        );
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-002
    #[test]
    fn create_dir_with_a_missing_parent_is_not_found() {
        let mut fs_import = FsImport;
        let result = fs_import.create_dir(fs_path("data", "missing-parent/child"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: API-005
    #[test]
    fn create_dir_outside_the_scope_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.create_dir(fs_path("data", "../outside"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn remove_of_a_scope_root_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.remove(fs_path("data", ""));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn rename_across_two_scopes_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.rename(fs_path("data", "a.txt"), fs_path("cache", "a.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-002
    #[test]
    fn watch_of_absent_folder_is_not_found() {
        let mut fs_import = FsImport;
        let result = fs_import.watch(fs_path("data", "no-such-folder"), true);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// REQ: SEC-058
    #[test]
    fn watch_outside_the_scope_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.watch(fs_path("data", "../elsewhere"), false);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn lock_of_a_file_that_does_not_exist_is_accepted() {
        let mut fs_import = FsImport;
        let result = fs_import.lock(fs_path("data", "not-yet-created.lock"), true, 2_000);
        assert!(result.is_ok(), "a lock need not name an existing file");
    }

    /// REQ: API-005
    #[test]
    fn lock_needs_write_grant_so_read_only_package_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.lock(fs_path("package", "assets/file.txt"), false, 0);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn pick_without_the_fs_pick_grant_is_not_permitted() {
        let mut fs_import = FsImport;
        let spec = PickSpec {
            kind: PickKind::OpenFile,
            title: "Choose a file".to_owned(),
            extensions: vec!["md".to_owned()],
        };
        let result = fs_import.pick(spec);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn read_with_revision_outside_the_scope_fails_in_the_call() {
        let mut fs_import = FsImport;
        let result = fs_import.read_with_revision(fs_path("data", "../secret.txt"));
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-005
    #[test]
    fn write_atomic_to_the_read_only_package_scope_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.write_atomic(fs_path("package", "assets/file.txt"), b"x".to_vec(), None);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: API-062
    #[test]
    fn tail_above_one_mib_is_invalid_argument() {
        let mut fs_import = FsImport;
        let result = fs_import.tail(fs_path("data", "log.txt"), 0, ONE_MIB + 1);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// REQ: API-005
    #[test]
    fn tail_outside_the_scope_fails_at_once() {
        let mut fs_import = FsImport;
        let result = fs_import.tail(fs_path("data", "../log.txt"), 0, 1024);
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }

    /// REQ: CAP-018
    #[test]
    fn propose_agent_config_without_its_grant_is_not_permitted() {
        let mut fs_import = FsImport;
        let result = fs_import.propose_agent_config(
            "claude".to_owned(),
            "Update settings".to_owned(),
            Vec::new(),
        );
        assert_eq!(error_code_of(result), Some(ErrorCode::NotPermitted));
    }
}
