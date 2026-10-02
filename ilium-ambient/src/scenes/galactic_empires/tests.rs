use super::*;
use crate::debug::render_frame;

fn scene() -> GalacticEmpiresScene {
    GalacticEmpiresScene::new(
        &GalacticEmpiresSettings {
            seed: 42,
            ..Default::default()
        },
        &SceneEnv::for_test(std::env::temp_dir().join("galaxy-unused-cache")),
    )
}

#[test]
fn status_excludes_wars_of_eliminated_empires() {
    let mut scene = scene();
    for star in &mut scene.galaxy.stars {
        star.owner = Some(0);
    }
    scene.galaxy.stars[0].owner = Some(1);
    scene.galaxy.relations[0][1].war = true;
    scene.galaxy.relations[0][2].war = true;
    scene.galaxy.relations[2][3].war = true;
    assert!(scene.status().unwrap().starts_with("2 empires · 1 wars"));
}

#[test]
fn simulation_and_camera_speeds_are_independent() {
    let mut slow = scene();
    let mut fast = scene();
    let mut orbiting = scene();
    slow.settings.simulation_speed = 25;
    slow.settings.camera_speed = 0;
    fast.settings.simulation_speed = 400;
    fast.settings.camera_speed = 0;
    orbiting.settings.simulation_speed = 25;
    orbiting.settings.camera_speed = 300;
    for seconds in 0..=10 {
        let time = Duration::from_secs(seconds);
        render_frame(&mut slow, 1, 1, time);
        render_frame(&mut fast, 1, 1, time);
        render_frame(&mut orbiting, 1, 1, time);
    }
    assert_eq!(slow.galaxy.tick, 5);
    assert_eq!(fast.galaxy.tick, 80);
    assert_eq!(slow.galaxy.stars, orbiting.galaxy.stars);
    assert_eq!(slow.galaxy.fleets, orbiting.galaxy.fleets);
    assert_eq!(slow.camera_time, fast.camera_time);
    let held = Camera::new(2.0, slow.camera_time, slow.settings.camera_speed);
    let moving = Camera::new(2.0, orbiting.camera_time, orbiting.settings.camera_speed);
    assert_eq!(held.center, Camera::new(2.0, 0.0, 0).center);
    assert_ne!(held.center, moving.center);
}

#[test]
fn simulation_is_cadence_independent_and_survives_live_global_speed_decreases() {
    let mut fine = scene();
    let mut coarse = scene();
    for tick in 0..=240 {
        render_frame(&mut fine, 1, 1, Duration::from_millis(tick * 250));
    }
    for tick in 0..=60 {
        render_frame(&mut coarse, 1, 1, Duration::from_secs(tick));
    }
    assert_eq!(fine.galaxy.stars, coarse.galaxy.stars);
    assert_eq!(fine.galaxy.stats, coarse.galaxy.stats);
    let previous_tick = coarse.galaxy.tick;
    let mut raster = crate::Raster::default();
    raster.resize(2, 4);
    let mut colors = vec![[0, 0, 0]];
    coarse.render(&mut Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width: 1,
        height: 1,
        time: Duration::from_secs_f64(30.5),
        wall: Duration::from_secs(61),
        now: std::time::SystemTime::UNIX_EPOCH,
    });
    assert_eq!(coarse.galaxy.tick, previous_tick + 1);
    assert_eq!(coarse.camera_time, 60.5);
}

#[test]
fn suspend_is_bounded_and_victory_is_visible_before_a_new_seeded_galaxy() {
    let mut scene = scene();
    render_frame(&mut scene, 1, 1, Duration::ZERO);
    render_frame(&mut scene, 1, 1, Duration::from_secs(100_000));
    assert!(scene.galaxy.tick <= 80);
    let first_positions: Vec<_> = scene
        .galaxy
        .stars
        .iter()
        .map(|star| star.position)
        .collect();
    while scene.galaxy.winner.is_none() {
        scene.galaxy.step();
    }
    assert!(scene.status().unwrap().contains("united"));
    let previous_cycle = scene.cycle;
    for seconds in 100_001..100_040 {
        render_frame(&mut scene, 1, 1, Duration::from_secs(seconds));
    }
    assert_eq!(scene.cycle, previous_cycle + 1);
    assert_ne!(
        first_positions,
        scene
            .galaxy
            .stars
            .iter()
            .map(|star| star.position)
            .collect::<Vec<_>>()
    );
}

#[test]
fn actual_render_has_stars_lanes_territories_colors_and_moving_fleets() {
    let mut scene = scene();
    for seconds in 0..30 {
        render_frame(&mut scene, 1, 1, Duration::from_secs(seconds));
    }
    let frame = render_frame(&mut scene, 100, 40, Duration::from_secs(30));
    assert!(frame.lit_dots() > 20);
    assert!(frame
        .raster
        .dots
        .iter()
        .any(|value| *value > 0.0 && *value < 0.5));
    assert!(frame.cell_colors.iter().any(|color| color[0] != color[1]));
    assert!(!scene.galaxy.fleets.is_empty());
    let before = scene.galaxy.fleets.clone();
    render_frame(&mut scene, 100, 40, Duration::from_secs(31));
    assert_ne!(before, scene.galaxy.fleets);
    for (width, height) in [(0, 0), (1, 1), (240, 1), (1, 80)] {
        let frame = render_frame(&mut scene, width, height, Duration::from_secs(32));
        assert!(frame
            .raster
            .dots
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
        assert_eq!(
            frame.cell_colors.len(),
            usize::from(width) * usize::from(height)
        );
    }
}

#[test]
fn rounded_defaults_preserve_explicit_saved_density_and_bounds() {
    let defaults: GalacticEmpiresSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.star_count, 320);
    let saved: GalacticEmpiresSettings = serde_json::from_str(r#"{"star_count":240}"#).unwrap();
    assert_eq!(saved.normalized().star_count, 240);
    assert_eq!(
        serde_json::from_str::<GalacticEmpiresSettings>(&serde_json::to_string(&saved).unwrap())
            .unwrap(),
        saved
    );
    for (input, expected) in [(i32::MIN, 120), (i32::MAX, 420)] {
        assert_eq!(
            GalacticEmpiresSettings {
                star_count: input,
                ..defaults.clone()
            }
            .normalized()
            .star_count,
            expected
        );
    }
}

#[test]
fn marker_cells_color_the_full_support_and_star_priority_is_order_independent() {
    let mut raster = crate::Raster::default();
    raster.resize(8, 8);
    let mut colors = vec![[0, 0, 0]; 8];
    let mut frame = Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width: 4,
        height: 2,
        time: Duration::ZERO,
        wall: Duration::ZERO,
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    let star = Marker {
        from: (0.5, 3.5 / 8.0),
        to: (0.5, 3.5 / 8.0),
        radius: 1.2,
        intensity: 1.0,
        color: [110, 190, 230],
        rank: 3,
    };
    let fleet = Marker {
        color: [230, 130, 150],
        rank: 2,
        ..star
    };
    for order in [[star, fleet], [fleet, star]] {
        frame.raster.dots.fill(0.8);
        frame.cell_colors.fill([0, 0, 0]);
        let mut cells = vec![None; 8];
        let mut primitives = Vec::new();
        for marker in order {
            GalacticEmpiresScene::queue_marker(&frame, &mut cells, &mut primitives, marker);
        }
        GalacticEmpiresScene::paint_markers(&mut frame, &cells, &primitives);
        for index in 0..8 {
            let expected = if [1, 2, 5, 6].contains(&index) {
                star.color
            } else {
                [0, 0, 0]
            };
            assert_eq!(frame.cell_colors[index], expected);
        }
        assert!(frame.raster.dots.iter().any(|&dot| dot > 0.9));
        assert_eq!(frame.raster.dots[2], 0.0); // No recolored background at cell corner.
                                               // Same-rank exact overlap keeps the first marker, never blends owners.
        GalacticEmpiresScene::queue_marker(
            &frame,
            &mut cells,
            &mut primitives,
            Marker { rank: 3, ..fleet },
        );
        GalacticEmpiresScene::paint_markers(&mut frame, &cells, &primitives);
        assert_eq!(frame.cell_colors[1], star.color);
        // Two same-color stars in one cell must both remain, even when their
        // supports do not overlap. A single winning primitive is insufficient.
        GalacticEmpiresScene::queue_marker(
            &frame,
            &mut cells,
            &mut primitives,
            Marker {
                from: (2.5 / 8.0, 0.5 / 8.0),
                to: (2.5 / 8.0, 0.5 / 8.0),
                ..star
            },
        );
        GalacticEmpiresScene::paint_markers(&mut frame, &cells, &primitives);
        assert!(frame.raster.dots[2] > 0.9);
        assert!(frame.raster.dots[3 * 8 + 3] > 0.9);
    }
}

#[test]
fn render_refreshes_captures_without_a_log_and_flash_keeps_the_current_owner() {
    let mut scene = scene();
    scene.settings.camera_speed = 0;
    scene.settings.show_fleets = false;
    let camera = Camera::new(1.5, 0.0, 0);
    scene.galaxy.stars.truncate(1);
    scene.galaxy.stars[0].position = camera.center;
    scene.galaxy.stars[0].owner = Some(0);
    scene.galaxy.lanes.clear();
    scene.galaxy.fleets.clear();
    render_frame(&mut scene, 120, 40, Duration::ZERO);
    // Renderer-only fixture: no simulation tick or fabricated launch is run.
    scene.galaxy.stars[0].owner = Some(1);
    scene.galaxy.last_captures = vec![(0, 0, 1)];
    let frame = render_frame(&mut scene, 120, 40, Duration::ZERO);
    let light = scene.galaxy.empires[1]
        .color
        .map(|c| (u16::from(c) + 255).div_ceil(2) as u8);
    let mut marked = 0;
    for (index, entry) in scene.marker_cells.iter().enumerate() {
        if let Some((marker, _)) = entry {
            assert_eq!(marker.radius, 1.5);
            assert_eq!(frame.cell_colors[index], light);
            marked += 1;
        }
    }
    assert!(marked > 0);
    scene.galaxy.last_captures.clear();
    scene.galaxy.stars[0].owner = Some(2);
    render_frame(&mut scene, 120, 40, Duration::ZERO);
    assert_eq!(scene.territory.sample(camera.center).unwrap().owner, 2);
    assert_eq!(scene.galaxy.tick, 0);
    assert_eq!(scene.galaxy.stars[0].owner, Some(2));
}

#[test]
fn zero_shading_disables_fill_and_contours_but_not_markers() {
    let mut scene = scene();
    let mut raster = crate::Raster::default();
    raster.resize(120, 80);
    let mut colors = vec![[96, 119, 148]; 60 * 20];
    let mut frame = Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width: 60,
        height: 20,
        time: Duration::ZERO,
        wall: Duration::ZERO,
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    let camera = Camera::new(1.5, 0.0, 0);
    scene.galaxy.stars.truncate(1);
    scene.galaxy.stars[0].position = camera.center;
    scene.galaxy.stars[0].owner = Some(0);
    scene.territory.sync(&scene.galaxy.stars);
    scene.settings.territory_strength = 0;
    scene.draw_field(&mut frame, &camera);
    assert!(frame.raster.dots.iter().all(|&v| v == 0.0));
    scene.settings.territory_strength = 100;
    scene.draw_field(&mut frame, &camera);
    assert!(frame.raster.dots.iter().any(|&v| v > 0.0));
    assert!(frame
        .raster
        .dots
        .iter()
        .all(|&v| v.is_finite() && (0.0..1.0).contains(&v)));
    scene.settings.territory_strength = 0;
    scene.galaxy.lanes.clear();
    scene.galaxy.fleets.clear();
    let rendered = render_frame(&mut scene, 60, 20, Duration::ZERO);
    assert!(rendered.lit_dots() > 0);
}

#[test]
fn a_fleet_in_a_star_cell_does_not_recolor_the_owned_system() {
    let mut scene = scene();
    for _ in 0..60 {
        scene.galaxy.step();
    }
    let mut fleet = scene.galaxy.fleets[0].clone();
    let camera = Camera::new(1.25, 0.0, 0);
    for star in &mut scene.galaxy.stars {
        star.position = (-0.8, -0.4);
    }
    scene.galaxy.stars[0].position = camera.center;
    scene.galaxy.stars[0].owner = Some(0);
    fleet.owner = 1;
    fleet.from = 0;
    fleet.to = 1;
    fleet.progress = 0.0;
    scene.galaxy.fleets = vec![fleet];
    scene.galaxy.last_captures.clear();
    scene.territory = Territory::new(&scene.galaxy.stars);
    scene.settings.camera_speed = 0;
    let frame = render_frame(&mut scene, 100, 40, Duration::ZERO);
    let system_color = scene.galaxy.empires[0]
        .color
        .map(|channel| (u16::from(channel) + 255).div_ceil(2) as u8);
    assert_eq!(frame.cell_colors[20 * 100 + 50], system_color);
}
