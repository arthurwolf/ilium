//! Checked `ilium:host/storage` adapter over the durable per-plugin engine.
//! Storage is always granted (api-storage.md); instance revocation and ownership
//! still apply. Runtime admission must supply the real scope and resolved limits.

use crate::storage::{PluginStorage, StorageContext};
use ilium_plugin_api::{
    generated::host::storage::{Bucket, Entry, HostStorage},
    ids::PluginId,
    ErrorCode, PluginError, Side, Tier,
};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

/// Host-owned admission metadata, never taken from a storage call's guest args.
#[derive(Debug, Clone)]
pub struct StorageImportContext {
    pub plugin: PluginId,
    pub side: Side,
    pub scope: StorageContext,
    pub tier: Tier,
    pub bucket_limit_bytes: usize,
    pub shared_limit_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageInstanceState {
    Active,
    Revoked,
    Inactive,
}

/// All lifecycle/revocation changes must lock this same gate. Lock ordering is
/// authorization then bucket transaction. Never acquire this gate from inside
/// a storage transaction or invoke an import while holding it.
#[derive(Debug)]
pub struct StorageAuthorization {
    plugin: PluginId,
    side: Side,
    state: StorageInstanceState,
}

impl StorageAuthorization {
    pub fn new(plugin: PluginId, side: Side, state: StorageInstanceState) -> Self {
        Self {
            plugin,
            side,
            state,
        }
    }
    pub fn set_state(&mut self, state: StorageInstanceState) {
        self.state = state;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOperation {
    Get,
    Put,
    Remove,
    Keys,
    WriteBatch,
    Lease,
}

type DiagnosticSink = dyn Fn(StorageOperation, &PluginError) + Send + Sync;

pub struct StorageImport {
    engine: PluginStorage,
    context: StorageImportContext,
    authorization: Arc<Mutex<StorageAuthorization>>,
    diagnostic: Box<DiagnosticSink>,
}

impl std::fmt::Debug for StorageImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StorageImport")
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

fn authorize<'a>(
    authorization: &'a Mutex<StorageAuthorization>,
    context: &StorageImportContext,
) -> Result<MutexGuard<'a, StorageAuthorization>, PluginError> {
    let guard = authorization.lock().map_err(|_| {
        PluginError::new(ErrorCode::Internal, "storage authorization gate poisoned")
    })?;
    if guard.plugin != context.plugin || guard.side != context.side {
        return Err(PluginError::new(
            ErrorCode::NotPermitted,
            "storage-instance-owner-mismatch",
        ));
    }
    match guard.state {
        StorageInstanceState::Active => Ok(guard),
        StorageInstanceState::Revoked => Err(PluginError::new(
            ErrorCode::Revoked,
            "storage-instance-revoked",
        )),
        StorageInstanceState::Inactive => Err(PluginError::new(
            ErrorCode::NotPermitted,
            "storage-instance-inactive",
        )),
    }
}

impl StorageImport {
    /// Creates only the admitted plugin's private engine. Authorization is
    /// checked before directory creation and held through engine admission.
    /// The sink must route refusals to the runtime's real diagnostic/log path;
    /// it must not reenter storage or take the authorization gate.
    pub fn open(
        root: &Path,
        context: StorageImportContext,
        authorization: Arc<Mutex<StorageAuthorization>>,
        diagnostic: impl Fn(StorageOperation, &PluginError) + Send + Sync + 'static,
    ) -> Result<Self, PluginError> {
        let engine = {
            let _permit = authorize(&authorization, &context)?;
            PluginStorage::open_with_context(
                root,
                context.plugin.clone(),
                context.scope.clone(),
                context.tier,
                context.bucket_limit_bytes,
                context.shared_limit_bytes,
            )?
        };
        Ok(Self {
            engine,
            context,
            authorization,
            diagnostic: Box::new(diagnostic),
        })
    }

    pub fn context(&self) -> &StorageImportContext {
        &self.context
    }

    /// Checked forms preserve errors hidden by the generated Option/void/list
    /// ABI. Host callers may use these directly and own their error reporting.
    pub fn try_get(&self, bucket: Bucket, key: &str) -> Result<Option<Vec<u8>>, PluginError> {
        let _permit = authorize(&self.authorization, &self.context)?;
        self.engine.get(bucket.as_str(), key)
    }
    pub fn try_put(
        &mut self,
        bucket: Bucket,
        key: &str,
        value: Vec<u8>,
    ) -> Result<(), PluginError> {
        let _permit = authorize(&self.authorization, &self.context)?;
        self.engine.put(bucket.as_str(), key, value)
    }
    pub fn try_remove(&mut self, bucket: Bucket, key: &str) -> Result<(), PluginError> {
        let _permit = authorize(&self.authorization, &self.context)?;
        self.engine.remove(bucket.as_str(), key)
    }
    pub fn try_keys(&self, bucket: Bucket, prefix: &str) -> Result<Vec<String>, PluginError> {
        let _permit = authorize(&self.authorization, &self.context)?;
        self.engine.keys(bucket.as_str(), prefix)
    }
    pub fn try_write_batch(
        &mut self,
        bucket: Bucket,
        entries: Vec<Entry>,
    ) -> Result<(), PluginError> {
        let _permit = authorize(&self.authorization, &self.context)?;
        let entries: Vec<_> = entries
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();
        self.engine.write_batch(bucket.as_str(), &entries)
    }
    pub fn try_lease(&self, key: &str, ttl_millis: u32) -> Result<bool, PluginError> {
        self.lease_at(key, ttl_millis, std::time::SystemTime::now())
    }
    /// Explicit host clock sample for deterministic replay/testing. The same
    /// instance authorization remains held through the durable lease mutation.
    pub fn lease_at(
        &self,
        key: &str,
        ttl_millis: u32,
        now: std::time::SystemTime,
    ) -> Result<bool, PluginError> {
        let _permit = authorize(&self.authorization, &self.context)?;
        self.engine.lease_at(key, ttl_millis, now)
    }

    fn report<T>(
        &self,
        operation: StorageOperation,
        result: Result<T, PluginError>,
    ) -> Result<T, PluginError> {
        if let Err(error) = &result {
            (self.diagnostic)(operation, error);
        }
        result
    }
}

impl HostStorage for StorageImport {
    fn get(&mut self, bucket: Bucket, key: String) -> Option<Vec<u8>> {
        self.report(StorageOperation::Get, self.try_get(bucket, &key))
            .ok()
            .flatten()
    }
    fn put(&mut self, bucket: Bucket, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        let result = self.try_put(bucket, &key, value);
        self.report(StorageOperation::Put, result)
    }
    fn remove(&mut self, bucket: Bucket, key: String) {
        let result = self.try_remove(bucket, &key);
        let _ = self.report(StorageOperation::Remove, result);
    }
    fn keys(&mut self, bucket: Bucket, prefix: String) -> Vec<String> {
        self.report(StorageOperation::Keys, self.try_keys(bucket, &prefix))
            .unwrap_or_default()
    }
    fn write_batch(&mut self, bucket: Bucket, entries: Vec<Entry>) -> Result<(), PluginError> {
        let result = self.try_write_batch(bucket, entries);
        self.report(StorageOperation::WriteBatch, result)
    }
    fn lease(&mut self, key: String, ttl_millis: u32) -> bool {
        self.report(StorageOperation::Lease, self.try_lease(&key, ttl_millis))
            .unwrap_or(false)
    }
}
