use ilium_animation_js::{
    native_storage::{
        list_selected, read_selected, write_selected, MemoryNamespace, NamespaceKey,
        PersistentNamespace, SelectedRead, SelectedStorage, SelectedWrite, StorageCancellation,
        StorageOperation,
    },
    permissions::{
        Capability, Ceiling, Channel, Demand, HostBinding, PackageIdentity, PermissionBroker,
        PermissionPlan, PermissionRequest, Right, Scope, Selection, UserChoice,
    },
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use ilium_platform::{
    animation_files::{PinnedDirectory, WriteMode},
    secure_fs::NoFollowDirectory,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
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
struct Fixture {
    path: PathBuf,
    root: Arc<PinnedDirectory>,
}
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
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}
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
