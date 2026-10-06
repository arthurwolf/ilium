use super::settings::WindSettings;
use super::sim::Sim;
use crate::control::{ControlValue, SceneSettings};
use crate::scene::OccupancyMask;

#[test]
fn mouse_force_is_enabled_by_default_and_can_be_disabled() {
    let mut settings = WindSettings::default();
    let row = settings
        .controls()
        .into_iter()
        .find(|row| row.id == "mouse_force");
    assert_eq!(row.map(|row| row.value), Some(ControlValue::Bool(true)));
    assert_eq!(
        settings.set_control("mouse_force", ControlValue::Bool(false)),
        Ok(true)
    );
    let row = settings
        .controls()
        .into_iter()
        .find(|row| row.id == "mouse_force");
    assert_eq!(row.map(|row| row.value), Some(ControlValue::Bool(false)));
}

fn still_sim() -> Sim {
    let settings = WindSettings {
        wind_strength: 0,
        gusts: 0,
        gravity_enabled: false,
        ..Default::default()
    };
    let mut sim = Sim::new(&settings);
    sim.set_mask(&OccupancyMask::empty(40, 20));
    sim.advance(0.0);
    sim
}

#[test]
fn mouse_pushes_dots_outward_in_every_direction() {
    for (x, y, sign_x, sign_y) in [
        (21.0, 10.0, 1.0, 0.0),
        (19.0, 10.0, -1.0, 0.0),
        (20.0, 11.0, 0.0, 1.0),
        (20.0, 9.0, 0.0, -1.0),
    ] {
        let mut sim = still_sim();
        sim.park_all_for_test(x, y);
        sim.set_pointer(Some([0.5, 0.5]));
        sim.advance(0.1);
        assert!(sim
            .dots()
            .iter()
            .all(|dot| { (dot.x - x) * sign_x + (dot.y - y) * sign_y > 0.01 }));
    }
}

#[test]
fn distant_absent_and_invalid_pointers_do_not_move_dots() {
    for pointer in [
        None,
        Some([0.0, 0.0]),
        Some([f32::NAN, 0.5]),
        Some([0.5, f32::INFINITY]),
        Some([-0.1, 0.5]),
        Some([1.1, 0.5]),
    ] {
        let mut sim = still_sim();
        sim.park_all_for_test(20.0, 10.0);
        sim.set_pointer(Some([0.5, 0.5]));
        sim.set_pointer(pointer);
        sim.advance(0.1);
        assert!(sim.dots().iter().all(|dot| dot.x == 20.0 && dot.y == 10.0));
    }
}

#[test]
fn disabling_mouse_force_stops_further_mouse_acceleration() {
    let mut sim = still_sim();
    let settings = WindSettings {
        wind_strength: 0,
        gusts: 0,
        mouse_force: false,
        ..Default::default()
    };
    sim.set_pointer(Some([0.5, 0.5]));
    sim.reconfigure(&settings);
    sim.park_all_for_test(21.0, 10.0);
    sim.advance(0.1);
    assert!(sim.dots().iter().all(|dot| dot.x == 21.0 && dot.y == 10.0));
}

#[test]
fn dot_exactly_under_mouse_moves_without_nonfinite_values() {
    let mut sim = still_sim();
    sim.park_all_for_test(20.0, 10.0);
    sim.set_pointer(Some([0.5, 0.5]));
    sim.advance(0.1);
    assert!(sim.dots().iter().all(|dot| {
        dot.x.is_finite()
            && dot.y.is_finite()
            && dot.vx.is_finite()
            && dot.vy.is_finite()
            && (dot.x - 20.0).hypot(dot.y - 10.0) > 0.01
    }));
}

#[test]
fn mouse_toggle_survives_serialization_and_old_settings_enable_it() {
    let old: WindSettings = serde_json::from_str("{}").unwrap();
    assert!(old.mouse_force);
    let disabled = WindSettings {
        mouse_force: false,
        ..old
    };
    let saved = serde_json::to_string(&disabled).unwrap();
    assert_eq!(
        serde_json::from_str::<WindSettings>(&saved).unwrap(),
        disabled
    );
}
