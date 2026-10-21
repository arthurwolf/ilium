//! Per-plugin key-value storage with quotas, journal and data-version migration
//! (reference-storage.md).

use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::PluginError;

/// One value is at most 1 MiB (reference-storage.md).
pub const VALUE_MAX_BYTES: usize = 1024 * 1024;

/// One bucket is at most 16 MiB (reference-storage.md).
pub const BUCKET_MAX_BYTES: usize = 16 * 1024 * 1024;

/// The storage of one plugin.
#[derive(Debug)]
pub struct PluginStorage;

impl PluginStorage {
    /// Opens the storage of a plugin under the data directory.
    pub fn open(_root: &std::path::Path, _plugin: PluginId) -> Result<PluginStorage, PluginError> {
        unimplemented!("host::storage::PluginStorage::open")
    }

    /// Reads a value.
    pub fn get(&self, _bucket: &str, _key: &str) -> Result<Option<Vec<u8>>, PluginError> {
        unimplemented!("host::storage::PluginStorage::get")
    }

    /// Writes a value; quota violations are `quota-exceeded`.
    pub fn put(&mut self, _bucket: &str, _key: &str, _value: Vec<u8>) -> Result<(), PluginError> {
        unimplemented!("host::storage::PluginStorage::put")
    }

    /// Runs the registered migrations up to the data version in the manifest.
    pub fn migrate(&mut self, _target_version: u32) -> Result<(), PluginError> {
        unimplemented!("host::storage::PluginStorage::migrate")
    }
}

/// Sets the data version a plugin's storage was last written with (reference-storage.md,
/// reference-install-and-update.md PKG-288).
pub fn write_bucket_stamp(
    _storage_root: &std::path::Path,
    _plugin: &str,
    _data_version: u64,
) -> std::io::Result<()> {
    unimplemented!("host::storage::write_bucket_stamp")
}

#[cfg(test)]
mod tests {
    use super::{write_bucket_stamp, PluginStorage, BUCKET_MAX_BYTES, VALUE_MAX_BYTES};
    use ilium_plugin_api::ids::PluginId;
    use ilium_plugin_api::{ErrorCode, PluginError};
    use std::path::Path;

    const PLUGIN_TEXT: &str = "example.storage";
    const KEY_MAX_BYTES: usize = 256;

    fn plugin_id() -> PluginId {
        PluginId(PLUGIN_TEXT.to_owned())
    }

    fn storage_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temporary storage root")
    }

    fn open_at(root: &Path) -> PluginStorage {
        PluginStorage::open(root, plugin_id()).expect("open plugin storage")
    }

    fn value_of_len(length: usize) -> Vec<u8> {
        vec![0xAB; length]
    }

    fn key_of_len(length: usize) -> String {
        "k".repeat(length)
    }

    /// Fills the named bucket with 15 values of one value-maximum each, keys `k00` to `k14`.
    /// That is 15 MiB plus 45 key bytes, so one more maximum-size value does not fit.
    fn fill_bucket_to_fifteen_mib(storage: &mut PluginStorage, bucket: &str) {
        for index in 0..15 {
            let key = format!("k{index:02}");
            storage
                .put(bucket, &key, value_of_len(VALUE_MAX_BYTES))
                .expect("write within the bucket quota");
        }
    }

    fn assert_refused_with(result: Result<(), PluginError>, expected: ErrorCode) {
        match result {
            Ok(()) => panic!("expected refusal {expected:?}, but the write was accepted"),
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
        }
    }

    /// REQ: API-061
    #[test]
    fn value_max_bytes_is_one_mib() {
        assert_eq!(VALUE_MAX_BYTES, 1_048_576);
    }

    /// REQ: API-061
    #[test]
    fn bucket_max_bytes_is_sixteen_mib() {
        assert_eq!(BUCKET_MAX_BYTES, 16_777_216);
    }

    /// REQ: LIF-102
    #[test]
    fn open_on_empty_root_gives_storage_with_no_values() {
        let root = storage_root();
        let storage = open_at(root.path());
        assert_eq!(storage.get("global", "anything").expect("get on new storage"), None);
    }

    /// REQ: API-002
    #[test]
    fn get_of_missing_key_returns_none_not_an_error() {
        let root = storage_root();
        let storage = open_at(root.path());
        let result = storage.get("global", "never-written");
        assert!(result.is_ok(), "a missing key is not an error: {result:?}");
        assert_eq!(result.expect("checked above"), None);
    }

    /// REQ: LIF-118
    #[test]
    fn put_value_survives_reopen_after_the_call_returns() {
        let root = storage_root();
        {
            let mut storage = open_at(root.path());
            storage
                .put("global", "cursor", b"line-42".to_vec())
                .expect("put on empty storage");
        }
        let reopened = open_at(root.path());
        assert_eq!(
            reopened.get("global", "cursor").expect("get after reopen"),
            Some(b"line-42".to_vec())
        );
    }

    /// REQ: API-061
    #[test]
    fn empty_key_is_invalid_argument() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        assert_refused_with(
            storage.put("global", "", b"value".to_vec()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn key_of_exactly_256_bytes_is_accepted() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        let key = key_of_len(KEY_MAX_BYTES);
        storage
            .put("global", &key, b"value".to_vec())
            .expect("a 256-byte key is within the limit");
        assert_eq!(
            storage.get("global", &key).expect("get the 256-byte key"),
            Some(b"value".to_vec())
        );
    }

    /// REQ: API-061
    #[test]
    fn key_of_257_bytes_is_invalid_argument() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        assert_refused_with(
            storage.put("global", &key_of_len(KEY_MAX_BYTES + 1), b"value".to_vec()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn key_limit_counts_utf8_bytes_not_characters() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        // "é" is two UTF-8 bytes: 128 of them are exactly 256 bytes, 129 are 258 bytes.
        let key_at_limit = "é".repeat(128);
        let key_over_limit = "é".repeat(129);
        storage
            .put("global", &key_at_limit, b"value".to_vec())
            .expect("128 two-byte characters are 256 bytes, which is allowed");
        assert_refused_with(
            storage.put("global", &key_over_limit, b"value".to_vec()),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn empty_value_is_allowed_and_reads_back_as_empty() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage
            .put("global", "flag", Vec::new())
            .expect("an empty value is allowed");
        assert_eq!(
            storage.get("global", "flag").expect("get the empty value"),
            Some(Vec::new())
        );
    }

    /// REQ: API-061
    #[test]
    fn value_of_exactly_one_mib_is_accepted() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage
            .put("global", "blob", value_of_len(VALUE_MAX_BYTES))
            .expect("a 1 MiB value is within the limit");
        let stored = storage
            .get("global", "blob")
            .expect("get the 1 MiB value")
            .expect("the value is present");
        assert_eq!(stored.len(), VALUE_MAX_BYTES);
    }

    /// REQ: API-061
    #[test]
    fn value_over_one_mib_is_invalid_argument_and_nothing_is_stored() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        assert_refused_with(
            storage.put("global", "blob", value_of_len(VALUE_MAX_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
        assert_eq!(storage.get("global", "blob").expect("get after refusal"), None);
    }

    /// REQ: LIF-118
    #[test]
    fn refused_put_keeps_the_previous_value() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage
            .put("global", "cursor", b"old".to_vec())
            .expect("first put");
        assert_refused_with(
            storage.put("global", "cursor", value_of_len(VALUE_MAX_BYTES + 1)),
            ErrorCode::InvalidArgument,
        );
        assert_eq!(
            storage.get("global", "cursor").expect("get after refusal"),
            Some(b"old".to_vec())
        );
    }

    /// REQ: API-061
    #[test]
    fn write_beyond_sixteen_mib_bucket_quota_is_quota_exceeded() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        fill_bucket_to_fifteen_mib(&mut storage, "global");
        assert_refused_with(
            storage.put("global", "k15", value_of_len(VALUE_MAX_BYTES)),
            ErrorCode::QuotaExceeded,
        );
    }

    /// REQ: API-061
    #[test]
    fn replacing_a_value_counts_only_the_size_difference() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        fill_bucket_to_fifteen_mib(&mut storage, "global");
        // Same-size replacement adds no bytes, so it fits even though the bucket is nearly full.
        storage
            .put("global", "k00", value_of_len(VALUE_MAX_BYTES))
            .expect("same-size replacement adds no bytes");
        // Shrinking k00 to an empty value frees about 1 MiB, so one more maximum-size value fits.
        storage
            .put("global", "k00", Vec::new())
            .expect("shrinking a value is allowed");
        storage
            .put("global", "k15", value_of_len(VALUE_MAX_BYTES))
            .expect("the freed bytes make room for one more maximum-size value");
    }

    /// REQ: API-061
    #[test]
    fn refused_quota_write_keeps_the_previous_value() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        fill_bucket_to_fifteen_mib(&mut storage, "global");
        let before = storage
            .get("global", "k00")
            .expect("get before the refused write");
        assert_refused_with(
            storage.put("global", "k15", value_of_len(VALUE_MAX_BYTES)),
            ErrorCode::QuotaExceeded,
        );
        assert_eq!(
            storage.get("global", "k15").expect("get the refused key"),
            None
        );
        assert_eq!(
            storage.get("global", "k00").expect("get the untouched key"),
            before
        );
    }

    /// REQ: API-061
    #[test]
    fn quota_is_counted_per_bucket() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        fill_bucket_to_fifteen_mib(&mut storage, "global");
        storage
            .put("project", "k15", value_of_len(VALUE_MAX_BYTES))
            .expect("a full global bucket does not block the project bucket");
    }

    /// REQ: API-002
    #[test]
    fn plugin_storages_do_not_share_values() {
        let root = storage_root();
        let mut first = PluginStorage::open(root.path(), PluginId("example.first".to_owned()))
            .expect("open first plugin");
        let second = PluginStorage::open(root.path(), PluginId("example.second".to_owned()))
            .expect("open second plugin");
        first
            .put("global", "shared-name", b"first".to_vec())
            .expect("write from the first plugin");
        assert_eq!(
            second.get("global", "shared-name").expect("read from the second plugin"),
            None
        );
    }

    /// REQ: LIF-106
    #[test]
    fn migrate_keeps_existing_values_and_is_idempotent() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage
            .put("global", "cursor", b"line-7".to_vec())
            .expect("put before migration");
        storage.migrate(2).expect("first migrate to version 2");
        storage
            .migrate(2)
            .expect("second migrate with the same target runs without changing data");
        assert_eq!(
            storage.get("global", "cursor").expect("get after migration"),
            Some(b"line-7".to_vec())
        );
    }

    /// REQ: LIF-121
    #[test]
    fn put_from_an_instance_older_than_the_stamp_is_stale_data_version() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage.migrate(2).expect("migrate instance to version 2");
        write_bucket_stamp(root.path(), PLUGIN_TEXT, 3).expect("stamp data version 3");
        let result = storage.put("global", "cursor", b"late".to_vec());
        match result {
            Ok(()) => panic!("a version-2 instance must not write over a version-3 stamp"),
            Err(error) => {
                assert_eq!(error.code, ErrorCode::Conflict);
                assert_eq!(error.message, "stale-data-version");
            }
        }
    }

    /// REQ: LIF-113
    #[test]
    fn stale_check_covers_project_and_session_buckets() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage.migrate(2).expect("migrate instance to version 2");
        write_bucket_stamp(root.path(), PLUGIN_TEXT, 3).expect("stamp data version 3");
        assert_refused_with(
            storage.put("project", "cursor", b"late".to_vec()),
            ErrorCode::Conflict,
        );
        assert_refused_with(
            storage.put("session", "cursor", b"late".to_vec()),
            ErrorCode::Conflict,
        );
    }

    /// REQ: LIF-113
    #[test]
    fn stamp_equal_to_instance_version_allows_writes() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage.migrate(2).expect("migrate instance to version 2");
        write_bucket_stamp(root.path(), PLUGIN_TEXT, 2).expect("stamp data version 2");
        storage
            .put("global", "cursor", b"current".to_vec())
            .expect("an instance matching the stamp may write");
    }

    /// REQ: LIF-113
    #[test]
    fn stamp_lower_than_instance_version_allows_writes() {
        let root = storage_root();
        let mut storage = open_at(root.path());
        storage.migrate(2).expect("migrate instance to version 2");
        write_bucket_stamp(root.path(), PLUGIN_TEXT, 1).expect("stamp data version 1");
        storage
            .put("global", "cursor", b"newer".to_vec())
            .expect("an instance newer than the stamp may write");
    }

    /// REQ: PKG-288
    #[test]
    fn write_bucket_stamp_accepts_zero_and_largest_version() {
        let root = storage_root();
        write_bucket_stamp(root.path(), PLUGIN_TEXT, 0).expect("stamp version 0");
        write_bucket_stamp(root.path(), PLUGIN_TEXT, u64::MAX).expect("stamp the largest version");
    }

    /// REQ: LIF-175
    #[test]
    fn corrupt_database_is_moved_aside_and_storage_starts_empty() {
        let root = storage_root();
        let database_path = root.path().join(format!("{PLUGIN_TEXT}.db"));
        std::fs::write(&database_path, b"this is not a storage database")
            .expect("write garbage over the database file");
        let storage = open_at(root.path());
        assert_eq!(storage.get("global", "anything").expect("get on empty storage"), None);
        let moved_aside = std::fs::read_dir(root.path())
            .expect("list the storage root")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".db.corrupt-"));
        assert!(moved_aside, "the corrupt file must be renamed to <id>.db.corrupt-<time>");
    }
}
