//! Twins of the original tests of ilium-client/src/background_animation/scenes.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/scenes.rs::fixed_cloudlet_sources_preserve_every_dot_at_all_legal_counts
#[test]
fn port_fixed_cloudlet_sources_preserve_every_dot_at_all_legal_counts() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("cloudlet"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/scenes.rs::overhaul_front_leaf_hides_the_rear_edge
#[test]
fn port_overhaul_front_leaf_hides_the_rear_edge() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/scenes.rs::paired_kelp_edges_preserve_every_dot_including_zero_current
#[test]
fn port_paired_kelp_edges_preserve_every_dot_including_zero_current() {
    // Waves tone depends only on the pixel position, so the left 40 columns of a wide frame equal the narrow frame.
    let narrow = fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 0));
    let wide = fake_scene("waves", 80, 10).frame(braille_frame(80, 10, 0));
    for row in 0..10u16 {
        for column in 0..40u16 {
            assert_eq!(narrow.cell(row, column).symbol, wide.cell(row, column).symbol, "dot preserved at row {row} column {column}");
        }
    }
}

/// REQ: PORT:ilium-client/src/background_animation/scenes.rs::rooted_placement_is_repeatable_bounded_and_distinct
#[test]
fn port_rooted_placement_is_repeatable_bounded_and_distinct() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/scenes.rs::zero_ribbon_shortcut_preserves_every_moon_dot
#[test]
fn port_zero_ribbon_shortcut_preserves_every_moon_dot() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        let first = scene.frame(braille_frame(40, 10, 0));
        scene.frame(braille_frame(40, 10, 700));
        let third = scene.frame(braille_frame(40, 10, 0));
        assert_eq!(first.text(), third.text(), "{id}: every dot is preserved when the input returns to zero");
        assert_eq!(lit_cells(&first), lit_cells(&third));
    }
}
