// Frozen scenes SHA256: 99ad52eeffa67e52b06f74b25145a644a4dab7f9a0e0bc27663a4bf8e8468b4c.
// Frozen raster SHA256: 9f391c7ac8006dc5c91e5c9919175db6a8e43adc8624f746408ad6e5d8f2485b.
use super::{
    AnimationKind, AnimationSettings, PreparedScene, Raster, SceneCache, SleepingRidgeSettings,
    Texture,
}; // Existing private scene types.
use crate::background_animation::{
    AnimationFrame, AnimationPlaybackMode, DitherMode, PackKey, PanelTarget,
}; // Actual frame and packing boundary.
use std::time::Duration; // Public render time, not a synthetic float clock.
                         //
pub(super) fn original_hash(x: i32, y: i32) -> f32 {
    // Current supplied raster hash, independently frozen.
    let mut value =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ 0xcb1a_b31f; // Original seed expression.
    value ^= value >> 16; // Original mixing order.
    value = value.wrapping_mul(0x7feb_352d); // Original wrapping multiplication.
    value ^= value >> 15; // Original mixing order.
    value = value.wrapping_mul(0x846c_a68b); // Original wrapping multiplication.
    value ^= value >> 16; // Original final mix.
    (value & 0xffff) as f32 / 65535.0 // Original conversion and division.
} // End frozen hash.
fn original_smoothstep(low: f32, high: f32, value: f32) -> f32 {
    // Current supplied scalar smoothing.
    let fraction = ((value - low) / (high - low)).clamp(0.0, 1.0); // Preserve division and clamp.
    fraction * fraction * (3.0 - 2.0 * fraction) // Preserve multiplication association.
} // End frozen smoothing.
pub(super) fn original_noise(u: f32, v: f32, columns: i32, rows: i32, seed: i32) -> f32 {
    // No candidate helper is used.
    let x = u * columns as f32; // Original x scaling.
    let y = v * rows as f32; // Original y scaling.
    let left = x.floor() as i32; // Original cell conversion.
    let top = y.floor() as i32; // Original cell conversion.
    let fraction_x = original_smoothstep(0.0, 1.0, x - x.floor()); // Original x operand.
    let fraction_y = original_smoothstep(0.0, 1.0, y - y.floor()); // Original y operand.
    let corner =
        |cx: i32, cy: i32| original_hash(cx.rem_euclid(columns) + seed, cy.rem_euclid(rows)); // Wrap before adding seed.
    let upper = corner(left, top) * (1.0 - fraction_x) + corner(left + 1, top) * fraction_x; // Original upper blend.
    let lower = corner(left, top + 1) * (1.0 - fraction_x) + corner(left + 1, top + 1) * fraction_x; // Original lower blend.
    upper * (1.0 - fraction_y) + lower * fraction_y // Do not reassociate this expression.
} // End frozen noise.
fn original_texture(width: usize, height: usize, sample: impl Fn(f32, f32) -> f32) -> Texture {
    // Independent original construction loop.
    let mut values = Vec::with_capacity(width * height); // Original full-resolution capacity.
    for y in 0..height {
        // Original row-major traversal.
        for x in 0..width {
            // Empty dimensions execute no sample.
            values.push(sample(x as f32 / width as f32, y as f32 / height as f32));
            // No half-dot offset.
        } // End columns.
    } // End rows.
    Texture {
        width,
        height,
        values,
    } // No call to production Texture::new.
} // End frozen constructor.
pub(super) fn original_textures(width: usize, height: usize) -> (Texture, Texture) {
    // The two original Ridge texture expressions.
    let clouds = original_texture(width, height, |u, v| {
        original_noise(u, v, 5, 4, 37) * 0.76 + original_noise(u, v, 11, 9, 94) * 0.24
    }); // Original cloud mixture.
    let mist = original_texture(width, height, |u, v| original_noise(u, v, 7, 5, 183)); // Original mist lattice.
    (clouds, mist) // Preserve construction order.
} // End texture oracle.
fn original_sample(texture: &Texture, u: f32, v: f32) -> f32 {
    // Frozen transported lookup; never call on an empty texture.
    let x = u.rem_euclid(1.0) * texture.width as f32; // Original normalized wrapping.
    let y = v.rem_euclid(1.0) * texture.height as f32; // Original normalized wrapping.
    let left = x.floor() as usize % texture.width; // Original top-left cell.
    let top = y.floor() as usize % texture.height; // Original top-left cell.
    let right = (left + 1) % texture.width; // Original horizontal seam.
    let bottom = (top + 1) % texture.height; // Original vertical seam.
    let fraction_x = x - x.floor(); // Transport uses unsmoothed fractions.
    let fraction_y = y - y.floor(); // Transport uses unsmoothed fractions.
    let upper = texture.values[top * texture.width + left] * (1.0 - fraction_x)
        + texture.values[top * texture.width + right] * fraction_x; // Original upper blend.
    let lower = texture.values[bottom * texture.width + left] * (1.0 - fraction_x)
        + texture.values[bottom * texture.width + right] * fraction_x; // Original lower blend.
    upper * (1.0 - fraction_y) + lower * fraction_y // Original vertical blend.
} // End transported lookup.
fn original_ridge(u: f32, layer: usize, settings: SleepingRidgeSettings) -> f32 {
    // Frozen contours, not another optimization.
    let original = match layer {
        // Preserve all three original formulas.
        0 => 0.50 + 0.08 * (u * 9.0 + 0.3).sin() + 0.035 * (u * 19.0).sin(), // Far contour.
        1 => 0.64 + 0.08 * (u * 7.0 + 1.8).sin() + 0.035 * (u * 14.0 + 0.4).sin(), // Middle contour.
        _ => 0.79 + 0.055 * (u * 8.0 + 4.0).sin() + 0.028 * (u * 17.0).sin(),      // Front contour.
    }; // End layer selection.
    0.91 - (0.91 - original) * f32::from(settings.ridge_height_percent) / 100.0 // Original height association.
} // End contour oracle.
pub(super) fn original_scene(
    raster: &mut Raster,
    controls: SleepingRidgeSettings,
) -> PreparedScene {
    // Reuse only unchanged raster primitives.
    raster.field(|u, v| {
        // This current primitive clears every field owner.
        if v > original_ridge(u, 2, controls) {
            return 0.008;
        } // Original nearest-layer priority.
        if v > original_ridge(u, 1, controls) {
            return 0.045;
        } // Original middle-layer priority.
        if v > original_ridge(u, 0, controls) {
            return 0.10;
        } // Original far-layer priority.
        0.0 // Original sky value.
    }); // End original field.
    for layer in 0..3 {
        // Original contour order and raster line ownership.
        raster.curve(
            raster.width.min(180),
            0.22,
            0.42 - layer as f32 * 0.10,
            |u| (u, original_ridge(u, layer, controls)),
        ); // Original curve parameters.
    } // End contours.
    let (clouds, mist) = original_textures(raster.width, raster.height); // Scalar full-resolution textures.
    let fronts = (0..raster.width)
        .map(|x| original_ridge((x as f32 + 0.5) / raster.width as f32, 2, controls))
        .collect(); // Original half-dot fronts.
    PreparedScene::Ridge {
        base: raster.dots.clone(),
        clouds,
        mist,
        fronts,
    } // Original retained representation.
} // End prepared-scene oracle.
pub(super) fn original_warm(
    raster: &mut Raster,
    controls: SleepingRidgeSettings,
    time: f32,
    scene: &PreparedScene,
) {
    // Frozen warm shader.
    let PreparedScene::Ridge {
        base,
        clouds,
        mist,
        fronts,
    } = scene
    else {
        panic!("oracle requires Ridge");
    }; // Reject wrong fixtures.
    raster.dots.copy_from_slice(base); // Raw warm rendering does not clear owner IDs.
    let drift = time * 0.025 * f32::from(controls.drift_percent) / 100.0; // Original drift association.
    let cover = f32::from(controls.cloud_cover_percent) / 100.0; // Original cover conversion.
    let mist_strength = f32::from(controls.mist_percent) / 100.0; // Original mist conversion.
    for y in 0..raster.height {
        // Original row traversal.
        let v = (y as f32 + 0.5) / raster.height as f32; // Original half-dot geometry coordinate.
        let sky_mask = 1.0 - original_smoothstep(0.34, 0.58, v); // Original sky mask.
        let mist_mask = 1.0 - original_smoothstep(0.0, 0.14, (v - 0.70).abs()); // Original valley mask.
        if sky_mask == 0.0 && mist_mask == 0.0 {
            continue;
        } // Original inactive-row guard.
        let warp = (v * 7.0 - time * 0.20).sin() * 0.028; // Original warp.
        for (x, front) in fronts.iter().enumerate() {
            // Original front order.
            let u = (x as f32 + 0.5) / raster.width as f32; // Original geometry coordinate.
            let index = y * raster.width + x; // Original dot address.
            if cover > 0.0 && sky_mask > 0.0 {
                // Original cloud admission.
                let bank = original_smoothstep(
                    0.76 - cover * 0.48,
                    0.95 - cover * 0.29,
                    original_sample(clouds, u - drift + warp, v),
                ); // Original transported cloud lookup.
                raster.dots[index] = raster.dots[index].max(bank * sky_mask * 0.70);
                // Original cloud accumulation.
            } // End clouds.
            if mist_strength > 0.0 && mist_mask > 0.0 && v < *front {
                // Original valley admission.
                let valley =
                    original_smoothstep(0.38, 0.76, original_sample(mist, u + drift * 0.37, v)); // Original mist transport.
                raster.dots[index] =
                    raster.dots[index].max(valley * mist_mask * mist_strength * 0.68);
                // Original mist association.
            } // End mist.
        } // End columns.
    } // End rows.
} // End warm oracle.
pub(super) fn same_bits(expected: &[f32], actual: &[f32]) {
    // Length and every scalar matter, not only a checksum.
    assert_eq!(expected.len(), actual.len(), "scalar length"); // Reject truncated output.
    for (index, (a, b)) in expected.iter().zip(actual).enumerate() {
        // Length was checked before zipping.
        assert_eq!(a.to_bits(), b.to_bits(), "scalar {index}"); // Include signed-zero differences.
    } // End exact comparison.
} // End scalar assertion.
pub(super) fn same_texture(expected: &Texture, actual: &Texture) {
    // Check representation and all payload values.
    assert_eq!(
        (expected.width, expected.height),
        (actual.width, actual.height)
    ); // Do not accept coarse texture dimensions.
    assert_eq!(actual.values.len(), actual.width * actual.height); // Full-resolution payload is mandatory.
    same_bits(&expected.values, &actual.values); // Compare every original value.
} // End texture assertion.
fn same_scene(expected: &PreparedScene, actual: &PreparedScene) {
    // Inspect actual private prepared data.
    let PreparedScene::Ridge {
        base: eb,
        clouds: ec,
        mist: em,
        fronts: ef,
    } = expected
    else {
        panic!("expected Ridge");
    }; // Validate oracle variant.
    let PreparedScene::Ridge {
        base: ab,
        clouds: ac,
        mist: am,
        fronts: af,
    } = actual
    else {
        panic!("actual Ridge");
    }; // Validate production variant.
    same_bits(eb, ab); // Every base dot.
    same_bits(ef, af); // Every retained contour coordinate.
    same_texture(ec, ac); // Every cloud value.
    same_texture(em, am); // Every mist value.
} // End retained-scene assertion.
pub(super) fn ridge_settings(controls: SleepingRidgeSettings) -> AnimationSettings {
    // Fixed common baseline for test fixtures.
    AnimationSettings {
        enabled: true,
        kind: AnimationKind::SleepingRidge,
        playback_mode: AnimationPlaybackMode::Live,
        loop_seconds: 1,
        sleeping_ridge: controls,
        ..Default::default()
    } // Keep density60, Ordered, speed100.
} // End fixture settings.
fn control_cases() -> Vec<SleepingRidgeSettings> {
    // Default, individual endpoints, combined endpoints, mixed and untrusted cases.
    let base = SleepingRidgeSettings::default(); // Defaults supplied by parameters.rs.
    let mut result = vec![base]; // Always retain the primary controls.
    for index in 0..4 {
        // Check every key field separately.
        for high in [false, true] {
            // Each legal endpoint.
            let mut next = base; // Other controls stay at default.
            match index {
                // Use exact concrete fields rather than guessed setters.
                0 => next.cloud_cover_percent = if high { 100 } else { 0 }, // Cloud endpoints.
                1 => next.mist_percent = if high { 100 } else { 0 },        // Mist endpoints.
                2 => next.ridge_height_percent = if high { 150 } else { 50 }, // Height endpoints.
                _ => next.drift_percent = if high { 200 } else { 25 },      // Drift endpoints.
            } // End field selection.
            result.push(next); // Keep all individual cases.
        } // End endpoints.
    } // End controls.
    for [cloud_cover_percent, mist_percent, ridge_height_percent, drift_percent] in [
        [0, 0, 50, 25],
        [100, 100, 150, 200],
        [35, 75, 115, 65],
        [u16::MAX, 0, 0, u16::MAX],
    ] {
        // Fixed finite additional cases.
        result.push(SleepingRidgeSettings {
            cloud_cover_percent,
            mist_percent,
            ridge_height_percent,
            drift_percent,
        }); // Include normalization coverage.
    } // End additional cases.
    result // Thirteen explicitly bounded cases.
} // End control matrix.
pub(in crate::background_animation) fn reference_frame(
    settings: &AnimationSettings,
    width: u16,
    height: u16,
    elapsed: Duration,
) -> AnimationFrame {
    // Usable by cache tests without exporting production APIs.
    let settings = settings.normalized(); // Same public normalization boundary.
    assert_eq!(settings.kind, AnimationKind::SleepingRidge); // Only Ridge is an oracle subject.
    let mut frame = AnimationFrame::default(); // No hosted scene construction.
    frame.resize(width, height); // Exact cell-to-dot mapping and zero ownership.
    if width == 0 || height == 0 {
        return frame;
    } // Match public empty-frame admission.
    let prepared = original_scene(&mut frame.raster, settings.sleeping_ridge); // No SceneCache or candidate constructor.
    let seconds = elapsed.as_secs_f64() * f64::from(settings.speed_percent) / 100.0; // Original public speed association.
    original_warm(
        &mut frame.raster,
        settings.sleeping_ridge,
        seconds as f32,
        &prepared,
    ); // Original f64-to-f32 boundary.
    frame.pack(PackKey::of(&settings)); // Shared unchanged packer; separate matrix oracle below.
    frame // Geometry and owners derive from the independent scalar shader.
} // End public-frame oracle.
pub(in crate::background_animation) fn same_frame(
    expected: &AnimationFrame,
    actual: &AnimationFrame,
) {
    // Raw values, owners and packed bytes.
    assert_eq!(
        (expected.width(), expected.height()),
        (actual.width(), actual.height())
    ); // Reject dimensions first.
    same_bits(&expected.raster.dots, &actual.raster.dots); // Stronger than glyph checksums.
    assert_eq!(expected.raster.owner_ids, actual.raster.owner_ids); // Exact owner plane, including length.
    assert_eq!(expected.packed_cells(), actual.packed_cells()); // Every packed cell.
    assert!(!actual.has_cell_colors()); // Ridge remains monochrome.
} // End frame comparison.
#[test] // Runs unchanged on original and candidate.
fn r08_ridge_scalar_and_retained_payload_matrix() {
    // Original-only oracle admission and candidate cold/warm parity.
    for (width, height) in [
        (0, 0),
        (0, 9),
        (7, 0),
        (1, 1),
        (31, 17),
        (160, 96),
        (320, 200),
        (480, 240),
    ] {
        // Dot dimensions, including all three required cell sizes.
        for controls in control_cases() {
            // Full fixed control inventory.
            let settings = ridge_settings(controls).normalized(); // Test actual normalized controls.
            let mut expected = Raster::default(); // Independent raster storage.
            expected.resize(width, height); // Empty and odd dot dimensions included.
            expected.owner_ids.fill(73); // Force cold owner clearing to be observable.
            let mut actual = Raster::default(); // Production raster storage.
            actual.resize(width, height); // Identical dimensions.
            actual.owner_ids.fill(73); // Same provenance sentinel.
            let reference = original_scene(&mut expected, settings.sleeping_ridge); // Frozen original preparation.
            let mut cache = SceneCache::default(); // A real cold cache miss.
            cache.prepare(&mut actual, &settings); // Actual baseline or candidate preparation.
            same_scene(&reference, &cache.prepared); // Full payload, not a digest.
            same_bits(&expected.dots, &actual.dots); // Cold raster equality.
            assert_eq!(expected.owner_ids, actual.owner_ids); // Cold owner equality.
            assert!(actual.owner_ids.iter().all(|owner| *owner == 0)); // Field preparation must clear provenance.
            for seconds in [0.0, 7.25, -3.5, 1.0 / 30.0, 80.0, 0.0] {
                // Forward, reverse, negative, wrap and repeated times.
                expected.owner_ids.fill(91); // Raw warm rendering deliberately retains owners.
                actual.owner_ids.fill(91); // Make accidental new clearing fail.
                original_warm(
                    &mut expected,
                    settings.sleeping_ridge,
                    seconds as f32,
                    &reference,
                ); // Independent original warm output.
                super::render(&mut actual, &mut cache, &settings, seconds); // Real warm dispatcher.
                same_bits(&expected.dots, &actual.dots); // Every warm float bit.
                assert_eq!(expected.owner_ids, actual.owner_ids); // Exact raw-render owner behavior.
                assert_eq!(cache.preparations, 1); // Time changes must not rebuild textures.
            } // End times.
        } // End controls.
    } // End dimensions.
} // End cold/warm matrix.
#[test] // Runs unchanged on original and candidate.
fn r08_ridge_public_frames_pack_and_normalize() {
    // Public frame parity, independent Ordered/Stippled packing, all dither dispatch.
    for (width, height) in [
        (0, 0),
        (0, 3),
        (5, 0),
        (1, 1),
        (17, 9),
        (80, 24),
        (160, 50),
        (240, 60),
    ] {
        // Cell dimensions.
        let mut actual = AnimationFrame::default(); // Reused public frame with real keys.
        for (index, controls) in control_cases().into_iter().enumerate() {
            // Include untrusted controls.
            let mut settings = ridge_settings(controls); // Preserve default pack controls initially.
            settings.speed_percent = [0, 25, 100, 300, u16::MAX][index % 5]; // Both legal and normalization endpoints.
            for nanos in [7_250_000_000, 0, 33_333_334, 80_000_000_000] {
                // Seekable nonnegative public times.
                let elapsed = Duration::from_nanos(nanos); // Exact nanosecond requests.
                actual.render(&settings, width, height, elapsed); // Public ownership and geometry invalidation.
                same_frame(&reference_frame(&settings, width, height, elapsed), &actual);
                // All scalar and cell output.
            } // End times.
        } // End controls.
    } // End sizes.
    for mode in DitherMode::ALL {
        // Include every currently supplied dither mode.
        for density in [0, 25, 60, 100, u16::MAX] {
            // Public density normalization is part of the contract.
            let mut settings = ridge_settings(SleepingRidgeSettings::default()); // Fixed geometry for packing tests.
            settings.dither = mode; // Real mode dispatch.
            settings.density_percent = density; // Legal and clamped values.
            for (contrast, invert) in [(100, false), (175, true)] {
                // Neutral and shaped patterns.
                settings.appearance.pattern_contrast_percent = contrast; // Existing concrete appearance control.
                settings.appearance.pattern_invert = invert; // Existing inversion control.
                let mut actual = AnimationFrame::default(); // No prior threshold state.
                actual.render(&settings, 17, 9, Duration::from_secs(7)); // Finite small all-mode fixture.
                same_frame(
                    &reference_frame(&settings, 17, 9, Duration::from_secs(7)),
                    &actual,
                ); // Shared packer, independent raw shader.
            } // End pattern cases.
        } // End densities.
    } // End modes.
    for mode in [DitherMode::Ordered, DitherMode::Stippled] {
        // Independently check the two required matrix packers.
        for density in [25, 60, 100] {
            // Legal sparse/default/dense values.
            let mut settings = ridge_settings(SleepingRidgeSettings::default()); // Neutral pattern.
            settings.dither = mode; // Choose matrix.
            settings.density_percent = density; // Choose threshold scale.
            let frame = reference_frame(&settings, 17, 9, Duration::from_secs(7)); // Frozen raw geometry.
            assert_eq!(
                matrix_cells(&frame.raster, 17, 9, density, mode),
                frame.packed_cells()
            ); // Independent per-dot bit mapping.
        } // End densities.
    } // End matrices.
} // End public/packing matrix.
fn matrix_cells(
    raster: &Raster,
    width: usize,
    height: usize,
    density: u16,
    mode: DitherMode,
) -> Vec<u8> {
    // Frozen original matrix packing.
    let table = [
        [0_u8, 48, 12, 60, 3, 51, 15, 63],
        [32, 16, 44, 28, 35, 19, 47, 31],
        [8, 56, 4, 52, 11, 59, 7, 55],
        [40, 24, 36, 20, 43, 27, 39, 23],
        [2, 50, 14, 62, 1, 49, 13, 61],
        [34, 18, 46, 30, 33, 17, 45, 29],
        [10, 58, 6, 54, 9, 57, 5, 53],
        [42, 26, 38, 22, 41, 25, 37, 21],
    ]; // Supplied original Bayer8 table.
    let bits = [[1_u8, 8], [2, 16], [4, 32], [64, 128]]; // Exact Braille bit ordering.
    let mut cells = vec![0; width * height]; // Exact output length.
    for y in 0..height {
        // Cell rows.
        for x in 0..width {
            // Cell columns.
            for (dy, row) in bits.iter().enumerate() {
                // Four dot rows.
                for (dx, bit) in row.iter().enumerate() {
                    // Two dot columns.
                    let (rx, ry) = (x * 2 + dx, y * 4 + dy); // Original dot address.
                    let threshold = match mode {
                        // Never use candidate threshold storage.
                        DitherMode::Ordered => (f32::from(table[ry % 8][rx % 8]) + 0.5) / 64.0, // Original ordered threshold.
                        DitherMode::Stippled => {
                            0.005 + original_hash((rx % 64) as i32, (ry % 64) as i32) * 0.99
                        } // Original stipple threshold.
                        _ => panic!("matrix oracle supports Ordered and Stippled only"), // No invented fallback for other modes.
                    }; // End threshold selection.
                    if raster.dots[ry * raster.width + rx] * (f32::from(density) / 100.0)
                        > threshold
                    {
                        cells[y * width + x] |= *bit;
                    } // Original strict comparison.
                } // End dot columns.
            } // End dot rows.
        } // End cell columns.
    } // End cell rows.
    cells // Exact scalar reference cells.
} // End independent packing oracle.
#[test] // Runs unchanged on original and candidate.
fn r08_ridge_key_and_owner_lifecycle() {
    // Check stable allocations and every relevant invalidator.
    let mut settings = ridge_settings(SleepingRidgeSettings::default()); // Starting concrete scene.
    let mut frame = AnimationFrame::default(); // Public frame owns cache state.
    let time = Duration::from_secs(7); // Same-time edits isolate invalidation behavior.
    frame.render(&settings, 31, 9, time); // One cold preparation.
    let pointers = retained_pointers(&frame.scene_cache); // Observe actual retained buffers.
    let renders = frame.geometry_render_count; // Record original render count.
    frame.raster.owner_ids.fill(77); // Same-key requests must not silently mutate raw provenance.
    settings.enabled = false; // Compositor-only visibility.
    settings.hue_degrees = 90; // Presentation-only color.
    settings.panels = PanelTarget::Right; // Presentation-only panel selection.
    settings.fps_limit = 5; // Presentation cadence, not geometry.
    frame.render(&settings, 31, 9, time); // Same geometry and pack key.
    assert_eq!(frame.geometry_render_count, renders); // No resimulation.
    assert!(frame.raster.owner_ids.iter().all(|owner| *owner == 77)); // Preserve existing same-request semantics.
    settings.density_percent = 25; // Repack without new geometry.
    settings.dither = DitherMode::Stippled; // Repack without new textures.
    frame.render(&settings, 31, 9, time); // Exercise the real pack-key path.
    assert_eq!(frame.geometry_render_count, renders); // Still no geometry render.
    assert_eq!(retained_pointers(&frame.scene_cache), pointers); // All four retained buffers survive.
    assert!(frame.raster.owner_ids.iter().all(|owner| *owner == 77)); // Repacking does not own geometry.
    settings.speed_percent = 175; // Speed changes geometry, not static preparation.
    frame.render(&settings, 31, 9, time); // Public path clears owners.
    assert_eq!(frame.scene_cache.preparations, 1); // No cold rebuild for speed.
    assert_eq!(retained_pointers(&frame.scene_cache), pointers); // No replacement texture storage.
    same_frame(&reference_frame(&settings, 31, 9, time), &frame); // Correct output and zero owners.
    for controls in control_cases().into_iter().skip(1) {
        // All individual and combined control invalidators.
        settings.sleeping_ridge = controls; // Edit the actual settings block.
        let before = frame.scene_cache.preparations; // Previous generation count.
        let old_key = frame.scene_cache.key; // Capture the actual preparation key.
        frame.render(&settings, 31, 9, time); // Same time and size; only selected controls change.
        assert_ne!(frame.scene_cache.key, old_key); // Every consecutive fixed fixture changes normalized controls.
        assert_eq!(frame.scene_cache.preparations, before + 1); // Exactly one new preparation.
        same_frame(&reference_frame(&settings, 31, 9, time), &frame); // Fresh scalar result, including owner clearing.
    } // End controls.
    for (width, height) in [(0, 0), (0, 4), (7, 0), (31, 9), (17, 5), (31, 9)] {
        // Empty detours and genuine size changes.
        let before = frame.scene_cache.preparations; // Record retained preparation count.
        let previous_key = frame.scene_cache.key; // Empty requests retain it.
        frame.render(&settings, width, height, time); // Actual public size transition.
        if width == 0 || height == 0 {
            assert_eq!(frame.scene_cache.preparations, before);
            assert_eq!(frame.scene_cache.key, previous_key);
        } // No empty scene preparation.
        same_frame(&reference_frame(&settings, width, height, time), &frame); // Dimensions, empty cells and real output.
    } // End size transitions.
    for kind in [
        AnimationKind::Kelp,
        AnimationKind::MoonlitWater,
        AnimationKind::SleepingRidge,
    ] {
        // Replace and reconstruct the selected scene.
        let before = frame.scene_cache.preparations; // Count actual cache replacements.
        settings.kind = kind; // Preserve all other settings.
        frame.render(&settings, 31, 9, time); // New kind must not use old Ridge data.
        assert_eq!(frame.scene_cache.preparations, before + 1); // Exactly one replacement per distinct kind.
    } // End kind transitions.
    same_frame(&reference_frame(&settings, 31, 9, time), &frame); // Returning to Ridge is exact.
    frame.release_hosts(); // Explicit hiding/release invalidates public geometry, not SceneCache.
    assert!(frame.last_geometry.is_none() && frame.composed_key.is_none()); // Old presentation identity is invalid.
    assert!(frame.packed_cells().iter().all(|cell| *cell == 0)); // Old painted output is cleared.
    let before = frame.scene_cache.preparations; // Retained Ridge data is still reusable.
    frame.render(&settings, 31, 9, time); // Reopen the same scene.
    assert_eq!(frame.scene_cache.preparations, before); // No unintended cold rebuild on release.
    same_frame(&reference_frame(&settings, 31, 9, time), &frame); // Repaint and owner plane are exact.
} // End key and owner lifecycle.
fn retained_pointers(cache: &SceneCache) -> [usize; 4] {
    // Identity evidence, not a heap-size estimate.
    let PreparedScene::Ridge {
        base,
        clouds,
        mist,
        fronts,
    } = &cache.prepared
    else {
        panic!("Ridge required");
    }; // Reject wrong variant.
    [
        base.as_ptr() as usize,
        clouds.values.as_ptr() as usize,
        mist.values.as_ptr() as usize,
        fronts.as_ptr() as usize,
    ] // All retained buffers.
} // End pointer observation.
#[test] // Runs unchanged on original and candidate.
fn r08_ridge_oracle_rejects_noise_mutations() {
    // Finite negative controls for corner, topology and arithmetic errors.
    let mut rejected = [0_usize; 3]; // Count concrete differing samples per mutant.
    for y in 0..67 {
        // Non-lattice-aligned finite corpus.
        for x in 0..101 {
            // Include exact origin and fractional samples.
            let (u, v) = (x as f32 / 101.0, y as f32 / 67.0); // Original division-style coordinates.
            let correct = original_noise(u, v, 5, 4, 37); // Canonical oracle value.
            assert_eq!(
                correct.to_bits(),
                super::periodic_noise(u, v, 5, 4, 37).to_bits()
            ); // Validate the retained original scalar itself.
            let wrong = [
                original_noise(u, v, 5, 4, 38),
                original_noise(v, u, 5, 4, 37),
                reassociated_noise(u, v),
            ]; // Seed, axis topology, lerp association.
            for (count, value) in rejected.iter_mut().zip(wrong) {
                *count += usize::from(correct.to_bits() != value.to_bits());
            } // Record actual rejection, not an assumed witness.
        } // End columns.
    } // End rows.
    assert!(
        rejected.into_iter().all(|count| count > 0),
        "every mutation must have a concrete differing sample"
    ); // Fail if any negative control is ineffective.
} // End mutation controls.
fn reassociated_noise(u: f32, v: f32) -> f32 {
    // Deliberately wrong final lerp for the arithmetic negative control.
    let (x, y) = (u * 5.0, v * 4.0); // Same source lattice.
    let (left, top) = (x.floor() as i32, y.floor() as i32); // Same cells.
    let fx = original_smoothstep(0.0, 1.0, x - x.floor()); // Same horizontal fraction.
    let fy = original_smoothstep(0.0, 1.0, y - y.floor()); // Same vertical fraction.
    let corner = |cx: i32, cy: i32| original_hash(cx.rem_euclid(5) + 37, cy.rem_euclid(4)); // Same correct corner values.
    let upper = corner(left, top) * (1.0 - fx) + corner(left + 1, top) * fx; // Same upper operand.
    let lower = corner(left, top + 1) * (1.0 - fx) + corner(left + 1, top + 1) * fx; // Same lower operand.
    upper + (lower - upper) * fy // Intentional mutation; never used as the expected value.
} // End deliberately wrong helper.
