//! The scenes of this package that run on the shared scene kit: scene id (the manifest
//! `contributes.scenes[].id`) to the function that builds the scene from the spec.
//!
//! Adding a scene is one arm of `create` plus its manifest entry; a scene needs nothing else
//! from the dispatcher in `lib.rs`.

use crate::kit::scene::{Scene, SceneEnv};
use serde_json::Value;

/// Builds the kit scene `scene_id` from the `spec` (the host's spec or a bare settings
/// object). `None` for an id this registry does not know (the dispatcher draws nothing then).
pub fn create(scene_id: &str, spec: &Value, env: &SceneEnv) -> Option<Box<dyn Scene>> {
    Some(match scene_id {
        "atlantic-dusk" => crate::atlantic_dusk::create(spec, env),
        "carpet" => crate::carpet::create(spec, env),
        "cube-clock" => crate::cube_clock::create(spec, env),
        "satellite-clouds" => crate::clouds::create(spec, env),
        "dither-water" => crate::dither_water::create(spec, env),
        "fbm-clouds" => crate::fbm_clouds::create(spec, env),
        "hex-expedition" => crate::hex_expedition::create(spec, env),
        "live-chess" => crate::live_chess::scene::create(spec, env),
        "machine-screen" => crate::machine_screen::create(spec, env),
        "night-lights" => crate::night_lights::create(spec, env),
        "pipes" => crate::pipes::create(spec, env),
        "pi-digits" => crate::pi_digits::create(spec, env),
        "stars-overhead" => crate::stars::create(spec, env),
        "topographic-maps" => crate::topographic_maps::create(spec, env),
        "wind" => crate::wind::create(spec, env),
        // The ten classic scenes (shoreline, moonlit water, ... quiet pond).
        id if crate::classic::IDS.contains(&id) => return crate::classic::create(id, spec, env),
        // The scenes of the second port round (quiet scenes, box machine, galactic empires,
        // dithr patterns, solar system) have their own table in `scenes2`.
        _ => return crate::scenes2::create(scene_id, spec, env),
    })
}
