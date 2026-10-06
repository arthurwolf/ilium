use super::*;
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
};
fn host() -> (Execution, AmbientResources, QuotaGroup) {
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 2,
        worker_bytes: 256 * 1024 * 1024,
    });
    let zero = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 1,
                priority: None,
                resident_bytes_per_thread: 1024,
            },
            io: zero,
            service: zero,
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
        })
        .unwrap();
    (execution, AmbientResources::new(client), quota)
}
fn fixture(resources: AmbientResources) -> (tempfile::TempDir, NativeSavedSource, SavedWorldGrant) {
    let temporary = tempfile::tempdir().unwrap();
    let parent_label = temporary.path().join("selected-saves");
    let selected_label = parent_label.join("original-world");
    std::fs::create_dir_all(&selected_label).unwrap();
    // Synthetic revision/admission fixture, deliberately not claimed as NBT.
    let bytes = b"synthetic metadata revision";
    std::fs::write(selected_label.join("level.dat"), bytes).unwrap();
    let archive_label = temporary.path().join("separate-host-archive.jar");
    std::fs::write(&archive_label, b"synthetic admission-only archive").unwrap();
    let native_jar =
        Arc::new(PinnedFile::from_host(std::fs::File::open(archive_label).unwrap()).unwrap());
    let parent = Arc::new(
        PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(&parent_label).unwrap(),
        ))
        .unwrap(),
    );
    let pinned_child = parent.child("original-world", false).unwrap();
    let child_identity = pinned_child.identity();
    let child = pinned_child.original_root();
    let root_identity = parent.identity();
    let mut digest = Sha256::new();
    digest.update(b"ilium-selected-world-metadata-v1");
    for value in [
        root_identity.device,
        root_identity.inode,
        child_identity.device,
        child_identity.inode,
        7,
    ] {
        digest.update(value.to_be_bytes());
    }
    digest.update(bytes);
    let grant = SavedWorldGrant::from_host(
        Arc::clone(&child),
        SourceIdentity::from_host_digest(digest.finalize().into()),
        7,
        true,
    )
    .unwrap();
    let environment = SceneEnv::for_test(temporary.path().join("host-cache"), resources);
    let history_label = temporary.path().join("host-history");
    std::fs::create_dir(&history_label).unwrap();
    let history_root = Arc::new(
        PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(&history_label).unwrap(),
        ))
        .unwrap(),
    );
    let source = NativeSavedSource {
        parent_label,
        selected_label,
        parent,
        child,
        child_identity,
        epoch: 7,
        native_jar,
        history_storage: history_label,
        history_root,
        runtime: Arc::clone(&environment.saved_runtime),
        settings: VoxelLandscapeSettings::default(),
        environment,
        stop: StopToken::default(),
    };
    (temporary, source, grant)
}
#[test]
fn native_saved_factory_revision_requires_original_arc_and_exact_metadata() {
    let (_execution, resources, _quota) = host();
    let (_temporary, source, grant) = fixture(resources);
    verify_revision(&source, &grant).unwrap();
    let other = Arc::new(NoFollowDirectory::open_root(&source.selected_label).unwrap());
    let substituted = SavedWorldGrant::from_host(other, grant.identity(), 7, true).unwrap();
    assert!(verify_revision(&source, &substituted).is_err());
    let path = source.selected_label.join("level.dat");
    let previous_age = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(previous_age))
        .unwrap();
    assert!(verify_revision(&source, &grant).is_err());
}
#[test]
fn native_saved_factory_replacement_cannot_certify_retained_original_child() {
    let (_execution, resources, _quota) = host();
    let (temporary, source, grant) = fixture(resources);
    std::fs::rename(
        &source.selected_label,
        temporary.path().join("retained-original"),
    )
    .unwrap();
    std::fs::create_dir(&source.selected_label).unwrap();
    std::fs::write(
        source.selected_label.join("level.dat"),
        b"synthetic metadata revision",
    )
    .unwrap();
    assert!(verify_revision(&source, &grant).is_err());
    assert_eq!(
        std::fs::read_dir(&source.history_storage).unwrap().count(),
        0
    );
}
#[test]
fn native_saved_factory_storage_refusal_consumes_request_without_history_or_credit_leak() {
    let (_execution, resources, quota) = host();
    let (_temporary, source, grant) = fixture(resources.clone());
    let history = source.history_storage.clone();
    let mut factory = NativeSavedFactory::from_host(source);
    let before = quota.snapshot().worker_bytes;
    let error = match factory.prepare(&grant, &resources) {
        Ok(_) => panic!("unadmitted native scene published"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("storage admission rejected"));
    assert!(factory.source.is_none());
    assert_eq!(std::fs::read_dir(&history).unwrap().count(), 0);
    assert_eq!(quota.snapshot().worker_bytes, before);
    assert!(factory.prepare(&grant, &resources).is_err());
}
#[test]
fn native_saved_factory_cancellation_refuses_before_metadata_or_history() {
    let (_execution, resources, _quota) = host();
    let (_temporary, source, grant) = fixture(resources);
    source.stop.stop();
    std::fs::remove_file(source.selected_label.join("level.dat")).unwrap();
    let error = verify_revision(&source, &grant).unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert_eq!(
        std::fs::read_dir(&source.history_storage).unwrap().count(),
        0
    );
}
