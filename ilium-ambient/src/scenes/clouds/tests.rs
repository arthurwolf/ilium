use super::providers::{plan_frame_times, provider};
use super::worker::{CloudFrame, FrameSet, Update};
use super::*;
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use crate::scenes::night_lights::tiles::test_support::FakeFetcher;
use crate::scenes::night_lights::tiles::{tile_bounds, GeoGrid, TileError, TileId, UtcTime};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

// --- synthetic weather ---------------------------------------------------------

/// Encode an RGBA image built by `pixel(x, y)`.
fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

/// A storm system: brightness 90 ground plus a bright blob whose centre drifts
/// east with the hour of day, so different times give different pictures.
fn storm(lon: f64, lat: f64, hours: f64) -> u8 {
    let centre_lon = -12.0 + 2.0 * hours;
    let distance = ((lon - centre_lon).powi(2) + (lat - 50.0).powi(2)).sqrt();
    (90.0 + 165.0 * (-(distance / 7.0).powi(2)).exp()) as u8
}

fn hours_of_day(time: UtcTime) -> f64 {
    let (_, _, _, hour, minute, _) = time.civil();
    hour as f64 + minute as f64 / 60.0
}

fn query<'a>(url: &'a str, key: &str) -> &'a str {
    let marker = format!("{key}=");
    let start = url.find(&marker).unwrap() + marker.len();
    url[start..].split('&').next().unwrap()
}

#[test]
fn local_cloud_grid_is_bounded_for_extreme_terminal_aspect_ratios() {
    let (width, height) = local_grid_size(80, usize::MAX / 2);
    assert!(width.checked_mul(height).unwrap() <= MAX_CLOUD_GRID_PIXELS);
    assert!(height >= 32);
}

#[test]
fn cloud_worker_admission_refusal_is_visible_and_retryable() {
    let (execution, resources) = crate::resources::isolated_test_resources();
    let mut held = Vec::new();
    while let Ok(reservation) = resources.reserve_worker(crate::resources::WorkerCost {
        threads: 1,
        resident_bytes: 1,
    }) {
        held.push(reservation);
        assert!(held.len() < 64, "isolated worker quota did not fill");
    }
    assert!(!held.is_empty());

    let cache = tempfile::tempdir().unwrap();
    let env = SceneEnv::for_test(cache.path().to_path_buf(), resources);
    let requests = Arc::new(AtomicUsize::new(0));
    let observed_requests = Arc::clone(&requests);
    let fetcher = Arc::new(FakeFetcher::new(move |_, _| {
        observed_requests.fetch_add(1, Ordering::Relaxed);
        Err(TileError::Network("offline".into()))
    }));
    let mut scene = CloudsScene::with_parts(
        &CloudsSettings::default(),
        &env,
        fetcher,
        fixed_now(),
        no_land(),
    );

    render_frame(&mut scene, 80, 24, Duration::ZERO);
    assert!(scene.worker.is_none());
    assert!(scene
        .problem
        .as_deref()
        .is_some_and(|problem| problem.contains("admission refused")));
    assert_eq!(requests.load(Ordering::Relaxed), 0);

    drop(held);
    render_frame(&mut scene, 80, 24, Duration::ZERO);
    let worker = scene.worker.take().expect("worker admission should retry");
    let retirement = worker.request_stop().expect("worker has a join ticket");
    drop(scene.receiver.take());
    retirement
        .join_until(std::time::Instant::now() + Duration::from_secs(3))
        .unwrap();
    drop(scene);
    drop(execution);
}

#[test]
fn cloud_worker_panic_is_reported_and_does_not_restart_immediately() {
    let (execution, resources) = crate::resources::isolated_test_resources();
    let cache = tempfile::tempdir().unwrap();
    let env = SceneEnv::for_test(cache.path().to_path_buf(), resources);
    let fetcher = Arc::new(FakeFetcher::new(|_, _| -> Result<Vec<u8>, TileError> {
        panic!("injected cloud worker failure")
    }));
    let mut scene = CloudsScene::with_parts(
        &CloudsSettings::default(),
        &env,
        fetcher,
        fixed_now(),
        no_land(),
    );

    render_frame(&mut scene, 80, 24, Duration::ZERO);
    let ticket = scene
        .worker
        .as_ref()
        .and_then(Worker::join_observer)
        .expect("started worker ticket");
    ticket
        .join_until(std::time::Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(
        ticket.exit(),
        Some(ilium_platform::owned_worker::WorkerExit::Panicked)
    );

    render_frame(&mut scene, 80, 24, Duration::ZERO);
    assert!(scene.worker.is_none());
    assert!(scene
        .problem
        .as_deref()
        .is_some_and(|problem| problem.contains("worker exited unexpectedly")));
    assert!(scene.worker_retry_after.is_some());
    drop(scene);
    drop(execution);
}

fn wms_map_png(url: &str) -> Vec<u8> {
    let numbers: Vec<f64> = query(url, "bbox")
        .split(',')
        .map(|value| value.parse().unwrap())
        .collect();
    let (width, height): (u32, u32) = (
        query(url, "width").parse().unwrap(),
        query(url, "height").parse().unwrap(),
    );
    let time = UtcTime::parse(query(url, "time")).unwrap();
    png(width, height, |x, y| {
        let lon = numbers[0] + (f64::from(x) + 0.5) / f64::from(width) * (numbers[2] - numbers[0]);
        let lat = numbers[3] - (f64::from(y) + 0.5) / f64::from(height) * (numbers[3] - numbers[1]);
        let value = storm(lon, lat, hours_of_day(time));
        [value, value, value, 255]
    })
}

fn tile_png(url: &str, red_tint: bool) -> Vec<u8> {
    // .../{layer}/default/{time}/{set}/{level}/{row}/{col}.{ext}
    let parts: Vec<&str> = url.split('/').collect();
    let n = parts.len();
    let id = TileId {
        level: parts[n - 3].parse().unwrap(),
        row: parts[n - 2].parse().unwrap(),
        col: parts[n - 1].split('.').next().unwrap().parse().unwrap(),
    };
    let time = UtcTime::parse(parts[n - 5]).unwrap();
    let bounds = tile_bounds(id);
    let degrees = (bounds.east - bounds.west) / 512.0;
    png(512, 512, |x, y| {
        let lon = bounds.west + (f64::from(x) + 0.5) * degrees;
        let lat = bounds.north - (f64::from(y) + 0.5) * degrees;
        let value = storm(lon, lat, hours_of_day(time));
        if red_tint {
            [value, value / 3, value / 3, 255]
        } else {
            [value, value, value, 255]
        }
    })
}

fn capabilities(layer: &str, default: &str) -> Vec<u8> {
    // Real workspace capabilities list layers without their workspace prefix.
    let layer = layer.rsplit(':').next().unwrap();
    format!(
        "<Layer><Name>{layer}</Name><Dimension name=\"time\" default=\"{default}\" units=\"ISO8601\">x</Dimension></Layer>"
    )
    .into_bytes()
}

/// Every service the scene may contact, answered from memory. GOES-East's
/// newest listed image (13:00) is not ingested yet, as in real life.
fn weather_fetcher() -> Arc<FakeFetcher> {
    Arc::new(FakeFetcher::new(|url, _| {
        if url.contains("GetCapabilities") {
            return Ok(if url.contains("/msg_fes/") {
                capabilities("msg_fes:ir108", "2026-09-30T13:15:00Z")
            } else if url.contains("/mumi/") {
                capabilities("mumi:worldcloudmap_ir108", "2026-09-30T12:00:00Z")
            } else {
                capabilities("msg_iodc:ir108", "2026-09-30T13:15:00Z")
            });
        }
        if url.contains("request=GetMap") {
            return Ok(wms_map_png(url));
        }
        if url.contains("/1.0.0/") {
            return Ok(b"<Domains><DimensionDomain><Domain>2026-09-30T09:00:00Z/2026-09-30T13:00:00Z/PT10M</Domain></DimensionDomain></Domains>".to_vec());
        }
        if url.contains("/default/2026-09-30T13:00:00Z/") {
            return Err(TileError::Missing);
        }
        Ok(tile_png(url, url.contains("GeoColor")))
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
    settings: &CloudsSettings,
    fetcher: Arc<dyn TileFetcher>,
    cache: &std::path::Path,
) -> CloudsScene {
    CloudsScene::with_parts(
        settings,
        &SceneEnv::for_test(cache.to_path_buf(), crate::resources::test_resources()),
        fetcher,
        fixed_now(),
        no_land(),
    )
}

/// Render once (starting the worker) and apply updates until `done` holds.
fn pump(scene: &mut CloudsScene, done: impl Fn(&CloudsScene) -> bool) {
    render_frame(scene, 120, 30, Duration::ZERO);
    while !done(scene) {
        assert!(
            scene.wait_for_update(Duration::from_secs(60)),
            "worker produced no update (status {:?})",
            scene.status()
        );
    }
}

/// Stop the worker so injected frames are not overwritten by real updates.
fn detach(scene: &mut CloudsScene) {
    scene.worker = None;
    scene.receiver = None;
}

fn settled(scene: &CloudsScene) -> bool {
    scene.set.is_some() && scene.progress.is_none()
}

fn frame_count(scene: &CloudsScene) -> usize {
    scene.set.as_ref().map_or(0, |set| set.frames.len())
}

fn mean(rendered: &Rendered) -> f32 {
    rendered.raster.dots.iter().sum::<f32>() / rendered.raster.dots.len() as f32
}

fn synthetic_set(times: &[i64], fill: impl Fn(usize, f64, f64) -> u8) -> Arc<FrameSet> {
    let bbox = GeoBox {
        west: -30.0,
        south: 30.0,
        east: 30.0,
        north: 70.0,
    };
    let frames = times
        .iter()
        .enumerate()
        .map(|(index, time)| {
            let mut grid = GeoGrid::blank(bbox, 120, 80, false);
            for y in 0..80 {
                for x in 0..120 {
                    let lon = -30.0 + (x as f64 + 0.5) / 120.0 * 60.0;
                    let lat = 70.0 - (y as f64 + 0.5) / 80.0 * 40.0;
                    grid.luma[y * 120 + x] = fill(index, lon, lat);
                }
            }
            Arc::new(CloudFrame::new(UtcTime(*time), grid))
        })
        .collect();
    Arc::new(FrameSet::new("Test", bbox, frames))
}

// --- settings ------------------------------------------------------------------

#[test]
fn defaults_are_valid_and_serde_fills_missing_keys() {
    let defaults = CloudsSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    assert_eq!(
        serde_json::from_str::<CloudsSettings>("{}").unwrap(),
        defaults
    );
    let partial: CloudsSettings = serde_json::from_str(
        r#"{"coverage":"global","source":"goes_west","history_hours":7,"zoom_level":99,"projection":"mollweide"}"#,
    )
    .unwrap();
    assert_eq!(partial.coverage, Coverage::Global);
    assert_eq!(partial.source, CloudSource::GoesWest);
    let normal = partial.normalized();
    assert_eq!((normal.history_hours, normal.zoom_level), (6, 6));
    let json = serde_json::to_string(&defaults).unwrap();
    for key in [
        "coverage",
        "source",
        "history_hours",
        "playback_fps",
        "refresh_minutes",
        "cell_colors",
    ] {
        assert!(json.contains(&format!("\"{key}\"")), "{key}");
    }
}

#[test]
fn normalization_clamps_every_field() {
    let wild = CloudsSettings {
        rotation_deg_per_min: 999,
        zoom_level: 0,
        history_hours: 1000,
        playback_fps: 0,
        smoothing_percent: 500,
        refresh_minutes: 1,
        contrast_percent: 0,
        brightness_percent: 900,
        ground_dim_percent: -3,
        land_underlay_percent: 100,
        ..CloudsSettings::default()
    }
    .normalized();
    assert_eq!(
        (
            wild.rotation_deg_per_min,
            wild.zoom_level,
            wild.history_hours,
            wild.playback_fps,
            wild.smoothing_percent,
            wild.refresh_minutes,
            wild.contrast_percent,
            wild.brightness_percent,
            wild.ground_dim_percent,
            wild.land_underlay_percent
        ),
        (30, 1, 24, 1, 100, 5, 50, 50, 0, 60)
    );
    assert_eq!(snap_history(3), 0);
    assert_eq!(snap_history(4), 6);
    assert_eq!(snap_history(9), 6);
    assert_eq!(snap_history(10), 12);
    assert_eq!(snap_history(20), 24);
}

#[test]
fn controls_are_documented_stable_and_conditional() {
    let ids = |settings: &CloudsSettings| -> Vec<&'static str> {
        settings.controls().iter().map(|row| row.id).collect()
    };
    let local = ids(&CloudsSettings::default());
    assert!(local.contains(&"zoom_level"));
    for hidden in [
        "projection",
        "rotation_deg_per_min",
        "playback_fps",
        "smoothing_percent",
        "land_underlay_percent",
    ] {
        assert!(!local.contains(&hidden), "{hidden}");
    }
    let global = ids(&CloudsSettings {
        coverage: Coverage::Global,
        history_hours: 12,
        land_underlay: true,
        ..Default::default()
    });
    for shown in [
        "projection",
        "rotation_deg_per_min",
        "playback_fps",
        "smoothing_percent",
        "land_underlay_percent",
    ] {
        assert!(global.contains(&shown), "{shown}");
    }
    assert!(!global.contains(&"zoom_level"));
    let everything = CloudsSettings {
        coverage: Coverage::Global,
        history_hours: 12,
        land_underlay: true,
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();
    for row in everything.controls() {
        assert!(seen.insert(row.id), "duplicate {}", row.id);
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
        assert_eq!(
            everything.clone().set_control(row.id, row.value.clone()),
            Ok(false),
            "{}",
            row.id
        );
    }
}

#[test]
fn set_control_reports_changes_and_ignores_garbage() {
    let mut settings = CloudsSettings::default();
    assert_eq!(
        settings.set_control("history_hours", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.history_hours, 12);
    assert_eq!(
        settings.set_control("history_hours", ControlValue::Index(2)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("history_hours", ControlValue::Index(77)),
        Ok(true)
    );
    assert_eq!(settings.history_hours, 24);
    assert_eq!(
        settings.set_control("coverage", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.coverage, Coverage::Global);
    assert_eq!(
        settings.set_control("source", ControlValue::Index(3)),
        Ok(true)
    );
    assert_eq!(settings.source, CloudSource::Meteosat);
    assert_eq!(
        settings.set_control("zoom_level", ControlValue::Number(40)),
        Ok(true)
    );
    assert_eq!(settings.zoom_level, 6);
    assert_eq!(
        settings.set_control("zoom_level", ControlValue::Text("x".into())),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("cell_colors", ControlValue::Bool(true)),
        Ok(true)
    );
    assert_eq!(
        settings.set_control("invert", ControlValue::Bool(true)),
        Ok(true)
    );
    assert_eq!(
        settings.set_control("nope", ControlValue::Bool(true)),
        Ok(false)
    );
}

// --- geometry and timing ---------------------------------------------------------

#[test]
fn local_view_box_keeps_square_dots_and_stays_on_the_map() {
    let bbox = local_view_box(48.0, 2.0, 30.0, 400, 200);
    assert!((bbox.height_degrees() - 30.0).abs() < 1e-9);
    // Physical width/height (with the cos(latitude) squeeze) matches the raster.
    let physical = bbox.width_degrees() * 48.0f64.to_radians().cos() / bbox.height_degrees();
    assert!((physical - 2.0).abs() < 0.01, "{physical}");
    let centre = bbox.center();
    assert!((centre.0 - 2.0).abs() < 1e-9 && (centre.1 - 48.0).abs() < 1e-9);
    // Near the dateline and the pole the box is shifted, never cropped.
    let edge = local_view_box(85.0, 179.0, 30.0, 400, 200);
    assert!(edge.east <= 180.0 && edge.west >= -180.0 && edge.north <= 90.0);
    assert!((edge.height_degrees() - 30.0).abs() < 1e-9);
    assert!(edge.width_degrees() <= 358.0);
    assert_eq!(span_for_zoom(1), 120.0);
    assert_eq!(span_for_zoom(3), 30.0);
    assert!((span_for_zoom(6) - 3.75).abs() < 1e-9);
    assert_eq!(span_for_zoom(99), span_for_zoom(6));
}

#[test]
fn loop_position_walks_the_frames_blends_and_holds() {
    let fps = 2.0;
    // Live picture: never moves.
    assert_eq!(loop_position(123.0, fps, 1, 0.5), (0, 0, 0.0));
    // 5 frames, no smoothing: cuts at 2 fps.
    let cut = |seconds: f64| loop_position(seconds, fps, 5, 0.0);
    assert_eq!(cut(0.0), (0, 1, 0.0));
    assert_eq!(cut(0.6), (1, 2, 0.0));
    assert_eq!(cut(1.6), (3, 4, 0.0));
    // The newest frame is held (three frame times), then the loop restarts.
    assert_eq!(cut(2.1), (4, 4, 0.0));
    assert_eq!(cut(3.4), (4, 4, 0.0));
    assert_eq!(cut(3.6), (0, 1, 0.0));
    // With smoothing the second half of an interval fades to the next frame.
    let (index, next, early) = loop_position(0.05, fps, 5, 0.5);
    assert_eq!((index, next), (0, 1));
    assert_eq!(early, 0.0);
    let (_, _, late) = loop_position(0.45, fps, 5, 0.5);
    assert!(late > 0.8 && late <= 1.0, "{late}");
    let (_, _, middle) = loop_position(0.375, fps, 5, 0.5);
    assert!(middle > 0.05 && middle < 0.95);
    for step in 0..400 {
        let (a, b, blend) = loop_position(f64::from(step) * 0.037, fps, 7, 0.7);
        assert!(a < 7 && b < 7 && (0.0..=1.0).contains(&blend));
    }
}

#[test]
fn transfer_maps_clouds_bright_and_supports_invert_contrast_and_dimming() {
    let set = synthetic_set(&[1000], |_, _, _| 128);
    let scene = |settings: CloudsSettings| {
        CloudsScene::with_parts(
            &settings,
            &SceneEnv::for_test(PathBuf::from("/x"), crate::resources::test_resources()),
            weather_fetcher(),
            fixed_now(),
            no_land(),
        )
    };
    let normal = scene(CloudsSettings {
        ground_dim_percent: 0,
        ..Default::default()
    });
    let cloud = set.white;
    let ground = set.black;
    assert!(normal.transfer(cloud, &set) > 0.9);
    assert!(normal.transfer(ground, &set) < 0.2);
    let inverted = scene(CloudsSettings {
        invert: true,
        ..Default::default()
    });
    assert!(inverted.transfer(cloud, &set) < 0.2);
    assert!(inverted.transfer(ground, &set) > 0.9);
    let dimmed = scene(CloudsSettings {
        ground_dim_percent: 100,
        contrast_percent: 100,
        ..Default::default()
    });
    let plain = scene(CloudsSettings {
        ground_dim_percent: 0,
        contrast_percent: 100,
        ..Default::default()
    });
    let mid_ground = set.black + 0.2 * (set.white - set.black);
    assert!(dimmed.transfer(mid_ground, &set) < plain.transfer(mid_ground, &set) * 0.5);
    assert!(
        (dimmed.transfer(cloud, &set) - plain.transfer(cloud, &set)).abs() < 0.02,
        "clouds keep their brightness"
    );
    let flat = scene(CloudsSettings {
        contrast_percent: 50,
        ground_dim_percent: 0,
        ..Default::default()
    });
    let bold = scene(CloudsSettings {
        contrast_percent: 300,
        ground_dim_percent: 0,
        ..Default::default()
    });
    let value = set.black + 0.75 * (set.white - set.black);
    assert!(bold.transfer(value, &set) > flat.transfer(value, &set));
    let brighter = scene(CloudsSettings {
        brightness_percent: 30,
        ground_dim_percent: 0,
        ..Default::default()
    });
    assert!(brighter.transfer(ground, &set) > normal.transfer(ground, &set));
}

// --- rendering injected frames -----------------------------------------------------

#[test]
fn injected_frames_render_clouds_where_they_are_and_are_deterministic() {
    let cache = tempfile::tempdir().unwrap();
    let mut scene = scene_with(&CloudsSettings::default(), weather_fetcher(), cache.path());
    render_frame(&mut scene, 100, 25, Duration::ZERO);
    detach(&mut scene);
    // A frame with a bright cloud on the location's longitude, elsewhere ground.
    scene.apply(Update::Frames(synthetic_set(&[1000], |_, lon, lat| {
        if lon.abs() < 6.0 && (lat - 50.0).abs() < 6.0 {
            230
        } else {
            60
        }
    })));
    scene.apply(Update::Progress(None));
    let rendered = render_frame(&mut scene, 100, 25, Duration::ZERO);
    // The set covers lon -30..30, lat 30..70 and is stretched over the raster.
    let at = |lon: f64, lat: f64| {
        let x = ((lon + 30.0) / 60.0 * 200.0) as usize;
        let y = ((70.0 - lat) / 40.0 * 100.0) as usize;
        rendered.raster.dots[y * 200 + x]
    };
    assert!(at(0.0, 50.0) > 0.9, "cloud is bright: {}", at(0.0, 50.0));
    assert!(at(-25.0, 35.0) < 0.3, "ground is dim: {}", at(-25.0, 35.0));
    assert!(rendered
        .raster
        .dots
        .iter()
        .all(|dot| (0.0..=1.0).contains(dot)));
    let again = render_frame(&mut scene, 100, 25, Duration::ZERO);
    assert_eq!(rendered.raster.dots, again.raster.dots);
    assert_eq!(scene.status().as_deref(), Some("Test 1970-01-01 00:16 UTC"));
}

#[test]
fn time_lapse_frames_cycle_with_time_and_label_the_frame_shown() {
    let cache = tempfile::tempdir().unwrap();
    let settings = CloudsSettings {
        history_hours: 6,
        playback_fps: 2,
        smoothing_percent: 0,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, weather_fetcher(), cache.path());
    render_frame(&mut scene, 100, 25, Duration::ZERO);
    detach(&mut scene);
    let times = [3600, 7200, 10_800];
    scene.apply(Update::Frames(synthetic_set(&times, |index, lon, _| {
        // A bright band that moves east one step per frame.
        if (lon - (index as f64 * 15.0 - 15.0)).abs() < 5.0 {
            230
        } else {
            50
        }
    })));
    let first = render_frame(&mut scene, 100, 25, Duration::ZERO);
    assert_eq!(
        scene.status().as_deref(),
        Some("Test 1970-01-01 01:00 UTC (1/3)")
    );
    let second = render_frame(&mut scene, 100, 25, Duration::from_millis(600));
    assert_eq!(
        scene.status().as_deref(),
        Some("Test 1970-01-01 02:00 UTC (2/3)")
    );
    let third = render_frame(&mut scene, 100, 25, Duration::from_millis(1100));
    assert_eq!(
        scene.status().as_deref(),
        Some("Test 1970-01-01 03:00 UTC (3/3)")
    );
    assert_ne!(first.raster.dots, second.raster.dots);
    assert_ne!(second.raster.dots, third.raster.dots);
    // Loop: after the hold the first frame comes back, identically.
    let again = render_frame(
        &mut scene,
        100,
        25,
        Duration::from_millis(500 * (2 + 3) + 100),
    );
    assert_eq!(first.raster.dots, again.raster.dots);
    assert!(scene.frames_per_second() >= 4);
}

#[test]
fn smoothing_cross_fades_between_frames() {
    let cache = tempfile::tempdir().unwrap();
    let settings = CloudsSettings {
        history_hours: 6,
        playback_fps: 1,
        smoothing_percent: 100,
        ground_dim_percent: 0,
        contrast_percent: 100,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, weather_fetcher(), cache.path());
    render_frame(&mut scene, 60, 15, Duration::ZERO);
    detach(&mut scene);
    scene.apply(Update::Frames(synthetic_set(&[100, 200], |index, _, _| {
        if index == 0 {
            40
        } else {
            220
        }
    })));
    let start = mean(&render_frame(&mut scene, 60, 15, Duration::from_millis(10)));
    let middle = mean(&render_frame(
        &mut scene,
        60,
        15,
        Duration::from_millis(500),
    ));
    let end = mean(&render_frame(
        &mut scene,
        60,
        15,
        Duration::from_millis(990),
    ));
    assert!(start < middle && middle < end, "{start} {middle} {end}");
}

#[test]
fn inverted_picture_is_the_complement_and_underlay_and_marker_draw() {
    let cache = tempfile::tempdir().unwrap();
    let base = CloudsSettings {
        ground_dim_percent: 0,
        contrast_percent: 100,
        ..Default::default()
    };
    let set = synthetic_set(&[1000], |_, lon, _| if lon < 0.0 { 240 } else { 20 });
    let render = |settings: &CloudsSettings, land: LandFn, time_ms: u64| {
        let mut scene = CloudsScene::with_parts(
            settings,
            &SceneEnv::for_test(
                cache.path().to_path_buf(),
                crate::resources::test_resources(),
            ),
            weather_fetcher(),
            fixed_now(),
            land,
        );
        render_frame(&mut scene, 80, 20, Duration::ZERO);
        detach(&mut scene);
        scene.apply(Update::Frames(Arc::clone(&set)));
        let rendered = render_frame(&mut scene, 80, 20, Duration::from_millis(time_ms));
        (scene, rendered)
    };
    let (_, normal) = render(&base, no_land(), 0);
    let (_, inverted) = render(
        &CloudsSettings {
            invert: true,
            ..base.clone()
        },
        no_land(),
        0,
    );
    for (a, b) in normal.raster.dots.iter().zip(&inverted.raster.dots) {
        assert!((a + b - 1.0).abs() < 0.01, "{a} {b}");
    }
    // Land underlay lifts the dark half only where the mask says land.
    let (_, plain) = render(&base, no_land(), 0);
    let with_land = CloudsSettings {
        land_underlay: true,
        land_underlay_percent: 40,
        ..base.clone()
    };
    let (_, filled) = render(&with_land, Box::new(|_, _| true), 0);
    let dark_before = plain.raster.dots.iter().filter(|dot| **dot < 0.1).count();
    let dark_after = filled.raster.dots.iter().filter(|dot| **dot < 0.1).count();
    assert!(
        dark_before > 500 && dark_after == 0,
        "{dark_before} {dark_after}"
    );
    let (_, unchanged) = render(&with_land, no_land(), 0);
    assert_eq!(unchanged.raster.dots, plain.raster.dots);
    // Marker blinks.
    let marked = CloudsSettings {
        marker: true,
        ..base.clone()
    };
    let (_, on) = render(&marked, no_land(), 100);
    let (_, off) = render(&marked, no_land(), 900);
    assert!(on.raster.dots.iter().sum::<f32>() != plain.raster.dots.iter().sum::<f32>());
    assert_eq!(off.raster.dots, plain.raster.dots);
}

// --- worker: local live view -------------------------------------------------------

#[test]
fn local_live_view_requests_the_nearest_satellite_and_reports_the_data_time() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = weather_fetcher();
    let mut scene = scene_with(&CloudsSettings::default(), fetcher.clone(), cache.path());
    pump(&mut scene, settled);
    assert_eq!(frame_count(&scene), 1);
    assert_eq!(
        scene.status().as_deref(),
        Some("Meteosat 2026-09-30 13:15 UTC")
    );
    let urls = fetcher.urls();
    assert!(
        urls[0].starts_with("https://view.eumetsat.int/geoserver/msg_fes/ows?"),
        "{}",
        urls[0]
    );
    let map = urls
        .iter()
        .find(|url| url.contains("request=GetMap"))
        .unwrap();
    assert!(map.contains("layers=msg_fes:ir108") && map.contains("time=2026-09-30T13:15:00Z"));
    assert!(map.starts_with("https://view.eumetsat.int/geoserver/ows?"));
    let bbox: Vec<f64> = query(map, "bbox")
        .split(',')
        .map(|value| value.parse().unwrap())
        .collect();
    // 60 degrees of latitude around Greenwich (zoom level 2).
    assert!((bbox[3] - bbox[1] - 60.0).abs() < 1e-3);
    assert!(bbox[0] < 0.0 && bbox[2] > 0.0 && bbox[1] < 51.5 && bbox[3] > 51.5);
    assert_eq!(
        urls.iter()
            .filter(|url| url.contains("request=GetMap"))
            .count(),
        1
    );
    // The storm is visible on the ground.
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert!(rendered.lit_dots() > 30);
    let lines = rendered.braille_lines(100, DitherMode::Ordered);
    assert!(lines.iter().any(|line| line.chars().any(|c| c != ' ')));
}

#[test]
fn explicit_source_overrides_the_automatic_choice() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = weather_fetcher();
    let settings = CloudsSettings {
        source: CloudSource::GoesWest,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
    pump(&mut scene, settled);
    assert!(fetcher
        .urls()
        .iter()
        .any(|url| url.contains("GOES-West_ABI_GeoColor")));
    assert!(fetcher.urls().iter().all(|url| !url.contains("msg_fes")));
    assert!(scene.status().unwrap().starts_with("GOES-West "));
}

#[test]
fn geostationary_tiles_fall_back_to_the_previous_image_and_stay_bounded() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = weather_fetcher();
    let settings = CloudsSettings {
        source: CloudSource::GoesEast,
        zoom_level: 3,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
    pump(&mut scene, settled);
    // 13:00 was listed but not ingested: the 12:50 image is shown.
    assert_eq!(
        scene.status().as_deref(),
        Some("GOES-East 2026-09-30 12:50 UTC")
    );
    let urls = fetcher.urls();
    let domains: Vec<&String> = urls.iter().filter(|url| url.contains("/1.0.0/")).collect();
    assert_eq!(domains.len(), 1);
    assert_eq!(
        domains[0],
        "https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/1.0.0/GOES-East_ABI_GeoColor/default/1km/all/2026-09-29T01:40:00Z--2026-09-30T13:40:00Z.xml"
    );
    let tiles: Vec<&String> = urls
        .iter()
        .filter(|url| url.contains("/1km/"))
        .filter(|url| !url.contains("/1.0.0/"))
        .collect();
    assert!(tiles
        .iter()
        .all(|url| url.contains("/GOES-East_ABI_GeoColor/default/2026-09-30T1")));
    let good_tiles = tiles
        .iter()
        .filter(|url| url.contains("T12:50:00Z"))
        .count();
    let missing_tiles = tiles
        .iter()
        .filter(|url| url.contains("T13:00:00Z"))
        .count();
    assert!(
        (1..=worker::MAX_TILES_PER_FRAME).contains(&good_tiles),
        "{good_tiles}"
    );
    assert!((1..=worker::MAX_TILES_PER_FRAME).contains(&missing_tiles));
    assert!(tiles.iter().all(|url| url.ends_with(".png")));
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert!(rendered.lit_dots() > 20);
}

#[test]
fn colour_tint_fills_every_cell_with_the_satellite_colour() {
    let cache = tempfile::tempdir().unwrap();
    let settings = CloudsSettings {
        source: CloudSource::GoesEast,
        cell_colors: true,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, weather_fetcher(), cache.path());
    assert!(scene.uses_cell_colors());
    pump(&mut scene, settled);
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert_eq!(rendered.cell_colors.len(), 120 * 30);
    assert!(
        rendered.cell_colors.iter().all(|color| *color != [0, 0, 0]),
        "every cell painted"
    );
    // The synthetic GeoColor tiles are red-dominant.
    let reddish = rendered
        .cell_colors
        .iter()
        .filter(|c| c[0] > c[1] && c[0] > c[2])
        .count();
    assert!(reddish > rendered.cell_colors.len() / 2, "{reddish}");
    let mut mono = scene_with(
        &CloudsSettings {
            cell_colors: false,
            ..settings
        },
        weather_fetcher(),
        cache.path(),
    );
    assert!(!mono.uses_cell_colors());
    pump(&mut mono, settled);
}

// --- worker: time-lapse ------------------------------------------------------------

#[test]
fn time_lapse_downloads_a_bounded_aligned_set_and_only_new_frames_later() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = weather_fetcher();
    let settings = CloudsSettings {
        history_hours: 12,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
    let expected = plan_frame_times(
        UtcTime::from_civil(2026, 9, 30, 13, 15, 0),
        12,
        provider(CloudSource::Meteosat).cadence_minutes,
        worker::MAX_FRAMES,
    );
    pump(&mut scene, |scene| {
        frame_count(scene) == expected.len() && scene.progress.is_none()
    });
    let set = scene.set.clone().unwrap();
    assert!(set.frames.len() <= worker::MAX_FRAMES && set.frames.len() >= 12);
    assert!(
        set.frames
            .windows(2)
            .all(|pair| pair[0].time < pair[1].time),
        "oldest first"
    );
    assert_eq!(
        set.frames.last().unwrap().time,
        UtcTime::from_civil(2026, 9, 30, 13, 15, 0)
    );
    let requested: Vec<UtcTime> = fetcher
        .urls()
        .iter()
        .filter(|url| url.contains("request=GetMap"))
        .map(|url| UtcTime::parse(query(url, "time")).unwrap())
        .collect();
    assert_eq!(
        requested.len(),
        expected.len(),
        "one request per frame, no duplicates"
    );
    // Frames really differ (the storm drifts).
    assert_ne!(
        set.frames[0].grid.luma,
        set.frames.last().unwrap().grid.luma
    );
    // Progress was reported while downloading.
    assert!(scene.status().unwrap().starts_with("Meteosat "));
}

#[test]
fn progress_text_counts_frames() {
    let cache = tempfile::tempdir().unwrap();
    let settings = CloudsSettings {
        history_hours: 6,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, weather_fetcher(), cache.path());
    render_frame(&mut scene, 60, 15, Duration::ZERO);
    let mut seen = Vec::new();
    while !settled(&scene) || frame_count(&scene) < 2 || scene.progress.is_some() {
        assert!(scene.wait_for_update(Duration::from_secs(60)));
        if let Some(progress) = &scene.progress {
            seen.push(progress.clone());
        }
    }
    let observed_counts: Vec<(usize, usize)> = seen
        .iter()
        .filter_map(|text| text.strip_prefix("Downloading frame "))
        .filter_map(|count| count.split_once('/'))
        .filter_map(|(completed, total)| Some((completed.parse().ok()?, total.parse().ok()?)))
        .collect();
    assert!(!observed_counts.is_empty(), "{seen:?}");
    assert!(
        observed_counts
            .iter()
            .all(|(completed, total)| *completed > 0 && completed <= total),
        "{observed_counts:?}"
    );
    assert!(
        observed_counts
            .windows(2)
            .all(|pair| { pair[0].0 <= pair[1].0 && pair[0].1 == pair[1].1 }),
        "{observed_counts:?}"
    );
    assert!(
        observed_counts
            .iter()
            .any(|(completed, total)| completed < total),
        "progress should be visible before all frames finish: {observed_counts:?}"
    );
}

// --- worker: global -----------------------------------------------------------------

#[test]
fn global_flat_map_uses_the_world_mosaic_over_the_whole_box() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = weather_fetcher();
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Flat,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
    pump(&mut scene, settled);
    let map = fetcher
        .urls()
        .into_iter()
        .find(|url| url.contains("request=GetMap"))
        .unwrap();
    assert!(map.contains("layers=mumi:worldcloudmap_ir108"));
    assert_eq!(query(&map, "bbox"), "-180.0000,-90.0000,180.0000,90.0000");
    assert_eq!(query(&map, "time"), "2026-09-30T12:00:00Z");
    // 120x30 cells: 240x120 dots, so a 512x256 image is requested (minimum).
    assert_eq!(
        (query(&map, "width"), query(&map, "height")),
        ("512", "256")
    );
    assert_eq!(
        scene.status().as_deref(),
        Some("World IR 2026-09-30 12:00 UTC")
    );
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert!(
        rendered.lit_dots() > 5,
        "the storm is drawn: {}",
        rendered.lit_dots()
    );
}

#[test]
fn global_globe_rotates_and_leaves_the_corners_empty() {
    let cache = tempfile::tempdir().unwrap();
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Globe,
        rotation_deg_per_min: 12,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, weather_fetcher(), cache.path());
    pump(&mut scene, settled);
    let now = render_frame(&mut scene, 100, 40, Duration::ZERO);
    let later = render_frame(&mut scene, 100, 40, Duration::from_secs(600));
    assert_ne!(now.raster.dots, later.raster.dots);
    assert_eq!(now.raster.dots[0], 0.0);
    assert_eq!(*now.raster.dots.last().unwrap(), 0.0);
    assert_eq!(scene.frames_per_second(), 12);
    let again = render_frame(&mut scene, 100, 40, Duration::from_secs(600));
    assert_eq!(later.raster.dots, again.raster.dots);
}

#[test]
fn global_mollweide_renders_an_ellipse() {
    let cache = tempfile::tempdir().unwrap();
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Mollweide,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, weather_fetcher(), cache.path());
    pump(&mut scene, settled);
    let rendered = render_frame(&mut scene, 120, 30, Duration::ZERO);
    assert_eq!(rendered.raster.dots[1], 0.0);
    assert!(
        rendered.lit_dots() > 5,
        "the storm is drawn: {}",
        rendered.lit_dots()
    );
}

// --- failure handling ----------------------------------------------------------------

#[test]
fn a_dead_primary_source_falls_back_to_the_next_one() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = Arc::new(FakeFetcher::new(|url, _| {
        if url.contains("eumetsat.int") {
            return Err(TileError::Network("connection refused".into()));
        }
        if url.contains("MODIS_Terra") {
            return Ok(tile_png(url, false));
        }
        Err(TileError::Missing)
    }));
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, fetcher.clone(), cache.path());
    pump(&mut scene, settled);
    assert_eq!(
        scene.status().as_deref(),
        Some("MODIS Terra 2026-09-30 00:00 UTC")
    );
    let modis: Vec<String> = fetcher
        .urls()
        .into_iter()
        .filter(|url| url.contains("MODIS_Terra"))
        .collect();
    assert!(modis.len() <= worker::MAX_TILES_PER_FRAME + 1);
    assert!(modis.iter().all(|url| url.contains("/250m/")
        && url.ends_with(".jpeg")
        && url.contains("/2026-09-30/")));
}

#[test]
fn without_cache_or_network_status_and_placeholder_explain_the_situation() {
    let cache = tempfile::tempdir().unwrap();
    let offline = Arc::new(FakeFetcher::new(|_, _| {
        Err(TileError::Network("dns error".into()))
    }));
    let mut scene = scene_with(&CloudsSettings::default(), offline, cache.path());
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
    assert!(lit > 100 && brightest < 0.5, "{lit} {brightest}");
}

#[test]
fn an_old_frame_stays_visible_when_the_network_goes_away() {
    let cache = tempfile::tempdir().unwrap();
    let online = Arc::new(AtomicBool::new(true));
    let switch = Arc::clone(&online);
    let inner = weather_fetcher();
    let fetcher = Arc::new(FakeFetcher::new(move |url, stop| {
        if switch.load(Ordering::SeqCst) {
            (inner.respond)(url, stop)
        } else {
            Err(TileError::Network("unreachable".into()))
        }
    }));
    let mut scene = scene_with(&CloudsSettings::default(), fetcher, cache.path());
    pump(&mut scene, settled);
    let before = render_frame(&mut scene, 60, 15, Duration::ZERO);
    online.store(false, Ordering::SeqCst);
    // A second scene over the same cache dir would re-fetch; here the held set stays.
    scene.apply(Update::Offline("unreachable".into()));
    let status = scene.status().unwrap();
    assert!(
        status.starts_with("Offline (unreachable); showing Meteosat 2026-09-30 13:15 UTC"),
        "{status}"
    );
    let after = render_frame(&mut scene, 60, 15, Duration::ZERO);
    assert_eq!(before.raster.dots, after.raster.dots);
}

#[test]
fn dropping_the_scene_stops_the_worker_and_render_never_blocks() {
    let cache = tempfile::tempdir().unwrap();
    let exited = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let (exit_flag, enter_flag) = (Arc::clone(&exited), Arc::clone(&entered));
    let hanging = Arc::new(FakeFetcher::new(move |_, stop| {
        enter_flag.store(true, Ordering::SeqCst);
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(2));
        }
        exit_flag.store(true, Ordering::SeqCst);
        Err(TileError::Stopped)
    }));
    let mut scene = scene_with(
        &CloudsSettings {
            history_hours: 24,
            ..Default::default()
        },
        hanging,
        cache.path(),
    );
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
    let ticket = scene.worker.as_ref().unwrap().join_observer().unwrap();
    let dropping = std::time::Instant::now();
    drop(scene);
    assert!(dropping.elapsed() < Duration::from_secs(1));
    assert_eq!(
        ticket
            .join_until(std::time::Instant::now() + Duration::from_secs(2))
            .unwrap(),
        ilium_platform::owned_worker::WorkerExit::Joined
    );
    assert!(
        exited.load(Ordering::SeqCst),
        "supervisor joined the stopped worker"
    );
}

#[test]
fn a_dead_scene_needs_no_size_to_be_built() {
    // Constructing and dropping without ever rendering starts no thread.
    let cache = tempfile::tempdir().unwrap();
    let scene = scene_with(&CloudsSettings::default(), weather_fetcher(), cache.path());
    assert!(scene.worker.is_none());
    assert_eq!(
        scene.status().as_deref(),
        Some("Preparing satellite imagery")
    );
    assert_eq!(scene.frames_per_second(), 2);
}

// --- looks ---------------------------------------------------------------------------

/// Uses `net/msg_ir.png`, a real EUMETView image saved by hand, as the
/// Meteosat frame. Set `ILIUM_CLOUD_FIXTURE` to another PNG to look at it.
#[test]
#[ignore = "visual check that needs a real image saved on disk"]
fn print_real_clouds() {
    let path = std::env::var("ILIUM_CLOUD_FIXTURE").unwrap_or_else(|_| {
        "/tmp/claude-1000/-home-arthur-dev-ai-ilium/381717b8-0831-4e69-8dfe-96eb7c8f00bb/scratchpad/satellite/net/msg_ir.png".to_owned()
    });
    let bytes = std::fs::read(path).unwrap();
    let inner = weather_fetcher();
    let fetcher = Arc::new(FakeFetcher::new(move |url, stop| {
        if url.contains("request=GetMap") {
            Ok(bytes.clone())
        } else {
            (inner.respond)(url, stop)
        }
    }));
    let location = crate::location::GeoLocation::new("Bay of Biscay", 45.0, 0.0);
    for (name, settings) in [
        ("default", CloudsSettings::default()),
        (
            "more contrast, less ground",
            CloudsSettings {
                contrast_percent: 180,
                ground_dim_percent: 80,
                ..Default::default()
            },
        ),
        (
            "inverted",
            CloudsSettings {
                invert: true,
                ..Default::default()
            },
        ),
        (
            "global globe",
            CloudsSettings {
                coverage: Coverage::Global,
                projection: Projection::Globe,
                rotation_deg_per_min: 0,
                marker: true,
                ..Default::default()
            },
        ),
    ] {
        let cache = tempfile::tempdir().unwrap();
        let env = SceneEnv {
            resources: crate::resources::test_resources(),
            location: location.clone(),
            cache_dir: cache.path().to_path_buf(),
            gpu: None,
            saved_runtime: std::sync::Arc::new(crate::minecraft::saved_runtime::SavedRuntime::new()),
            palette: Default::default(),
        };
        let mut scene =
            CloudsScene::with_parts(&settings, &env, fetcher.clone(), fixed_now(), no_land());
        pump(&mut scene, settled);
        let rendered = render_frame(&mut scene, 110, 30, Duration::from_millis(100));
        println!("=== {name}: {}", scene.status().unwrap());
        for line in rendered.braille_lines(100, DitherMode::Ordered) {
            println!("{line}");
        }
    }
}

/// Talks to the real EUMETView WMS and NASA GIBS (a few dozen requests).
#[test]
#[ignore = "uses the network"]
fn live_clouds_from_eumetsat_and_gibs() {
    for (name, settings, location) in [
        (
            "Meteosat 6 h loop",
            CloudsSettings {
                history_hours: 6,
                ..Default::default()
            },
            crate::location::GeoLocation::default(),
        ),
        (
            "GOES-East live",
            CloudsSettings::default(),
            crate::location::GeoLocation::new("New York", 40.7, -74.0),
        ),
        (
            "world IR globe",
            CloudsSettings {
                coverage: Coverage::Global,
                ..Default::default()
            },
            crate::location::GeoLocation::default(),
        ),
    ] {
        let cache = tempfile::tempdir().unwrap();
        let env = SceneEnv {
            resources: crate::resources::test_resources(),
            location,
            cache_dir: cache.path().to_path_buf(),
            gpu: None,
            saved_runtime: std::sync::Arc::new(crate::minecraft::saved_runtime::SavedRuntime::new()),
            palette: Default::default(),
        };
        let mut scene = CloudsScene::new(&settings, &env);
        let want = if settings.history_hours > 0 { 6 } else { 1 };
        pump(&mut scene, |scene| {
            (frame_count(scene) >= want && scene.progress.is_none()) || scene.problem.is_some()
        });
        println!("=== {name}: {:?}", scene.status());
        let rendered = render_frame(&mut scene, 110, 26, Duration::from_millis(100));
        for line in rendered.braille_lines(100, DitherMode::Ordered) {
            println!("{line}");
        }
        assert!(scene.set.is_some(), "{name}: {:?}", scene.status());
    }
}

#[test]
fn scene_follows_the_provided_palette_natively() {
    let cache = tempfile::tempdir().expect("tempdir");
    let settings = CloudsSettings {
        source: CloudSource::GoesEast,
        cell_colors: true,
        ..Default::default()
    };
    let mut plain = scene_with(&settings, weather_fetcher(), cache.path());
    pump(&mut plain, settled);
    let plain_colors = render_frame(&mut plain, 60, 20, Duration::ZERO).cell_colors;
    let mut env = SceneEnv::for_test(
        cache.path().to_path_buf(),
        crate::resources::test_resources(),
    );
    env.palette = crate::style::ScenePalette {
        stops: vec![[0, 40, 0], [0, 255, 120]],
        ..Default::default()
    };
    let mut themed =
        CloudsScene::with_parts(&settings, &env, weather_fetcher(), fixed_now(), no_land());
    assert!(themed.follows_palette());
    pump(&mut themed, settled);
    let themed_colors = render_frame(&mut themed, 60, 20, Duration::ZERO).cell_colors;
    assert_ne!(plain_colors, themed_colors);
    themed.set_palette(&crate::style::ScenePalette::default());
    let restored = render_frame(&mut themed, 60, 20, Duration::ZERO).cell_colors;
    assert_eq!(plain_colors, restored);
}
