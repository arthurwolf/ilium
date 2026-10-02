//! Real project persistence and native presentation for the six live scenes.
use super::super::{AnimationFrame, AnimationKind, AnimationSettings};
use ilium_ambient::{ControlKind, ControlValue};
use std::time::{Duration, Instant};

const LIVE_KINDS: [AnimationKind; 6] = [
    AnimationKind::Graph,
    AnimationKind::Pi,
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
        // Exercise conditional font and per-digit controls too.
        if kind == AnimationKind::Pi {
            settings.ambient.pi.mode = 1;
            settings.ambient.pi.digit_colors = true;
        }
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

#[test]
fn pi_native_digits_reach_client_glyphs_and_clear_on_scene_change() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::Pi,
        ..Default::default()
    };
    settings.ambient.pi.digits_limit = 32;
    settings.ambient.pi.scroll_speed = 0;
    let mut frame = AnimationFrame::default();
    let start = Instant::now();
    let text = loop {
        frame.render(&settings, 40, 8, start.elapsed());
        let text: String = (0..8)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .map(|(x, y)| frame.glyph(x, y))
            .collect();
        if text.contains("3.1415926535897932384626433832795") {
            break text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Pi readiness: {:?}; {text}",
            frame.status()
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(text.contains("3.1415926535897932384626433832795"));
    settings.kind = AnimationKind::QuietPond;
    frame.render(&settings, 40, 8, Duration::ZERO);
    let cleared: String = (0..8)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|(x, y)| frame.glyph(x, y))
        .collect();
    assert!(!cleared.contains("3.14159"));
}
