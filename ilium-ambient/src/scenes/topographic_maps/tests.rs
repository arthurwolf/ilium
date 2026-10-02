use super::data::{self, Heightfield};
use super::settings::{BodyChoice, PanStyle, Projection, TopographicMapsSettings, WorldId};
use super::*;
use crate::control::{Control, ControlValue, SceneSettings};
use crate::raster::Raster;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn body_for(world: WorldId) -> BodyChoice {
    BodyChoice::ALL[3 + WorldId::REAL
        .iter()
        .chain(WorldId::FICTIONAL.iter())
        .position(|candidate| *candidate == world)
        .expect("known world")]
}

fn settings_for(world: WorldId) -> TopographicMapsSettings {
    TopographicMapsSettings {
        body: body_for(world),
        ..TopographicMapsSettings::default()
    }
}

fn render(
    scene: &mut TopographicMapsScene,
    seconds: f64,
    columns: u16,
    rows: u16,
) -> (Raster, Vec<[u8; 3]>) {
    let mut raster = Raster::default();
    raster.resize(usize::from(columns) * 2, usize::from(rows) * 4);
    let mut cell_colors = Vec::new();
    let mut frame = Frame {
        raster: &mut raster,
        cell_colors: &mut cell_colors,
        width: columns,
        height: rows,
        time: Duration::from_secs_f64(seconds),
        wall: Duration::from_secs_f64(seconds),
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    scene.render(&mut frame);
    (raster, cell_colors)
}

/// Render until the background worker delivered the first world.
fn render_loaded(
    scene: &mut TopographicMapsScene,
    seconds: f64,
    columns: u16,
    rows: u16,
) -> (Raster, Vec<[u8; 3]>) {
    for _ in 0..500 {
        let frame = render(scene, seconds, columns, rows);
        if scene.current.is_some() {
            return frame;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("world never loaded: {:?}", scene.status());
}

fn lit(raster: &Raster) -> usize {
    raster.dots.iter().filter(|dot| **dot > 0.5).count()
}

fn scene(settings: &TopographicMapsSettings) -> TopographicMapsScene {
    TopographicMapsScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

#[test]
fn every_real_world_decodes_with_plausible_relief() {
    for world in WorldId::REAL {
        let field = data::load_real(world).expect("embedded elevation decodes");
        assert_eq!(field.meters.len(), field.width * field.height, "{world:?}");
        assert!(
            field.width >= 1440 && field.height * 2 == field.width,
            "{world:?}"
        );
        assert!(field.min_m < 0.0 && field.max_m > 2000.0, "{world:?}");
        assert!(
            field.meters.iter().all(|value| value.is_finite()),
            "{world:?}"
        );
    }
}

#[test]
fn earth_samples_match_known_geography() {
    let earth = data::load_real(WorldId::Earth).expect("earth");
    // Tibetan Plateau is high ground; the mid-Pacific is deep sea.
    assert!(earth.sample(88.0, 31.0) > 3500.0);
    assert!(earth.sample(-150.0, 0.0) < -3000.0);
    // Longitude wraps across the antimeridian.
    assert!((earth.sample(180.0, 10.0) - earth.sample(-180.0, 10.0)).abs() < 1.0);
}

#[test]
fn mars_has_its_big_volcano_and_deep_basin() {
    let mars = data::load_real(WorldId::Mars).expect("mars");
    // Olympus Mons near 226E, 18N; Hellas Planitia near 70E, -42.
    assert!(mars.sample(-134.0, 18.0) > 12_000.0);
    assert!(mars.sample(70.0, -42.0) < -4000.0);
}

#[test]
fn fictional_worlds_are_deterministic_and_seed_dependent() {
    let never = AtomicBool::new(false);
    for world in WorldId::FICTIONAL {
        let one = data::generate_fictional(world, 7, &never).expect("generated");
        let again = data::generate_fictional(world, 7, &never).expect("generated");
        let other = data::generate_fictional(world, 8, &never).expect("generated");
        assert_eq!(one.meters, again.meters, "{world:?}");
        assert_ne!(one.meters, other.meters, "{world:?}");
        assert!(
            one.min_m < 0.0 && one.max_m > 0.0,
            "{world:?} has land and sea"
        );
    }
}

#[test]
fn generation_stops_when_asked() {
    let stop = AtomicBool::new(true);
    assert!(data::generate_fictional(WorldId::Craterlands, 1, &stop).is_none());
}

#[test]
fn heightfield_sample_is_bilinear_and_clamped() {
    let field = Heightfield {
        width: 2,
        height: 2,
        meters: vec![0.0, 100.0, 0.0, 100.0],
        min_m: 0.0,
        max_m: 100.0,
        name: String::new(),
    };
    assert!((field.sample(0.0, 0.0) - 50.0).abs() < 1.0);
    assert!(field.sample(0.0, 90.0).is_finite());
    assert!(field.sample(0.0, -90.0).is_finite());
}

#[test]
fn nice_intervals_are_survey_steps() {
    assert_eq!(nice_interval(480.0), 500.0);
    assert_eq!(nice_interval(1900.0), 2000.0);
    assert_eq!(nice_interval(1.0), 5.0);
}

#[test]
fn flat_and_globe_both_draw_contours() {
    for projection in [Projection::Flat, Projection::Globe] {
        let mut scene = scene(&TopographicMapsSettings {
            projection,
            ..settings_for(WorldId::Earth)
        });
        let (raster, _) = render_loaded(&mut scene, 0.0, 100, 40);
        assert!(lit(&raster) > 300, "{projection:?} drew {}", lit(&raster));
    }
}

#[test]
fn contour_count_follows_the_setting() {
    let few = TopographicMapsSettings {
        contour_levels: 6,
        shading_percent: 0,
        ..settings_for(WorldId::Moon)
    };
    let many = TopographicMapsSettings {
        contour_levels: 60,
        ..few.clone()
    };
    let drawn = |settings: &TopographicMapsSettings| {
        lit(&render_loaded(&mut scene(settings), 0.0, 120, 40).0)
    };
    assert!(drawn(&many) > drawn(&few));
}

#[test]
fn fixed_spacing_overrides_level_count() {
    let mut scene = scene(&TopographicMapsSettings {
        interval_m: 1000,
        ..settings_for(WorldId::Mars)
    });
    render_loaded(&mut scene, 0.0, 80, 30);
    assert_eq!(
        scene.current.as_ref().map(|loaded| loaded.interval_m),
        Some(1000.0)
    );
}

#[test]
fn hidden_below_zero_lines_draw_less_than_solid() {
    let base = TopographicMapsSettings {
        shading_percent: 0,
        ..settings_for(WorldId::Earth)
    };
    let drawn = |style| {
        lit(&render_loaded(
            &mut scene(&TopographicMapsSettings {
                below_style: style,
                ..base.clone()
            }),
            0.0,
            120,
            40,
        )
        .0)
    };
    assert!(drawn(BelowStyle::Hidden) < drawn(BelowStyle::Dotted));
    assert!(drawn(BelowStyle::Dotted) < drawn(BelowStyle::Solid));
}

#[test]
fn panning_moves_the_view_and_still_does_not() {
    let moving = TopographicMapsSettings {
        pan: PanStyle::East,
        pan_speed: 400,
        ..settings_for(WorldId::Earth)
    };
    let still = TopographicMapsSettings {
        pan: PanStyle::Still,
        ..moving.clone()
    };
    let frames = |settings: &TopographicMapsSettings| {
        let mut scene = scene(settings);
        (
            render_loaded(&mut scene, 0.0, 100, 40).0.dots,
            render(&mut scene, 5.0, 100, 40).0.dots,
        )
    };
    let (a, b) = frames(&moving);
    assert_ne!(a, b);
    let (a, b) = frames(&still);
    assert_eq!(a, b);
}

#[test]
fn rendering_is_deterministic() {
    let settings = settings_for(WorldId::Venus);
    let one = render_loaded(&mut scene(&settings), 33.0, 90, 30).0.dots;
    let two = render_loaded(&mut scene(&settings), 33.0, 90, 30).0.dots;
    assert_eq!(one, two);
}

#[test]
fn colours_are_only_supplied_when_a_palette_is_chosen() {
    let tinted = settings_for(WorldId::Mars);
    let mut scene_tinted = scene(&tinted);
    assert!(scene_tinted.uses_cell_colors());
    let (_, colors) = render_loaded(&mut scene_tinted, 0.0, 60, 20);
    assert_eq!(colors.len(), 60 * 20);
    assert!(colors.windows(2).any(|pair| pair[0] != pair[1]));
    let plain = TopographicMapsSettings {
        palette: PaletteChoice::Global,
        ..tinted
    };
    assert!(!scene(&plain).uses_cell_colors());
}

#[test]
fn tides_move_the_zero_level() {
    let scene = scene(&TopographicMapsSettings {
        tide_range_m: 1000,
        tide_seconds: 100,
        ..settings_for(WorldId::Earth)
    });
    assert!(scene.zero_level(0.0).abs() < 1e-3);
    assert!((scene.zero_level(25.0) - 500.0).abs() < 1.0);
    assert!((scene.zero_level(75.0) + 500.0).abs() < 1.0);
}

#[test]
fn cycle_switches_world_without_blocking_and_dissolves() {
    let mut scene = scene(&TopographicMapsSettings {
        body_seconds: 20,
        ..TopographicMapsSettings::default()
    });
    render_loaded(&mut scene, 0.0, 60, 20);
    assert_eq!(
        scene.current.as_ref().map(|loaded| loaded.id),
        Some(WorldId::Earth)
    );
    render(&mut scene, 21.0, 60, 20);
    // The worker may still be running; wait for it, then draw again.
    for _ in 0..200 {
        if scene
            .current
            .as_ref()
            .is_some_and(|loaded| loaded.id == WorldId::Moon)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
        render(&mut scene, 21.5, 60, 20);
    }
    assert_eq!(
        scene.current.as_ref().map(|loaded| loaded.id),
        Some(WorldId::Moon)
    );
    assert!(scene.previous.is_some());
    render(&mut scene, 27.0, 60, 20);
    assert!(scene.previous.is_none());
}

#[test]
fn tiny_rasters_do_not_panic() {
    let mut scene = scene(&TopographicMapsSettings::default());
    for (columns, rows) in [(1, 1), (2, 1), (3, 3)] {
        render(&mut scene, 1.0, columns, rows);
    }
}

#[test]
fn extreme_settings_normalize_and_render() {
    let wild = TopographicMapsSettings {
        zoom_percent: 99_999,
        contour_levels: 0,
        interval_m: 1_000_000,
        line_thickness: 99,
        center_latitude: 500,
        tide_seconds: 0,
        ..settings_for(WorldId::Craterlands)
    };
    let normalized = wild.normalized();
    assert_eq!(normalized.zoom_percent, 1600);
    assert_eq!(normalized.contour_levels, 6);
    assert_eq!(normalized.line_thickness, 3);
    let mut scene = scene(&wild);
    let (raster, _) = render_loaded(&mut scene, 5.0, 50, 20);
    assert!(raster.dots.iter().all(|dot| dot.is_finite() && *dot <= 1.0));
}

#[test]
fn every_control_round_trips_through_set_control() {
    let mut settings = TopographicMapsSettings {
        tide_range_m: 100,
        ..TopographicMapsSettings::default()
    };
    let rows: Vec<Control> = settings.controls();
    assert!(rows.iter().any(|row| row.id == "tide_seconds"));
    for row in rows {
        let stepped = row.stepped(1).expect("steppable row");
        settings
            .set_control(row.id, stepped.clone())
            .unwrap_or_else(|error| panic!("{}: {error}", row.id));
        let after = settings
            .controls()
            .into_iter()
            .find(|candidate| candidate.id == row.id);
        assert_eq!(
            after.map(|candidate| candidate.value),
            Some(stepped),
            "{}",
            row.id
        );
    }
    assert_eq!(
        settings.set_control("nope", ControlValue::Bool(true)),
        Ok(false)
    );
    assert!(settings
        .set_control("coastline", ControlValue::Number(1))
        .is_err());
}

#[test]
fn body_choice_index_round_trips() {
    for index in 0..BodyChoice::LABELS.len() {
        let choice = BodyChoice::from_index(index).expect("index");
        assert_eq!(choice.index(), index);
        assert!(!choice.worlds().is_empty());
    }
    assert!(BodyChoice::from_index(BodyChoice::LABELS.len()).is_none());
}

#[test]
fn settings_survive_serde() {
    let settings = settings_for(WorldId::Ceres);
    let json = serde_json::to_string(&settings).expect("serialize");
    let back: TopographicMapsSettings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(settings, back);
    let sparse: TopographicMapsSettings = serde_json::from_str("{}").expect("defaults");
    assert_eq!(sparse, TopographicMapsSettings::default());
}

#[test]
fn status_names_the_world_and_spacing() {
    let mut scene = scene(&settings_for(WorldId::Moon));
    assert_eq!(scene.status(), None);
    render(&mut scene, 0.0, 40, 12);
    assert!(scene
        .status()
        .expect("loading status")
        .starts_with("Loading"));
    render_loaded(&mut scene, 0.0, 40, 12);
    let status = scene.status().expect("status");
    assert!(status.starts_with("Moon"), "{status}");
    assert!(status.contains("m contours"), "{status}");
}

#[test]
fn reconfigure_keeps_the_loaded_world_unless_the_world_changes() {
    let base = settings_for(WorldId::Earth);
    let mut scene = scene(&base);
    render_loaded(&mut scene, 0.0, 60, 20);
    let loaded = Arc::clone(&scene.current.as_ref().expect("loaded").field);
    let mut ambient = AmbientSettings::default();
    ambient.topographic_maps = TopographicMapsSettings {
        interval_m: 2000,
        zoom_percent: 300,
        ..base.clone()
    };
    assert!(scene.reconfigure(&ambient));
    let current = scene.current.as_ref().expect("still loaded");
    assert!(Arc::ptr_eq(&loaded, &current.field));
    assert_eq!(current.interval_m, 2000.0);
    assert_eq!(scene.settings.zoom_percent, 300);
    ambient.topographic_maps.body = BodyChoice::Mars;
    assert!(!scene.reconfigure(&ambient));
    ambient.topographic_maps = TopographicMapsSettings {
        fictional_seed: 99,
        ..base
    };
    assert!(!scene.reconfigure(&ambient));
}
