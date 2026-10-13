//! Host implementation of `ilium:host/fs` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostFs` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::fs::{
    DirEntry, FsMetadata, HostFs, PickSpec, RevisionToken, Revisioned, ScannedEntry, ScopeRoot,
    TailChunk, WriteMode,
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

    fn metadata(&mut self, path: FsPath) -> Result<FsMetadata, PluginError> {
        unimplemented!("host::imports::fs::metadata")
    }

    fn list_dir(&mut self, path: FsPath, max_entries: u32) -> Result<Vec<DirEntry>, PluginError> {
        unimplemented!("host::imports::fs::list_dir")
    }

    fn read(&mut self, path: FsPath, offset: u64, max_bytes: u32) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::fs::read")
    }

    fn read_all(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::read_all")
    }

    fn scan(
        &mut self,
        path: FsPath,
        glob: String,
        max_entries: u32,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::scan")
    }

    fn write(
        &mut self,
        path: FsPath,
        data: Vec<u8>,
        mode: WriteMode,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::write")
    }

    fn create_dir(&mut self, path: FsPath) -> Result<(), PluginError> {
        unimplemented!("host::imports::fs::create_dir")
    }

    fn remove(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::remove")
    }

    fn rename(&mut self, source: FsPath, target: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::rename")
    }

    fn watch(&mut self, path: FsPath, recursive: bool) -> Result<WatchId, PluginError> {
        unimplemented!("host::imports::fs::watch")
    }

    fn unwatch(&mut self, watch: WatchId) {
        unimplemented!("host::imports::fs::unwatch")
    }

    fn lock(
        &mut self,
        path: FsPath,
        exclusive: bool,
        timeout_millis: u32,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::lock")
    }

    fn unlock(&mut self, lock: LockId) {
        unimplemented!("host::imports::fs::unlock")
    }

    fn pick(&mut self, spec: PickSpec) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::pick")
    }

    fn read_with_revision(&mut self, path: FsPath) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::read_with_revision")
    }

    fn write_atomic(
        &mut self,
        path: FsPath,
        data: Vec<u8>,
        expected: Option<RevisionToken>,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::fs::write_atomic")
    }

    fn tail(
        &mut self,
        path: FsPath,
        cursor: u64,
        max_bytes: u32,
    ) -> Result<TailChunk, PluginError> {
        unimplemented!("host::imports::fs::tail")
    }

    fn take_bytes(&mut self, request: RequestId) -> Option<Vec<u8>> {
        unimplemented!("host::imports::fs::take_bytes")
    }

    fn take_entries(&mut self, request: RequestId) -> Option<Vec<ScannedEntry>> {
        unimplemented!("host::imports::fs::take_entries")
    }

    fn take_lock(&mut self, request: RequestId) -> Option<LockId> {
        unimplemented!("host::imports::fs::take_lock")
    }

    fn take_picked(&mut self, request: RequestId) -> Option<FsPath> {
        unimplemented!("host::imports::fs::take_picked")
    }

    fn take_revisioned(&mut self, request: RequestId) -> Option<Revisioned> {
        unimplemented!("host::imports::fs::take_revisioned")
    }

    fn take_written(&mut self, request: RequestId) -> Option<RevisionToken> {
        unimplemented!("host::imports::fs::take_written")
    }
}
