//! Carpet uses the normal hosted scene, project persistence and pointer path.
use super::super::{AmbientHost, AnimationFrame, AnimationKind, AnimationSettings};
use ilium_ambient::{AmbientKind, ControlKind, ControlValue, Frame, Scene};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn all_nine_carpet_modes_and_every_conditional_control_survive_real_project_reload() {
    let project = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut seen = std::collections::HashSet::new();
    for mode in 0..9 {
        let mut settings = AnimationSettings {
            enabled: true,
            kind: AnimationKind::Carpet,
            ..Default::default()
        };
        settings.ambient.carpet.mode = mode;
        assert_eq!(settings.kind.ambient(), Some(AmbientKind::Carpet));
        assert!(settings.kind.is_live_only());
        assert!(!settings.uses_loop_cache());
        for row in settings.scene_controls() {
            seen.insert(row.id);
            if row.id == "carpet_mode" {
                continue;
            }
            let value = match (row.kind, row.value) {
                (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) => {
                    ControlValue::Number(if value == max { min } else { max })
                }
                (ControlKind::Toggle, ControlValue::Bool(value)) => ControlValue::Bool(!value),
                _ => continue,
            };
            settings.set_scene_control(row.id, value).unwrap();
        }
        let expected = settings.normalized();
        crate::project_config::set_animation(project.path(), settings).unwrap();
        let loaded = crate::project_config::load(project.path())
            .unwrap()
            .animation;
        assert_eq!(loaded, expected, "mode{mode}");
        assert_eq!(
            crate::project_config::load(other.path()).unwrap().animation,
            AnimationSettings::default()
        );
    }
    assert!(seen.contains("carpet_infinite_lines"));
    assert!(
        seen.len() == 45,
        "all advertised Carpet controls must be covered: {}",
        seen.len()
    );
}

#[test]
fn real_nonnetwork_carpet_modes_render_braille_and_clear_when_released() {
    for mode in [0, 1, 2, 3, 5, 6, 7, 8] {
        let mut settings = AnimationSettings {
            kind: AnimationKind::Carpet,
            ..Default::default()
        };
        settings.ambient.carpet.mode = mode;
        let mut frame = AnimationFrame::default();
        frame.pointer(Some([0.7, 0.4]));
        frame.render(&settings, 80, 24, Duration::ZERO);
        frame.render(&settings, 80, 24, Duration::from_secs(1));
        let ink = (0..24)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.glyph(x, y) != ' ')
            .count();
        assert!(ink > 40, "mode{mode} yielded only {ink} ink cells");
        assert!(
            !frame.has_cell_colors(),
            "Carpet must inherit shared appearance"
        );
        frame.release_hosts();
        assert_eq!(frame.glyph(40, 12), ' ');
        assert!(!frame.host().is_hosted());
    }
}

struct PointerProbe(Arc<Mutex<Vec<Option<[f32; 2]>>>>);
impl Scene for PointerProbe {
    fn wants_pointer(&self) -> bool {
        true
    }

    fn pointer(&mut self, position: Option<[f32; 2]>) {
        self.0.lock().unwrap().push(position);
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(1.0);
    }
}

struct PointerInsensitiveProbe(Arc<Mutex<usize>>);
impl Scene for PointerInsensitiveProbe {
    fn render(&mut self, frame: &mut Frame<'_>) {
        *self.0.lock().unwrap() += 1;
        frame.raster.dots.fill(1.0);
    }
}

#[test]
fn pointer_coordinates_reach_host_once_per_render_and_invalid_values_clear() {
    let positions = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&positions);
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = AmbientHost::with_factory(Box::new(move |_, _, _| {
        Box::new(PointerProbe(Arc::clone(&shared)))
    }));
    let settings = AnimationSettings {
        kind: AnimationKind::Carpet,
        ..Default::default()
    };
    frame.pointer(Some([2.0, -1.0]));
    frame.render(&settings, 80, 24, Duration::ZERO);
    frame.render(&settings, 80, 24, Duration::ZERO);
    assert_eq!(*positions.lock().unwrap(), vec![Some([1.0, 0.0])]);
    frame.pointer(Some([f32::NAN, 0.5]));
    frame.render(&settings, 80, 24, Duration::from_millis(50));
    assert_eq!(*positions.lock().unwrap(), vec![Some([1.0, 0.0]), None]);
}

#[test]
fn pointer_change_at_same_animation_time_invalidates_cached_host_frame() {
    let positions = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&positions);
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = AmbientHost::with_factory(Box::new(move |_, _, _| {
        Box::new(PointerProbe(Arc::clone(&shared)))
    }));
    let settings = AnimationSettings {
        kind: AnimationKind::Carpet,
        ..Default::default()
    };
    let elapsed = Duration::from_secs(1);

    frame.pointer(Some([0.25, 0.5]));
    frame.render(&settings, 80, 24, elapsed);
    frame.pointer(Some([0.75, 0.5]));
    frame.render(&settings, 80, 24, elapsed);

    assert_eq!(
        *positions.lock().unwrap(),
        vec![Some([0.25, 0.5]), Some([0.75, 0.5])]
    );
}

#[test]
fn pointer_change_does_not_rerender_pointer_insensitive_scene() {
    let render_count = Arc::new(Mutex::new(0));
    let shared = Arc::clone(&render_count);
    let mut frame = AnimationFrame::default();
    *frame.host_mut() = AmbientHost::with_factory(Box::new(move |_, _, _| {
        Box::new(PointerInsensitiveProbe(Arc::clone(&shared)))
    }));
    let settings = AnimationSettings {
        kind: AnimationKind::Carpet,
        ..Default::default()
    };
    let elapsed = Duration::from_secs(1);
    frame.render(&settings, 80, 24, elapsed);
    frame.pointer(Some([0.25, 0.5]));
    frame.render(&settings, 80, 24, elapsed);
    assert_eq!(*render_count.lock().unwrap(), 1);
}

#[test]
fn production_wind_scene_declares_pointer_sensitive_rendering() {
    let settings = AnimationSettings {
        kind: AnimationKind::Wind,
        ..Default::default()
    };
    let mut frame = AnimationFrame::default();
    frame.render(&settings, 80, 24, Duration::from_secs(1));
    assert!(
        frame.host().wants_pointer(),
        "Wind's mouse force changes the rendered particle positions"
    );
}

#[test]
fn ordinary_mouse_path_supplies_screen_relative_pointer_without_consuming_foreground() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::{buffer::Buffer, layout::Rect};
    let project = tempfile::tempdir().unwrap();
    let mut app = crate::app::App::new("carpet-pointer".into(), project.path().to_path_buf());
    app.set_screen_area(Rect::new(0, 0, 80, 24));
    app.animation_settings.enabled = true;
    app.animation_settings.kind = AnimationKind::Carpet;
    let positions = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&positions);
    *app.animation_frame.host_mut() = AmbientHost::with_factory(Box::new(move |_, _, _| {
        Box::new(PointerProbe(Arc::clone(&shared)))
    }));
    crate::mouse::handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 60,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
    buffer[(60, 10)].set_symbol("X");
    crate::background_composition::compose_ready_for_test(
        &mut buffer,
        &mut app,
        Duration::from_secs(1),
    );
    assert_eq!(
        *positions.lock().unwrap(),
        vec![Some([60.5 / 80.0, 10.5 / 24.0])]
    );
    assert_eq!(buffer[(60, 10)].symbol(), "X");
    app.set_terminal_focused(false);
    assert_eq!(app.animation_pointer(Rect::new(0, 0, 80, 24)), None);
}
