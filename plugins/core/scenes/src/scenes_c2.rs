//! The scenes of the third port round that run on the shared scene kit (Growth and the quiet
//! scenes Frost, Embroidery and Ink that hesitates): scene id to the function that builds the
//! scene from the spec. `registry::create` reaches this table through `scenes2::create`.

use crate::kit::scene::{Scene, SceneEnv};
use crate::scenes2::settings_from_spec;
use serde_json::Value;

/// Manifest ids of the scenes this module builds, in catalogue order.
pub const IDS: &[&str] = &[
    "frost",
    "embroidery",
    "hesitating-ink",
    "growth",
    "vector-td",
];

/// Builds the scene `scene_id` from the `spec` (the host's spec or a bare settings object);
/// `None` for an id this module does not know.
pub fn create(scene_id: &str, spec: &Value, env: &SceneEnv) -> Option<Box<dyn Scene>> {
    use crate::growth::{GrowthScene, GrowthSettings};
    use crate::quiet::{
        embroidery::{EmbroideryScene, EmbroiderySettings},
        frost::{FrostScene, FrostSettings},
        hesitating_ink::{HesitatingInkScene, HesitatingInkSettings},
    };
    use crate::vector_td::{VectorTdScene, VectorTdSettings};
    Some(match scene_id {
        "frost" => {
            let settings: FrostSettings = settings_from_spec(spec);
            Box::new(FrostScene::new(&settings, env))
        }
        "embroidery" => {
            let settings: EmbroiderySettings = settings_from_spec(spec);
            Box::new(EmbroideryScene::new(&settings, env))
        }
        "hesitating-ink" => {
            let settings: HesitatingInkSettings = settings_from_spec(spec);
            Box::new(HesitatingInkScene::new(&settings, env))
        }
        "growth" => {
            let settings: GrowthSettings = settings_from_spec(spec);
            Box::new(GrowthScene::new(&settings, env))
        }
        "vector-td" => {
            let settings: VectorTdSettings = settings_from_spec(spec);
            Box::new(VectorTdScene::new(&settings, env))
        }
        // The live-data scenes (graph, earthquakes, aircraft, boats) have their own table.
        _ => return crate::live_data::create(scene_id, spec, env),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::GrowthSettings;
    use crate::kit::control::SceneSettings;
    use crate::kit::debug::render_frame;
    use crate::kit::settings::assert_schema_matches_controls;
    use crate::quiet::{
        embroidery::EmbroiderySettings, frost::FrostSettings, hesitating_ink::HesitatingInkSettings,
    };
    use crate::vector_td::VectorTdSettings;
    use serde_json::json;
    use std::time::Duration;

    fn default_spec(id: &str) -> Value {
        let manifest: Value = serde_json::from_str(include_str!("../manifest.json"))
            .expect("manifest.json is valid JSON");
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
    fn manifest_rows_match_every_scene_and_defaults_round_trip() {
        check::<FrostSettings>("frost");
        check::<EmbroiderySettings>("embroidery");
        check::<HesitatingInkSettings>("hesitating-ink");
        check::<GrowthSettings>("growth");
        check::<VectorTdSettings>("vector-td");
    }

    #[test]
    fn every_listed_scene_builds_and_draws() {
        let env = SceneEnv::for_test();
        for id in IDS {
            let mut scene =
                create(id, &default_spec(id), &env).unwrap_or_else(|| panic!("{id} builds"));
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
    fn registry_reaches_the_table_through_scenes2() {
        let env = SceneEnv::for_test();
        for id in IDS {
            assert!(
                crate::registry::create(id, &default_spec(id), &env).is_some(),
                "{id} is reachable from the registry"
            );
        }
    }
}
