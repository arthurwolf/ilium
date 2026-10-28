//! Twins of the animation help tests of ilium-client/src/settings_help/tests.rs. The owner plugin contributes the
//! topics of its own rows through `contributes.help` (core-plugin-ambient.md, "Guided setup and per-option help").
//! The entry shape `{ "id": ..., "text": ... }` is not in the manual; see API_GAPS_ports.md.

use crate::common::*;
use serde_json::json;

fn topic_text(id: &str) -> Option<String> {
    let manifest = manifest_json();
    let topics = manifest["contributes"]["help"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    topics
        .iter()
        .find(|topic| topic["id"] == json!(id))
        .and_then(|topic| topic["text"].as_str().map(str::to_owned))
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::animation_help_covers_shared_palette_and_all_named_scene_controls
#[test]
fn port_animation_help_explains_the_palette_ranges_and_has_a_topic_per_scene_control() {
    for (id, phrase) in [("AN-30", "60%"), ("AN-31", "359"), ("AN-32", "0%")] {
        let text =
            topic_text(id).unwrap_or_else(|| panic!("palette help topic {id} is contributed"));
        assert!(
            text.contains(phrase),
            "{id} explains its range or default: {text}"
        );
    }
    for index in 39..=42 {
        let id = format!("AN-{index:02}");
        assert!(
            topic_text(&id).is_some(),
            "scene-control help topic {id} is contributed"
        );
    }
    for id in ["AN-52", "AN-53", "AN-54"] {
        assert!(
            topic_text(id).is_some(),
            "scene help topic {id} is contributed"
        );
    }
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::animation_help_covers_shared_palette_and_all_named_scene_controls
#[test]
fn port_animation_help_palette_topics_have_non_empty_text_and_unknown_topics_stay_absent() {
    for id in ["AN-30", "AN-31", "AN-32"] {
        let text = topic_text(id).unwrap_or_default();
        assert!(!text.trim().is_empty(), "palette help topic {id} has text");
    }
    assert!(
        topic_text("AN-999").is_none(),
        "an undeclared help topic is not contributed"
    );
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::narrow_animation_settings_help_anchors_reach_every_lower_control
#[test]
fn port_narrow_animation_settings_page_renders_without_failures_at_minimum_size() {
    let mut host = activated_client();
    let _ = host.render(
        ilium_plugin_sdk::types::ViewTarget::SettingsPage("ambient".to_string()),
        24,
        6,
    );
    assert!(
        host.failures().is_empty(),
        "narrow settings page renders cleanly"
    );
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::narrow_animation_settings_help_anchors_reach_every_lower_control
#[test]
fn port_narrow_animation_settings_render_every_control_with_its_help_reachable() {
    let mut host = activated_client();
    let rows = manifest_json()["contributes"]["settings"][0]["sections"][0]["rows"].clone();
    let keys: Vec<String> = rows
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row["key"].as_str().map(str::to_owned))
        .collect();
    assert!(!keys.is_empty(), "the settings page declares keyed rows");
    let text = host
        .render(
            ilium_plugin_sdk::types::ViewTarget::SettingsPage("ambient".to_string()),
            80,
            24,
        )
        .text();
    for key in &keys {
        let id = manifest_row(key)["help"].as_str().map(str::to_owned);
        assert!(
            id.as_deref().is_some_and(|id| topic_text(id).is_some()),
            "row {key} names a contributed help topic"
        );
    }
    for key in &keys {
        let label = manifest_row(key)["label"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default();
        assert!(
            text.contains(&label),
            "row {key} with label {label:?} is rendered on the narrow page: {text}"
        );
    }
}
