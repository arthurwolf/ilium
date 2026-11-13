//! Twins of the unit tests of `ilium-client/src/padding.rs` against the plugin's pure model.

use ilium_plugin_core_layout::padding::{
    Area, PaddingMode, PaddingPanel, PaddingSettings, PaddingSides,
};

/// REQ: PORT:ilium-client/src/padding.rs::default_is_disabled_with_a_200_column_threshold
#[test]
fn default_is_disabled_with_a_200_column_threshold() {
    let settings = PaddingSettings::default();
    assert_eq!(settings.mode, PaddingMode::Disabled);
    assert_eq!(settings.width_threshold, 200);
    assert!(!settings.is_active(10_000));
    assert_eq!(
        settings.sides_for(PaddingPanel::Tree, 10_000),
        PaddingSides::NONE
    );
    assert_eq!(settings.validate(), Ok(()));
}

/// REQ: PORT:ilium-client/src/padding.rs::enabled_always_pads_and_width_dependent_switches_at_the_threshold
#[test]
fn enabled_always_pads_and_width_dependent_switches_at_the_threshold() {
    let mut settings = PaddingSettings {
        mode: PaddingMode::Enabled,
        ..PaddingSettings::default()
    };
    assert!(settings.is_active(1));
    assert_eq!(
        settings.sides_for(PaddingPanel::Terminal, 1),
        PaddingSides::uniform(1)
    );
    settings.mode = PaddingMode::WidthDependent;
    assert!(!settings.is_active(199));
    assert!(settings.is_active(200));
    assert!(settings.is_active(201));
    settings.width_threshold = 120;
    assert!(settings.is_active(120));
    assert!(!settings.is_active(119));
}

/// REQ: PORT:ilium-client/src/padding.rs::linked_panels_use_one_value_and_unlinked_ones_keep_each_side
#[test]
fn linked_panels_use_one_value_and_unlinked_ones_keep_each_side() {
    let mut settings = PaddingSettings {
        mode: PaddingMode::Enabled,
        ..PaddingSettings::default()
    };
    settings.tree.all = 3;
    assert_eq!(
        settings.sides_for(PaddingPanel::Tree, 80),
        PaddingSides::uniform(3)
    );
    settings.tree.linked = false;
    settings.tree.sides = PaddingSides {
        left: 0,
        right: 1,
        top: 2,
        bottom: 3,
    };
    assert_eq!(
        settings.sides_for(PaddingPanel::Tree, 80),
        PaddingSides {
            left: 0,
            right: 1,
            top: 2,
            bottom: 3
        }
    );
}

/// REQ: PORT:ilium-client/src/padding.rs::shared_right_panels_ignore_per_kind_values_until_unshared
#[test]
fn shared_right_panels_ignore_per_kind_values_until_unshared() {
    let mut settings = PaddingSettings {
        mode: PaddingMode::Enabled,
        ..PaddingSettings::default()
    };
    settings.right_shared.all = 2;
    settings.editor.all = 3;
    assert_eq!(
        settings.sides_for(PaddingPanel::Editor, 80),
        PaddingSides::uniform(2)
    );
    settings.right_panels_shared = false;
    assert_eq!(
        settings.sides_for(PaddingPanel::Editor, 80),
        PaddingSides::uniform(3)
    );
    assert_eq!(
        settings.sides_for(PaddingPanel::Terminal, 80),
        PaddingSides::uniform(1)
    );
}

/// REQ: PORT:ilium-client/src/padding.rs::inset_shrinks_by_each_side_and_never_removes_the_last_cell
#[test]
fn inset_shrinks_by_each_side_and_never_removes_the_last_cell() {
    let area = Area::new(2, 3, 20, 10);
    let sides = PaddingSides {
        left: 1,
        right: 2,
        top: 3,
        bottom: 0,
    };
    assert_eq!(sides.inset(area), Area::new(3, 6, 17, 7));
    assert_eq!(PaddingSides::NONE.inset(area), area);
    let narrow = Area::new(0, 0, 3, 3);
    assert_eq!(PaddingSides::uniform(2).inset(narrow), narrow);
    assert_eq!(
        PaddingSides::uniform(1).inset(Area::new(0, 0, 3, 3)),
        Area::new(1, 1, 1, 1)
    );
}

/// REQ: PORT:ilium-client/src/padding.rs::validation_rejects_small_thresholds_and_large_sides
#[test]
fn validation_rejects_small_thresholds_and_large_sides() {
    let mut settings = PaddingSettings {
        width_threshold: 10,
        ..PaddingSettings::default()
    };
    assert!(settings.validate().is_err());
    settings.width_threshold = 11;
    assert_eq!(settings.validate(), Ok(()));
    settings.editor.sides.top = 4;
    assert!(settings.validate().is_err());
}

/// REQ: PORT:ilium-client/src/padding.rs::settings_round_trip_through_toml_and_partial_tables_use_defaults
#[test]
fn settings_round_trip_and_partial_tables_use_defaults() {
    let mut settings = PaddingSettings {
        mode: PaddingMode::WidthDependent,
        width_threshold: 150,
        ..PaddingSettings::default()
    };
    settings.tree.linked = false;
    settings.tree.sides.left = 3;
    let text = serde_json::to_string(&settings).unwrap();
    let parsed: PaddingSettings = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, settings);
    let partial: PaddingSettings = serde_json::from_str(r#"{"mode":"enabled"}"#).unwrap();
    assert_eq!(partial.mode, PaddingMode::Enabled);
    assert_eq!(partial.width_threshold, 200);
}
