use ilium_ambient::{ControlValue, SceneSettings, VoxelLandscapeSettings};

#[test]
fn invalid_loaded_settings_are_bounded_without_changing_seed() {
    let settings: VoxelLandscapeSettings = serde_json::from_str(r#"{"seed":4294967295,"zoom_percent":-20,"detail":900,"pan_speed_percent":900,"hue_degrees":900,"saturation_percent":-8,"lightness_percent":900}"#).unwrap();
    let normalized = settings.normalized();
    assert_eq!(normalized.seed, u32::MAX);
    assert_eq!(normalized.zoom_percent, 25);
    assert_eq!(normalized.detail, 3);
    assert_eq!(normalized.pan_speed_percent, 200);
    assert_eq!(normalized.hue_degrees, 360);
    assert_eq!(normalized.saturation_percent, 0);
    assert_eq!(normalized.lightness_percent, 100);
}

#[test]
fn all_editable_controls_round_trip_and_reject_wrong_types() {
    let mut settings = VoxelLandscapeSettings::default();
    for row in settings.controls() {
        if let Some(value) = row.stepped(1) {
            // A world-source edit changes which later controls are visible.
            // Test each original control against its own unchanged baseline.
            let mut edited = VoxelLandscapeSettings::default();
            edited.set_control(row.id, value.clone()).unwrap();
            let loaded: VoxelLandscapeSettings =
                serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
            assert_eq!(loaded, edited, "{}", row.id);
            let loaded_row = loaded
                .controls()
                .into_iter()
                .find(|candidate| candidate.id == row.id)
                .unwrap();
            assert_eq!(loaded_row.value, value, "{}", row.id);
        }
    }
    assert!(settings
        .set_control("zoom", ControlValue::Bool(true))
        .is_err());
    assert!(!settings
        .set_control("unknown", ControlValue::Number(1))
        .unwrap());
    assert!(settings
        .set_control("seed", ControlValue::Text("not a number".into()))
        .is_err());
    settings
        .set_control("seed", ControlValue::Text("4294967295".into()))
        .unwrap();
    assert_eq!(settings.seed, u32::MAX);
}
