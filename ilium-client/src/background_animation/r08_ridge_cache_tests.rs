use super::*; // Exact cache.rs private state and render_loop_sample.
use crate::background_animation::scenes::r08_ridge_oracle::{reference_frame, same_frame}; // Independent frozen original geometry.
use crate::background_animation::test_support::{fake_host, FakeProbe}; // Supplied no-I/O hosted scene.
use crate::background_animation::{AnimationKind, DitherMode, PackKey, SleepingRidgeSettings}; // Existing concrete controls.
                                                                                              //
fn loop_settings() -> AnimationSettings {
    // Fixed one-second Ridge loop fixture.
    AnimationSettings {
        enabled: true,
        kind: AnimationKind::SleepingRidge,
        loop_seconds: 1,
        ..Default::default()
    } // Default Loop, speed100, density60, Ordered.
} // End fixture.
fn reference_loop(
    settings: &AnimationSettings,
    width: u16,
    height: u16,
    index: usize,
) -> AnimationFrame {
    // Frozen cache seam expressions with scalar-original geometry.
    let time = index as f64 / f64::from(CACHE_FPS); // Original sample coordinate.
    let duration = f64::from(settings.loop_seconds); // Original duration conversion.
    let window = (duration / 4.0).min(2.0); // Original crossfade window.
    let mut generator = reference_frame(
        settings,
        width,
        height,
        Duration::from_secs_f64(time + window),
    ); // Independent main frame.
    let tail_time = time - (duration - window); // Original tail coordinate.
    if tail_time < 0.0 {
        return generator;
    } // Original no-blend branch.
    let head = reference_frame(settings, width, height, Duration::from_secs_f64(tail_time)); // Independent original head frame.
    let fraction = (tail_time / window) as f32; // Original precision boundary.
    let weight = fraction * fraction * (3.0 - 2.0 * fraction); // Original easing association.
    for (dot, target) in generator.raster.dots.iter_mut().zip(&head.raster.dots) {
        *dot = *dot * (1.0 - weight) + target * weight;
    } // Original blend order.
    generator.pack(PackKey::of(settings)); // Unchanged packer after original float blend.
    generator // No production render_loop_sample call in this oracle.
} // End loop oracle.
fn wait_ready(
    cache: &mut AnimationLoopCache,
    settings: &AnimationSettings,
    width: u16,
    height: u16,
) {
    // Finite test-only worker wait.
    let deadline = Instant::now() + Duration::from_secs(10); // Existing suite's watchdog, not a performance threshold.
    while !cache.step(settings, width, height, 1) {
        // Same production poll API as the supplied tests.
        let status = cache.status(); // Inspect actual failure/admission state.
        assert!(
            !status.has_error && !status.is_limited,
            "cache failed or limited: {status:?}"
        ); // Never accept fallback bytes.
        assert!(Instant::now() < deadline, "Ridge cache did not complete"); // No retry or timeout extension.
        std::thread::sleep(Duration::from_millis(1)); // Test-only polling, no UI wait.
    } // End finite polling.
} // End ready barrier.
fn check_ready(cache: &AnimationLoopCache, settings: &AnimationSettings, width: u16, height: u16) {
    // Full cached payload comparison.
    assert_eq!((cache.width, cache.height), (width, height)); // Exact generation dimensions.
    assert_eq!(cache.frames.len(), 30); // One second at the original rate.
    for (index, cells) in cache.frames.iter().enumerate() {
        // No favorable frame subset.
        assert_eq!(cells.len(), usize::from(width) * usize::from(height)); // Complete packed frame length.
        assert_eq!(
            cells,
            reference_loop(settings, width, height, index).packed_cells()
        ); // Every cell against original loop geometry.
    } // End frames.
    let resident = cache.frames.capacity() * std::mem::size_of::<Vec<u8>>()
        + cache.frames.iter().map(Vec::capacity).sum::<usize>(); // Actual packed allocation capacities.
    assert_eq!(cache.status().resident_bytes, resident); // Do not include Ridge scratch or call this RSS.
} // End cached payload check.
#[test] // Runs on both original and candidate arms.
fn r08_ridge_loop_raw_bits_and_ready_ownership() {
    // Exact seam rendering plus real worker generation and host retirement.
    let mut settings = loop_settings(); // Fixed initial loop controls.
    for (width, height) in [(1, 1), (17, 9), (80, 24)] {
        // Tiny, odd and required small terminal sizes.
        for mode in [DitherMode::Ordered, DitherMode::Stippled] {
            // Both frozen matrix modes.
            settings.dither = mode; // Actual pack control.
            let (mut generator, mut head) = (AnimationFrame::default(), AnimationFrame::default()); // Reused real generator pair.
            for index in 0..30 {
                // Include all tail/crossfade frames.
                render_loop_sample(&mut generator, &mut head, &settings, width, height, index); // Real production loop sampling.
                same_frame(&reference_loop(&settings, width, height, index), &generator);
                // Every raw float, owner and packed byte.
            } // End loop.
            assert_eq!(generator.scene_cache.preparations, 1); // Main frames share one cold preparation.
            assert_eq!(head.scene_cache.preparations, 1); // Tail head has its own one cold preparation.
        } // End modes.
    } // End sizes.
    let settings = loop_settings(); // Restore Ordered defaults for actual worker checks.
    let resources =
        ilium_ambient::resources::AmbientResources::new(crate::execution::test_client());
    let mut cache = AnimationLoopCache::new(resources); // Real worker-owned loop cache.
    wait_ready(&mut cache, &settings, 17, 9); // Complete exactly one finite generation.
    check_ready(&cache, &settings, 17, 9); // Strong cached-byte and capacity checks.
    let probe = FakeProbe::new(); // No native host resources are opened.
    probe.uses_colors.store(true, Ordering::SeqCst); // Force colored-host metadata to be observable.
    let mut frame = AnimationFrame::default(); // Start with no Ridge preparation.
    *frame.host_mut() = fake_host(&probe); // Install the supplied fake factory.
    let hosted = AnimationSettings {
        kind: AnimationKind::Pipes,
        ..Default::default()
    }; // Concrete hosted scene using the fake.
    frame.render(&hosted, 17, 9, Duration::ZERO); // Construct and render the fake host.
    assert_eq!(probe.alive(), 1); // Real lifecycle precondition.
    frame.raster.owner_ids.fill(41); // Seed foreign frame-specific provenance.
    frame.composed(frame.packed_cells().to_vec()); // Seed a valid hosted composition key.
    assert!(frame.composed_key.is_some()); // Ensure the stale-key guard is exercised.
    for index in [0_u64, 1, 29, 30, 31, 59] {
        // First, last and wrapped frame buckets.
        let time = Duration::from_nanos((index * 1_000_000_000).div_ceil(30)); // Exact cached bucket addressing.
        assert!(cache.copy_frame_into(time, &mut frame)); // Copy the actual ready cache.
        assert_eq!(
            frame.packed_cells(),
            cache.frames[index as usize % 30].as_slice()
        ); // Exact selected frame, not merely nonempty output.
        assert!(frame.raster.owner_ids.iter().all(|owner| *owner == 0)); // No hosted provenance reaches built-in cached ink.
        assert!(
            frame.last_geometry.is_none()
                && frame.last_ambient.is_none()
                && frame.composed_key.is_none()
        ); // Old render/receipt identities are invalid.
        assert!(!frame.has_cell_colors()); // No stale hosted colors.
    } // End copy sequence.
    assert_eq!(probe.alive(), 0); // Ready built-in copy drops the hosted instance.
    assert_eq!(probe.dropped.load(Ordering::SeqCst), 1); // Exactly one retirement.
    assert_eq!(frame.geometry_render_count, 0); // Copies never render geometry.
    assert_eq!(frame.scene_cache.preparations, 0); // Copies never build Ridge textures.
    frame.render(&settings, 17, 9, Duration::ZERO); // Return from packed playback to the public live render path.
    same_frame(&reference_frame(&settings, 17, 9, Duration::ZERO), &frame); // No cached raw-dot assumptions leak into rendering.
    frame.raster.owner_ids.fill(99); // Exercise partial-input clearing independently.
    frame.load_packed_cells(17, 9, &[3, 5]); // Existing partial-input API must zero the tail.
    assert_eq!(&frame.packed_cells()[..2], &[3, 5]); // Exact supplied prefix.
    assert!(frame.packed_cells()[2..].iter().all(|cell| *cell == 0)); // No old cached tail.
    assert!(frame.raster.owner_ids.iter().all(|owner| *owner == 0)); // No stale owner attribution.
    let preparations = frame.scene_cache.preparations; // Retained Ridge preparation survives a hosted detour.
    frame.render(&hosted, 17, 9, Duration::from_secs(1)); // Recreate the fake host without replacing SceneCache.
    assert_eq!(probe.alive(), 1); // Direct hosted-to-live transition precondition.
    frame.raster.owner_ids.fill(57); // Seed fresh hosted provenance.
    frame.composed(frame.packed_cells().to_vec()); // Seed a current hosted receipt.
    assert!(frame.composed_key.is_some()); // The invalidation test is nonvacuous.
    frame.render(&settings, 17, 9, Duration::ZERO); // Direct hosted-to-live Ridge return.
    assert_eq!(frame.scene_cache.preparations, preparations); // Reuse the still-valid static Ridge preparation.
    assert_eq!(probe.alive(), 0); // The recreated host is retired.
    assert_eq!(probe.dropped.load(Ordering::SeqCst), 2); // Neither host lifetime leaks or drops twice.
    assert!(frame.composed_key.is_none() && frame.last_ambient.is_none()); // Hosted receipt identity cannot survive.
    same_frame(&reference_frame(&settings, 17, 9, Duration::ZERO), &frame); // Original live bits and cleared owners.
} // End loop and ownership test.
#[test] // Runs on both original and candidate arms.
fn r08_ridge_loop_control_replacement_and_pause() {
    // Cache reuse, every Ridge key field and replacement generation.
    let base = loop_settings(); // Canonical default controls.
    let resources =
        ilium_ambient::resources::AmbientResources::new(crate::execution::test_client());
    let mut cache = AnimationLoopCache::new(resources); // Fresh real cache.
    wait_ready(&mut cache, &base, 11, 5); // Complete the original generation.
    let pointer = cache.frames.as_ptr(); // Actual ready-frame container identity.
    let mut presentation = base.clone(); // Presentation edits must reuse packed geometry.
    presentation.enabled = false; // Stripped by cache key normalization.
    presentation.hue_degrees = 93; // Stripped palette field.
    presentation.appearance.brightness_percent = 40; // Stripped look field.
    cache.begin(&presentation, 11, 5); // Same effective packed generation.
    assert!(cache.status().is_ready && cache.build.is_none()); // No replacement worker.
    assert_eq!(cache.frames.as_ptr(), pointer); // Exact retained frame storage.
    cache.pause(); // A ready cache remains available while hidden.
    assert!(cache.status().is_ready); // Preserve ready state.
    assert_eq!(cache.frames.as_ptr(), pointer); // Preserve allocation identity.
    for controls in [
        SleepingRidgeSettings {
            cloud_cover_percent: 0,
            ..Default::default()
        },
        SleepingRidgeSettings {
            mist_percent: 0,
            ..Default::default()
        },
        SleepingRidgeSettings {
            ridge_height_percent: 150,
            ..Default::default()
        },
        SleepingRidgeSettings {
            drift_percent: 200,
            ..Default::default()
        },
    ] {
        // Every selected control changes the generation.
        let next = AnimationSettings {
            sleeping_ridge: controls,
            ..base.clone()
        }; // Other key fields stay fixed.
        cache.begin(&next, 11, 5); // Request exactly this new control state.
        assert!(!cache.status().is_ready && cache.frames.is_empty()); // Old ready frames cannot survive a key change.
        wait_ready(&mut cache, &next, 11, 5); // Poll this same generation, not a retry.
        check_ready(&cache, &next, 11, 5); // All frames match the new original-scalar controls.
    } // End control replacements.
    cache.begin(&base, 31, 9); // Start a distinct unfinished generation.
    assert!(!cache.status().is_ready); // No completed result has been polled into readiness.
    cache.pause(); // Cancel only owned unfinished work through the supplied API.
    assert!(cache.build.is_none() && cache.frames.is_empty()); // Receiver and unpublished frames are no longer eligible.
    assert_eq!(cache.status().resident_bytes, 0); // Public residency resets; not a worker-join claim.
    wait_ready(&mut cache, &base, 19, 7); // Replacement has distinct dimensions and receiver ownership.
    check_ready(&cache, &base, 19, 7); // No stale result from the cancelled generation.
} // End cache replacement test.
