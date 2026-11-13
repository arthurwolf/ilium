//! Twins of the animation help tests of ilium-client/src/settings_help/tests.rs, run against the
//! plugin's `help`, `panel` and `rows` modules. The kernel tests resolve the topic ids against the
//! help catalogue (kernel, `settings_help/catalog`) and read the anchors the Settings screen lays
//! out. The plugin owns which topic id each row maps to and how the selected row's help line is
//! built, so each twin asserts that mapping on the synthetic registry of `panel_fixture`. The
//! catalogue text and the anchor drawing stay with the client (KERNEL-STAYS).

use crate::panel_fixture::*;
use ilium_plugin_core_ambient::help::{help_id, help_ids, SCENE_CONTROL_TOPICS, STYLE_HELP_IDS};
use ilium_plugin_core_ambient::panel::{selected_control_help, Scrolls};
use ilium_plugin_core_ambient::rows::{AnimationRow, Region};
use std::collections::HashSet;

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::animation_help_covers_shared_palette_and_all_named_scene_controls
#[test]
fn animation_help_covers_shared_palette_and_all_named_scene_controls_twin() {
    let known: HashSet<String> = help_ids().into_iter().collect();
    // The shared palette topics and the four named scene controls are part of the catalogue ids.
    for id in [
        "AN-30", "AN-31", "AN-32", "AN-39", "AN-40", "AN-41", "AN-42",
    ] {
        assert!(known.contains(id), "{id} is a known help topic");
    }
    // Every named scene control of every scene resolves to its numbered shared topic in order.
    let ids = scene_ids();
    for scene in SCENES {
        let controls = scene_controls_for(scene);
        for (position, control) in controls.iter().enumerate() {
            let topic = help_id(&AnimationRow::SceneControl(control.id), &controls, &ids);
            let expected = format!("AN-{:02}", 39 + position.min(SCENE_CONTROL_TOPICS - 1));
            assert_eq!(topic, expected, "{scene}: {}", control.id);
            assert!(known.contains(&topic), "{scene}: {topic} resolves");
        }
    }
    // The semantic scope row has a topic of its own.
    assert_eq!(
        help_id(&AnimationRow::SceneControl("semantic_scope"), &[], &ids),
        "AN-65"
    );
    // Scene rows map onto the named topics, the rest onto AN-01.. in registry order.
    for (index, scene) in SCENES.iter().enumerate() {
        let topic = help_id(&AnimationRow::Scene((*scene).to_owned()), &[], &ids);
        assert!(known.contains(&topic), "{scene}: {topic} resolves");
        let named = matches!(
            *scene,
            "boats"
                | "voxel-landscape"
                | "open-street-map"
                | "wikipedia"
                | "semantic"
                | "topographic-maps"
                | "aurora"
        ) || *scene == ilium_plugin_core_ambient::rows::OPENSTREETMAP_ID;
        if !named {
            assert_eq!(topic, format!("AN-{:02}", index + 1), "{scene}");
        }
    }
    assert_eq!(
        help_id(
            &AnimationRow::Scene("voxel-landscape".to_owned()),
            &[],
            &ids
        ),
        "AN-52"
    );
    assert_eq!(
        help_id(&AnimationRow::Scene("wikipedia".to_owned()), &[], &ids),
        "AN-54"
    );
    assert_eq!(
        help_id(
            &AnimationRow::Scene("topographic-maps".to_owned()),
            &[],
            &ids
        ),
        "AN-56"
    );
    // The shared look rows carry their own numbered style topics, distinct from the scene ones.
    let mut seen = HashSet::new();
    for (row, topic) in STYLE_HELP_IDS {
        assert!(
            seen.insert(topic),
            "style topic {topic} of {row} is used once"
        );
        assert!(known.contains(topic), "{row}: {topic} resolves");
    }
    for (row, topic) in STYLE_HELP_IDS {
        let row_id = AnimationRow::Common(row);
        assert_eq!(help_id(&row_id, &[], &ids), topic);
    }
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::narrow_animation_settings_help_anchors_reach_every_lower_control
#[test]
fn narrow_animation_settings_help_anchors_reach_every_lower_control_twin() {
    let known: HashSet<String> = help_ids().into_iter().collect();
    let ids = scene_ids();
    for scene in SCENES {
        let model = fixture_model(scene);
        let controls = scene_controls_for(scene);
        let panel = panel(area(80, 24), &model);
        let mut scrolls = Scrolls::default();
        for row in 0..model.len() {
            scrolls = panel.follow_selection(row, scrolls);
            let topic = help_id(model.row(row).unwrap(), &controls, &ids);
            assert!(
                known.contains(&topic),
                "{scene}: row {row}: {topic} resolves"
            );
            // The selected row is inside its own region's window, so its help anchor stays reachable.
            let region = model.region(row).unwrap();
            let position = model.visual_row(row).unwrap();
            let top = scrolls.get(region);
            let visible = panel.visible_rows(region);
            assert!(
                position >= top && position < top.saturating_add(visible.max(1)),
                "{scene}: row {row} ({region:?}) at {position} is outside {top}..{}",
                top + visible
            );
            // The pinned help line of that row exists and names it.
            let help = selected_control_help(&model, row);
            let view = model.view(row).unwrap();
            assert!(help.starts_with(&view.label), "{scene}: row {row}: {help}");
        }
        assert_eq!(scrolls, panel.clamped(scrolls), "{scene}");
        // Lower controls: the last control row follows into view in a compact right column.
        let last = model
            .region_rows(Region::Controls)
            .last()
            .expect("every scene has per-animation rows");
        let after = panel.follow_selection(last, Scrolls::default());
        let position = model.visual_row(last).unwrap();
        let top = after.get(Region::Controls);
        assert!(
            position >= top && position < top + panel.visible_rows(Region::Controls).max(1),
            "{scene}: the last control row is reachable"
        );
    }
}
