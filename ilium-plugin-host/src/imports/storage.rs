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
    fn remove(&mut self, bucket: Bucket, key: String) -> Result<(), PluginError> {
        let result = self.try_remove(bucket, &key);
        self.report(StorageOperation::Remove, result)
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

#[cfg(test)]
mod tests {
    use super::{
        StorageAuthorization, StorageImport, StorageImportContext, StorageInstanceState,
    };
    use crate::storage::{StorageContext, BUCKET_MAX_BYTES};
    use ilium_plugin_api::{ids::PluginId, Side, Tier};
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };
    use ilium_plugin_api::generated::host::storage::{Bucket, Entry, HostStorage};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// Longest key the host accepts, in bytes (api-storage, Rules).
    const MAX_KEY_BYTES: usize = 256;
    /// Largest value one `put` accepts, in bytes (1 MiB).
    const MAX_VALUE_BYTES: usize = 1024 * 1024;
    /// Default size of one bucket, key bytes plus value bytes (16 MiB).
    const BUCKET_QUOTA_BYTES: usize = 16 * 1024 * 1024;
    /// Most entries one `write-batch` accepts (LIF-117).
    const MAX_BATCH_ENTRIES: usize = 256;
    /// Most key and value bytes one `write-batch` accepts together (4 MiB, LIF-117).
    const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;
    /// Length of every key in the quota test, chosen so that a value fills one MiB with its key.
    const QUOTA_KEY_BYTES: usize = 16;

    /// Opens an active, admitted storage import for one test plugin under `root`.
    fn open_storage_at(root: &Path) -> StorageImport {
        let plugin = PluginId("example.storage-test".to_owned());
        let context = StorageImportContext {
            plugin: plugin.clone(),
            side: Side::Server,
            scope: StorageContext {
                project: Some(PathBuf::from("/tmp/ilium-storage-test-project")),
                session: Some("default".to_owned()),
                data_version: 1,
            },
            tier: Tier::T2,
            bucket_limit_bytes: BUCKET_MAX_BYTES,
            shared_limit_bytes: 128 * 1024 * 1024,
        };
        let authorization = Arc::new(Mutex::new(StorageAuthorization::new(
            plugin,
            Side::Server,
            StorageInstanceState::Active,
        )));
        StorageImport::open(root, context, authorization, |_, _| {})
            .expect("open storage import")
    }

    /// A fresh storage import on its own temp root; keep the returned guard alive for the test.
    fn open_storage() -> (tempfile::TempDir, StorageImport) {
        let root = tempfile::tempdir().expect("temp storage root");
        let storage = open_storage_at(root.path());
        (root, storage)
    }

    /// A run-unique token, so keys from an earlier run of the suite never collide with this one.
    fn run_nonce() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    }

    /// Key scoped to one test and to this run.
    fn unique_key(test_name: &str, suffix: &str) -> String {
        format!("test/{}/{test_name}/{suffix}", run_nonce())
    }

    /// The error code of a result that must have failed.
    fn error_code_of(result: Result<(), PluginError>) -> ErrorCode {
        match result {
            Err(error) => error.code,
            Ok(()) => panic!("expected an error code, but the call returned Ok"),
        }
    }

    /// Builds one batch entry.
    fn entry(key: &str, value: Option<Vec<u8>>) -> Entry {
        Entry {
            key: key.to_owned(),
            value,
        }
    }

    /// Removes keys from a bucket, failing the test if the host refuses a removal.
    fn clear_keys(storage: &mut StorageImport, bucket: Bucket, keys: &[String]) {
        for key in keys {
            assert_eq!(storage.remove(bucket, key.clone()), Ok(()));
        }
    }

    /// REQ: CAP-011
    #[test]
    fn get_of_missing_key_returns_none_without_error() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("get_missing", "absent");
        assert_eq!(storage.get(Bucket::Global, key), None);
    }

    /// REQ: CAP-011
    #[test]
    fn remove_of_missing_key_is_not_an_error() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("remove_missing", "absent");
        assert_eq!(storage.remove(Bucket::Global, key), Ok(()));
    }

    /// REQ: CAP-011
    #[test]
    fn put_then_get_returns_the_value_without_any_grant() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("put_get", "counter");
        assert_eq!(
            storage.put(Bucket::Global, key.clone(), vec![1, 2, 3]),
            Ok(())
        );
        assert_eq!(storage.get(Bucket::Global, key), Some(vec![1, 2, 3]));
    }

    /// REQ: LIF-118
    #[test]
    fn put_is_readable_as_soon_as_it_returns() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("put_readback", "cursor");
        assert_eq!(
            storage.put(Bucket::Session, key.clone(), 7u32.to_le_bytes().to_vec()),
            Ok(())
        );
        assert_eq!(
            storage.get(Bucket::Session, key),
            Some(7u32.to_le_bytes().to_vec())
        );
    }

    /// REQ: LIF-118
    #[test]
    fn second_put_replaces_the_whole_previous_value() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("put_replace", "value");
        assert_eq!(
            storage.put(Bucket::Global, key.clone(), vec![9; 100]),
            Ok(())
        );
        assert_eq!(storage.put(Bucket::Global, key.clone(), vec![4]), Ok(()));
        assert_eq!(storage.get(Bucket::Global, key), Some(vec![4]));
    }

    /// REQ: LIF-118
    #[test]
    fn empty_value_is_stored_and_differs_from_a_missing_key() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("empty_value", "flag");
        assert_eq!(storage.put(Bucket::Global, key.clone(), Vec::new()), Ok(()));
        assert_eq!(storage.get(Bucket::Global, key), Some(Vec::new()));
    }

    /// REQ: CAP-011
    #[test]
    fn same_key_in_two_buckets_holds_two_values() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("buckets", "shared-name");
        assert_eq!(storage.put(Bucket::Global, key.clone(), vec![1]), Ok(()));
        assert_eq!(storage.put(Bucket::Project, key.clone(), vec![2]), Ok(()));
        assert_eq!(storage.get(Bucket::Global, key.clone()), Some(vec![1]));
        assert_eq!(storage.get(Bucket::Project, key), Some(vec![2]));
    }

    /// REQ: CAP-011
    #[test]
    fn remove_deletes_the_key() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("remove", "gone");
        assert_eq!(storage.put(Bucket::Global, key.clone(), vec![5]), Ok(()));
        assert_eq!(storage.remove(Bucket::Global, key.clone()), Ok(()));
        assert_eq!(storage.get(Bucket::Global, key), None);
    }

    /// REQ: LIF-117
    #[test]
    fn put_rejects_an_empty_key_with_invalid_argument() {
        let (_root, mut storage) = open_storage();
        let result = storage.put(Bucket::Global, String::new(), vec![1]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
    }

    /// REQ: LIF-117
    #[test]
    fn put_accepts_a_key_of_exactly_256_bytes() {
        let (_root, mut storage) = open_storage();
        let key = "k".repeat(MAX_KEY_BYTES);
        assert_eq!(storage.put(Bucket::Global, key.clone(), vec![1]), Ok(()));
        assert_eq!(storage.get(Bucket::Global, key), Some(vec![1]));
    }

    /// REQ: LIF-117
    #[test]
    fn put_rejects_a_key_of_257_bytes_and_keeps_the_previous_value() {
        let (_root, mut storage) = open_storage();
        let short_key = unique_key("key_limit", "previous");
        let long_key = "k".repeat(MAX_KEY_BYTES + 1);
        assert_eq!(
            storage.put(Bucket::Global, short_key.clone(), vec![8]),
            Ok(())
        );
        let result = storage.put(Bucket::Global, long_key.clone(), vec![1]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
        assert_eq!(storage.get(Bucket::Global, long_key), None);
        assert_eq!(storage.get(Bucket::Global, short_key), Some(vec![8]));
    }

    /// REQ: LIF-117
    #[test]
    fn put_accepts_a_value_of_exactly_one_mib() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("value_limit", "full");
        let value = vec![0xAB; MAX_VALUE_BYTES];
        assert_eq!(
            storage.put(Bucket::Global, key.clone(), value.clone()),
            Ok(())
        );
        assert_eq!(storage.get(Bucket::Global, key), Some(value));
    }

    /// REQ: LIF-117
    #[test]
    fn put_rejects_a_value_one_byte_over_one_mib_and_keeps_the_previous_value() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("value_over", "kept");
        assert_eq!(storage.put(Bucket::Global, key.clone(), vec![3]), Ok(()));
        let result = storage.put(Bucket::Global, key.clone(), vec![0; MAX_VALUE_BYTES + 1]);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
        assert_eq!(storage.get(Bucket::Global, key), Some(vec![3]));
    }

    ///
    /// Fills one bucket to exactly its 16 MiB quota, then checks that the next write is refused
    /// with `quota-exceeded` (never `io`), and that replacing a value of the same size still fits
    /// because only the difference counts.
    /// REQ: LIF-119
    #[test]
    fn put_refuses_past_the_bucket_quota_with_quota_exceeded_and_counts_only_the_difference() {
        let (_root, mut storage) = open_storage();
        let quota_keys: Vec<String> = (0..=16).map(|index| format!("quota/{index:010}")).collect();
        assert!(quota_keys.iter().all(|key| key.len() == QUOTA_KEY_BYTES));
        clear_keys(&mut storage, Bucket::Session, &quota_keys);

        let full_value_len = MAX_VALUE_BYTES - QUOTA_KEY_BYTES;
        for key in &quota_keys[..16] {
            assert_eq!(
                storage.put(Bucket::Session, key.clone(), vec![1; full_value_len]),
                Ok(())
            );
        }
        assert_eq!(BUCKET_QUOTA_BYTES, 16 * MAX_VALUE_BYTES);

        let replacement = vec![2; full_value_len];
        assert_eq!(
            storage.put(Bucket::Session, quota_keys[0].clone(), replacement.clone()),
            Ok(())
        );
        assert_eq!(
            storage.get(Bucket::Session, quota_keys[0].clone()),
            Some(replacement)
        );

        let refused = storage.put(Bucket::Session, quota_keys[16].clone(), vec![1]);
        assert_eq!(error_code_of(refused), ErrorCode::QuotaExceeded);
        assert_eq!(storage.get(Bucket::Session, quota_keys[16].clone()), None);
    }

    /// REQ: CAP-011
    #[test]
    fn keys_returns_only_matching_prefix_in_byte_order() {
        let (_root, mut storage) = open_storage();
        let nonce = run_nonce();
        let prefix = format!("test/{nonce}/keys/");
        let outside = format!("test/{nonce}/keyz");
        let keys = [
            format!("{prefix}b"),
            format!("{prefix}B"),
            format!("{prefix}a"),
        ];
        for key in keys.iter().chain(std::iter::once(&outside)) {
            assert_eq!(storage.put(Bucket::Global, key.clone(), vec![1]), Ok(()));
        }
        let expected = vec![
            format!("{prefix}B"),
            format!("{prefix}a"),
            format!("{prefix}b"),
        ];
        assert_eq!(storage.keys(Bucket::Global, prefix), expected);
    }

    /// REQ: CAP-011
    #[test]
    fn keys_with_no_matching_prefix_returns_an_empty_list() {
        let (_root, mut storage) = open_storage();
        let prefix = format!("test/{}/nothing-here/", run_nonce());
        assert_eq!(storage.keys(Bucket::Global, prefix), Vec::<String>::new());
    }

    /// REQ: CAP-011
    #[test]
    fn keys_with_empty_prefix_lists_stored_keys_sorted_by_byte_order() {
        let (_root, mut storage) = open_storage();
        let first = unique_key("keys_all", "one");
        let second = unique_key("keys_all", "two");
        assert_eq!(storage.put(Bucket::Global, first.clone(), vec![1]), Ok(()));
        assert_eq!(storage.put(Bucket::Global, second.clone(), vec![2]), Ok(()));
        let listed = storage.keys(Bucket::Global, String::new());
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted);
        assert!(listed.contains(&first));
        assert!(listed.contains(&second));
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_applies_every_entry_including_a_removal() {
        let (_root, mut storage) = open_storage();
        let project_key = unique_key("batch_all", "a");
        let removed_key = unique_key("batch_all", "b");
        let added_key = unique_key("batch_all", "c");
        assert_eq!(
            storage.put(Bucket::Project, removed_key.clone(), vec![9]),
            Ok(())
        );
        let entries = vec![
            entry(&project_key, Some(vec![1])),
            entry(&removed_key, None),
            entry(&added_key, Some(vec![3])),
        ];
        assert_eq!(storage.write_batch(Bucket::Project, entries), Ok(()));
        assert_eq!(storage.get(Bucket::Project, project_key), Some(vec![1]));
        assert_eq!(storage.get(Bucket::Project, removed_key), None);
        assert_eq!(storage.get(Bucket::Project, added_key), Some(vec![3]));
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_with_no_entries_succeeds() {
        let (_root, mut storage) = open_storage();
        assert_eq!(storage.write_batch(Bucket::Project, Vec::new()), Ok(()));
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_with_a_bad_last_entry_applies_nothing_and_keeps_previous_values() {
        let (_root, mut storage) = open_storage();
        let first_key = unique_key("batch_atomic", "first");
        assert_eq!(
            storage.put(Bucket::Project, first_key.clone(), vec![5]),
            Ok(())
        );
        let entries = vec![entry(&first_key, Some(vec![6])), entry("", Some(vec![7]))];
        let result = storage.write_batch(Bucket::Project, entries);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
        assert_eq!(storage.get(Bucket::Project, first_key), Some(vec![5]));
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_of_exactly_256_entries_succeeds() {
        let (_root, mut storage) = open_storage();
        let nonce = run_nonce();
        let entries: Vec<Entry> = (0..MAX_BATCH_ENTRIES)
            .map(|index| entry(&format!("test/{nonce}/batch256/{index:03}"), Some(vec![1])))
            .collect();
        let last_key = format!("test/{nonce}/batch256/{:03}", MAX_BATCH_ENTRIES - 1);
        assert_eq!(storage.write_batch(Bucket::Project, entries), Ok(()));
        assert_eq!(storage.get(Bucket::Project, last_key), Some(vec![1]));
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_of_257_entries_is_invalid_argument_and_writes_nothing() {
        let (_root, mut storage) = open_storage();
        let nonce = run_nonce();
        let entries: Vec<Entry> = (0..=MAX_BATCH_ENTRIES)
            .map(|index| entry(&format!("test/{nonce}/batch257/{index:03}"), Some(vec![1])))
            .collect();
        let first_key = format!("test/{nonce}/batch257/000");
        let result = storage.write_batch(Bucket::Project, entries);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
        assert_eq!(storage.get(Bucket::Project, first_key), None);
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_of_exactly_4_mib_of_key_and_value_bytes_succeeds() {
        let (_root, mut storage) = open_storage();
        let keys: Vec<String> = (0..4).map(|index| format!("batch/4mib/{index}")).collect();
        clear_keys(&mut storage, Bucket::Project, &keys);
        let value_len = MAX_VALUE_BYTES - keys[0].len();
        let entries: Vec<Entry> = keys
            .iter()
            .map(|key| entry(key, Some(vec![2; value_len])))
            .collect();
        assert_eq!(entries.len() * (keys[0].len() + value_len), MAX_BATCH_BYTES);
        assert_eq!(storage.write_batch(Bucket::Project, entries), Ok(()));
        assert_eq!(
            storage.get(Bucket::Project, keys[3].clone()),
            Some(vec![2; value_len])
        );
    }

    /// REQ: LIF-117
    #[test]
    fn write_batch_over_4_mib_of_key_and_value_bytes_is_invalid_argument() {
        let (_root, mut storage) = open_storage();
        let keys: Vec<String> = (0..4).map(|index| format!("batch/over4/{index}")).collect();
        let entries: Vec<Entry> = keys
            .iter()
            .map(|key| entry(key, Some(vec![2; MAX_VALUE_BYTES])))
            .collect();
        let result = storage.write_batch(Bucket::Project, entries);
        assert_eq!(error_code_of(result), ErrorCode::InvalidArgument);
        assert_eq!(storage.get(Bucket::Project, keys[0].clone()), None);
    }

    /// REQ: LIF-120
    #[test]
    fn two_instances_share_one_store_and_the_last_writer_wins() {
        let root = tempfile::tempdir().expect("temp storage root");
        let mut first = open_storage_at(root.path());
        let mut second = open_storage_at(root.path());
        let key = unique_key("two_writers", "shared-key");
        assert_eq!(first.put(Bucket::Global, key.clone(), vec![1]), Ok(()));
        assert_eq!(second.get(Bucket::Global, key.clone()), Some(vec![1]));
        assert_eq!(second.put(Bucket::Global, key.clone(), vec![2]), Ok(()));
        assert_eq!(first.get(Bucket::Global, key), Some(vec![2]));
    }

    /// REQ: AUT-010
    #[test]
    fn lease_is_granted_to_one_caller_until_its_ttl_passes() {
        let root = tempfile::tempdir().expect("temp storage root");
        let mut first = open_storage_at(root.path());
        let mut second = open_storage_at(root.path());
        let key = unique_key("lease_one", "index");
        assert!(first.lease(key.clone(), 60_000));
        assert!(!second.lease(key, 60_000));
    }

    /// REQ: AUT-010
    #[test]
    fn lease_is_granted_again_after_its_ttl_passes() {
        let (_root, mut storage) = open_storage();
        let key = unique_key("lease_expiry", "index");
        assert!(storage.lease(key.clone(), 1));
        std::thread::sleep(Duration::from_millis(20));
        assert!(storage.lease(key, 1));
    }

    /// REQ: AUT-010
    #[test]
    fn leases_on_different_keys_are_independent() {
        let (_root, mut storage) = open_storage();
        let index_key = unique_key("lease_independent", "index");
        let thumbnail_key = unique_key("lease_independent", "thumbnail");
        assert!(storage.lease(index_key, 60_000));
        assert!(storage.lease(thumbnail_key, 60_000));
    }
}
