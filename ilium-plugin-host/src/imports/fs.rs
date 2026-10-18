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
}
