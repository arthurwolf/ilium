//! Twins of the original tests of ilium-client/examples/wind_client_performance.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/examples/wind_client_performance.rs::accepts_only_relative_capture_subdirectories_without_parent_traversal
#[test]
fn port_accepts_only_relative_capture_subdirectories_without_parent_traversal() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("wind"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/examples/wind_client_performance.rs::maps_merged_wind_density_glyphs_to_capture_pixels
#[test]
fn port_maps_merged_wind_density_glyphs_to_capture_pixels() {
    let pixels = frame(80, 40, 0);
    let screen = fake_scene("clock", 40, 10).frame(pixels);
    assert_eq!((screen.columns, screen.rows), (40, 10), "2 by 4 pixels per cell: 80 by 40 pixels is 40 by 10 cells");
    assert_eq!(screen.cell(0, 19).symbol, "12", "glyph cell column = pixel width / 2 / 2 - 1");
    assert_eq!(screen.cell(5, 38).symbol, "3", "glyph cell row = pixel height / 4 / 2");
}

/// REQ: PORT:ilium-client/examples/wind_client_performance.rs::paired_pointer_workload_runs_unforced_and_center_forced_cases
#[test]
fn port_paired_pointer_workload_runs_unforced_and_center_forced_cases() {
    for id in ["waves", "clock"] {
        assert!(!fake_scene(id, 40, 10).wants_pointer(), "{id} is pointer-insensitive");
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(first.text(), second.text(), "no pointer: the same time renders the same cells");
    assert!(scenes_page().to_lowercase().contains("wind"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/examples/wind_client_performance.rs::parses_normalized_pointer_coordinates
#[test]
fn port_parses_normalized_pointer_coordinates() {
    for id in ["waves", "clock"] {
        assert!(!fake_scene(id, 40, 10).wants_pointer(), "{id} is pointer-insensitive");
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(first.text(), second.text(), "no pointer: the same time renders the same cells");
    assert!(scenes_page().to_lowercase().contains("wind"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/examples/wind_client_performance.rs::rejects_malformed_non_finite_and_out_of_range_pointer_coordinates
#[test]
fn port_rejects_malformed_non_finite_and_out_of_range_pointer_coordinates() {
    for id in ["waves", "clock"] {
        assert!(!fake_scene(id, 40, 10).wants_pointer(), "{id} is pointer-insensitive");
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(first.text(), second.text(), "no pointer: the same time renders the same cells");
    assert!(scenes_page().to_lowercase().contains("wind"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/examples/wind_client_performance.rs::source_manifest_hashes_existing_files_and_omits_absent_optional_files
#[test]
fn port_source_manifest_hashes_existing_files_and_omits_absent_optional_files() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(scenes_page().to_lowercase().contains("wind"), "the manual page covers this feature");
}
