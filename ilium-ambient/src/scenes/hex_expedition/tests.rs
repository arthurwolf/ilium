use super::canvas::{sd_box, sd_ellipse, sd_segment, sd_triangle, Canvas};
use super::settings::{MapChoice, PanStyle};
use super::world::{
    generate_island, hex_distance, locate, Atlas, Feature, Terrain, Tile, ISLAND_COLUMNS,
    ISLAND_ROWS, NEIGHBORS,
};
use super::*;
use crate::control::{Control, ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use crate::registry::{AmbientKind, AmbientSettings};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

fn scene_with(settings: &HexExpeditionSettings) -> HexExpeditionScene {
    HexExpeditionScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

fn frame_at(settings: &HexExpeditionSettings, cols: u16, rows: u16, seconds: f64) -> Rendered {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, cols, rows, Duration::from_secs_f64(seconds))
}

fn fixed(map: MapChoice) -> HexExpeditionSettings {
    HexExpeditionSettings {
        map,
        ..HexExpeditionSettings::default()
    }
}

/// Pan and every tile animation off: only the camera could still move.
fn frozen() -> HexExpeditionSettings {
    HexExpeditionSettings {
        pan_speed: 0,
        animation_speed: 0,
        weather: false,
        cloud_shadows: false,
        ..HexExpeditionSettings::default()
    }
}

/// Dots that survive the host's ordered dither at full density.
fn lit(rendered: &Rendered) -> usize {
    let width = rendered.raster.width;
    rendered
        .raster
        .dots
        .iter()
        .enumerate()
        .filter(|(index, dot)| {
            **dot > crate::raster::threshold(index % width, index / width, DitherMode::Ordered)
        })
        .count()
}

#[test]
fn same_time_gives_identical_raster_and_colors() {
    let settings = HexExpeditionSettings::default();
    let first = frame_at(&settings, 60, 24, 12.5);
    let second = frame_at(&settings, 60, 24, 12.5);
    assert_eq!(first.raster.dots, second.raster.dots);
    assert_eq!(first.cell_colors, second.cell_colors);
}

#[test]
fn render_does_not_depend_on_previous_frames() {
    let settings = HexExpeditionSettings::default();
    let mut scene = scene_with(&settings);
    let _ = render_frame(&mut scene, 60, 24, Duration::from_secs(3));
    let later = render_frame(&mut scene, 60, 24, Duration::from_secs(40));
    let fresh = frame_at(&settings, 60, 24, 40.0);
    assert_eq!(later.raster.dots, fresh.raster.dots);
    assert_eq!(later.cell_colors, fresh.cell_colors);
}

#[test]
fn different_times_seeds_and_maps_differ() {
    let base = HexExpeditionSettings::default();
    let first = frame_at(&base, 60, 24, 1.0);
    assert_ne!(first.raster.dots, frame_at(&base, 60, 24, 6.0).raster.dots);
    let other_seed = HexExpeditionSettings {
        seed: 500,
        ..base.clone()
    };
    assert_ne!(
        first.raster.dots,
        frame_at(&other_seed, 60, 24, 1.0).raster.dots
    );
    let mut seen = HashSet::new();
    for choice in MapChoice::ALL.iter().skip(1) {
        let rendered = frame_at(&fixed(*choice), 60, 24, 5.0);
        let colors: Vec<u8> = rendered.cell_colors.iter().flatten().copied().collect();
        assert!(seen.insert(colors), "{choice:?} repeats another map");
    }
}

#[test]
fn every_map_draws_a_non_uniform_picture_within_range() {
    for choice in MapChoice::ALL {
        let rendered = frame_at(&fixed(choice), 80, 30, 9.0);
        let total = rendered.raster.dots.len();
        let on = lit(&rendered);
        assert!(on > total / 50, "{choice:?} too empty: {on}/{total}");
        assert!(on < total * 9 / 10, "{choice:?} too full: {on}/{total}");
        assert!(rendered
            .raster
            .dots
            .iter()
            .all(|dot| dot.is_finite() && (0.0..=1.0).contains(dot)));
        // Tone has real range: sprites near solid, ground sparse.
        let bright = rendered
            .raster
            .dots
            .iter()
            .filter(|dot| **dot > 0.8)
            .count();
        let dim = rendered
            .raster
            .dots
            .iter()
            .filter(|dot| (0.02..0.5).contains(*dot))
            .count();
        assert!(bright > 0 && dim > 0, "{choice:?} has no tonal range");
    }
}

#[test]
fn tint_controls_cell_colors_and_the_palette_stays_biome_coloured() {
    let plain = HexExpeditionSettings {
        tint: false,
        ..HexExpeditionSettings::default()
    };
    assert!(!scene_with(&plain).uses_cell_colors());
    assert!(scene_with(&HexExpeditionSettings::default()).uses_cell_colors());
    let rendered = frame_at(&fixed(MapChoice::Jungle), 80, 30, 4.0);
    let distinct: HashSet<[u8; 3]> = rendered.cell_colors.iter().copied().collect();
    assert!(distinct.len() > 4, "only {} colours", distinct.len());
    // Jungle is mostly green and blue: green dominates red on average.
    let (mut red, mut green) = (0u64, 0u64);
    for color in &rendered.cell_colors {
        red += u64::from(color[0]);
        green += u64::from(color[1]);
    }
    assert!(green > red);
}

#[test]
fn raster_has_continuous_tone_not_only_one_bit() {
    let rendered = frame_at(&HexExpeditionSettings::default(), 60, 24, 3.0);
    let distinct: HashSet<u32> = rendered
        .raster
        .dots
        .iter()
        .map(|dot| (dot * 100.0) as u32)
        .collect();
    assert!(distinct.len() > 12, "only {} tone levels", distinct.len());
}

#[test]
fn tiny_sizes_and_zero_do_not_panic() {
    for (cols, rows) in [(1, 1), (3, 2), (1, 5), (7, 1), (200, 3)] {
        for choice in MapChoice::ALL {
            for tint in [true, false] {
                let settings = HexExpeditionSettings {
                    map: choice,
                    tint,
                    ..HexExpeditionSettings::default()
                };
                let rendered = frame_at(&settings, cols, rows, 3.0);
                assert_eq!(
                    rendered.raster.dots.len(),
                    usize::from(cols) * 2 * usize::from(rows) * 4
                );
            }
        }
    }
    let mut scene = scene_with(&HexExpeditionSettings::default());
    let empty = render_frame(&mut scene, 0, 0, Duration::ZERO);
    assert!(empty.raster.dots.is_empty());
}

#[test]
fn extreme_settings_and_times_stay_finite() {
    let extremes = [
        HexExpeditionSettings {
            tile_size: 10,
            pan_speed: 300,
            animation_speed: 300,
            landmarks: 200,
            brightness: 150,
            ..HexExpeditionSettings::default()
        },
        HexExpeditionSettings {
            tile_size: 40,
            pan_speed: 0,
            animation_speed: 0,
            landmarks: 0,
            brightness: 50,
            stepped: false,
            ..HexExpeditionSettings::default()
        },
    ];
    for settings in extremes {
        for seconds in [0.0, 1.0e6, 3.0e8, 4.0e9] {
            let rendered = frame_at(&settings, 50, 20, seconds);
            assert!(lit(&rendered) > 0, "empty at t={seconds}");
            assert!(rendered
                .raster
                .dots
                .iter()
                .all(|dot| dot.is_finite() && (0.0..=1.0).contains(dot)));
        }
    }
}

#[test]
fn a_still_camera_with_frozen_tiles_is_static() {
    let still = HexExpeditionSettings {
        map: MapChoice::Jungle,
        ..frozen()
    };
    let first = frame_at(&still, 60, 24, 2.0);
    let later = frame_at(&still, 60, 24, 500.0);
    assert_eq!(first.raster.dots, later.raster.dots);
    assert_eq!(first.cell_colors, later.cell_colors);
}

#[test]
fn panning_moves_the_camera_steadily_and_matches_the_requested_route() {
    let east = HexExpeditionSettings {
        pan_style: PanStyle::East,
        ..HexExpeditionSettings::default()
    };
    let scene = scene_with(&east);
    let mut previous = scene.camera(0.0);
    for step in 1..=200 {
        let camera = scene.camera(f64::from(step) * 0.5);
        assert!(camera.x > previous.x, "east pan must advance");
        assert!((camera.y - previous.y).abs() < 1e-9);
        previous = camera;
    }
    let north_east = scene_with(&HexExpeditionSettings {
        pan_style: PanStyle::NorthEast,
        ..HexExpeditionSettings::default()
    });
    assert!(north_east.camera(50.0).y < north_east.camera(0.0).y);
    let south = scene_with(&HexExpeditionSettings {
        pan_style: PanStyle::South,
        ..HexExpeditionSettings::default()
    });
    assert!(south.camera(50.0).y > south.camera(0.0).y);
    // Speed scales distance.
    let slow = scene_with(&HexExpeditionSettings {
        pan_speed: 50,
        pan_style: PanStyle::East,
        ..HexExpeditionSettings::default()
    });
    let travelled = |scene: &HexExpeditionScene| scene.camera(100.0).x - scene.camera(0.0).x;
    assert!((travelled(&scene) / travelled(&slow) - 2.0).abs() < 1e-6);
}

#[test]
fn the_wandering_route_never_stops_or_jumps() {
    let scene = scene_with(&HexExpeditionSettings::default());
    let step_dots = f64::from(scene.radius()) * 3.0;
    let mut previous = scene.camera(0.0);
    let mut moved = 0.0;
    for step in 1..=2000 {
        let camera = scene.camera(f64::from(step) * 0.25);
        let distance = ((camera.x - previous.x).powi(2) + (camera.y - previous.y).powi(2)).sqrt();
        assert!(distance < step_dots, "jump of {distance} dots");
        moved += distance;
        previous = camera;
    }
    assert!(moved > 500.0 * 0.1 * f64::from(scene.radius()));
}

#[test]
fn pan_is_smooth_between_frames() {
    // One frame step at 15 fps moves the picture by less than a tile.
    let scene = scene_with(&HexExpeditionSettings::default());
    let a = scene.camera(10.0);
    let b = scene.camera(10.0 + 1.0 / 15.0);
    let moved = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    assert!(moved < f64::from(scene.radius()) * 0.3, "{moved}");
}

#[test]
fn the_cycle_visits_every_map_in_order_and_hands_over_seamlessly() {
    let settings = HexExpeditionSettings {
        map_seconds: 60,
        ..HexExpeditionSettings::default()
    };
    let scene = scene_with(&settings);
    let mut order = Vec::new();
    for epoch in 0..10u32 {
        let plan = scene.map_plan(f64::from(epoch) * 60.0 + 1.0);
        assert_eq!(plan.reveal, 0.0);
        order.push(plan.current.kind);
        // The world revealed at the end of one epoch is the world shown at
        // the start of the next, bit for bit.
        let before = scene.map_plan(f64::from(epoch) * 60.0 + 59.999);
        let after = scene.map_plan(f64::from(epoch + 1) * 60.0 + 0.001);
        assert_eq!(before.next.kind, after.current.kind);
        assert_eq!(before.next.seed, after.current.seed);
        assert!(before.reveal > 0.99, "reveal ends at {}", before.reveal);
    }
    assert_eq!(&order[..5], &MapKind::ALL);
    assert_eq!(&order[5..], &MapKind::ALL);
}

#[test]
fn the_reveal_is_monotonic_and_only_happens_near_the_end_of_a_map() {
    let scene = scene_with(&HexExpeditionSettings {
        map_seconds: 100,
        ..HexExpeditionSettings::default()
    });
    let mut previous = 0.0;
    let mut revealing = false;
    for step in 0..1000 {
        let plan = scene.map_plan(f64::from(step) * 0.1);
        if plan.reveal > 0.0 {
            revealing = true;
            assert!(plan.reveal >= previous, "reveal went backwards");
            assert!(plan.reveal <= 1.0 + 1e-6);
            assert!(f64::from(step) * 0.1 >= 100.0 - TRANSITION_SECONDS - 0.2);
        }
        previous = plan.reveal;
    }
    assert!(revealing);
    // Short dwell: the reveal never takes more than 40 % of it.
    let short = scene_with(&HexExpeditionSettings {
        map_seconds: 20,
        ..HexExpeditionSettings::default()
    });
    assert_eq!(short.map_plan(11.9).reveal, 0.0);
    assert!(short.map_plan(12.5).reveal > 0.0);
}

#[test]
fn a_fixed_map_never_reveals_anything() {
    for choice in MapChoice::ALL.iter().skip(1) {
        let scene = scene_with(&fixed(*choice));
        for seconds in [0.0, 59.9, 61.0, 1.0e5] {
            let plan = scene.map_plan(seconds);
            assert_eq!(plan.reveal, 0.0);
            assert_eq!(plan.current.kind, plan.next.kind);
        }
    }
    assert_eq!(
        scene_with(&fixed(MapChoice::Desert))
            .map_plan(5.0)
            .current
            .kind,
        MapKind::Desert
    );
}

#[test]
fn the_reveal_front_sweeps_across_the_screen_with_a_fog_band() {
    let plan = |reveal: f32| MapPlan {
        current: World::new(MapKind::Jungle, 1, 1.0),
        next: World::new(MapKind::Arctic, 2, 1.0),
        reveal,
    };
    // No reveal under way: always the current map, never fog.
    assert_eq!(world_at(&plan(0.0), 0.5).0.kind, MapKind::Jungle);
    assert_eq!(world_at(&plan(0.0), 0.5).1, 0.0);
    // Halfway: left of the front is the new map, right of it the old one.
    let halfway = plan(0.5);
    assert_eq!(world_at(&halfway, 0.0).0.kind, MapKind::Arctic);
    assert_eq!(world_at(&halfway, 0.99).0.kind, MapKind::Jungle);
    let mut fog_seen = false;
    for step in 0..=100 {
        let (world, fog) = world_at(&halfway, step as f32 / 100.0);
        if fog > 0.0 {
            fog_seen = true;
            assert_eq!(world.kind, MapKind::Arctic);
            assert!(fog <= 1.0);
        }
    }
    assert!(fog_seen);
    // Complete: everything on screen is the new map.
    for step in 0..=100 {
        assert_eq!(
            world_at(&plan(1.0), step as f32 / 100.0).0.kind,
            MapKind::Arctic
        );
    }
}

#[test]
fn the_reveal_changes_the_picture_over_time() {
    let settings = HexExpeditionSettings {
        pan_speed: 0,
        animation_speed: 0,
        weather: false,
        cloud_shadows: false,
        ..HexExpeditionSettings::default()
    };
    let before = frame_at(&settings, 60, 24, 10.0);
    let middle = frame_at(&settings, 60, 24, 53.0);
    let after = frame_at(&settings, 60, 24, 59.99);
    assert_ne!(before.raster.dots, middle.raster.dots);
    assert_ne!(middle.raster.dots, after.raster.dots);
    // The finished reveal looks like the next map at the start of its epoch
    // (same world, same camera when panning is off).
    let next_start = frame_at(&settings, 60, 24, 60.0);
    assert_eq!(after.cell_colors.len(), next_start.cell_colors.len());
    let same = after
        .raster
        .dots
        .iter()
        .zip(&next_start.raster.dots)
        .filter(|(a, b)| (**a - **b).abs() < 1e-6)
        .count();
    assert!(
        same * 100 > after.raster.dots.len() * 95,
        "{same}/{} dots match across the hand-over",
        after.raster.dots.len()
    );
}

/// Island 0,0 of a world, with the global axial coordinate of each tile.
type PlacedTile = ((i32, i32), Tile);

fn island_tiles(kind: MapKind, seed: u32, scale: f32) -> (World, Vec<PlacedTile>) {
    let world = World::new(kind, seed, scale);
    let island = generate_island(&world, 0, 0);
    let mut tiles = Vec::new();
    for row in 0..ISLAND_ROWS {
        for column in 0..ISLAND_COLUMNS {
            tiles.push(((column - row.div_euclid(2), row), island.tile(column, row)));
        }
    }
    (world, tiles)
}

fn features_of(tiles: &[PlacedTile], wanted: Feature) -> Vec<(i32, i32)> {
    tiles
        .iter()
        .filter(|(_, tile)| tile.feature == wanted)
        .map(|(at, _)| *at)
        .collect()
}

const SEEDS: [u32; 6] = [7, 11, 42, 99, 123, 777];

#[test]
fn every_island_has_one_ship_and_one_distant_goal() {
    let mut atlas = Atlas::default();
    for kind in MapKind::ALL {
        for seed in SEEDS {
            let (world, tiles) = island_tiles(kind, seed, 1.0);
            let ships = features_of(&tiles, Feature::Ship);
            assert_eq!(ships.len(), 1, "{kind:?}/{seed}: ships {ships:?}");
            let goal_feature = if matches!(kind, MapKind::Savanna | MapKind::Desert) {
                Feature::Pyramid
            } else {
                Feature::Temple
            };
            let goals = features_of(&tiles, goal_feature);
            assert_eq!(goals.len(), 1, "{kind:?}/{seed}: goals {goals:?}");
            let other_goal = if goal_feature == Feature::Pyramid {
                Feature::Temple
            } else {
                Feature::Pyramid
            };
            assert!(features_of(&tiles, other_goal).is_empty());
            assert!(
                hex_distance(ships[0], goals[0]) >= 12,
                "{kind:?}/{seed}: goal is only {} from the ship",
                hex_distance(ships[0], goals[0])
            );
            // The ship floats on open water beside the shore.
            let ship_tile = atlas.tile(&world, ships[0].0, ships[0].1);
            assert_eq!(ship_tile.terrain, Terrain::Water);
            let touches_land = NEIGHBORS.iter().any(|(dq, dr)| {
                !atlas
                    .tile(&world, ships[0].0 + dq, ships[0].1 + dr)
                    .terrain
                    .is_water()
            });
            assert!(touches_land || kind == MapKind::Arctic);
            // The goal stands on open ground well away from every shore.
            let goal_tile = atlas.tile(&world, goals[0].0, goals[0].1);
            assert!(
                goal_tile.terrain.is_flat_land(),
                "{kind:?}: {:?}",
                goal_tile.terrain
            );
            for (dq, dr) in NEIGHBORS {
                // A lake may touch it, the sea may not.
                assert_ne!(
                    atlas.tile(&world, goals[0].0 + dq, goals[0].1 + dr).terrain,
                    Terrain::DeepWater
                );
            }
        }
    }
}

#[test]
fn landmarks_keep_their_distance_and_suit_their_terrain() {
    for kind in MapKind::ALL {
        for seed in SEEDS {
            let (_, tiles) = island_tiles(kind, seed, 1.0);
            let placed: Vec<PlacedTile> = tiles
                .iter()
                .filter(|(_, tile)| tile.feature != Feature::None)
                .copied()
                .collect();
            for (index, (at, tile)) in placed.iter().enumerate() {
                for (other_at, other) in &placed[index + 1..] {
                    let distance = hex_distance(*at, *other_at);
                    let needed = match (tile.feature, other.feature) {
                        (Feature::Village, Feature::Village) => 4,
                        (Feature::Ship, _) | (_, Feature::Ship) => 1,
                        _ => 2,
                    };
                    assert!(
                        distance >= needed,
                        "{kind:?}/{seed}: {:?} and {:?} only {distance} apart",
                        tile.feature,
                        other.feature
                    );
                }
                let terrain = tile.terrain;
                match tile.feature {
                    Feature::Ship => assert!(terrain.is_water()),
                    Feature::Village => assert!(
                        terrain.is_flat_land() && terrain != Terrain::Beach,
                        "{kind:?}: village on {terrain:?}"
                    ),
                    Feature::Cave => assert!(
                        matches!(terrain, Terrain::Hills | Terrain::Mountain | Terrain::Mesa),
                        "{kind:?}: cave on {terrain:?}"
                    ),
                    Feature::Mine => {
                        assert!(terrain.is_rough_land(), "{kind:?}: mine on {terrain:?}")
                    }
                    Feature::Shrine => assert!(
                        matches!(
                            terrain,
                            Terrain::Hills
                                | Terrain::Forest
                                | Terrain::Rock
                                | Terrain::Jungle
                                | Terrain::Pines
                        ),
                        "{kind:?}: shrine on {terrain:?}"
                    ),
                    Feature::Pyramid | Feature::Temple | Feature::Camp | Feature::Ruins => {
                        assert!(
                            !terrain.is_water(),
                            "{kind:?}: {:?} on {terrain:?}",
                            tile.feature
                        )
                    }
                    Feature::None => {}
                }
            }
            // A believable amount: not bare, not crowded.
            assert!(
                (10..=30).contains(&placed.len()),
                "{kind:?}/{seed}: {} landmarks",
                placed.len()
            );
            assert!(features_of(&tiles, Feature::Village).len() >= 3);
            assert!(!features_of(&tiles, Feature::Camp).is_empty());
        }
    }
}

#[test]
fn the_landmark_setting_scales_counts_and_zero_removes_everything() {
    for kind in MapKind::ALL {
        let count = |scale: f32| {
            island_tiles(kind, 7, scale)
                .1
                .iter()
                .filter(|(_, tile)| tile.feature != Feature::None)
                .count()
        };
        assert_eq!(
            count(0.0),
            0,
            "{kind:?}: zero landmarks leaves {}",
            count(0.0)
        );
        assert!(
            count(0.4) < count(1.0) && count(1.0) < count(2.0),
            "{kind:?}"
        );
        // The ship and the goal never multiply with density.
        let tiles = island_tiles(kind, 7, 2.0).1;
        assert_eq!(features_of(&tiles, Feature::Ship).len(), 1);
    }
}

#[test]
fn islands_are_single_landmasses_ringed_by_sea() {
    let mut atlas = Atlas::default();
    for kind in MapKind::ALL {
        for seed in SEEDS {
            let (world, tiles) = island_tiles(kind, seed, 1.0);
            let land: Vec<(i32, i32)> = tiles
                .iter()
                .filter(|(_, tile)| {
                    !tile.terrain.is_water() && tile.terrain != Terrain::Ice || {
                        kind == MapKind::Arctic && tile.terrain == Terrain::Ice
                    }
                })
                .map(|(at, _)| *at)
                .collect();
            assert!(
                land.len() > 250,
                "{kind:?}/{seed}: only {} land tiles",
                land.len()
            );
            assert!(
                land.len() < 900,
                "{kind:?}/{seed}: {} land tiles",
                land.len()
            );
            // Connected: flood from one land tile reaches all of it.
            let set: HashSet<(i32, i32)> = land.iter().copied().collect();
            let mut seen = HashSet::new();
            let mut stack = vec![land[0]];
            while let Some(at) = stack.pop() {
                if !seen.insert(at) {
                    continue;
                }
                for (dq, dr) in NEIGHBORS {
                    let next = (at.0 + dq, at.1 + dr);
                    if set.contains(&next) && !seen.contains(&next) {
                        stack.push(next);
                    }
                }
            }
            let share = seen.len() as f32 / land.len() as f32;
            assert!(
                share > 0.97,
                "{kind:?}/{seed}: landmass is {share} connected"
            );
            // The border band of the cell is open water.
            for ((q, r), tile) in &tiles {
                let (_, _, column, row) = locate(*q, *r);
                if column < 2 || row < 2 || column >= ISLAND_COLUMNS - 2 || row >= ISLAND_ROWS - 2 {
                    assert!(
                        tile.terrain.is_water() || tile.terrain == Terrain::Ice,
                        "{kind:?}/{seed}: {:?} on the border",
                        tile.terrain
                    );
                }
            }
            // Neighbouring islands never touch this one.
            let beyond = atlas.tile(&world, ISLAND_COLUMNS + 1, 0);
            assert!(beyond.terrain.is_water());
        }
    }
}

#[test]
fn shores_are_built_from_beach_and_deep_water_is_far_from_land() {
    let mut atlas = Atlas::default();
    for kind in MapKind::ALL {
        for seed in SEEDS {
            let (world, tiles) = island_tiles(kind, seed, 1.0);
            for ((q, r), tile) in &tiles {
                let neighbors: Vec<Tile> = NEIGHBORS
                    .iter()
                    .map(|(dq, dr)| atlas.tile(&world, q + dq, r + dr))
                    .collect();
                let touches_water = neighbors.iter().any(|neighbor| neighbor.terrain.is_water());
                match tile.terrain {
                    Terrain::Mangrove => {
                        assert!(touches_water, "{kind:?}/{seed}: inland mangrove")
                    }
                    Terrain::DeepWater => assert!(
                        neighbors.iter().all(|neighbor| neighbor.terrain.is_water()),
                        "{kind:?}/{seed}: deep water beside land"
                    ),
                    Terrain::Mountain | Terrain::Volcano | Terrain::Glacier | Terrain::Mesa => {
                        assert!(
                            !neighbors
                                .iter()
                                .any(|neighbor| neighbor.terrain == Terrain::DeepWater),
                            "{kind:?}/{seed}: {:?} on the shore",
                            tile.terrain
                        )
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn every_map_type_has_its_own_terrain_mix() {
    let expect: [(MapKind, &[Terrain], &[Terrain]); 5] = [
        (
            MapKind::Jungle,
            &[
                Terrain::Jungle,
                Terrain::Forest,
                Terrain::Mangrove,
                Terrain::Swamp,
                Terrain::Mountain,
                Terrain::Beach,
                Terrain::Reef,
            ],
            &[
                Terrain::Snow,
                Terrain::Dunes,
                Terrain::Lava,
                Terrain::Glacier,
                Terrain::Pines,
            ],
        ),
        (
            MapKind::Savanna,
            &[
                Terrain::Grass,
                Terrain::Forest,
                Terrain::Desert,
                Terrain::Hills,
                Terrain::Beach,
            ],
            &[
                Terrain::Snow,
                Terrain::Lava,
                Terrain::Mangrove,
                Terrain::Dunes,
                Terrain::Glacier,
            ],
        ),
        (
            MapKind::Desert,
            &[
                Terrain::Desert,
                Terrain::Dunes,
                Terrain::Mesa,
                Terrain::Oasis,
                Terrain::Mountain,
                Terrain::Beach,
            ],
            &[
                Terrain::Snow,
                Terrain::Jungle,
                Terrain::Lava,
                Terrain::Pines,
                Terrain::Swamp,
            ],
        ),
        (
            MapKind::Arctic,
            &[
                Terrain::Snow,
                Terrain::Pines,
                Terrain::Glacier,
                Terrain::Ice,
                Terrain::Mountain,
                Terrain::DeepWater,
            ],
            &[
                Terrain::Jungle,
                Terrain::Desert,
                Terrain::Lava,
                Terrain::Beach,
                Terrain::Reef,
            ],
        ),
        (
            MapKind::Volcanic,
            &[
                Terrain::Rock,
                Terrain::DeadForest,
                Terrain::Lava,
                Terrain::Volcano,
                Terrain::Geyser,
                Terrain::Mountain,
            ],
            &[
                Terrain::Snow,
                Terrain::Jungle,
                Terrain::Pines,
                Terrain::Swamp,
                Terrain::Oasis,
            ],
        ),
    ];
    let mut union: HashSet<Terrain> = HashSet::new();
    for (kind, wanted, forbidden) in expect {
        let mut seen: HashMap<Terrain, usize> = HashMap::new();
        for seed in SEEDS {
            for (_, tile) in island_tiles(kind, seed, 1.0).1 {
                *seen.entry(tile.terrain).or_default() += 1;
            }
        }
        for terrain in wanted {
            assert!(seen.contains_key(terrain), "{kind:?} never has {terrain:?}");
        }
        for terrain in forbidden {
            assert!(!seen.contains_key(terrain), "{kind:?} has {terrain:?}");
        }
        let total: usize = seen.values().sum();
        let dominant = seen.values().copied().max().unwrap_or(0);
        assert!(dominant * 10 < total * 5, "{kind:?} is mostly one terrain");
        union.extend(seen.keys().copied());
    }
    // Every terrain type is used by at least one map.
    for terrain in ALL_TERRAIN {
        assert!(union.contains(&terrain), "{terrain:?} never generated");
    }
    assert!(ALL_TERRAIN.len() >= 24);
}

const ALL_TERRAIN: [Terrain; 24] = [
    Terrain::DeepWater,
    Terrain::Water,
    Terrain::Reef,
    Terrain::Ice,
    Terrain::Beach,
    Terrain::Grass,
    Terrain::Forest,
    Terrain::Jungle,
    Terrain::Mangrove,
    Terrain::Swamp,
    Terrain::Desert,
    Terrain::Dunes,
    Terrain::Mesa,
    Terrain::Hills,
    Terrain::Mountain,
    Terrain::Glacier,
    Terrain::Snow,
    Terrain::Pines,
    Terrain::Rock,
    Terrain::DeadForest,
    Terrain::Lava,
    Terrain::Geyser,
    Terrain::Volcano,
    Terrain::Oasis,
];

#[test]
fn every_landmark_type_is_generated() {
    let mut seen: HashSet<Feature> = HashSet::new();
    for kind in MapKind::ALL {
        for seed in SEEDS {
            for (_, tile) in island_tiles(kind, seed, 1.0).1 {
                seen.insert(tile.feature);
            }
        }
    }
    for feature in [
        Feature::Village,
        Feature::Temple,
        Feature::Pyramid,
        Feature::Camp,
        Feature::Ruins,
        Feature::Ship,
        Feature::Cave,
        Feature::Shrine,
        Feature::Mine,
    ] {
        assert!(seen.contains(&feature), "{feature:?} never placed");
    }
}

#[test]
fn volcanic_islands_have_a_great_volcano_ringed_by_high_ground() {
    let mut atlas = Atlas::default();
    for seed in SEEDS {
        let (world, tiles) = island_tiles(MapKind::Volcanic, seed, 1.0);
        let volcanoes: Vec<(i32, i32)> = tiles
            .iter()
            .filter(|(_, tile)| tile.terrain == Terrain::Volcano)
            .map(|(at, _)| *at)
            .collect();
        assert!(
            (1..=2).contains(&volcanoes.len()),
            "{seed}: {} volcanoes",
            volcanoes.len()
        );
        for at in volcanoes {
            for (dq, dr) in NEIGHBORS {
                let ring = atlas.tile(&world, at.0 + dq, at.1 + dr).terrain;
                assert!(
                    matches!(
                        ring,
                        Terrain::Mountain | Terrain::Hills | Terrain::Lava | Terrain::Volcano
                    ),
                    "{seed}: {ring:?} beside a volcano"
                );
            }
        }
    }
}

#[test]
fn islands_are_pure_distinct_and_survive_atlas_eviction() {
    let world = World::new(MapKind::Jungle, 42, 1.0);
    let mut atlas = Atlas::default();
    let reference: Vec<Tile> = (0..60)
        .map(|step| atlas.tile(&world, step - 20, step / 3))
        .collect();
    // Touch far more islands than the memo holds, then ask again.
    for island in 0..120 {
        let _ = atlas.tile(&world, island * ISLAND_COLUMNS, 0);
    }
    let again: Vec<Tile> = (0..60)
        .map(|step| atlas.tile(&world, step - 20, step / 3))
        .collect();
    assert_eq!(reference, again);
    let mut fresh = Atlas::default();
    let third: Vec<Tile> = (0..60)
        .map(|step| fresh.tile(&world, step - 20, step / 3))
        .collect();
    assert_eq!(reference, third);
    // Seeds and neighbouring islands differ.
    let other = World::new(MapKind::Jungle, 43, 1.0);
    let differing = (0..ISLAND_ROWS)
        .flat_map(|r| (0..ISLAND_COLUMNS).map(move |c| (c - r.div_euclid(2), r)))
        .filter(|(q, r)| atlas.tile(&world, *q, *r) != atlas.tile(&other, *q, *r))
        .count();
    assert!(differing > 200, "seeds barely differ: {differing}");
    let east: Vec<Tile> = (0..ISLAND_ROWS)
        .map(|r| atlas.tile(&world, 20 + ISLAND_COLUMNS - r.div_euclid(2), r))
        .collect();
    let home: Vec<Tile> = (0..ISLAND_ROWS)
        .map(|r| atlas.tile(&world, 20 - r.div_euclid(2), r))
        .collect();
    assert_ne!(east, home);
    // Huge and negative coordinates are fine.
    for (q, r) in [
        (0, 0),
        (-7, -13),
        (900_000, -900_000),
        (i32::MAX / 4, i32::MIN / 4),
    ] {
        assert_eq!(atlas.tile(&world, q, r), atlas.tile(&world, q, r));
    }
}

#[test]
fn locating_a_tile_round_trips_through_island_cells() {
    for (q, r) in [
        (0, 0),
        (39, 0),
        (40, 0),
        (-1, -1),
        (-41, 29),
        (12, 30),
        (-5, -61),
        (100, 100),
    ] {
        let (island_x, island_y, column, row) = locate(q, r);
        assert!((0..ISLAND_COLUMNS).contains(&column) && (0..ISLAND_ROWS).contains(&row));
        let back_r = island_y * ISLAND_ROWS + row;
        let back_q = island_x * ISLAND_COLUMNS + column - back_r.div_euclid(2);
        assert_eq!((back_q, back_r), (q, r));
    }
    assert_eq!(hex_distance((0, 0), (3, -3)), 3);
    assert_eq!(hex_distance((0, 0), (2, 2)), 4);
    assert_eq!(hex_distance((5, 5), (5, 5)), 0);
}

#[test]
fn camera_precision_survives_days_of_uptime() {
    // Two instants a frame apart, three days in: the picture must still move
    // by a sub-dot amount, not jitter or freeze.
    let scene = scene_with(&HexExpeditionSettings::default());
    let day = 86_400.0 * 3.0;
    let a = scene.camera(day);
    let b = scene.camera(day + 1.0 / 15.0);
    let moved = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    assert!(moved > 0.1 && moved < 2.0, "moved {moved} dots");
}

#[test]
fn tile_animation_follows_its_speed_and_stepping() {
    let smooth = scene_with(&HexExpeditionSettings {
        stepped: false,
        ..HexExpeditionSettings::default()
    });
    assert!((smooth.tile_time(10.0) - 10.0).abs() < 1e-4);
    let fast = scene_with(&HexExpeditionSettings {
        stepped: false,
        animation_speed: 200,
        ..HexExpeditionSettings::default()
    });
    assert!((fast.tile_time(10.0) - 20.0).abs() < 1e-4);
    let frozen_tiles = scene_with(&HexExpeditionSettings {
        animation_speed: 0,
        ..HexExpeditionSettings::default()
    });
    assert_eq!(frozen_tiles.tile_time(1234.5), 0.0);
    // Stepped time changes only at frame boundaries.
    let stepped = scene_with(&HexExpeditionSettings::default());
    let quantum = 1.0 / f64::from(STEPPED_FPS);
    assert_eq!(
        stepped.tile_time(1.01),
        stepped.tile_time(1.0 + quantum * 0.9)
    );
    assert!(stepped.tile_time(1.0 + quantum * 1.1) > stepped.tile_time(1.0));
}

#[test]
fn tile_animation_moves_pixels_while_the_camera_is_still() {
    let animated = HexExpeditionSettings {
        pan_speed: 0,
        weather: false,
        cloud_shadows: false,
        map: MapChoice::Volcanic,
        ..HexExpeditionSettings::default()
    };
    let first = frame_at(&animated, 80, 30, 1.0);
    let second = frame_at(&animated, 80, 30, 3.0);
    assert_ne!(first.raster.dots, second.raster.dots);
    let changed = first
        .raster
        .dots
        .iter()
        .zip(&second.raster.dots)
        .filter(|(a, b)| (**a - **b).abs() > 0.1)
        .count();
    // Animation is local: most of the picture stays put.
    assert!(
        changed > 0 && changed * 3 < first.raster.dots.len(),
        "{changed}"
    );
}

#[test]
fn weather_and_cloud_shadows_are_optional_layers() {
    let base = HexExpeditionSettings {
        pan_speed: 0,
        animation_speed: 0,
        ..HexExpeditionSettings::default()
    };
    for choice in MapChoice::ALL.iter().skip(1) {
        let none = HexExpeditionSettings {
            map: *choice,
            weather: false,
            cloud_shadows: false,
            ..base.clone()
        };
        let weather = HexExpeditionSettings {
            weather: true,
            ..none.clone()
        };
        let clouds = HexExpeditionSettings {
            cloud_shadows: true,
            ..none.clone()
        };
        let plain = frame_at(&none, 80, 30, 7.0);
        assert_ne!(
            plain.raster.dots,
            frame_at(&weather, 80, 30, 7.0).raster.dots
        );
        assert_ne!(
            plain.raster.dots,
            frame_at(&clouds, 80, 30, 7.0).raster.dots
        );
        // Weather moves; a still camera still shows different air later.
        assert_ne!(
            frame_at(&weather, 80, 30, 7.0).raster.dots,
            frame_at(&weather, 80, 30, 11.0).raster.dots
        );
    }
}

#[test]
fn brightness_scales_tone_and_never_leaves_range() {
    let dim = frame_at(
        &HexExpeditionSettings {
            brightness: 50,
            ..frozen()
        },
        60,
        24,
        3.0,
    );
    let bright = frame_at(
        &HexExpeditionSettings {
            brightness: 150,
            ..frozen()
        },
        60,
        24,
        3.0,
    );
    let sum = |rendered: &Rendered| rendered.raster.dots.iter().sum::<f32>();
    assert!(sum(&dim) < sum(&bright));
    for rendered in [&dim, &bright] {
        assert!(rendered
            .raster
            .dots
            .iter()
            .all(|dot| (0.0..=1.0).contains(dot)));
    }
}

#[test]
fn tile_size_sets_the_hex_radius_and_changes_the_picture() {
    let at = |size: u32| {
        let settings = HexExpeditionSettings {
            tile_size: size,
            ..frozen()
        };
        (
            scene_with(&settings).radius(),
            frame_at(&settings, 80, 30, 3.0),
        )
    };
    let (small_radius, small) = at(10);
    let (large_radius, large) = at(40);
    assert_eq!((small_radius, large_radius), (10.0, 40.0));
    assert_ne!(small.raster.dots, large.raster.dots);
    // Bigger hexes, fewer of them: the camera moves proportionally farther.
    let travel = |size: u32| {
        let scene = scene_with(&HexExpeditionSettings {
            tile_size: size,
            pan_style: PanStyle::East,
            ..HexExpeditionSettings::default()
        });
        scene.camera(60.0).x - scene.camera(0.0).x
    };
    assert!((travel(40) / travel(10) - 4.0).abs() < 1e-6);
}

#[test]
fn settings_normalize_round_trip_and_accept_partial_files() {
    let defaults = HexExpeditionSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let wild = HexExpeditionSettings {
        map_seconds: 0,
        tile_size: 9_999,
        pan_speed: 99_999,
        animation_speed: 99_999,
        landmarks: 99_999,
        brightness: 0,
        seed: 1_000_000,
        ..defaults.clone()
    };
    let clean = wild.normalized();
    assert_eq!(
        (
            clean.map_seconds,
            clean.tile_size,
            clean.pan_speed,
            clean.animation_speed,
            clean.landmarks,
            clean.brightness,
            clean.seed
        ),
        (20, 40, 300, 300, 200, 50, 999)
    );
    let low = HexExpeditionSettings {
        tile_size: 0,
        map_seconds: 99_999,
        brightness: 9_999,
        ..defaults.clone()
    };
    let low = low.normalized();
    assert_eq!(
        (low.tile_size, low.map_seconds, low.brightness),
        (10, 300, 150)
    );
    let settings = HexExpeditionSettings {
        map: MapChoice::Arctic,
        pan_style: PanStyle::NorthEast,
        stepped: false,
        ..defaults
    };
    let text = serde_json::to_string(&settings).expect("serialize");
    assert!(text.contains("\"arctic\"") && text.contains("\"north_east\""));
    let back: HexExpeditionSettings = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back, settings);
    let partial: HexExpeditionSettings =
        serde_json::from_str(r#"{"map": "volcanic"}"#).expect("partial");
    assert_eq!(partial.map, MapChoice::Volcanic);
    assert_eq!(partial.tile_size, 20);
    assert!(serde_json::from_str::<HexExpeditionSettings>("{}").is_ok());
}

fn alternative_value(control: &Control) -> ControlValue {
    match (&control.kind, &control.value) {
        (ControlKind::Slider { min, max, .. }, ControlValue::Number(number)) => {
            ControlValue::Number(if number == min { *max } else { *min })
        }
        (ControlKind::Choice { options }, ControlValue::Index(index)) => {
            ControlValue::Index((index + 1) % options.len())
        }
        (ControlKind::Toggle, ControlValue::Bool(on)) => ControlValue::Bool(!on),
        _ => panic!("unexpected control kind"),
    }
}

#[test]
fn controls_have_sane_shape_and_hide_dependent_rows() {
    let controls = HexExpeditionSettings::default().controls();
    assert!((8..=16).contains(&controls.len()));
    let ids: HashSet<_> = controls.iter().map(|control| control.id).collect();
    assert_eq!(ids.len(), controls.len());
    for control in &controls {
        assert!(!control.help.is_empty() && !control.label.is_empty());
        if let (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) =
            (&control.kind, &control.value)
        {
            assert!((min..=max).contains(&value), "{} out of range", control.id);
        }
    }
    assert!(ids.contains("map_seconds"), "cycle shows its dwell time");
    let fixed_ids: HashSet<_> = fixed(MapChoice::Desert)
        .controls()
        .iter()
        .map(|control| control.id)
        .collect();
    assert!(
        !fixed_ids.contains("map_seconds"),
        "a fixed map has no dwell time"
    );
    assert!(fixed_ids.contains("map") && fixed_ids.contains("seed"));
}

#[test]
fn every_control_round_trips_through_set_control() {
    for settings in [HexExpeditionSettings::default(), fixed(MapChoice::Jungle)] {
        for control in settings.controls() {
            let mut edited = settings.clone();
            let target = alternative_value(&control);
            assert_eq!(
                edited.set_control(control.id, target.clone()),
                Ok(true),
                "{}",
                control.id
            );
            let after = edited
                .controls()
                .into_iter()
                .find(|row| row.id == control.id)
                .expect("row remains");
            assert_eq!(after.value, target, "{}", control.id);
            assert_eq!(edited.set_control(control.id, target), Ok(false));
            assert_eq!(edited.normalized(), edited);
        }
    }
}

#[test]
fn set_control_rejects_unknown_ids_and_bad_values() {
    let mut settings = HexExpeditionSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("tile_size", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("map", ControlValue::Index(99))
        .is_err());
    assert!(settings
        .set_control("pan_style", ControlValue::Index(99))
        .is_err());
    assert!(settings
        .set_control("weather", ControlValue::Number(1))
        .is_err());
    assert_eq!(
        settings.set_control("pan_speed", ControlValue::Number(10_000)),
        Ok(true)
    );
    assert_eq!(settings.pan_speed, 300);
    assert_eq!(
        settings.set_control("tile_size", ControlValue::Number(-5)),
        Ok(true)
    );
    assert_eq!(settings.tile_size, 10);
}

#[test]
fn status_names_the_visible_map_and_the_frame_rate_is_valid() {
    let mut scene = scene_with(&fixed(MapChoice::Arctic));
    let _ = render_frame(&mut scene, 40, 12, Duration::from_secs(2));
    assert_eq!(scene.status().as_deref(), Some("Arctic map"));
    assert!((1..=30).contains(&scene.frames_per_second()));
    let mut cycle = scene_with(&HexExpeditionSettings::default());
    let _ = render_frame(&mut cycle, 40, 12, Duration::from_secs(130));
    assert_eq!(cycle.status().as_deref(), Some("Desert map"));
}

#[test]
fn the_scene_is_registered_with_the_client_facing_catalog() {
    assert!(AmbientKind::ALL.contains(&AmbientKind::HexExpedition));
    assert_eq!(AmbientKind::HexExpedition.label(), "Hex expedition");
    assert!(!AmbientKind::HexExpedition.description().is_empty());
    assert!(AmbientKind::HexExpedition.is_live_only());
    assert!(!AmbientKind::HexExpedition.has_gpu_backend());
    assert!(!AmbientKind::HexExpedition.uses_location());
    let settings = AmbientSettings::default();
    assert!(!settings.controls(AmbientKind::HexExpedition).is_empty());
    let mut edited = settings.clone();
    assert_eq!(
        edited.set_control(
            AmbientKind::HexExpedition,
            "map",
            ControlValue::Index(MapChoice::Desert.index())
        ),
        Ok(true)
    );
    assert_eq!(edited.hex_expedition.map, MapChoice::Desert);
    // A rebuilt scene is requested exactly when its own settings change.
    assert_ne!(
        settings.scene_key(AmbientKind::HexExpedition),
        edited.scene_key(AmbientKind::HexExpedition)
    );
    assert_eq!(
        settings.scene_key(AmbientKind::Pipes),
        edited.scene_key(AmbientKind::Pipes)
    );
    let mut scene = edited.create_scene(
        AmbientKind::HexExpedition,
        &SceneEnv::for_test(std::env::temp_dir()),
    );
    let rendered = render_frame(scene.as_mut(), 40, 12, Duration::from_secs(1));
    assert!(lit(&rendered) > 0);
}

#[test]
fn painter_shapes_cover_clip_and_do_not_panic_off_canvas() {
    let (width, height) = (40usize, 30usize);
    let mut tone = vec![0.0f32; width * height];
    let mut color = vec![[0u8; 3]; width * height];
    let mut canvas = Canvas::new(width, height, &mut tone, &mut color);
    canvas.ellipse((20.0, 15.0), (8.0, 5.0), 0.0, |_, _| (1.0, [10, 20, 30]));
    assert!(canvas.tone_at(20, 15) > 0.99);
    assert_eq!(canvas.tone_at(2, 2), 0.0);
    assert_eq!(canvas.tone_at(width + 5, 1), 0.0);
    // Entirely and partly off canvas, including negative coordinates.
    canvas.ellipse((-50.0, -50.0), (4.0, 4.0), 1.0, |_, _| (1.0, [1, 1, 1]));
    canvas.ellipse((width as f32 + 30.0, 5.0), (4.0, 4.0), 1.0, |_, _| {
        (1.0, [1, 1, 1])
    });
    canvas.triangle((-10.0, -10.0), (60.0, 5.0), (10.0, 80.0), 1.0, |_, _| {
        (0.5, [9, 9, 9])
    });
    canvas.capsule((-5.0, 3.0), (90.0, 3.0), 2.0, 0.5, |_, _| (0.7, [5, 5, 5]));
    canvas.rect((0.0, 0.0), (3.0, 3.0), 1.0, |_, _| (0.9, [7, 7, 7]));
    canvas.blend(9_999, 9_999, 1.0, 1.0, [1, 2, 3]);
    canvas.scale_tone_at(9_999, 9_999, 0.5);
    // Outline paints black around the fill and leaves colours alone.
    let mut tone = vec![0.5f32; width * height];
    let mut color = vec![[200u8; 3]; width * height];
    let mut canvas = Canvas::new(width, height, &mut tone, &mut color);
    canvas.rect((20.0, 15.0), (4.0, 4.0), 1.0, |_, _| (1.0, [1, 2, 3]));
    assert!(canvas.tone_at(20, 15) > 0.99);
    assert!(canvas.tone_at(15, 15) < 0.2, "outline ring is dark");
    assert_eq!(canvas.tone_at(5, 5), 0.5);
}

#[test]
fn distance_functions_have_the_right_sign_and_scale() {
    assert!(sd_ellipse(0.0, 0.0, 5.0, 3.0) < 0.0);
    assert!(sd_ellipse(10.0, 0.0, 5.0, 3.0) > 0.0);
    assert!(sd_ellipse(5.0, 0.0, 5.0, 3.0).abs() < 0.2);
    assert!(sd_ellipse(0.0, 0.0, 0.0, 0.0).is_finite());
    assert!((sd_segment((0.0, 3.0), (-5.0, 0.0), (5.0, 0.0)) - 3.0).abs() < 1e-5);
    assert!((sd_segment((9.0, 0.0), (-5.0, 0.0), (5.0, 0.0)) - 4.0).abs() < 1e-5);
    assert!(sd_segment((1.0, 1.0), (2.0, 2.0), (2.0, 2.0)).is_finite());
    assert!(sd_box(0.0, 0.0, 2.0, 2.0) < 0.0);
    assert!((sd_box(5.0, 0.0, 2.0, 2.0) - 3.0).abs() < 1e-5);
    let triangle = |p| sd_triangle(p, (0.0, 0.0), (10.0, 0.0), (0.0, 10.0));
    assert!(triangle((2.0, 2.0)) < 0.0);
    assert!(triangle((8.0, 8.0)) > 0.0);
    assert!((triangle((-3.0, 0.0)) - 3.0).abs() < 1e-4);
    // Winding order must not matter.
    let reversed = sd_triangle((2.0, 2.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0));
    assert!(reversed < 0.0);
}

#[test]
fn cell_colors_follow_the_lit_dots_and_fall_back_to_the_mean() {
    let (width, height) = (4usize, 8usize);
    let mut tone = vec![0.0f32; width * height];
    let mut color = vec![[0u8; 3]; width * height];
    for x in 0..2 {
        for y in 0..4 {
            color[y * width + x] = [10, 20, 30];
        }
    }
    // Cell (0, 0): one bright dot among dim ones takes the bright colour.
    color[0] = [200, 100, 0];
    tone[0] = 1.0;
    // Cell (1, 0): nothing lit, plain mean of its colours.
    for y in 0..4 {
        for x in 2..4 {
            color[y * width + x] = [40, 40, 40];
        }
    }
    let canvas = Canvas::new(width, height, &mut tone, &mut color);
    let mut cells = vec![[0u8; 3]; 2 * 2];
    canvas.reduce_to_cells(2, 2, &mut cells);
    assert_eq!(cells[0], [200, 100, 0]);
    assert_eq!(cells[1], [40, 40, 40]);
    assert_eq!(cells[2], [0, 0, 0]);
}

#[test]
fn the_hex_window_indexes_every_tile_exactly_once() {
    let window = TileWindow {
        q_min: -3,
        r_min: 5,
        columns: 4,
        rows: 3,
    };
    let mut seen = HashSet::new();
    for r in 5..8 {
        for q in -3..1 {
            let index = window.index(q, r).expect("inside");
            assert!(seen.insert(index));
        }
    }
    assert_eq!(seen.len(), 12);
    assert_eq!(window.index(-4, 5), None);
    assert_eq!(window.index(1, 5), None);
    assert_eq!(window.index(-3, 4), None);
    assert_eq!(window.index(-3, 8), None);
}

#[test]
fn hex_geometry_tiles_the_plane_without_gaps() {
    // Every point of a sample grid belongs to exactly one hex whose centre is
    // within one circumradius, so neighbouring hexes share edges exactly.
    let radius = 20.0f64;
    let column_width = radius * f64::from(SQRT3);
    let row_height = radius * 1.5;
    let mut owners = 0;
    for step_y in 0..60 {
        for step_x in 0..60 {
            let (x, y) = (f64::from(step_x) * 3.7 + 0.3, f64::from(step_y) * 3.1 + 0.2);
            let mut near = 0;
            for r in -2..(y / row_height) as i32 + 3 {
                for q in -3..(x / column_width) as i32 + 4 {
                    let cx = column_width * (f64::from(q) + f64::from(r) * 0.5);
                    let cy = row_height * f64::from(r);
                    let (u, v) = ((x - cx) / radius, (y - cy) / radius);
                    let inset = 0.866_025_4
                        - u.abs()
                            .max((0.5 * u + 0.866_025_4 * v).abs())
                            .max((-0.5 * u + 0.866_025_4 * v).abs());
                    if inset >= 0.0 {
                        near += 1;
                    }
                }
            }
            assert!(near >= 1, "gap at ({x}, {y})");
            owners += near;
        }
    }
    assert!(owners >= 3600);
}

#[test]
fn every_terrain_and_landmark_draws_on_every_map_without_leaving_the_canvas() {
    use super::sprites::{paint_decor, paint_feature, paint_ground, paint_smoke, Smoke};
    let radius = 22.0f32;
    let (width, height) = (96usize, 96usize);
    for kind in MapKind::ALL {
        for terrain in ALL_TERRAIN {
            for time in [0.0f32, 1.7, 55.3] {
                let mut tone = vec![0.0f32; width * height];
                let mut color = vec![[0u8; 3]; width * height];
                let mut canvas = Canvas::new(width, height, &mut tone, &mut color);
                let tile = Tile {
                    terrain,
                    feature: Feature::None,
                    variant: 0x1234_5678,
                };
                let view = sprites::TileView {
                    cx: 48.0,
                    cy: 48.0,
                    radius,
                };
                let mut smoke: Vec<Smoke> = Vec::new();
                paint_ground(&mut canvas, &tile, &view, &[Terrain::Grass; 6], kind, time);
                let ground: f32 = (0..width * height)
                    .map(|i| canvas.tone_at(i % width, i / width))
                    .sum();
                assert!(ground > 20.0, "{kind:?}/{terrain:?} ground is empty");
                paint_decor(&mut canvas, &tile, &view, kind, time, &mut smoke);
                paint_smoke(&mut canvas, &smoke, time);
                let total: f32 = (0..width * height)
                    .map(|i| canvas.tone_at(i % width, i / width))
                    .sum();
                assert!(total.is_finite() && total > 0.0);
                for feature in [
                    Feature::Village,
                    Feature::Temple,
                    Feature::Pyramid,
                    Feature::Camp,
                    Feature::Ruins,
                    Feature::Ship,
                    Feature::Cave,
                    Feature::Shrine,
                    Feature::Mine,
                ] {
                    let mut tone = vec![0.0f32; width * height];
                    let mut color = vec![[0u8; 3]; width * height];
                    let mut canvas = Canvas::new(width, height, &mut tone, &mut color);
                    let tile = Tile {
                        terrain: Terrain::Grass,
                        feature,
                        variant: 0x9e37_79b9,
                    };
                    let mut smoke: Vec<Smoke> = Vec::new();
                    paint_feature(&mut canvas, &tile, &view, kind, time, &mut smoke);
                    paint_smoke(&mut canvas, &smoke, time);
                    let drawn: f32 = (0..width * height)
                        .map(|i| canvas.tone_at(i % width, i / width))
                        .sum();
                    assert!(drawn > 8.0, "{kind:?}/{feature:?} draws nothing");
                }
            }
        }
    }
}

#[test]
fn landmark_sprites_are_tall_enough_to_read_but_stay_near_their_tile() {
    use super::sprites::{paint_feature, Smoke};
    // At the default size a landmark must light a real number of dots and
    // keep within about two hex radii of its centre, so it never spills
    // across several rows of tiles.
    let radius = 20.0f32;
    let (width, height) = (160usize, 160usize);
    for feature in [
        Feature::Village,
        Feature::Temple,
        Feature::Pyramid,
        Feature::Camp,
        Feature::Ruins,
        Feature::Ship,
        Feature::Cave,
        Feature::Shrine,
        Feature::Mine,
    ] {
        let mut tone = vec![0.0f32; width * height];
        let mut color = vec![[0u8; 3]; width * height];
        let mut canvas = Canvas::new(width, height, &mut tone, &mut color);
        let tile = Tile {
            terrain: Terrain::Grass,
            feature,
            variant: 7,
        };
        let view = sprites::TileView {
            cx: 80.0,
            cy: 80.0,
            radius,
        };
        let mut smoke: Vec<Smoke> = Vec::new();
        paint_feature(&mut canvas, &tile, &view, MapKind::Jungle, 1.0, &mut smoke);
        let mut lit_dots = 0;
        for y in 0..height {
            for x in 0..width {
                if canvas.tone_at(x, y) > 0.3 {
                    lit_dots += 1;
                    let (dx, dy) = (x as f32 - 80.0, y as f32 - 80.0);
                    assert!(
                        dx.abs() < radius * 1.6 && dy.abs() < radius * 2.0,
                        "{feature:?} spills to ({dx},{dy})"
                    );
                }
            }
        }
        assert!(lit_dots > 40, "{feature:?} lights only {lit_dots} dots");
    }
}

#[test]
fn the_demo_credits_curious_expedition_with_its_steam_page() {
    assert_eq!(
        INSPIRED_BY,
        ["https://store.steampowered.com/app/358130/The_Curious_Expedition/"]
    );
    assert_eq!(AmbientKind::HexExpedition.inspired_by(), INSPIRED_BY);
}
