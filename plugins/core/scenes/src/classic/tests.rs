use super::*;
use crate::kit::control::{ControlValue, SceneSettings};
use crate::kit::debug::render_frame;
use crate::kit::settings::assert_schema_matches_controls;
use serde_json::json;
use std::time::Duration;

/// The spec the host would send for `id` with every manifest default.
fn default_spec(id: &str) -> Value {
    let manifest: Value =
        serde_json::from_str(include_str!("../../manifest.json")).expect("manifest.json is valid");
    let scene = manifest["contributes"]["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|scene| scene["id"] == id))
        .cloned()
        .unwrap_or_else(|| panic!("scene {id} is in the manifest"));
    let mut settings = serde_json::Map::new();
    if let Some(properties) = scene["settings_schema"]["properties"].as_object() {
        for (key, property) in properties {
            settings.insert(key.clone(), property["default"].clone());
        }
    }
    json!({ "scene": id, "settings": settings, "palette": [], "mode": "live" })
}

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
fn manifest_rows_match_every_classic_scene_and_defaults_round_trip() {
    check::<ShorelineSettings>("shoreline");
    check::<MoonlitWaterSettings>("moonlit-water");
    check::<SleepingRidgeSettings>("sleeping-ridge");
    check::<WindyHillsideSettings>("windy-hillside");
    check::<TeaSteamSettings>("tea-steam");
    check::<KelpSettings>("kelp");
    check::<StoneCausticsSettings>("stone-caustics");
    check::<CloudletSettings>("cloudlets");
    check::<TwoRipplesSettings>("two-ripples");
    check::<QuietPondSettings>("quiet-pond");
}

#[test]
fn every_classic_scene_builds_and_draws_the_same_frame_at_the_same_time() {
    let env = SceneEnv::for_test();
    for id in IDS {
        let spec = default_spec(id);
        let mut first = create(id, &spec, &env).unwrap_or_else(|| panic!("{id} builds"));
        let mut second = create(id, &spec, &env).unwrap_or_else(|| panic!("{id} builds"));
        let at = Duration::from_secs(7);
        let one = render_frame(first.as_mut(), 48, 14, at);
        // Seekable: a scene that already drew other instants draws the same frame again.
        let _ = render_frame(second.as_mut(), 48, 14, Duration::from_secs(2));
        let two = render_frame(second.as_mut(), 48, 14, at);
        assert!(one.lit_dots() > 0, "{id} draws nothing at 7 s");
        assert_eq!(one.lit_dots(), two.lit_dots(), "{id} is not seekable");
    }
}

#[test]
fn the_registry_reaches_every_classic_scene_and_nothing_else() {
    let env = SceneEnv::for_test();
    for id in IDS {
        assert!(
            crate::registry::create(id, &default_spec(id), &env).is_some(),
            "{id} is reachable from the registry"
        );
    }
    assert!(create("no-such-scene", &json!({}), &env).is_none());
}

#[test]
fn classic_shoreline_style_hides_the_rich_rows_and_rich_shows_them() {
    let rich = ShorelineSettings::default();
    let classic = ShorelineSettings {
        style: ShorelineStyle::Classic,
        ..rich
    };
    assert_eq!(rich.controls().len(), 4 + 1 + 15);
    assert_eq!(classic.controls().len(), 4 + 1);
}

#[test]
fn set_control_clamps_and_reports_change() {
    let mut settings = TeaSteamSettings::default();
    assert_eq!(
        settings.set_control("steam_ribbons", ControlValue::Number(99)),
        Ok(true)
    );
    assert_eq!(settings.steam_ribbons, 6);
    assert_eq!(
        settings.set_control("steam_ribbons", ControlValue::Number(6)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("no_such_row", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("steam_ribbons", ControlValue::Bool(true))
        .is_err());
}
