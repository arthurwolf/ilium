//! Scene-level tests: fake frame sources and fake processes stand in for
//! ffmpeg, so nothing here needs the network, ffmpeg or the real clock.

use super::command::{
    ChildControl, ChildSlot, CommandRunner, CommandSpec, PixelFormat, PlayRequest, SpawnedChild,
    SystemRunner,
};
use super::discover::MediaInput;
use super::player::{FrameSource, FrameStream, OpenError, PlayerConfig};
use super::render::{stream_options, VideoScene};
use super::settings::{PlaybackMode, RenderStyle, VideoSettings};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use crate::scene::Scene;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PATIENCE: Duration = Duration::from_secs(10);

fn wait_until(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    false
}

/// Render repeatedly at a fixed wall time until `done` accepts the state.
fn draw_until(
    scene: &mut VideoScene,
    size: (u16, u16),
    wall: Duration,
    mut done: impl FnMut(&VideoScene, &Rendered) -> bool,
) -> bool {
    wait_until(|| {
        let rendered = render_frame(scene, size.0, size.1, wall);
        done(scene, &rendered)
    })
}

/// Render with a wall clock that advances 0.1 s per call, like a running host.
fn run_clock(scene: &mut VideoScene, size: (u16, u16), mut done: impl FnMut() -> bool) -> bool {
    let mut wall = 0.0;
    wait_until(|| {
        render_frame(scene, size.0, size.1, secs(wall));
        wall += 0.1;
        done()
    })
}

fn secs(value: f64) -> Duration {
    Duration::from_secs_f64(value)
}

fn near(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-3
}

// ---------------------------------------------------------------- fake source

#[derive(Clone)]
enum Plan {
    /// The clip ends at once without a frame (an unplayable file).
    Empty,
    Frames(usize),
    Endless,
}

type Content = fn(usize, &PlayRequest, PixelFormat) -> Vec<u8>;

fn level_for(index: usize) -> u8 {
    ((index * 10 + 20) % 256) as u8
}

fn flat_content(index: usize, request: &PlayRequest, format: PixelFormat) -> Vec<u8> {
    let mut data = vec![level_for(index); request.frame_bytes(format)];
    if format == PixelFormat::Rgb24 {
        for pixel in data.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[220, 30, 30]);
        }
    }
    data
}

struct FakeSource {
    log: Arc<Mutex<Vec<PlayRequest>>>,
    plan: Arc<dyn Fn(&PlayRequest) -> Plan + Send + Sync>,
    duration: Option<f64>,
    format: PixelFormat,
    content: Content,
}

struct FakeStream {
    request: PlayRequest,
    format: PixelFormat,
    content: Content,
    index: usize,
    total: Option<usize>,
}

impl FrameStream for FakeStream {
    fn read_frame(&mut self, buffer: &mut Vec<u8>) -> std::io::Result<bool> {
        if self.total.is_some_and(|total| self.index >= total) {
            return Ok(false);
        }
        *buffer = (self.content)(self.index, &self.request, self.format);
        self.index += 1;
        std::thread::sleep(Duration::from_millis(1));
        Ok(true)
    }
}

impl FrameSource for FakeSource {
    fn probe_duration(&mut self, _input: &MediaInput) -> Option<f64> {
        self.duration
    }

    fn open(&mut self, request: &PlayRequest) -> Result<Box<dyn FrameStream>, OpenError> {
        self.log.lock().unwrap().push(request.clone());
        let total = match (self.plan)(request) {
            Plan::Empty => Some(0),
            Plan::Frames(count) => Some(count),
            Plan::Endless => None,
        };
        Ok(Box::new(FakeStream {
            request: request.clone(),
            format: self.format,
            content: self.content,
            index: 0,
            total,
        }))
    }
}

struct Fixture {
    scene: VideoScene,
    log: Arc<Mutex<Vec<PlayRequest>>>,
    directory: tempfile::TempDir,
}

impl Fixture {
    fn requests(&self) -> Vec<PlayRequest> {
        self.log.lock().unwrap().clone()
    }

    fn names(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|r| r.input.display_name())
            .collect()
    }
}

fn base_settings(files: &[&str]) -> (VideoSettings, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    for name in files {
        std::fs::write(directory.path().join(name), b"fake").unwrap();
    }
    let settings = VideoSettings {
        source: directory.path().display().to_string(),
        contrast: 0,
        detail: 0,
        frame_rate: 10,
        ..VideoSettings::default()
    };
    (settings, directory)
}

fn fixture_with(
    settings: VideoSettings,
    directory: tempfile::TempDir,
    plan: impl Fn(&PlayRequest) -> Plan + Send + Sync + 'static,
    content: Content,
    duration: Option<f64>,
) -> Fixture {
    let log = Arc::new(Mutex::new(Vec::new()));
    let options = stream_options(&settings);
    let source = FakeSource {
        log: Arc::clone(&log),
        plan: Arc::new(plan),
        duration,
        format: options.pixel_format,
        content,
    };
    let mut config = PlayerConfig::new(settings.clone(), options, u64::from(settings.seed) + 1);
    config.backoff_base = Duration::from_millis(5);
    config.backoff_max = Duration::from_millis(40);
    config.idle_wait = Duration::from_millis(5);
    let scene = VideoScene::with_source(&settings, config, Box::new(source), ChildSlot::default());
    Fixture {
        scene,
        log,
        directory,
    }
}

fn fixture(files: &[&str], mutate: impl FnOnce(&mut VideoSettings), plan: Plan) -> Fixture {
    let (mut settings, directory) = base_settings(files);
    mutate(&mut settings);
    fixture_with(
        settings,
        directory,
        move |_| plan.clone(),
        flat_content,
        Some(100.0),
    )
}

// ------------------------------------------------------------ scheduling tests

#[test]
fn frames_are_shown_in_presentation_order_never_early() {
    let mut fx = fixture(&["a.mp4"], |_| {}, Plan::Endless);
    let size = (20, 5);
    assert!(draw_until(&mut fx.scene, size, secs(0.0), |scene, _| {
        scene.displayed_seconds().is_some_and(|s| near(s, 0.0))
    }));
    // 10 fps: at wall 0.35 the newest due frame is index 3 (0.3 s).
    assert!(draw_until(&mut fx.scene, size, secs(0.35), |scene, _| {
        scene.displayed_seconds().is_some_and(|s| near(s, 0.3))
    }));
    for _ in 0..30 {
        let rendered = render_frame(&mut fx.scene, size.0, size.1, secs(0.35));
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            near(fx.scene.displayed_seconds().unwrap(), 0.3),
            "frame 4 is not due yet"
        );
        let expected = f32::from(level_for(3)) / 255.0;
        assert!((rendered.raster.dots[0] - expected).abs() < 0.01);
    }
    assert!(draw_until(&mut fx.scene, size, secs(2.0), |scene, _| {
        scene.displayed_seconds().is_some_and(|s| near(s, 2.0))
    }));
}

#[test]
fn a_late_render_skips_to_the_newest_due_frame() {
    let mut fx = fixture(&["a.mp4"], |_| {}, Plan::Endless);
    assert!(draw_until(&mut fx.scene, (20, 5), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    // The host stalled for 3 s: the next render jumps straight ahead.
    assert!(draw_until(&mut fx.scene, (20, 5), secs(3.0), |s, _| {
        s.displayed_seconds().is_some_and(|value| near(value, 3.0))
    }));
}

#[test]
fn slowed_playback_reports_source_position_duration_and_speed() {
    let mut fx = fixture(
        &["clip.mp4"],
        |s| {
            s.mode = PlaybackMode::Slowed;
            s.slowed_percent = 50;
        },
        Plan::Endless,
    );
    assert!(draw_until(&mut fx.scene, (20, 5), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    assert!(draw_until(&mut fx.scene, (20, 5), secs(4.0), |scene, _| {
        scene.displayed_seconds().is_some_and(|s| near(s, 2.0))
    }));
    // 4 s of output at 50 % speed is 2 s of the 100 s source.
    assert_eq!(
        fx.scene.status().unwrap(),
        "Playing: clip.mp4 00:02 / 01:40 (50% speed)"
    );
    assert_eq!(fx.scene.frames_per_second(), 10);
    assert!(!fx.scene.uses_cell_colors());
}

#[test]
fn status_warns_about_plain_http_urls() {
    let mut fx = fixture(
        &[],
        |s| s.source = "http://example.invalid/live.mp4".to_owned(),
        Plan::Endless,
    );
    assert!(draw_until(&mut fx.scene, (20, 5), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    let status = fx.scene.status().unwrap();
    assert!(status.starts_with("Playing: live.mp4 00:00"), "{status}");
    assert!(status.contains("http:// is not encrypted"));
    assert!(
        !status.contains('/') || !status.contains(" / "),
        "no duration for URLs: {status}"
    );
}

#[test]
fn resizes_are_debounced_and_restart_at_the_same_position() {
    let mut fx = fixture(&["a.mp4"], |_| {}, Plan::Endless);
    let small = (20, 5);
    let large = (30, 8);
    assert!(draw_until(&mut fx.scene, small, secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    assert_eq!(fx.requests().len(), 1);
    assert_eq!(
        (fx.requests()[0].dots_width, fx.requests()[0].dots_height),
        (40, 20)
    );

    // A size that flickers (never holds 500 ms) does not restart the pipeline,
    // and the old picture is stretched to the new size meanwhile.
    for (step, size) in [large, small, large, small, large].into_iter().enumerate() {
        let wall = secs(0.1 + step as f64 * 0.2);
        let rendered = render_frame(&mut fx.scene, size.0, size.1, wall);
        assert_eq!(rendered.raster.width, usize::from(size.0) * 2);
        assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
    }
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(
        fx.requests().len(),
        1,
        "no restart while the size keeps changing"
    );

    // Held for 500 ms: restart at the new resolution, resuming where it was.
    render_frame(&mut fx.scene, large.0, large.1, secs(1.0));
    render_frame(&mut fx.scene, large.0, large.1, secs(1.6));
    assert!(wait_until(|| fx.requests().len() == 2));
    let restarted = fx.requests()[1].clone();
    assert_eq!((restarted.dots_width, restarted.dots_height), (60, 32));
    assert!(restarted.start_seconds > 0.0, "resumes, does not rewind");
    assert!(draw_until(
        &mut fx.scene,
        large,
        secs(1.6),
        |scene, rendered| {
            rendered.raster.width == 60
                && scene
                    .displayed_seconds()
                    .is_some_and(|s| s >= restarted.start_seconds)
        }
    ));
}

#[test]
fn unplayable_files_back_off_instead_of_spinning() {
    let mut fx = fixture(&["a.mp4"], |_| {}, Plan::Empty);
    let started = Instant::now();
    assert!(wait_until(|| {
        render_frame(&mut fx.scene, 20, 5, secs(0.0));
        fx.scene
            .status()
            .is_some_and(|s| s.starts_with("cannot play a.mp4"))
    }));
    std::thread::sleep(Duration::from_millis(300).saturating_sub(started.elapsed()));
    let attempts = fx.requests().len();
    assert!(attempts >= 3, "keeps retrying: {attempts}");
    // 5, 10, 20, 40, 40 ms ... would be ~40 attempts without the cap; with
    // doubling from 5 ms up to 40 ms there are at most about 12 in 300 ms.
    assert!(attempts <= 14, "backs off: {attempts}");
    let rendered = render_frame(&mut fx.scene, 40, 10, secs(0.5));
    let peak = rendered.raster.dots.iter().copied().fold(0.0_f32, f32::max);
    assert!(peak > 0.1 && peak < 0.6, "dim placeholder, peak {peak}");
    assert!(fx.scene.status().unwrap().contains("next try in"));
}

#[test]
fn a_failing_file_is_skipped_and_playback_moves_to_the_next() {
    let (mut settings, directory) = base_settings(&["a.mp4", "b.mp4", "c.mp4"]);
    settings.mode = PlaybackMode::Live;
    let mut fx = fixture_with(
        settings,
        directory,
        |request| {
            if request.input.display_name() == "a.mp4" {
                Plan::Empty
            } else {
                Plan::Endless
            }
        },
        flat_content,
        Some(60.0),
    );
    assert!(draw_until(&mut fx.scene, (20, 5), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    assert_eq!(fx.names()[..2], ["a.mp4", "b.mp4"]);
    assert!(fx.scene.status().unwrap().starts_with("Playing: b.mp4"));
}

#[test]
fn finished_clips_advance_through_the_playlist_and_wrap() {
    let mut fx = fixture(&["a.mp4", "b.mp4"], |_| {}, Plan::Frames(20));
    let log = Arc::clone(&fx.log);
    assert!(run_clock(&mut fx.scene, (20, 5), || log
        .lock()
        .unwrap()
        .len()
        >= 3));
    assert_eq!(fx.names()[..3], ["a.mp4", "b.mp4", "a.mp4"]);
}

#[test]
fn empty_or_missing_sources_say_so_and_draw_a_placeholder() {
    let mut fx = fixture(&[], |s| s.source = String::new(), Plan::Endless);
    assert!(wait_until(|| {
        render_frame(&mut fx.scene, 20, 5, secs(0.0));
        fx.scene
            .status()
            .is_some_and(|s| s.contains("No video source"))
    }));
    assert!(fx.requests().is_empty());
    let (mut settings, directory) = base_settings(&[]);
    settings.source = format!("{}/*.mp4", directory.path().display());
    let mut fx = fixture_with(settings, directory, |_| Plan::Endless, flat_content, None);
    assert!(wait_until(|| {
        render_frame(&mut fx.scene, 20, 5, secs(0.0));
        fx.scene
            .status()
            .is_some_and(|s| s.starts_with("No videos found"))
    }));
    assert!(fx.requests().is_empty());
    let _ = &fx.directory;
}

#[test]
fn colored_style_supplies_a_color_for_every_cell() {
    let mut fx = fixture(
        &["a.mp4"],
        |s| s.style = RenderStyle::Colored,
        Plan::Endless,
    );
    assert!(fx.scene.uses_cell_colors());
    // Before any frame: dim grey placeholder colors, but still one per cell.
    let early = render_frame(&mut fx.scene, 12, 4, secs(0.0));
    assert_eq!(early.cell_colors.len(), 48);
    assert!(draw_until(&mut fx.scene, (12, 4), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    let rendered = render_frame(&mut fx.scene, 12, 4, secs(0.0));
    assert_eq!(rendered.cell_colors.len(), 48);
    assert!(rendered
        .cell_colors
        .iter()
        .all(|[r, g, b]| *r > 150 && *g < 100 && *b < 100));
}

fn timeline_of(seed: u32) -> Vec<(String, u64)> {
    let (mut settings, directory) = base_settings(&["a.mp4", "b.mp4", "c.mp4", "d.mp4"]);
    settings.mode = PlaybackMode::RandomScenes;
    settings.scene_seconds = 10;
    settings.seed = seed;
    let mut fx = fixture_with(
        settings,
        directory,
        |_| Plan::Frames(3),
        flat_content,
        Some(300.0),
    );
    let log = Arc::clone(&fx.log);
    assert!(run_clock(&mut fx.scene, (20, 5), || log
        .lock()
        .unwrap()
        .len()
        >= 6));
    fx.requests()
        .iter()
        .take(6)
        .map(|request| {
            assert_eq!(request.limit_seconds, Some(10.0));
            assert!(request.start_seconds <= 290.0);
            (
                request.input.display_name(),
                (request.start_seconds * 10.0) as u64,
            )
        })
        .collect()
}

#[test]
fn random_scenes_are_reproducible_with_a_seed() {
    let first = timeline_of(11);
    assert_eq!(first, timeline_of(11));
    assert_ne!(first, timeline_of(12));
    let starts: std::collections::BTreeSet<u64> = first.iter().map(|(_, start)| *start).collect();
    assert!(starts.len() > 3, "random start positions: {first:?}");
}

// ------------------------------------------------------------ fake processes

struct FakeChild {
    killed: AtomicBool,
    kills: AtomicUsize,
    reaps: AtomicUsize,
}

impl FakeChild {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            killed: AtomicBool::new(false),
            kills: AtomicUsize::new(0),
            reaps: AtomicUsize::new(0),
        })
    }
}

impl ChildControl for FakeChild {
    fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        self.kills.fetch_add(1, Ordering::SeqCst);
    }

    fn reap(&self, _timeout: Duration) -> bool {
        self.reaps.fetch_add(1, Ordering::SeqCst);
        true
    }
}

/// A stdout that yields nothing until its child is killed, like a stalled stream.
struct BlockedPipe(Arc<FakeChild>);

impl Read for BlockedPipe {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        while !self.0.killed.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(0)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum FakeMode {
    Blocked,
    MissingTools,
}

struct FakeRunner {
    mode: FakeMode,
    spawned: Mutex<Vec<(CommandSpec, Arc<FakeChild>)>>,
}

impl FakeRunner {
    fn new(mode: FakeMode) -> Arc<Self> {
        Arc::new(Self {
            mode,
            spawned: Mutex::new(Vec::new()),
        })
    }

    fn specs(&self, program: &str) -> Vec<CommandSpec> {
        self.spawned
            .lock()
            .unwrap()
            .iter()
            .filter(|(spec, _)| spec.program == program)
            .map(|(spec, _)| spec.clone())
            .collect()
    }

    fn children(&self) -> Vec<Arc<FakeChild>> {
        self.spawned
            .lock()
            .unwrap()
            .iter()
            .map(|(_, child)| Arc::clone(child))
            .collect()
    }
}

impl CommandRunner for FakeRunner {
    fn spawn(&self, spec: &CommandSpec) -> std::io::Result<SpawnedChild> {
        if self.mode == FakeMode::MissingTools {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        let child = FakeChild::new();
        self.spawned
            .lock()
            .unwrap()
            .push((spec.clone(), Arc::clone(&child)));
        let stdout: Box<dyn Read + Send> = if spec.program == "ffprobe" {
            Box::new(std::io::Cursor::new(b"100.000000\n".to_vec()))
        } else {
            Box::new(BlockedPipe(Arc::clone(&child)))
        };
        Ok(SpawnedChild {
            stdout,
            control: child,
        })
    }
}

fn wait_for_ffmpeg(runner: &FakeRunner) -> bool {
    wait_until(|| !runner.specs("ffmpeg").is_empty())
}

#[test]
fn dropping_the_scene_kills_and_reaps_a_blocked_child_promptly() {
    let (settings, _directory) = base_settings(&["a.mp4"]);
    let runner = FakeRunner::new(FakeMode::Blocked);
    let mut scene = VideoScene::with_runner(&settings, runner.clone());
    render_frame(&mut scene, 20, 5, secs(0.0));
    assert!(wait_for_ffmpeg(&runner));
    let started = Instant::now();
    drop(scene);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
    let children = runner.children();
    assert!(!children.is_empty());
    for child in children {
        assert!(
            child.kills.load(Ordering::SeqCst) >= 1,
            "every child killed"
        );
        assert!(
            child.reaps.load(Ordering::SeqCst) >= 1,
            "every child reaped"
        );
    }
}

#[test]
fn each_mode_reaches_ffmpeg_with_the_expected_arguments() {
    let (mut settings, directory) = base_settings(&[]);
    let odd = directory.path().join("-odd \u{e9}\u{4e2d} name.mp4");
    std::fs::write(&odd, b"fake").unwrap();
    settings.source = odd.display().to_string();
    for (mode, expect_seek) in [
        (PlaybackMode::Live, false),
        (PlaybackMode::Slowed, false),
        (PlaybackMode::RandomScenes, true),
    ] {
        settings.mode = mode;
        settings.scene_seconds = 20;
        settings.style = RenderStyle::Colored;
        let runner = FakeRunner::new(FakeMode::Blocked);
        let mut scene = VideoScene::with_runner(&settings, runner.clone());
        render_frame(&mut scene, 30, 6, secs(0.0));
        assert!(wait_for_ffmpeg(&runner));
        let spec = runner.specs("ffmpeg").remove(0);
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let position = |flag: &str| args.iter().position(|arg| arg == flag);
        let input = position("-i").unwrap();
        assert_eq!(args[input + 1], format!("file:{}", odd.display()));
        assert_eq!(position("-ss").is_some(), expect_seek);
        assert_eq!(position("-t").is_some(), expect_seek);
        assert!(args.contains(&"rgb24".to_owned()));
        let filter = &args[position("-vf").unwrap() + 1];
        assert!(filter.contains("scale=60:24"), "{filter}");
        assert_eq!(
            filter.contains("setpts=PTS/0.5"),
            mode == PlaybackMode::Slowed,
            "{mode:?}"
        );
        // Every path is passed as one argument, never through a shell.
        assert!(
            spec.args
                .iter()
                .filter(|a| a.to_string_lossy().contains("odd"))
                .count()
                == 1
        );
        // Live mode still probes the file for the status line.
        assert_eq!(runner.specs("ffprobe").len(), 1);
    }
}

#[test]
fn missing_ffmpeg_is_reported_with_an_install_hint() {
    let (settings, _directory) = base_settings(&["a.mp4"]);
    let runner = FakeRunner::new(FakeMode::MissingTools);
    let mut scene = VideoScene::with_runner(&settings, runner);
    assert!(wait_until(|| {
        render_frame(&mut scene, 20, 5, secs(0.0));
        scene
            .status()
            .is_some_and(|s| s.starts_with("ffmpeg not found: install ffmpeg"))
    }));
    let rendered = render_frame(&mut scene, 40, 10, secs(0.0));
    assert!(
        rendered.raster.dots.iter().any(|dot| *dot > 0.1),
        "dim placeholder is drawn"
    );
    assert_eq!(rendered.lit_dots(), 0, "and it stays dim");
}

/// Spawns a real long-running `sleep` in place of ffmpeg to prove the real
/// process path kills and reaps its child.
#[cfg(target_os = "linux")]
struct SleepRunner {
    controls: Mutex<Vec<Arc<dyn ChildControl>>>,
}

#[cfg(target_os = "linux")]
impl CommandRunner for SleepRunner {
    fn spawn(&self, spec: &CommandSpec) -> std::io::Result<SpawnedChild> {
        if spec.program == "ffprobe" {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        let child = SystemRunner.spawn(&CommandSpec {
            program: "sleep".into(),
            args: vec!["31.415".into()],
        })?;
        self.controls
            .lock()
            .unwrap()
            .push(Arc::clone(&child.control));
        Ok(child)
    }
}

#[cfg(target_os = "linux")]
fn sleepers_running() -> usize {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| std::fs::read(entry.path().join("cmdline")).ok())
        .filter(|cmdline| cmdline.starts_with(b"sleep\x0031.415"))
        .count()
}

#[cfg(target_os = "linux")]
#[test]
fn a_real_child_process_is_killed_and_reaped_on_drop() {
    let (settings, _directory) = base_settings(&["a.mp4"]);
    let runner = Arc::new(SleepRunner {
        controls: Mutex::new(Vec::new()),
    });
    let mut scene = VideoScene::with_runner(&settings, runner.clone());
    render_frame(&mut scene, 20, 5, secs(0.0));
    assert!(wait_until(|| sleepers_running() >= 1));
    let started = Instant::now();
    drop(scene);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(sleepers_running(), 0, "no sleeper outlives the scene");
    for control in runner.controls.lock().unwrap().iter() {
        assert!(
            control.reap(Duration::from_millis(200)),
            "child was reaped, not a zombie"
        );
    }
}

// --------------------------------------------------------------- appearance

/// A lit sphere drifting over a checkerboard floor: smooth shading, hard
/// edges and texture, enough to judge the dither at a glance.
fn synthetic_content(index: usize, request: &PlayRequest, format: PixelFormat) -> Vec<u8> {
    let (width, height) = (request.dots_width as usize, request.dots_height as usize);
    let time = index as f32 * 0.15;
    let radius = height as f32 * 0.36;
    let center = (
        width as f32 * (0.5 + 0.22 * time.sin()),
        height as f32 * (0.5 + 0.1 * (time * 1.7).cos()),
    );
    let mut data = Vec::with_capacity(width * height * format.bytes_per_dot());
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (x as f32 - center.0, y as f32 - center.1);
            let distance = (dx * dx + dy * dy).sqrt();
            let level = if distance < radius {
                let z = (1.0 - (distance / radius).powi(2)).sqrt();
                let light = (-0.4 * dx / radius - 0.5 * dy / radius + 0.75 * z).max(0.0);
                0.08 + 0.92 * light
            } else {
                let checker = ((x / 6 + y / 6) % 2) as f32;
                0.12 + 0.16 * checker + 0.25 * (y as f32 / height as f32)
            };
            let value = (level.clamp(0.0, 1.0) * 255.0) as u8;
            match format {
                PixelFormat::Gray => data.push(value),
                PixelFormat::Rgb24 => data.extend_from_slice(&[value, value / 2, 255 - value]),
            }
        }
    }
    data
}

fn dots_in(cell: char) -> usize {
    (cell as u32).saturating_sub(0x2800).count_ones() as usize
}

fn show(style: RenderStyle, dither: DitherMode, density: u16) -> Vec<String> {
    let (mut settings, directory) = base_settings(&["a.mp4"]);
    settings.style = style;
    let mut fx = fixture_with(
        settings,
        directory,
        |_| Plan::Endless,
        synthetic_content,
        Some(90.0),
    );
    assert!(draw_until(&mut fx.scene, (60, 16), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    render_frame(&mut fx.scene, 60, 16, secs(0.0)).braille_lines(density, dither)
}

#[test]
fn synthetic_video_dithers_into_a_recognizable_picture() {
    let lines = show(RenderStyle::Dithered, DitherMode::Ordered, 60);
    println!("--- dithered (ordered)");
    lines.iter().for_each(|line| println!("{line}"));
    let lit: usize = lines
        .iter()
        .flat_map(|line| line.chars())
        .map(dots_in)
        .sum();
    let total = 60 * 16 * 8;
    assert!(
        lit > total / 10 && lit < total * 9 / 10,
        "lit {lit} of {total}"
    );
    // The sphere is brighter than the floor: its side has more ink.
    let ink = |columns: std::ops::Range<usize>| -> usize {
        lines
            .iter()
            .flat_map(|line| line.chars().skip(columns.start).take(columns.len()))
            .map(dots_in)
            .sum()
    };
    assert!(
        ink(20..44) > ink(0..24),
        "sphere region is denser than the far left floor"
    );
}

#[test]
fn every_render_style_draws_and_looks_different() {
    let dithered = show(RenderStyle::Dithered, DitherMode::Stippled, 60);
    let ink = show(RenderStyle::MonoInk, DitherMode::Ordered, 60);
    println!("--- stippled");
    dithered.iter().for_each(|line| println!("{line}"));
    println!("--- mono ink");
    ink.iter().for_each(|line| println!("{line}"));
    assert_ne!(dithered, ink);
    assert!(ink.iter().any(|line| line.chars().any(|c| c != ' ')));
    let colored = show(RenderStyle::Colored, DitherMode::Ordered, 60);
    assert!(colored.iter().any(|line| line.chars().any(|c| c != ' ')));
}

#[test]
fn consecutive_frames_differ_as_the_video_moves() {
    let (settings, directory) = base_settings(&["a.mp4"]);
    let mut fx = fixture_with(
        settings,
        directory,
        |_| Plan::Endless,
        synthetic_content,
        None,
    );
    assert!(draw_until(&mut fx.scene, (40, 12), secs(0.0), |s, _| s
        .displayed_seconds()
        .is_some()));
    let first = render_frame(&mut fx.scene, 40, 12, secs(0.0)).raster.dots;
    assert!(draw_until(&mut fx.scene, (40, 12), secs(2.0), |s, _| {
        s.displayed_seconds().is_some_and(|value| near(value, 2.0))
    }));
    let later = render_frame(&mut fx.scene, 40, 12, secs(2.0)).raster.dots;
    assert_ne!(first, later);
    let changed = first
        .iter()
        .zip(&later)
        .filter(|(a, b)| (**a - **b).abs() > 0.2)
        .count();
    assert!(changed > 20, "{changed} dots moved");
}

// ---------------------------------------------------- real ffmpeg (ignored)

/// Runs the real ffmpeg end to end. Needs ffmpeg and ffprobe on `PATH`:
/// `cargo test -p ilium-ambient real_ffmpeg -- --ignored --nocapture`.
#[test]
#[ignore = "runs the real ffmpeg"]
fn real_ffmpeg_test_pattern_plays_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let clip = directory.path().join("test pattern \u{e9}.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("testsrc2=size=320x180:rate=25")
        .args(["-t", "6", "-pix_fmt", "yuv420p"])
        .arg(&clip)
        .status()
        .expect("ffmpeg is installed");
    assert!(status.success());

    for (mode, style) in [
        (PlaybackMode::Live, RenderStyle::Dithered),
        (PlaybackMode::Slowed, RenderStyle::MonoInk),
        (PlaybackMode::RandomScenes, RenderStyle::Colored),
    ] {
        let settings = VideoSettings {
            source: clip.display().to_string(),
            mode,
            style,
            scene_seconds: 3,
            frame_rate: 12,
            ..VideoSettings::default()
        };
        let env = crate::scene::SceneEnv::for_test(directory.path().to_path_buf());
        let mut scene = VideoScene::new(&settings, &env);
        let mut lit_frames = Vec::new();
        let mut previous = -1.0;
        for step in 0..3 {
            let wall = secs(step as f64 * 0.6);
            let seen = draw_until(&mut scene, (60, 16), wall, |scene, rendered| {
                let position = scene.displayed_seconds().unwrap_or(-1.0);
                position > previous && rendered.lit_dots() > 0
            });
            previous = scene.displayed_seconds().unwrap_or(previous);
            assert!(
                seen,
                "{mode:?}: no frame decoded, status {:?}",
                scene.status()
            );
            lit_frames.push(render_frame(&mut scene, 60, 16, wall).raster.dots);
        }
        assert!(
            lit_frames[0] != lit_frames[2],
            "{mode:?}: the picture moves"
        );
        let status = scene.status().unwrap();
        assert!(
            status.starts_with("Playing: test pattern \u{e9}.mp4"),
            "{status}"
        );
        println!("{mode:?}/{style:?}: {status}");
        if mode == PlaybackMode::Live {
            render_frame(&mut scene, 60, 16, secs(1.2))
                .braille_lines(60, DitherMode::Ordered)
                .iter()
                .for_each(|line| println!("{line}"));
            // Resize while playing: the pipeline restarts at the new size and
            // carries on from where the picture was, not from the start.
            let before = scene.displayed_seconds().unwrap_or(0.0);
            let mut wall = 2.0;
            let resumed = wait_until(|| {
                let rendered = render_frame(&mut scene, 40, 10, secs(wall));
                wall += 0.1;
                rendered.raster.width == 80
                    && scene.displayed_seconds().is_some_and(|p| p > before)
                    && rendered.lit_dots() > 0
            });
            assert!(resumed, "status {:?}", scene.status());
        }
        let started = Instant::now();
        drop(scene);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

/// Prints real decoded frames in each style for eyeballing the dither:
/// `cargo test -p ilium-ambient real_ffmpeg_gallery -- --ignored --nocapture`.
#[test]
#[ignore = "runs the real ffmpeg and prints pictures"]
fn real_ffmpeg_gallery_prints_frames() {
    let directory = tempfile::tempdir().unwrap();
    for (name, filter) in [
        ("mandelbrot", "mandelbrot=size=640x360:rate=20"),
        ("gradients", "gradients=size=640x360:rate=20:speed=0.02"),
    ] {
        let clip = directory.path().join(format!("{name}.mp4"));
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                filter,
            ])
            .args(["-t", "4", "-pix_fmt", "yuv420p"])
            .arg(&clip)
            .status()
            .expect("ffmpeg is installed");
        assert!(status.success());
        for style in [RenderStyle::Dithered, RenderStyle::MonoInk] {
            let settings = VideoSettings {
                source: clip.display().to_string(),
                style,
                fit: super::settings::FitMode::Fill,
                ..VideoSettings::default()
            };
            let env = crate::scene::SceneEnv::for_test(directory.path().to_path_buf());
            let mut scene = VideoScene::new(&settings, &env);
            assert!(draw_until(&mut scene, (72, 20), secs(0.0), |s, _| s
                .displayed_seconds()
                .is_some()));
            assert!(draw_until(&mut scene, (72, 20), secs(1.5), |s, _| {
                s.displayed_seconds().is_some_and(|p| p > 1.2)
            }));
            println!("--- {name} / {style:?}");
            render_frame(&mut scene, 72, 20, secs(1.5))
                .braille_lines(70, DitherMode::Ordered)
                .iter()
                .for_each(|line| println!("{line}"));
        }
    }
}
