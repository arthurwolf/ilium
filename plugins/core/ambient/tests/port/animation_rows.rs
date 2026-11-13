//! Twins of the tests of ilium-client/src/animation_rows.rs, run against the plugin's `rows`
//! module. The kernel tests build rows from `AnimationSettings`; the plugin takes the scene
//! registry and the shared look as `RowInputs`, so each twin builds the same inputs by hand.

use ilium_plugin_core_ambient::control::{Control, ControlKind, ControlValue};
use ilium_plugin_core_ambient::rows::{
    cache_line, AnimationRow, CacheStatus, RowContext, RowInputs, RowKind, RowModel, SceneEntry,
    OPENSTREETMAP_ID,
};
use ilium_plugin_core_ambient::values::VOXEL_LANDSCAPE_ID;

fn scene(id: &str) -> SceneEntry {
    SceneEntry {
        id: id.to_owned(),
        label: id.to_owned(),
        description: format!("The {id} scene."),
    }
}

/// A representative registry slice plus the shared rows the kernel derives from `Appearance`.
fn inputs(selected: &str) -> RowInputs {
    RowInputs {
        scenes: [VOXEL_LANDSCAPE_ID, OPENSTREETMAP_ID, "boats", "rain"]
            .into_iter()
            .map(scene)
            .collect(),
        selected: selected.to_owned(),
        look: vec![Control::choice(
            "look_mode",
            "Color mode",
            0,
            &["Color", "Greyscale", "Monotone"],
            "",
        )],
        pattern: vec![Control::slider(
            "look_pattern_contrast",
            "Pattern contrast",
            0,
            (-100, 100, 5),
            "%",
            "",
        )],
        common: vec![
            Control::toggle("panels", "Panels", true, ""),
            Control::toggle("background", "Background", true, ""),
            Control::slider("speed", "Speed", 100, (10, 400, 10), "%", ""),
            Control::slider("fps_limit", "Frame rate", 30, (1, 60, 1), " fps", ""),
            Control::slider("density", "Density", 100, (10, 300, 10), "%", ""),
            Control::choice("dither", "Dither", 0, &["Off", "Bayer"], ""),
            Control::choice("playback", "Playback", 1, &["Live", "Loop"], ""),
            Control::slider("loop_seconds", "Loop length", 12, (1, 120, 1), "s", ""),
        ],
        playback_loop: true,
        is_hosted: true,
        observer: "Lisbon (38.72, -9.14)".to_owned(),
        map_location: Some("Paris (48.86, 2.35)".to_owned()),
        ..RowInputs::default()
    }
}

fn index_of(model: &RowModel, row: AnimationRow) -> usize {
    model
        .rows()
        .iter()
        .position(|candidate| *candidate == row)
        .unwrap_or_else(|| panic!("row {row:?} missing"))
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::voxel_world_seed_is_a_wide_number_over_the_original_text_control
#[test]
fn voxel_world_seed_is_a_wide_number_over_the_original_text_control_twin() {
    let mut voxel = inputs(VOXEL_LANDSCAPE_ID);
    voxel.scene_controls = vec![Control::text(
        "seed",
        "World seed",
        "71839",
        "0-4294967295",
        "",
    )];
    let model = RowModel::new(&voxel, &RowContext::default());
    let row = index_of(&model, AnimationRow::SceneControl("seed"));
    let control = model.control(row).expect("native seed control");
    assert!(matches!(control.kind, ControlKind::Text { .. }));
    assert!(matches!(control.value, ControlValue::Text(_)));
    assert_eq!(
        model.view(row).and_then(|view| view.number()),
        Some((0, u32::MAX as i128, 1, 71_839)),
    );
    assert!(model.view(row).unwrap().slider().is_none());
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::displayed_controls_retain_their_full_option_and_numeric_metadata
#[test]
fn displayed_controls_retain_their_full_option_and_numeric_metadata_twin() {
    for selected in [VOXEL_LANDSCAPE_ID, OPENSTREETMAP_ID, "boats", "rain"] {
        let mut data = inputs(selected);
        data.scene_controls = vec![
            Control::choice("backend", "Backend", 0, &["Software", "GPU"], "")
                .with_disabled_option(1, "No device. Install one."),
            Control::slider("count", "Count", 5, (0, 9, 1), "", ""),
        ];
        let model = RowModel::new(&data, &RowContext::default());
        for (index, row) in model.rows().iter().enumerate() {
            let expected = match row {
                AnimationRow::Common(id) => data
                    .look
                    .iter()
                    .chain(&data.pattern)
                    .chain(&data.common)
                    .find(|control| control.id == *id),
                AnimationRow::SceneControl(id) => {
                    data.scene_controls.iter().find(|control| control.id == *id)
                }
                _ => None,
            };
            assert_eq!(model.control(index), expected, "{selected}: {row:?}");
            if let Some(control) = model.control(index) {
                assert_eq!(model.view(index).unwrap().value, control.display_value());
            }
        }
        assert!(model.control(model.len()).is_none());
        let backend = index_of(&model, AnimationRow::SceneControl("backend"));
        let view = model.view(backend).unwrap();
        assert_eq!(view.disabled_options.len(), 1);
        assert_eq!(
            view.disabled_notice().as_deref(),
            Some("GPU unavailable: No device.")
        );
    }
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::overhaul_ram_usage_is_visible_before_progress_details
#[test]
fn overhaul_ram_usage_is_visible_before_progress_details_twin() {
    let context = RowContext {
        loop_bytes: 1024 * 1024,
        ..Default::default()
    };
    let text = cache_line(&context);
    assert!(
        text.starts_with("RAM "),
        "RAM must survive short readout truncation: {text}"
    );
    let building = RowContext {
        cache: CacheStatus {
            completed_frames: 5,
            total_frames: 10,
            ..Default::default()
        },
        ..context
    };
    assert!(cache_line(&building).contains(" 50% building"));
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::overhaul_loop_duration_uses_a_logarithmic_track
#[test]
fn overhaul_loop_duration_uses_a_logarithmic_track_twin() {
    let model = RowModel::new(&inputs("rain"), &RowContext::default());
    let index = index_of(&model, AnimationRow::Common("loop_seconds"));
    let slider = model.view(index).unwrap().slider().unwrap();
    assert_eq!(slider.value_at(0, 101), 1);
    assert_eq!(slider.value_at(100, 101), 120);
    assert!((10..=12).contains(&slider.value_at(50, 101)));
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::osm_location_row_displays_the_scene_selection_not_the_shared_observer
#[test]
fn osm_location_row_displays_the_scene_selection_not_the_shared_observer_twin() {
    let model = RowModel::new(&inputs(OPENSTREETMAP_ID), &RowContext::default());
    let index = index_of(&model, AnimationRow::Location);
    let view = model.view(index).unwrap();
    assert_eq!(view.label, "Map location");
    assert!(!view.value.contains("Lisbon"));
    assert!(view.value.contains("Paris"));
    let mut observer_scene = inputs("rain");
    observer_scene.uses_location = true;
    let model = RowModel::new(&observer_scene, &RowContext::default());
    let view = model
        .view(index_of(&model, AnimationRow::Location))
        .unwrap();
    assert_eq!(view.label, "Location");
    assert!(view.value.contains("Lisbon"));
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::saved_scene_status_keeps_the_bar_compact_and_the_activity_log_in_help
#[test]
fn saved_scene_status_keeps_the_bar_compact_and_the_activity_log_in_help_twin() {
    let status = "Route [====......] 4/16\nElapsed: 00:42 \u{b7} ETA unavailable\nRecent activity:\n+00:08 \u{b7} Scanning saved region files";
    let model = RowModel::new(
        &inputs(VOXEL_LANDSCAPE_ID),
        &RowContext {
            scene_status: Some(status.to_owned()),
            ..Default::default()
        },
    );
    let view = model
        .view(index_of(&model, AnimationRow::SceneStatus))
        .unwrap();
    assert_eq!(view.value, "Route [====......] 4/16");
    assert!(view.help.contains("Elapsed: 00:42 \u{b7} ETA unavailable"));
    assert!(view.help.contains("Scanning saved region files"));
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::boat_information_is_exposed_as_full_text_prompt_values
#[test]
fn boat_information_is_exposed_as_full_text_prompt_values_twin() {
    let mut boats = inputs("boats");
    boats.scene_controls = [
        ("boat_credit", "OpenSeaFeed contributors"),
        ("boat_provider", "https://openseafeed.com/"),
        (
            "boat_license",
            "https://creativecommons.org/licenses/by/4.0/",
        ),
        ("boat_coverage", "position age unavailable"),
        ("boat_changes", "projected"),
    ]
    .into_iter()
    .map(|(id, value)| Control::text(id, id, value, "", ""))
    .collect();
    let model = RowModel::new(&boats, &RowContext::default());
    for (id, expected) in [
        ("boat_credit", "OpenSeaFeed contributors"),
        ("boat_provider", "https://openseafeed.com/"),
        (
            "boat_license",
            "https://creativecommons.org/licenses/by/4.0/",
        ),
        ("boat_coverage", "position age unavailable"),
        ("boat_changes", "projected"),
    ] {
        let view = model
            .view(index_of(&model, AnimationRow::SceneControl(id)))
            .unwrap();
        assert_eq!(view.kind, RowKind::Text);
        assert!(view.value.contains(expected));
    }
}
