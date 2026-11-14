//! The `[animations]` table that the ambient plugin writes (`ilium.core.ambient/set-setting`),
//! turned into the scene the client shows.
//!
//! The plugin stores `scene` and `semantic` through `kernel-settings` (tab `animations`, which
//! lives in the `[animations]` table of `config.toml`). The animation itself is a client
//! setting kept in the animation home's `.ilium/config.yaml`, so nothing applies a plugin's
//! write until [`requested_kind`] decides what the table asks for and the App selects it
//! (`App::apply_plugin_animation_table`). This module is pure: no I/O.

use crate::background_animation::AnimationKind;

/// The scene the `scene` row of the plugin manifest defaults to.
const DEFAULT_SCENE: &str = "stars";

/// The scene the `[animations]` table asks for, or `None` when it asks for no change.
///
/// - `semantic = true` selects the semantic animation, whatever `scene` says.
/// - Otherwise `scene` names the scene by its kernel id (`stars`, `clouds`, ...). An unknown
///   name asks for nothing.
/// - `semantic = false` while the semantic animation is current falls back to `scene`, or to
///   the manifest default when the table holds none.
pub(crate) fn requested_kind(table: &toml::Table, current: AnimationKind) -> Option<AnimationKind> {
    let semantic = table.get("semantic").and_then(toml::Value::as_bool);
    let scene = table
        .get("scene")
        .and_then(toml::Value::as_str)
        .and_then(scene_kind);
    let wanted = if semantic == Some(true) {
        AnimationKind::Semantic
    } else if let Some(scene) = scene {
        scene
    } else if semantic == Some(false) && current == AnimationKind::Semantic {
        scene_kind(DEFAULT_SCENE)?
    } else {
        return None;
    };
    (wanted != current).then_some(wanted)
}

/// A scene by its kernel id, which is the snake-case name of an [`AnimationKind`].
fn scene_kind(name: &str) -> Option<AnimationKind> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).expect("the test table parses")
    }

    #[test]
    fn a_scene_name_selects_that_scene() {
        assert_eq!(
            requested_kind(&table("scene = \"clouds\""), AnimationKind::Stars),
            Some(AnimationKind::Clouds)
        );
    }

    #[test]
    fn the_current_scene_asks_for_no_change() {
        assert_eq!(
            requested_kind(&table("scene = \"stars\""), AnimationKind::Stars),
            None
        );
    }

    #[test]
    fn an_unknown_scene_asks_for_nothing() {
        assert_eq!(
            requested_kind(&table("scene = \"no-such-scene\""), AnimationKind::Stars),
            None
        );
        assert_eq!(
            requested_kind(&table("scene = 3"), AnimationKind::Stars),
            None
        );
    }

    #[test]
    fn semantic_on_selects_the_semantic_animation_over_the_scene() {
        assert_eq!(
            requested_kind(
                &table("scene = \"clouds\"\nsemantic = true"),
                AnimationKind::Stars
            ),
            Some(AnimationKind::Semantic)
        );
    }

    #[test]
    fn semantic_off_returns_to_the_stored_scene() {
        assert_eq!(
            requested_kind(
                &table("scene = \"clouds\"\nsemantic = false"),
                AnimationKind::Semantic
            ),
            Some(AnimationKind::Clouds)
        );
    }

    #[test]
    fn semantic_off_without_a_scene_returns_to_the_manifest_default() {
        assert_eq!(
            requested_kind(&table("semantic = false"), AnimationKind::Semantic),
            Some(AnimationKind::Stars)
        );
    }

    #[test]
    fn semantic_off_leaves_any_other_scene_alone() {
        assert_eq!(
            requested_kind(&table("semantic = false"), AnimationKind::Clouds),
            None
        );
    }

    #[test]
    fn an_empty_table_asks_for_nothing() {
        assert_eq!(
            requested_kind(&toml::Table::new(), AnimationKind::Semantic),
            None
        );
    }

    #[test]
    fn the_keys_the_client_does_not_apply_are_ignored() {
        assert_eq!(
            requested_kind(
                &table("observer = { lat = 1.0, lon = 2.0 }\n[saved_places]\nhome = { lat = 1.0, lon = 2.0 }"),
                AnimationKind::Stars
            ),
            None
        );
    }
}
