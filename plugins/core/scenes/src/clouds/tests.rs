use super::providers::{plan_frame_times, provider};
use super::refresh::{Answer, Job, Refresher, Transport, Update, MAX_FRAMES, MAX_TILES_PER_FRAME};
use super::*;
use crate::kit::debug::Rendered;
use crate::kit::geo_tiles::{tile_bounds, GeoGrid, TileId, UtcTime};
use crate::kit::raster::{DitherMode, Raster};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

// --- fake network and clock ----------------------------------------------------------

type Respond = Arc<dyn Fn(&str) -> Answer + Send + Sync>;
type Urls = Arc<Mutex<Vec<String>>>;

/// A transport answering every request at once from `respond`, logging the urls asked.
struct FakeTransport {
    respond: Respond,
    urls: Urls,
    ready: HashMap<u64, Answer>,
    next: u64,
}

impl FakeTransport {
    fn new(respond: impl Fn(&str) -> Answer + Send + Sync + 'static) -> (Box<Self>, Urls) {
        let urls = Urls::default();
        let transport = Box::new(Self {
            respond: Arc::new(respond),
            urls: Arc::clone(&urls),
            ready: HashMap::new(),
            next: 1,
        });
        (transport, urls)
    }
}

impl Transport for FakeTransport {
    fn start(&mut self, url: &str) -> Result<u64, String> {
        self.urls.lock().unwrap().push(url.to_owned());
        let id = self.next;
        self.next += 1;
        self.ready.insert(id, (self.respond)(url));
        Ok(id)
    }

    fn take(&mut self, id: u64) -> Option<Answer> {
        self.ready.remove(&id)
    }
}

/// A transport that takes requests and never answers them.
struct Silent;

impl Transport for Silent {
    fn start(&mut self, _url: &str) -> Result<u64, String> {
        Ok(1)
    }

    fn take(&mut self, _id: u64) -> Option<Answer> {
        None
    }
}

fn urls_of(urls: &Urls) -> Vec<String> {
    urls.lock().unwrap().clone()
}

/// 2026-09-30 13:40:24 UTC
const NOW_SECONDS: u64 = 1_790_775_624;

fn now_at(extra_seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECONDS + extra_seconds)
}

/// Render one frame at animation time `time` and civil time `now`.
fn render_at(
    scene: &mut CloudsScene,
    width: u16,
    height: u16,
    time: Duration,
    now: SystemTime,
) -> Rendered {
    let mut raster = Raster::default();
    raster.resize(usize::from(width) * 2, usize::from(height) * 4);
    let mut cell_colors = vec![[0, 0, 0]; usize::from(width) * usize::from(height)];
    {
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut cell_colors,
            width,
            height,
            time,
            wall: time,
            now,
        };
        scene.render(&mut frame);
    }
    Rendered {
        width,
        height,
        raster,
        cell_colors,
    }
}

fn render(scene: &mut CloudsScene, width: u16, height: u16, time: Duration) -> Rendered {
    render_at(scene, width, height, time, now_at(0))
}

fn greenwich() -> SceneEnv {
    SceneEnv::for_test().with_location(SceneLocation {
        latitude: 51.4779,
        longitude: -0.0015,
    })
}

fn no_land() -> LandFn {
    Box::new(|_, _| false)
}

fn scene_with(settings: &CloudsSettings, transport: Box<dyn Transport>) -> CloudsScene {
    CloudsScene::with_parts(settings, &greenwich(), transport, no_land())
}

/// A scene whose supply never hears back, for tests that inject frames.
fn quiet_scene(settings: &CloudsSettings) -> CloudsScene {
    scene_with(settings, Box::new(Silent))
}

/// Render (advancing the supply one step each time) until `done` holds.
fn pump(scene: &mut CloudsScene, done: impl Fn(&CloudsScene) -> bool) {
    for _ in 0..3000 {
        render(scene, 120, 30, Duration::ZERO);
        if done(scene) {
            return;
        }
    }
    panic!("the supply did not settle (status {:?})", scene.status());
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

fn lit_dots(rendered: &Rendered) -> usize {
    rendered
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.5)
        .count()
}

// --- synthetic weather ---------------------------------------------------------------

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
fn weather(url: &str) -> Answer {
    if url.contains("GetCapabilities") {
        return Answer::Body(if url.contains("/msg_fes/") {
            capabilities("msg_fes:ir108", "2026-09-30T13:15:00Z")
        } else if url.contains("/mumi/") {
            capabilities("mumi:worldcloudmap_ir108", "2026-09-30T12:00:00Z")
        } else {
            capabilities("msg_iodc:ir108", "2026-09-30T13:15:00Z")
        });
    }
    if url.contains("request=GetMap") {
        return Answer::Body(wms_map_png(url));
    }
    if url.contains("/1.0.0/") {
        return Answer::Body(b"<Domains><DimensionDomain><Domain>2026-09-30T09:00:00Z/2026-09-30T13:00:00Z/PT10M</Domain></DimensionDomain></Domains>".to_vec());
    }
    if url.contains("/default/2026-09-30T13:00:00Z/") {
        return Answer::Status(404);
    }
    Answer::Body(tile_png(url, url.contains("GeoColor")))
}

fn weather_transport() -> (Box<FakeTransport>, Urls) {
    FakeTransport::new(weather)
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

/// Gives a quiet scene a frame set and clears the progress text.
fn inject(scene: &mut CloudsScene, set: Arc<FrameSet>) {
    render(scene, 100, 25, Duration::ZERO);
    scene.apply(Update::Frames(set));
    scene.apply(Update::Progress(None));
}

// --- settings ------------------------------------------------------------------------

#[test]
fn defaults_are_valid_and_serde_fills_missing_keys() {
    let defaults = CloudsSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    assert_eq!(
        serde_json::from_str::<CloudsSettings>("{}").unwrap(),
        defaults
    );
    let partial: CloudsSettings = serde_json::from_str(
        r#"{"coverage":"global","source":"goes_west","history":"6h","zoom_level":99,"projection":"mollweide"}"#,
    )
    .unwrap();
    assert_eq!(partial.coverage, Coverage::Global);
    assert_eq!(partial.source, CloudSource::GoesWest);
    assert_eq!(partial.history, History::SixHours);
    let normal = partial.normalized();
    assert_eq!(normal.zoom_level, 6);
    let json = serde_json::to_string(&defaults).unwrap();
    for key in [
        "coverage",
        "source",
        "history",
        "playback_fps",
        "refresh_minutes",
        "cell_colors",
    ] {
        assert!(json.contains(&format!("\"{key}\"")), "{key}");
    }
    assert_eq!(History::TwentyFourHours.hours(), 24);
    assert_eq!(History::Latest.hours(), 0);
}

#[test]
fn normalization_clamps_every_field() {
    let wild = CloudsSettings {
        rotation_deg_per_min: 999,
        zoom_level: 0,
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
            wild.playback_fps,
            wild.smoothing_percent,
            wild.refresh_minutes,
            wild.contrast_percent,
            wild.brightness_percent,
            wild.ground_dim_percent,
            wild.land_underlay_percent
        ),
        (30, 1, 1, 100, 5, 50, 50, 0, 60)
    );
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
    let everything = CloudsSettings {
        coverage: Coverage::Global,
        history: History::TwelveHours,
        land_underlay: true,
        ..Default::default()
    };
    let global = ids(&everything);
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
        settings.set_control("history", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.history, History::TwelveHours);
    assert_eq!(
        settings.set_control("history", ControlValue::Index(2)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("history", ControlValue::Index(77)),
        Ok(true)
    );
    assert_eq!(settings.history, History::Latest);
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

#[test]
fn the_manifest_rows_match_the_scene_controls() {
    // Local coverage carries the zoom row, global coverage the projection and rotation rows;
    // the manifest lists both, so the check runs on the union of the two row sets.
    let global = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Globe,
        history: History::TwelveHours,
        land_underlay: true,
        ..Default::default()
    };
    let mut rows = global.controls();
    rows.extend(
        CloudsSettings::default()
            .controls()
            .into_iter()
            .filter(|row| row.id == "zoom_level"),
    );
    crate::kit::settings::assert_schema_matches_controls("satellite-clouds", &rows);
}

#[test]
fn a_spec_gives_clamped_settings_and_a_working_scene() {
    let spec = serde_json::json!({
        "settings": { "zoom_level": 99, "history": "24h", "coverage": "global" },
        "location": { "latitude": 10.0, "longitude": 20.0 }
    });
    let settings: CloudsSettings = crate::kit::settings::from_spec(&spec);
    assert_eq!(settings.zoom_level, 6);
    assert_eq!(settings.history, History::TwentyFourHours);
    assert_eq!(settings.coverage, Coverage::Global);
    let env = SceneEnv::for_test().with_location(SceneLocation::from_spec(&spec));
    let scene = create(&spec, &env);
    assert_eq!(scene.frames_per_second(), 12);
    assert_eq!(
        scene.status().as_deref(),
        Some("Preparing satellite imagery")
    );
}

// --- geometry and timing ---------------------------------------------------------------

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
fn local_cloud_grid_is_bounded_for_extreme_terminal_aspect_ratios() {
    for (dots_w, dots_h) in [(4000, 8), (8, 4000), (1, 1), (2000, 2000)] {
        let (width, height) = local_grid_size(dots_w, dots_h);
        assert!(width >= 64 && height >= 32, "{width}x{height}");
        assert!(width * height <= MAX_CLOUD_GRID_PIXELS, "{width}x{height}");
    }
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
    let scene = |settings: CloudsSettings| quiet_scene(&settings);
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

// --- rendering injected frames -----------------------------------------------------------

#[test]
fn injected_frames_render_clouds_where_they_are_and_are_deterministic() {
    let mut scene = quiet_scene(&CloudsSettings::default());
    // A frame with a bright cloud on the location's longitude, elsewhere ground.
    inject(
        &mut scene,
        synthetic_set(&[1000], |_, lon, lat| {
            if lon.abs() < 6.0 && (lat - 50.0).abs() < 6.0 {
                230
            } else {
                60
            }
        }),
    );
    let rendered = render(&mut scene, 100, 25, Duration::ZERO);
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
    let again = render(&mut scene, 100, 25, Duration::ZERO);
    assert_eq!(rendered.raster.dots, again.raster.dots);
    assert_eq!(scene.status().as_deref(), Some("Test 1970-01-01 00:16 UTC"));
}

#[test]
fn time_lapse_frames_cycle_with_time_and_label_the_frame_shown() {
    let settings = CloudsSettings {
        history: History::SixHours,
        playback_fps: 2,
        smoothing_percent: 0,
        ..Default::default()
    };
    let mut scene = quiet_scene(&settings);
    let times = [3600, 7200, 10_800];
    inject(
        &mut scene,
        synthetic_set(&times, |index, lon, _| {
            // A bright band that moves east one step per frame.
            if (lon - (index as f64 * 15.0 - 15.0)).abs() < 5.0 {
                230
            } else {
                50
            }
        }),
    );
    let first = render(&mut scene, 100, 25, Duration::ZERO);
    assert_eq!(
        scene.status().as_deref(),
        Some("Test 1970-01-01 01:00 UTC (1/3)")
    );
    let second = render(&mut scene, 100, 25, Duration::from_millis(600));
    assert_eq!(
        scene.status().as_deref(),
        Some("Test 1970-01-01 02:00 UTC (2/3)")
    );
    let third = render(&mut scene, 100, 25, Duration::from_millis(1100));
    assert_eq!(
        scene.status().as_deref(),
        Some("Test 1970-01-01 03:00 UTC (3/3)")
    );
    assert_ne!(first.raster.dots, second.raster.dots);
    assert_ne!(second.raster.dots, third.raster.dots);
    // Loop: after the hold the first frame comes back, identically.
    let again = render(
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
    let settings = CloudsSettings {
        history: History::SixHours,
        playback_fps: 1,
        smoothing_percent: 100,
        ground_dim_percent: 0,
        contrast_percent: 100,
        ..Default::default()
    };
    let mut scene = quiet_scene(&settings);
    inject(
        &mut scene,
        synthetic_set(&[100, 200], |index, _, _| if index == 0 { 40 } else { 220 }),
    );
    let start = mean(&render(&mut scene, 60, 15, Duration::from_millis(10)));
    let middle = mean(&render(&mut scene, 60, 15, Duration::from_millis(500)));
    let end = mean(&render(&mut scene, 60, 15, Duration::from_millis(990)));
    assert!(start < middle && middle < end, "{start} {middle} {end}");
}

#[test]
fn inverted_picture_is_the_complement_and_underlay_and_marker_draw() {
    let base = CloudsSettings {
        ground_dim_percent: 0,
        contrast_percent: 100,
        ..Default::default()
    };
    let set = synthetic_set(&[1000], |_, lon, _| if lon < 0.0 { 240 } else { 20 });
    let draw = |settings: &CloudsSettings, land: LandFn, time_ms: u64| {
        let mut scene = CloudsScene::with_parts(settings, &greenwich(), Box::new(Silent), land);
        inject(&mut scene, Arc::clone(&set));
        render(&mut scene, 80, 20, Duration::from_millis(time_ms))
    };
    let normal = draw(&base, no_land(), 0);
    let inverted = draw(
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
    let plain = draw(&base, no_land(), 0);
    let with_land = CloudsSettings {
        land_underlay: true,
        land_underlay_percent: 40,
        ..base.clone()
    };
    let filled = draw(&with_land, Box::new(|_, _| true), 0);
    let dark_before = plain.raster.dots.iter().filter(|dot| **dot < 0.1).count();
    let dark_after = filled.raster.dots.iter().filter(|dot| **dot < 0.1).count();
    assert!(
        dark_before > 500 && dark_after == 0,
        "{dark_before} {dark_after}"
    );
    let unchanged = draw(&with_land, no_land(), 0);
    assert_eq!(unchanged.raster.dots, plain.raster.dots);
    // Marker blinks.
    let marked = CloudsSettings {
        marker: true,
        ..base.clone()
    };
    let on = draw(&marked, no_land(), 100);
    let off = draw(&marked, no_land(), 900);
    assert!(on.raster.dots.iter().sum::<f32>() != plain.raster.dots.iter().sum::<f32>());
    assert_eq!(off.raster.dots, plain.raster.dots);
}

// --- the supply: local live view -------------------------------------------------------

#[test]
fn local_live_view_requests_the_nearest_satellite_and_reports_the_data_time() {
    let (transport, urls) = weather_transport();
    let mut scene = scene_with(&CloudsSettings::default(), transport);
    pump(&mut scene, settled);
    assert_eq!(frame_count(&scene), 1);
    assert_eq!(
        scene.status().as_deref(),
        Some("Meteosat 2026-09-30 13:15 UTC")
    );
    let urls = urls_of(&urls);
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
    let rendered = render(&mut scene, 120, 30, Duration::ZERO);
    assert!(lit_dots(&rendered) > 30);
    let lines = rendered.braille_lines(100, DitherMode::Ordered);
    assert!(lines.iter().any(|line| line.chars().any(|c| c != ' ')));
}

#[test]
fn explicit_source_overrides_the_automatic_choice() {
    let (transport, urls) = weather_transport();
    let settings = CloudsSettings {
        source: CloudSource::GoesWest,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    pump(&mut scene, settled);
    let urls = urls_of(&urls);
    assert!(urls
        .iter()
        .any(|url| url.contains("GOES-West_ABI_GeoColor")));
    assert!(urls.iter().all(|url| !url.contains("msg_fes")));
    assert!(scene.status().unwrap().starts_with("GOES-West "));
}

#[test]
fn geostationary_tiles_fall_back_to_the_previous_image_and_stay_bounded() {
    let (transport, urls) = weather_transport();
    let settings = CloudsSettings {
        source: CloudSource::GoesEast,
        zoom_level: 3,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    pump(&mut scene, settled);
    // 13:00 was listed but not ingested: the 12:50 image is shown.
    assert_eq!(
        scene.status().as_deref(),
        Some("GOES-East 2026-09-30 12:50 UTC")
    );
    let urls = urls_of(&urls);
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
        (1..=MAX_TILES_PER_FRAME).contains(&good_tiles),
        "{good_tiles}"
    );
    assert!((1..=MAX_TILES_PER_FRAME).contains(&missing_tiles));
    assert!(tiles.iter().all(|url| url.ends_with(".png")));
    let rendered = render(&mut scene, 120, 30, Duration::ZERO);
    assert!(lit_dots(&rendered) > 20);
}

#[test]
fn colour_tint_fills_every_cell_with_the_satellite_colour() {
    let (transport, _) = weather_transport();
    let settings = CloudsSettings {
        source: CloudSource::GoesEast,
        cell_colors: true,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    assert!(scene.uses_cell_colors());
    pump(&mut scene, settled);
    let rendered = render(&mut scene, 120, 30, Duration::ZERO);
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
    let (mono_transport, _) = weather_transport();
    let mut mono = scene_with(
        &CloudsSettings {
            cell_colors: false,
            ..settings
        },
        mono_transport,
    );
    assert!(!mono.uses_cell_colors());
    pump(&mut mono, settled);
}

// --- the supply: time-lapse --------------------------------------------------------------

#[test]
fn time_lapse_downloads_a_bounded_aligned_set() {
    let (transport, urls) = weather_transport();
    let settings = CloudsSettings {
        history: History::TwelveHours,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    let expected = plan_frame_times(
        UtcTime::from_civil(2026, 9, 30, 13, 15, 0),
        12,
        provider(CloudSource::Meteosat).cadence_minutes,
        MAX_FRAMES,
    );
    pump(&mut scene, |scene| {
        frame_count(scene) == expected.len() && scene.progress.is_none()
    });
    let set = scene.set.clone().unwrap();
    assert!(set.frames.len() <= MAX_FRAMES && set.frames.len() >= 12);
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
    let requested: Vec<UtcTime> = urls_of(&urls)
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
    assert!(scene.status().unwrap().starts_with("Meteosat "));
}

fn local_job(history_hours: i64) -> Job {
    Job {
        primary: CloudSource::Meteosat,
        bbox: local_view_box(51.4779, -0.0015, 60.0, 240, 120),
        grid_width: 240,
        grid_height: 120,
        want_rgb: false,
        history_hours,
        refresh: Duration::from_secs(20 * 60),
    }
}

/// Polls the supply once per call, as a scene does, collecting everything it reports.
fn run_supply(
    refresher: &mut Refresher,
    now_ms: i64,
    steps: usize,
    mut stop: impl FnMut(&[Update]) -> bool,
) -> Vec<Update> {
    let mut all = Vec::new();
    for _ in 0..steps {
        refresher.poll(now_ms);
        all.extend(refresher.take_updates());
        if stop(&all) {
            break;
        }
    }
    all
}

fn settled_updates(updates: &[Update]) -> bool {
    updates
        .iter()
        .rev()
        .find_map(|update| match update {
            Update::Progress(text) => Some(text.is_none()),
            _ => None,
        })
        .unwrap_or(false)
}

#[test]
fn progress_text_counts_frames_and_each_step_decodes_at_most_one_frame() {
    let (transport, _) = weather_transport();
    let mut refresher = Refresher::with_transport(local_job(6), transport);
    let now_ms = (NOW_SECONDS * 1000) as i64;
    let mut frames_per_call = Vec::new();
    let mut seen = Vec::new();
    let mut all = Vec::new();
    for _ in 0..200 {
        refresher.poll(now_ms);
        let updates = refresher.take_updates();
        frames_per_call.push(
            updates
                .iter()
                .filter(|update| matches!(update, Update::Frames(_)))
                .count(),
        );
        all.extend(updates);
        if settled_updates(&all) {
            break;
        }
    }
    for update in &all {
        if let Update::Progress(Some(text)) = update {
            seen.push(text.clone());
        }
    }
    assert!(settled_updates(&all), "the supply finished");
    assert!(
        frames_per_call.iter().all(|count| *count <= 1),
        "{frames_per_call:?}"
    );
    let counts: Vec<(usize, usize)> = seen
        .iter()
        .filter_map(|text| text.strip_prefix("Downloading frame "))
        .filter_map(|count| count.split_once('/'))
        .filter_map(|(completed, total)| Some((completed.parse().ok()?, total.parse().ok()?)))
        .collect();
    assert!(!counts.is_empty(), "{seen:?}");
    assert!(
        counts
            .iter()
            .all(|(completed, total)| *completed > 0 && completed <= total),
        "{counts:?}"
    );
    assert!(
        counts
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 == pair[1].1),
        "{counts:?}"
    );
    assert!(
        counts.iter().any(|(completed, total)| completed < total),
        "progress is visible before all frames finish: {counts:?}"
    );
}

#[test]
fn the_supply_checks_again_after_the_refresh_interval_and_fetches_only_new_frames() {
    let (transport, urls) = weather_transport();
    let mut refresher = Refresher::with_transport(local_job(6), transport);
    let first = (NOW_SECONDS * 1000) as i64;
    let all = run_supply(&mut refresher, first, 200, settled_updates);
    assert!(settled_updates(&all));
    let after_first = urls_of(&urls).len();
    // Before the interval is over nothing is requested.
    run_supply(&mut refresher, first + 19 * 60 * 1000, 5, |_| false);
    assert_eq!(urls_of(&urls).len(), after_first);
    // Afterwards the catalogue is asked again, and the frames already held are not.
    let later = first + 21 * 60 * 1000;
    let all = run_supply(&mut refresher, later, 200, settled_updates);
    assert!(settled_updates(&all));
    let requests = urls_of(&urls);
    assert!(requests.len() > after_first);
    let maps = requests
        .iter()
        .filter(|url| url.contains("request=GetMap"))
        .count();
    let distinct: std::collections::HashSet<&str> = requests
        .iter()
        .filter(|url| url.contains("request=GetMap"))
        .map(|url| query(url, "time"))
        .collect();
    assert_eq!(maps, distinct.len(), "no frame is requested twice");
}

// --- the supply: global ----------------------------------------------------------------

#[test]
fn global_flat_map_uses_the_world_mosaic_over_the_whole_box() {
    let (transport, urls) = weather_transport();
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Flat,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    pump(&mut scene, settled);
    let map = urls_of(&urls)
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
    let rendered = render(&mut scene, 120, 30, Duration::ZERO);
    assert!(
        lit_dots(&rendered) > 5,
        "the storm is drawn: {}",
        lit_dots(&rendered)
    );
}

#[test]
fn global_globe_rotates_and_leaves_the_corners_empty() {
    let (transport, _) = weather_transport();
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Globe,
        rotation_deg_per_min: 12,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    pump(&mut scene, settled);
    let now = render(&mut scene, 100, 40, Duration::ZERO);
    let later = render(&mut scene, 100, 40, Duration::from_secs(600));
    assert_ne!(now.raster.dots, later.raster.dots);
    assert_eq!(now.raster.dots[0], 0.0);
    assert_eq!(*now.raster.dots.last().unwrap(), 0.0);
    assert_eq!(scene.frames_per_second(), 12);
    let again = render(&mut scene, 100, 40, Duration::from_secs(600));
    assert_eq!(later.raster.dots, again.raster.dots);
}

#[test]
fn global_mollweide_renders_an_ellipse() {
    let (transport, _) = weather_transport();
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        projection: Projection::Mollweide,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    pump(&mut scene, settled);
    let rendered = render(&mut scene, 120, 30, Duration::ZERO);
    assert_eq!(rendered.raster.dots[1], 0.0);
    assert!(
        lit_dots(&rendered) > 5,
        "the storm is drawn: {}",
        lit_dots(&rendered)
    );
}

// --- failure handling ------------------------------------------------------------------

#[test]
fn a_dead_primary_source_falls_back_to_the_next_one() {
    let (transport, urls) = FakeTransport::new(|url| {
        if url.contains("eumetsat.int") {
            return Answer::Status(503);
        }
        if url.contains("MODIS_Terra") {
            return Answer::Body(tile_png(url, false));
        }
        Answer::Status(404)
    });
    let settings = CloudsSettings {
        coverage: Coverage::Global,
        ..Default::default()
    };
    let mut scene = scene_with(&settings, transport);
    pump(&mut scene, settled);
    assert_eq!(
        scene.status().as_deref(),
        Some("MODIS Terra 2026-09-30 00:00 UTC")
    );
    let modis: Vec<String> = urls_of(&urls)
        .into_iter()
        .filter(|url| url.contains("MODIS_Terra"))
        .collect();
    assert!(modis.len() <= MAX_TILES_PER_FRAME + 1);
    assert!(modis.iter().all(|url| url.contains("/250m/")
        && url.ends_with(".jpeg")
        && url.contains("/2026-09-30/")));
}

#[test]
fn without_network_status_and_placeholder_explain_the_situation() {
    let (transport, _) = FakeTransport::new(|_| Answer::Status(503));
    let mut scene = scene_with(&CloudsSettings::default(), transport);
    pump(&mut scene, |scene| scene.problem.is_some());
    let status = scene.status().unwrap();
    assert!(
        status.contains("No imagery yet") && status.contains("HTTP 503"),
        "{status}"
    );
    let rendered = render(&mut scene, 100, 25, Duration::ZERO);
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
    let online = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let switch = Arc::clone(&online);
    let (transport, _) = FakeTransport::new(move |url| {
        if switch.load(std::sync::atomic::Ordering::SeqCst) {
            weather(url)
        } else {
            Answer::Status(503)
        }
    });
    let mut scene = scene_with(&CloudsSettings::default(), transport);
    pump(&mut scene, settled);
    let before = render(&mut scene, 60, 15, Duration::ZERO);
    online.store(false, std::sync::atomic::Ordering::SeqCst);
    // Past the refresh interval the supply asks again and finds the network gone.
    for _ in 0..50 {
        render_at(&mut scene, 60, 15, Duration::ZERO, now_at(21 * 60));
        if scene.problem.is_some() {
            break;
        }
    }
    let status = scene.status().unwrap();
    assert!(
        status.starts_with("Offline (HTTP 503); showing Meteosat 2026-09-30 13:15 UTC"),
        "{status}"
    );
    let after = render(&mut scene, 60, 15, Duration::ZERO);
    assert_eq!(before.raster.dots, after.raster.dots);
}

#[test]
fn a_request_the_host_never_answers_is_given_up() {
    let mut scene = quiet_scene(&CloudsSettings::default());
    render_at(&mut scene, 100, 25, Duration::ZERO, now_at(0));
    assert!(scene.problem.is_none(), "still waiting");
    for step in 1..=20 {
        render_at(&mut scene, 100, 25, Duration::ZERO, now_at(step * 10));
        if scene.problem.is_some() {
            break;
        }
    }
    let status = scene.status().unwrap();
    assert!(
        status.contains("No imagery yet") && status.contains("timed out"),
        "{status}"
    );
}

#[test]
fn a_scene_needs_no_size_to_be_built_and_asks_for_nothing_until_rendered() {
    let (transport, urls) = weather_transport();
    let scene = scene_with(&CloudsSettings::default(), transport);
    assert!(scene.refresher.is_none());
    assert!(urls_of(&urls).is_empty());
    assert_eq!(
        scene.status().as_deref(),
        Some("Preparing satellite imagery")
    );
    assert_eq!(scene.frames_per_second(), 2);
}

// --- looks -----------------------------------------------------------------------------------

#[test]
fn scene_follows_the_provided_palette_natively() {
    let settings = CloudsSettings {
        source: CloudSource::GoesEast,
        cell_colors: true,
        ..Default::default()
    };
    let (transport, _) = weather_transport();
    let mut plain = scene_with(&settings, transport);
    pump(&mut plain, settled);
    let plain_colors = render(&mut plain, 60, 20, Duration::ZERO).cell_colors;
    let mut env = greenwich();
    env.palette = ScenePalette {
        stops: vec![[0, 40, 0], [0, 255, 120]],
        ..Default::default()
    };
    let (transport, _) = weather_transport();
    let mut themed = CloudsScene::with_parts(&settings, &env, transport, no_land());
    assert!(themed.follows_palette());
    pump(&mut themed, settled);
    let themed_colors = render(&mut themed, 60, 20, Duration::ZERO).cell_colors;
    assert_ne!(plain_colors, themed_colors);
    themed.set_palette(&ScenePalette::default());
    let restored = render(&mut themed, 60, 20, Duration::ZERO).cell_colors;
    assert_eq!(plain_colors, restored);
}
