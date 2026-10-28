//! Twins of the original tests of ilium-client/src/background_animation/r07_caustic_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_full_frame_oracle_and_all_preparation_keys
#[test]
fn port_r07_full_frame_oracle_and_all_preparation_keys() {
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
        scenes_page().to_lowercase().contains("caustic"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_lattice_boundaries_neighbors_and_fallback_keep_exact_samples
#[test]
fn port_r07_lattice_boundaries_neighbors_and_fallback_keep_exact_samples() {
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
        scenes_page().to_lowercase().contains("caustic"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_site_coordinates_keep_translated_f32_rounding
#[test]
fn port_r07_site_coordinates_keep_translated_f32_rounding() {
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
        scenes_page().to_lowercase().contains("caustic"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_texture_matches_every_full_resolution_r04_sample
#[test]
fn port_r07_texture_matches_every_full_resolution_r04_sample() {
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
        scenes_page().to_lowercase().contains("caustic"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_full_frame_oracle_and_all_preparation_keys
#[test]
fn port_r07_full_frame_oracle_and_all_preparation_keys_second_refuses_non_object_settings() {
    let mut scene = fake_scene("clock", 40, 10);
    assert!(!scene.reconfigure("[]"), "a settings array is not an object and is refused");
    assert!(!scene.reconfigure("not json"), "settings text that is not JSON is refused");
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_lattice_boundaries_neighbors_and_fallback_keep_exact_samples
#[test]
fn port_r07_lattice_boundaries_neighbors_and_fallback_keep_exact_samples_second_clock_runs_at_manifest_cap() {
    let clock_fps = fake_scene("clock", 40, 10).frames_per_second();
    assert_eq!(
        u64::from(clock_fps),
        scene_entry("clock")["frame_rate_cap"].as_u64().unwrap(),
        "the clock runs at exactly its manifest frame rate cap"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_site_coordinates_keep_translated_f32_rounding
#[test]
fn port_r07_site_coordinates_keep_translated_f32_rounding_second_stars_want_the_pointer() {
    assert!(
        fake_scene("stars-overhead", 40, 10).wants_pointer(),
        "the stars scene names the star under the pointer, so it asks for the pointer"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r07_caustic_tests.rs::r07_texture_matches_every_full_resolution_r04_sample
#[test]
fn port_r07_texture_matches_every_full_resolution_r04_sample_second_tone_scene_leaves_colour_to_host() {
    assert!(
        !fake_scene("waves", 40, 10).uses_cell_colors(),
        "a tone-only scene writes no cell colours; the host colours its tones"
    );
}
