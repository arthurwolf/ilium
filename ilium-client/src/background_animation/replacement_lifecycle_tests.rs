// Regression tests for renderer ownership at the existing loop-cache boundary.
// These test complete frames after pause/replacement, not interruption within
// one renderer call (that remains the separately allocated G04 mechanism).
use super::*;
use crate::background_animation::{AnimationKind, scenes};

const KINDS: &[AnimationKind] = &[
    AnimationKind::Cloudlets,
    AnimationKind::QuietPond,
    AnimationKind::StoneCaustics,
];

fn oracle(
    settings: &AnimationSettings,
    width: u16,
    height: u16,
    elapsed: Duration,
) -> AnimationFrame {
    match settings.kind {
        AnimationKind::Cloudlets => {
            scenes::r05_cloudlet_tests::reference_frame(settings, width, height, elapsed)
        }
        AnimationKind::QuietPond => {
            scenes::r06_pond_tests::reference_frame(settings, width, height, elapsed)
        }
        AnimationKind::StoneCaustics => {
            scenes::r07_caustic_tests::reference_frame(settings, width, height, elapsed)
        }
        _ => panic!("missing frozen renderer oracle"),
    }
}

fn changed_settings(kind: AnimationKind) -> AnimationSettings {
    let mut settings = AnimationSettings {
        kind,
        loop_seconds: 1,
        speed_percent: 175,
        density_percent: 85,
        ..Default::default()
    };
    match kind {
        AnimationKind::Cloudlets => settings.cloudlets.form_count = 9,
        AnimationKind::QuietPond => {
            settings.quiet_pond.pad_count = 24;
            settings.quiet_pond.natural_placement = true;
        }
        AnimationKind::StoneCaustics => {
            settings.stone_caustics.caustic_scale_percent = 150;
            settings.stone_caustics.dome_height_percent = 175;
        }
        _ => panic!("missing replacement settings"),
    }
    settings
}

fn expected_sample(settings: &AnimationSettings, width: u16, height: u16, index: usize) -> Vec<u8> {
    // Independent copy of R04's complete loop/crossfade formula.
    let time = index as f64 / f64::from(CACHE_FPS);
    let duration = f64::from(settings.loop_seconds);
    let window = (duration / 4.0).min(2.0);
    let mut frame = oracle(
        settings,
        width,
        height,
        Duration::from_secs_f64(time + window),
    );
    let tail_time = time - (duration - window);
    if tail_time >= 0.0 {
        let head = oracle(settings, width, height, Duration::from_secs_f64(tail_time));
        let fraction = (tail_time / window) as f32;
        let weight = fraction * fraction * (3.0 - 2.0 * fraction);
        for (dot, target) in frame.raster.dots.iter_mut().zip(&head.raster.dots) {
            *dot = *dot * (1.0 - weight) + target * weight;
        }
        frame.pack(crate::background_animation::PackKey::of(settings));
    }
    frame.packed_cells().to_vec()
}

fn wait_ready(
    cache: &mut AnimationLoopCache,
    settings: &AnimationSettings,
    width: u16,
    height: u16,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cache.step(settings, width, height, 1) {
        assert!(!cache.status().has_error, "builder failed");
        assert!(
            !cache.status().is_limited,
            "small test fixture must be admitted"
        );
        assert!(
            Instant::now() < deadline,
            "builder admission/completion timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn replacements_pause_then_build_exact_new_generation_and_ready_copies() {
    for &kind in KINDS {
        let old = AnimationSettings {
            kind,
            loop_seconds: 120,
            ..Default::default()
        };
        let mut cache = AnimationLoopCache::new(ilium_ambient::resources::AmbientResources::new(
            crate::execution::test_client(),
        ));
        cache.begin(&old, 80, 24);
        // Never poll the old receiver: whether it has started, is rendering, or
        // has already sent, pause must detach it from all future publication.
        cache.pause();
        assert!(cache.build.is_none());
        assert!(!cache.status().is_ready);
        assert!(cache.frames.is_empty());
        let mut display = AnimationFrame::default();
        assert!(!cache.copy_frame_into(Duration::ZERO, &mut display));
        let settings = changed_settings(kind);
        wait_ready(&mut cache, &settings, 9, 5);
        assert_eq!((cache.width, cache.height), (9, 5));
        assert_eq!(cache.frames.len(), 30);
        for (index, cells) in cache.frames.iter().enumerate() {
            assert_eq!(
                cells,
                &expected_sample(&settings, 9, 5, index),
                "{kind:?} frame {index}"
            );
        }
        let resident = cache.frames.capacity() * std::mem::size_of::<Vec<u8>>()
            + cache.frames.iter().map(Vec::capacity).sum::<usize>();
        assert_eq!(cache.status().resident_bytes, resident);
        for nanos in [
            0,
            33_333_333,
            33_333_334,
            999_999_999,
            1_000_000_000,
            7_000_000_000,
        ] {
            let elapsed = Duration::from_nanos(nanos);
            let index = cache.frame_index(elapsed);
            assert!(cache.copy_frame_into(elapsed, &mut display));
            assert_eq!(display.packed_cells(), cache.frames[index].as_slice());
        }
        assert_eq!(display.geometry_render_count, 0);
        assert_eq!(display.scene_cache.preparations, 0);
        // Hidden ready playback remains reusable; no new renderer state is built.
        cache.pause();
        assert!(cache.status().is_ready);
        assert!(cache.copy_frame_into(Duration::ZERO, &mut display));
    }
}

#[test]
fn replacements_discard_a_completed_but_unpolled_old_generation() {
    for &kind in KINDS {
        let old = AnimationSettings {
            kind,
            loop_seconds: 1,
            ..Default::default()
        };
        let mut cache = AnimationLoopCache::new(ilium_ambient::resources::AmbientResources::new(
            crate::execution::test_client(),
        ));
        let deadline = Instant::now() + Duration::from_secs(20);
        // start_build retries admission without consuming the generation receiver.
        cache.begin(&old, 8, 4);
        loop {
            cache.start_build();
            if cache.status().completed_frames == 30 {
                break;
            }
            assert!(!cache.status().has_error);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!cache.status().is_ready);
        assert!(cache.frames.is_empty());
        let settings = changed_settings(kind);
        // The old sender may now be queued or just about to send. Both races
        // are safe only because begin drops the old generation's receiver.
        wait_ready(&mut cache, &settings, 7, 3);
        for (index, cells) in cache.frames.iter().enumerate() {
            assert_eq!(
                cells,
                &expected_sample(&settings, 7, 3, index),
                "{kind:?} frame {index}"
            );
        }
        assert_eq!(cache.frames.len(), 30);
        assert!(cache.frames.iter().all(|cells| cells.len() == 21));
    }
}

#[test]
fn cancelling_a_build_retires_its_owned_worker_before_release_is_reported() {
    let mut cache = AnimationLoopCache::new(ilium_ambient::resources::AmbientResources::new(
        crate::execution::test_client(),
    ));
    // ClientExecution bootstrap owns several platform workers; snapshot only
    // after that shared test fixture exists so this test measures its delta.
    let before = ilium_platform::owned_worker::supervisor_status()
        .expect("the shared test client initializes the worker supervisor");
    let baseline_registered = before.registered_workers;
    let baseline_reserved = before.reserved_workers;
    let baseline_builders = BUILDERS.load(Ordering::Acquire);
    let settings = AnimationSettings {
        kind: AnimationKind::Kelp,
        loop_seconds: 120,
        ..Default::default()
    };

    // Stay just under the 128 MiB admission cap while using a large raster and
    // long cache so cancellation leaves real work for the owned worker.
    cache.begin(&settings, 256, 128);
    assert!(
        !cache.status().is_limited,
        "fixture must admit a cache worker"
    );
    let launch_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = ilium_platform::owned_worker::supervisor_status()
            .expect("the worker supervisor remains installed after launch");
        if current.registered_workers > baseline_registered {
            assert_eq!(
                current.registered_workers,
                baseline_registered + 1,
                "isolated cache fixture must add exactly one owned worker"
            );
            break;
        }
        assert!(
            Instant::now() < launch_deadline,
            "cache worker was not admitted"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    while cache.status().completed_frames == 0 {
        assert!(!cache.status().has_error, "cache builder failed");
        assert!(
            Instant::now() < launch_deadline,
            "cache worker did not render a frame"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    cache.pause();
    assert_eq!(cache.status().resident_bytes, 0);
    let retirement_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = ilium_platform::owned_worker::supervisor_status()
            .expect("the worker supervisor remains installed after cancellation");
        if current.registered_workers == baseline_registered
            && current.reserved_workers == baseline_reserved
            && current.joining_worker == before.joining_worker
            && BUILDERS.load(Ordering::Acquire) == baseline_builders
        {
            break;
        }
        assert!(
            Instant::now() < retirement_deadline,
            "cache worker or its builder permit must remain accounted until real retirement"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
