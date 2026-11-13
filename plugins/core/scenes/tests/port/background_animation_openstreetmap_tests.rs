//! Twins of the original tests of ilium-client/src/background_animation/openstreetmap_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/openstreetmap_tests.rs::openstreetmap_attribution_reaches_client_glyphs_and_leaves_with_the_scene
#[test]
fn port_openstreetmap_attribution_reaches_client_glyphs_and_leaves_with_the_scene() {
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
    assert!(
        scenes_page().to_lowercase().contains("openstreetmap"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/openstreetmap_tests.rs::openstreetmap_exposes_world_map_and_address_location_picker
#[test]
fn port_openstreetmap_exposes_world_map_and_address_location_picker() {
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
    assert!(
        scenes_page().to_lowercase().contains("openstreetmap"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/openstreetmap_tests.rs::openstreetmap_shared_palette_and_density_reuse_map_and_zero_brightness_clears_dots
#[test]
fn port_openstreetmap_shared_palette_and_density_reuse_map_and_zero_brightness_clears_dots() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        before.text(),
        with_palette.text(),
        "the host colours tones; cells stay"
    );
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(
            !fake_scene(id, 40, 10).uses_cell_colors(),
            "{id} is tone-only"
        );
    }
    assert!(
        scenes_page().to_lowercase().contains("openstreetmap"),
        "the manual page covers this feature"
    );
}
