//! Tests of the second-round scene table: the manifest entries agree with each scene's rows,
//! a spec of manifest defaults gives the default settings, and every scene draws something.

use super::*;
use crate::box_machine::BoxMachineSettings;
use crate::dithr_patterns::DithrPatternsSettings;
use crate::galactic_empires::GalacticEmpiresSettings;
use crate::kit::debug::render_frame;
use crate::kit::settings::assert_schema_matches_controls;
use crate::quiet::{
    almost_touching::AlmostTouchingSettings, aurora::AuroraSettings,
    crop_circles::CropCirclesSettings, delayed_reflection::DelayedReflectionSettings,
    fireflies::FirefliesSettings, hidden_wheel::HiddenWheelSettings,
    lighthouse::LighthouseSettings, needle_threads::NeedleThreadsSettings,
    paper_fold::PaperFoldSettings, pollen::PollenSettings,
    prime_constellations::PrimeConstellationsSettings, unfinished_circle::UnfinishedCircleSettings,
    wallpaper::WallpaperSettings, window_sunlight::WindowSunlightSettings,
};
use crate::solar_system::SolarSystemSettings;
use serde_json::json;
use std::time::Duration;

fn manifest_scene(id: &str) -> Value {
    let manifest: Value = serde_json::from_str(include_str!("../../manifest.json"))
        .expect("manifest.json is valid JSON");
    manifest["contributes"]["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|scene| scene["id"] == id))
        .cloned()
        .unwrap_or_else(|| panic!("scene {id} is in the manifest"))
}

/// The spec the host would send when every row shows its default.
fn default_spec(id: &str) -> Value {
    let scene = manifest_scene(id);
    let mut settings = serde_json::Map::new();
    if let Some(properties) = scene["settings_schema"]["properties"].as_object() {
        for (key, property) in properties {
            settings.insert(key.clone(), property["default"].clone());
        }
    }
    json!({ "scene": id, "settings": settings, "palette": [], "mode": "live" })
}

/// Checks one scene: rows against the manifest, and the default spec against the defaults.
fn check<S: SceneSettings + Default + PartialEq + std::fmt::Debug>(id: &str) {
    assert_schema_matches_controls(id, &S::default().controls());
    let from_manifest: S = settings_from_spec(&default_spec(id));
    assert_eq!(
        from_manifest,
        S::default().normalized(),
        "{id}: manifest defaults"
    );
}

#[test]
fn manifest_rows_match_every_scene_and_defaults_round_trip() {
    check::<AlmostTouchingSettings>("almost-touching");
    check::<AuroraSettings>("aurora");
    check::<CropCirclesSettings>("crop-circles");
    check::<DelayedReflectionSettings>("delayed-reflection");
    check::<FirefliesSettings>("fireflies");
    check::<HiddenWheelSettings>("hidden-wheel");
    check::<LighthouseSettings>("lighthouse");
    check::<NeedleThreadsSettings>("needle-threads");
    check::<PaperFoldSettings>("paper-fold");
    check::<PollenSettings>("pollen");
    check::<PrimeConstellationsSettings>("prime-constellations");
    check::<UnfinishedCircleSettings>("unfinished-circle");
    check::<WallpaperSettings>("wallpaper");
    check::<WindowSunlightSettings>("window-sunlight");
    check::<BoxMachineSettings>("box-machine");
    check::<DithrPatternsSettings>("dithr-patterns");
    check::<GalacticEmpiresSettings>("galactic-empires");
    check::<SolarSystemSettings>("solar-system");
}

#[test]
fn every_listed_scene_is_in_the_manifest_and_builds() {
    let env = SceneEnv::for_test();
    for id in IDS {
        let spec = default_spec(id);
        let mut scene = create(id, &spec, &env).unwrap_or_else(|| panic!("{id} builds"));
        let lit: usize = [1, 5, 12]
            .into_iter()
            .map(|seconds| {
                render_frame(scene.as_mut(), 40, 12, Duration::from_secs(seconds)).lit_dots()
            })
            .sum();
        assert!(lit > 0, "{id} draws no ink at 1, 5 or 12 s");
    }
}

#[test]
fn an_unknown_scene_id_builds_nothing() {
    assert!(create("no-such-scene", &json!({}), &SceneEnv::for_test()).is_none());
}

#[test]
fn option_values_are_lower_case_dash_separated_labels() {
    assert_eq!(option_value("Milk Hill six arms"), "milk-hill-six-arms");
    assert_eq!(option_value("2 x 3"), "2-x-3");
    assert_eq!(option_value("Quarter turns"), "quarter-turns");
    assert_eq!(option_value("  Frame edges! "), "frame-edges");
}

#[test]
fn a_choice_arrives_as_its_enum_string_or_its_index() {
    let by_name: WallpaperSettings = settings_from_spec(
        &json!({"settings": {"motif": "pinwheels", "symmetry": "quarter-turns"}}),
    );
    assert_eq!((by_name.motif, by_name.symmetry), (2, 2));
    let bare: WallpaperSettings = settings_from_spec(&json!({"motif": 1}));
    assert_eq!(bare.motif, 1);
    let unusable: WallpaperSettings =
        settings_from_spec(&json!({"motif": "no-such", "columns": "x"}));
    assert_eq!(unusable, WallpaperSettings::default());
}

#[test]
fn numbers_are_clamped_and_unknown_keys_are_ignored() {
    let loud: AuroraSettings = settings_from_spec(
        &json!({"settings": {"curtain_count": 999, "unknown": 5, "trees": false}}),
    );
    assert_eq!(loud.curtain_count, 10);
    assert!(!loud.trees);
}
