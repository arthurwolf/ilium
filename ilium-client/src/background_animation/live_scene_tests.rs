//! Real project persistence and native presentation for the five live scenes.
use super::super::{AnimationKind, AnimationSettings};
use ilium_ambient::{ControlKind, ControlValue};

const LIVE_KINDS: [AnimationKind; 5] = [
    AnimationKind::Graph,
    AnimationKind::Earthquakes,
    AnimationKind::Aircraft,
    AnimationKind::Boats,
    AnimationKind::Chess,
];

#[test]
fn live_scene_controls_survive_real_project_write_and_reload() {
    let project = tempfile::tempdir().unwrap();
    for kind in LIVE_KINDS {
        let mut settings = AnimationSettings {
            kind,
            enabled: true,
            ..Default::default()
        };
        let ambient = kind.ambient().unwrap();
        for control in settings.ambient.controls(ambient) {
            let value = match (&control.kind, &control.value) {
                (ControlKind::Slider { min, max, .. }, ControlValue::Number(current)) => {
                    ControlValue::Number(if current == max { *min } else { *max })
                }
                (ControlKind::Toggle, ControlValue::Bool(current)) => ControlValue::Bool(!current),
                (ControlKind::Choice { options }, ControlValue::Index(current)) => {
                    let next = (0..options.len()).find(|index| {
                        index != current
                            && !control
                                .disabled_options
                                .iter()
                                .any(|(disabled, _)| disabled == index)
                    });
                    let Some(next) = next else { continue };
                    ControlValue::Index(next)
                }
                _ => continue,
            };
            settings
                .ambient
                .set_control(ambient, control.id, value)
                .unwrap();
        }
        let expected = settings.normalized();
        crate::project_config::set_animation(project.path(), settings).unwrap();
        let reloaded = crate::project_config::load(project.path())
            .unwrap()
            .animation;
        assert_eq!(reloaded, expected, "{}", kind.label());
        assert!(!reloaded.uses_loop_cache());
    }
}

#[test] // Real project persistence, not only a serde round trip.
fn boat_source_choices_survive_project_save_reload_without_serializing_notices() {
    // No network or scene startup.
    use ilium_ambient::live_data::maps::BoatSource; // Actual persisted enum.
    let project = tempfile::tempdir().unwrap(); // Isolated project configuration.
    for source in [BoatSource::OpenSeaFeed, BoatSource::Digitraffic] {
        // Both explicit choices.
        let mut settings = AnimationSettings {
            kind: AnimationKind::Boats,
            enabled: true,
            ..Default::default()
        }; // Existing client settings pipeline.
        settings.ambient.boats.boat_source = source; // Set the actual source field.
        crate::project_config::set_animation(project.path(), settings).unwrap(); // Authoritative writer.
        let loaded = crate::project_config::load(project.path())
            .unwrap()
            .animation; // Authoritative reader.
        assert_eq!(loaded.ambient.boats.boat_source, source); // Provider survives a real project write/reload.
        assert!(!loaded.uses_loop_cache()); // Source data is never precomputed into a fake animation loop.
        let saved = serde_json::to_value(&loaded.ambient.boats).unwrap(); // Inspect only the public typed settings.
        assert!(saved.get("boat_credit").is_none() && saved.get("boat_license").is_none());
        // Notices are derived metadata, never editable provenance.
    } // End block.
} // End block.
