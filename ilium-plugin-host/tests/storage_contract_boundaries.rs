use ilium_plugin_api::{ids::PluginId, ErrorCode, Tier};
use ilium_plugin_host::storage::{
    PluginStorage, StorageContext, BUCKET_MAX_BYTES, VALUE_MAX_BYTES,
};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

fn open(
    root: &Path,
    plugin: &str,
    version: u32,
    project: &str,
    session: &str,
    quota: usize,
) -> PluginStorage {
    PluginStorage::open_with_context(
        root,
        PluginId(plugin.into()),
        StorageContext {
            project: Some(Path::new(project).to_path_buf()),
            session: Some(session.into()),
            data_version: version,
        },
        Tier::T2,
        quota,
        quota,
    )
    .unwrap()
}

#[test]
fn persisted_bytes_reopen_and_exact_keys_are_not_paths() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/project", "default", 1024);
    storage
        .put("global", "../../some/é", vec![0, 255, 10])
        .unwrap();
    storage.put("global", "empty", Vec::new()).unwrap();
    drop(storage);
    let storage = open(root.path(), "example.notes", 1, "/project", "default", 1024);
    assert_eq!(
        storage.get("global", "../../some/é").unwrap(),
        Some(vec![0, 255, 10])
    );
    assert_eq!(storage.get("global", "empty").unwrap(), Some(Vec::new()));
    assert_eq!(storage.keys("global", "..").unwrap(), vec!["../../some/é"]);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn explicit_context_keeps_plugin_project_and_session_ownership() {
    let root = tempfile::tempdir().unwrap();
    let mut first = open(root.path(), "example.notes", 1, "/a", "alpha", 1024);
    for bucket in ["global", "project", "session"] {
        first.put(bucket, "k", vec![1]).unwrap();
    }
    let other_session = open(root.path(), "example.notes", 1, "/a", "beta", 1024);
    assert_eq!(other_session.get("project", "k").unwrap(), Some(vec![1]));
    assert_eq!(other_session.get("session", "k").unwrap(), None);
    let other_project = open(root.path(), "example.notes", 1, "/b", "alpha", 1024);
    assert_eq!(other_project.get("global", "k").unwrap(), Some(vec![1]));
    assert_eq!(other_project.get("project", "k").unwrap(), None);
    let other_plugin = open(root.path(), "example.other", 1, "/a", "alpha", 1024);
    assert_eq!(other_plugin.get("global", "k").unwrap(), None);
    let no_context = PluginStorage::open(root.path(), PluginId("example.notes".into())).unwrap();
    assert_eq!(
        no_context.get("project", "k").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn quota_counts_key_and_value_and_replacement_only_once() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", 8);
    storage.put("global", "key", vec![1; 5]).unwrap();
    assert_eq!(
        storage.put("global", "key", vec![2; 6]).unwrap_err().code,
        ErrorCode::QuotaExceeded
    );
    assert_eq!(storage.get("global", "key").unwrap(), Some(vec![1; 5]));
    storage.put("global", "key", vec![3; 4]).unwrap();
    storage.put("global", "x", Vec::new()).unwrap();
    storage.remove("global", "missing").unwrap();
    assert_eq!(storage.keys("global", "").unwrap(), vec!["key", "x"]);
}

#[test]
fn invalid_keys_values_and_bucket_names_change_nothing() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", BUCKET_MAX_BYTES);
    for key in [String::new(), "é".repeat(129)] {
        assert_eq!(
            storage.put("global", &key, vec![1]).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    storage.put("global", &"é".repeat(128), Vec::new()).unwrap();
    assert_eq!(
        storage
            .put("global", "large", vec![0; VALUE_MAX_BYTES + 1])
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        storage.put("../outside", "k", vec![1]).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(storage.keys("global", "").unwrap().len(), 1);
}

#[test]
fn batch_quota_failure_is_all_or_nothing_and_removals_free_space() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", 8);
    storage.put("global", "a", vec![1; 3]).unwrap();
    assert_eq!(
        storage
            .write_batch(
                "global",
                &[("a".into(), Some(vec![2])), ("b".into(), Some(vec![0; 7]))]
            )
            .unwrap_err()
            .code,
        ErrorCode::QuotaExceeded
    );
    assert_eq!(storage.get("global", "a").unwrap(), Some(vec![1; 3]));
    assert_eq!(storage.get("global", "b").unwrap(), None);
    storage
        .write_batch(
            "global",
            &[("a".into(), None), ("b".into(), Some(vec![3; 7]))],
        )
        .unwrap();
    assert_eq!(storage.keys("global", "").unwrap(), vec!["b"]);
}

#[test]
fn batch_count_and_four_mib_byte_boundaries_are_exact() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", BUCKET_MAX_BYTES);
    let entries: Vec<_> = (0..256)
        .map(|index| (format!("k{index}"), Some(Vec::new())))
        .collect();
    storage.write_batch("global", &entries).unwrap();
    let mut excessive = entries.clone();
    excessive.push(("extra".into(), None));
    assert_eq!(
        storage.write_batch("global", &excessive).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    let exact: Vec<_> = (0..4)
        .map(|index| (format!("v{index}"), Some(vec![0; VALUE_MAX_BYTES - 2])))
        .collect();
    storage.write_batch("global", &exact).unwrap();
    let mut excess = exact.clone();
    excess[0].1.as_mut().unwrap().push(1);
    assert_eq!(
        storage.write_batch("global", &excess).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        storage.get("global", "v0").unwrap().unwrap().len(),
        VALUE_MAX_BYTES - 2
    );
    assert_eq!(
        storage
            .write_batch(
                "global",
                &[("oversized".into(), Some(vec![0; VALUE_MAX_BYTES + 1]))]
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn separate_handles_reload_committed_values_and_atomic_batches() {
    let root = tempfile::tempdir().unwrap();
    let first = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    let second = open(root.path(), "example.notes", 1, "/b", "s", 1024);
    std::thread::scope(|scope| {
        for (mut storage, byte) in [(first, 1), (second, 2)] {
            scope.spawn(move || {
                for _ in 0..10 {
                    storage
                        .write_batch(
                            "global",
                            &[
                                ("a".into(), Some(vec![byte])),
                                ("b".into(), Some(vec![byte])),
                            ],
                        )
                        .unwrap();
                }
            });
        }
    });
    let storage = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    assert_eq!(
        storage.get("global", "a").unwrap(),
        storage.get("global", "b").unwrap()
    );
}

#[test]
fn shared_cache_evicts_oldest_write_and_never_returns_size_quota_error() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", 4);
    storage.put("shared", "a", vec![1]).unwrap();
    storage.put("shared", "b", vec![2]).unwrap();
    storage.put("shared", "a", vec![3]).unwrap();
    storage.put("shared", "c", vec![4]).unwrap();
    assert_eq!(storage.keys("shared", "").unwrap(), vec!["a", "c"]);
    storage.put("shared", "large", vec![0; 8]).unwrap();
    assert!(storage.keys("shared", "").unwrap().is_empty());
}

#[test]
fn shared_refresh_lease_survives_reopen_and_expires_at_exact_ttl() {
    let root = tempfile::tempdir().unwrap();
    let first = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    assert!(first.lease_at("index", 1000, UNIX_EPOCH).unwrap());
    let second = open(root.path(), "example.notes", 1, "/b", "s", 1024);
    assert!(!second
        .lease_at("index", 1000, UNIX_EPOCH + Duration::from_millis(999))
        .unwrap());
    assert!(second
        .lease_at("index", 1000, UNIX_EPOCH + Duration::from_millis(1000))
        .unwrap());
}

#[test]
fn migration_callback_failure_preserves_bytes_and_success_rejects_old_writers() {
    let root = tempfile::tempdir().unwrap();
    let mut old = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    old.put("global", "old", vec![1]).unwrap();
    let mut new = open(root.path(), "example.notes", 2, "/a", "s", 1024);
    assert!(new
        .migrate_bucket_with("global", 2, |from, to, data| {
            assert_eq!((from, to), (1, 2));
            data.clear();
            Err(ilium_plugin_api::PluginError {
                code: ErrorCode::Internal,
                message: "guest failed".into(),
                retry_after_ms: None,
            })
        })
        .is_err());
    assert_eq!(old.get("global", "old").unwrap(), Some(vec![1]));
    new.migrate_bucket_with("global", 2, |_, _, data| {
        let value = data.remove("old").unwrap();
        data.insert("new".into(), value);
        Ok(())
    })
    .unwrap();
    let failure = old.remove("global", "new").unwrap_err();
    assert_eq!(failure.code, ErrorCode::Conflict);
    assert_eq!(failure.message, "stale-data-version");
    assert_eq!(new.keys("global", "").unwrap(), vec!["new"]);
    assert!(new.migrate(1).is_err());
}

#[test]
fn migration_is_never_fabricated_and_unstamped_data_is_not_migrated() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    assert_eq!(
        storage.migrate(2).unwrap_err().message,
        "migration-context-required"
    );
    assert!(storage
        .migrate_bucket_with(
            "global",
            2,
            |_, _, _: &mut BTreeMap<String, Vec<u8>>| panic!("must not migrate unstamped data")
        )
        .is_err());
}

#[test]
fn integrity_failure_requires_explicit_recovery_and_preserves_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    storage.put("global", "k", vec![1]).unwrap();
    let path = root.path().join("example.notes/global.db");
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&path, &bytes).unwrap();
    assert_eq!(
        storage.get("global", "k").unwrap_err().message,
        "storage-integrity"
    );
    let saved = storage
        .recover_corrupt_bucket("global", UNIX_EPOCH)
        .unwrap();
    assert_eq!(fs::read(saved).unwrap(), bytes);
    assert_eq!(storage.get("global", "k").unwrap(), None);
}

#[test]
fn invalid_identity_and_tier_quotas_are_refused_before_writes() {
    let root = tempfile::tempdir().unwrap();
    assert!(PluginStorage::open(root.path(), PluginId("../outside".into())).is_err());
    assert!(PluginStorage::open_with_context(
        root.path(),
        PluginId("example.notes".into()),
        StorageContext::default(),
        Tier::T2,
        BUCKET_MAX_BYTES + 1,
        1024
    )
    .is_err());
    assert!(fs::read_dir(root.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn private_files_and_symlink_refusals_preserve_external_targets() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    storage.put("global", "k", vec![1]).unwrap();
    let directory = root.path().join("example.notes");
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in fs::read_dir(&directory).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let external = root.path().join("external");
    fs::write(&external, b"preserve").unwrap();
    fs::remove_file(directory.join("global.db")).unwrap();
    symlink(&external, directory.join("global.db")).unwrap();
    assert!(storage.put("global", "k", vec![2]).is_err());
    assert_eq!(fs::read(&external).unwrap(), b"preserve");
    symlink(&directory, root.path().join("example.link")).unwrap();
    assert!(PluginStorage::open(root.path(), PluginId("example.link".into())).is_err());
}

#[test]
fn bucket_lock_serializes_real_process_read_modify_write() {
    const CHILD_ROOT: &str = "ILIUM_STORAGE_BOUNDARY_CHILD_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let mut storage = open(
            Path::new(&root),
            "example.notes",
            1,
            "/a",
            "child",
            16 * 1024,
        );
        for index in 0..32 {
            storage
                .put("global", &format!("child/{index}"), vec![1])
                .unwrap();
        }
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut storage = open(root.path(), "example.notes", 1, "/a", "parent", 16 * 1024);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("bucket_lock_serializes_real_process_read_modify_write")
        .env(CHILD_ROOT, root.path())
        .spawn()
        .unwrap();
    for index in 0..32 {
        storage
            .put("global", &format!("parent/{index}"), vec![2])
            .unwrap();
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(storage.keys("global", "child/").unwrap().len(), 32);
    assert_eq!(storage.keys("global", "parent/").unwrap().len(), 32);
}

#[test]
fn host_stamp_update_does_not_expose_metadata_as_keys_or_stamp_new_buckets() {
    use ilium_plugin_host::storage::write_bucket_stamp;
    let root = tempfile::tempdir().unwrap();
    let mut old = open(root.path(), "example.notes", 1, "/a", "s", 1024);
    old.put("global", "k", vec![1]).unwrap();
    old.put("project", "first", vec![1]).unwrap();
    write_bucket_stamp(root.path(), "example.notes", 2).unwrap();
    assert_eq!(old.keys("global", "").unwrap(), vec!["k"]);
    assert_eq!(
        old.put("global", "k", vec![2]).unwrap_err().message,
        "stale-data-version"
    );
    // A project not in the global activation keeps its lazy migration stamp.
    old.put("project", "first", vec![1]).unwrap();
    assert!(write_bucket_stamp(root.path(), "example.notes", 1).is_err());
    assert!(write_bucket_stamp(root.path(), "example.notes", 0).is_err());
}
