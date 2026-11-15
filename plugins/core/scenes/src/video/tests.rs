//! Tests of the Video scene: the manifest entry, and the scene driven frame by frame over a
//! scripted host (`FakeIo`).

use super::player::Player;
use super::settings::{
    FitMode, PlaybackMode, RenderStyle, VideoSettings, VideoSource, PICKED_FILE, PICKED_FOLDERS,
};
use super::source::fake::{ClipScript, FakeIo};
use super::source::{CatalogueClip, DirItem, Fetched, FramePull, ItemKind, Opened, VideoIo};
use super::*;
use crate::control::SceneSettings;
use crate::kit::debug::{render_frame, Rendered};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn manifest() -> Value {
    serde_json::from_str(include_str!("../../manifest.json")).expect("manifest.json is JSON")
}

fn video_entry() -> Value {
    manifest()["contributes"]["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|scene| scene["id"] == "video"))
        .cloned()
        .expect("the manifest declares the video scene")
}

fn properties() -> Value {
    video_entry()["settings_schema"]["properties"].clone()
}

// ---- the manifest entry ----------------------------------------------------------------------

#[test]
fn every_row_exists_once_and_the_schema_check_agrees_with_the_manifest() {
    let rows = VideoSettings::default().controls();
    let mut ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "no id twice");
    crate::kit::settings::assert_schema_matches_controls("video", &rows);
}

#[test]
fn the_scene_has_no_colour_rows_of_its_own() {
    for forbidden in [
        "brightness",
        "contrast",
        "gamma",
        "invert",
        "hue",
        "saturation",
        "palette",
        "intensity",
        "preset",
        "tint",
    ] {
        assert!(
            properties().get(forbidden).is_none(),
            "{forbidden} belongs to the shared look"
        );
    }
}

fn assert_enum<T: Serialize + Copy>(properties: &Value, key: &str, all: &[T]) {
    let declared = properties[key]["enum"].as_array().expect("enum values");
    assert_eq!(declared.len(), all.len(), "{key} option count");
    for (value, item) in declared.iter().zip(all) {
        assert_eq!(*value, serde_json::to_value(item).unwrap(), "{key}");
    }
}

#[test]
fn the_manifest_values_are_the_serde_names_of_the_settings_enums() {
    let properties = properties();
    assert_enum(&properties, "source", VideoSource::ALL);
    assert_enum(&properties, "mode", PlaybackMode::ALL);
    assert_enum(&properties, "style", RenderStyle::ALL);
    assert_enum(&properties, "fit", FitMode::ALL);
}

#[test]
fn picker_rows_come_in_the_order_the_scene_expects_their_scopes() {
    let properties = properties();
    let mut pickers: Vec<(i64, String, String)> = properties
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, property)| {
            Some((
                property["order"].as_i64()?,
                key.clone(),
                property["picker"].as_str()?.to_owned(),
            ))
        })
        .collect();
    pickers.sort();
    let keys: Vec<&str> = pickers.iter().map(|(_, key, _)| key.as_str()).collect();
    // The host lends the n-th picker row as `picked/<n>`.
    assert_eq!(keys, ["file", "folder", "folder_2", "folder_3"]);
    assert_eq!(PICKED_FILE, 0);
    assert_eq!(PICKED_FOLDERS, [1, 2, 3]);
    assert_eq!(pickers[0].2, "file");
    assert!(pickers[1..].iter().all(|(_, _, kind)| kind == "directory"));
}

#[test]
fn rows_are_shown_only_where_they_apply() {
    let properties = properties();
    let shown_in = |key: &str| -> (String, Vec<String>) {
        let rule = &properties[key]["visible_when"];
        (
            rule["key"].as_str().unwrap_or_default().to_owned(),
            rule["in"]
                .as_array()
                .unwrap_or_else(|| panic!("{key} has a visibility rule"))
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect(),
        )
    };
    assert_eq!(
        shown_in("file"),
        ("source".to_owned(), vec!["file".to_owned()])
    );
    for key in ["folder", "folder_2", "folder_3", "recursive"] {
        assert_eq!(
            shown_in(key),
            ("source".to_owned(), vec!["folders".to_owned()]),
            "{key}"
        );
    }
    assert_eq!(
        shown_in("slowed_percent"),
        ("mode".to_owned(), vec!["slowed".to_owned()])
    );
    assert_eq!(
        shown_in("scene_seconds"),
        ("mode".to_owned(), vec!["random_scenes".to_owned()])
    );
    for key in [
        "source",
        "mode",
        "seed",
        "style",
        "fit",
        "detail",
        "frame_rate",
    ] {
        assert!(properties[key].get("visible_when").is_none(), "{key}");
    }
}

#[test]
fn the_manifest_grants_exactly_what_the_scene_reads() {
    let manifest = manifest();
    let grants: Vec<&str> = manifest["capabilities"]["scene"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(grants.contains(&"fs.read:picked"));
    assert!(grants.contains(&"media.video"));
    assert!(grants.contains(&"http:upload.wikimedia.org"));
    let entry = video_entry();
    assert_eq!(entry["family"], "media");
    assert_eq!(entry["frame_rate_cap"], 24);
    let clips = source::catalogue();
    assert!(!clips.is_empty());
    assert!(clips
        .iter()
        .all(|clip| clip.url.starts_with("https://upload.wikimedia.org/")));
}

#[test]
fn the_registry_builds_the_scene_by_its_manifest_id() {
    let mut scene = crate::registry::create(
        "video",
        &json!({"source": "file", "file": ""}),
        &SceneEnv::for_test(),
    )
    .expect("the registry knows video");
    let rendered = render_frame(scene.as_mut(), 8, 3, Duration::ZERO);
    assert_eq!(rendered.cell_colors.len(), 24);
    assert_eq!(scene.status().as_deref(), Some("Choose a video file"));
}

// ---- the scene -------------------------------------------------------------------------------

/// A [`VideoIo`] the test keeps a handle on after the scene took ownership of it.
#[derive(Clone)]
struct Shared(Arc<Mutex<FakeIo>>);

impl Shared {
    fn new(io: FakeIo) -> Self {
        Self(Arc::new(Mutex::new(io)))
    }

    fn with<T>(&self, look: impl FnOnce(&mut FakeIo) -> T) -> T {
        look(&mut self.0.lock().unwrap())
    }
}

impl VideoIo for Shared {
    fn list_dir(&mut self, scope: usize, path: &str) -> Result<Vec<DirItem>, String> {
        self.with(|io| io.list_dir(scope, path))
    }
    fn open_file(&mut self, scope: usize, path: &str) -> Result<Opened, String> {
        self.with(|io| io.open_file(scope, path))
    }
    fn open_bytes(&mut self, bytes: Vec<u8>, mime: &str) -> Result<Opened, String> {
        self.with(|io| io.open_bytes(bytes, mime))
    }
    fn next_frame(&mut self, video: u64, width: u32, height: u32) -> Result<FramePull, String> {
        self.with(|io| io.next_frame(video, width, height))
    }
    fn seek(&mut self, video: u64, millis: u64) -> Result<(), String> {
        self.with(|io| io.seek(video, millis))
    }
    fn close(&mut self, video: u64) {
        self.with(|io| io.close(video));
    }
    fn fetch_range(&mut self, url: &str, start: u64, end: u64) -> Result<u64, String> {
        self.with(|io| io.fetch_range(url, start, end))
    }
    fn take_fetched(&mut self, id: u64) -> Option<Fetched> {
        self.with(|io| io.take_fetched(id))
    }
    fn sha256_hex(&mut self, bytes: &[u8]) -> String {
        self.with(|io| io.sha256_hex(bytes))
    }
}

const STEP: Duration = Duration::from_millis(100);

fn file_settings() -> VideoSettings {
    VideoSettings {
        source: VideoSource::File,
        file: "/videos/clip.mp4".to_owned(),
        fit: FitMode::Stretch,
        detail: 0,
        ..VideoSettings::default()
    }
}

fn scene_with(settings: &VideoSettings, io: &Shared) -> VideoScene {
    VideoScene::new(settings, &SceneEnv::for_test(), Box::new(io.clone()))
}

/// Renders `count` frames of `width` x `height` cells, `STEP` apart from `*clock`; returns the
/// last one.
fn run(
    scene: &mut VideoScene,
    clock: &mut Duration,
    count: usize,
    (width, height): (u16, u16),
) -> Rendered {
    let mut last = None;
    for _ in 0..count {
        *clock += STEP;
        last = Some(render_frame(scene, width, height, *clock));
    }
    last.expect("at least one frame")
}

fn io_with_file_clip(script: ClipScript) -> Shared {
    let mut io = FakeIo::default();
    io.clip(PICKED_FILE, "", script);
    Shared::new(io)
}

fn clip_of(frames: u64) -> ClipScript {
    ClipScript::new(160, 90, 10.0, frames)
}

#[test]
fn a_picked_file_plays_and_the_status_names_it() {
    let io = io_with_file_clip(clip_of(1_000));
    let mut scene = scene_with(&file_settings(), &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 1, (8, 3));
    assert_eq!(scene.status().as_deref(), Some("Starting video..."));
    let rendered = run(&mut scene, &mut clock, 3, (8, 3));
    assert!(rendered.lit_dots() > 40, "the picture is drawn");
    let status = scene.status().expect("a status line");
    assert!(
        status.starts_with("Playing: /videos/clip.mp4 00:00"),
        "{status}"
    );
    assert!(status.contains("/ 01:40"), "{status}");
    // Stretch asks the decoder for exactly the dot canvas.
    assert!(io.with(|io| io.requested_sizes.iter().all(|size| *size == (16, 12))));
    assert_eq!(
        io.with(|io| io.open_calls.clone()),
        vec![(PICKED_FILE, String::new())]
    );
}

#[test]
fn live_playback_follows_the_clock_and_slowed_playback_runs_at_the_chosen_speed() {
    let live = io_with_file_clip(clip_of(1_000));
    let mut scene = scene_with(&file_settings(), &live);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 13, (8, 3));
    let status = scene.status().unwrap();
    assert!(
        status.starts_with("Playing: /videos/clip.mp4 00:01"),
        "{status}"
    );

    let slowed = io_with_file_clip(clip_of(1_000));
    let settings = VideoSettings {
        mode: PlaybackMode::Slowed,
        slowed_percent: 50,
        ..file_settings()
    };
    let mut scene = scene_with(&settings, &slowed);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 13, (8, 3));
    let status = scene.status().unwrap();
    assert!(
        status.starts_with("Playing: /videos/clip.mp4 00:00"),
        "{status}"
    );
    assert!(status.contains("(50% speed)"), "{status}");
}

#[test]
fn a_decoder_that_is_not_ready_keeps_the_last_picture_on_screen() {
    let mut script = clip_of(1_000);
    script.pending_between = 2;
    let io = io_with_file_clip(script);
    let mut scene = scene_with(&file_settings(), &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 5, (8, 3));
    for _ in 0..10 {
        let rendered = run(&mut scene, &mut clock, 1, (8, 3));
        assert!(rendered.lit_dots() > 40, "the picture never goes away");
        assert!(scene.status().unwrap().starts_with("Playing:"));
    }
}

#[test]
fn a_finished_clip_is_closed_and_played_again() {
    let io = io_with_file_clip(clip_of(5));
    let mut scene = scene_with(&file_settings(), &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 30, (8, 3));
    assert!(io.with(|io| io.open_calls.len()) >= 3, "the clip loops");
    assert!(io.with(|io| !io.closed.is_empty()));
}

#[test]
fn random_scenes_seek_into_the_clip_and_end_after_the_scene_length() {
    let io = io_with_file_clip(ClipScript::new(160, 90, 10.0, 600));
    let settings = VideoSettings {
        mode: PlaybackMode::RandomScenes,
        scene_seconds: 3,
        seed: 7,
        ..file_settings()
    };
    let mut scene = scene_with(&settings, &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 120, (8, 3));
    assert!(
        io.with(|io| io.open_calls.len()) >= 3,
        "an excerpt ends after about 3 s"
    );
    io.with(|io| {
        assert!(io
            .seeks
            .iter()
            .all(|(_, millis)| *millis <= 57_000 && millis % 100 == 0));
    });
}

#[test]
fn folders_are_discovered_and_played_in_name_order() {
    let mut io = FakeIo::default();
    io.folder(
        PICKED_FOLDERS[0],
        "",
        &[
            ("b.mp4", ItemKind::File),
            ("a.mp4", ItemKind::File),
            ("n.txt", ItemKind::File),
        ],
    );
    io.clip(PICKED_FOLDERS[0], "a.mp4", clip_of(1_000));
    io.clip(PICKED_FOLDERS[0], "b.mp4", clip_of(1_000));
    let io = Shared::new(io);
    let settings = VideoSettings {
        source: VideoSource::Folders,
        folder: "/videos".to_owned(),
        ..file_settings()
    };
    let mut scene = scene_with(&settings, &io);
    assert_eq!(scene.status().as_deref(), Some("Scanning video folders..."));
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 7, (8, 3));
    assert_eq!(
        io.with(|io| io.open_calls.clone()),
        vec![(PICKED_FOLDERS[0], "a.mp4".to_owned())]
    );
    assert!(scene.status().unwrap().starts_with("Playing: a.mp4"));
}

#[test]
fn no_folder_chosen_says_so() {
    let io = Shared::new(FakeIo::default());
    let settings = VideoSettings {
        source: VideoSource::Folders,
        ..file_settings()
    };
    let mut scene = scene_with(&settings, &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 2, (8, 3));
    assert_eq!(scene.status().as_deref(), Some("Choose a folder of videos"));
    assert!(io.with(|io| io.open_calls.is_empty()));
}

#[test]
fn an_unplayable_clip_is_skipped_and_the_next_one_plays() {
    let mut io = FakeIo::default();
    io.folder(
        1,
        "",
        &[("bad.mp4", ItemKind::File), ("good.mp4", ItemKind::File)],
    );
    io.clip(1, "good.mp4", clip_of(1_000));
    let io = Shared::new(io);
    let settings = VideoSettings {
        source: VideoSource::Folders,
        folder: "/videos".to_owned(),
        ..file_settings()
    };
    let mut scene = scene_with(&settings, &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 8, (8, 3));
    assert!(scene.status().unwrap().starts_with("Playing: good.mp4"));
    assert_eq!(io.with(|io| io.open_calls[0].1.clone()), "bad.mp4");
}

#[test]
fn when_every_clip_fails_the_list_is_retried_only_after_a_pause() {
    let io = Shared::new(FakeIo::default());
    let mut scene = scene_with(&file_settings(), &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 20, (8, 3));
    assert_eq!(io.with(|io| io.open_calls.len()), 1);
    let status = scene.status().unwrap();
    assert!(status.contains("not a video"), "{status}");
    run(&mut scene, &mut clock, 50, (8, 3));
    assert!(
        io.with(|io| io.open_calls.len()) >= 2,
        "retried after the pause"
    );
}

#[test]
fn germination_downloads_verifies_and_plays_a_catalogue_clip() {
    let mut fake = FakeIo::serving(vec![9u8; 100], 206);
    fake.bytes_clip = Some(clip_of(1_000));
    let io = Shared::new(fake);
    let clip = CatalogueClip {
        id: "t".into(),
        title: "Seedling".into(),
        url: "https://upload.wikimedia.org/t.webm".into(),
        author: "A. Person".into(),
        license: "CC0".into(),
        expected_download_bytes: 100,
        download_sha256: "fake-sha256-100".into(),
        mime: "video/webm".into(),
    };
    let settings = VideoSettings {
        source: VideoSource::Germination,
        ..file_settings()
    };
    let player = Player::with_catalogue(&settings, Box::new(io.clone()), vec![clip]);
    let mut scene = VideoScene::with_player(settings, &SceneEnv::for_test(), player);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 1, (8, 3));
    assert!(scene.status().unwrap().starts_with("Downloading: Seedling"));
    run(&mut scene, &mut clock, 8, (8, 3));
    let status = scene.status().unwrap();
    assert!(
        status.starts_with("Playing: Seedling - A. Person (CC0)"),
        "{status}"
    );
    assert_eq!(io.with(|io| io.opened_bytes.clone()), vec![100]);
    assert_eq!(io.with(|io| io.ranges.clone()), vec![(0, 99)]);
}

#[test]
fn a_clip_that_fails_its_digest_is_never_played() {
    let mut fake = FakeIo::serving(vec![1u8; 100], 206);
    fake.bytes_clip = Some(clip_of(1_000));
    let io = Shared::new(fake);
    let clip = CatalogueClip {
        id: "t".into(),
        title: "Seedling".into(),
        url: "https://upload.wikimedia.org/t.webm".into(),
        author: "A".into(),
        license: "CC0".into(),
        expected_download_bytes: 100,
        download_sha256: "some-other-digest".into(),
        mime: "video/webm".into(),
    };
    let settings = VideoSettings {
        source: VideoSource::Germination,
        ..file_settings()
    };
    let player = Player::with_catalogue(&settings, Box::new(io.clone()), vec![clip]);
    let mut scene = VideoScene::with_player(settings, &SceneEnv::for_test(), player);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 10, (8, 3));
    assert!(io.with(|io| io.opened_bytes.is_empty()));
    assert!(scene.status().unwrap().contains("digest"));
}

#[test]
fn a_new_terminal_size_is_asked_of_the_decoder_after_it_has_settled() {
    let io = io_with_file_clip(clip_of(10_000));
    let mut scene = scene_with(&file_settings(), &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 5, (8, 3));
    run(&mut scene, &mut clock, 2, (16, 6));
    assert!(
        io.with(|io| io.requested_sizes.last().copied()) == Some((16, 12)),
        "still the old size while the new one settles"
    );
    run(&mut scene, &mut clock, 12, (16, 6));
    assert_eq!(
        io.with(|io| io.requested_sizes.last().copied()),
        Some((32, 24))
    );
    assert!(scene.status().unwrap().starts_with("Playing:"));
}

#[test]
fn the_colored_style_supplies_cell_colors_and_the_others_do_not() {
    let io = io_with_file_clip(clip_of(1_000));
    let settings = VideoSettings {
        style: RenderStyle::Colored,
        ..file_settings()
    };
    let mut scene = scene_with(&settings, &io);
    assert!(scene.uses_cell_colors());
    assert!(scene.follows_palette());
    let mut clock = Duration::ZERO;
    let rendered = run(&mut scene, &mut clock, 4, (8, 3));
    assert_eq!(rendered.cell_colors.len(), 24);
    assert!(rendered
        .cell_colors
        .iter()
        .all(|color| color.iter().all(|channel| *channel >= 24)));

    let plain = scene_with(&file_settings(), &io_with_file_clip(clip_of(10)));
    assert!(!plain.uses_cell_colors());
}

#[test]
fn the_redraw_rate_is_the_frame_rate_setting() {
    let io = Shared::new(FakeIo::default());
    for (rate, expected) in [(6, 6), (15, 15), (24, 24)] {
        let settings = VideoSettings {
            frame_rate: rate,
            ..file_settings()
        };
        assert_eq!(scene_with(&settings, &io).frames_per_second(), expected);
    }
}

#[test]
fn dropping_the_scene_closes_the_open_clip() {
    let io = io_with_file_clip(clip_of(1_000));
    let mut scene = scene_with(&file_settings(), &io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 4, (8, 3));
    drop(scene);
    assert_eq!(io.with(|io| io.closed.len()), 1);
}
