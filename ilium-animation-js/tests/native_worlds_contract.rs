#![cfg(feature = "native-host")]
use ilium_ambient::resources::AmbientResources;
use ilium_animation_js::native_worlds::{
    GeneratedWorldSettings, TerminalPaintEvidence, WorldRenderRequest, WorldService,
};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
};
use std::time::{Duration, SystemTime};
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
    let resources = AmbientResources::new(
        execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 1024,
                result_bytes: 1024,
            })
            .unwrap(),
    );
    (execution, resources, quota)
}
fn request() -> WorldRenderRequest {
    WorldRenderRequest {
        width: 32,
        height: 16,
        time: Duration::ZERO,
        wall: Duration::ZERO,
        now: SystemTime::UNIX_EPOCH,
        pre_rendered: false,
    }
}
#[test]
fn generated_native_raster_and_three_receipt_leases_are_real_and_bounded() {
    let (_execution, resources, quota) = host();
    let mut service = WorldService::new(resources, quota, 7).unwrap();
    let handle = service
        .insert_generated(GeneratedWorldSettings::default())
        .unwrap();
    let a = service.render(handle, request()).unwrap();
    assert_eq!(a.raster().dots.len(), 32 * 16 * 8);
    assert!(a.raster().dots.iter().any(|dot| *dot > 0.));
    assert!(a.has_cell_colors());
    let b = service.render(handle, request()).unwrap();
    let c = service.render(handle, request()).unwrap();
    assert!(service.render(handle, request()).is_err());
    drop(b);
    let d = service.render(handle, request()).unwrap();
    let evidence = TerminalPaintEvidence::after_host_emission(&a, &[]).unwrap();
    service.settle(&a, evidence).unwrap();
    drop(c);
    drop(d);
}
#[test]
fn cancellation_stops_issuance_but_preserves_committed_native_settlement() {
    let (_execution, resources, quota) = host();
    let mut service = WorldService::new(resources, quota, 1).unwrap();
    let handle = service
        .insert_generated(GeneratedWorldSettings::default())
        .unwrap();
    let frame = service.render(handle, request()).unwrap();
    let evidence = TerminalPaintEvidence::after_host_emission(&frame, &[]).unwrap();
    service.cancel();
    assert!(service.render(handle, request()).is_err());
    assert!(TerminalPaintEvidence::after_host_emission(&frame, &[]).is_err());
    assert!(service.settle(&frame, evidence).is_ok());
}
#[test]
fn retained_native_frame_is_charged_after_service_drop() {
    let (_execution, resources, quota) = host();
    let baseline = quota.snapshot().worker_bytes;
    let mut service = WorldService::new(resources.clone(), quota.clone(), 2).unwrap();
    let handle = service
        .insert_generated(GeneratedWorldSettings::default())
        .unwrap();
    let frame = service.render(handle, request()).unwrap();
    drop(service);
    assert!(quota.snapshot().worker_bytes > baseline);
    drop(frame);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
#[test]
fn pre_rendered_frames_never_mint_terminal_credit_and_sizes_reject_before_render() {
    let (_execution, resources, quota) = host();
    let mut service = WorldService::new(resources, quota, 9).unwrap();
    let handle = service
        .insert_generated(GeneratedWorldSettings::default())
        .unwrap();
    let mut pre = request();
    pre.pre_rendered = true;
    let frame = service.render(handle, pre).unwrap();
    assert!(TerminalPaintEvidence::after_host_emission(&frame, &[]).is_err());
    let mut huge = request();
    huge.width = 1000;
    assert!(service.render(handle, huge).is_err());
}

#[test]
fn native_saved_receipts_bind_exact_root_frame_and_final_dot_counts() {
    // Task-local synthetic source fixture uses the real native Raster owner
    // writer and Scene seal/presentation protocol, without loading user worlds.
    use ilium_ambient::{raster::PaintedOwner, scene::FrameReceiptId, Frame, Scene};
    use ilium_animation_js::{
        error::Result,
        native_worlds::{HostWorldScene, SavedWorldFactory, SavedWorldGrant, SourceIdentity},
    };
    use ilium_platform::secure_fs::NoFollowDirectory;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    struct Fixture(Arc<Mutex<Vec<PaintedOwner>>>, Arc<AtomicBool>);
    impl Scene for Fixture {
        fn render(&mut self, frame: &mut Frame<'_>) {
            frame.raster.owned_dot(0, 0, 1., 9);
            frame.raster.owned_dot(1, 0, 1., 9);
        }
        fn presented_frame(&mut self, _: FrameReceiptId, owners: &[PaintedOwner]) {
            self.0.lock().unwrap().extend_from_slice(owners);
        }
        fn uses_cell_colors(&self) -> bool {
            self.1.load(Ordering::Acquire)
        }
    }
    struct Factory {
        quota: QuotaGroup,
        credited: Arc<Mutex<Vec<PaintedOwner>>>,
        root: Arc<NoFollowDirectory>,
        colors_present: Arc<AtomicBool>,
    }
    impl SavedWorldFactory for Factory {
        fn prepare(
            &mut self,
            grant: &SavedWorldGrant,
            _: &AmbientResources,
        ) -> Result<HostWorldScene> {
            let admitted = Arc::new(self.quota.reserve_external_storage(4096).unwrap());
            Ok(HostWorldScene::from_host(
                Box::new(Fixture(self.credited.clone(), self.colors_present.clone())),
                grant.identity(),
                admitted,
                self.root.clone(),
            ))
        }
    }
    let (_execution, resources, quota) = host();
    let path = std::env::temp_dir().join(format!(
        "ilium-native-world-receipts-{}",
        std::process::id()
    ));
    std::fs::create_dir(&path).unwrap();
    let root = Arc::new(NoFollowDirectory::open_root(&path).unwrap());
    let credited = Arc::new(Mutex::new(Vec::new()));
    let colors_present = Arc::new(AtomicBool::new(false));
    let mut factory = Factory {
        quota: quota.clone(),
        credited: credited.clone(),
        root: root.clone(),
        colors_present: colors_present.clone(),
    };
    let grant =
        SavedWorldGrant::from_host(root, SourceIdentity::from_host_digest([7; 32]), 11, true)
            .unwrap();
    let mut service = WorldService::new(resources, quota, 11).unwrap();
    let handle = service.insert_saved(grant, &mut factory).unwrap();
    let frame = service.render(handle, request()).unwrap();
    assert!(TerminalPaintEvidence::after_host_emission(&frame, &[0, 0]).is_err());
    assert!(!frame.has_cell_colors());
    assert!(frame.colors().iter().all(|color| *color == [0; 3]));
    assert!(TerminalPaintEvidence::after_host_emission(&frame, &[usize::MAX]).is_err());
    colors_present.store(true, Ordering::Release);
    let other = service.render(handle, request()).unwrap();
    assert!(other.has_cell_colors());
    assert!(other.colors().iter().all(|color| *color == [0; 3]));
    assert!(!frame.has_cell_colors());
    let wrong = TerminalPaintEvidence::after_host_emission(&frame, &[0, 1]).unwrap();
    assert!(service.settle(&other, wrong).is_err());
    assert!(credited.lock().unwrap().is_empty());
    let actual = TerminalPaintEvidence::after_host_emission(&frame, &[0, 1]).unwrap();
    service.cancel();
    assert!(TerminalPaintEvidence::after_host_emission(&frame, &[0, 1]).is_err());
    service.settle(&frame, actual).unwrap();
    assert_eq!(
        *credited.lock().unwrap(),
        vec![PaintedOwner { id: 9, dots: 2 }]
    );
    let mut pre = request();
    pre.pre_rendered = true;
    assert!(service.render(handle, pre).is_err());
    drop(other);
    drop(frame);
    drop(service);
    drop(factory);
    std::fs::remove_dir(&path).unwrap();
}
