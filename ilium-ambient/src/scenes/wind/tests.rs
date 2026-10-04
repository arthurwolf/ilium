use super::flow::{analyze, PushKind};
use super::settings::{EdgeMode, WindMode, WindSettings};
use super::sim::Sim;
use super::WindScene;
use crate::control::{ControlValue, SceneSettings};
use crate::raster::Raster;
use crate::scene::{Frame, OccupancyMask, Scene};
use std::time::{Duration, SystemTime};

fn mask_from(rows: &[&str]) -> OccupancyMask {
    let height = rows.len() as u16;
    let width = rows[0].chars().count() as u16;
    OccupancyMask::from_fn(width, height, |column, row| {
        rows[usize::from(row)].chars().nth(usize::from(column)) == Some('#')
    })
}

fn quiet() -> WindSettings {
    WindSettings {
        dot_count: 40,
        wind_strength: 0,
        gusts: 0,
        gravity_enabled: false,
        ..Default::default()
    }
}

#[test]
fn defaults_are_inside_their_ranges() {
    let defaults = WindSettings::default();
    assert_eq!(defaults.normalized(), defaults);
}

#[test]
fn normalization_clamps_every_number() {
    let wild = WindSettings {
        dot_count: 0,
        dot_weight: 9999,
        rotation_speed: -5000,
        merge_threshold: 0,
        push_reach: 77,
        ..Default::default()
    };
    let normalized = wild.normalized();
    assert_eq!(normalized.dot_count, 10);
    assert_eq!(normalized.dot_weight, 100);
    assert_eq!(normalized.rotation_speed, -90);
    assert_eq!(normalized.merge_threshold, 2);
    assert_eq!(normalized.push_reach, 4);
}

#[test]
fn every_control_round_trips_through_set_control() {
    let settings = WindSettings {
        wind_mode: WindMode::Rotating,
        gravity_enabled: true,
        merge_dots: true,
        ..Default::default()
    };
    for control in settings.controls() {
        let mut copy = WindSettings {
            wind_mode: WindMode::Rotating,
            gravity_enabled: true,
            merge_dots: true,
            ..Default::default()
        };
        let value = match &control.value {
            ControlValue::Number(number) => ControlValue::Number(*number),
            other => other.clone(),
        };
        assert!(
            copy.set_control(control.id, value).is_ok(),
            "{} must accept its own value",
            control.id
        );
    }
}

#[test]
fn rows_appear_only_when_their_mode_needs_them() {
    let mut settings = WindSettings::default();
    let ids = |settings: &WindSettings| -> Vec<&'static str> {
        settings
            .controls()
            .iter()
            .map(|control| control.id)
            .collect()
    };
    assert!(!ids(&settings).contains(&"rotation_speed"));
    assert!(!ids(&settings).contains(&"gravity_strength"));
    assert!(!ids(&settings).contains(&"merge_threshold"));
    settings.wind_mode = WindMode::Rotating;
    settings.gravity_enabled = true;
    settings.merge_dots = true;
    let shown = ids(&settings);
    assert!(shown.contains(&"rotation_speed"));
    assert!(shown.contains(&"gravity_strength"));
    assert!(shown.contains(&"merge_threshold"));
}

#[test]
fn unknown_control_changes_nothing() {
    let mut settings = WindSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("dot_count", ControlValue::Bool(true))
        .is_err());
}

#[test]
fn dots_start_and_stay_in_empty_cells() {
    let mask = mask_from(&[
        "..........",
        ".####.....",
        ".#..#.....",
        ".####.....",
        "..........",
    ]);
    let settings = WindSettings {
        dot_count: 200,
        wind_strength: 80,
        gravity_enabled: true,
        edge_mode: EdgeMode::Bounce,
        ..Default::default()
    };
    let mut sim = Sim::new(&settings);
    sim.set_mask(&mask);
    for tick in 1..=600 {
        sim.advance(tick as f32 / 30.0);
        for dot in sim.dots() {
            let (column, row) = (dot.x.floor() as i32, dot.y.floor() as i32);
            assert!(
                !mask.is_occupied(column, row),
                "dot at {},{} sits in occupied cell {column},{row} on tick {tick}",
                dot.x,
                dot.y
            );
        }
    }
}

#[test]
fn wind_carries_dots_in_its_direction() {
    let mask = OccupancyMask::empty(60, 12);
    let settings = WindSettings {
        dot_count: 50,
        wind_strength: 60,
        gusts: 0,
        wind_angle: 0,
        edge_mode: EdgeMode::Wrap,
        ..Default::default()
    };
    let mut sim = Sim::new(&settings);
    sim.set_mask(&mask);
    sim.advance(0.0);
    for tick in 1..=30 {
        sim.advance(tick as f32 / 30.0);
    }
    let average: f32 = sim.dots().iter().map(|dot| dot.vx).sum::<f32>() / sim.dots().len() as f32;
    assert!(
        average > 1.0,
        "dots should drift right, average vx {average}"
    );
}

#[test]
fn rotating_wind_changes_direction() {
    let mask = OccupancyMask::empty(80, 24);
    let settings = WindSettings {
        dot_count: 30,
        wind_strength: 60,
        gusts: 0,
        wind_mode: WindMode::Rotating,
        rotation_speed: 90,
        edge_mode: EdgeMode::Wrap,
        ..Default::default()
    };
    let mut sim = Sim::new(&settings);
    sim.set_mask(&mask);
    sim.advance(0.0);
    for tick in 1..=20 {
        sim.advance(tick as f32 / 20.0 * 0.5);
    }
    let early: f32 = sim.dots().iter().map(|dot| dot.vx).sum();
    for tick in 1..=60 {
        sim.advance(0.5 + tick as f32 / 20.0);
    }
    let late: f32 = sim.dots().iter().map(|dot| dot.vx).sum();
    assert!(early > 0.0, "wind starts to the right");
    assert!(
        late < early,
        "rotated wind no longer pushes right: {early} then {late}"
    );
}

#[test]
fn gravity_pulls_down_and_heavy_dots_fall_faster() {
    let mask = OccupancyMask::empty(40, 60);
    let fall = |weight: u32| {
        let settings = WindSettings {
            dot_count: 20,
            dot_weight: weight,
            weight_variation: 0,
            wind_strength: 0,
            gusts: 0,
            gravity_enabled: true,
            gravity_strength: 40,
            edge_mode: EdgeMode::Wrap,
            ..Default::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&mask);
        sim.advance(0.0);
        for tick in 1..=30 {
            sim.advance(tick as f32 / 30.0);
        }
        sim.dots().iter().map(|dot| dot.vy).sum::<f32>() / sim.dots().len() as f32
    };
    let (light, heavy) = (fall(10), fall(80));
    assert!(light > 0.5, "gravity pulls down, got {light}");
    assert!(
        heavy > light,
        "heavy {heavy} should fall faster than light {light}"
    );
}

#[test]
fn scrolled_rows_are_recognised_as_scroll() {
    let before = mask_from(&[
        "..........",
        ".##.###.#.",
        ".#.##.##..",
        ".####..##.",
        "..#.#.#.#.",
        "..........",
    ]);
    // Everything moved up by one row.
    let after = mask_from(&[
        ".##.###.#.",
        ".#.##.##..",
        ".####..##.",
        "..#.#.#.#.",
        "..........",
        "..........",
    ]);
    let pushes = analyze(&before, &after, 3);
    assert!(!pushes.is_empty());
    assert!(
        pushes.iter().all(|push| matches!(
            push.kind,
            PushKind::Scroll {
                dx: 0,
                dy: -1,
                distance: 1
            }
        )),
        "{pushes:?}"
    );
}

#[test]
fn typing_is_not_scrolling() {
    let before = mask_from(&["..........", ".###......", ".........."]);
    let after = mask_from(&["..........", ".####.....", ".........."]);
    let pushes = analyze(&before, &after, 3);
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].kind, PushKind::Appear);
}

#[test]
fn a_new_line_under_an_old_one_is_not_a_scroll() {
    let before = mask_from(&[".####.....", "..........", ".........."]);
    let after = mask_from(&[".####.....", ".####.....", ".........."]);
    let pushes = analyze(&before, &after, 3);
    assert!(
        pushes.iter().all(|push| push.kind == PushKind::Appear),
        "{pushes:?}"
    );
}

#[test]
fn scrolling_text_pushes_a_dot_in_its_direction() {
    let before = mask_from(&[
        "..........",
        "..........",
        ".#.##.##..",
        ".####..##.",
        ".#.#.#.#..",
        "..........",
    ]);
    // Rows 2..=4 move up one: row 1 gains cells above where the dot rests.
    let after = mask_from(&[
        "..........",
        ".#.##.##..",
        ".####..##.",
        ".#.#.#.#..",
        "..........",
        "..........",
    ]);
    let settings = WindSettings {
        dot_count: 10,
        scroll_push: 100,
        push_reach: 1,
        wind_strength: 0,
        gusts: 0,
        weight_variation: 0,
        ..Default::default()
    };
    let mut sim = Sim::new(&settings);
    sim.set_mask(&before);
    // Park every dot in the empty top row where text will arrive.
    sim.park_all_for_test(1.5, 1.5);
    sim.set_mask(&after);
    assert!(
        sim.dots().iter().all(|dot| dot.vy < -1.0),
        "dots should be carried up: {:?}",
        sim.dots().iter().map(|dot| dot.vy).collect::<Vec<_>>()
    );
    for dot in sim.dots() {
        assert!(!after.is_occupied(dot.x.floor() as i32, dot.y.floor() as i32));
    }
}

#[test]
fn text_from_nowhere_pushes_gentler_than_scrolling() {
    let before = OccupancyMask::empty(9, 5);
    let mut after = OccupancyMask::empty(9, 5);
    after.set(4, 2, true);
    let run = |scroll: u32, appear: u32| {
        let settings = WindSettings {
            dot_count: 10,
            scroll_push: scroll,
            appear_push: appear,
            weight_variation: 0,
            wind_strength: 0,
            gusts: 0,
            ..Default::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&before);
        sim.park_all_for_test(4.5, 2.5);
        sim.set_mask(&after);
        sim.dots()
            .iter()
            .map(|dot| (dot.vx * dot.vx + dot.vy * dot.vy).sqrt())
            .fold(0.0_f32, f32::max)
    };
    assert!(run(100, 100) > 1.0, "appear push must move a dot");
    assert!(run(100, 20) < run(100, 100));
    assert!(run(100, 0) < 0.01, "appear push 0 means no push");
}

#[test]
fn dots_only_move_on_when_their_cell_fills() {
    let before = OccupancyMask::empty(9, 5);
    let mut after = OccupancyMask::empty(9, 5);
    after.set(0, 0, true);
    let settings = WindSettings {
        dot_count: 10,
        push_reach: 1,
        ..quiet()
    };
    let mut sim = Sim::new(&settings);
    sim.set_mask(&before);
    sim.park_all_for_test(7.5, 3.5);
    sim.set_mask(&after);
    assert!(sim.dots().iter().all(|dot| dot.vx == 0.0 && dot.vy == 0.0));
}

#[test]
fn resizing_the_screen_rehomes_dots() {
    let mut sim = Sim::new(&quiet());
    sim.set_mask(&OccupancyMask::empty(80, 24));
    sim.set_mask(&OccupancyMask::empty(20, 6));
    for dot in sim.dots() {
        assert!(dot.x >= 0.0 && dot.x < 20.0 && dot.y >= 0.0 && dot.y < 6.0);
    }
}

fn render(scene: &mut WindScene, width: u16, height: u16, seconds: f32) -> Raster {
    let mut raster = Raster::default();
    raster.resize(usize::from(width) * 2, usize::from(height) * 4);
    let mut colors = Vec::new();
    scene.render(&mut Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width,
        height,
        time: Duration::from_secs_f32(seconds),
        wall: Duration::from_secs_f32(seconds),
        now: SystemTime::UNIX_EPOCH,
    });
    raster
}

#[test]
fn scene_draws_dots_and_asks_for_the_screen_mask() {
    let env = crate::scene::SceneEnv::for_test(
        std::env::temp_dir().join("ilium-wind-test"),
        crate::resources::test_resources(),
    );
    let mut scene = WindScene::new(&quiet(), &env);
    assert!(scene.wants_occupancy());
    let raster = render(&mut scene, 40, 12, 0.0);
    let lit = raster.dots.iter().filter(|dot| **dot > 0.0).count();
    assert!(lit > 0 && lit <= 40, "lit dots: {lit}");
}

#[test]
fn merged_dots_become_a_larger_glyph() {
    let env = crate::scene::SceneEnv::for_test(
        std::env::temp_dir().join("ilium-wind-test"),
        crate::resources::test_resources(),
    );
    let settings = WindSettings {
        dot_count: 30,
        merge_dots: true,
        merge_threshold: 3,
        ..quiet()
    };
    let mut scene = WindScene::new(&settings, &env);
    // A one-cell screen puts every dot in the same cell.
    let raster = render(&mut scene, 1, 1, 0.0);
    assert!(scene.native_glyph(0, 0).is_some());
    assert_eq!(raster.dots.iter().filter(|dot| **dot > 0.0).count(), 0);
    assert_eq!(scene.native_glyph(5, 5), None);
    let mut unmerged = WindScene::new(&quiet(), &env);
    render(&mut unmerged, 1, 1, 0.0);
    assert_eq!(unmerged.native_glyph(0, 0), None);
}
