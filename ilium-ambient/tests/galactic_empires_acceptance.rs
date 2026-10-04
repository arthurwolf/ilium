//! Public registry/host acceptance for the procedural galaxy background.
mod support;

use ilium_ambient::{AmbientKind, AmbientSettings, ControlValue, Frame, Raster, SceneEnv};
use std::time::{Duration, SystemTime};

#[test]
fn galactic_empires_is_selectable_and_settings_roundtrip() {
    let kind = AmbientKind::GalacticEmpires;
    assert!(AmbientKind::ALL.contains(&kind));
    let mut settings = AmbientSettings::default();
    let original_key = settings.scene_key(kind);
    let controls = settings.controls(kind);
    assert!(controls
        .iter()
        .any(|control| control.label.contains("speed")));
    for control in controls {
        if let Some(value) = control.stepped(1) {
            settings.set_control(kind, control.id, value).unwrap();
        }
    }
    assert_ne!(settings.scene_key(kind), original_key);
    assert!(!settings
        .set_control(kind, "unknown", ControlValue::Bool(true))
        .unwrap());
    let saved = serde_json::to_vec(&settings).unwrap();
    let loaded: AmbientSettings = serde_json::from_slice(&saved).unwrap();
    assert_eq!(settings, loaded);
    assert_eq!(settings.scene_key(kind), loaded.scene_key(kind));
}

#[test]
fn galactic_empires_renders_colored_finite_frames_at_extreme_sizes() {
    let resources_fixture = support::ResourcesFixture::new().unwrap();
    let env = SceneEnv::for_test(
        std::env::temp_dir().join("ilium-galaxy-no-io"),
        resources_fixture.resources.clone(),
    );
    let settings = AmbientSettings::default();
    for (width, height) in [(0, 0), (1, 1), (40, 20), (120, 40), (240, 80)] {
        let mut scene = settings.create_scene(AmbientKind::GalacticEmpires, &env);
        let mut raster = Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        let mut colors = vec![[0, 0, 0]; usize::from(width) * usize::from(height)];
        for seconds in [0, 1, 30, 600, 3600] {
            raster.dots.fill(0.0);
            scene.render(&mut Frame {
                raster: &mut raster,
                cell_colors: &mut colors,
                width,
                height,
                time: Duration::from_secs(seconds),
                wall: Duration::from_secs(seconds),
                now: SystemTime::UNIX_EPOCH,
            });
            assert!(raster
                .dots
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert_eq!(colors.len(), usize::from(width) * usize::from(height));
            if width >= 40 {
                assert!(raster.dots.iter().any(|value| *value > 0.5));
                assert!(colors
                    .iter()
                    .any(|color| color[0] != color[1] || color[1] != color[2]));
            }
        }
        assert!(scene.uses_cell_colors());
    }
}

#[test]
fn expanded_controls_can_turn_every_visual_layer_off() {
    let kind = AmbientKind::GalacticEmpires;
    let mut settings = AmbientSettings::default();
    for (id, value) in [
        ("territory_strength", ControlValue::Number(0)),
        ("star_brightness", ControlValue::Number(0)),
        ("show_lanes", ControlValue::Bool(false)),
        ("show_fleets", ControlValue::Bool(false)),
    ] {
        assert!(
            settings.set_control(kind, id, value).unwrap(),
            "missing or inert {id}"
        );
    }
    let resources_fixture = support::ResourcesFixture::new().unwrap();
    let env = SceneEnv::for_test(
        std::env::temp_dir().join("galaxy-dark-no-io"),
        resources_fixture.resources.clone(),
    );
    let mut scene = settings.create_scene(kind, &env);
    let mut raster = Raster::default();
    raster.resize(160, 96);
    let mut colors = vec![[0, 0, 0]; 80 * 24];
    for seconds in [0, 30, 120] {
        raster.dots.fill(0.0);
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 80,
            height: 24,
            time: Duration::from_secs(seconds),
            wall: Duration::from_secs(seconds),
            now: SystemTime::UNIX_EPOCH,
        });
        assert!(
            raster.dots.iter().all(|value| *value == 0.0),
            "a disabled visual layer still emits dots"
        );
    }
}
