//! Synthetic, owner-isolated repository fixtures; no user saves are discovered.
use super::*;
use std::cell::Cell;

fn fixture() -> (tempfile::TempDir, Repository, PathBuf, Vec<PathBuf>) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("saves");
    std::fs::create_dir(&root).unwrap();
    let root = paths::canonicalize(&root).unwrap();
    let paths = ["same metadata", "雪 "]
        .map(|name| {
            let path = root.join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("level.dat"), b"identical synthetic metadata").unwrap();
            paths::canonicalize(&path).unwrap()
        })
        .to_vec();
    let store = Repository::new(temp.path().join("private-history")).unwrap();
    (temp, store, root, paths)
}

#[test]
fn missing_state_is_default_and_reload_keeps_identifiers_and_revision() {
    let (_temp, store, root, paths) = fixture();
    let empty = store.load(&|| false).unwrap();
    assert_eq!(empty.revision(), 0);
    assert_eq!(empty.history(), History::default());
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    assert_eq!(bound.snapshot.revision(), 1);
    assert_ne!(bound.maps[0].map, bound.maps[1].map);
    let reopened = Repository::new(store.directory.clone()).unwrap();
    assert_eq!(reopened.load(&|| false).unwrap(), bound.snapshot);
    let mut reversed = paths.clone();
    reversed.reverse();
    let again = reopened
        .bind_catalog(1, &root, &reversed, &|| false)
        .unwrap();
    assert_eq!(again.maps[0].map, bound.maps[1].map);
    assert_eq!(again.maps[1].map, bound.maps[0].map);
    assert_eq!(
        again.snapshot.revision(),
        1,
        "unchanged catalog needs no write"
    );
}

#[test]
fn replacement_and_observed_absence_retire_instead_of_relabeling_old_rows() {
    let (_temp, store, root, paths) = fixture();
    let first = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    std::fs::rename(
        &paths[0],
        root.parent().unwrap().join("old retained directory"),
    )
    .unwrap();
    std::fs::create_dir(&paths[0]).unwrap();
    let second = store.bind_catalog(1, &root, &paths, &|| false).unwrap();
    assert_ne!(first.maps[0].map, second.maps[0].map);
    assert_eq!(first.maps[1].map, second.maps[1].map);
    assert_eq!(second.snapshot.bindings().len(), 3);
    let departed: Vec<_> = (0..paths.len())
        .map(|index| root.parent().unwrap().join(format!("departed-{index}")))
        .collect();
    for (path, retained) in paths.iter().zip(&departed) {
        std::fs::rename(path, retained).unwrap();
    }
    let absent = store.bind_catalog(2, &root, &[], &|| false).unwrap();
    for (path, retained) in paths.iter().zip(&departed) {
        std::fs::rename(retained, path).unwrap();
    }
    let back = store.bind_catalog(3, &root, &paths, &|| false).unwrap();
    assert_ne!(back.maps[1].map, second.maps[1].map);
    assert!(absent
        .snapshot
        .bindings()
        .iter()
        .all(|binding| !binding.active));
    assert_eq!(back.snapshot.bindings().len(), 5);
}

#[test]
fn stale_history_commit_preserves_newer_bindings_and_returns_current_snapshot() {
    let (_temp, store, root, paths) = fixture();
    let initial = store.load(&|| false).unwrap();
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    match store.commit_history(initial.revision(), History::default(), &|| false) {
        Err(Error::Conflict { current }) => assert_eq!(*current, bound.snapshot),
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!(store.load(&|| false).unwrap(), bound.snapshot);
    let saved = store
        .commit_history(1, History::default(), &|| false)
        .unwrap();
    assert_eq!(saved.revision(), 2);
}

#[test]
fn complete_batch_rejects_duplicates_outside_paths_and_limits_without_publication() {
    let (temp, store, root, paths) = fixture();
    assert!(matches!(
        store.bind_catalog(0, &root, &[paths[0].clone(), paths[0].clone()], &|| false),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        store.bind_catalog(0, &root, &[temp.path().to_owned()], &|| false),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        store.bind_catalog(
            0,
            &root,
            &vec![paths[0].clone(); MAX_ADMISSION + 1],
            &|| false
        ),
        Err(Error::Limit(_))
    ));
    assert_eq!(store.load(&|| false).unwrap().revision(), 0);
    assert!(!store.directory.join(STATE_FILE).exists());
}

#[test]
fn cancellation_during_admission_or_before_publish_never_leaves_partial_state() {
    let (_temp, store, root, paths) = fixture();
    let checks = Cell::new(0);
    let cancel = || {
        let n = checks.get();
        checks.set(n + 1);
        n >= 3
    };
    assert!(matches!(
        store.bind_catalog(0, &root, &paths, &cancel),
        Err(Error::Cancelled)
    ));
    assert_eq!(store.load(&|| false).unwrap().revision(), 0);
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    assert!(matches!(
        store.commit_history(1, History::default(), &|| true),
        Err(Error::Cancelled)
    ));
    assert_eq!(store.load(&|| false).unwrap(), bound.snapshot);
}

#[test]
fn corrupt_unknown_schema_oversized_and_non_regular_state_never_reset() {
    let (_temp, store, _root, _paths) = fixture();
    store.load(&|| false).unwrap();
    let state = store.directory.join(STATE_FILE);
    for bytes in [
        b"broken".to_vec(),
        b"{\"schema\":999}".to_vec(),
        vec![b' '; MAX_STATE_BYTES + 1],
    ] {
        std::fs::write(&state, &bytes).unwrap();
        assert!(store.load(&|| false).is_err());
        assert_eq!(std::fs::read(&state).unwrap(), bytes);
    }
    std::fs::remove_file(&state).unwrap();
    std::fs::create_dir(&state).unwrap();
    assert!(store.load(&|| false).is_err());
}

#[test]
fn held_lock_is_busy_without_waiting_and_release_recovers() {
    let (_temp, store, _root, _paths) = fixture();
    store.load(&|| false).unwrap();
    let held = ExclusiveFileLock::acquire(&store.directory.join(LOCK_FILE)).unwrap();
    assert!(matches!(store.load(&|| false), Err(Error::Busy)));
    drop(held);
    assert!(store.load(&|| false).is_ok());
}

#[test]
fn invalid_history_is_rejected_by_actual_controller_contract() {
    let (_temp, store, _root, _paths) = fixture();
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = serde_json::json!(u64::MAX);
    let history = serde_json::from_value(value).unwrap();
    assert!(matches!(
        store.commit_history(0, history, &|| false),
        Err(Error::Invalid(_))
    ));
    assert_eq!(store.load(&|| false).unwrap().revision(), 0);
}

#[test]
fn failed_publication_preserves_previous_authoritative_bytes() {
    let (_temp, store, root, paths) = fixture();
    let initial = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let state = store.directory.join(STATE_FILE);
    let before = std::fs::read(&state).unwrap();
    // Inject failure at the write boundary without depending on root/ACL privileges.
    assert!(store
        .commit_history_with(1, History::default(), &|| false, &|_| Err(
            std::io::Error::other("synthetic write failure")
        ))
        .is_err());
    assert_eq!(std::fs::read(&state).unwrap(), before);
    assert_eq!(store.load(&|| false).unwrap(), initial.snapshot);
}

#[test]
fn validation_before_loading_withholds_replaced_or_disappeared_directory() {
    let (_temp, store, root, paths) = fixture();
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    bound.maps[0].verify(&root).unwrap();
    std::fs::rename(&paths[0], root.join("moved")).unwrap();
    assert!(bound.maps[0].verify(&root).is_err());
    std::fs::create_dir(&paths[0]).unwrap();
    assert!(bound.maps[0].verify(&root).is_err());
}

#[test]
fn relative_storage_directory_is_rejected() {
    assert!(matches!(
        Repository::new(PathBuf::from("relative")),
        Err(Error::Invalid(_))
    ));
}

fn shown_history(map: MapId) -> History {
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = serde_json::json!(1);
    value["revision"] = serde_json::json!(2);
    value["seen"][0] = serde_json::json!({
        "key": {"map":map,"revision":super::super::evidence::RULE_REVISION,
            "category":"OpenGrassland","tile":[0,0],"anchor":[1,64,1]},
        "source":{"map":map,"generation":7}, "confidence":"Corroborated", "run":1
    });
    value["routes"][0] = serde_json::json!({
        "route":{"map":map,"endpoints":[[0,0],[48,48]]}, "run":1
    });
    serde_json::from_value(value).unwrap()
}

#[test]
fn actual_appearance_and_route_history_survive_reload_and_catalog_rebuild() {
    let (_temp, store, root, paths) = fixture();
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let history = shown_history(bound.maps[0].map);
    let saved = store.commit_history(1, history, &|| false).unwrap();
    assert_eq!(saved.history().appearances().count(), 1);
    assert_eq!(saved.history().traversals().count(), 1);
    assert_eq!(store.load(&|| false).unwrap().history(), history);
    let again = store.bind_catalog(2, &root, &paths, &|| false).unwrap();
    assert_eq!(again.snapshot, saved);
    assert_eq!(again.maps[0].map, bound.maps[0].map);
    assert!(matches!(
        store.commit_history(2, shown_history(MapId([231; 16])), &|| false),
        Err(Error::Invalid(_))
    ));
    assert_eq!(store.load(&|| false).unwrap(), saved);
    std::fs::rename(&paths[0], root.parent().unwrap().join("historical-world")).unwrap();
    std::fs::create_dir(&paths[0]).unwrap();
    let replaced = store.bind_catalog(2, &root, &paths, &|| false).unwrap();
    assert_ne!(replaced.maps[0].map, bound.maps[0].map);
    assert_eq!(replaced.snapshot.history(), history);
    assert!(replaced
        .snapshot
        .bindings()
        .iter()
        .any(|row| row.map == bound.maps[0].map && !row.active));
    assert_eq!(store.load(&|| false).unwrap(), replaced.snapshot);
}

#[test]
fn cancellation_at_last_pre_rename_checkpoint_cleans_exact_owned_temporary() {
    let (_temp, store, root, paths) = fixture();
    let initial = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let should_cancel = Cell::new(false);
    let failure =
        store.commit_history_with(1, History::default(), &|| should_cancel.get(), &|file| {
            // This checkpoint fires after an actual temporary handle was opened.
            file.write_all(b"")?;
            should_cancel.set(true);
            Ok(())
        });
    assert!(matches!(failure, Err(Error::Cancelled)));
    assert_eq!(store.load(&|| false).unwrap(), initial.snapshot);
    let names: BTreeSet<_> = std::fs::read_dir(&store.directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [STATE_FILE.into(), LOCK_FILE.into()].into());
    assert_eq!(
        std::fs::read_dir(&paths[0]).unwrap().count(),
        1,
        "no marker written into save"
    );
}

#[test]
fn retained_binding_cap_and_store_revision_overflow_fail_whole_transaction() {
    let (_temp, store, root, paths) = fixture();
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let mut full = bound.snapshot;
    let template = full.bindings[0].clone();
    full.bindings = (0..MAX_BINDINGS)
        .map(|index| {
            let mut row = template.clone();
            row.map = MapId((index as u128 + 1).to_le_bytes());
            row.root_key = minecraft::native_path_key(Path::new("/")).unwrap();
            row.path_key = minecraft::native_path_key(Path::new("/a")).unwrap();
            row.active = false;
            row
        })
        .collect();
    let bytes = serde_json::to_vec(&full).unwrap();
    assert!(bytes.len() <= MAX_STATE_BYTES);
    std::fs::write(store.directory.join(STATE_FILE), &bytes).unwrap();
    assert!(matches!(
        store.bind_catalog(1, &root, &paths, &|| false),
        Err(Error::Limit("retained bindings"))
    ));
    assert_eq!(
        std::fs::read(store.directory.join(STATE_FILE)).unwrap(),
        bytes
    );
    full.revision = u64::MAX;
    std::fs::write(
        store.directory.join(STATE_FILE),
        serde_json::to_vec(&full).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        store.commit_history(u64::MAX, History::default(), &|| false),
        Err(Error::Limit("store revision"))
    ));
    assert_eq!(store.load(&|| false).unwrap().revision(), u64::MAX);
}

#[test]
fn unknown_nested_history_fields_are_not_silently_discarded() {
    let (_temp, store, root, paths) = fixture();
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let mut value = serde_json::to_value(bound.snapshot).unwrap();
    value["history"]["future_authored_data"] = serde_json::json!([1, 2, 3]);
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(store.directory.join(STATE_FILE), &bytes).unwrap();
    assert!(matches!(
        store.load(&|| false),
        Err(Error::Invalid("unrecognized persisted fields"))
    ));
    assert_eq!(
        std::fs::read(store.directory.join(STATE_FILE)).unwrap(),
        bytes
    );
}

#[test]
fn serialization_cap_refuses_before_any_temporary_or_authority_write() {
    let (_temp, store, root, paths) = fixture();
    let bound = store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let previous = std::fs::read(store.directory.join(STATE_FILE)).unwrap();
    let mut enlarged = bound.snapshot;
    let mut row = enlarged.bindings[0].clone();
    row.active = false;
    row.path_key = vec![42; MAX_PATH_BYTES];
    enlarged.bindings = (0..128)
        .map(|index| {
            let mut item = row.clone();
            item.map = MapId((index as u128 + 1).to_le_bytes());
            item
        })
        .collect();
    let transaction = store.transaction(&|| false).unwrap();
    assert!(matches!(
        transaction.publish(&enlarged, &|| false, &|_| Ok(())),
        Err(Error::Limit("serialized state bytes"))
    ));
    assert_eq!(
        std::fs::read(store.directory.join(STATE_FILE)).unwrap(),
        previous
    );
    assert_eq!(std::fs::read_dir(&store.directory).unwrap().count(), 2);
}

#[test]
fn two_workers_using_same_revision_cannot_overwrite_each_other() {
    let (_temp, store, root, paths) = fixture();
    store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_history(1, History::default(), &|| false)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results
        .iter()
        .any(|result| matches!(result, Err(Error::Busy | Error::Conflict { .. }))));
    let saved = store.load(&|| false).unwrap();
    assert_eq!(saved.revision(), 2);
    assert_eq!(saved.bindings().len(), 2);
}

#[test]
fn cancellation_arriving_after_last_publish_checkpoint_is_reported_as_committed() {
    let (_temp, store, root, paths) = fixture();
    store.bind_catalog(0, &root, &paths, &|| false).unwrap();
    let requested = Cell::new(false);
    let cancelled = || {
        if requested.get() {
            return true;
        }
        // A fully written owned temporary identifies the final checkpoint:
        // cancellation arriving immediately afterward cannot undo rename.
        let written_temp = std::fs::read_dir(&store.directory).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry.file_name().to_string_lossy().ends_with(".tmp")
                && entry.metadata().unwrap().len() > 0
        });
        if written_temp {
            requested.set(true);
        }
        false
    };
    let committed = store
        .commit_history(1, History::default(), &cancelled)
        .unwrap();
    assert!(requested.get());
    assert_eq!(committed.revision(), 2);
    assert_eq!(store.load(&|| false).unwrap(), committed);
}
