use super::*;
use crate::minecraft::settings::WorldSource;
use std::cell::RefCell;
use std::time::Instant;

#[test]
fn pending_route_replays_pixels_without_receipt_or_history_credit() {
    // Synthetic presentation-only buffer: no claimed world decode or native
    // paint proof. Actual original blank transitions are retained in audit319.
    let env = SceneEnv::for_test(
        std::env::temp_dir().join("saved-display-no-io"),
        crate::resources::test_resources(),
    );
    let saved = SavedMapsSettings {
        source: WorldSource::SavedMaps,
        saves_folder: "relative-folder".into(),
    };
    let mut scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
    let bundle = Arc::new(catalog_retained_fixture());
    scene.controller = Some(Controller::new(1, History::default()).unwrap());
    scene.bundle = Some(Arc::clone(&bundle));
    scene.plan_pending = Some((1, [4, 4]));
    let mut raster = crate::raster::Raster::default();
    raster.resize(4, 4);
    let mut colors = vec![[20, 30, 40]; 2];
    let mut frame = Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width: 2,
        height: 1,
        time: Duration::ZERO,
        wall: Duration::ZERO,
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    let scale = f64::from(VoxelLandscapeScene::scale(&scene.settings));
    let before = bundle.budget.used();
    let mut display =
        display::Display::new(&frame, scale, &bundle.budget, Cancel::new(&RASTER_STOP)).unwrap();
    frame.raster.dots[3] = 0.75;
    frame.raster.owner_ids[3] = 88;
    display.capture(&frame, scale);
    scene.display = Some(display);
    assert!(bundle.budget.used() >= before + 16 * 4 + 2 * 3);
    frame.raster.dots.fill(0.0);
    frame.raster.owner_ids.fill(0);
    frame.cell_colors.fill([0; 3]);
    scene.set_palette(&ScenePalette::default());
    scene.render(&mut frame);
    assert_eq!(frame.raster.dots[3], 0.75);
    assert_eq!(frame.cell_colors[0], [20, 30, 40]);
    assert!(frame.raster.owner_ids.iter().all(|owner| *owner == 0));
    assert!(scene.receipt.is_none());
    let id = FrameReceiptId::new(0, 1).unwrap();
    scene.seal_frame(id);
    assert!(scene.receipt_slots[0].is_none());
    scene.presented_frame(id, &[]);
    // Synthetic delayed owner88 from the previous painted view: held pixels
    // must not acquire a new issued view or enqueue category persistence.
    let old_owner = [PaintedOwner { id: 88, dots: 4 }];
    scene.presented_frame(id, &old_owner);
    scene.presented_frame(FrameReceiptId::new(0, 2).unwrap(), &old_owner);
    assert!(scene.pending_history.is_none());
    assert_eq!(
        scene.controller.as_ref().unwrap().history(),
        History::default()
    );
    assert!(scene.route.is_none() && scene.plan.is_none());
    scene.set_palette(&ScenePalette {
        stops: vec![[0, 0, 0], [255, 0, 0]],
        reverse: false,
        shift_percent: 0,
    });
    scene.render(&mut frame);
    assert!(frame.raster.dots.iter().all(|dot| *dot == 0.0));
    assert!(frame.cell_colors.iter().all(|color| *color == [0; 3]));
    assert!(scene.receipt.is_none());
    scene.display = None;
    assert_eq!(bundle.budget.used(), before);
}

#[test]
fn saved_display_invalidates_changed_palette_geometry_and_zoom() {
    let stop = AtomicBool::new(false);
    let budget = ByteBudget::new(1024 * 1024).unwrap();
    let mut raster = crate::raster::Raster::default();
    raster.resize(4, 4);
    let mut colors = vec![[5, 6, 7]; 2];
    let mut frame = Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width: 2,
        height: 1,
        time: Duration::ZERO,
        wall: Duration::ZERO,
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    let mut display = display::Display::new(&frame, 4.0, &budget, Cancel::new(&stop)).unwrap();
    frame.raster.dots.fill(0.5);
    display.capture(&frame, 4.0);
    assert!(!display.replay(&mut frame, 5.0));
    frame.width = 1;
    assert!(!display.replay(&mut frame, 4.0));
    frame.width = 2;
    frame.raster.resize(4, 8);
    assert!(!display.replay(&mut frame, 4.0));
    frame.raster.resize(4, 4);
    display.invalidate();
    assert!(!display.replay(&mut frame, 4.0));
    drop(display);
    assert_eq!(budget.used(), 0);
    stop.store(true, Ordering::Relaxed);
    assert!(display::Display::new(&frame, 4.0, &budget, Cancel::new(&stop)).is_err());
    assert_eq!(budget.used(), 0);
    let tiny = ByteBudget::new(16).unwrap();
    stop.store(false, Ordering::Relaxed);
    assert!(display::Display::new(&frame, 4.0, &tiny, Cancel::new(&stop)).is_err());
    assert_eq!(tiny.used(), 0);
}

#[test]
fn route_survey_orders_short_novel_before_longer_repeated_and_stays_finite() {
    let tiers = qualification_tiers(1024.0);
    assert_eq!(tiers.iter().map(|tier| tier.2).sum::<usize>(), 16);
    assert_eq!(tiers[0], (Choice::NovelAppearance, 1024.0, 1));
    assert_eq!(tiers[5], (Choice::NovelAppearance, 64.0, 1));
    assert_eq!(tiers[6], (Choice::RepeatedAppearance, 256.0, 1));
    assert!(tiers[..6]
        .iter()
        .all(|(choice, _, _)| *choice == Choice::NovelAppearance));
    assert!(tiers[6..10]
        .iter()
        .all(|(choice, _, _)| *choice == Choice::RepeatedAppearance));
}

#[test]
fn route_qualifies_each_candidate_before_surveying_a_weaker_tier() {
    let events = RefCell::new(Vec::new());
    let qualified = qualify_in_tier_order(
        48.0,
        1024.0,
        |choice, cap| {
            events.borrow_mut().push(("survey", choice, cap));
            if choice == Choice::NovelAppearance && (cap == 1024.0 || cap == 128.0) {
                Ok::<_, ()>(SurveyStep::Candidate(cap))
            } else {
                Ok(SurveyStep::EmptyTier)
            }
        },
        |cap| {
            events
                .borrow_mut()
                .push(("qualify", Choice::NovelAppearance, cap));
            Ok::<_, ()>((cap == 128.0).then_some(cap))
        },
    )
    .unwrap();
    assert_eq!(qualified, Some(128.0));
    assert_eq!(
        events.into_inner(),
        vec![
            ("survey", Choice::NovelAppearance, 1024.0),
            ("qualify", Choice::NovelAppearance, 1024.0),
            ("survey", Choice::NovelAppearance, 512.0),
            ("survey", Choice::NovelAppearance, 256.0),
            ("survey", Choice::NovelAppearance, 160.0),
            ("survey", Choice::NovelAppearance, 128.0),
            ("qualify", Choice::NovelAppearance, 128.0),
        ]
    );
}

#[test]
fn failed_candidate_is_qualified_before_the_next_slot_and_cumulative_stop_is_final() {
    let events = RefCell::new(Vec::new());
    let mut slot = 0;
    let qualified = qualify_in_tier_order(
        48.0,
        1024.0,
        |choice, cap| {
            events.borrow_mut().push(("survey", choice, cap));
            if choice == Choice::NovelAppearance && cap == 256.0 {
                slot += 1;
                Ok::<_, ()>(SurveyStep::Candidate(slot))
            } else if choice == Choice::NovelAppearance && cap == 160.0 {
                Ok(SurveyStep::StopSurvey)
            } else {
                Ok(SurveyStep::EmptyTier)
            }
        },
        |candidate| {
            events
                .borrow_mut()
                .push(("qualify", Choice::NovelAppearance, 256.0));
            assert!(candidate <= 2);
            Ok::<_, ()>(None::<usize>)
        },
    )
    .unwrap();
    assert_eq!(qualified, None);
    assert_eq!(slot, 2);
    let events = events.into_inner();
    let first_256 = events
        .iter()
        .position(|event| *event == ("survey", Choice::NovelAppearance, 256.0))
        .unwrap();
    assert_eq!(events[first_256 + 1].0, "qualify");
    assert_eq!(events[first_256 + 2].0, "survey");
    assert_eq!(events[first_256 + 3].0, "qualify");
    assert_eq!(
        events.last().unwrap(),
        &("survey", Choice::NovelAppearance, 160.0)
    );
}

fn history(completed: u64) -> History {
    let mut value = serde_json::to_value(History::default()).unwrap();
    value["completed"] = completed.into();
    value["revision"] = completed.into();
    let history: History = serde_json::from_value(value).unwrap();
    Controller::new(1, history).unwrap();
    history
}

#[test]
fn malformed_authored_root_is_reported_without_starting_worker() {
    let env = SceneEnv::for_test(
        std::env::temp_dir().join("saved-scene-no-io-test"),
        crate::resources::test_resources(),
    );
    let saved = SavedMapsSettings {
        source: WorldSource::SavedMaps,
        saves_folder: "relative-folder".into(),
    };
    let scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
    assert!(scene.worker.is_none());
    assert!(scene.status().unwrap().contains("absolute"));
}

#[test]
fn planner_core_radius_is_separate_from_complete_projected_viewport() {
    let policy = SavedScene::policy([160, 96], 4.0).unwrap();
    assert_eq!(policy.envelope.radius(), 16.0);
    assert_eq!(policy.envelope.eye_y, 512.0);
    assert_eq!(policy.envelope.cell_heights, [-64, 319]);
}

#[test]
fn busy_final_credit_survives_scene_drop_and_successor_reads_it() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = Repository::new(temporary.path().join("private scene history")).unwrap();
    let env = SceneEnv::for_test(
        temporary.path().join("ambient cache"),
        crate::resources::test_resources(),
    );
    let runtime = Arc::clone(&env.saved_runtime);
    runtime
        .install(Writer::start(repository.clone(), 0, &crate::resources::test_resources()).unwrap())
        .unwrap();

    // A malformed authored root constructs the same disposable scene without
    // opening a second catalog. Replace only its owned worker/terminal channel.
    let saved = SavedMapsSettings {
        source: WorldSource::SavedMaps,
        saves_folder: "relative-folder".into(),
    };
    let mut scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
    assert!(scene.worker.is_none());
    let (sender, receiver) = mpsc::sync_channel(1);
    scene.terminal_history = sender;
    let worker_runtime = Arc::clone(&runtime);
    scene.worker = Some(
        Worker::try_spawn("synthetic-saved-final-handoff", move |stop| {
            while !stop.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            finish_worker_handoff(&worker_runtime, receiver.try_recv().ok());
        })
        .unwrap(),
    );

    let latest = history(1);
    scene.pending_history = Some(latest);
    runtime.with_test_lock(|| {
        assert!(matches!(
            runtime.submit(latest),
            Err(super::super::saved_runtime::Error::Busy)
        ));
        drop(scene);
        assert_eq!(runtime.gate(), Gate::Draining);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match runtime.gate() {
            Gate::Ready => break,
            Gate::Busy | Gate::Draining => {}
            gate => panic!("final scene credit requires an unexpected recovery: {gate:?}"),
        }
        assert!(
            Instant::now() < deadline,
            "final scene handoff did not drain"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(repository.load(&|| false).unwrap().history(), latest);
}

#[test]
fn old_planner_response_cannot_replace_resized_viewport_request() {
    let temporary = tempfile::tempdir().unwrap();
    let env = SceneEnv::for_test(
        temporary.path().join("scene cache"),
        crate::resources::test_resources(),
    );
    let saved = SavedMapsSettings {
        source: WorldSource::SavedMaps,
        saves_folder: "relative-folder".into(),
    };
    let mut scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
    scene.plan_pending = Some((2, [200, 100]));
    *scene.plan_results.lock().unwrap() = Some(PlanResponse {
        sequence: 1,
        size: [160, 96],
        outcome: Ok(None),
    });
    scene.receive_plan();
    assert_eq!(scene.plan_pending, Some((2, [200, 100])));
    assert_eq!(scene.unavailable_size, None);
    assert!(scene.plan.is_none());
}

// Synthetic empty-map fixtures test owned capacity accounting and lifetime,
// never claim Minecraft decode or raster acceptance.
fn catalog_retained_fixture() -> Bundle {
    let budget = ByteBudget::new(SCENE_ACCOUNT).unwrap();
    let loaded = super::super::loader::LoadedWindow {
        retained_storage_charge: std::mem::size_of::<super::super::loader::LoadedWindow>() + 65536,
        ..Default::default()
    };
    let map = PreparedMap::new(
        super::super::evidence::Source {
            map: MapId([7; 16]),
            generation: 1,
        },
        0,
        Arc::new(loaded),
        Vec::with_capacity(8),
        &mut tours::Budget::new(16000000, &|| false),
    )
    .unwrap();
    Bundle {
        history: History::default(),
        maps: vec![Arc::new(map)],
        bindings: Vec::with_capacity(3),
        world_seeds: BTreeMap::new(),
        root: PathBuf::from("/synthetic/saves"),
        jar: PathBuf::from("/synthetic/client.jar"),
        warnings: vec![String::from("synthetic warning")],
        _catalog_reservation: Arc::new(
            budget
                .reserve(CATALOG_CHARGE, Cancel::new(&AtomicBool::new(false)))
                .unwrap(),
        ),
        budget,
    }
}
#[test]
fn catalog_retained_charge_covers_capacity_and_last_map_owner() {
    let mut bundle = catalog_retained_fixture();
    let accounted = bundle.retained_catalog_charge().unwrap();
    let baseline = std::mem::size_of::<Bundle>() + (1 << 20);
    assert!(accounted > baseline + bundle.maps[0].loaded().retained_storage_charge);
    let before = accounted;
    bundle.warnings[0].reserve(32768);
    assert!(bundle.retained_catalog_charge().unwrap() >= before + 32768 - 32);
    let retained = bundle.retained_catalog_charge().unwrap();
    Arc::get_mut(&mut bundle._catalog_reservation)
        .unwrap()
        .shrink_to(retained as u64)
        .unwrap();
    Arc::get_mut(&mut bundle.maps[0])
        .unwrap()
        .retain_catalog_charge(Arc::clone(&bundle._catalog_reservation))
        .unwrap();
    let budget = bundle.budget.clone();
    let map_owner = Arc::clone(&bundle.maps[0]);
    drop(bundle);
    assert_eq!(budget.used(), retained as u64);
    drop(map_owner);
    assert_eq!(budget.used(), 0);
}
#[test]
fn catalog_retained_charge_rejects_missing_loader_accounting() {
    let mut bundle = catalog_retained_fixture();
    let map = Arc::get_mut(&mut bundle.maps[0]).unwrap();
    // The fixture's sole owner can replace the loader before publication.
    *map = PreparedMap::new(
        super::super::evidence::Source {
            map: MapId([7; 16]),
            generation: 1,
        },
        0,
        Arc::new(super::super::loader::LoadedWindow::default()),
        Vec::new(),
        &mut tours::Budget::new(16000000, &|| false),
    )
    .unwrap();
    assert!(bundle.retained_catalog_charge().is_none());
}
