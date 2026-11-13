//! Twins of the tests of ilium-client/src/ui_openstreetmap_tests.rs.
//!
//! The kernel tests composite a whole frame (PTY content, footer, voice control) and read the
//! credit back from the buffer. The compositor stays with the client; the plugin owns the credit
//! text and the strip it reserves (`credit`), so each twin asserts those rules over screen sizes.

use ilium_plugin_core_ambient::credit::{
    area_for_scene, attribution_area, attribution_rows, help_caveat, map_is_displayable,
    OSM_ATTRIBUTION, OSM_COPYRIGHT_URL,
};
use ilium_plugin_core_ambient::hover::Rect;
use ilium_plugin_core_ambient::rows::OPENSTREETMAP_ID;

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::ordinary_osm_credit_survives_final_composition_with_pty_footer_and_voice
#[test]
fn ordinary_osm_credit_survives_final_composition_with_pty_footer_and_voice_twin() {
    for (width, height) in [(80u16, 24u16), (120, 40)] {
        let screen = Rect::new(0, 0, width, height);
        let credit = attribution_area(screen);
        assert!(!credit.is_empty(), "{width}x{height}");
        assert_eq!(credit.width, width);
        // The strip sits directly above the footer row, so the status and voice rows stay put.
        assert_eq!(credit.bottom(), height - 1, "{width}x{height}");
        assert_eq!(
            usize::from(credit.height * width) >= OSM_ATTRIBUTION.len(),
            true
        );
    }
    assert_eq!(OSM_ATTRIBUTION, "(c) OpenStreetMap contributors / ODbL");
    assert!(
        OSM_ATTRIBUTION.is_ascii(),
        "measured width equals byte length"
    );
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::settings_and_fullscreen_osm_credit_have_separate_hint_and_voice_rows
#[test]
fn settings_and_fullscreen_osm_credit_have_separate_hint_and_voice_rows_twin() {
    for (screen, expected) in [
        (Rect::new(0, 0, 80, 24), Rect::new(0, 22, 80, 1)),
        (Rect::new(7, 11, 20, 10), Rect::new(7, 18, 20, 2)),
    ] {
        assert_eq!(attribution_area(screen), expected);
        let footer_row = screen.bottom() - 1;
        assert!(
            expected.bottom() <= footer_row,
            "never overlaps the hint and voice row"
        );
    }
    for width in [1u16, 7, 20, 36, 37, 80, 200] {
        let rows = attribution_rows(width);
        assert_eq!(usize::from(rows * width) >= OSM_ATTRIBUTION.len(), true);
        assert_eq!(
            usize::from((rows - 1) * width) < OSM_ATTRIBUTION.len(),
            true
        );
    }
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::disabled_or_switched_scene_clears_credit_and_restores_workspace_height
#[test]
fn disabled_or_switched_scene_clears_credit_and_restores_workspace_height_twin() {
    let screen = Rect::new(0, 0, 80, 24);
    let shown = area_for_scene(OPENSTREETMAP_ID, true, screen);
    assert!(!shown.is_empty());
    // The workspace above the strip is the full height minus the footer minus the credit rows.
    assert_eq!(shown.y, 22);
    assert!(
        area_for_scene(OPENSTREETMAP_ID, false, screen).is_empty(),
        "disabled"
    );
    for other in ["shoreline", "semantic", "wikipedia", ""] {
        assert!(area_for_scene(other, true, screen).is_empty(), "{other:?}");
    }
    // Enabling again restores the same strip.
    assert_eq!(area_for_scene(OPENSTREETMAP_ID, true, screen), shown);
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::too_small_to_credit_suppresses_the_map
#[test]
fn too_small_to_credit_suppresses_the_map_twin() {
    let tiny = Rect::new(0, 0, 10, 4);
    assert!(attribution_area(tiny).is_empty());
    assert!(!map_is_displayable(tiny));
    assert!(attribution_area(Rect::new(0, 0, 0, 24)).is_empty());
    assert!(map_is_displayable(Rect::new(0, 0, 80, 24)));
    // Four rows are not enough for two credit rows, but are for one.
    assert!(!map_is_displayable(Rect::new(0, 0, 20, 5)));
    assert!(map_is_displayable(Rect::new(0, 0, 80, 5)));
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::osm_settings_help_links_the_credited_source_and_license
#[test]
fn osm_settings_help_links_the_credited_source_and_license_twin() {
    let caveat = help_caveat();
    assert!(caveat.contains("OpenStreetMap contributors / ODbL"));
    assert!(caveat.contains("https://www.openstreetmap.org/copyright"));
    assert_eq!(OSM_COPYRIGHT_URL, "https://www.openstreetmap.org/copyright");
}
