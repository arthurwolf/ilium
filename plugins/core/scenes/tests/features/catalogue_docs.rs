//! The scene catalogue of the page (F-095..F-101, F-103) against the user documentation.
use crate::common::*;

fn catalogue() -> Vec<Vec<String>> {
    table_after(&scenes_page(), "| Family | Scenes")
}

fn scene_names(cell: &str) -> Vec<String> {
    cell.split([',', '(', ')']).flat_map(|p| p.split(" and ")).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// REQ: F-095
#[test]
fn catalogue_095_natural_cached_loops_lists_ten_scenes() {
    let rows = catalogue();
    let row = rows.iter().find(|r| r[0] == "Natural cached loops").expect("row");
    assert_eq!(scene_names(&row[1]).len(), 10);
    assert!(row[1].contains("Shoreline") && row[1].contains("Lily pads"));
}

/// REQ: F-096
#[test]
fn catalogue_096_quiet_procedural_and_machines_rows_exist() {
    let rows = catalogue();
    assert_eq!(scene_names(&rows.iter().find(|r| r[0] == "Quiet procedural").unwrap()[1]).len(), 6);
    assert!(rows.iter().any(|r| r[0] == "Machines and clocks" && r[1].contains("Cube clock")));
}

/// REQ: F-097
#[test]
fn catalogue_097_geometry_pipes_wind_growth_rows_exist() {
    let rows = catalogue();
    assert_eq!(scene_names(&rows.iter().find(|r| r[0] == "Geometry and mathematics").unwrap()[1]).len(), 11);
    assert!(rows.iter().any(|r| r[0] == "Pipes, wind, growth" && r[1].contains("3D pipes") && r[1].contains("Wind") && r[1].contains("Growth")));
}

/// REQ: F-098
#[test]
fn catalogue_098_simulations_and_games_rows_exist() {
    let rows = catalogue();
    assert!(rows.iter().any(|r| r[0] == "Games" && r[1].contains("Galactic empires") && r[1].contains("Hex expedition") && r[1].contains("Vector TD")));
    assert!(rows.iter().any(|r| r[0] == "Carpet simulations" && r[1].contains("Slow Life")));
}

/// REQ: F-099
#[test]
fn catalogue_099_live_data_row_lists_four_sources() {
    let rows = catalogue();
    let row = rows.iter().find(|r| r[0] == "Live data").expect("row");
    for name in ["Earthquakes", "aircraft", "boats", "live graphs"] {
        assert!(row[1].contains(name), "{name}");
    }
}

/// REQ: F-100
#[test]
fn catalogue_100_maps_and_space_and_wikipedia_rows_exist() {
    let rows = catalogue();
    let maps = rows.iter().find(|r| r[0] == "Maps and space").expect("row");
    assert_eq!(scene_names(&maps[1]).len(), 6);
    assert!(rows.iter().any(|r| r[0] == "Wikipedia"));
}

/// REQ: F-101
#[test]
fn catalogue_101_media_rows_video_spectrum_images() {
    let rows = catalogue();
    for family in ["Video", "Audio spectrum", "Images"] {
        assert!(rows.iter().any(|r| r[0] == family), "{family}");
    }
}

/// REQ: F-095, F-096, F-097, F-098, F-099, F-100, F-101
#[test]
fn catalogue_095_every_listed_scene_name_is_documented_for_users() {
    for row in catalogue() {
        let doc = user_doc(&row[2]).to_lowercase();
        for name in scene_names(&row[1]) {
            if name.contains("chess") || name.contains("Lichess") || name.starts_with("automated") {
                continue;
            }
            assert!(doc.contains(&name.to_lowercase()), "{name} missing in src/docs/{}.md", row[2]);
        }
    }
}

/// REQ: F-095
#[test]
fn catalogue_095_boundary_rows_have_nonempty_family_and_phase() {
    for row in catalogue() {
        assert!(!row[0].is_empty() && !row[1].is_empty());
        assert!(["4c", "4d", "4e"].contains(&row[3].as_str()), "{} phase {}", row[0], row[3]);
    }
}

/// REQ: F-103
#[test]
fn catalogue_103_semantic_animation_is_owned_with_titles() {
    let trace = page("reference-feature-traceability");
    let line = trace.lines().find(|l| l.starts_with("| F-103")).unwrap();
    assert!(line.contains("scenes plugin with titles"));
    assert!(user_doc("animations").to_lowercase().contains("semantic"));
}

/// REQ: F-102
#[test]
fn catalogue_102_packaged_javascript_is_retired_into_wasm_scene_packages() {
    let trace = page("reference-feature-traceability");
    let line = trace.lines().find(|l| l.starts_with("| F-102")).unwrap();
    assert!(line.contains("retired"));
}
