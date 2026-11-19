use ilium_plugin_api::{
    generated::host::storage::{Bucket, Entry, HostStorage},
    ids::PluginId,
    ErrorCode, PluginError, Side, Tier,
};
use ilium_plugin_host::{
    imports::storage::{
        StorageAuthorization, StorageImport, StorageImportContext, StorageInstanceState,
        StorageOperation,
    },
    storage::StorageContext,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

type Diagnostics = Arc<Mutex<Vec<(StorageOperation, PluginError)>>>;
fn context(
    plugin: &str,
    side: Side,
    project: &str,
    session: &str,
    quota: usize,
) -> StorageImportContext {
    StorageImportContext {
        plugin: PluginId(plugin.into()),
        side,
        scope: StorageContext {
            project: Some(project.into()),
            session: Some(session.into()),
            data_version: 1,
        },
        tier: Tier::T2,
        bucket_limit_bytes: quota,
        shared_limit_bytes: quota,
    }
}
fn adapter(
    root: &Path,
    context: StorageImportContext,
) -> (StorageImport, Arc<Mutex<StorageAuthorization>>, Diagnostics) {
    let gate = Arc::new(Mutex::new(StorageAuthorization::new(
        context.plugin.clone(),
        context.side,
        StorageInstanceState::Active,
    )));
    let diagnostics: Diagnostics = Arc::default();
    let sink = diagnostics.clone();
    let gate_at_report = gate.clone();
    let import = StorageImport::open(root, context, gate.clone(), move |operation, error| {
        // Reporting happens after the operation drops its authorization guard.
        assert!(gate_at_report.try_lock().is_ok());
        sink.lock().unwrap().push((operation, error.clone()));
    })
    .unwrap();
    (import, gate, diagnostics)
}

#[test]
fn generated_storage_trait_uses_real_persistent_values_and_atomic_batches() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context("example.notes", Side::Server, "/project", "default", 1024);
    let (mut storage, _, diagnostics) = adapter(root.path(), ctx.clone());
    storage.put(Bucket::Global, "a".into(), vec![1, 2]).unwrap();
    storage
        .write_batch(
            Bucket::Global,
            vec![
                Entry {
                    key: "a".into(),
                    value: None,
                },
                Entry {
                    key: "b".into(),
                    value: Some(vec![255, 0]),
                },
            ],
        )
        .unwrap();
    drop(storage);
    let (mut reopened, _, _) = adapter(root.path(), ctx);
    assert_eq!(reopened.get(Bucket::Global, "a".into()).expect("storage read succeeds"), None);
    assert_eq!(reopened.get(Bucket::Global, "b".into()).expect("storage read succeeds"), Some(vec![255, 0]));
    assert_eq!(reopened.keys(Bucket::Global, "".into()).expect("storage read succeeds"), vec!["b"]);
    let _ = reopened.remove(Bucket::Global, "b".into());
    assert_eq!(reopened.get(Bucket::Global, "b".into()).expect("storage read succeeds"), None);
    assert!(diagnostics.lock().unwrap().is_empty());
}

#[test]
fn conditional_storage_update_has_one_winner_across_instances() {
    use std::sync::Barrier;
    let root = tempfile::tempdir().unwrap();
    let context = context("example.notes", Side::Server, "/project", "default", 1024);
    let (first, _, _) = adapter(root.path(), context.clone());
    let (second, _, _) = adapter(root.path(), context);
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [first, second].into_iter().enumerate().map(|(index, mut storage)| {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            storage.compare_exchange(Bucket::Shared, "reservation".into(), None, Some(vec![index as u8])).unwrap()
        })
    }).collect();
    let winners = threads.into_iter().map(|thread| thread.join().unwrap()).filter(|won| *won).count();
    assert_eq!(winners, 1);
}

#[test]
fn conditional_storage_mismatch_and_quota_failure_preserve_the_value() {
    let root = tempfile::tempdir().unwrap();
    let (mut storage, _, _) = adapter(root.path(), context("example.notes", Side::Server, "/project", "default", 16));
    storage.put(Bucket::Global, "key".into(), b"old".to_vec()).unwrap();
    assert!(!storage.compare_exchange(Bucket::Global, "key".into(), Some(b"wrong".to_vec()), Some(b"new".to_vec())).unwrap());
    assert_eq!(storage.compare_exchange(Bucket::Global, "key".into(), Some(b"old".to_vec()), Some(vec![0; 16])).unwrap_err().code, ErrorCode::QuotaExceeded);
    assert_eq!(storage.get(Bucket::Global, "key".into()).unwrap(), Some(b"old".to_vec()));
    assert!(storage.compare_exchange(Bucket::Global, "key".into(), Some(b"old".to_vec()), None).unwrap());
    assert_eq!(storage.get(Bucket::Global, "key".into()).unwrap(), None);
}

#[test]
fn server_client_and_session_contexts_share_only_the_documented_buckets() {
    let root = tempfile::tempdir().unwrap();
    let (mut server, _, _) = adapter(
        root.path(),
        context("example.notes", Side::Server, "/a", "one", 1024),
    );
    for bucket in [
        Bucket::Global,
        Bucket::Project,
        Bucket::Session,
        Bucket::Shared,
    ] {
        server.put(bucket, "k".into(), vec![1]).unwrap();
    }
    let (mut client, _, _) = adapter(
        root.path(),
        context("example.notes", Side::Client, "/a", "two", 1024),
    );
    assert_eq!(client.get(Bucket::Global, "k".into()).expect("storage read succeeds"), Some(vec![1]));
    assert_eq!(client.get(Bucket::Project, "k".into()).expect("storage read succeeds"), Some(vec![1]));
    assert_eq!(client.get(Bucket::Session, "k".into()).expect("storage read succeeds"), None);
    assert_eq!(client.get(Bucket::Shared, "k".into()).expect("storage read succeeds"), Some(vec![1]));
    let (mut other, _, _) = adapter(
        root.path(),
        context("example.other", Side::Server, "/a", "one", 1024),
    );
    assert_eq!(other.get(Bucket::Shared, "k".into()).expect("storage read succeeds"), None);
    let (mut project, _, _) = adapter(
        root.path(),
        context("example.notes", Side::Server, "/b", "one", 1024),
    );
    assert_eq!(project.get(Bucket::Project, "k".into()).expect("storage read succeeds"), None);
}

#[test]
fn revoked_instance_returns_or_reports_every_refusal_and_preserves_data() {
    let root = tempfile::tempdir().unwrap();
    let (mut storage, gate, diagnostics) = adapter(
        root.path(),
        context("example.notes", Side::Server, "/a", "s", 1024),
    );
    storage.put(Bucket::Global, "k".into(), vec![1]).unwrap();
    gate.lock()
        .unwrap()
        .set_state(StorageInstanceState::Revoked);
    assert_eq!(
        storage
            .put(Bucket::Global, "k".into(), vec![2])
            .unwrap_err()
            .code,
        ErrorCode::Revoked
    );
    assert_eq!(
        storage
            .write_batch(
                Bucket::Global,
                vec![Entry {
                    key: "k".into(),
                    value: None
                }]
            )
            .unwrap_err()
            .code,
        ErrorCode::Revoked
    );
    assert_eq!(storage.get(Bucket::Global, "k".into()).unwrap_err().code, ErrorCode::Revoked);
    assert_eq!(storage.keys(Bucket::Global, "".into()).unwrap_err().code, ErrorCode::Revoked);
    let _ = storage.remove(Bucket::Global, "k".into());
    assert!(!storage.lease("index".into(), 1000));
    assert_eq!(diagnostics.lock().unwrap().len(), 6);
    assert!(diagnostics
        .lock()
        .unwrap()
        .iter()
        .all(|(_, error)| error.code == ErrorCode::Revoked));
    gate.lock().unwrap().set_state(StorageInstanceState::Active);
    assert_eq!(storage.try_get(Bucket::Global, "k").unwrap(), Some(vec![1]));
}

#[test]
fn constructor_refuses_wrong_plugin_side_and_inactive_instance_before_io() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context("example.notes", Side::Server, "/a", "s", 1024);
    for (plugin, side, state) in [
        ("example.other", Side::Server, StorageInstanceState::Active),
        ("example.notes", Side::Client, StorageInstanceState::Active),
        (
            "example.notes",
            Side::Server,
            StorageInstanceState::Inactive,
        ),
    ] {
        let gate = Arc::new(Mutex::new(StorageAuthorization::new(
            PluginId(plugin.into()),
            side,
            state,
        )));
        assert_eq!(
            StorageImport::open(root.path(), ctx.clone(), gate, |_, _| {})
                .unwrap_err()
                .code,
            ErrorCode::NotPermitted
        );
    }
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn missing_scope_and_invalid_keys_are_reported_without_fabricating_values() {
    let root = tempfile::tempdir().unwrap();
    let mut ctx = context("example.notes", Side::Server, "/a", "s", 1024);
    ctx.scope.project = None;
    let (mut storage, _, diagnostics) = adapter(root.path(), ctx);
    assert_eq!(
        storage.try_get(Bucket::Project, "k").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(storage.get(Bucket::Project, "k".into()).unwrap_err().code, ErrorCode::InvalidArgument);
    let _ = storage.remove(Bucket::Global, "".into());
    assert_eq!(diagnostics.lock().unwrap().len(), 2);
    assert_eq!(
        storage.try_keys(Bucket::Global, "").unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn invalid_and_over_quota_batches_cannot_mutate_existing_values() {
    let root = tempfile::tempdir().unwrap();
    let (mut storage, _, diagnostics) = adapter(
        root.path(),
        context("example.notes", Side::Server, "/a", "s", 8),
    );
    storage.put(Bucket::Global, "a".into(), vec![1]).unwrap();
    let batch = |key: &str, value: Vec<u8>| {
        vec![
            Entry {
                key: "a".into(),
                value: None,
            },
            Entry {
                key: key.into(),
                value: Some(value),
            },
        ]
    };
    assert_eq!(
        storage
            .write_batch(Bucket::Global, batch("", vec![2]))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        storage
            .write_batch(Bucket::Global, batch("large", vec![2; 8]))
            .unwrap_err()
            .code,
        ErrorCode::QuotaExceeded
    );
    assert_eq!(storage.get(Bucket::Global, "a".into()).expect("storage read succeeds"), Some(vec![1]));
    assert_eq!(diagnostics.lock().unwrap().len(), 2);
}

#[test]
fn lease_ttl_is_strict_and_other_real_handles_observe_the_winner() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context("example.notes", Side::Server, "/a", "s", 1024);
    let (mut first, _, diagnostics) = adapter(root.path(), ctx.clone());
    let (mut second, _, _) = adapter(root.path(), ctx);
    assert!(!first.lease("index".into(), 0));
    assert!(!first.lease("index".into(), 3_600_001));
    assert_eq!(diagnostics.lock().unwrap().len(), 2);
    assert!(first.lease("index".into(), 3_600_000));
    assert!(!second.lease("index".into(), 3_600_000));
    assert_eq!(
        diagnostics.lock().unwrap().len(),
        2,
        "losing an admitted lease is not a refusal"
    );
}

#[test]
fn stale_void_remove_is_diagnosed_and_keeps_the_committed_value() {
    let root = tempfile::tempdir().unwrap();
    let (mut storage, _, diagnostics) = adapter(
        root.path(),
        context("example.notes", Side::Server, "/a", "s", 1024),
    );
    storage.put(Bucket::Global, "k".into(), vec![1]).unwrap();
    ilium_plugin_host::storage::write_bucket_stamp(root.path(), "example.notes", 2).unwrap();
    let _ = storage.remove(Bucket::Global, "k".into());
    assert_eq!(storage.try_get(Bucket::Global, "k").unwrap(), Some(vec![1]));
    let events = diagnostics.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, StorageOperation::Remove);
    assert_eq!(events[0].1.code, ErrorCode::Conflict);
    assert_eq!(events[0].1.message, "stale-data-version");
}
