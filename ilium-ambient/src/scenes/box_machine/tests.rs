use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use std::time::Duration;

fn scene_with(settings: &BoxMachineSettings) -> BoxMachineScene {
    BoxMachineScene::new(
        settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    )
}

fn frame_at(scene: &mut BoxMachineScene, cells: (u16, u16), time: f64) -> Rendered {
    render_frame(scene, cells.0, cells.1, Duration::from_secs_f64(time))
}

#[test]
#[ignore = "prints the picture for eyeballing"]
fn print_picture() {
    let mut scene = scene_with(&BoxMachineSettings::default());
    for time in [0.0] {
        let rendered = frame_at(&mut scene, (80, 40), time);
        for line in rendered.braille_lines(100, DitherMode::Ordered) {
            println!("{line}");
        }
    }
}

fn distinct_values(rendered: &Rendered) -> usize {
    let mut values: Vec<u32> = rendered.raster.dots.iter().map(|v| v.to_bits()).collect();
    values.sort_unstable();
    values.dedup();
    values.len()
}

#[test]
fn same_time_gives_identical_raster() {
    let settings = BoxMachineSettings::default();
    let first = frame_at(&mut scene_with(&settings), (60, 30), 3.7);
    let second = frame_at(&mut scene_with(&settings), (60, 30), 3.7);
    assert_eq!(first.raster.dots, second.raster.dots);
    let mut same_scene = scene_with(&settings);
    let early = frame_at(&mut same_scene, (60, 30), 3.7);
    frame_at(&mut same_scene, (60, 30), 90.0);
    let again = frame_at(&mut same_scene, (60, 30), 3.7);
    assert_eq!(early.raster.dots, again.raster.dots);
}

#[test]
fn differing_times_differ() {
    let mut scene = scene_with(&BoxMachineSettings::default());
    let first = frame_at(&mut scene, (60, 30), 1.0);
    let second = frame_at(&mut scene, (60, 30), 6.0);
    assert_ne!(first.raster.dots, second.raster.dots);
}

#[test]
fn different_seeds_build_different_machines() {
    let render = |seed| {
        let settings = BoxMachineSettings {
            seed,
            ..BoxMachineSettings::default()
        };
        frame_at(&mut scene_with(&settings), (60, 30), 0.0)
            .raster
            .dots
    };
    assert_ne!(render(1), render(2));
}

#[test]
fn normalized_clamps_out_of_range_values() {
    let defaults = BoxMachineSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let wild = BoxMachineSettings {
        seed: 1_000_000,
        tile_size: 999,
        density: 0,
        screens: 99,
        speed: 0,
        spacing: 1,
        cube_size: 99,
        rail_level: 500,
        cube_level: 0,
        ..BoxMachineSettings::default()
    };
    let clean = wild.normalized();
    assert_eq!(
        (
            clean.seed,
            clean.tile_size,
            clean.density,
            clean.screens,
            clean.speed,
            clean.spacing,
            clean.cube_size,
            clean.rail_level,
            clean.cube_level
        ),
        (9999, 28, 5, 6, 1, 16, 6, 60, 10)
    );
    let odd = BoxMachineSettings {
        tile_size: 15,
        ..BoxMachineSettings::default()
    };
    assert_eq!(odd.normalized().tile_size % 4, 0);
}

#[test]
fn missing_keys_default_and_round_trip() {
    let parsed: BoxMachineSettings = serde_json::from_str("{\"seed\": 3}").unwrap();
    assert_eq!(parsed.seed, 3);
    assert_eq!(parsed.tile_size, 20);
    let json = serde_json::to_string(&BoxMachineSettings::default()).unwrap();
    assert_eq!(
        serde_json::from_str::<BoxMachineSettings>(&json).unwrap(),
        BoxMachineSettings::default()
    );
}

#[test]
fn controls_have_sane_shape() {
    let controls = BoxMachineSettings::default().controls();
    assert!((6..=12).contains(&controls.len()));
    for control in &controls {
        assert!(!control.help.is_empty(), "{} has no help", control.id);
        if let (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) =
            (&control.kind, &control.value)
        {
            assert!(
                (*min..=*max).contains(value),
                "{} default out of range",
                control.id
            );
        }
    }
}

#[test]
fn every_control_round_trips_through_set_control() {
    let base = BoxMachineSettings::default();
    for control in base.controls() {
        let mut settings = base.clone();
        assert_eq!(
            settings.set_control(control.id, control.value.clone()),
            Ok(false),
            "re-applying the current value of {} must change nothing",
            control.id
        );
        let stepped = control.stepped(1).unwrap();
        let stepped = if stepped == control.value {
            control.stepped(-1).unwrap()
        } else {
            stepped
        };
        assert_eq!(
            settings.set_control(control.id, stepped.clone()),
            Ok(true),
            "{}",
            control.id
        );
        let shown = settings
            .controls()
            .into_iter()
            .find(|row| row.id == control.id)
            .unwrap();
        assert_eq!(shown.value, stepped, "{}", control.id);
        assert_eq!(settings.normalized(), settings);
    }
}

#[test]
fn invalid_control_id_and_wrong_type() {
    let mut settings = BoxMachineSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("speed", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("rails_dashed", ControlValue::Number(1))
        .is_err());
    assert_eq!(settings, BoxMachineSettings::default());
}

#[test]
fn defaults_render_something_but_not_uniform() {
    let rendered = frame_at(
        &mut scene_with(&BoxMachineSettings::default()),
        (100, 40),
        2.0,
    );
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
    assert!(
        rendered
            .raster
            .dots
            .iter()
            .filter(|dot| **dot > 0.05)
            .count()
            > 20
    );
    assert!(distinct_values(&rendered) > 2);
    let mean = rendered.raster.dots.iter().sum::<f32>() / rendered.raster.dots.len() as f32;
    assert!(mean < 0.08, "mean intensity {mean}");
}

#[test]
fn tiny_sizes_do_not_panic() {
    for cells in [(1, 1), (3, 2), (6, 3), (7, 9)] {
        for tile_size in [12, 28] {
            let settings = BoxMachineSettings {
                tile_size,
                ..BoxMachineSettings::default()
            };
            let mut scene = scene_with(&settings);
            let rendered = frame_at(&mut scene, cells, 1.0);
            assert_eq!(
                rendered.raster.dots.len(),
                usize::from(cells.0) * 2 * usize::from(cells.1) * 4
            );
        }
    }
    let mut scene = scene_with(&BoxMachineSettings::default());
    let mut empty = frame_at(&mut scene, (0, 0), 0.0);
    empty.raster.dots.clear();
}

#[test]
fn resizing_rebuilds_the_layout() {
    let mut scene = scene_with(&BoxMachineSettings::default());
    let small = frame_at(&mut scene, (40, 20), 1.0);
    let large = frame_at(&mut scene, (80, 40), 1.0);
    assert_eq!(small.raster.dots.len(), 80 * 80);
    assert_eq!(large.raster.dots.len(), 160 * 160);
    assert!(large.raster.dots.iter().any(|dot| *dot > 0.0));
}

#[test]
fn inspired_by_lists_the_source() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
    assert_eq!(
        BoxMachineScene::new(
            &BoxMachineSettings::default(),
            &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources())
        )
        .frames_per_second(),
        10
    );
}

fn build_layout(settings: &BoxMachineSettings, size: (usize, usize)) -> (Library, Layout) {
    let library = Library::new(settings.tile_size);
    let layout = Layout::build(&library, settings, size.0, size.1);
    (library, layout)
}

#[test]
fn library_tiles_are_consistent() {
    for tile_size in [12, 16, 20, 24, 28] {
        let library = Library::new(tile_size);
        assert!(library.tiles.len() > 100, "{} tiles", library.tiles.len());
        for tile in &library.tiles {
            let ports: Vec<usize> = tile
                .routes
                .iter()
                .flat_map(|r| library.routes[*r].ports)
                .collect();
            let mut unique = ports.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), ports.len(), "tiles never share a port");
            let bits: u32 = tile.edge_masks.iter().map(|m| m.count_ones()).sum();
            assert_eq!(bits as usize, ports.len());
        }
    }
}

#[test]
fn neighbouring_tiles_agree_and_border_is_closed() {
    for seed in [0, 7, 123, 9999] {
        let settings = BoxMachineSettings {
            seed,
            density: 100,
            ..BoxMachineSettings::default()
        };
        let (library, layout) = build_layout(&settings, (200, 160));
        let (grid_width, grid_height) = layout.grid;
        for y in 0..grid_height {
            for x in 0..grid_width {
                let masks = library.tiles[layout.tiles[y * grid_width + x]].edge_masks;
                if x + 1 < grid_width {
                    let east = library.tiles[layout.tiles[y * grid_width + x + 1]].edge_masks;
                    assert_eq!(masks[library::EAST], east[library::WEST]);
                } else {
                    assert_eq!(masks[library::EAST], 0);
                }
                if y + 1 < grid_height {
                    let south = library.tiles[layout.tiles[(y + 1) * grid_width + x]].edge_masks;
                    assert_eq!(masks[library::SOUTH], south[library::NORTH]);
                } else {
                    assert_eq!(masks[library::SOUTH], 0);
                }
                if x == 0 {
                    assert_eq!(masks[library::WEST], 0);
                }
                if y == 0 {
                    assert_eq!(masks[library::NORTH], 0);
                }
            }
        }
    }
}

#[test]
fn every_rail_belongs_to_a_closed_loop() {
    for seed in [1, 7, 42, 500] {
        let settings = BoxMachineSettings {
            seed,
            density: 100,
            ..BoxMachineSettings::default()
        };
        let (library, layout) = build_layout(&settings, (200, 160));
        let route_length: i32 = layout
            .tiles
            .iter()
            .flat_map(|tile| &library.tiles[*tile].routes)
            .map(|route| {
                library.routes[*route]
                    .points
                    .windows(2)
                    .map(|pair| (pair[1].0 - pair[0].0).abs() + (pair[1].1 - pair[0].1).abs())
                    .sum::<i32>()
            })
            .sum();
        let loop_length: i32 = layout.loops.iter().map(|l| l.length()).sum();
        assert!(!layout.loops.is_empty(), "seed {seed} has no loops");
        assert_eq!(
            route_length, loop_length,
            "seed {seed}: rails lost or duplicated"
        );
        for cube_loop in &layout.loops {
            for pair in cube_loop.points.windows(2) {
                assert!(pair[0].0 == pair[1].0 || pair[0].1 == pair[1].1);
            }
        }
    }
}

#[test]
fn boxes_stay_on_their_rails() {
    let (_, layout) = build_layout(&BoxMachineSettings::default(), (200, 160));
    for cube_loop in &layout.loops {
        for step in 0..200 {
            let distance = f64::from(step) * 3.37 - 100.0;
            let (x, y) = cube_loop.position(distance);
            let on_rail = super::layout::segment_pairs(&cube_loop.points).any(|(a, b)| {
                let (min_x, max_x) = (f64::from(a.0.min(b.0)), f64::from(a.0.max(b.0)));
                let (min_y, max_y) = (f64::from(a.1.min(b.1)), f64::from(a.1.max(b.1)));
                (min_x - 1e-6..=max_x + 1e-6).contains(&x)
                    && (min_y - 1e-6..=max_y + 1e-6).contains(&y)
            });
            assert!(on_rail, "({x}, {y}) left the rail");
        }
        let length = f64::from(cube_loop.length());
        let (x0, y0) = cube_loop.position(5.0);
        let (x1, y1) = cube_loop.position(5.0 + length);
        assert!(
            (x0 - x1).abs() < 1e-6 && (y0 - y1).abs() < 1e-6,
            "loop must wrap"
        );
    }
}

#[test]
fn screen_count_respects_the_cap() {
    for cap in 0..=6 {
        let settings = BoxMachineSettings {
            screens: cap,
            ..BoxMachineSettings::default()
        };
        let (_, layout) = build_layout(&settings, (200, 160));
        assert!(layout.screens.len() <= cap as usize);
        if cap == 0 {
            assert!(layout.screens.is_empty());
        }
    }
    let settings = BoxMachineSettings {
        screens: 6,
        ..BoxMachineSettings::default()
    };
    let (_, layout) = build_layout(&settings, (200, 160));
    assert!(!layout.screens.is_empty());
}

#[test]
fn density_controls_the_amount_of_rail() {
    let rail_length = |density| {
        let settings = BoxMachineSettings {
            density,
            ..BoxMachineSettings::default()
        };
        (0..8)
            .map(|seed| {
                let settings = BoxMachineSettings {
                    seed,
                    ..settings.clone()
                };
                let (_, layout) = build_layout(&settings, (200, 160));
                layout.loops.iter().map(|l| l.length()).sum::<i32>()
            })
            .sum::<i32>()
    };
    assert!(rail_length(100) > rail_length(10));
}

#[test]
fn speed_moves_boxes_and_zero_rail_level_hides_rails() {
    let settings = BoxMachineSettings {
        rail_level: 0,
        ..BoxMachineSettings::default()
    };
    let mut scene = scene_with(&settings);
    let first = frame_at(&mut scene, (100, 40), 0.0);
    let (_, layout) = build_layout(&settings, (200, 160));
    assert!(layout.rails.iter().all(|v| *v == 0.0));
    let later = frame_at(&mut scene, (100, 40), 4.0);
    assert_ne!(first.raster.dots, later.raster.dots);
    let brightest = first.raster.dots.iter().copied().fold(0.0, f32::max);
    assert!((brightest - 0.8).abs() < 1e-6, "boxes use cube_level");
}

#[test]
fn reverse_alt_flips_alternate_loops() {
    let alternating = build_layout(&BoxMachineSettings::default(), (200, 160)).1;
    let uniform = build_layout(
        &BoxMachineSettings {
            reverse_alt: false,
            ..BoxMachineSettings::default()
        },
        (200, 160),
    )
    .1;
    assert!(uniform.loops.iter().all(|l| l.direction > 0.0));
    if alternating.loops.len() > 1 {
        assert!(alternating.loops.iter().any(|l| l.direction < 0.0));
    }
}

#[test]
fn boxes_never_overlap_on_one_loop() {
    let (_, layout) = build_layout(&BoxMachineSettings::default(), (200, 160));
    for cube_loop in &layout.loops {
        let gap = f64::from(cube_loop.length()) / cube_loop.cube_count as f64;
        if cube_loop.cube_count > 1 {
            assert!(gap >= 12.0 / 2.0, "boxes spaced {gap}");
        }
    }
}

#[test]
fn full_frame_is_cheap() {
    let mut scene = scene_with(&BoxMachineSettings::default());
    frame_at(&mut scene, (100, 40), 0.0);
    let started = std::time::Instant::now();
    for step in 0..20 {
        frame_at(&mut scene, (100, 40), f64::from(step) * 0.1);
    }
    // Generous bound so debug builds and loaded machines pass.
    assert!(started.elapsed() < Duration::from_millis(20 * 250));
}
