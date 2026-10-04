use super::*;
use ilium_ambient::AmbientKind;

#[test]
fn galactic_empires_uses_live_host_and_project_settings() {
    let kind = AnimationKind::GalacticEmpires;
    assert!(AnimationKind::ALL.contains(&kind));
    assert_eq!(kind.ambient(), Some(AmbientKind::GalacticEmpires));
    assert!(kind.is_live_only());
    let mut settings = AnimationSettings {
        enabled: true,
        kind,
        ..Default::default()
    };
    let controls = settings.scene_controls();
    assert_eq!(controls.len(), 27);
    assert_eq!(settings.ambient.galactic_empires.star_count, 480);
    let key = settings.ambient.scene_key(AmbientKind::GalacticEmpires);
    for control in controls {
        if let Some(value) = control.stepped(1) {
            settings
                .ambient
                .set_control(AmbientKind::GalacticEmpires, control.id, value)
                .unwrap();
        }
    }
    assert_ne!(
        key,
        settings.ambient.scene_key(AmbientKind::GalacticEmpires)
    );
    let project = tempfile::tempdir().unwrap();
    crate::project_config::set_animation(project.path(), settings.clone()).unwrap();
    let loaded = crate::project_config::load(project.path())
        .unwrap()
        .animation;
    assert_eq!(settings, loaded);
    let mut frame = AnimationFrame::default();
    frame.render(&loaded, 80, 24, Duration::ZERO);
    assert!(frame.has_cell_colors());
}
