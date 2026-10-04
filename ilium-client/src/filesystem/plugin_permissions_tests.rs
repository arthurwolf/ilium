//! Native task-local fixture IO only; no real user config or capture.
#![cfg(target_os = "linux")]
use super::*;
use ilium_animation_js::permissions::{Ceiling, PermissionBroker};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
};
use ilium_platform::secure_fs::NoFollowDirectory;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
struct Fixture {
    directory: tempfile::TempDir,
    root: Arc<PinnedDirectory>,
    client: Client,
    execution: Execution,
    quota: QuotaGroup,
    _root_storage: StorageAdmission,
}
impl Fixture {
    fn new() -> Self {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 8,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 128 * 1024 * 1024,
            result_bytes: 128 * 1024 * 1024,
            worker_threads: 4,
            worker_bytes: 256 * 1024 * 1024,
        });
        let root_storage = quota.reserve_external_storage(64 * 1024).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(directory.path()).unwrap(),
            ))
            .unwrap(),
        );
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: 64 * 1024,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane(1),
                io: lane(1),
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 128 * 1024 * 1024,
                result_bytes: 128 * 1024 * 1024,
            })
            .unwrap();
        Self {
            directory,
            root,
            client,
            execution,
            quota,
            _root_storage: root_storage,
        }
    }
    fn owner(&self) -> PluginPermissionFiles {
        PluginPermissionFiles::new(
            &self.client,
            Arc::clone(&self.root),
            Arc::new(Notify::new()),
            Arc::new(|| {}),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.execution.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            self.execution
                .join_until_background(Instant::now() + Duration::from_secs(3))
                .unwrap()
                .remaining_workers,
            0
        );
    }
}
fn native_identity(bytes: &[u8]) -> PackageIdentity {
    PackageIdentity::unverified("fixture".into(), bytes).unwrap()
}
fn stamp(revision: u64) -> PersistenceStamp {
    PersistenceStamp {
        selection_revision: revision,
        instance_id: revision,
        plan_revision: 1,
        authorization_epoch: 1,
    }
}
fn exported(identity: &PackageIdentity) -> Vec<u8> {
    PermissionBroker::new(
        identity.clone(),
        Ceiling {
            permissions: vec![],
        },
        Ceiling {
            permissions: vec![],
        },
    )
    .unwrap()
    .export_remembered()
    .unwrap()
}
async fn completion(owner: &mut PluginPermissionFiles) -> PermissionCompletion {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = owner.collect() {
            return result;
        }
        let notification = owner.notification();
        tokio::time::timeout_at(deadline, notification.notified())
            .await
            .expect("native persistence completion notification");
    }
}
async fn loaded(
    owner: &mut PluginPermissionFiles,
    fence: &Arc<PermissionFence>,
) -> Retained<Result<LedgerSnapshot>> {
    owner.request_load(fence).unwrap();
    let PermissionCompletion::Loaded(result) = completion(owner).await else {
        panic!("load completion missing")
    };
    result
}
fn make_namespace(fixture: &Fixture, fence: &PermissionFence) -> Arc<PinnedDirectory> {
    Arc::new(fixture.root.child(&fence.namespace, true).unwrap())
}
fn write_fixture(directory: &Arc<PinnedDirectory>, bytes: &[u8]) {
    let mut write = directory
        .begin_atomic(LEAF, WriteMode::ReplaceEntry)
        .unwrap();
    write.write(bytes, STATE_BYTES + 1).unwrap();
    write.prepare_durable().unwrap();
    write.publish_entry().unwrap();
    write.durable_ack().unwrap();
}
#[tokio::test]
async fn native_export_round_trip_requires_real_durable_receipt_and_reopen() {
    let fixture = Fixture::new();
    let identity = native_identity(b"exact immutable fixture package");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let initial = loaded(&mut owner, &fence).await;
    let snapshot = initial.view().as_ref().unwrap();
    assert!(snapshot.bytes().is_none());
    let bytes = exported(&identity);
    let pending_write = owner.request_commit(snapshot, &bytes).unwrap();
    assert!(owner.is_pending());
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("durable completion missing")
    };
    let receipt = result.view().as_ref().unwrap();
    assert_eq!(receipt.write_id(), pending_write);
    assert_eq!(receipt.digest(), sha(&bytes));
    assert_eq!(receipt.epoch(), 1);
    assert_eq!(receipt.stamp(), stamp(1));
    assert!(receipt.is_current());
    drop(result);
    drop(initial);
    owner.close_admission();
    drop(owner);
    let mut restarted = fixture.owner();
    let restarted_fence = restarted.bind(&identity, stamp(2)).unwrap();
    let reopened = loaded(&mut restarted, &restarted_fence).await;
    assert_eq!(
        reopened.view().as_ref().unwrap().bytes(),
        Some(bytes.as_slice())
    );
    assert_eq!(reopened.view().as_ref().unwrap().stamp(), stamp(2));
    // These opaque IO bytes still require consuming native broker restore.
}
#[tokio::test]
async fn empty_corrupt_oversize_and_wrong_principal_are_errors_not_first_use() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let directory = make_namespace(&fixture, &fence);
    let other = exported(&native_identity(b"another archive"));
    for bytes in [
        vec![],
        b"{}".to_vec(),
        b"{\"version\":1}".to_vec(),
        vec![b'x'; STATE_BYTES + 1],
        other,
    ] {
        write_fixture(&directory, &bytes);
        let result = loaded(&mut owner, &fence).await;
        assert!(result.view().is_err());
    }
}
#[tokio::test]
async fn normalized_duplicate_denials_and_unknown_record_fields_fail_closed() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let directory = make_namespace(&fixture, &fence);
    let record = serde_json::json!({"right":{"id":"input.pointer","scope":{"kind":"animation_viewport"}},"allowed":false,"remembered":true,"binding_hash":null,"approved_hash":identity.content_hash()});
    let mut envelope: serde_json::Value = serde_json::from_slice(&exported(&identity)).unwrap();
    envelope["records"] = serde_json::json!([record.clone(), record.clone()]);
    write_fixture(&directory, &serde_json::to_vec(&envelope).unwrap());
    let result = loaded(&mut owner, &fence).await;
    assert!(matches!(
        result.view(),
        Err(PersistenceError::Invalid("duplicate normalized scope"))
    ));
    drop(result);
    let mut bad = record;
    bad["publisher_grant"] = serde_json::json!(true);
    envelope["records"] = serde_json::json!([bad]);
    write_fixture(&directory, &serde_json::to_vec(&envelope).unwrap());
    let result = loaded(&mut owner, &fence).await;
    assert!(matches!(result.view(), Err(PersistenceError::Invalid(_))));
}
#[tokio::test]
async fn wrong_type_namespace_and_ledger_do_not_become_missing_state() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    // Exact task-owned namespace path only; no symlink/FIFO OS code in client.
    std::fs::write(
        fixture.directory.path().join(&fence.namespace),
        b"regular instead of directory",
    )
    .unwrap();
    let result = loaded(&mut owner, &fence).await;
    assert!(matches!(
        result.view(),
        Err(PersistenceError::Io {
            stage: "namespace",
            ..
        })
    ));
    drop(result);
    std::fs::remove_file(fixture.directory.path().join(&fence.namespace)).unwrap();
    let directory = make_namespace(&fixture, &fence);
    directory.child(LEAF, true).unwrap();
    let result = loaded(&mut owner, &fence).await;
    assert!(matches!(
        result.view(),
        Err(PersistenceError::Io {
            stage: "open regular ledger",
            ..
        })
    ));
}
#[tokio::test]
async fn stale_base_refuses_to_overwrite_concurrent_native_denial() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut first = fixture.owner();
    let fence1 = first.bind(&identity, stamp(1)).unwrap();
    let base1 = loaded(&mut first, &fence1).await;
    let mut second = fixture.owner();
    let fence2 = second.bind(&identity, stamp(2)).unwrap();
    let base2 = loaded(&mut second, &fence2).await;
    let mut denial: serde_json::Value = serde_json::from_slice(&exported(&identity)).unwrap();
    denial["block_all"] = serde_json::json!(true);
    let denial = serde_json::to_vec(&denial).unwrap();
    second
        .request_commit(base2.view().as_ref().unwrap(), &denial)
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut second).await else {
        panic!("write missing")
    };
    assert!(result.view().is_ok());
    drop(result);
    first
        .request_commit(base1.view().as_ref().unwrap(), &exported(&identity))
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut first).await else {
        panic!("conflict completion missing")
    };
    assert!(matches!(result.view(), Err(PersistenceError::Conflict)));
    drop(result);
    let latest = loaded(&mut second, &fence2).await;
    assert_eq!(
        latest.view().as_ref().unwrap().bytes(),
        Some(denial.as_slice())
    );
}
#[tokio::test]
async fn cancellation_before_issue_drops_stage_without_publishing() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let initial = loaded(&mut owner, &fence).await;
    owner
        .enqueue_commit(
            initial.view().as_ref().unwrap(),
            &exported(&identity),
            Some(TestFault::InvalidateBeforePublish),
        )
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("cancellation missing")
    };
    assert!(matches!(result.view(), Err(PersistenceError::Canceled)));
    drop(result);
    let directory = make_namespace(&fixture, &fence);
    assert!(directory.open_file(LEAF).is_err());
    assert!(directory.list(8).unwrap().is_empty());
}
#[tokio::test]
async fn post_publish_failure_never_reports_durable_success_from_readback() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let initial = loaded(&mut owner, &fence).await;
    let bytes = exported(&identity);
    owner
        .enqueue_commit(
            initial.view().as_ref().unwrap(),
            &bytes,
            Some(TestFault::FailAfterPublish),
        )
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("unknown effect missing")
    };
    assert!(matches!(
        result.view(),
        Err(PersistenceError::PublicationUnconfirmed(_))
    ));
    drop(result);
    let observed = loaded(&mut owner, &fence).await;
    assert_eq!(
        observed.view().as_ref().unwrap().bytes(),
        Some(bytes.as_slice())
    );
    // Observation exists, but the failed original commit has no Durable receipt.
}
#[tokio::test]
async fn independent_broker_fence_and_old_selection_cannot_commit_or_claim_current() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let base = loaded(&mut owner, &fence).await;
    let mut other = fixture.owner();
    let foreign = other.bind(&identity, stamp(1)).unwrap();
    assert!(matches!(
        owner.request_load(&foreign),
        Err(PersistenceError::Stale)
    ));
    owner
        .request_commit(base.view().as_ref().unwrap(), &exported(&identity))
        .unwrap();
    let PermissionCompletion::Written(receipt) = completion(&mut owner).await else {
        panic!("write missing")
    };
    assert!(receipt.view().as_ref().unwrap().is_current());
    let next = owner.bind(&identity, stamp(2)).unwrap();
    assert!(!base.view().as_ref().unwrap().is_current());
    assert!(!receipt.view().as_ref().unwrap().is_current());
    assert!(matches!(
        owner.request_commit(base.view().as_ref().unwrap(), &exported(&identity)),
        Err(PersistenceError::Stale)
    ));
    assert_eq!(next.stamp(), stamp(2));
}
#[tokio::test]
async fn closed_admission_drains_exact_accepted_write_and_preserves_outcome_charge() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let base = loaded(&mut owner, &fence).await;
    owner
        .request_commit(base.view().as_ref().unwrap(), &exported(&identity))
        .unwrap();
    owner.close_admission();
    assert!(matches!(
        owner.request_load(&fence),
        Err(PersistenceError::Admission(RejectReason::Closed))
    ));
    let PermissionCompletion::Written(outcome) = completion(&mut owner).await else {
        panic!("drain completion missing")
    };
    assert!(outcome.view().is_ok());
    assert!(fixture.quota.snapshot().result_bytes >= COST.result_bytes);
    drop(outcome);
    drop(base);
    assert_eq!(fixture.quota.snapshot().result_bytes, 0);
}
#[tokio::test]
async fn admitted_worker_capture_remains_charged_through_actual_cancelled_exit() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let base = loaded(&mut owner, &fence).await;
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    owner
        .enqueue_commit(
            base.view().as_ref().unwrap(),
            &exported(&identity),
            Some(TestFault::PauseAfterFlush {
                entered,
                release: gate,
            }),
        )
        .unwrap();
    assert!(owner.collect().is_none());
    // This current-thread test awaits only its own task-local worker signal.
    observed.recv_timeout(Duration::from_secs(3)).unwrap();
    owner.invalidate(&fence).unwrap();
    assert!(fixture.quota.snapshot().input_bytes >= COST.input_bytes * 2);
    release.send(()).unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("actual exit outcome missing")
    };
    assert!(matches!(result.view(), Err(PersistenceError::Canceled)));
    assert!(fixture.quota.snapshot().input_bytes >= COST.input_bytes * 2);
    drop(result);
    drop(base);
    assert_eq!(fixture.quota.snapshot().input_bytes, 0);
}
#[tokio::test]
async fn snapshot_survives_owner_drop_without_uncharged_clone_or_namespace_escape() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let base = loaded(&mut owner, &fence).await;
    owner
        .request_commit(base.view().as_ref().unwrap(), &exported(&identity))
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("write missing")
    };
    drop(result);
    drop(base);
    let loaded = loaded(&mut owner, &fence).await;
    let bytes = loaded.view().as_ref().unwrap().bytes().unwrap();
    let expected = sha(bytes);
    owner.close_admission();
    drop(owner);
    assert!(!loaded.view().as_ref().unwrap().is_current());
    assert_eq!(
        sha(loaded.view().as_ref().unwrap().bytes().unwrap()),
        expected
    );
    assert!(fixture.quota.snapshot().worker_bytes >= SNAPSHOT_BYTES);
    assert_eq!(fence.namespace.len(), 64);
    assert!(fence.namespace.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(PackageIdentity::unverified("../escape".into(), b"archive").is_err());
}

#[tokio::test]
async fn same_bytes_in_replaced_namespace_do_not_satisfy_loaded_native_base() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let directory = make_namespace(&fixture, &fence);
    let bytes = exported(&identity);
    write_fixture(&directory, &bytes);
    let base = loaded(&mut owner, &fence).await;
    let old_identity = base
        .view()
        .as_ref()
        .unwrap()
        .directory
        .as_ref()
        .unwrap()
        .identity();
    let current_path = fixture.directory.path().join(&fence.namespace);
    let retired_path = fixture.directory.path().join("retained-native-namespace");
    std::fs::rename(&current_path, &retired_path).unwrap();
    let replacement = make_namespace(&fixture, &fence);
    assert_ne!(old_identity, replacement.identity());
    write_fixture(&replacement, &bytes);
    owner
        .request_commit(base.view().as_ref().unwrap(), &bytes)
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("identity conflict missing")
    };
    assert!(matches!(result.view(), Err(PersistenceError::Conflict)));
    let latest = loaded(&mut owner, &fence).await;
    assert_eq!(
        latest.view().as_ref().unwrap().bytes(),
        Some(bytes.as_slice())
    );
}

#[tokio::test]
async fn epoch_rollback_and_original_client_admission_fail_before_effect() {
    let fixture = Fixture::new();
    let identity = native_identity(b"archive");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let directory = make_namespace(&fixture, &fence);
    let mut saved: serde_json::Value = serde_json::from_slice(&exported(&identity)).unwrap();
    saved["epoch"] = serde_json::json!(9);
    let bytes = serde_json::to_vec(&saved).unwrap();
    write_fixture(&directory, &bytes);
    let base = loaded(&mut owner, &fence).await;
    owner
        .request_commit(base.view().as_ref().unwrap(), &exported(&identity))
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("epoch rejection missing")
    };
    assert!(matches!(
        result.view(),
        Err(PersistenceError::Invalid("ledger epoch rollback"))
    ));
    let restrictive = fixture
        .execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1,
            result_bytes: 1,
        })
        .unwrap();
    let mut refused = PluginPermissionFiles::new(
        &restrictive,
        Arc::clone(&fixture.root),
        Arc::new(Notify::new()),
        Arc::new(|| {}),
    )
    .unwrap();
    let refused_fence = refused.bind(&identity, stamp(2)).unwrap();
    let jobs_before = fixture.quota.snapshot().jobs;
    assert!(matches!(
        refused.request_load(&refused_fence),
        Err(PersistenceError::Admission(
            RejectReason::InputBytes | RejectReason::ResultBytes
        ))
    ));
    assert_eq!(fixture.quota.snapshot().jobs, jobs_before);
    assert!(!refused.is_pending());
    let latest = loaded(&mut owner, &fence).await;
    assert_eq!(
        latest.view().as_ref().unwrap().bytes(),
        Some(bytes.as_slice())
    );
}

/// Uses the real finite IO bank, native ledger load, and durable write. Wake
/// callbacks never collect, block, or wait for jobs on the execution thread.
#[tokio::test]
async fn load_and_durable_write_wake_native_actor_and_ui_and_retain_capture() {
    let mut fixture = Fixture::new();
    let ui_ready = Arc::new(Notify::new());
    let actor_ready = Arc::new(Notify::new());
    let (sender, receiver) = mpsc::sync_channel(1);
    let capture = Arc::new(fixture.quota.reserve_external_storage(4096).unwrap());
    let weak_capture = Arc::downgrade(&capture);
    let captured_ready = Arc::clone(&actor_ready);
    let actor_wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        // Keep the ORIGINAL root admission with the actual bounded actor hint.
        let _retained_original_admission = &capture;
        let _ = sender.try_send(());
        captured_ready.notify_one();
    });
    let mut owner = PluginPermissionFiles::new(
        &fixture.client,
        Arc::clone(&fixture.root),
        Arc::clone(&ui_ready),
        Arc::clone(&actor_wake),
    )
    .unwrap();
    drop(actor_wake);
    let identity = native_identity(b"actor wake immutable fixture");
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    owner.request_load(&fence).unwrap();
    tokio::time::timeout(Duration::from_secs(3), actor_ready.notified())
        .await
        .expect("native actor load wake");
    assert_eq!(receiver.try_recv(), Ok(()));
    tokio::time::timeout(Duration::from_secs(3), ui_ready.notified())
        .await
        .expect("UI load wake");
    let Some(PermissionCompletion::Loaded(initial)) = owner.collect() else {
        panic!("load available at native terminal wake")
    };
    owner
        .request_commit(initial.view().as_ref().unwrap(), &exported(&identity))
        .unwrap();
    // Consume the distinct admission UI hint before testing terminal delivery.
    ui_ready.notified().await;
    assert!(owner.collect().is_none()); // starts the ordered IO job
    tokio::time::timeout(Duration::from_secs(3), actor_ready.notified())
        .await
        .expect("native actor durable write wake");
    assert_eq!(receiver.try_recv(), Ok(()));
    tokio::time::timeout(Duration::from_secs(3), ui_ready.notified())
        .await
        .expect("UI durable write wake");
    let Some(PermissionCompletion::Written(written)) = owner.collect() else {
        panic!("durable result available at native terminal wake")
    };
    assert!(written.view().as_ref().unwrap().is_current());
    drop(owner);
    drop(fence);
    // Terminal notification can precede destruction of the bank's callback
    // capture. Join this fixture's own bank before checking LAST-owner release.
    fixture.execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        fixture
            .execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
    assert!(weak_capture.upgrade().is_some());
    drop(initial);
    assert!(weak_capture.upgrade().is_some()); // escaped durable receipt still owns it
    drop(written);
    assert!(weak_capture.upgrade().is_none());
}

#[tokio::test]
async fn controller_matches_real_native_receipt_and_refuses_every_mismatched_coordinate() {
    use super::super::plugin_permission_controller::{receipt_matches, WriteExpectation};
    let fixture = Fixture::new();
    let identity = native_identity(b"controller durable fixture");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let initial = loaded(&mut owner, &fence).await;
    let bytes = exported(&identity);
    let write_id = owner
        .request_commit(initial.view().as_ref().unwrap(), &bytes)
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("real write outcome");
    };
    let receipt = result.view().as_ref().unwrap();
    let expected = WriteExpectation {
        write_id,
        storage_stamp: stamp(1),
        candidate_digest: sha(&bytes),
        candidate_epoch: 1,
        native_plan_revision: 1,
    };
    assert!(receipt_matches(receipt, expected, &fence));
    let mut other_owner = fixture.owner();
    let foreign_fence = other_owner.bind(&identity, stamp(1)).unwrap();
    assert!(!receipt_matches(receipt, expected, &foreign_fence));
    let mut wrong = expected;
    wrong.write_id = WriteId(write_id.0 + 1);
    assert!(!receipt_matches(receipt, wrong, &fence));
    let mut wrong = expected;
    wrong.storage_stamp.selection_revision += 1;
    assert!(!receipt_matches(receipt, wrong, &fence));
    let mut wrong = expected;
    wrong.storage_stamp.instance_id += 1;
    assert!(!receipt_matches(receipt, wrong, &fence));
    let mut wrong = expected;
    wrong.storage_stamp.plan_revision += 1;
    assert!(!receipt_matches(receipt, wrong, &fence));
    let mut wrong = expected;
    wrong.storage_stamp.authorization_epoch += 1;
    assert!(!receipt_matches(receipt, wrong, &fence));
    let mut wrong = expected;
    wrong.candidate_digest[0] ^= 1;
    assert!(!receipt_matches(receipt, wrong, &fence));
    let mut wrong = expected;
    wrong.candidate_epoch += 1;
    assert!(!receipt_matches(receipt, wrong, &fence));
    owner.invalidate(&fence).unwrap();
    assert!(!receipt_matches(receipt, expected, &fence));
}

#[tokio::test]
async fn controller_distinguishes_storage_review_epoch_from_real_exported_candidate_epoch() {
    use super::super::plugin_permission_controller::{receipt_matches, WriteExpectation};
    let fixture = Fixture::new();
    let identity = native_identity(b"controller epoch fixture");
    let mut owner = fixture.owner();
    let fence = owner.bind(&identity, stamp(1)).unwrap();
    let initial = loaded(&mut owner, &fence).await;
    // Native consuming restore advances epoch without inventing a persistence receipt.
    // The broker restore method is crate-private to animation-js, so use the
    // REAL public revoke operation to produce a new authentic exported epoch.
    let mut broker = PermissionBroker::new(
        identity.clone(),
        Ceiling {
            permissions: vec![],
        },
        Ceiling {
            permissions: vec![],
        },
    )
    .unwrap();
    let _invalidation = broker
        .revoke(Right {
            id: ilium_animation_js::permissions::Capability::InputPointer,
            scope: Scope::AnimationViewport,
        })
        .unwrap();
    let bytes = broker.export_remembered().unwrap();
    let epoch = broker.authorization_epoch();
    assert!(epoch > fence.stamp().authorization_epoch);
    let write_id = owner
        .request_commit(initial.view().as_ref().unwrap(), &bytes)
        .unwrap();
    let PermissionCompletion::Written(result) = completion(&mut owner).await else {
        panic!("real write outcome");
    };
    let receipt = result.view().as_ref().unwrap();
    assert!(receipt_matches(
        receipt,
        WriteExpectation {
            write_id,
            storage_stamp: stamp(1),
            candidate_digest: sha(&bytes),
            candidate_epoch: epoch,
            native_plan_revision: 1,
        },
        &fence
    ));
    assert_eq!(receipt.stamp().authorization_epoch, 1);
    assert_eq!(receipt.epoch(), epoch);
}
