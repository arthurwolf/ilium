use std::collections::BTreeSet;

use super::catalog;

#[test]
fn catalog_covers_every_settings_help_id_once_with_complete_content() {
    let topics = catalog::all();
    assert!(
        topics.windows(2).all(|pair| pair[0].id < pair[1].id),
        "catalog::by_id requires stable sorted IDs"
    );
    let ids = topics
        .iter()
        .map(|topic| topic.id.clone())
        .collect::<BTreeSet<_>>();
    let expected = expected_ids();

    assert_eq!(topics.len(), expected.len(), "catalog topic count");
    assert_eq!(ids.len(), topics.len(), "catalog IDs must be unique");
    assert_eq!(ids, expected, "catalog must cover the reviewed inventory");
    for topic in topics {
        assert!(!topic.title.trim().is_empty(), "{} title", topic.id);
        assert!(
            !topic.explanation.trim().is_empty(),
            "{} explanation",
            topic.id
        );
        assert!(!topic.specimen.trim().is_empty(), "{} specimen", topic.id);
        assert!(!topic.states.trim().is_empty(), "{} states", topic.id);
        assert!(!topic.frames.is_empty(), "{} frames", topic.id);
        assert!(
            !topic.motion.trim().is_empty(),
            "{} motion guidance",
            topic.id
        );
        assert!(!topic.caveat.trim().is_empty(), "{} caveat", topic.id);
        assert!(
            !topic.caveat.contains("## B"),
            "{} must not include the next response batch",
            topic.id
        );
    }

    let tree_order = catalog::by_id("AP-06").expect("AP-06 topic exists");
    for mode in [
        "Manual", "Type", "Age up", "Age down", "Name A-Z", "Name Z-A",
    ] {
        assert!(tree_order.specimen.contains(mode), "AP-06 includes {mode}");
    }
    assert!(tree_order.caveat.contains("one mode at a time"));

    let lock_closed = catalog::by_id("AP-24").expect("AP-24 topic exists");
    assert!(lock_closed.specimen.contains("Unlock"));
    assert!(lock_closed.specimen.contains("Off"));
    assert!(lock_closed.caveat.contains("Rename"));
    assert!(lock_closed.states.contains("both"));
    let separators = catalog::by_id("AP-25").expect("AP-25 topic exists");
    assert!(separators.specimen.contains("OFF"));
    assert!(separators.specimen.contains("ON"));
    assert!(separators.specimen.contains("────────"));
    assert!(separators.caveat.contains("not a tree row"));
    assert!(
        topics.iter().any(|topic| topic.frames.len() > 1),
        "catalog includes a stepping or animated specimen"
    );
}

#[test]
fn closing_help_restores_the_exact_settings_navigation_state() {
    use crate::app::{App, Mode, SettingsState, SettingsTab};
    use crate::config::MotionLevel;

    let project_directory = tempfile::tempdir().unwrap();
    let mut app = App::new(
        "settings-help-test".to_owned(),
        project_directory.path().to_path_buf(),
    );
    let settings = SettingsState {
        tab: SettingsTab::VoiceControl,
        selected_row: 4,
        scroll: 8,
        icons_preview_real: true,
        trigger_action_cursor: 2,
        ..SettingsState::default()
    };

    app.push_modal_over(
        Mode::Settings(settings),
        Mode::SettingsHelp(super::dialog::SettingsHelpState::new(
            "VOICE-03",
            1,
            MotionLevel::Off,
        )),
    );
    assert!(matches!(app.mode, Mode::SettingsHelp(_)));

    app.pop_modal();

    let Mode::Settings(restored) = app.mode else {
        panic!("closing help must restore Settings");
    };
    assert_eq!(restored.tab, SettingsTab::VoiceControl);
    assert_eq!(restored.selected_row, 4);
    assert_eq!(restored.scroll, 8);
    assert!(restored.icons_preview_real);
    assert_eq!(restored.trigger_action_cursor, 2);
}

fn expected_ids() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    add_range(&mut ids, "AP", 1, 25);
    for id in crate::animation_rows::help_ids() {
        ids.insert(id);
    }
    add_range(&mut ids, "AM", 1, 23);
    add_range(&mut ids, "IC", 1, 48);
    add_range(&mut ids, "KEY", 1, 38);
    add_range(&mut ids, "TERM", 1, 2);
    add_range(&mut ids, "ED", 1, 6);
    add_range(&mut ids, "SES", 1, 2);
    add_range(&mut ids, "GIT", 1, 7);
    add_range(&mut ids, "KAN", 1, 2);
    add_range(&mut ids, "SND", 1, 9);
    add_range(&mut ids, "VOICE", 1, 14);
    add_range(&mut ids, "RESET", 1, 3);
    add_range(&mut ids, "COST", 1, 23);
    add_range(&mut ids, "SETUP", 1, 2);
    ids.insert("DEBUG-01".to_owned());
    ids.insert("API-01".to_owned());
    add_range(&mut ids, "INF", 1, 14);
    add_range(&mut ids, "LLM", 1, 7);
    ids.insert("TITLE-01".to_owned());
    add_range(&mut ids, "TRIGGER", 1, 8);
    ids.insert("TEXTTRIGGER-01".to_owned());
    ids
}

fn add_range(ids: &mut BTreeSet<String>, prefix: &str, start: usize, end: usize) {
    for value in start..=end {
        ids.insert(format!("{prefix}-{value:02}"));
    }
}

#[test]
fn animation_help_covers_shared_palette_and_all_named_scene_controls() {
    use crate::animation_rows::{AnimationRow, RowContext};
    use crate::background_animation::{AnimationKind, AnimationSettings};

    let settings = AnimationSettings::default();
    for (id, phrase) in [("AN-30", "60%"), ("AN-31", "359"), ("AN-32", "0%")] {
        let topic = catalog::by_id(id).expect("palette help topic exists");
        assert!(
            topic.explanation.contains(phrase),
            "{id} explains its range/default"
        );
    }
    // Every built-in scene's four named controls are explained by the shared
    // numbered scene-control topics AN-39..AN-42.
    for kind in AnimationKind::ALL
        .into_iter()
        .filter(|kind| !kind.is_ambient() && *kind != AnimationKind::Wikipedia)
    {
        let selected = AnimationSettings {
            kind,
            ..settings.clone()
        };
        for (index, slider) in selected.scene_sliders().iter().enumerate() {
            let id = format!("AN-{:02}", index + 39);
            let topic = catalog::by_id(&id).expect("scene-control help topic exists");
            assert!(
                topic.specimen.contains(slider.label),
                "{id} explains {} for {}",
                slider.label,
                kind.label()
            );
        }
    }
    // Scene rows map one-to-one onto AN-01.. in catalog order and name their scene.
    for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
        let id = if kind == AnimationKind::VoxelLandscape {
            "AN-52".to_owned()
        } else if kind == AnimationKind::GalacticEmpires {
            "AN-53".to_owned()
        } else if kind == AnimationKind::Wikipedia {
            "AN-54".to_owned()
        } else if kind == AnimationKind::OpenStreetMap {
            "AN-55".to_owned()
        } else if kind == AnimationKind::TopographicMaps {
            "AN-56".to_owned()
        } else if kind == AnimationKind::Graph {
            "AN-57".to_owned()
        } else if kind == AnimationKind::Pi {
            "AN-58".to_owned()
        } else if kind == AnimationKind::Earthquakes {
            "AN-59".to_owned()
        } else if kind == AnimationKind::Aircraft {
            "AN-60".to_owned()
        } else if kind == AnimationKind::Boats {
            "AN-61".to_owned()
        } else if kind == AnimationKind::Chess {
            "AN-62".to_owned()
        } else if kind == AnimationKind::Carpet {
            "AN-63".to_owned()
        } else if kind == AnimationKind::SolarSystem {
            "AN-49".to_owned()
        } else if kind == AnimationKind::HexExpedition {
            "AN-50".to_owned()
        } else if kind == AnimationKind::VectorTd {
            "AN-51".to_owned()
        } else {
            format!("AN-{:02}", index + 1)
        };
        assert_eq!(
            AnimationRow::Scene(kind).help_id(&[]),
            id,
            "scene row help id"
        );
        assert_eq!(catalog::by_id(&id).unwrap().title, kind.label());
    }
    // Every row of every scene's list resolves to an existing topic, whatever
    // controls a scene adds later.
    for kind in AnimationKind::ALL {
        for colored in [false, true] {
            let selected = AnimationSettings {
                kind,
                ..settings.clone()
            };
            let context = RowContext {
                scene_uses_cell_colors: colored,
                ..Default::default()
            };
            let controls = selected.scene_controls();
            for row in crate::animation_rows::rows(&selected, &context) {
                let id = row.help_id(&controls);
                assert!(catalog::by_id(&id).is_some(), "{kind:?} {row:?} -> {id}");
            }
        }
    }
    for (id, phrase) in [
        ("AN-36", "Location"),
        ("AN-38", "Full screen"),
        ("AN-37", "status"),
    ] {
        let topic = catalog::by_id(id).unwrap();
        let text = format!("{} {} {}", topic.title, topic.explanation, topic.specimen);
        assert!(
            text.to_lowercase().contains(&phrase.to_lowercase()),
            "{id} mentions {phrase}"
        );
    }
}

#[test]
fn narrow_animation_settings_help_anchors_reach_every_lower_control() {
    use crate::app::{App, SettingsState, SettingsTab};
    use ratatui::layout::Rect;

    let project = tempfile::tempdir().unwrap();
    let app = App::new(
        "animation-help-test".to_owned(),
        project.path().to_path_buf(),
    );
    let layout = crate::settings_ui::compute_layout(Rect::new(0, 0, 80, 24));
    let model = app.animation_row_model();
    let scene_controls = app.animation_settings.scene_controls();
    for row in 0..model.len() {
        let scrolls = crate::animation_settings_ui::follow_selection(
            layout.content_area,
            &model,
            row,
            crate::animation_settings_ui::Scrolls::default(),
        );
        let state = SettingsState {
            tab: SettingsTab::Animations,
            selected_row: row,
            scroll: scrolls.controls,
            scene_scroll: scrolls.scenes,
            global_scroll: scrolls.global,
            ..SettingsState::default()
        };
        let anchors = crate::settings_ui::settings_help_anchors(&layout, &app, &state);
        let expected_id = model.row(row).unwrap().help_id(&scene_controls);
        assert!(
            catalog::by_id(&expected_id).is_some(),
            "{expected_id} resolves"
        );
        assert!(
            anchors
                .iter()
                .any(|anchor| anchor.topic_id == expected_id && anchor.selected),
            "selected row {row}'s help anchor remains reachable when scrolled"
        );
    }
}
