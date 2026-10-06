use super::r08_ridge_oracle::{original_hash, original_noise, original_textures, same_texture}; // Frozen original-only numerical oracle.
use super::{
    prepare_ridge_textures, ridge_noise_grid, ridge_noise_horizontal, RidgeNoiseAxis,
    RidgeNoiseColumn, RIDGE_NOISE_SCRATCH_BYTES,
}; // Exact B1 private helper APIs.
   //
#[test] // Candidate-only; no baseline registration for this file.
fn r08_ridge_helper_full_resolution_and_cap_fallback() {
    // Compare direct helper outputs on both sides of scratch admission.
    let limit = RIDGE_NOISE_SCRATCH_BYTES / std::mem::size_of::<RidgeNoiseColumn>(); // Actual compiled type size, not an assumed layout.
    assert!(limit > 1); // The fixed payload ceiling must hold at least two columns.
    for (width, height) in [
        (0, 0),
        (0, 7),
        (9, 0),
        (1, 1),
        (7, 9),
        (31, 17),
        (160, 96),
        (320, 200),
        (480, 240),
        (limit - 1, 3),
        (limit, 3),
        (limit + 1, 3),
        (limit + 1, 0),
    ] {
        // Dot dimensions including exact cap neighbors.
        let expected = original_textures(width, height); // Complete scalar oracle textures.
        let actual = prepare_ridge_textures(width, height); // Unmodified B1 helper.
        same_texture(&expected.0, &actual.0); // Every cloud bit and exact full-resolution dimensions.
        same_texture(&expected.1, &actual.1); // Every mist bit and exact full-resolution dimensions.
        assert_eq!(
            actual.0.values.len() + actual.1.values.len(),
            2 * width * height
        ); // No coarse payload or omitted texture.
    } // End fixed dimension matrix.
} // End helper parity and scalar-cap fallback.
#[test] // Candidate-only.
fn r08_ridge_grid_corners_signed_seams_and_axis_bits() {
    // Corner identity, wrapping and exact two-stage interpolation.
    grid_case::<5, 4>(37); // Original broad cloud recipe.
    grid_case::<11, 9>(94); // Original fine cloud recipe.
    grid_case::<7, 5>(183); // Original mist recipe.
} // End recipe dispatch.
fn grid_case<const COLUMNS: usize, const ROWS: usize>(seed: i32) {
    // One bounded test for each literal B1 lattice shape.
    let grid = ridge_noise_grid::<COLUMNS, ROWS>(seed); // Actual prepared corners.
    for (y, row) in grid.iter().enumerate() {
        // All rows, not selected witnesses.
        for (x, corner) in row.iter().enumerate() {
            // Every original corner.
            assert_eq!(
                corner.to_bits(),
                original_hash(x as i32 + seed, y as i32).to_bits()
            ); // Hash after wrapping, no changed seed order.
        } // End corners.
    } // End grid rows.
    let mut coordinates = vec![
        -2.0_f32,
        -1.0,
        -0.0,
        0.0,
        1.0,
        2.0,
        f32::from_bits(1),
        f32::from_bits(0x8000_0001),
    ]; // Signed zero, subnormals and wrap endpoints.
    for period in [COLUMNS, ROWS] {
        // Exercise both kinds of lattice boundary.
        for cell in -(period as i32)..=period as i32 {
            // Finite signed domain; no integer overflow is invoked.
            let center = cell as f32 / period as f32; // Boundary expressed with original division order.
            coordinates.extend([center - 0.000001, center, center + 0.000001]); // Values around each seam and cell transition.
        } // End cells.
    } // End periods.
    for &u in &coordinates {
        // Every horizontal boundary case.
        let horizontal = ridge_noise_horizontal(&grid, u); // Actual prepared horizontal arithmetic.
        for &v in &coordinates {
            // Every vertical boundary case.
            let actual = RidgeNoiseAxis::new(v, ROWS as i32).sample_rows(&horizontal); // Actual B1 vertical arithmetic.
            assert_eq!(
                actual.to_bits(),
                original_noise(u, v, COLUMNS as i32, ROWS as i32, seed).to_bits(),
                "u={u:?}, v={v:?}, seed={seed}"
            ); // Exact bits, no tolerance.
        } // End vertical samples.
    } // End horizontal samples.
} // End lattice boundary case.
#[test] // Candidate-only.
fn r08_ridge_prepared_mutations_are_detected() {
    // Check negative controls through the actual prepared evaluation path.
    let grid = ridge_noise_grid::<5, 4>(37); // Correct original recipe.
    let mut corner_mutant = grid; // Mutate only the diagnostic copy.
    corner_mutant[0][0] = f32::from_bits(corner_mutant[0][0].to_bits() ^ 1); // One-bit corner mutation.
    let value =
        RidgeNoiseAxis::new(0.0, 4).sample_rows(&ridge_noise_horizontal(&corner_mutant, 0.0)); // Exact lattice origin exposes that corner.
    assert_ne!(
        value.to_bits(),
        original_noise(0.0, 0.0, 5, 4, 37).to_bits()
    ); // The full-float oracle rejects a one-bit error.
    let mut topology_mutant = grid; // A separate diagnostic topology mutation.
    topology_mutant.rotate_left(1); // Wrong vertical row identity without changing payload size.
    let mut differing = 0; // Require an actual differing sample, not a guessed coordinate.
    for y in 0..17 {
        // Finite nonaligned rows.
        for x in 0..23 {
            // Finite nonaligned columns.
            let (u, v) = (x as f32 / 23.0, y as f32 / 17.0); // Original-style sample coordinates.
            let actual =
                RidgeNoiseAxis::new(v, 4).sample_rows(&ridge_noise_horizontal(&topology_mutant, u)); // Evaluate the wrong prepared grid.
            differing += usize::from(actual.to_bits() != original_noise(u, v, 5, 4, 37).to_bits());
            // Count concrete oracle rejections.
        } // End columns.
    } // End rows.
    assert!(differing > 0); // A vacuous topology negative control fails the suite.
} // End prepared mutation controls.
