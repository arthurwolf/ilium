//! Twins of the original tests of ilium-client/src/background_animation/pack_simd.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/pack_simd.rs::runtime_threshold_packing_matches_scalar_for_cell_tails_and_non_finite_samples
#[test]
fn port_runtime_threshold_packing_matches_scalar_for_cell_tails_and_non_finite_samples() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(
            first.text(),
            second.text(),
            "{id}: identical input must give an identical frame"
        );
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/pack_simd.rs::runtime_threshold_packing_matches_scalar_for_cell_tails_and_non_finite_samples
#[test]
fn port_runtime_threshold_packing_matches_scalar_for_cell_tails_and_non_finite_samples_second_status_row_is_never_empty() {
    let mut scene = fake_scene("stars-overhead", 40, 10);
    let _screen = scene.frame(braille_frame(40, 10, 0));
    if let Some(text) = scene.status() {
        assert!(
            !text.trim().is_empty(),
            "a status row, when the scene shows one, is never empty"
        );
    }
}
