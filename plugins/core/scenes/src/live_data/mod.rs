//! Typed, bounded public observations for live scenes: the graph of a public series, and the
//! maps of reported earthquakes, aircraft and boats.
//!
//! A scene owns no thread and no file. Every source is polled over the host's http import, one
//! step per frame (`poll`), shared request floors live in the plugin's shared storage (`rate`),
//! and fleet geometry is prepared in place when the data or the viewport changed
//! (`fleet_layer`). The Wikipedia source is a stream the scene world cannot open, so it reports
//! itself unavailable (`events`).
pub mod catalog;
pub mod chart;
pub mod events;
pub mod fetch;
pub mod fleet;
pub mod graph;
pub mod map;
pub mod maps;
pub mod model;
pub mod openseafeed;
pub mod parse;
pub mod poll;
pub mod rate;
pub mod series;

mod fleet_layer;
mod map_markers;

use crate::scene::{Scene, SceneEnv};
use serde_json::Value;

/// Manifest ids of the scenes this module builds.
pub const IDS: &[&str] = &["graph", "earthquakes", "aircraft", "boats"];

/// Builds the live-data scene `scene_id` from the `spec` (the host's spec or a bare settings
/// object); `None` for an id this module does not know.
pub fn create(scene_id: &str, spec: &Value, env: &SceneEnv) -> Option<Box<dyn Scene>> {
    use maps::{LiveMapScene, MapKind};
    let kind = match scene_id {
        "graph" => {
            let settings: graph::GraphSettings = crate::scenes2::settings_from_spec(spec);
            return Some(Box::new(graph::GraphScene::new(&settings, env)));
        }
        "earthquakes" => MapKind::Earthquakes,
        "aircraft" => MapKind::Aircraft,
        "boats" => MapKind::Boats,
        _ => return None,
    };
    let settings = maps::settings_from_spec(kind, spec);
    Some(Box::new(LiveMapScene::new(kind, &settings, env)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_listed_scene_builds_without_network_access() {
        let env = SceneEnv::for_test();
        for id in IDS {
            assert!(create(id, &json!({}), &env).is_some(), "{id} builds");
        }
    }

    #[test]
    fn an_unknown_id_builds_nothing() {
        assert!(create("no-such-scene", &json!({}), &SceneEnv::for_test()).is_none());
    }

    #[test]
    fn the_registry_reaches_the_live_scenes() {
        let env = SceneEnv::for_test();
        for id in IDS {
            assert!(
                crate::registry::create(id, &json!({}), &env).is_some(),
                "{id}"
            );
        }
    }
}
