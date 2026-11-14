//! Reading the settings of a scene spec, and checking a scene's rows against its manifest.
//!
//! The host shows a scene's rows from `settings_schema` of the manifest entry and sends the
//! edited values as the `settings` object of the spec (or as the spec itself, for a bare
//! object). A scene keeps its typed settings struct (serde, `#[serde(default)]`); the keys of
//! the struct are the property names of the schema.

use super::control::SceneSettings;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The settings object of a scene spec: the `settings` member when the spec has one, else
/// the spec itself (a bare settings object).
pub fn settings_object(spec: &Value) -> &Value {
    match spec.get("settings") {
        Some(settings) if settings.is_object() => settings,
        _ => spec,
    }
}

/// The settings of type `T` a spec describes, clamped to their ranges. Anything missing or
/// unreadable keeps its default.
pub fn from_spec<T: SceneSettings + DeserializeOwned + Default>(spec: &Value) -> T {
    let object = settings_object(spec);
    serde_json::from_value::<T>(object.clone())
        .unwrap_or_default()
        .normalized()
}

/// Checks that the rows `controls` and the properties of one manifest scene entry agree on
/// keys, kinds, ranges and defaults. Used by each scene's tests so the rows the host shows
/// are exactly the ones the scene understands.
#[cfg(test)]
pub fn assert_schema_matches_controls(
    manifest_scene_id: &str,
    controls: &[super::control::Control],
) {
    use super::control::{ControlKind, ControlValue};
    let manifest: Value = serde_json::from_str(include_str!("../../manifest.json"))
        .expect("manifest.json is valid JSON");
    let scene = manifest["contributes"]["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|scene| scene["id"] == manifest_scene_id))
        .unwrap_or_else(|| panic!("scene {manifest_scene_id} is in the manifest"));
    let properties = scene["settings_schema"]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("scene {manifest_scene_id} has a properties object"));
    let mut expected: Vec<&str> = controls.iter().map(|control| control.id).collect();
    expected.sort_unstable();
    let mut declared: Vec<&str> = properties.keys().map(String::as_str).collect();
    declared.sort_unstable();
    assert_eq!(
        declared, expected,
        "scene {manifest_scene_id}: manifest properties differ from the scene's rows"
    );
    for control in controls {
        let property = &properties[control.id];
        match (&control.kind, &control.value) {
            (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) => {
                assert_eq!(
                    property["minimum"].as_i64(),
                    Some(i64::from(*min)),
                    "{} minimum",
                    control.id
                );
                assert_eq!(
                    property["maximum"].as_i64(),
                    Some(i64::from(*max)),
                    "{} maximum",
                    control.id
                );
                assert_eq!(
                    property["default"].as_i64(),
                    Some(i64::from(*value)),
                    "{} default",
                    control.id
                );
            }
            (ControlKind::Toggle, ControlValue::Bool(on)) => {
                assert_eq!(property["type"], "boolean", "{} type", control.id);
                assert_eq!(
                    property["default"].as_bool(),
                    Some(*on),
                    "{} default",
                    control.id
                );
            }
            (ControlKind::Choice { options }, ControlValue::Index(index)) => {
                let titles: Vec<&str> = property["enum_titles"]
                    .as_array()
                    .map(|titles| titles.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                assert_eq!(&titles, options, "{} option titles", control.id);
                let values = property["enum"].as_array().map_or(0, Vec::len);
                assert_eq!(values, options.len(), "{} option count", control.id);
                let default = property["default"].as_str().unwrap_or_default();
                let position = property["enum"]
                    .as_array()
                    .and_then(|values| values.iter().position(|value| value == default));
                assert_eq!(position, Some(*index), "{} default option", control.id);
            }
            (ControlKind::Text { .. }, ControlValue::Text(text)) => {
                assert_eq!(property["type"], "string", "{} type", control.id);
                assert_eq!(property["default"].as_str(), Some(text.as_str()));
            }
            _ => panic!("{}: unsupported control kind", control.id),
        }
    }
}
