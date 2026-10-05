use ilium_animation_js::native_storage::{MemoryNamespace, NamespaceKey};
use ilium_animation_js::permissions::PackageIdentity;
#[cfg(target_os = "linux")]
use ilium_animation_js::{
    native_storage::{
        list_selected, read_selected, write_selected, PersistentCache, PersistentNamespace,
        SelectedRead, SelectedStorage, SelectedWrite, StorageCancellation, StorageOperation,
    },
    permissions::{
        Capability, Ceiling, Channel, Demand, HostBinding, PermissionBroker, PermissionPlan,
        PermissionRequest, Right, Scope, Selection, UserChoice,
    },
};
use ilium_execution::{QuotaGroup, QuotaLimits};
#[cfg(target_os = "linux")]
use ilium_platform::animation_files::{PinnedDirectory, WriteMode};
use ilium_platform::secure_fs::NoFollowDirectory;
#[cfg(target_os = "linux")]
use sha2::Digest;
#[cfg(target_os = "linux")]
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
#[cfg(target_os = "linux")]
fn operation<'a>(
    broker: &'a mut PermissionBroker,
    channel: &'a Channel,
    cancellation: &'a StorageCancellation,
) -> StorageOperation<'a> {
    StorageOperation {
        broker,
        channel,
        demand: "storage",
        cancellation,
    }
}
fn quota() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 2,
        worker_bytes: 64 * 1024 * 1024,
    })
}
#[cfg(target_os = "linux")]
struct Fixture {
    path: PathBuf,
    root: Arc<PinnedDirectory>,
}
#[cfg(target_os = "linux")]
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ilium-storage-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(&path).unwrap()))
                .unwrap(),
        );
        Self { path, root }
    }
}
#[cfg(target_os = "linux")]
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}
#[cfg(target_os = "linux")]
fn activate(
    principal: &PackageIdentity,
    rights: Vec<Right>,
    binding: Option<HostBinding>,
) -> (PermissionBroker, Channel) {
    let ceiling = Ceiling {
        permissions: rights.clone(),
    };
    let mut broker = PermissionBroker::new(principal.clone(), ceiling.clone(), ceiling).unwrap();
    let mut permissions = Vec::new();
    let mut requests = BTreeSet::new();
    let mut bindings = BTreeMap::new();
    let mut answers = BTreeMap::new();
    for (index, right) in rights.into_iter().enumerate() {
        let id = format!("right_{index}");
        requests.insert(id.clone());
        if matches!(right.scope, Scope::Disk { .. }) {
            bindings.insert(id.clone(), binding.clone().unwrap());
        }
        answers.insert(id.clone(), UserChoice::AllowSession);
        permissions.push(PermissionRequest {
            request_id: Some(id),
            id: right.id,
            scope: right.scope,
            required: true,
            reason: "Read and write the isolated test fixture.".into(),
        });
    }
    let review = broker
        .prepare(
            1,
            1,
            PermissionPlan {
                permissions,
                demands: vec![Demand {
                    demand_id: "storage".into(),
                    request_ids: requests,
                }],
            },
            bindings,
        )
        .unwrap();
    let channel = broker
        .resolve(review, answers)
        .unwrap()
        .activation
        .unwrap()
        .channel;
    (broker, channel)
}
fn principal() -> PackageIdentity {
    PackageIdentity::unverified("storage-fixture".into(), b"fixture-v1").unwrap()
}
#[cfg(target_os = "linux")]
fn disk(id: Capability) -> Right {
    Right {
        id,
        scope: Scope::Disk {
            slot: "files".into(),
            selection: Selection::Folder,
        },
    }
}
#[test]
#[cfg(target_os = "linux")]
fn selected_folder_atomic_write_read_hash_and_listing_are_actual() {
    let fixture = Fixture::new();
    let quota = quota();
    let selected = SelectedStorage::folder_from_host(
        fixture.root.clone(),
        "files".into(),
        true,
        quota.clone(),
    )
    .unwrap();
    let (mut broker, channel) = activate(
        &principal(),
        vec![disk(Capability::DiskRead), disk(Capability::DiskWrite)],
        Some(selected.binding().clone()),
    );
    let cancel = StorageCancellation::default();
    let receipt = write_selected(
        operation(&mut broker, &channel, &cancel),
        &selected,
        SelectedWrite {
            leaf: "scene.json",
            bytes: br#"{"frame":7}"#,
            mode: WriteMode::CreateNew,
        },
        &quota,
    )
    .unwrap();
    assert!(receipt.durable);
    assert!(!receipt.metadata_preserved);
    let data = read_selected(
        operation(&mut broker, &channel, &cancel),
        &selected,
        SelectedRead {
            leaf: Some("scene.json"),
            maximum: 1024,
        },
        &quota,
    )
    .unwrap()
    .deliver(&mut broker)
    .unwrap();
    assert_eq!(data.view(), br#"{"frame":7}"#);
    assert_eq!(receipt.sha256, data.sha256());
    let listing = list_selected(
        &mut broker,
        &channel,
        "storage",
        &selected,
        8,
        &cancel,
        &quota,
    )
    .unwrap()
    .deliver(&mut broker)
    .unwrap();
    assert_eq!(listing.view()[0].name, "scene.json");
    assert_eq!(broker.pending_operations(), 0);
}
#[test]
#[cfg(target_os = "linux")]
fn revoked_or_cancelled_native_reads_cannot_deliver_and_guards_retire() {
    let fixture = Fixture::new();
    std::fs::write(fixture.path.join("value"), b"sensitive").unwrap();
    let quota = quota();
    let selected = SelectedStorage::folder_from_host(
        fixture.root.clone(),
        "files".into(),
        false,
        quota.clone(),
    )
    .unwrap();
    let baseline = quota.snapshot().worker_bytes;
    let (mut broker, channel) = activate(
        &principal(),
        vec![disk(Capability::DiskRead)],
        Some(selected.binding().clone()),
    );
    let cancellation = StorageCancellation::default();
    let read = read_selected(
        operation(&mut broker, &channel, &cancellation),
        &selected,
        SelectedRead {
            leaf: Some("value"),
            maximum: 1024,
        },
        &quota,
    )
    .unwrap();
    assert!(quota.snapshot().worker_bytes > baseline);
    cancellation.cancel();
    assert!(read.deliver(&mut broker).is_err());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    assert_eq!(broker.pending_operations(), 0);
    let cancel = StorageCancellation::default();
    let read = read_selected(
        operation(&mut broker, &channel, &cancel),
        &selected,
        SelectedRead {
            leaf: Some("value"),
            maximum: 1024,
        },
        &quota,
    )
    .unwrap();
    let _invalidation = broker.revoke(disk(Capability::DiskRead)).unwrap();
    assert!(read.deliver(&mut broker).is_err());
    assert_eq!(broker.pending_operations(), 0);
}
#[test]
#[cfg(target_os = "linux")]
fn traversal_bounds_and_wrong_binding_fail_without_effects() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let quota = quota();
    let selected = SelectedStorage::folder_from_host(
        fixture.root.clone(),
        "files".into(),
        true,
        quota.clone(),
    )
    .unwrap();
    let foreign =
        SelectedStorage::folder_from_host(other.root.clone(), "files".into(), true, quota.clone())
            .unwrap();
    let (mut broker, channel) = activate(
        &principal(),
        vec![disk(Capability::DiskRead), disk(Capability::DiskWrite)],
        Some(selected.binding().clone()),
    );
    let cancel = StorageCancellation::default();
    assert!(write_selected(
        operation(&mut broker, &channel, &cancel),
        &foreign,
        SelectedWrite {
            leaf: "value",
            bytes: b"bad",
            mode: WriteMode::CreateNew
        },
        &quota
    )
    .is_err());
    assert!(write_selected(
        operation(&mut broker, &channel, &cancel),
        &selected,
        SelectedWrite {
            leaf: "../escape",
            bytes: b"bad",
            mode: WriteMode::CreateNew
        },
        &quota
    )
    .is_err());
    assert!(list_selected(
        &mut broker,
        &channel,
        "storage",
        &selected,
        1025,
        &cancel,
        &quota
    )
    .is_err());
    assert!(fixture.root.list(8).unwrap().is_empty());
    assert!(other.root.list(8).unwrap().is_empty());
}
#[test]
fn immutable_cache_get_keeps_original_admission_and_isolates_full_principal() {
    let quota = quota();
    let baseline = quota.snapshot().worker_bytes;
    let a = principal();
    let b = PackageIdentity::unverified("storage-fixture".into(), b"fixture-v2").unwrap();
    let mut cache = MemoryNamespace::new(&a, quota.clone()).unwrap();
    let other = MemoryNamespace::new(&b, quota.clone()).unwrap();
    assert_ne!(cache.principal_digest(), other.principal_digest());
    cache.put("frame", b"retained").unwrap();
    let retained = cache.get("frame").unwrap().unwrap();
    let shared = cache.get("frame").unwrap().unwrap();
    assert!(Arc::ptr_eq(&retained, &shared));
    assert!(other.get("frame").unwrap().is_none());
    drop(shared);
    drop(cache);
    drop(other);
    assert!(quota.snapshot().worker_bytes > baseline);
    drop(retained);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
#[test]
fn memory_cache_respects_ttl_remove_and_caller_read_bound() {
    let mut cache = MemoryNamespace::new(&principal(), quota()).unwrap();
    cache.put_with_ttl("short", b"value", Some(1)).unwrap();
    assert_eq!(
        cache.get_bounded("short", 5).unwrap().unwrap().view(),
        b"value"
    );
    assert!(cache.get_bounded("short", 4).is_err());
    assert!(cache.put_with_ttl("bad", b"x", Some(0)).is_err());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(cache.get("short").unwrap().is_none());
    assert!(!cache.remove("short").unwrap());
    cache.put("long", b"stable").unwrap();
    assert!(cache.remove("long").unwrap());
    assert!(!cache.remove("long").unwrap());
    assert!(cache.get("long").unwrap().is_none());
}
#[test]
fn memory_cache_rejects_overlarge_value_before_copy() {
    let mut cache = MemoryNamespace::new(&principal(), quota()).unwrap();
    let large = vec![9u8; 8 * 1024 * 1024 + 1];
    assert!(cache.put("too_big", &large).is_err());
    assert!(cache.get("too_big").unwrap().is_none());
}
#[cfg(target_os = "linux")]
#[test]
fn native_picker_pins_selected_folder_and_rejects_symlinked_ancestor() {
    use ilium_animation_js::permissions::Selection;
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("chosen");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("item"), b"pinned").unwrap();
    let quota = quota();
    let selected = SelectedStorage::pin_user_path(
        &folder,
        Selection::Folder,
        "selected".into(),
        false,
        quota.clone(),
    )
    .unwrap();
    let right = Right {
        id: Capability::DiskRead,
        scope: Scope::Disk {
            slot: "selected".into(),
            selection: Selection::Folder,
        },
    };
    assert!(selected.matches_right(&right));
    let bytes = selected
        .read_after_issue("item", 16, &StorageCancellation::default(), &quota)
        .unwrap();
    assert_eq!(bytes.view(), b"pinned");
    std::os::unix::fs::symlink(&folder, temp.path().join("linked")).unwrap();
    assert!(SelectedStorage::pin_user_path(
        &temp.path().join("linked"),
        Selection::Folder,
        "selected".into(),
        false,
        quota
    )
    .is_err());
}
#[cfg(target_os = "linux")]
#[test]
fn selected_folder_write_uses_pinned_inode_after_path_substitution_and_rejects_child_symlink() {
    use ilium_animation_js::permissions::Selection;
    let temp = tempfile::tempdir().unwrap();
    let chosen = temp.path().join("chosen");
    let moved = temp.path().join("moved");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&chosen).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let quota = quota();
    let selected = SelectedStorage::pin_user_path(
        &chosen,
        Selection::Folder,
        "selected".into(),
        true,
        quota.clone(),
    )
    .unwrap();
    let write_right = Right {
        id: Capability::DiskWrite,
        scope: Scope::Disk {
            slot: "selected".into(),
            selection: Selection::Folder,
        },
    };
    assert!(selected.matches_right(&write_right));
    // Replacing the path after the picker pins it cannot retarget the grant.
    std::fs::rename(&chosen, &moved).unwrap();
    std::fs::create_dir(&chosen).unwrap();
    let cancellation = StorageCancellation::default();
    let hash = selected
        .write_after_issue("frame.bin", b"original", true, &cancellation, &quota)
        .unwrap();
    let expected: [u8; 32] = sha2::Sha256::digest(b"original").into();
    assert_eq!(hash, expected);
    assert_eq!(std::fs::read(moved.join("frame.bin")).unwrap(), b"original");
    assert!(!chosen.join("frame.bin").exists());
    std::os::unix::fs::symlink(&outside, moved.join("alias")).unwrap();
    assert!(selected
        .write_after_issue("alias/escape", b"bad", true, &cancellation, &quota)
        .is_err());
    assert!(!outside.join("escape").exists());
}
#[test]
#[cfg(target_os = "linux")]
fn persistent_state_durable_ack_reload_and_principal_fence_are_real() {
    let fixture = Fixture::new();
    let quota = quota();
    let principal = principal();
    let right = Right {
        id: Capability::StatePersist,
        scope: Scope::Namespace {
            name: "animation".into(),
        },
    };
    let (mut broker, channel) = activate(&principal, vec![right], None);
    let namespace = PersistentNamespace::open_from_host(
        &mut broker,
        &channel,
        "storage",
        &principal,
        fixture.root.clone(),
        "animation".into(),
        quota.clone(),
    )
    .unwrap();
    let cancel = StorageCancellation::default();
    namespace
        .write(
            operation(&mut broker, &channel, &cancel),
            NamespaceKey {
                key: "settings",
                state: true,
            },
            br#"{"speed":1.2}"#,
        )
        .unwrap();
    drop(namespace);
    let namespace = PersistentNamespace::open_from_host(
        &mut broker,
        &channel,
        "storage",
        &principal,
        fixture.root.clone(),
        "animation".into(),
        quota.clone(),
    )
    .unwrap();
    let bytes = namespace
        .read(
            operation(&mut broker, &channel, &cancel),
            NamespaceKey {
                key: "settings",
                state: true,
            },
            4096,
        )
        .unwrap()
        .deliver(&mut broker)
        .unwrap();
    assert_eq!(bytes.view(), br#"{"speed":1.2}"#);
    let foreign = PackageIdentity::unverified("other".into(), b"bytes").unwrap();
    assert!(PersistentNamespace::open_from_host(
        &mut broker,
        &channel,
        "storage",
        &foreign,
        fixture.root.clone(),
        "animation".into(),
        quota
    )
    .is_err());
    assert_eq!(broker.pending_operations(), 0);
}
#[test]
#[cfg(target_os = "linux")]
fn persistent_cache_ttl_reload_remove_and_principal_isolation_are_real() {
    let fixture = Fixture::new();
    let quota = quota();
    let cancel = StorageCancellation::default();
    let store = PersistentCache::new(&principal(), fixture.root.clone(), quota.clone()).unwrap();
    assert!(store.get("missing", 16, &cancel).unwrap().is_none());
    let hash = store.put("kept", b"durable", None, &cancel).unwrap();
    let expected: [u8; 32] = sha2::Sha256::digest(b"durable").into();
    assert_eq!(hash, expected);
    assert_eq!(
        store.get("kept", 7, &cancel).unwrap().unwrap().view(),
        b"durable"
    );
    assert!(store.get("kept", 6, &cancel).is_err());
    drop(store);
    let reopened = PersistentCache::new(&principal(), fixture.root.clone(), quota.clone()).unwrap();
    assert_eq!(
        reopened.get("kept", 16, &cancel).unwrap().unwrap().view(),
        b"durable"
    );
    let foreign = PackageIdentity::unverified("other-cache".into(), b"other-v1").unwrap();
    let other = PersistentCache::new(&foreign, fixture.root.clone(), quota).unwrap();
    assert!(other.get("kept", 16, &cancel).unwrap().is_none());
    assert!(reopened.remove("kept", &cancel).unwrap());
    assert!(!reopened.remove("kept", &cancel).unwrap());
    reopened.put("short", b"ttl", Some(1), &cancel).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(reopened.get("short", 16, &cancel).unwrap().is_none());
}

#[cfg(not(target_os = "linux"))]
#[test]
fn native_atomic_storage_reports_unsupported_on_non_linux_platforms() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let no_follow_directory = NoFollowDirectory::open_root(temporary_directory.path()).unwrap();
    let error = match PinnedDirectory::from_host(Arc::new(no_follow_directory)) {
        Ok(_) => panic!("native atomic storage unexpectedly qualified on this platform"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(
        error.to_string(),
        "animation atomic storage is qualified only on Linux"
    );
}
