use std::collections::BTreeSet;

use super::catalog;

#[test]
fn catalog_covers_every_settings_help_id_once_with_complete_content() {
    let topics = catalog::all();
    let ids = topics
        .iter()
        .map(|topic| topic.id.clone())
        .collect::<BTreeSet<_>>();
    let expected = expected_ids();

    assert_eq!(topics.len(), 205, "catalog topic count");
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
    add_range(&mut ids, "AM", 1, 21);
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
    add_range(&mut ids, "SETUP", 1, 2);
    ids.insert("DEBUG-01".to_owned());
    ids.insert("API-01".to_owned());
    add_range(&mut ids, "INF", 1, 14);
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
