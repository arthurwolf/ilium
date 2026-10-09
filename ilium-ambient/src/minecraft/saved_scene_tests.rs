use super::*;
use crate::minecraft::settings::WorldSource;
use std::cell::RefCell;
use std::time::{Duration, Instant};

#[test]
fn allocated_proto_chunks_are_rejected_before_full_route_decode() {
    let support = BTreeSet::from([[-18, -7], [-18, -6]]);
    let allocated = BTreeSet::from([[-18, -7], [-18, -6]]);
    let full = BTreeSet::from([[-18, -6]]);

    assert!(
        !route_support_is_eligible(&support, &allocated, |position| {
            Ok(full.contains(&position))
        })
        .unwrap(),
        "an allocated chunk with Status=structure_starts must not reach full viewport decode"
    );
    assert!(route_support_is_eligible(&support, &allocated, |position| {
        Ok(support.contains(&position))
    })
    .unwrap());
    assert!(
        !route_support_is_eligible(&support, &BTreeSet::from([[-18, -6]]), |_| Ok(true)).unwrap()
    );
}

#[test]
fn saved_scene_status_exposes_phase_elapsed_time_and_unknown_eta_while_preparing() {
    let env = SceneEnv::for_test(
        std::env::temp_dir().join("saved-status-progress"),
        crate::resources::test_resources(),
    );
    let saved = SavedMapsSettings {
        source: WorldSource::SavedMaps,
        saves_folder: std::env::temp_dir()
            .join("ilium-saved-status-progress-empty")
            .display()
            .to_string(),
    };
    let scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);

    let status = scene.status().expect("preparing scene has a status report");
    let summary = status.lines().next().unwrap_or_default();
    assert!(
        summary.contains('['),
        "summary should show its progress bar: {summary}"
    );
    assert!(
        status.contains("Elapsed:"),
        "report should show elapsed time: {status}"
    );
    assert!(
        status.contains("ETA: unavailable"),
        "report should not invent an ETA without a measured work rate: {status}"
    );
}

#[test]
fn preparation_progress_is_bounded_and_keeps_phase_counts_monotonic() {
    let progress = PreparationProgress::new();
    progress.phase(
        4,
        "Checking projected route",
        "Reached projected route checks",
    );
    progress.phase(2, "Scanning maps", "A stale phase update arrived");
    for event in 0..40 {
        progress.record(&format!("test activity {event}"));
    }

    let report = progress.report();
    assert!(
        report.contains("phase 4/6"),
        "phase count regressed: {report}"
    );
    assert!(
        report.contains("ETA: unavailable"),
        "ETA must remain honest: {report}"
    );
    assert!(
        report.contains("test activity 8"),
        "oldest retained event missing: {report}"
    );
    assert!(
        report.contains("test activity 39"),
        "latest event missing: {report}"
    );
    assert!(
        !report.contains("test activity 7"),
        "event log exceeded its bound: {report}"
    );
}

#[test]
fn stale_viewport_progress_cannot_replace_the_new_request_activity() {
    let progress = PreparationProgress::new();
    let desired_plan = AtomicU64::new(0);
    progress.begin_request(
        1,
        &desired_plan,
        "Selecting the first viewport",
        "First viewport requested",
    );
    let first_viewport = progress.for_request(1);

    progress.begin_request(
        2,
        &desired_plan,
        "Selecting the current viewport",
        "Current viewport requested",
    );
    let current_viewport = progress.for_request(2);
    current_viewport.record("Current viewport is scanning its saved chunks");

    first_viewport.phase(3, "Stale viewport phase", "Stale viewport scan resumed");
    first_viewport.work(
        std::path::Path::new("/saves/Old World"),
        "Checking route coverage",
        9,
        10,
        "obsolete viewport work",
    );
    first_viewport.scan(
        std::path::Path::new("/saves/Old World"),
        ScanProgress {
            stage: ScanStage::ChunkSlots,
            completed: 512,
            total: Some(1_024),
        },
    );
    first_viewport.route_candidates(MAX_ROUTE_QUALIFICATIONS, MAX_ROUTE_QUALIFICATIONS);
    first_viewport.record("Obsolete viewport replaced the current activity");
    first_viewport.finish("Obsolete viewport incorrectly completed");
    first_viewport.fail("Obsolete viewport incorrectly failed");

    let report = progress.report();
    assert!(
        report.contains("Selecting the current viewport")
            && report.contains("Current viewport is scanning its saved chunks"),
        "the latest viewport activity should remain visible: {report}"
    );
    assert!(
        !report.contains("Stale viewport")
            && !report.contains("Old World")
            && !report.contains("Obsolete viewport")
            && !report.contains("Checking route coverage"),
        "callbacks from an older viewport must be ignored: {report}"
    );
    assert!(
        !report
            .lines()
            .next()
            .unwrap_or_default()
            .contains("overall 100%"),
        "a stale finish callback must not mark the current viewport complete: {report}"
    );
}

#[test]
fn preparation_overall_progress_does_not_regress_across_scan_stages_or_maps() {
    let progress = PreparationProgress::new();
    progress.phase(
        2,
        "Building the viewport's saved-chunk inventory",
        "Checking allocated chunks across saved maps",
    );

    let mut reports = Vec::new();
    for (map, stage, completed, total) in [
        ("First Save", ScanStage::SavedRootEntries, 64, None),
        ("First Save", ScanStage::RegionHeaders, 4, Some(10)),
        ("First Save", ScanStage::ChunkSlots, 0, Some(1_024)),
        ("First Save", ScanStage::CandidateWindows, 300, Some(2_048)),
        ("First Save", ScanStage::CandidateWindows, 300, Some(300)),
        ("First Save", ScanStage::ChunkPayloads, 0, Some(12)),
        ("Second Save", ScanStage::RegionDirectory, 0, None),
    ] {
        progress.scan(
            std::path::Path::new("/saves").join(map).as_path(),
            ScanProgress {
                stage,
                completed,
                total,
            },
        );
        reports.push(progress.report());
    }
    assert!(
        reports[0].contains("Listing Minecraft saves-root entries")
            && reports[0].contains("64 of at most 4096 entries")
            && reports[0].contains("minimum estimated overall")
            && reports[0].contains("unavailable until this directory ends"),
        "saved-folder enumeration should be visible in the progress report: {}",
        reports[0]
    );

    let percentages = reports
        .iter()
        .map(|report| {
            report
                .lines()
                .next()
                .and_then(|line| line.split_once("overall "))
                .and_then(|(_, suffix)| suffix.split_once('%'))
                .and_then(|(value, _)| value.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("overall progress percentage missing: {report}"))
        })
        .collect::<Vec<_>>();
    assert!(
        percentages[0] > 0,
        "measured scan work should advance overall progress: {reports:?}"
    );
    assert!(
        percentages.windows(2).all(|pair| pair[1] >= pair[0]),
        "overall progress regressed at a scan-stage or map boundary: {percentages:?}"
    );
    let state = progress
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let busy_report = progress.report();
    assert!(
        busy_report
            .lines()
            .next()
            .is_some_and(|line| line.contains(&format!("overall {}%", percentages[2]))),
        "a busy status mutex should retain the lock-free overall percentage: {busy_report}"
    );
    drop(state);
}

#[test]
fn failed_preparation_does_not_keep_an_eta_for_work_that_has_stopped() {
    let progress = PreparationProgress::new();
    progress.work(
        std::path::Path::new("/saves/Example"),
        "Reading chunk payloads",
        2,
        10,
        "Decoding saved chunks",
    );
    progress.route_candidates(3, MAX_ROUTE_QUALIFICATIONS);
    {
        let mut state = progress
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let work = state.work.as_mut().expect("current work was recorded");
        work.rate = Some(ScanRate {
            total: Some(10),
            started_at: Instant::now() - Duration::from_secs(5),
            starting_completed: 0,
            last_completed: 2,
        });
    }
    assert!(
        progress.report().contains("on this measured stage"),
        "running work should expose its measured stage estimate"
    );

    progress.fail("Saved-world preparation failed after the payload read");

    let report = progress.report();
    assert!(
        report.contains("ETA: unavailable"),
        "failed work must not retain a live ETA: {report}"
    );
    assert!(
        !report.contains("on this measured stage"),
        "failed work must not claim that the stopped stage is still progressing: {report}"
    );
    assert!(
        !report.contains("route candidates remain"),
        "failed work must not retain candidates from the stopped route: {report}"
    );
    assert!(
        report.contains("Saved-world preparation failed after the payload read"),
        "failure reason must remain in the activity history: {report}"
    );
}

#[test]
fn preparation_progress_restarts_for_a_new_route_selection_after_success() {
    let progress = PreparationProgress::new();
    progress.finish("First saved camera route passed viewport qualification");
    assert!(!progress.is_active());
    let finished_state = progress
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let first_started_at = finished_state.started_at;
    assert!(
        finished_state.finished_at.is_some(),
        "a completed route must retain its final elapsed-time boundary"
    );
    drop(finished_state);

    progress.phase(
        2,
        "Starting route selection for the current viewport",
        "Route selection requested for 200 by 100 cells",
    );

    assert!(
        progress.is_active(),
        "a new viewport route request must resume visible progress"
    );
    let report = progress.report();
    assert!(
        report.contains("phase 2/6"),
        "new route selection must not inherit the completed route's 6/6 count: {report}"
    );
    assert!(
        report.contains("Starting route selection for the current viewport"),
        "the visible phase should identify the active viewport request: {report}"
    );
    let state = progress
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(
        state.started_at > first_started_at,
        "a resumed route must start a fresh elapsed-time clock"
    );
    assert!(
        state.finished_at.is_none(),
        "an active route must not retain the previous route's finished time"
    );
    assert!(
        state
            .events
            .iter()
            .all(|(_, event)| !event.contains("First saved camera route")),
        "the new route activity log must not retain the completed route's event"
    );
}

#[test]
fn busy_history_runtime_keeps_the_live_preparation_log_advancing() {
    let temporary = tempfile::tempdir().unwrap();
    let saves = temporary.path().join("saves");
    std::fs::create_dir(&saves).unwrap();
    let env = SceneEnv::for_test(
        temporary.path().join("cache"),
        crate::resources::test_resources(),
    );
    let runtime = Arc::clone(&env.saved_runtime);
    let (locked_tx, locked_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let lock_runtime = Arc::clone(&runtime);
    let lock_worker = std::thread::spawn(move || {
        lock_runtime.with_test_lock(|| {
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
    });
    locked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("test should hold the saved-history runtime lock");

    let saved = SavedMapsSettings {
        source: WorldSource::SavedMaps,
        saves_folder: saves.display().to_string(),
    };
    let scene = SavedScene::new(&saved, &VoxelLandscapeSettings::default(), &env);
    std::thread::sleep(Duration::from_secs(11));
    let report = scene
        .status()
        .expect("waiting saved-world preparation remains visible");
    drop(scene);
    release_tx.send(()).unwrap();
    lock_worker.join().unwrap();

    assert!(
        report
            .matches("Saved history runtime remains busy after")
            .count()
            >= 2,
        "the live log should repeat a timed explanation while history remains busy: {report}"
    );
}

#[test]
fn preparation_eta_uses_measured_stage_work_and_never_formats_an_infinite_rate() {
    let now = Instant::now();
    let started_at = now.checked_sub(Duration::from_secs(5)).unwrap();
    let scan = ScanStatus {
        map: "Example Save".into(),
        stage: ScanStage::ChunkSlots,
        completed: 200,
        total: Some(1_000),
        stage_rates: [
            None,
            None,
            None,
            Some(ScanRate {
                total: Some(1_000),
                started_at,
                starting_completed: 100,
                last_completed: 200,
            }),
            None,
            None,
        ],
        last_logged_fraction: 0,
        last_logged_at: now,
    };

    assert_eq!(
        measured_scan_eta(&scan, now),
        Some((Duration::from_secs(40), Some(20.0)))
    );

    let completed = ScanStatus {
        completed: 1_000,
        ..scan
    };
    assert_eq!(
        measured_scan_eta(&completed, now),
        Some((Duration::ZERO, None))
    );
}

#[test]
fn preparation_report_shows_a_measured_stage_estimate_and_unestimated_later_work() {
    let progress = PreparationProgress::new();
    progress.work(
        std::path::Path::new("/saves/Example Save"),
        "Checking route coverage",
        200,
        1_000,
        "Comparing candidate camera views",
    );
    {
        let mut state = progress
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let work = state.work.as_mut().expect("current work was recorded");
        work.rate = Some(ScanRate {
            total: Some(1_000),
            started_at: Instant::now() - Duration::from_secs(5),
            starting_completed: 100,
            last_completed: 200,
        });
    }

    let report = progress.report();
    assert!(
        report.contains("ETA: about ") && report.contains("for this measured stage (20 items/s)"),
        "report should quantify the known remaining work without implying a full ETA: {report}"
    );
    assert!(
        report.contains("later stages are unestimated"),
        "report should name uncertainty from unmeasured stages: {report}"
    );
}

#[test]
fn preparation_report_labels_an_incomplete_total_eta_and_remaining_route_candidates() {
    let progress = PreparationProgress::new();
    progress.phase(
        4,
        "Decoding and projecting the selected route",
        "Validating a saved camera route",
    );
    progress.route_candidates(3, MAX_ROUTE_QUALIFICATIONS);
    progress.work(
        std::path::Path::new("/saves/Example Save"),
        "Decoding route chunks",
        2,
        10,
        "Projecting the current candidate into the viewport",
    );
    {
        let mut state = progress
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let work = state.work.as_mut().expect("current work was recorded");
        work.rate = Some(ScanRate {
            total: Some(10),
            started_at: Instant::now() - Duration::from_secs(5),
            starting_completed: 0,
            last_completed: 2,
        });
    }

    let report = progress.report();
    assert!(
        report.contains("Total ETA: incomplete; about 00:20 for the measured stage"),
        "the total estimate should label the measured current-stage remainder as an estimate: {report}"
    );
    assert!(
        report.contains("up to 13 route candidates remain without a measured duration"),
        "the report should expose finite remaining route qualification work and its uncertainty: {report}"
    );
}

#[test]
fn preparation_report_keeps_total_eta_unavailable_until_a_rate_is_measured() {
    let progress = PreparationProgress::new();
    progress.phase(
        3,
        "Surveying camera-route candidates",
        "Starting the finite route survey",
    );
    progress.route_candidates(0, MAX_ROUTE_QUALIFICATIONS);

    let report = progress.report();
    assert!(
        report.contains("Total ETA: unavailable until a remaining stage has a measured rate"),
        "the total ETA must remain unavailable without an observed rate: {report}"
    );
    assert!(
        report.contains("up to 16 route candidates remain"),
        "the finite candidate ceiling should remain visible while the ETA is unavailable: {report}"
    );
}

#[test]
fn preparation_report_estimates_total_eta_from_qualified_overall_progress() {
    let progress = PreparationProgress::new();
    let too_early_started_at = Instant::now() - Duration::from_secs(20);
    {
        let mut state = progress
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.started_at = too_early_started_at;
        state.finished_at = Some(too_early_started_at + Duration::from_secs(20));
    }
    progress.overall_percent.store(25, Ordering::Relaxed);
    assert!(
        progress
            .report()
            .contains("Total ETA: unavailable until overall progress is measured"),
        "a short sample must not produce an unstable whole-run estimate"
    );

    let started_at = Instant::now() - Duration::from_secs(120);
    {
        let mut state = progress
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.started_at = started_at;
        state.finished_at = Some(started_at + Duration::from_secs(120));
        state.completed_phases = 1;
    }
    progress.overall_percent.store(9, Ordering::Relaxed);
    assert!(
        progress
            .report()
            .contains("Total ETA: unavailable until overall progress is measured"),
        "early weighted progress must not be extrapolated into a total estimate"
    );
    progress.overall_percent.store(25, Ordering::Relaxed);

    let report = progress.report();
    assert!(
        report.contains("Total ETA: rough estimate 06:00 remaining from 25% estimated overall progress"),
        "once enough overall work is measured, users need a provisional whole-run estimate: {report}"
    );
    assert!(
        report.contains("save size and route qualification can change this estimate"),
        "the report must explain why the whole-run estimate can move: {report}"
    );
}

#[test]
fn preparation_report_estimates_remaining_route_candidates_after_one_measured_candidate() {
    let progress = PreparationProgress::new();
    progress.phase(
        4,
        "Decoding and projecting the selected route",
        "Validating a saved camera route",
    );
    progress.route_candidates(0, 4);
    let no_sample = progress.report();
    assert!(
        no_sample.contains("Route ETA: unavailable until one candidate completes"),
        "route estimate must remain unavailable before a completed candidate: {no_sample}"
    );

    progress.route_candidates(1, 4);
    {
        let mut state = progress
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rate = state
            .route_candidates
            .as_mut()
            .and_then(|candidates| candidates.rate.as_mut())
            .expect("one completed candidate starts the route sample");
        rate.started_at = Instant::now() - Duration::from_secs(5);
    }
    progress.route_candidates(2, 4);
    let measured = progress.report();
    assert!(
        measured.contains("Route ETA: about "),
        "one completed route candidate should establish a provisional estimate: {measured}"
    );
    assert!(
        measured.contains("based on 1 completed candidate"),
        "the estimate must disclose its measured sample count: {measured}"
    );
    assert!(
        measured.contains("assuming all 3 remaining candidate checks still need qualification"),
        "the estimate must include the in-progress candidate and disclose its upper-bound assumption: {measured}"
    );

    progress.route_candidates(4, 4);
    let final_candidate = progress.report();
    assert!(
        final_candidate.contains("assuming all 1 remaining candidate checks still need qualification"),
        "the final in-progress candidate must not be reported as already complete: {final_candidate}"
    );
}

#[test]
fn preparation_report_names_measured_chunk_payload_decoding() {
    let progress = PreparationProgress::new();
    progress.scan(
        std::path::Path::new("/saves/Example Save"),
        ScanProgress {
            stage: ScanStage::ChunkPayloads,
            completed: 2,
            total: Some(5),
        },
    );

    let report = progress.report();
    assert!(
        report.contains("Decoding map chunks for Example Save: 2/5 items (40%)"),
        "report should expose actual payload decode progress: {report}"
    );
}

#[test]
fn preparation_report_names_candidate_search_work_units() {
    let progress = PreparationProgress::new();
    progress.scan(
        std::path::Path::new("/saves/Example Save"),
        ScanProgress {
            stage: ScanStage::CandidateWindows,
            completed: 256,
            total: Some(1_024),
        },
    );

    let report = progress.report();
    assert!(
        report.contains(
            "Checking saved-world candidate-search work units for Example Save: 256/1024 items (25%)"
        ),
        "report should expose bounded candidate-search work: {report}"
    );
}

#[test]
fn preparation_report_drives_the_progress_bar_from_scanned_work() {
    let progress = PreparationProgress::new();
    progress.scan(
        std::path::Path::new("/saves/Example Save"),
        ScanProgress {
            stage: ScanStage::ChunkSlots,
            completed: 500,
            total: Some(1_000),
        },
    );

    let report = progress.report();
    assert!(
        report.contains("overall 10% · phase 0/6"),
        "scan work should advance the whole-preparation progress bar: {report}"
    );
    assert!(
        report.contains("Indexing chunk slots for Example Save: 500/1000 items (50%)"),
        "report should identify the current measured stage: {report}"
    );
}

#[test]
fn preparation_report_tracks_selected_texture_import_work() {
    let progress = PreparationProgress::new();
    progress.work(
        std::path::Path::new("/saves/Example Save"),
        "Loading selected texture assets",
        37,
        100,
        "currently minecraft:block/stone",
    );

    let report = progress.report();
    assert!(
        report.contains("overall 6% · phase 0/6"),
        "measured pack work should advance whole-preparation progress: {report}"
    );
    assert!(
        report.contains("Loading selected texture assets for Example Save: 37/100 items (37%)"),
        "report should identify the measured pack-import stage: {report}"
    );
    assert!(
        report.contains("currently minecraft:block/stone"),
        "report should identify the texture currently being loaded: {report}"
    );
    assert!(
        report.contains("later stages are unestimated"),
        "a stage ETA must identify work that remains unmeasured: {report}"
    );
}

#[test]
fn route_survey_progress_accumulates_across_candidate_passes() {
    let progress = PreparationProgress::new();
    let saves_root = std::path::Path::new("/home/example/.minecraft/saves");
    progress.phase(
        3,
        "Surveying camera-route candidates",
        "Starting the finite route survey",
    );
    progress.work(
        saves_root,
        "Checking route coverage",
        30_000_000,
        MAX_SELECTION_WORK as usize,
        "Pass 1 of 16: comparing candidate camera views",
    );
    let first_pass = progress.report();
    assert!(
        first_pass.contains("overall 50% · phase 3/6"),
        "route survey should include bounded work in whole-preparation progress: {first_pass}"
    );

    progress.work(
        saves_root,
        "Checking route coverage",
        33_000_000,
        MAX_SELECTION_WORK as usize,
        "Pass 2 of 16: comparing candidate camera views",
    );
    let next_pass = progress.report();
    assert!(
        next_pass.contains("overall 51% · phase 3/6"),
        "route survey progress should continue across candidate passes: {next_pass}"
    );
    assert!(
        next_pass.contains("Pass 2 of 16"),
        "route survey detail should identify its current bounded pass: {next_pass}"
    );
}

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
    // A finite survey can find no eligible next line even though the last
    // saved-world frame is valid. Keep showing that confirmed frame until a
    // later viewport or tour can replace it; replay carries no display owners.
    scene.plan_pending = None;
    scene.unavailable_size = Some([4, 4]);
    frame.raster.dots.fill(0.0);
    frame.raster.owner_ids.fill(0);
    frame.cell_colors.fill([0; 3]);
    scene.render(&mut frame);
    assert_eq!(frame.raster.dots[3], 0.75);
    assert_eq!(frame.cell_colors[0], [20, 30, 40]);
    assert!(frame.raster.owner_ids.iter().all(|owner| *owner == 0));
    assert!(scene.receipt.is_none());
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
        selected: None,
        native_jar: None,
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

#[cfg(target_os = "linux")]
fn pinned_admission_source(
    temporary: &tempfile::TempDir,
    runtime: Arc<SavedRuntime>,
) -> PinnedSceneSource {
    use ilium_platform::{animation_files::PinnedDirectory, secure_fs::NoFollowDirectory};
    let label = temporary.path().join("selected-root");
    std::fs::create_dir(&label).unwrap();
    std::fs::write(
        label.join("separate-native-archive.jar"),
        b"synthetic admission-only file",
    )
    .unwrap();
    let root = Arc::new(
        PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(&label).unwrap()))
            .unwrap(),
    );
    let native_jar = Arc::new(root.open_file("separate-native-archive.jar").unwrap());
    PinnedSceneSource {
        root_label: label,
        selected_world: None,
        selected_identity: None,
        root,
        native_jar,
        history_storage: temporary.path().join("host-history"),
        history_root: None,
        stop: None,
        runtime,
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pinned_scene_refuses_invalid_history_before_worker_or_runtime_retirement() {
    let temporary = tempfile::tempdir().unwrap();
    let env = SceneEnv::for_test(
        temporary.path().join("cache"),
        crate::resources::test_resources(),
    );
    let mut source = pinned_admission_source(&temporary, Arc::clone(&env.saved_runtime));
    source.history_storage = PathBuf::from("relative-history");
    let scene =
        SavedScene::new_source(None, Some(source), &VoxelLandscapeSettings::default(), &env);
    assert!(scene.worker.is_none());
    assert!(!scene.history_retirement_owned);
    let error = match scene.take_admitted() {
        Ok(_) => panic!("unadmitted scene published"),
        Err(error) => error,
    };
    assert!(error.contains("absolute"));
    assert!(!temporary.path().join("host-history").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn pinned_scene_refuses_shared_storage_exhaustion_before_worker_or_history() {
    let temporary = tempfile::tempdir().unwrap();
    let (_execution, resources) = crate::resources::isolated_test_resources();
    let env = SceneEnv::for_test(temporary.path().join("cache"), resources);
    // Existing host-owned credit leaves less than the original 1 GiB scene account.
    let _occupied = env
        .resources
        .reserve_storage(2 * 1024 * 1024 * 1024)
        .unwrap();
    let source = pinned_admission_source(&temporary, Arc::clone(&env.saved_runtime));
    let result = SavedScene::new_pinned(source, &VoxelLandscapeSettings::default(), &env);
    let error = match result {
        Ok(_) => panic!("unadmitted scene published"),
        Err(error) => error,
    };
    assert!(error.contains("storage admission rejected"));
    assert!(!temporary.path().join("host-history").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn pinned_scene_original_request_stop_refuses_before_worker_storage_or_history_retirement() {
    let temporary = tempfile::tempdir().unwrap();
    let env = SceneEnv::for_test(
        temporary.path().join("cache"),
        crate::resources::test_resources(),
    );
    let mut source = pinned_admission_source(&temporary, Arc::clone(&env.saved_runtime));
    let original = ilium_platform::owned_worker::StopToken::default();
    source.stop = Some(original.child());
    original.stop();
    let before = env.resources.finite().quota_group().snapshot().worker_bytes;
    let scene =
        SavedScene::new_source(None, Some(source), &VoxelLandscapeSettings::default(), &env);
    assert!(scene.worker.is_none());
    assert!(!scene.history_retirement_owned);
    assert!(matches!(env.saved_runtime.gate(), Gate::Ready));
    let reason = match scene.take_admitted() {
        Ok(_) => panic!("cancelled scene published"),
        Err(reason) => reason,
    };
    assert!(reason.contains("already cancelled"));
    assert_eq!(
        env.resources.finite().quota_group().snapshot().worker_bytes,
        before
    );
    assert!(matches!(env.saved_runtime.gate(), Gate::Ready));
    assert!(!temporary.path().join("host-history").exists());
}
