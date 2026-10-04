use super::director::Director;
use super::maps::{grid_width_for, Level, GRID_HEIGHT, MAPS};
use super::model::{self, MonsterKind, TowerKind};
use super::settings::{Difficulty, Palette, VectorTdSettings};
use super::sim::{Game, Phase, Rules, Transition, STEP};
use super::VectorTdScene;
use crate::control::{ControlKind, ControlValue, SceneSettings};
use crate::debug::{render_frame, Rendered};
use crate::scene::{Scene, SceneEnv};
use std::time::Duration;

fn scene_with(settings: &VectorTdSettings) -> VectorTdScene {
    VectorTdScene::new(
        settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    )
}

fn frame_at(settings: &VectorTdSettings, cols: u16, rows: u16, time: f64) -> Rendered {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, cols, rows, Duration::from_secs_f64(time))
}

fn rules() -> Rules {
    Rules {
        difficulty: 1.0,
        waves: 20,
    }
}

fn run_game(game: &mut Game, seconds: f32) {
    for _ in 0..(seconds / STEP) as usize {
        game.step();
    }
}

// ---- determinism and rendering ----

#[test]
fn same_time_gives_identical_raster_and_colors() {
    let settings = VectorTdSettings::default();
    let first = frame_at(&settings, 100, 30, 25.0);
    let second = frame_at(&settings, 100, 30, 25.0);
    assert_eq!(first.raster.dots, second.raster.dots);
    assert_eq!(first.cell_colors, second.cell_colors);
}

#[test]
fn frame_cadence_does_not_change_the_game() {
    let settings = VectorTdSettings::default();
    let direct = frame_at(&settings, 100, 30, 12.0);
    let mut stepped_scene = scene_with(&settings);
    let mut stepped = render_frame(&mut stepped_scene, 100, 30, Duration::ZERO);
    for step in 1..=240 {
        stepped = render_frame(
            &mut stepped_scene,
            100,
            30,
            Duration::from_secs_f64(f64::from(step) * 0.05),
        );
    }
    assert_eq!(direct.raster.dots, stepped.raster.dots);
}

/// Render a scene at `times` in order and return the last frame, as the host
/// does: frames arrive often, so the game plays through every second.
fn frame_after(settings: &VectorTdSettings, times: &[f64]) -> Rendered {
    let mut scene = scene_with(settings);
    let mut last = render_frame(&mut scene, 100, 30, Duration::ZERO);
    for &time in times {
        last = render_frame(&mut scene, 100, 30, Duration::from_secs_f64(time));
    }
    last
}

#[test]
fn different_seeds_and_times_differ() {
    let path: Vec<f64> = (1..=16).map(|step| f64::from(step) * 2.5).collect();
    let first = frame_after(&VectorTdSettings::default(), &path);
    let other_seed = VectorTdSettings {
        seed: 500,
        ..VectorTdSettings::default()
    };
    assert_ne!(
        first.raster.dots,
        frame_after(&other_seed, &path).raster.dots
    );
    let later: Vec<f64> = (1..=24).map(|step| f64::from(step) * 2.5).collect();
    assert_ne!(
        first.raster.dots,
        frame_after(&VectorTdSettings::default(), &later)
            .raster
            .dots
    );
}

#[test]
fn black_and_white_has_no_cell_colors_and_colour_has() {
    let mono = VectorTdSettings {
        palette: Palette::BlackWhite,
        ..VectorTdSettings::default()
    };
    assert!(!scene_with(&mono).uses_cell_colors());
    let colour = VectorTdSettings::default();
    let mut scene = scene_with(&colour);
    assert!(scene.uses_cell_colors());
    let rendered = render_frame(&mut scene, 100, 30, Duration::from_secs(30));
    let lit_cell_colors: std::collections::HashSet<[u8; 3]> = rendered
        .cell_colors
        .iter()
        .copied()
        .filter(|color| *color != [0, 0, 0])
        .collect();
    assert!(lit_cell_colors.len() >= 4, "{lit_cell_colors:?}");
    assert!(rendered.lit_dots() > 200);
}

#[test]
fn colour_sliders_change_the_colours_but_black_and_white_ignores_them() {
    let base = frame_at(&VectorTdSettings::default(), 100, 30, 30.0);
    for adjust in [
        VectorTdSettings {
            hue: 120,
            ..VectorTdSettings::default()
        },
        VectorTdSettings {
            saturation: 0,
            ..VectorTdSettings::default()
        },
        VectorTdSettings {
            brightness: 50,
            ..VectorTdSettings::default()
        },
        VectorTdSettings {
            scheme: super::settings::Scheme::Phosphor,
            ..VectorTdSettings::default()
        },
    ] {
        let changed = frame_at(&adjust, 100, 30, 30.0);
        assert_ne!(base.cell_colors, changed.cell_colors, "{adjust:?}");
    }
    let mono = |hue| VectorTdSettings {
        palette: Palette::BlackWhite,
        hue,
        ..VectorTdSettings::default()
    };
    assert_eq!(
        frame_at(&mono(0), 100, 30, 30.0).raster.dots,
        frame_at(&mono(200), 100, 30, 30.0).raster.dots
    );
}

#[test]
fn glow_zero_draws_only_outlines_and_less_light() {
    let lit = |glow| {
        let settings = VectorTdSettings {
            glow,
            ..VectorTdSettings::default()
        };
        let rendered = frame_at(&settings, 100, 30, 60.0);
        rendered.raster.dots.iter().sum::<f32>()
    };
    assert!(lit(0) < lit(100));
}

#[test]
fn tiny_and_odd_sizes_never_panic() {
    let settings = VectorTdSettings::default();
    for (cols, rows) in [(1u16, 1u16), (2, 2), (10, 4), (3, 40), (300, 6), (80, 24)] {
        let mut scene = scene_with(&settings);
        for second in [0.0, 3.0, 20.0, 70.0] {
            let rendered = render_frame(&mut scene, cols, rows, Duration::from_secs_f64(second));
            assert_eq!(rendered.raster.width, usize::from(cols) * 2);
        }
    }
}

#[test]
fn the_screen_changing_shape_rebuilds_the_board() {
    let settings = VectorTdSettings::default();
    let mut scene = scene_with(&settings);
    let _ = render_frame(&mut scene, 100, 30, Duration::from_secs(10));
    let wide = render_frame(&mut scene, 220, 40, Duration::from_secs(11));
    let narrow = render_frame(&mut scene, 60, 40, Duration::from_secs(12));
    assert!(wide.lit_dots() > 0 && narrow.lit_dots() > 0);
}

#[test]
fn every_frame_overwrites_the_whole_color_buffer() {
    let mut scene = scene_with(&VectorTdSettings::default());
    let mut raster = crate::raster::Raster::default();
    raster.resize(100, 60);
    let mut colors = vec![[7, 7, 7]; 50 * 15];
    let mut frame = crate::scene::Frame {
        raster: &mut raster,
        cell_colors: &mut colors,
        width: 50,
        height: 15,
        time: Duration::from_secs(2),
        wall: Duration::from_secs(2),
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    scene.render(&mut frame);
    let untouched = colors.iter().filter(|color| **color == [7, 7, 7]).count();
    assert_eq!(untouched, 0);
}

// ---- maps ----

#[test]
fn every_map_has_axis_aligned_paths_and_room_to_build_at_every_width() {
    for definition in &MAPS {
        for path in definition.paths {
            for pair in path.windows(2) {
                assert!(
                    pair[0].0 == pair[1].0 || pair[0].1 == pair[1].1,
                    "{} has a diagonal segment",
                    definition.name
                );
            }
        }
    }
    for index in 0..MAPS.len() {
        for width in [34, 44, 60, 96] {
            let level = Level::build(index, width);
            assert!(level.buildable.len() > 120, "{} at {width}", level.name);
            assert!(!level.samples.is_empty());
            for &cell in &level.buildable {
                let index = level.cell_index(cell).unwrap();
                assert!(!level.blocked[index]);
                for path in &level.paths {
                    for pair in path.windows(2) {
                        let distance = super::maps::segment_distance(
                            (cell.0 as f32, cell.1 as f32),
                            pair[0],
                            pair[1],
                        );
                        assert!(distance >= 0.95, "{} {cell:?}", level.name);
                    }
                }
            }
            // Every tower kind finds somewhere with real coverage.
            for kind in TowerKind::ALL {
                let best = level.coverage[kind.index()]
                    .iter()
                    .copied()
                    .fold(0.0f32, f32::max);
                assert!(best > 6.0, "{} {kind:?} at {width}: {best}", level.name);
            }
        }
    }
}

#[test]
fn grid_width_follows_the_screen_aspect_within_limits() {
    assert_eq!(grid_width_for(400, 200), (GRID_HEIGHT as f32 * 2.0) as i32);
    assert_eq!(grid_width_for(10, 400), 34);
    assert_eq!(grid_width_for(4000, 100), 96);
}

#[test]
fn position_along_a_path_is_continuous_and_ends_at_the_exit() {
    let level = Level::build(3, 48);
    for path in 0..level.paths.len() {
        let length = level.path_length(path);
        let mut previous = level.position(path, 0.0).0;
        let mut distance = 0.0;
        while distance < length {
            distance += 0.25;
            let now = level.position(path, distance).0;
            assert!((now.0 - previous.0).hypot(now.1 - previous.1) < 0.3);
            previous = now;
        }
        let end = level.position(path, length + 5.0).0;
        assert_eq!(end, *level.paths[path].last().unwrap());
    }
}

// ---- rules ----

fn quiet_game() -> Game {
    let mut game = Game::new(0, 44, 0, 0, rules(), 1);
    game.towers.clear();
    game
}

#[test]
fn building_costs_money_only_on_free_buildable_cells() {
    let mut game = quiet_game();
    let cell = game.level.buildable[10];
    let before = game.money;
    assert!(game.build(TowerKind::Pulse, cell));
    assert_eq!(game.money, before - TowerKind::Pulse.stats().cost);
    assert!(!game.build(TowerKind::Pulse, cell), "occupied");
    let blocked = (0, 0);
    assert!(!game.build(TowerKind::Pulse, blocked), "off the playfield");
    let on_path = (
        game.level.paths[0][1].0 as i32,
        game.level.paths[0][1].1 as i32,
    );
    assert!(!game.build(TowerKind::Pulse, on_path), "on the path");
    game.money = 1.0;
    assert!(
        !game.build(TowerKind::Needle, game.level.buildable[40]),
        "too poor"
    );
}

#[test]
fn locked_towers_cannot_be_built_until_their_stage() {
    let mut game = quiet_game();
    game.money = 10_000.0;
    let cell = game.level.buildable[10];
    assert!(!game.build(TowerKind::Hive, cell));
    let mut later = Game::new(0, 44, 4, 0, rules(), 1);
    later.towers.clear();
    later.money = 10_000.0;
    assert!(later.build(TowerKind::Hive, cell));
}

#[test]
fn upgrades_cost_money_respect_the_tech_cap_and_sell_refunds() {
    let mut game = quiet_game();
    game.money = 10_000.0;
    let cell = game.level.buildable[10];
    assert!(game.build(TowerKind::Needle, cell));
    let cap = game.max_tower_level();
    for _ in 1..cap {
        assert!(game.upgrade(0));
    }
    assert!(!game.upgrade(0), "capped by the level's tech");
    assert_eq!(game.towers[0].level, cap);
    let before = game.money;
    assert!(game.sell(0));
    assert!(game.money > before);
    assert!(game.towers.is_empty());
}

#[test]
fn a_tower_kills_a_monster_in_range_and_pays_the_bounty() {
    let mut game = quiet_game();
    game.phase = Phase::Running;
    game.money = 10_000.0;
    let (x, y) = game.level.position(0, 6.0).0;
    let cell = game
        .level
        .buildable
        .iter()
        .copied()
        .find(|&(cx, cy)| (cx as f32 - x).hypot(cy as f32 - y) < 2.5)
        .unwrap();
    assert!(game.build(TowerKind::Needle, cell));
    game.wave_timer = 0.0;
    let money = game.money;
    for _ in 0..(12.0 / STEP) as usize {
        game.ai_timer = 1e9;
        game.step();
    }
    assert!(game.kills > 0);
    assert!(game.money > money);
    assert_eq!(game.lives, model::STARTING_LIVES, "nothing leaked");
}

#[test]
fn leaking_monsters_cost_lives_and_zero_lives_is_a_defeat_then_a_retry() {
    let mut game = Game::new(0, 44, 0, 0, rules(), 1);
    game.towers.clear();
    game.phase = Phase::Running;
    game.money = 0.0;
    game.ai_timer = 1e9;
    game.wave_timer = 0.0;
    let mut transition = None;
    for _ in 0..(600.0 / STEP) as usize {
        game.ai_timer = 1e9;
        transition = game.step();
        if transition.is_some() {
            break;
        }
    }
    assert_eq!(transition, Some(Transition::Retry));
    assert!(game.leaks > 0);
}

#[test]
fn chill_slows_monsters_and_bosses_resist_it() {
    let mut game = quiet_game();
    game.phase = Phase::Running;
    game.money = 10_000.0;
    let (x, y) = game.level.position(0, 6.0).0;
    let cell = game
        .level
        .buildable
        .iter()
        .copied()
        .find(|&(cx, cy)| (cx as f32 - x).hypot(cy as f32 - y) < 2.0)
        .unwrap();
    assert!(game.build(TowerKind::Chill, cell));
    game.wave_timer = 0.0;
    let mut slowed = false;
    for _ in 0..(10.0 / STEP) as usize {
        game.ai_timer = 1e9;
        game.step();
        slowed |= game
            .monsters
            .iter()
            .any(|monster| monster.slow_factor < 1.0);
    }
    assert!(slowed);
    let (factor, _) = TowerKind::Chill.slow(1).unwrap();
    assert!(1.0 - (1.0 - factor) * MonsterKind::Boss.slow_effect() > factor);
}

#[test]
fn splitters_leave_shards_and_waves_follow_the_ten_wave_cycle() {
    let kinds: Vec<MonsterKind> = (1..=10)
        .map(|wave| model::wave_groups(wave)[0].kind)
        .collect();
    assert_eq!(kinds[2], MonsterKind::Swarm);
    assert_eq!(kinds[5], MonsterKind::Splitter);
    assert_eq!(kinds[6], MonsterKind::Wisp);
    assert_eq!(kinds[9], MonsterKind::Boss);
    assert_eq!(model::wave_groups(20)[0].kind, MonsterKind::Boss);
    let mut game = quiet_game();
    game.phase = Phase::Running;
    game.ai_timer = 1e9;
    game.wave = 5;
    game.wave_timer = 0.0;
    let mut saw_shard = false;
    let (x, y) = game.level.position(0, 6.0).0;
    let cell = game
        .level
        .buildable
        .iter()
        .copied()
        .find(|&(cx, cy)| (cx as f32 - x).hypot(cy as f32 - y) < 2.0)
        .unwrap();
    game.money = 10_000.0;
    assert!(game.build(TowerKind::Needle, cell));
    for _ in 0..(40.0 / STEP) as usize {
        game.ai_timer = 1e9;
        game.step();
        saw_shard |= game
            .monsters
            .iter()
            .any(|monster| monster.kind == MonsterKind::Shard);
    }
    assert!(saw_shard);
}

#[test]
fn sending_a_wave_early_pays_the_remaining_timer() {
    let mut game = quiet_game();
    game.phase = Phase::Running;
    game.since_wave = 10.0;
    game.wave_timer = 10.0;
    let money = game.money;
    assert!(game.send_wave_early());
    assert!(game.money >= money + 8.0);
    assert_eq!(game.wave, 1);
    assert!(!game.send_wave_early(), "not twice in a row");
}

#[test]
fn waves_get_harder_and_levels_pay_more() {
    assert!(model::hp_scale(10, 0) > model::hp_scale(2, 0));
    assert!(model::hp_scale(3, 2) > model::hp_scale(3, 0));
    assert!(model::bounty(MonsterKind::Drone, 10, 0) > model::bounty(MonsterKind::Drone, 1, 0));
    assert!(model::max_level(5) > model::max_level(0));
    assert!(model::starting_money(3) > model::starting_money(0));
    assert!(model::retry_relief(3) < model::retry_relief(0));
    for kind in TowerKind::ALL {
        let stats = kind.stats();
        assert!(stats.cost > 0.0 && stats.range > 0.0);
        assert!(model::upgrade_cost(kind, 2) > model::upgrade_cost(kind, 1));
    }
}

// ---- the AI ----

#[test]
fn the_ai_only_builds_unlocked_towers_on_free_buildable_cells() {
    for (map, stage) in [(0usize, 0u32), (3, 2), (5, 5)] {
        let mut game = Game::new(map, 44, stage, 0, rules(), 3);
        run_game(&mut game, 150.0);
        assert!(!game.towers.is_empty(), "map {map}");
        let mut seen = std::collections::HashSet::new();
        for tower in &game.towers {
            assert!(
                game.level.is_buildable(tower.cell),
                "on the path or off board"
            );
            assert!(seen.insert(tower.cell), "two towers on one cell");
            assert!(
                tower.kind.unlock_stage() <= stage,
                "{:?} at stage {stage}",
                tower.kind
            );
            assert!(tower.level <= game.max_tower_level());
        }
    }
}

#[test]
fn the_ai_spends_its_starting_money_and_varies_its_towers() {
    let mut game = Game::new(1, 44, 3, 0, rules(), 5);
    run_game(&mut game, 400.0);
    let kinds: std::collections::HashSet<TowerKind> =
        game.towers.iter().map(|tower| tower.kind).collect();
    assert!(kinds.len() >= 3, "{kinds:?}");
    assert!(game.towers.iter().any(|tower| tower.level > 1), "upgrades");
}

#[test]
fn the_ai_clears_an_easy_level_and_the_director_moves_on() {
    let settings = VectorTdSettings {
        map: 0,
        waves: 6,
        difficulty: Difficulty::Easy,
        ..VectorTdSettings::default()
    };
    let mut director = Director::new(&settings, 44);
    let first_map = director.game.level.name;
    for second in 0..400 {
        director.advance_to(second as f32);
        if director.stage > 0 {
            break;
        }
    }
    assert_eq!(director.stage, 1, "level cleared");
    assert_ne!(director.game.level.name, first_map, "next map");
    assert_eq!(director.defeats, 0);
}

#[test]
fn a_fixed_map_stays_put_across_levels() {
    let settings = VectorTdSettings {
        map: 3,
        waves: 5,
        difficulty: Difficulty::Easy,
        ..VectorTdSettings::default()
    };
    let mut director = Director::new(&settings, 44);
    for second in 0..400 {
        director.advance_to(second as f32);
        if director.stage > 0 {
            break;
        }
    }
    assert!(director.stage > 0);
    assert_eq!(director.game.level.name, "COMB");
}

#[test]
fn long_gaps_are_skipped_not_replayed() {
    let mut director = Director::new(&VectorTdSettings::default(), 44);
    director.advance_to(100_000.0);
    assert!(director.sim_time() >= 99_999.0);
}

// ---- settings ----

#[test]
fn settings_normalize_untrusted_ranges() {
    let wild = VectorTdSettings {
        map: 99,
        start_level: 0,
        waves: 1000,
        game_speed: 0,
        brightness: 9999,
        contrast: 0,
        hue: 9999,
        saturation: 9999,
        glow: 9999,
        seed: 123_456,
        ..VectorTdSettings::default()
    };
    let tame = wild.normalized();
    assert_eq!(tame.map, 6);
    assert_eq!(tame.start_level, 1);
    assert_eq!(tame.waves, 40);
    assert_eq!(tame.game_speed, 25);
    assert_eq!(tame.brightness, 200);
    assert_eq!(tame.contrast, 50);
    assert_eq!(tame.hue, 360);
    assert_eq!(tame.saturation, 200);
    assert_eq!(tame.glow, 100);
    assert_eq!(tame.seed, 999);
    let from_empty: VectorTdSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(from_empty, VectorTdSettings::default());
}

#[test]
fn every_row_edits_its_field_and_colour_rows_hide_in_black_and_white() {
    let mut settings = VectorTdSettings::default();
    let rows = settings.controls();
    let ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
    let unique: std::collections::HashSet<&&str> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
    for expected in [
        "scheme",
        "hue",
        "saturation",
        "brightness",
        "contrast",
        "glow",
    ] {
        assert!(ids.contains(&expected), "{expected}");
    }
    for row in &rows {
        let next = match &row.kind {
            ControlKind::Slider { min, max, .. } => {
                ControlValue::Number(if row.value == ControlValue::Number(*max) {
                    *min
                } else {
                    *max
                })
            }
            ControlKind::Choice { options } => ControlValue::Index(
                (match row.value {
                    ControlValue::Index(index) => index,
                    _ => 0,
                } + 1)
                    % options.len(),
            ),
            ControlKind::Toggle => ControlValue::Bool(row.value != ControlValue::Bool(true)),
            ControlKind::Text { .. } => continue,
        };
        assert_eq!(settings.set_control(row.id, next), Ok(true), "{}", row.id);
    }
    assert_ne!(settings, VectorTdSettings::default());
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("hue", ControlValue::Bool(true))
        .is_err());
    let mono = VectorTdSettings {
        palette: Palette::BlackWhite,
        ..VectorTdSettings::default()
    };
    let mono_ids: Vec<&str> = mono.controls().iter().map(|row| row.id).collect();
    for hidden in ["scheme", "hue", "saturation"] {
        assert!(
            !mono_ids.contains(&hidden),
            "{hidden} hidden in black and white"
        );
    }
    assert!(mono_ids.contains(&"brightness") && mono_ids.contains(&"contrast"));
}

#[test]
fn the_inspiration_is_credited() {
    assert_eq!(
        super::INSPIRED_BY,
        &["https://www.crazygames.com/game/vector-td"]
    );
}

// ---- diagnostics (ignored): balance, traces, frame dumps, timing ----

#[test]
#[ignore]
fn balance_report() {
    for seed in 0..1u32 {
        for map in 1..=6 {
            let settings = VectorTdSettings {
                map,
                seed,
                start_level: 1,
                ..VectorTdSettings::default()
            };
            let mut director = Director::new(&settings, 42);
            let mut log = Vec::new();
            let mut last = (director.stage, director.defeats);
            for second in 0..1500 {
                director.advance_to(second as f32);
                let now = (director.stage, director.defeats);
                if now != last {
                    log.push(format!("t={second} stage {} defeats {}", now.0, now.1));
                    last = now;
                }
            }
            println!(
                "seed {seed} map {map}: {} | towers {} money {:.0} wave {} lives {}",
                log.join("; "),
                director.game.towers.len(),
                director.game.money,
                director.game.wave,
                director.game.lives
            );
        }
    }
}

#[test]
#[ignore]
fn trace_level() {
    let settings = VectorTdSettings {
        map: 1,
        ..VectorTdSettings::default()
    };
    let mut director = Director::new(&settings, 42);
    let mut last_wave = 0;
    let mut last_signature = String::new();
    for second in 0..400 {
        director.advance_to(second as f32);
        let game = &director.game;
        {
            let signature: Vec<String> = game
                .towers
                .iter()
                .map(|t| format!("{}{}@{},{}", t.kind.label(), t.level, t.cell.0, t.cell.1))
                .collect();
            let joined = signature.join(" ");
            if joined != last_signature {
                println!(
                    "  buy t={second} money {:.0}: {}",
                    game.money,
                    joined.replace(&last_signature, "").trim()
                );
                last_signature = joined;
            }
        }
        if game.wave != last_wave || second % 20 == 0 {
            last_wave = game.wave;
            let mut kinds = std::collections::BTreeMap::new();
            for tower in &game.towers {
                *kinds
                    .entry(format!("{}{}", tower.kind.label(), tower.level))
                    .or_insert(0) += 1;
            }
            println!(
                "t={second} wave {} money {:.0} lives {} mon {} threat {:.0} towers {:?}",
                game.wave,
                game.money,
                game.lives,
                game.monsters.len(),
                game.threat_hp(),
                kinds
            );
        }
    }
}

/// Writes thresholded frames as PPM for visual checks:
/// `VECTOR_TD_DUMP=/dir cargo test ... dump_frames -- --ignored`.
#[test]
#[ignore]
fn dump_frames() {
    use super::VectorTdScene;
    use crate::debug::render_frame;
    use crate::raster::{threshold, DitherMode};
    use crate::scene::SceneEnv;
    let Ok(dir) = std::env::var("VECTOR_TD_DUMP") else {
        return;
    };
    let map: u32 = std::env::var("VECTOR_TD_MAP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mono = std::env::var("VECTOR_TD_MONO").is_ok();
    let times: Vec<f32> = std::env::var("VECTOR_TD_TIMES")
        .unwrap_or_else(|_| "6,40,90,200".into())
        .split(',')
        .filter_map(|v| v.parse().ok())
        .collect();
    let start_level: u32 = std::env::var("VECTOR_TD_LEVEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let settings = VectorTdSettings {
        map,
        start_level,
        palette: if mono {
            super::settings::Palette::BlackWhite
        } else {
            super::settings::Palette::Colour
        },
        ..VectorTdSettings::default()
    };
    let mut scene = VectorTdScene::new(
        &settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    );
    let (cols, rows) = (200u16, 56u16);
    for time in times {
        // advance in small increments so the sim is stepped smoothly
        let mut t = 0.0;
        while t < time {
            let _ = render_frame(
                &mut scene,
                cols,
                rows,
                std::time::Duration::from_secs_f32(t),
            );
            t += 2.0;
        }
        let rendered = render_frame(
            &mut scene,
            cols,
            rows,
            std::time::Duration::from_secs_f32(time),
        );
        let (w, h) = (rendered.raster.width, rendered.raster.height);
        let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
        for y in 0..h {
            for x in 0..w {
                let v = rendered.raster.dots[y * w + x];
                let lit = v > threshold(x, y, DitherMode::Ordered);
                let c = rendered.cell_colors[(y / 4) * (cols as usize) + x / 2];
                let px = if lit {
                    if mono {
                        [235, 235, 235]
                    } else {
                        c
                    }
                } else {
                    [0, 0, 0]
                };
                out.extend_from_slice(&px);
            }
        }
        std::fs::write(format!("{dir}/frame_{map}_{time}.ppm"), out).unwrap();
    }
}

/// Wall-clock cost of frames over a long game; run in release.
#[test]
#[ignore]
fn bench_frames() {
    use super::VectorTdScene;
    use crate::debug::render_frame;
    use crate::scene::SceneEnv;
    let settings = VectorTdSettings {
        map: 0,
        game_speed: 400,
        ..VectorTdSettings::default()
    };
    let mut scene = VectorTdScene::new(
        &settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    );
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    let frames = 20 * 60 * 20;
    for frame in 0..frames {
        let started = std::time::Instant::now();
        let _ = render_frame(
            &mut scene,
            200,
            50,
            std::time::Duration::from_secs_f64(frame as f64 / 20.0),
        );
        let elapsed = started.elapsed().as_secs_f64();
        worst = worst.max(elapsed);
        total += elapsed;
    }
    println!(
        "frames {frames} mean {:.2} ms worst {:.2} ms",
        total / frames as f64 * 1000.0,
        worst * 1000.0
    );
}

#[test]
fn seed_picks_the_first_map_of_the_cycle() {
    let name = |seed| {
        let settings = VectorTdSettings {
            seed,
            ..VectorTdSettings::default()
        };
        Director::new(&settings, 44).game.level.name
    };
    assert_eq!(name(0), "SWITCHBACK");
    assert_eq!(name(2), "COMB");
    assert_ne!(name(0), name(1));
}

#[test]
#[ignore]
fn long_progression() {
    for seed in [0u32, 1, 2] {
        let settings = VectorTdSettings {
            map: 0,
            seed,
            ..VectorTdSettings::default()
        };
        let mut director = Director::new(&settings, 44);
        let mut last = (director.stage, director.defeats);
        let mut log = Vec::new();
        for second in 0..9000 {
            director.advance_to(second as f32);
            let now = (director.stage, director.defeats);
            if now != last {
                log.push(format!("{second}:s{}d{}", now.0, now.1));
                last = now;
            }
        }
        println!("seed {seed}: {}", log.join(" "));
    }
}

#[test]
fn visual_edits_keep_the_game_playing_and_gameplay_edits_rebuild_it() {
    use crate::registry::AmbientSettings;
    let mut ambient = AmbientSettings::default();
    let mut scene = scene_with(&ambient.vector_td);
    let path: Vec<f64> = (1..=20).map(|step| f64::from(step) * 2.5).collect();
    let mut before = render_frame(&mut scene, 100, 30, Duration::ZERO);
    for &time in &path {
        before = render_frame(&mut scene, 100, 30, Duration::from_secs_f64(time));
    }
    // A colour change is applied in place and the board is unchanged.
    ambient.vector_td.hue = 120;
    assert!(scene.reconfigure(&ambient));
    let recoloured = render_frame(&mut scene, 100, 30, Duration::from_secs_f64(50.0));
    assert_eq!(before.raster.dots.len(), recoloured.raster.dots.len());
    assert_ne!(before.cell_colors, recoloured.cell_colors);
    // Switching to black and white in place stops supplying cell colours.
    ambient.vector_td.palette = Palette::BlackWhite;
    assert!(scene.reconfigure(&ambient));
    assert!(!scene.uses_cell_colors());
    // Changing what is played asks for a rebuild.
    ambient.vector_td.difficulty = Difficulty::Hard;
    assert!(!scene.reconfigure(&ambient));
    let mut other_map = AmbientSettings::default();
    other_map.vector_td.map = 4;
    assert!(!scene.reconfigure(&other_map));
}

#[test]
fn game_speed_changes_never_jump_the_game() {
    let mut scene = scene_with(&VectorTdSettings::default());
    let frame = |scene: &mut VectorTdScene, seconds: f64| {
        render_frame(scene, 100, 30, Duration::from_secs_f64(seconds))
    };
    for step in 0..40 {
        frame(&mut scene, f64::from(step) * 0.5);
    }
    let ambient = crate::registry::AmbientSettings {
        vector_td: VectorTdSettings {
            game_speed: 400,
            ..VectorTdSettings::default()
        },
        ..Default::default()
    };
    assert!(scene.reconfigure(&ambient));
    let game_time = |scene: &VectorTdScene| scene.director.as_ref().map(|d| d.game.time).unwrap();
    let before = game_time(&scene);
    frame(&mut scene, 20.0);
    let after = game_time(&scene);
    assert!(
        (after - before - 2.0).abs() < 0.2,
        "0.5 s at 400% is 2 game seconds, got {}",
        after - before
    );
}

#[test]
fn time_moving_backwards_pauses_instead_of_restarting() {
    let settings = VectorTdSettings::default();
    let mut scene = scene_with(&settings);
    let _ = render_frame(&mut scene, 100, 30, Duration::ZERO);
    for step in 1..=30 {
        let _ = render_frame(
            &mut scene,
            100,
            30,
            Duration::from_secs_f64(f64::from(step)),
        );
    }
    let played = scene.director.as_ref().unwrap().game.time;
    let _ = render_frame(&mut scene, 100, 30, Duration::from_secs(5));
    assert!(scene.director.as_ref().unwrap().game.time >= played);
}

#[test]
fn provided_palette_changes_cell_colors_and_none_keeps_them() {
    let settings = VectorTdSettings::default();
    let plain = frame_at(&settings, 50, 15, 20.0);
    let mut env = SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources());
    env.palette = crate::style::ScenePalette {
        stops: vec![[10, 40, 200], [240, 230, 60]],
        reverse: false,
        shift_percent: 0,
    };
    let mut scene = VectorTdScene::new(&settings, &env);
    let tinted = render_frame(&mut scene, 50, 15, Duration::from_secs_f64(20.0));
    assert_ne!(plain.cell_colors, tinted.cell_colors);
    scene.set_palette(&crate::style::ScenePalette::default());
    let reset = render_frame(&mut scene, 50, 15, Duration::from_secs_f64(20.0));
    assert_eq!(plain.cell_colors, reset.cell_colors);
    assert!(scene.follows_palette());
}
