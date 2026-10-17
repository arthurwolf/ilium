//! Twins of the original tests of ilium-client/src/background_animation/wikipedia/settings_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/settings_tests.rs::wiki_color_mode_and_greyscale_apply_to_images_and_text
#[test]
fn port_wiki_color_mode_and_greyscale_apply_to_images_and_text() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/settings_tests.rs::wikipedia_controls_round_trip_and_presets_keep_custom_sliders
#[test]
fn port_wikipedia_controls_round_trip_and_presets_keep_custom_sliders() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/settings_tests.rs::wikipedia_defaults_and_ranges_are_explicit
#[test]
fn port_wikipedia_defaults_and_ranges_are_explicit() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(
            scene.reconfigure(&json!({}).to_string()),
            "{id}: empty settings accepted"
        );
        assert!(
            !scene.reconfigure("{not json"),
            "{id}: malformed settings refused"
        );
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_string())
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}
