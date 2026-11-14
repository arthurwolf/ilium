use super::loader::test_support::FakeImagery;
use super::loader::{Imagery, BLACK_MARBLE_LAYER, DAILY_LAYER};
use super::*;
use crate::debug::render_frame;
use crate::kit::geo_tiles::test_support::gray_png;
use crate::kit::geo_tiles::{tile_bounds, tile_span_degrees, TileId, TILE_PIXELS};
use crate::live_data::poll::Answer;
use crate::raster::DitherMode;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

const CITIES: [(f64, f64); 5] = [
    (2.35, 48.85),
    (139.7, 35.7),
    (-74.0, 40.7),
    (-46.6, -23.5),
    (151.2, -33.9),
];

/// `render_frame` shows this instant at animation time zero.
const FRAME_UNIX_SECONDS: u64 = 1_790_000_000;

/// The newest day the fake service has: the day before the frames' "today".
fn available_date() -> String {
    UtcTime::from_system_time(SystemTime::UNIX_EPOCH + Duration::from_secs(FRAME_UNIX_SECONDS))
        .start_of_day()
        .plus_seconds(-86_400)
        .date()
}

fn today_date() -> String {
    UtcTime::from_system_time(SystemTime::UNIX_EPOCH + Duration::from_secs(FRAME_UNIX_SECONDS))
        .date()
}

/// A synthetic night map: dark ocean plus a bright blob per city.
fn synthetic_tile_png(level: u8, row: u32, col: u32) -> Vec<u8> {
    type TileCache = Mutex<HashMap<(u8, u32, u32), Vec<u8>>>;
    static CACHE: OnceLock<TileCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(bytes) = cache.lock().unwrap().get(&(level, row, col)) {
        return bytes.clone();
    }
    let bounds = tile_bounds(TileId { level, col, row });
    let degrees_per_pixel = tile_span_degrees(level) / TILE_PIXELS as f64;
    let bytes = gray_png(TILE_PIXELS as u32, TILE_PIXELS as u32, |x, y| {
        let lon = bounds.west + (f64::from(x) + 0.5) * degrees_per_pixel;
        let lat = bounds.north - (f64::from(y) + 0.5) * degrees_per_pixel;
        let mut value = 12.0f64;
        for (city_lon, city_lat) in CITIES {
            let distance = ((lon - city_lon).powi(2) + (lat - city_lat).powi(2)).sqrt();
            value += 243.0 * (-(distance / 1.2).powi(2)).exp();
        }
        value.min(255.0) as u8
    });
    cache
        .lock()
        .unwrap()
        .insert((level, row, col), bytes.clone());
    bytes
}

/// `(layer, date, level, row, col)` of a GIBS tile URL.
fn parse_tile_url(url: &str) -> (String, String, u8, u32, u32) {
    let rest = url.split("/best/").nth(1).unwrap();
    let fields: Vec<&str> = rest.trim_end_matches(".png").split('/').collect();
    // layer/default/date/500m/level/row/col
    (
        fields[0].to_owned(),
        fields[2].to_owned(),
        fields[4].parse().unwrap(),
        fields[5].parse().unwrap(),
        fields[6].parse().unwrap(),
    )
}

fn online() -> FakeImagery {
    FakeImagery::new(|url| {
        let (layer, date, level, row, col) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != available_date() {
            return Answer::Status(404);
        }
        Answer::Body(synthetic_tile_png(level, row, col))
    })
}

fn offline() -> FakeImagery {
    FakeImagery::new(|_| Answer::Failed("dns error".to_owned()))
}

fn no_land() -> LandFn {
    Box::new(|_, _| false)
}

fn scene_with(settings: &NightLightsSettings, imagery: FakeImagery) -> NightLightsScene {
    NightLightsScene::with_parts(
        settings,
        &SceneEnv::for_test(),
        Box::new(imagery),
        no_land(),
    )
}

fn scene_at(
    settings: &NightLightsSettings,
    location: SceneLocation,
    imagery: FakeImagery,
) -> NightLightsScene {
    NightLightsScene::with_parts(
        settings,
        &SceneEnv::for_test().with_location(location),
        Box::new(imagery),
        no_land(),
    )
}

const PARIS: SceneLocation = SceneLocation {
    latitude: 48.85,
    longitude: 2.35,
};

/// Renders frames (each advances the loader one step) until `done` holds. Returns every status
/// line seen on the way.
fn pump(scene: &mut NightLightsScene, done: impl Fn(&NightLightsScene) -> bool) -> Vec<String> {
    let mut statuses = Vec::new();
    for _ in 0..2_000 {
        render_frame(scene, 200, 50, Duration::ZERO);
        statuses.extend(scene.status());
        if done(scene) {
            return statuses;
        }
    }
    panic!("the loader did not reach the wanted state; statuses: {statuses:?}");
}

fn has_mosaic(scene: &NightLightsScene) -> bool {
    scene.mosaic.is_some() && scene.progress.is_none()
}

fn flat_view(scene: &NightLightsScene, width: usize, height: usize) -> View {
    View {
        projection: scene.settings.projection,
        center_lon: scene.location.longitude,
        center_lat: scene.location.latitude,
        zoom: f64::from(scene.settings.zoom_percent) / 100.0,
        dots_w: width * 2,
        dots_h: height * 4,
    }
}

fn dot_at(rendered: &crate::debug::Rendered, x: f32, y: f32) -> f32 {
    rendered.raster.dots[y as usize * rendered.raster.width + x as usize]
}

fn max_near(rendered: &crate::debug::Rendered, x: f32, y: f32, radius: i32) -> f32 {
    let mut best = 0.0f32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (px, py) = (x as i32 + dx, y as i32 + dy);
            if px >= 0
                && py >= 0
                && (px as usize) < rendered.raster.width
                && (py as usize) < rendered.raster.height
            {
                best = best
                    .max(rendered.raster.dots[py as usize * rendered.raster.width + px as usize]);
            }
        }
    }
    best
}

// --- settings -------------------------------------------------------------

#[test]
fn defaults_are_valid_and_serde_fills_missing_keys() {
    let defaults = NightLightsSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let parsed: NightLightsSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, defaults);
    let partial: NightLightsSettings = serde_json::from_str(
        r#"{"projection":"globe","zoom_percent":900,"source":"black_marble"}"#,
    )
    .unwrap();
    assert_eq!(partial.projection, Projection::Globe);
    assert_eq!(partial.source, NightSource::BlackMarble);
    assert_eq!(partial.normalized().zoom_percent, 600);
    let json = serde_json::to_string(&defaults).unwrap();
    for key in [
        "source",
        "projection",
        "zoom_percent",
        "gamma_percent",
        "refresh_hours",
        "coastline",
    ] {
        assert!(json.contains(&format!("\"{key}\"")), "{key} in {json}");
    }
}

#[test]
fn normalization_clamps_every_field() {
    let wild = NightLightsSettings {
        zoom_percent: -5,
        rotation_deg_per_min: 900,
        brightness_percent: 0,
        gamma_percent: 10_000,
        threshold_percent: 99,
        glow_percent: -1,
        terminator_strength_percent: 500,
        coastline_strength_percent: 0,
        refresh_hours: 0,
        ..NightLightsSettings::default()
    }
    .normalized();
    assert_eq!(
        (
            wild.zoom_percent,
            wild.rotation_deg_per_min,
            wild.brightness_percent,
            wild.gamma_percent,
            wild.threshold_percent,
            wild.glow_percent,
            wild.terminator_strength_percent,
            wild.coastline_strength_percent,
            wild.refresh_hours
        ),
        (100, 30, 25, 300, 60, 0, 100, 5, 1)
    );
}

#[test]
fn controls_are_documented_stable_and_conditional() {
    use crate::control::{ControlKind, ControlValue};
    let mut settings = NightLightsSettings::default();
    let ids = |settings: &NightLightsSettings| -> Vec<&'static str> {
        settings.controls().iter().map(|row| row.id).collect()
    };
    let base = ids(&settings);
    assert!(!base.contains(&"rotation_deg_per_min"));
    assert!(!base.contains(&"terminator_strength_percent"));
    assert!(!base.contains(&"coastline_strength_percent"));
    settings.projection = Projection::Globe;
    settings.terminator = true;
    settings.coastline = true;
    let all = ids(&settings);
    for id in [
        "rotation_deg_per_min",
        "terminator_strength_percent",
        "coastline_strength_percent",
    ] {
        assert!(all.contains(&id), "{id}");
    }
    let mut seen = std::collections::HashSet::new();
    for row in settings.controls() {
        assert!(seen.insert(row.id), "duplicate id {}", row.id);
        assert!(
            !row.label.is_empty() && row.label.len() <= 24,
            "{}",
            row.label
        );
        assert!(row.help.ends_with('.') && row.help.len() > 20, "{}", row.id);
        if let (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) =
            (&row.kind, &row.value)
        {
            assert!((*min..=*max).contains(value), "{} {value}", row.id);
        }
        assert!(!row.display_value().is_empty());
    }
}

#[test]
fn manifest_rows_match_the_scene_controls() {
    // Globe, day/night shading and coastlines show every conditional row.
    let settings = NightLightsSettings {
        projection: Projection::Globe,
        terminator: true,
        coastline: true,
        ..NightLightsSettings::default()
    };
    crate::kit::settings::assert_schema_matches_controls("night-lights", &settings.controls());
}

#[test]
fn manifest_enum_values_are_the_serde_names() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../manifest.json")).unwrap();
    let entry = manifest["contributes"]["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scene| scene["id"] == "night-lights")
        .expect("night-lights is in the catalogue");
    assert_eq!(entry["uses_location"], true);
    let properties = &entry["settings_schema"]["properties"];
    let values = |key: &str| -> Vec<String> {
        properties[key]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    };
    // Every enum value of the manifest deserializes to the option at the same position.
    for (position, value) in values("source").iter().enumerate() {
        let parsed: NightSource = serde_json::from_value(serde_json::json!(value)).unwrap();
        assert_eq!(parsed.index(), position, "source {value}");
    }
    for (position, value) in values("projection").iter().enumerate() {
        let parsed: Projection = serde_json::from_value(serde_json::json!(value)).unwrap();
        assert_eq!(parsed.index(), position, "projection {value}");
    }
    for (position, value) in values("detail").iter().enumerate() {
        let parsed: Detail = serde_json::from_value(serde_json::json!(value)).unwrap();
        assert_eq!(parsed.index(), position, "detail {value}");
    }
    // The spec of the host builds the scene with those values.
    let spec = serde_json::json!({
        "settings": {"projection": "globe", "source": "black_marble", "detail": "fine"},
        "location": {"latitude": 48.85, "longitude": 2.35},
    });
    let settings: NightLightsSettings = crate::kit::settings::from_spec(&spec);
    assert_eq!(settings.projection, Projection::Globe);
    assert_eq!(settings.source, NightSource::BlackMarble);
    assert_eq!(settings.detail, Detail::Fine);
}

#[test]
fn set_control_reports_changes_clamps_and_ignores_garbage() {
    use crate::control::ControlValue;
    let mut settings = NightLightsSettings::default();
    assert_eq!(
        settings.set_control("gamma_percent", ControlValue::Number(80)),
        Ok(true)
    );
    assert_eq!(settings.gamma_percent, 80);
    assert_eq!(
        settings.set_control("gamma_percent", ControlValue::Number(80)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("gamma_percent", ControlValue::Number(9999)),
        Ok(true)
    );
    assert_eq!(settings.gamma_percent, 300);
    assert_eq!(
        settings.set_control("gamma_percent", ControlValue::Bool(true)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("projection", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.projection, Projection::Globe);
    assert_eq!(
        settings.set_control("projection", ControlValue::Index(1)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("source", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.source, NightSource::BlackMarble);
    assert_eq!(
        settings.set_control("detail", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(
        settings.set_control("marker", ControlValue::Bool(true)),
        Ok(true)
    );
    assert!(settings.marker);
    assert_eq!(
        settings.set_control("nonsense", ControlValue::Number(1)),
        Ok(false)
    );
    // Every control row can be edited through its own id with its own value.
    for row in settings.controls() {
        assert_eq!(
            settings.clone().set_control(row.id, row.value.clone()),
            Ok(false),
            "{}",
            row.id
        );
    }
}

// --- level choice -----------------------------------------------------------

#[test]
fn tile_level_follows_terminal_size_zoom_and_detail() {
    let scene = |settings: NightLightsSettings| scene_with(&settings, online());
    let flat = scene(NightLightsSettings::default());
    assert_eq!(flat.choose_level(400, 200), 2);
    assert_eq!(flat.choose_level(4000, 2000), 3);
    let coarse = scene(NightLightsSettings {
        detail: Detail::Coarse,
        ..Default::default()
    });
    assert_eq!(coarse.choose_level(400, 200), 1);
    let fine = scene(NightLightsSettings {
        detail: Detail::Fine,
        ..Default::default()
    });
    assert_eq!(fine.choose_level(400, 200), 3);
    let globe = scene(NightLightsSettings {
        projection: Projection::Globe,
        ..Default::default()
    });
    assert_eq!(globe.choose_level(400, 200), 2);
    let zoomed = scene(NightLightsSettings {
        projection: Projection::Globe,
        zoom_percent: 300,
        ..Default::default()
    });
    assert_eq!(zoomed.choose_level(400, 200), 2);
    assert_eq!(zoomed.choose_level(2000, 1000), 3);
}

// --- downloading and rendering ---------------------------------------------

#[test]
fn loader_probes_recent_days_and_downloads_a_bounded_tile_set_with_progress() {
    let imagery = online();
    let mut scene = scene_with(&NightLightsSettings::default(), imagery.clone());
    let statuses = pump(&mut scene, has_mosaic);
    let urls = imagery.urls();
    // Today is probed first and is missing, then yesterday succeeds.
    assert!(
        urls[0].contains(&format!("/{}/", today_date())),
        "{}",
        urls[0]
    );
    assert!(
        urls[1].contains(&format!("/{}/", available_date())),
        "{}",
        urls[1]
    );
    // The probe tile is cached by the probe, so the day costs exactly its 15 level-2 tiles.
    let daily: Vec<&String> = urls
        .iter()
        .filter(|url| url.contains(&available_date()))
        .collect();
    assert_eq!(
        daily.len(),
        15,
        "the 15 tiles of level 2, the probe included"
    );
    assert!(urls
        .iter()
        .all(|url| url.starts_with("https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/")));
    assert!(
        statuses
            .iter()
            .any(|status| status.starts_with("Downloading imagery ")
                && status.ends_with("/15 tiles")),
        "{statuses:?}"
    );
    assert_eq!(
        scene.status(),
        Some(format!("VIIRS daily {}", available_date()))
    );
    // The tiles and the day were kept for offline starts.
    let cache = imagery.cache.lock().unwrap();
    assert_eq!(
        cache.get("night-lights/latest/daily").map(Vec::as_slice),
        Some(available_date().as_bytes())
    );
    assert!(cache.keys().filter(|key| key.contains("/500m/2/")).count() >= 15);
}

#[test]
fn city_lights_land_at_their_geographic_positions_on_the_flat_map() {
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, online());
    pump(&mut scene, has_mosaic);
    let rendered = render_frame(&mut scene, 200, 50, Duration::ZERO);
    let view = flat_view(&scene, 200, 50);
    for (lon, lat) in CITIES {
        let (x, y) = view.project(lon, lat).unwrap();
        assert!(
            max_near(&rendered, x, y, 1) > 0.7,
            "city at {lon},{lat} dot {x},{y}"
        );
    }
    // Open ocean and a polar strip stay dark.
    for (lon, lat) in [(-30.0, 0.0), (80.0, -50.0), (-150.0, 20.0)] {
        let (x, y) = view.project(lon, lat).unwrap();
        assert!(max_near(&rendered, x, y, 2) < 0.02, "dark at {lon},{lat}");
    }
    assert!(rendered.lit_dots() > 20);
}

#[test]
fn flat_rendering_is_deterministic_and_static_without_animation_features() {
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, online());
    pump(&mut scene, has_mosaic);
    let first = render_frame(&mut scene, 120, 30, Duration::ZERO);
    let second = render_frame(&mut scene, 120, 30, Duration::ZERO);
    let later = render_frame(&mut scene, 120, 30, Duration::from_secs(400));
    assert_eq!(first.raster.dots, second.raster.dots);
    assert_eq!(first.raster.dots, later.raster.dots);
    assert_eq!(scene.frames_per_second(), 4);
}

#[test]
fn globe_shows_only_the_visible_hemisphere_and_turns_over_time() {
    let settings = NightLightsSettings {
        projection: Projection::Globe,
        rotation_deg_per_min: 6,
        ..Default::default()
    };
    let mut scene = scene_at(&settings, PARIS, online());
    pump(&mut scene, has_mosaic);
    let now = render_frame(&mut scene, 100, 40, Duration::ZERO);
    let later = render_frame(&mut scene, 100, 40, Duration::from_secs(600));
    assert_ne!(
        now.raster.dots, later.raster.dots,
        "rotation changes the picture"
    );
    // Corners are outside the sphere.
    assert_eq!(dot_at(&now, 0.0, 0.0), 0.0);
    assert_eq!(dot_at(&now, 199.0, 159.0), 0.0);
    // Paris is the location, so it is visible at time zero.
    let view = View {
        projection: Projection::Globe,
        center_lon: scene.location.longitude,
        center_lat: scene.location.latitude,
        zoom: 1.0,
        dots_w: 200,
        dots_h: 160,
    };
    let (x, y) = view.project(2.35, 48.85).unwrap();
    assert!(max_near(&now, x, y, 1) > 0.6);
    // Sydney is on the far side of the globe.
    assert!(view.project(151.2, -33.9).is_none());
    assert_eq!(scene.frames_per_second(), 15);
    // Still deterministic for equal times.
    let again = render_frame(&mut scene, 100, 40, Duration::from_secs(600));
    assert_eq!(later.raster.dots, again.raster.dots);
}

#[test]
fn stationary_globe_without_rotation_does_not_change() {
    let settings = NightLightsSettings {
        projection: Projection::Globe,
        rotation_deg_per_min: 0,
        ..Default::default()
    };
    let mut scene = scene_at(&settings, PARIS, online());
    pump(&mut scene, has_mosaic);
    let first = render_frame(&mut scene, 100, 40, Duration::ZERO);
    let later = render_frame(&mut scene, 100, 40, Duration::from_secs(900));
    assert_eq!(first.raster.dots, later.raster.dots);
    // The rim outside the limb is drawn faintly, never bright.
    let rim = first
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.05 && **dot < 0.2)
        .count();
    assert!(rim > 20, "rim dots {rim}");
}

#[test]
fn mollweide_leaves_the_corners_empty() {
    let settings = NightLightsSettings {
        projection: Projection::Mollweide,
        ..Default::default()
    };
    let mut scene = scene_at(&settings, PARIS, online());
    pump(&mut scene, has_mosaic);
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert_eq!(dot_at(&rendered, 1.0, 1.0), 0.0);
    assert!(rendered.lit_dots() > 10);
}

#[test]
fn terminator_hides_lights_on_the_sunlit_side() {
    let uniform = FakeImagery::new(|url| {
        let (layer, date, ..) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != available_date() {
            return Answer::Status(404);
        }
        Answer::Body(gray_png(512, 512, |_, _| 200))
    });
    let settings = NightLightsSettings {
        terminator: true,
        terminator_strength_percent: 100,
        glow_percent: 0,
        ..Default::default()
    };
    let mut scene = scene_at(&settings, PARIS, uniform);
    pump(&mut scene, has_mosaic);
    let (width, height) = (200u16, 50u16);
    let time = Duration::from_secs(5);
    let rendered = render_frame(&mut scene, width, height, time);
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(FRAME_UNIX_SECONDS) + time;
    let sun = sun_position(UtcTime::from_system_time(now).0 as f64);
    let view = flat_view(&scene, 200, 50);
    let (mut day_sum, mut day_count, mut night_sum, mut night_count) = (0.0f32, 0, 0.0f32, 0);
    for y in 0..rendered.raster.height {
        for x in 0..rendered.raster.width {
            let Some((lon, lat)) = view.locate(x, y) else {
                continue;
            };
            let value = rendered.raster.dots[y * rendered.raster.width + x];
            let daylight = sun.daylight(lon, lat);
            if daylight > 0.98 {
                day_sum += value;
                day_count += 1;
            } else if daylight < 0.02 {
                night_sum += value;
                night_count += 1;
            }
        }
    }
    assert!(day_count > 1000 && night_count > 1000);
    assert!(
        day_sum / (day_count as f32) < 0.01,
        "day side dark: {}",
        day_sum / day_count as f32
    );
    assert!(
        night_sum / night_count as f32 > 0.4,
        "night side lit: {}",
        night_sum / night_count as f32
    );
    assert_eq!(scene.frames_per_second(), 8);
}

#[test]
fn coastline_draws_only_where_the_injected_land_mask_changes() {
    let dark = FakeImagery::new(|url| {
        let (layer, date, ..) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != available_date() {
            return Answer::Status(404);
        }
        Answer::Body(gray_png(512, 512, |_, _| 0))
    });
    let build = |coastline: bool| {
        let settings = NightLightsSettings {
            coastline,
            coastline_strength_percent: 100,
            ..Default::default()
        };
        NightLightsScene::with_parts(
            &settings,
            &SceneEnv::for_test().with_location(PARIS),
            Box::new(dark.clone()),
            Box::new(|lon, lat| lon.abs() < 30.0 && lat.abs() < 20.0),
        )
    };
    let mut off = build(false);
    pump(&mut off, has_mosaic);
    let mut on = build(true);
    pump(&mut on, has_mosaic);
    let plain = render_frame(&mut off, 200, 50, Duration::ZERO);
    let coast = render_frame(&mut on, 200, 50, Duration::ZERO);
    assert_eq!(
        plain.raster.dots.iter().filter(|dot| **dot > 0.0).count(),
        0
    );
    let view = flat_view(&on, 200, 50);
    let (east_x, mid_y) = view.project(30.0, 0.0).unwrap();
    let lit: Vec<(usize, usize)> = (0..coast.raster.height)
        .flat_map(|y| (0..coast.raster.width).map(move |x| (x, y)))
        .filter(|(x, y)| coast.raster.dots[y * coast.raster.width + x] > 0.5)
        .collect();
    assert!(lit.len() > 100, "coast has {} dots", lit.len());
    let (west_x, _) = view.project(-30.0, 0.0).unwrap();
    let (_, north_y) = view.project(0.0, 20.0).unwrap();
    let (_, south_y) = view.project(0.0, -20.0).unwrap();
    for (x, y) in &lit {
        let (x, y) = (*x as f32, *y as f32);
        let on_vertical = ((x - east_x).abs() < 2.0 || (x - west_x).abs() < 2.0)
            && y >= north_y - 2.0
            && y <= south_y + 2.0;
        let on_horizontal = ((y - north_y).abs() < 2.0 || (y - south_y).abs() < 2.0)
            && x >= west_x - 2.0
            && x <= east_x + 2.0;
        assert!(on_vertical || on_horizontal, "stray coast dot at {x},{y}");
    }
    assert!(lit.iter().any(|(_, y)| (*y as f32 - mid_y).abs() < 3.0));
}

#[test]
fn the_shared_land_mask_feeds_the_coastline() {
    let env = SceneEnv::for_test();
    // Placeholder graticule only (no imagery needed); the coastline adds dots exactly when
    // `worldmap::is_land` knows both land and sea.
    let mut with_coast = NightLightsScene::new(
        &NightLightsSettings {
            coastline: true,
            coastline_strength_percent: 100,
            ..Default::default()
        },
        &env,
    );
    let mut plain = NightLightsScene::new(&NightLightsSettings::default(), &env);
    // `new` talks to the host, which a unit test has none of; swap in a silent imagery.
    with_coast.imagery = Some(Box::new(online()));
    plain.imagery = Some(Box::new(online()));
    let coast = render_frame(&mut with_coast, 120, 30, Duration::ZERO);
    let baseline = render_frame(&mut plain, 120, 30, Duration::ZERO);
    let has_land = (-170..170).any(|lon| crate::worldmap::is_land(f64::from(lon), 20.0));
    let has_sea = (-170..170).any(|lon| !crate::worldmap::is_land(f64::from(lon), 20.0));
    if has_land && has_sea {
        let extra = coast
            .raster
            .dots
            .iter()
            .zip(&baseline.raster.dots)
            .filter(|(with, without)| with > without)
            .count();
        assert!(extra > 100, "coastline adds dots: {extra}");
    } else {
        assert_eq!(coast.raster.dots, baseline.raster.dots);
    }
}

#[test]
fn marker_blinks_at_the_shared_location() {
    let settings = NightLightsSettings {
        marker: true,
        ..Default::default()
    };
    let mut scene = scene_at(&settings, PARIS, online());
    pump(&mut scene, has_mosaic);
    let on = render_frame(&mut scene, 200, 50, Duration::from_millis(100));
    let off = render_frame(&mut scene, 200, 50, Duration::from_millis(900));
    let view = flat_view(&scene, 200, 50);
    let (x, y) = view
        .project(scene.location.longitude, scene.location.latitude)
        .unwrap();
    assert!(dot_at(&on, x, y) > 0.9, "marker core lit");
    assert!(dot_at(&off, x, y) < 0.5, "marker dark in the off phase");
    assert!(on.raster.dots.iter().sum::<f32>() > off.raster.dots.iter().sum::<f32>());
}

// --- offline behaviour and caching -----------------------------------------

#[test]
fn without_cache_or_network_a_status_and_placeholder_grid_are_shown() {
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, offline());
    pump(&mut scene, |scene| scene.problem.is_some());
    let status = scene.status().unwrap();
    assert!(
        status.contains("No imagery yet") && status.contains("dns error"),
        "{status}"
    );
    let rendered = render_frame(&mut scene, 100, 25, Duration::ZERO);
    let lit = rendered
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.0)
        .count();
    let brightest = rendered.raster.dots.iter().cloned().fold(0.0f32, f32::max);
    assert!(lit > 100, "placeholder graticule drawn ({lit} dots)");
    assert!(brightest < 0.5, "placeholder stays dim ({brightest})");
    let lines = rendered.braille_lines(100, DitherMode::Ordered);
    assert!(lines.iter().any(|line| line.chars().any(|c| c != ' ')));
}

#[test]
fn cached_mosaic_is_shown_immediately_and_offline() {
    let first = online();
    {
        let mut online_scene = scene_at(&NightLightsSettings::default(), PARIS, first.clone());
        pump(&mut online_scene, has_mosaic);
    }
    let second = offline();
    *second.cache.lock().unwrap() = first.cache.lock().unwrap().clone();
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, second.clone());
    pump(&mut scene, |scene| scene.mosaic.is_some());
    assert_eq!(
        scene.label.as_deref(),
        Some(format!("VIIRS daily {} (cached)", available_date()).as_str())
    );
    let rendered = render_frame(&mut scene, 200, 50, Duration::ZERO);
    let view = flat_view(&scene, 200, 50);
    let (x, y) = view.project(139.7, 35.7).unwrap();
    assert!(max_near(&rendered, x, y, 1) > 0.6);
    // Showing the cached day asked the network for nothing.
    assert!(second.urls().is_empty(), "{:?}", second.urls());
}

#[test]
fn a_cache_that_misses_tiles_is_refilled_not_shown() {
    let first = online();
    {
        let mut online_scene = scene_at(&NightLightsSettings::default(), PARIS, first.clone());
        pump(&mut online_scene, has_mosaic);
    }
    // Lose one tile of the day, as a storage eviction would.
    let second = online();
    {
        let mut cache = first.cache.lock().unwrap().clone();
        cache.retain(|key, _| !key.ends_with("/500m/2/1/1.png"));
        *second.cache.lock().unwrap() = cache;
    }
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, second.clone());
    pump(&mut scene, has_mosaic);
    let label = scene.label.clone().unwrap();
    assert!(!label.contains("cached"), "{label}");
    let fetched: Vec<String> = second
        .urls()
        .into_iter()
        .filter(|url| url.contains(&available_date()))
        .collect();
    assert_eq!(
        fetched.len(),
        1,
        "only the evicted tile is fetched: {fetched:?}"
    );
    assert!(fetched[0].ends_with("/500m/2/1/1.png"), "{}", fetched[0]);
}

#[test]
fn an_up_to_date_cache_is_not_downloaded_again() {
    let first = online();
    {
        let mut scene = scene_at(&NightLightsSettings::default(), PARIS, first.clone());
        pump(&mut scene, has_mosaic);
    }
    let second = online();
    *second.cache.lock().unwrap() = first.cache.lock().unwrap().clone();
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, second.clone());
    pump(&mut scene, has_mosaic);
    for _ in 0..60 {
        render_frame(&mut scene, 200, 50, Duration::ZERO);
    }
    assert!(
        second.urls().len() <= 1,
        "at most the today probe ran: {:?}",
        second.urls()
    );
}

#[test]
fn without_recent_daily_data_black_marble_is_used() {
    let imagery = FakeImagery::new(|url| {
        let (layer, _, level, row, col) = parse_tile_url(url);
        if layer == DAILY_LAYER {
            Answer::Status(404)
        } else {
            assert_eq!(layer, BLACK_MARBLE_LAYER);
            Answer::Body(synthetic_tile_png(level, row, col))
        }
    });
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, imagery.clone());
    pump(&mut scene, has_mosaic);
    assert_eq!(
        scene.status().as_deref(),
        Some("Black Marble 2016-01-01 (no recent daily data)")
    );
    let daily_probes = imagery
        .urls()
        .iter()
        .filter(|url| url.contains(DAILY_LAYER))
        .count();
    assert_eq!(daily_probes, 5, "bounded look-back: today and four days");
}

#[test]
fn black_marble_source_never_asks_for_daily_tiles() {
    let imagery = online();
    let settings = NightLightsSettings {
        source: NightSource::BlackMarble,
        ..Default::default()
    };
    let mut scene = scene_at(&settings, PARIS, imagery.clone());
    pump(&mut scene, has_mosaic);
    assert!(imagery
        .urls()
        .iter()
        .all(|url| url.contains("VIIRS_Black_Marble/default/2016-01-01/500m/")));
    assert_eq!(scene.status().as_deref(), Some("Black Marble 2016-01-01"));
}

#[test]
fn partially_downloaded_imagery_is_flagged_and_not_remembered_as_complete() {
    let imagery = FakeImagery::new(|url| {
        let (layer, date, level, row, col) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != available_date() {
            return Answer::Status(404);
        }
        if (row, col) == (1, 1) {
            return Answer::Status(404);
        }
        Answer::Body(synthetic_tile_png(level, row, col))
    });
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, imagery.clone());
    pump(&mut scene, has_mosaic);
    let status = scene.status().unwrap();
    assert!(status.contains("partial: 14 of 15 tiles"), "{status}");
    assert!(
        !imagery
            .cache
            .lock()
            .unwrap()
            .contains_key("night-lights/latest/daily"),
        "an incomplete mosaic is not offered to the next start"
    );
}

#[test]
fn a_hanging_network_never_blocks_a_frame() {
    // Requests that never finish: every frame must still return at once.
    struct Hanging(u64);
    impl Imagery for Hanging {
        fn start(&mut self, _url: &str) -> Result<u64, String> {
            self.0 += 1;
            Ok(self.0)
        }
        fn take(&mut self, _id: u64) -> Option<Answer> {
            None
        }
        fn recall(&mut self, _key: &str) -> Option<Vec<u8>> {
            None
        }
        fn remember(&mut self, _key: &str, _value: &[u8]) {}
    }
    let mut scene = NightLightsScene::with_parts(
        &NightLightsSettings::default(),
        &SceneEnv::for_test(),
        Box::new(Hanging(0)),
        no_land(),
    );
    let started = std::time::Instant::now();
    for step in 0..50 {
        render_frame(&mut scene, 100, 25, Duration::from_millis(step * 100));
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "render never waits for the network"
    );
    assert_eq!(
        scene.status().as_deref(),
        Some("Preparing night imagery"),
        "still waiting for the first probe"
    );
}

#[test]
fn a_failed_network_is_retried_after_the_retry_delay() {
    let imagery = offline();
    let mut scene = scene_at(&NightLightsSettings::default(), PARIS, imagery.clone());
    pump(&mut scene, |scene| scene.problem.is_some());
    let before = imagery.urls().len();
    // Within the retry delay nothing is requested.
    for step in 1..20 {
        render_frame(&mut scene, 100, 25, Duration::from_secs(step));
    }
    assert_eq!(imagery.urls().len(), before);
    // After it, the loader probes again.
    for step in 0..20 {
        render_frame(&mut scene, 100, 25, Duration::from_secs(400 + step));
    }
    assert!(imagery.urls().len() > before);
}

// --- shaping ---------------------------------------------------------------------

#[test]
fn shaping_applies_threshold_gamma_and_gain() {
    let mut settings = NightLightsSettings {
        threshold_percent: 10,
        gamma_percent: 100,
        brightness_percent: 100,
        ..Default::default()
    };
    let scene = |settings: &NightLightsSettings| scene_with(settings, online());
    let plain = scene(&settings);
    assert_eq!(plain.shape(0.05), 0.0);
    assert!((plain.shape(1.0) - 1.0).abs() < 1e-6);
    let previous = (0..=20)
        .map(|step| plain.shape(step as f32 / 20.0))
        .collect::<Vec<_>>();
    assert!(previous.windows(2).all(|pair| pair[1] >= pair[0]));
    settings.gamma_percent = 50;
    let lifted = scene(&settings);
    assert!(
        lifted.shape(0.3) > plain.shape(0.3),
        "lower gamma lifts faint light"
    );
    settings.brightness_percent = 200;
    assert!(scene(&settings).shape(0.3) > lifted.shape(0.3) * 1.9);
}

#[test]
fn placeholder_grid_has_lines_every_thirty_degrees_only() {
    let width = 0.5;
    assert!(graticule_value(30.0, 10.0, width) > 0.0);
    assert!(graticule_value(10.0, 60.0, width) > 0.0);
    assert_eq!(graticule_value(17.0, 17.0, width), 0.0);
    assert!(
        graticule_value(0.0, 45.0, width) > graticule_value(30.0, 45.0, width),
        "prime meridian is brighter"
    );
}

#[test]
fn the_scene_is_built_from_a_host_spec() {
    let spec = serde_json::json!({
        "settings": {"projection": "mollweide", "marker": true},
        "location": {"latitude": 10.0, "longitude": 20.0},
    });
    let env = SceneEnv::for_test().with_location(SceneLocation::from_spec(&spec));
    let scene = create(&spec, &env);
    // Before the first frame the loader has not started: the scene asks for a brisk cadence.
    assert_eq!(scene.frames_per_second(), 15, "loading is brisk");
}
