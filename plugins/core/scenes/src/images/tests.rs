//! Tests of the Images scene: settings and rows, the manifest entry, and the scene driven frame
//! by frame over a scripted host (`FakeIo`).

use super::settings::{
    Easing, FitMode, ImagesMode, SingleSource, SlideOrder, BUILTIN_IMAGES, PICKED_FOLDERS,
};
use super::source::fake::FakeIo;
use super::source::{Fetched, ItemKind};
use super::*;
use crate::control::{Control, ControlValue, SceneSettings};
use crate::kit::debug::{render_frame, Rendered};
use crate::kit::style::ScenePalette;
use serde::Serialize;
use serde_json::{json, Value};

fn manifest() -> Value {
    serde_json::from_str(include_str!("../../manifest.json")).expect("manifest.json is JSON")
}

fn images_entry() -> Value {
    manifest()["contributes"]["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|scene| scene["id"] == "images"))
        .cloned()
        .expect("the manifest declares the images scene")
}

fn properties() -> Value {
    images_entry()["settings_schema"]["properties"].clone()
}

fn control<'a>(rows: &'a [Control], id: &str) -> &'a Control {
    rows.iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("row {id}"))
}

// ---- settings -------------------------------------------------------------------------------

#[test]
fn defaults_are_the_builtin_picture_with_a_slow_zoom() {
    let settings = ImagesSettings::default();
    assert_eq!(settings.mode, ImagesMode::Builtin);
    assert_eq!(settings.source, SingleSource::NightCafe);
    assert!(settings.recursive);
    assert_eq!(settings.order, SlideOrder::Sequential);
    assert_eq!(
        (
            settings.display_seconds,
            settings.transition_seconds,
            settings.motion_strength_percent
        ),
        (30, 3, 12)
    );
    assert_eq!(settings.motion, Motion::ZoomIn);
    assert_eq!(settings.easing, Easing::EaseInOut);
    assert_eq!(settings.fit, FitMode::Fill);
    assert!(settings.folders().is_empty());
    assert_eq!(settings.normalized(), settings);
}

#[test]
fn a_spec_is_clamped_and_unknown_keys_are_ignored() {
    let settings: ImagesSettings = crate::kit::settings::from_spec(&json!({
        "mode": "folders",
        "display_seconds": 5_000,
        "transition_seconds": 99,
        "motion_strength_percent": 900,
        "shuffle_seed": 60_000,
        "brightness": 3
    }));
    assert_eq!(settings.mode, ImagesMode::Folders);
    assert_eq!(settings.display_seconds, 1800);
    assert_eq!(settings.transition_seconds, 30);
    assert_eq!(settings.motion_strength_percent, 50);
    assert_eq!(settings.shuffle_seed, 9999);
    let low: ImagesSettings = crate::kit::settings::from_spec(&json!({"display_seconds": 0}));
    assert_eq!(low.display_seconds, 3);
}

#[test]
fn path_text_is_trimmed_and_cut_on_a_character_boundary() {
    let settings = ImagesSettings {
        file: "  /pictures/a.png \n".to_owned(),
        folder: format!("a{}", "\u{e9}".repeat(600)),
        ..ImagesSettings::default()
    }
    .normalized();
    assert_eq!(settings.file, "/pictures/a.png");
    // 1 + 2 * 600 bytes cut at 1024 falls inside a character and backs off to 1023.
    assert_eq!(settings.folder.len(), 1023);
    assert!(settings
        .folder
        .chars()
        .skip(1)
        .all(|letter| letter == '\u{e9}'));
}

#[test]
fn the_cross_fade_is_at_most_half_the_display_time_and_only_for_a_slideshow() {
    let slideshow = ImagesSettings {
        mode: ImagesMode::Folders,
        display_seconds: 4,
        transition_seconds: 30,
        ..ImagesSettings::default()
    };
    assert!(slideshow.is_slideshow_mode());
    assert_eq!(slideshow.effective_transition_seconds(), 2);
    let single = ImagesSettings::default();
    assert!(!single.is_slideshow_mode());
    assert_eq!(single.effective_transition_seconds(), 0);
}

#[test]
fn only_the_filled_folder_rows_are_listed_with_their_picked_scope() {
    let settings = ImagesSettings {
        folder: "/a".into(),
        folder_3: "/c".into(),
        ..ImagesSettings::default()
    };
    assert_eq!(settings.folders(), vec![(1, "/a"), (3, "/c")]);
}

#[test]
fn every_row_exists_once_and_the_schema_check_agrees_with_the_manifest() {
    let rows = ImagesSettings::default().controls();
    let mut ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "no id twice");
    crate::kit::settings::assert_schema_matches_controls("images", &rows);
}

#[test]
fn the_scene_has_no_colour_rows_of_its_own() {
    for forbidden in [
        "brightness",
        "contrast",
        "hue",
        "saturation",
        "palette",
        "intensity",
        "opacity",
        "preset",
        "tint",
    ] {
        assert!(
            properties().get(forbidden).is_none(),
            "{forbidden} belongs to the shared look"
        );
    }
}

#[test]
fn rows_edit_their_field_and_report_whether_it_changed() {
    let mut settings = ImagesSettings::default();
    assert_eq!(
        settings.set_control("mode", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.mode, ImagesMode::Folders);
    assert_eq!(
        settings.set_control("mode", ControlValue::Index(2)),
        Ok(false)
    );
    assert!(settings
        .set_control("mode", ControlValue::Index(9))
        .is_err());
    assert_eq!(
        settings.set_control("source", ControlValue::Index(3)),
        Ok(true)
    );
    assert_eq!(settings.source, SingleSource::UnsplashDesk);
    assert_eq!(
        settings.set_control("file", ControlValue::Text("  /p/a.png ".into())),
        Ok(true)
    );
    assert_eq!(settings.file, "/p/a.png");
    assert_eq!(
        settings.set_control("file", ControlValue::Text("/p/a.png".into())),
        Ok(false)
    );
    assert!(settings
        .set_control("file", ControlValue::Number(1))
        .is_err());
    for (id, field) in [("folder", 0), ("folder_2", 1), ("folder_3", 2)] {
        assert_eq!(
            settings.set_control(id, ControlValue::Text(format!("/f{field}"))),
            Ok(true)
        );
    }
    assert_eq!(settings.folder_2, "/f1");
    assert_eq!(
        settings.set_control("recursive", ControlValue::Bool(false)),
        Ok(true)
    );
    assert!(!settings.recursive);
    assert!(settings
        .set_control("recursive", ControlValue::Number(1))
        .is_err());
    assert_eq!(
        settings.set_control("order", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.order, SlideOrder::Shuffle);
    assert_eq!(
        settings.set_control("shuffle_seed", ControlValue::Number(99_999)),
        Ok(true)
    );
    assert_eq!(settings.shuffle_seed, 9999);
    assert_eq!(
        settings.set_control("display_seconds", ControlValue::Number(1)),
        Ok(true)
    );
    assert_eq!(settings.display_seconds, 3);
    assert_eq!(
        settings.set_control("transition_seconds", ControlValue::Number(-5)),
        Ok(true)
    );
    assert_eq!(settings.transition_seconds, 0);
    assert_eq!(
        settings.set_control("motion", ControlValue::Index(6)),
        Ok(true)
    );
    assert_eq!(settings.motion, Motion::None);
    assert_eq!(
        settings.set_control("motion_strength_percent", ControlValue::Number(80)),
        Ok(true)
    );
    assert_eq!(settings.motion_strength_percent, 50);
    assert_eq!(
        settings.set_control("easing", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.easing, Easing::EaseOut);
    assert_eq!(
        settings.set_control("fit", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.fit, FitMode::Stretch);
    assert_eq!(
        settings.set_control("no_such_row", ControlValue::Number(1)),
        Ok(false)
    );
    // Each row now shows the edited value.
    let rows = settings.controls();
    assert_eq!(control(&rows, "fit").value, ControlValue::Index(2));
    assert_eq!(
        control(&rows, "folder_3").value,
        ControlValue::Text("/f2".into())
    );
}

// ---- the manifest entry ----------------------------------------------------------------------

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
    assert_enum(&properties, "mode", ImagesMode::ALL);
    assert_enum(&properties, "source", SingleSource::ALL);
    assert_enum(&properties, "order", SlideOrder::ALL);
    assert_enum(&properties, "motion", Motion::ALL);
    assert_enum(&properties, "easing", Easing::ALL);
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
fn rows_are_shown_only_in_the_mode_that_uses_them() {
    let properties = properties();
    let shown_in = |key: &str| -> Vec<String> {
        properties[key]["visible_when"]["in"]
            .as_array()
            .unwrap_or_else(|| panic!("{key} has a visibility rule"))
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(properties["source"]["visible_when"]["key"], "mode");
    assert_eq!(shown_in("source"), ["builtin"]);
    assert_eq!(shown_in("file"), ["file"]);
    for key in [
        "folder",
        "folder_2",
        "folder_3",
        "recursive",
        "order",
        "shuffle_seed",
        "transition_seconds",
    ] {
        assert_eq!(shown_in(key), ["folders"], "{key}");
    }
    for key in [
        "mode",
        "display_seconds",
        "motion",
        "motion_strength_percent",
        "easing",
        "fit",
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
    for (_, url) in BUILTIN_IMAGES {
        let host = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .expect("an https url");
        assert!(
            grants.contains(&format!("http:{host}").as_str()),
            "http:{host} is granted"
        );
    }
    let entry = images_entry();
    assert_eq!(entry["family"], "media");
    assert_eq!(entry["frame_rate_cap"], 12);
}

#[test]
fn the_registry_builds_the_scene_by_its_manifest_id() {
    let mut scene = crate::registry::create(
        "images",
        &json!({"mode": "file", "file": ""}),
        &SceneEnv::for_test(),
    )
    .expect("the registry knows images");
    let rendered = render_frame(scene.as_mut(), 8, 3, Duration::ZERO);
    assert_eq!(rendered.cell_colors.len(), 24);
    assert_eq!(scene.status().as_deref(), Some("Choose an image file"));
}

// ---- the scene -------------------------------------------------------------------------------

const STEP: Duration = Duration::from_millis(100);

fn scene_with(settings: &ImagesSettings, io: FakeIo) -> ImagesScene {
    ImagesScene::new(settings, &SceneEnv::for_test(), Box::new(io))
}

/// Renders `count` frames of `width` x `height` cells, `STEP` apart from `*clock`, calling `look`
/// after each; returns the last one.
fn run(
    scene: &mut ImagesScene,
    clock: &mut Duration,
    count: usize,
    (width, height): (u16, u16),
    mut look: impl FnMut(&Rendered, &ImagesScene),
) -> Rendered {
    let mut last = None;
    for _ in 0..count {
        *clock += STEP;
        let rendered = render_frame(scene, width, height, *clock);
        look(&rendered, scene);
        last = Some(rendered);
    }
    last.expect("at least one frame")
}

fn lit_fraction(rendered: &Rendered) -> f32 {
    let lit = rendered
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.3)
        .count();
    lit as f32 / rendered.raster.dots.len().max(1) as f32
}

fn file_settings() -> ImagesSettings {
    ImagesSettings {
        mode: ImagesMode::File,
        file: "/pictures/a.png".to_owned(),
        motion: Motion::None,
        ..ImagesSettings::default()
    }
}

fn file_io(color: [u8; 3]) -> FakeIo {
    let mut io = FakeIo::default();
    io.files
        .insert((0, String::new()), FakeIo::picture(32, 18, color));
    io
}

const SIZE: (u16, u16) = (20, 6);

#[test]
fn a_picked_file_shows_a_placeholder_then_the_picture() {
    let mut scene = scene_with(&file_settings(), file_io([255, 255, 255]));
    let mut clock = Duration::ZERO;
    let first = run(&mut scene, &mut clock, 1, SIZE, |_, _| {});
    assert_eq!(scene.status().as_deref(), Some("Loading image..."));
    assert!(lit_fraction(&first) < 0.2, "only a faint placeholder");
    assert_eq!(scene.frames_per_second(), ACTIVE_FPS);

    let shown = run(&mut scene, &mut clock, 8, SIZE, |_, _| {});
    assert!(
        lit_fraction(&shown) > 0.9,
        "a white picture lights the dots"
    );
    // One picture: no slide counter.
    assert_eq!(scene.status(), None);
    // No motion, nothing loading: nothing changes any more.
    assert_eq!(scene.frames_per_second(), IDLE_FPS);
    assert!(scene.uses_cell_colors() && scene.follows_palette());
}

#[test]
fn a_still_picture_is_drawn_once_and_reused() {
    let mut scene = scene_with(&file_settings(), file_io([200, 100, 50]));
    let mut clock = Duration::ZERO;
    let before = run(&mut scene, &mut clock, 8, SIZE, |_, _| {});
    assert!(scene.frame_cache.is_some());
    let after = run(&mut scene, &mut clock, 1, SIZE, |_, _| {});
    assert_eq!(before.raster.dots, after.raster.dots);
    assert_eq!(before.cell_colors, after.cell_colors);
}

#[test]
fn a_moving_picture_keeps_the_frame_rate_up() {
    let settings = ImagesSettings {
        motion: Motion::ZoomIn,
        ..file_settings()
    };
    let mut scene = scene_with(&settings, file_io([255, 255, 255]));
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 8, SIZE, |_, _| {});
    assert_eq!(scene.frames_per_second(), ACTIVE_FPS);
}

#[test]
fn an_unreadable_file_is_reported_and_stops_the_animation() {
    let mut scene = scene_with(&file_settings(), FakeIo::default());
    let mut clock = Duration::ZERO;
    let rendered = run(&mut scene, &mut clock, 10, SIZE, |_, _| {});
    let status = scene.status().expect("a reason is shown");
    assert!(status.contains("/pictures/a.png"), "{status}");
    assert_eq!(scene.frames_per_second(), IDLE_FPS);
    assert_eq!(lit_fraction(&rendered), 0.0);
}

#[test]
fn a_file_mode_without_a_chosen_file_asks_for_one() {
    let settings = ImagesSettings {
        mode: ImagesMode::File,
        ..ImagesSettings::default()
    };
    let mut scene = scene_with(&settings, FakeIo::default());
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 2, SIZE, |_, _| {});
    assert_eq!(scene.status().as_deref(), Some("Choose an image file"));
    assert_eq!(scene.frames_per_second(), IDLE_FPS);
}

#[test]
fn a_builtin_picture_is_downloaded_from_its_address() {
    let settings = ImagesSettings {
        source: SingleSource::MountainLake,
        motion: Motion::None,
        ..ImagesSettings::default()
    };
    let mut io = FakeIo::answering_after(2);
    // The fake refuses any address that is not scripted, so only the right one can load.
    io.urls.insert(
        SingleSource::MountainLake.picture().1.to_owned(),
        Fetched::Body(FakeIo::picture(32, 18, [255, 255, 255])),
    );
    let mut scene = scene_with(&settings, io);
    let mut clock = Duration::ZERO;
    let rendered = run(&mut scene, &mut clock, 12, SIZE, |_, _| {});
    assert!(lit_fraction(&rendered) > 0.9);
}

#[test]
fn a_failed_download_shows_the_server_answer() {
    let settings = ImagesSettings::default();
    let mut io = FakeIo::default();
    io.urls.insert(
        SingleSource::NightCafe.picture().1.to_owned(),
        Fetched::Status(404),
    );
    let mut scene = scene_with(&settings, io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 10, SIZE, |_, _| {});
    assert_eq!(scene.status().as_deref(), Some("the server answered 404"));
    assert_eq!(scene.frames_per_second(), IDLE_FPS);
}

fn slideshow_settings() -> ImagesSettings {
    ImagesSettings {
        mode: ImagesMode::Folders,
        folder: "/pics".to_owned(),
        display_seconds: 3,
        transition_seconds: 0,
        motion: Motion::None,
        ..ImagesSettings::default()
    }
}

fn slideshow_io(pictures: &[(&str, [u8; 3])], missing: &[&str]) -> FakeIo {
    let mut io = FakeIo::default();
    let items: Vec<(&str, ItemKind)> = pictures
        .iter()
        .map(|(name, _)| (*name, ItemKind::File))
        .collect();
    io.folder(1, "", &items);
    for (name, color) in pictures {
        if !missing.contains(name) {
            io.files
                .insert((1, (*name).to_owned()), FakeIo::picture(16, 9, *color));
        }
    }
    io
}

/// The distinct consecutive statuses of `frames` frames.
fn statuses(scene: &mut ImagesScene, frames: usize) -> (Vec<String>, Vec<[u8; 3]>) {
    let mut seen: Vec<String> = Vec::new();
    let mut colours: Vec<[u8; 3]> = Vec::new();
    let mut clock = Duration::ZERO;
    run(scene, &mut clock, frames, SIZE, |rendered, scene| {
        if let Some(status) = scene.status() {
            if seen.last() != Some(&status) {
                seen.push(status);
                colours.push(rendered.cell_colors[0]);
            }
        }
    });
    (seen, colours)
}

#[test]
fn a_slideshow_walks_through_the_folder_in_name_order_and_wraps() {
    let io = slideshow_io(
        &[
            ("c.png", [64, 64, 64]),
            ("a.png", [255, 255, 255]),
            ("b.png", [128, 128, 128]),
            ("notes.txt", [0, 0, 0]),
        ],
        &["notes.txt"],
    );
    let mut scene = scene_with(&slideshow_settings(), io);
    let (seen, colours) = statuses(&mut scene, 160);
    let slides: Vec<&str> = seen
        .iter()
        .filter(|status| status.starts_with("Slide "))
        .map(String::as_str)
        .collect();
    assert_eq!(
        slides[..4],
        [
            "Slide 1/3: a.png",
            "Slide 2/3: b.png",
            "Slide 3/3: c.png",
            "Slide 1/3: a.png"
        ]
    );
    // White is brighter than grey and grey brighter than dark grey, so the slides really differ.
    let brightness = |color: [u8; 3]| u32::from(color[0]);
    let first = colours
        .iter()
        .zip(&seen)
        .find(|(_, status)| status.starts_with("Slide 1"))
        .unwrap()
        .0;
    let third = colours
        .iter()
        .zip(&seen)
        .find(|(_, status)| status.starts_with("Slide 3"))
        .unwrap()
        .0;
    assert!(brightness(*first) > brightness(*third));
}

#[test]
fn an_unreadable_picture_is_skipped_and_counted() {
    let io = slideshow_io(
        &[
            ("a.png", [255, 255, 255]),
            ("b.png", [128, 128, 128]),
            ("c.png", [64, 64, 64]),
        ],
        &["b.png"],
    );
    let mut scene = scene_with(&slideshow_settings(), io);
    let (seen, _) = statuses(&mut scene, 120);
    assert!(
        !seen.iter().any(|status| status.contains("b.png")),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|status| status == "Slide 3/3: c.png (1 unreadable)"),
        "{seen:?}"
    );
}

#[test]
fn a_shuffled_slideshow_follows_the_seeded_order() {
    let io = slideshow_io(
        &[
            ("a.png", [255, 255, 255]),
            ("b.png", [128, 128, 128]),
            ("c.png", [64, 64, 64]),
        ],
        &[],
    );
    let settings = ImagesSettings {
        order: SlideOrder::Shuffle,
        shuffle_seed: 7,
        ..slideshow_settings()
    };
    let mut scene = scene_with(&settings, io);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 3, SIZE, |_, _| {});
    assert_eq!(scene.order, build_order(3, SlideOrder::Shuffle, 7));
    assert_eq!(scene.entries.len(), 3);
}

#[test]
fn folders_without_pictures_say_so() {
    let mut empty = FakeIo::default();
    empty.folder(1, "", &[("notes.txt", ItemKind::File)]);
    let mut scene = scene_with(&slideshow_settings(), empty);
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 3, SIZE, |_, _| {});
    assert_eq!(
        scene.status().as_deref(),
        Some("No pictures found in the chosen folders")
    );
    assert_eq!(scene.frames_per_second(), IDLE_FPS);

    let none = ImagesSettings {
        folder: String::new(),
        ..slideshow_settings()
    };
    let mut scene = scene_with(&none, FakeIo::default());
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 3, SIZE, |_, _| {});
    assert_eq!(
        scene.status().as_deref(),
        Some("Choose a folder of pictures")
    );
}

#[test]
fn scanning_shows_progress_until_the_list_is_known() {
    let io = slideshow_io(&[("a.png", [255, 255, 255])], &[]);
    let mut scene = scene_with(&slideshow_settings(), io);
    assert_eq!(scene.status().as_deref(), Some("Scanning folders..."));
    assert_eq!(scene.frames_per_second(), ACTIVE_FPS);
}

#[test]
fn a_much_larger_frame_restarts_loading_at_the_higher_resolution() {
    let mut scene = scene_with(&file_settings(), file_io([255, 255, 255]));
    let mut clock = Duration::ZERO;
    run(&mut scene, &mut clock, 8, (10, 4), |_, _| {});
    assert_eq!(scene.generation, 0);
    assert_eq!(scene.images.len(), 1);
    run(&mut scene, &mut clock, 1, (400, 100), |_, _| {});
    assert_eq!(scene.generation, 1);
    assert_eq!(scene.images.len(), 0, "the old resolution is dropped");
    let again = run(&mut scene, &mut clock, 8, (400, 100), |_, _| {});
    assert_eq!(scene.images.len(), 1);
    assert!(lit_fraction(&again) > 0.9);
}

#[test]
fn the_colours_follow_the_shared_palette() {
    let env = SceneEnv::new(ScenePalette::from_rgb_words(&[0x000000, 0xff0000]));
    let mut scene = ImagesScene::new(&file_settings(), &env, Box::new(file_io([255, 255, 255])));
    let mut clock = Duration::ZERO;
    let rendered = run(&mut scene, &mut clock, 8, SIZE, |_, _| {});
    assert!(rendered
        .cell_colors
        .iter()
        .all(|color| color[0] > 0 && color[1] == 0 && color[2] == 0));
    // A palette change reaches the next frame without reloading anything.
    scene.set_palette(&ScenePalette::default());
    let plain = run(&mut scene, &mut clock, 1, SIZE, |_, _| {});
    assert!(plain.cell_colors.iter().all(|color| color[1] > 0));
}

#[test]
fn an_empty_frame_is_ignored() {
    let mut scene = scene_with(&file_settings(), file_io([255, 255, 255]));
    let rendered = render_frame(&mut scene, 0, 0, Duration::ZERO);
    assert!(rendered.cell_colors.is_empty());
}
