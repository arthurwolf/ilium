use super::tiles::test_support::{gray_png, FakeFetcher};
use super::*;
use crate::debug::render_frame;
use crate::raster::DitherMode;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

const CITIES: [(f64, f64); 5] = [
    (2.35, 48.85),
    (139.7, 35.7),
    (-74.0, 40.7),
    (-46.6, -23.5),
    (151.2, -33.9),
];

/// A synthetic night map: dark ocean plus a bright blob per city.
fn synthetic_tile_png(level: u8, row: u32, col: u32) -> Vec<u8> {
    type TileCache = Mutex<HashMap<(u8, u32, u32), Vec<u8>>>;
    static CACHE: OnceLock<TileCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(bytes) = cache.lock().unwrap().get(&(level, row, col)) {
        return bytes.clone();
    }
    let bounds = tiles::tile_bounds(TileId { level, col, row });
    let degrees_per_pixel = tiles::tile_span_degrees(level) / TILE_PIXELS as f64;
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

fn online_fetcher() -> Arc<FakeFetcher> {
    Arc::new(FakeFetcher::new(|url, _| {
        let (layer, date, level, row, col) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != "2026-09-29" {
            return Err(TileError::Missing);
        }
        Ok(synthetic_tile_png(level, row, col))
    }))
}

fn fixed_now() -> NowFn {
    // 2026-09-30 13:40:24 UTC
    Arc::new(|| SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_775_624))
}

fn no_land() -> LandFn {
    Box::new(|_, _| false)
}

fn scene_with(
    settings: &NightLightsSettings,
    fetcher: Arc<dyn TileFetcher>,
    cache: &Path,
) -> NightLightsScene {
    NightLightsScene::with_parts(
        settings,
        &SceneEnv::for_test(cache.to_path_buf()),
        fetcher,
        fixed_now(),
        no_land(),
    )
}

/// Render once (which starts the worker) and apply worker updates until
/// `done` holds. Returns every status line seen on the way.
fn pump(scene: &mut NightLightsScene, done: impl Fn(&NightLightsScene) -> bool) -> Vec<String> {
    render_frame(scene, 200, 50, Duration::ZERO);
    let mut statuses = Vec::new();
    while !done(scene) {
        assert!(
            scene.wait_for_update(Duration::from_secs(60)),
            "worker produced no update; statuses so far: {statuses:?}"
        );
        statuses.extend(scene.status());
    }
    statuses
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
        if let (control::ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) =
            (&row.kind, &row.value)
        {
            assert!((*min..=*max).contains(value), "{} {value}", row.id);
        }
        assert!(!row.display_value().is_empty());
    }
}

#[test]
fn set_control_reports_changes_clamps_and_ignores_garbage() {
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
    let scene = |settings: NightLightsSettings| {
        NightLightsScene::with_parts(
            &settings,
            &SceneEnv::for_test(PathBuf::from("/nonexistent")),
            online_fetcher(),
            fixed_now(),
            no_land(),
        )
    };
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
fn worker_probes_recent_days_and_downloads_a_bounded_tile_set_with_progress() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = online_fetcher();
    let mut scene = scene_with(
        &NightLightsSettings::default(),
        fetcher.clone(),
        cache.path(),
    );
    let statuses = pump(&mut scene, has_mosaic);
    let urls = fetcher.urls();
    // Today (09-30) is probed first, is missing, then 09-29 succeeds.
    assert!(urls[0].contains("/2026-09-30/"), "{}", urls[0]);
    assert!(urls[1].contains("/2026-09-29/"), "{}", urls[1]);
    let daily: Vec<&String> = urls
        .iter()
        .filter(|url| url.contains("2026-09-29"))
        .collect();
    assert_eq!(
        daily.len(),
        1 + 15,
        "one probe plus the 15 tiles of level 2"
    );
    assert!(urls
        .iter()
        .all(|url| url.starts_with("https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/")));
    assert!(urls.iter().all(|url| url.contains("/500m/2/")
        || !url.contains("/2026-09-29/")
        || url.contains("/500m/2/")));
    assert!(
        statuses
            .iter()
            .any(|status| status == "Downloading imagery 7/15 tiles"),
        "{statuses:?}"
    );
    assert_eq!(scene.status().as_deref(), Some("VIIRS daily 2026-09-29"));
    // The mosaic was persisted for offline starts.
    let cached: Vec<String> = std::fs::read_dir(cache.path().join("night_lights"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        cached.contains(&"mosaic_daily_2026-09-29_L2.png".to_owned()),
        "{cached:?}"
    );
}

#[test]
fn city_lights_land_at_their_geographic_positions_on_the_flat_map() {
    let cache = tempfile::tempdir().unwrap();
    let mut scene = scene_with(
        &NightLightsSettings::default(),
        online_fetcher(),
        cache.path(),
    );
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
    let cache = tempfile::tempdir().unwrap();
    let mut scene = scene_with(
        &NightLightsSettings::default(),
        online_fetcher(),
        cache.path(),
    );
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
    let cache = tempfile::tempdir().unwrap();
    let settings = NightLightsSettings {
        projection: Projection::Globe,
        rotation_deg_per_min: 6,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, online_fetcher(), cache.path());
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
    // Paris is the location's neighbour, so it is visible at time zero.
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
    let cache = tempfile::tempdir().unwrap();
    let settings = NightLightsSettings {
        projection: Projection::Globe,
        rotation_deg_per_min: 0,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, online_fetcher(), cache.path());
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
    let cache = tempfile::tempdir().unwrap();
    let settings = NightLightsSettings {
        projection: Projection::Mollweide,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, online_fetcher(), cache.path());
    pump(&mut scene, has_mosaic);
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert_eq!(dot_at(&rendered, 1.0, 1.0), 0.0);
    assert!(rendered.lit_dots() > 10);
}

#[test]
fn terminator_hides_lights_on_the_sunlit_side() {
    let cache = tempfile::tempdir().unwrap();
    let uniform = Arc::new(FakeFetcher::new(|url, _| {
        let (layer, date, ..) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != "2026-09-29" {
            return Err(TileError::Missing);
        }
        Ok(gray_png(512, 512, |_, _| 200))
    }));
    let settings = NightLightsSettings {
        terminator: true,
        terminator_strength_percent: 100,
        glow_percent: 0,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, uniform, cache.path());
    pump(&mut scene, has_mosaic);
    let (width, height) = (200u16, 50u16);
    let time = Duration::from_secs(5);
    let rendered = render_frame(&mut scene, width, height, time);
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000) + time;
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
    let cache = tempfile::tempdir().unwrap();
    let dark = Arc::new(FakeFetcher::new(|url, _| {
        let (layer, date, ..) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != "2026-09-29" {
            return Err(TileError::Missing);
        }
        Ok(gray_png(512, 512, |_, _| 0))
    }));
    let build = |coastline: bool| {
        let settings = NightLightsSettings {
            coastline,
            coastline_strength_percent: 100,
            ..Default::default()
        };
        NightLightsScene::with_parts(
            &settings,
            &SceneEnv::for_test(cache.path().to_path_buf()),
            dark.clone(),
            fixed_now(),
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
    let cache = tempfile::tempdir().unwrap();
    let env = SceneEnv::for_test(cache.path().to_path_buf());
    // Placeholder graticule only (no worker data needed); the coastline adds
    // dots exactly when `worldmap::is_land` knows both land and sea.
    let mut with_coast = NightLightsScene::new(
        &NightLightsSettings {
            coastline: true,
            coastline_strength_percent: 100,
            ..Default::default()
        },
        &env,
    );
    let mut plain = NightLightsScene::new(&NightLightsSettings::default(), &env);
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
    let cache = tempfile::tempdir().unwrap();
    let settings = NightLightsSettings {
        marker: true,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, online_fetcher(), cache.path());
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
    let cache = tempfile::tempdir().unwrap();
    let offline = Arc::new(FakeFetcher::new(|_, _| {
        Err(TileError::Network("dns error".into()))
    }));
    let mut scene = scene_with(&NightLightsSettings::default(), offline, cache.path());
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
    let cache = tempfile::tempdir().unwrap();
    {
        let mut online = scene_with(
            &NightLightsSettings::default(),
            online_fetcher(),
            cache.path(),
        );
        pump(&mut online, has_mosaic);
    }
    let offline = Arc::new(FakeFetcher::new(|_, _| {
        Err(TileError::Network("no route".into()))
    }));
    let mut scene = scene_with(
        &NightLightsSettings::default(),
        offline.clone(),
        cache.path(),
    );
    pump(&mut scene, |scene| scene.mosaic.is_some());
    assert_eq!(
        scene.label.as_deref(),
        Some("VIIRS daily 2026-09-29 (cached)")
    );
    let rendered = render_frame(&mut scene, 200, 50, Duration::ZERO);
    let view = flat_view(&scene, 200, 50);
    let (x, y) = view.project(139.7, 35.7).unwrap();
    assert!(max_near(&rendered, x, y, 1) > 0.6);
    // The network failure is then reported but the picture stays.
    pump(&mut scene, |scene| scene.problem.is_some());
    let status = scene.status().unwrap();
    assert!(
        status.starts_with("Offline") && status.contains("cached"),
        "{status}"
    );
    assert!(scene.mosaic.is_some());
}

#[test]
fn an_up_to_date_cache_is_not_downloaded_again() {
    let cache = tempfile::tempdir().unwrap();
    {
        let mut first = scene_with(
            &NightLightsSettings::default(),
            online_fetcher(),
            cache.path(),
        );
        pump(&mut first, has_mosaic);
    }
    let fetcher = online_fetcher();
    let mut second = scene_with(
        &NightLightsSettings::default(),
        fetcher.clone(),
        cache.path(),
    );
    pump(&mut second, |scene| scene.mosaic.is_some());
    // Wait for the freshness check to finish (progress None after a Progress(None)).
    for _ in 0..200 {
        if fetcher.urls().len() >= 2 {
            break;
        }
        assert!(second.wait_for_update(Duration::from_secs(30)));
    }
    assert!(
        fetcher.urls().len() <= 2,
        "only the date probes ran: {:?}",
        fetcher.urls()
    );
}

#[test]
fn without_recent_daily_data_black_marble_is_used() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = Arc::new(FakeFetcher::new(|url, _| {
        let (layer, _, level, row, col) = parse_tile_url(url);
        if layer == DAILY_LAYER {
            Err(TileError::Missing)
        } else {
            assert_eq!(layer, BLACK_MARBLE_LAYER);
            Ok(synthetic_tile_png(level, row, col))
        }
    }));
    let mut scene = scene_with(
        &NightLightsSettings::default(),
        fetcher.clone(),
        cache.path(),
    );
    pump(&mut scene, has_mosaic);
    assert_eq!(
        scene.status().as_deref(),
        Some("Black Marble 2016-01-01 (no recent daily data)")
    );
    let daily_probes = fetcher
        .urls()
        .iter()
        .filter(|url| url.contains(DAILY_LAYER))
        .count();
    assert_eq!(
        daily_probes as i64,
        DAILY_LOOKBACK_DAYS + 1,
        "bounded look-back"
    );
}

#[test]
fn black_marble_source_never_asks_for_daily_tiles() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = online_fetcher();
    let settings = NightLightsSettings {
        source: NightSource::BlackMarble,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
    pump(&mut scene, has_mosaic);
    assert!(fetcher
        .urls()
        .iter()
        .all(|url| url.contains("VIIRS_Black_Marble/default/2016-01-01/500m/")));
    assert_eq!(scene.status().as_deref(), Some("Black Marble 2016-01-01"));
}

#[test]
fn partially_downloaded_imagery_is_flagged_and_not_cached() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = Arc::new(FakeFetcher::new(|url, _| {
        let (layer, date, level, row, col) = parse_tile_url(url);
        if layer == DAILY_LAYER && date != "2026-09-29" {
            return Err(TileError::Missing);
        }
        if (row, col) == (1, 1) {
            return Err(TileError::Missing);
        }
        Ok(synthetic_tile_png(level, row, col))
    }));
    let mut scene = scene_with(&NightLightsSettings::default(), fetcher, cache.path());
    pump(&mut scene, has_mosaic);
    let status = scene.status().unwrap();
    assert!(status.contains("partial: 14 of 15 tiles"), "{status}");
    let files = std::fs::read_dir(cache.path().join("night_lights"))
        .map(|entries| entries.flatten().count())
        .unwrap_or(0);
    assert_eq!(files, 0, "incomplete mosaics are not persisted");
}

// --- lifecycle --------------------------------------------------------------

#[test]
fn dropping_the_scene_stops_the_worker_and_render_never_blocks() {
    let cache = tempfile::tempdir().unwrap();
    let exited = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let (exit_flag, enter_flag) = (Arc::clone(&exited), Arc::clone(&entered));
    // A fetcher that hangs until asked to stop, like a stalled connection.
    let hanging = Arc::new(FakeFetcher::new(move |_, stop| {
        enter_flag.store(true, Ordering::SeqCst);
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(2));
        }
        exit_flag.store(true, Ordering::SeqCst);
        Err(TileError::Stopped)
    }));
    let mut scene = scene_with(&NightLightsSettings::default(), hanging, cache.path());
    let started = std::time::Instant::now();
    for step in 0..5 {
        render_frame(&mut scene, 100, 25, Duration::from_millis(step * 100));
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "render never waits for the worker"
    );
    while !entered.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!exited.load(Ordering::SeqCst));
    drop(scene);
    assert!(
        exited.load(Ordering::SeqCst),
        "worker joined before drop returned"
    );
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
    let scene = |settings: &NightLightsSettings| {
        NightLightsScene::with_parts(
            settings,
            &SceneEnv::for_test(PathBuf::from("/nonexistent")),
            online_fetcher(),
            fixed_now(),
            no_land(),
        )
    };
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
    assert!(projection::graticule_value(30.0, 10.0, width) > 0.0);
    assert!(projection::graticule_value(10.0, 60.0, width) > 0.0);
    assert_eq!(projection::graticule_value(17.0, 17.0, width), 0.0);
    assert!(
        projection::graticule_value(0.0, 45.0, width)
            > projection::graticule_value(30.0, 45.0, width),
        "prime meridian is brighter"
    );
}

// --- looks -----------------------------------------------------------------------

/// Reads level-2 tiles saved by hand from GIBS: set `ILIUM_TILE_FIXTURES` to a
/// directory holding `<layer>_2_<row>_<col>.png`. Prints Braille art.
#[test]
#[ignore = "visual check that needs real tiles saved on disk"]
fn print_real_night_map() {
    let directory = PathBuf::from(std::env::var("ILIUM_TILE_FIXTURES").unwrap_or_else(|_| {
        "/tmp/claude-1000/-home-arthur-dev-ai-ilium/381717b8-0831-4e69-8dfe-96eb7c8f00bb/scratchpad/satellite/net/t".to_owned()
    }));
    let fetcher = Arc::new(FakeFetcher::new(move |url, _| {
        let (layer, date, level, row, col) = parse_tile_url(url);
        if level != 2 || (layer == DAILY_LAYER && date != "2026-09-29") {
            return Err(TileError::Missing);
        }
        std::fs::read(directory.join(format!("{layer}_2_{row}_{col}.png")))
            .map_err(|_| TileError::Missing)
    }));
    for (name, settings) in [
        ("daily flat", NightLightsSettings::default()),
        (
            "black marble flat",
            NightLightsSettings {
                source: NightSource::BlackMarble,
                ..Default::default()
            },
        ),
        (
            "daily globe",
            NightLightsSettings {
                projection: Projection::Globe,
                rotation_deg_per_min: 0,
                terminator: true,
                terminator_strength_percent: 60,
                marker: true,
                ..Default::default()
            },
        ),
        (
            "daily mollweide",
            NightLightsSettings {
                projection: Projection::Mollweide,
                ..Default::default()
            },
        ),
    ] {
        let cache = tempfile::tempdir().unwrap();
        let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
        pump(&mut scene, has_mosaic);
        let rendered = render_frame(&mut scene, 110, 30, Duration::from_millis(100));
        println!("=== {name}: {}", scene.status().unwrap());
        for line in rendered.braille_lines(100, DitherMode::Ordered) {
            println!("{line}");
        }
    }
}

/// Talks to the real GIBS service (about 20 small requests) to prove the URL
/// templates, the 404 handling and the mosaic path end to end.
#[test]
#[ignore = "uses the network"]
fn live_night_lights_from_nasa_gibs() {
    let cache = tempfile::tempdir().unwrap();
    let mut scene = NightLightsScene::new(
        &NightLightsSettings {
            marker: true,
            ..Default::default()
        },
        &SceneEnv::for_test(cache.path().to_path_buf()),
    );
    let statuses = pump(&mut scene, |scene| {
        scene.mosaic.is_some() && scene.progress.is_none() || scene.problem.is_some()
    });
    println!("statuses: {statuses:?}");
    println!("final status: {:?}", scene.status());
    let rendered = render_frame(&mut scene, 110, 30, Duration::from_millis(100));
    for line in rendered.braille_lines(100, DitherMode::Ordered) {
        println!("{line}");
    }
    assert!(scene.mosaic.is_some(), "{:?}", scene.status());
}
