use super::decode::fixtures::{png_bytes, png_rgba_bytes};
use super::*;
use crate::control::{Control, ControlKind, ControlValue, SceneSettings};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use std::path::Path;
use std::time::Instant;

const WIDTH: u16 = 40;
const HEIGHT: u16 = 10;

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn write_png(path: &Path, color: [u8; 3]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, png_bytes(64, 32, |_, _| color)).expect("write png");
}

fn scene_for(settings: &ImagesSettings, cache: &Path) -> ImagesScene {
    ImagesScene::new(settings, &SceneEnv::for_test(cache.to_path_buf()))
}

fn folder_settings(folder: &Path) -> ImagesSettings {
    ImagesSettings {
        mode: ImagesMode::Folders,
        folders: folder.to_string_lossy().into_owned(),
        motion: Motion::None,
        display_seconds: 10,
        transition_seconds: 4,
        ..ImagesSettings::default()
    }
}

/// Render at t = 0 until `done(scene)` or 10 real seconds pass.
fn settle(scene: &mut ImagesScene, done: impl Fn(&ImagesScene) -> bool) -> Rendered {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rendered = render_frame(scene, WIDTH, HEIGHT, Duration::ZERO);
        if done(scene) {
            return rendered;
        }
        assert!(
            Instant::now() < deadline,
            "scene never settled: {:?}",
            scene.status()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn mean_color(rendered: &Rendered) -> [f32; 3] {
    let count = rendered.cell_colors.len() as f32;
    let mut sum = [0.0f32; 3];
    for color in &rendered.cell_colors {
        for channel in 0..3 {
            sum[channel] += f32::from(color[channel]);
        }
    }
    sum.map(|value| value / count)
}

fn vivid_full() -> ImagesSettings {
    ImagesSettings {
        preset: ImagePreset::Vivid,
        brightness_percent: 100,
        contrast_percent: 100,
        saturation_percent: 100,
        intensity_percent: 100,
        opacity_percent: 100,
        ..ImagesSettings::default()
    }
}

#[test]
fn defaults_keep_the_old_numbers_and_ranges() {
    let defaults = ImagesSettings::default();
    assert_eq!(defaults.source, ImageSource::Builtin(0));
    assert_eq!(defaults.preset, ImagePreset::Dimmed);
    assert_eq!(
        (
            defaults.brightness_percent,
            defaults.contrast_percent,
            defaults.hue_degrees,
            defaults.saturation_percent,
            defaults.intensity_percent,
            defaults.opacity_percent
        ),
        (72, 82, 180, 82, 48, 58)
    );
    assert_eq!(ImageSource::BUILTIN.len(), 4);
    assert_eq!(ImageSource::BUILTIN[0].0, "NightCafe dreamscape");
    assert!(ImageSource::BUILTIN[0].1.contains("nightcafe.studio"));
    assert_eq!(defaults.normalized(), defaults);
}

#[test]
fn old_project_settings_still_load() {
    let old = r#"{"source":{"Local":"/tmp/x.png"},"preset":"vivid","brightness_percent":100,
        "contrast_percent":90,"hue_degrees":200,"saturation_percent":70,
        "intensity_percent":60,"opacity_percent":80}"#;
    let settings: ImagesSettings = serde_json::from_str(old).expect("old settings");
    assert_eq!(settings.source, ImageSource::Local("/tmp/x.png".into()));
    assert_eq!(settings.preset, ImagePreset::Vivid);
    assert_eq!(settings.hue_degrees, 200);
    assert_eq!(settings.mode, ImagesMode::Single);
    assert_eq!(settings.display_seconds, 30);
    let builtin: ImagesSettings = serde_json::from_str(r#"{"source":{"Builtin":2}}"#).expect("b");
    assert_eq!(builtin.source, ImageSource::Builtin(2));
    let empty: ImagesSettings = serde_json::from_str("{}").expect("empty");
    assert_eq!(empty, ImagesSettings::default());
    let json = serde_json::to_string(&ImagesSettings::default()).expect("json");
    let back: ImagesSettings = serde_json::from_str(&json).expect("roundtrip");
    assert_eq!(back, ImagesSettings::default());
}

#[test]
fn normalized_clamps_every_field() {
    let wild = ImagesSettings {
        source: ImageSource::Builtin(99),
        brightness_percent: 999,
        contrast_percent: 999,
        hue_degrees: 999,
        saturation_percent: 999,
        intensity_percent: 999,
        opacity_percent: 999,
        display_seconds: 0,
        shuffle_seed: 60000,
        transition_seconds: 500,
        motion_strength_percent: 500,
        ..ImagesSettings::default()
    }
    .normalized();
    assert_eq!(wild.source, ImageSource::Builtin(3));
    assert_eq!(
        (
            wild.brightness_percent,
            wild.contrast_percent,
            wild.hue_degrees
        ),
        (200, 200, 360)
    );
    assert_eq!(
        (
            wild.saturation_percent,
            wild.intensity_percent,
            wild.opacity_percent
        ),
        (200, 100, 100)
    );
    assert_eq!(
        (
            wild.display_seconds,
            wild.shuffle_seed,
            wild.transition_seconds,
            wild.motion_strength_percent
        ),
        (3, 9999, 30, 50)
    );
}

fn ids(controls: &[Control]) -> Vec<&'static str> {
    controls.iter().map(|control| control.id).collect()
}

#[test]
fn controls_follow_the_mode_and_have_stable_unique_ids() {
    let mut settings = ImagesSettings::default();
    let single = settings.controls();
    assert!(ids(&single).contains(&"builtin_image"));
    assert!(!ids(&single).contains(&"folders"));
    assert!(!ids(&single).contains(&"transition_seconds"));

    settings.mode = ImagesMode::Folders;
    let folders = ids(&settings.controls());
    assert!(folders.contains(&"folders") && folders.contains(&"recursive"));
    assert!(folders.contains(&"transition_seconds") && folders.contains(&"order"));
    assert!(!folders.contains(&"shuffle_seed"));
    settings.order = SlideOrder::Shuffle;
    assert!(ids(&settings.controls()).contains(&"shuffle_seed"));

    settings.mode = ImagesMode::UrlList;
    assert!(ids(&settings.controls()).contains(&"urls"));

    settings.motion = Motion::None;
    let quiet = ids(&settings.controls());
    assert!(!quiet.contains(&"motion_strength") && !quiet.contains(&"easing"));

    for mode in ImagesMode::ALL {
        settings.mode = *mode;
        let rows = settings.controls();
        let mut seen = ids(&rows);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), rows.len(), "unique ids in {mode:?}");
        for row in &rows {
            assert!(
                !row.label.is_empty() && row.help.ends_with('.'),
                "{}",
                row.id
            );
        }
    }
}

#[test]
fn setting_a_control_to_its_current_value_reports_no_change() {
    for mode in ImagesMode::ALL {
        let mut settings = ImagesSettings {
            mode: *mode,
            folders: std::env::temp_dir().to_string_lossy().into_owned(),
            urls: "https://example.com/a.jpg".to_owned(),
            ..ImagesSettings::default()
        };
        for row in settings.controls() {
            if matches!(row.kind, ControlKind::Text { .. })
                && row.id != "folders"
                && row.id != "urls"
            {
                continue;
            }
            let outcome = settings.set_control(row.id, row.value.clone());
            assert_eq!(outcome, Ok(false), "{} in {mode:?}", row.id);
        }
    }
}

#[test]
fn every_choice_and_slider_row_edits_its_field() {
    let mut settings = ImagesSettings::default();
    assert_eq!(
        settings.set_control("brightness", ControlValue::Number(150)),
        Ok(true)
    );
    assert_eq!(settings.brightness_percent, 150);
    assert_eq!(
        settings.set_control("brightness", ControlValue::Number(9999)),
        Ok(true)
    );
    assert_eq!(settings.brightness_percent, 200);
    assert_eq!(
        settings.set_control("hue", ControlValue::Number(-5)),
        Ok(true)
    );
    assert_eq!(settings.hue_degrees, 0);
    assert_eq!(
        settings.set_control("motion", ControlValue::Index(Motion::Drift.index())),
        Ok(true)
    );
    assert_eq!(settings.motion, Motion::Drift);
    assert_eq!(
        settings.set_control("fit", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.fit, FitMode::Fit);
    assert_eq!(
        settings.set_control("easing", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.easing, Easing::Linear);
    assert_eq!(
        settings.set_control("preset", ControlValue::Index(3)),
        Ok(true)
    );
    assert_eq!(settings.preset, ImagePreset::Cool);
    assert_eq!(
        settings.set_control("display_seconds", ControlValue::Number(1)),
        Ok(true)
    );
    assert_eq!(settings.display_seconds, 3);
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("motion", ControlValue::Index(99))
        .is_err());
    assert!(settings
        .set_control("motion", ControlValue::Text("x".into()))
        .is_err());
    assert_eq!(
        settings.set_control("source_kind", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.source, ImageSource::Url(String::new()));
    assert_eq!(
        settings.set_control("source_kind", ControlValue::Index(0)),
        Ok(true)
    );
    assert_eq!(
        settings.set_control("builtin_image", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.source, ImageSource::Builtin(2));
}

#[test]
fn invalid_user_input_is_rejected_with_a_message() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut settings = ImagesSettings::default();
    let missing = directory.path().join("missing.png");
    let error = settings
        .set_control(
            "file",
            ControlValue::Text(missing.to_string_lossy().into_owned()),
        )
        .expect_err("missing file");
    assert!(error.contains("File not found"));
    let text_file = directory.path().join("notes.txt");
    std::fs::write(&text_file, "x").expect("write");
    assert!(settings
        .set_control(
            "file",
            ControlValue::Text(text_file.to_string_lossy().into_owned())
        )
        .expect_err("not an image")
        .contains("supported"));
    assert!(settings
        .set_control("file", ControlValue::Text(String::new()))
        .is_err());
    let good = directory.path().join("ok.png");
    write_png(&good, [1, 2, 3]);
    assert_eq!(
        settings.set_control(
            "file",
            ControlValue::Text(good.to_string_lossy().into_owned())
        ),
        Ok(true)
    );
    assert_eq!(settings.source, ImageSource::Local(good));

    assert!(settings
        .set_control("url", ControlValue::Text("http://example.com/a.jpg".into()))
        .expect_err("http")
        .contains("https"));
    assert_eq!(
        settings.set_control(
            "url",
            ControlValue::Text(" https://example.com/a.jpg ".into())
        ),
        Ok(true)
    );
    assert_eq!(
        settings.source,
        ImageSource::Url("https://example.com/a.jpg".into())
    );

    assert!(settings
        .set_control("folders", ControlValue::Text("/definitely/not/here".into()))
        .expect_err("folder")
        .contains("Folder not found"));
    assert!(settings
        .set_control("folders", ControlValue::Text(" ; ".into()))
        .is_err());
    let spec = format!(
        "{}/*.png;{}",
        directory.path().display(),
        directory.path().display()
    );
    assert_eq!(
        settings.set_control("folders", ControlValue::Text(spec.clone())),
        Ok(true)
    );
    assert_eq!(settings.folders, spec);
    assert!(settings
        .set_control(
            "urls",
            ControlValue::Text("https://a/1.jpg;ftp://b/2.jpg".into())
        )
        .is_err());
    assert_eq!(
        settings.set_control(
            "urls",
            ControlValue::Text("https://a/1.jpg; https://b/2.jpg".into())
        ),
        Ok(true)
    );
    assert_eq!(settings.urls, "https://a/1.jpg;https://b/2.jpg");
}

#[test]
fn a_single_local_image_renders_off_thread_and_is_static_without_motion() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("red.png");
    write_png(&path, [230, 40, 40]);
    let settings = ImagesSettings {
        source: ImageSource::Local(path),
        motion: Motion::None,
        ..vivid_full()
    };
    let mut scene = scene_for(&settings, directory.path());
    assert_eq!(scene.status().as_deref(), Some("Loading image..."));
    assert!(scene.uses_cell_colors());
    let first = render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
    assert!(
        first.lit_dots() == 0,
        "placeholder is dim, image not loaded yet"
    );
    let loaded = settle(&mut scene, |scene| scene.scheduler.current().is_some());
    assert!(loaded.lit_dots() > 300, "image lit: {}", loaded.lit_dots());
    assert_eq!(scene.status(), None);
    let color = mean_color(&loaded);
    assert!(
        color[0] > color[1] * 2.0 && color[0] > color[2] * 2.0,
        "{color:?}"
    );

    let later = render_frame(&mut scene, WIDTH, HEIGHT, secs(500));
    assert_eq!(loaded.raster.dots, later.raster.dots, "identical frames");
    assert_eq!(loaded.cell_colors, later.cell_colors);
    assert_eq!(scene.frames_per_second(), 1, "nothing moves");
}

#[test]
fn motion_makes_frames_differ_deterministically_and_asks_for_12_fps() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("grad.png");
    std::fs::write(
        &path,
        png_bytes(320, 180, |x, y| {
            [(x * 255 / 319) as u8, (y * 255 / 179) as u8, 90]
        }),
    )
    .expect("write");
    let settings = ImagesSettings {
        source: ImageSource::Local(path),
        motion: Motion::ZoomPan,
        motion_strength_percent: 40,
        display_seconds: 20,
        ..vivid_full()
    };
    let mut scene = scene_for(&settings, directory.path());
    settle(&mut scene, |scene| scene.scheduler.current().is_some());
    let at_start = render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
    let at_middle = render_frame(&mut scene, WIDTH, HEIGHT, secs(10));
    assert_ne!(at_start.raster.dots, at_middle.raster.dots);
    assert_ne!(at_start.cell_colors, at_middle.cell_colors);
    assert_eq!(scene.frames_per_second(), 12);

    let mut twin = scene_for(&settings, directory.path());
    settle(&mut twin, |scene| scene.scheduler.current().is_some());
    let twin_middle = render_frame(&mut twin, WIDTH, HEIGHT, secs(10));
    assert_eq!(at_middle.raster.dots, twin_middle.raster.dots);
    assert_eq!(at_middle.cell_colors, twin_middle.cell_colors);

    // A lone image bounces back instead of jumping at the end of its move.
    let end = render_frame(&mut scene, WIDTH, HEIGHT, secs(20));
    let just_after = render_frame(&mut scene, WIDTH, HEIGHT, Duration::from_millis(20_100));
    let changed = end
        .raster
        .dots
        .iter()
        .zip(&just_after.raster.dots)
        .filter(|(a, b)| (**a - **b).abs() > 0.2)
        .count();
    assert!(
        changed < end.raster.dots.len() / 20,
        "no jump at the turnaround: {changed}"
    );
}

fn three_color_folder() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("tempdir");
    write_png(&directory.path().join("a_red.png"), [255, 0, 0]);
    write_png(&directory.path().join("b_green.png"), [0, 255, 0]);
    write_png(&directory.path().join("c_blue.png"), [0, 0, 255]);
    std::fs::write(directory.path().join("readme.txt"), "not an image").expect("write");
    directory
}

fn loaded_slideshow(directory: &Path, settings: ImagesSettings) -> ImagesScene {
    let mut scene = scene_for(&settings, &directory.join("cache-dir"));
    settle(&mut scene, |scene| {
        scene.scheduler.current().is_some() && scene.images.len() == 3
    });
    scene
}

#[test]
fn slideshow_advances_with_a_cross_fade_and_reports_the_slide() {
    let directory = three_color_folder();
    let settings = ImagesSettings {
        preset: ImagePreset::Vivid,
        ..folder_settings(directory.path())
    };
    let mut scene = loaded_slideshow(directory.path(), settings);
    let first = render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
    assert_eq!(scene.status().as_deref(), Some("Slide 1/3: a_red.png"));
    let red = mean_color(&first);
    assert!(red[0] > 3.0 * red[1].max(red[2]), "{red:?}");

    let before = render_frame(&mut scene, WIDTH, HEIGHT, Duration::from_millis(9_900));
    assert_eq!(mean_color(&before), red, "still the first slide");

    // The fade starts when the due time is first observed (t = 10 s) and
    // lasts 4 s; halfway both slides are equally weighted.
    render_frame(&mut scene, WIDTH, HEIGHT, secs(10));
    let mid = render_frame(&mut scene, WIDTH, HEIGHT, secs(12));
    let blend = mean_color(&mid);
    assert!(scene.scheduler.is_fading());
    assert!(
        blend[0] > 20.0 && blend[1] > 20.0,
        "both colours present: {blend:?}"
    );
    assert!(
        (blend[0] - blend[1]).abs() < 8.0,
        "equal weights at the midpoint: {blend:?}"
    );
    assert_eq!(scene.status().as_deref(), Some("Slide 2/3: b_green.png"));
    assert_eq!(scene.frames_per_second(), 12);

    let after = render_frame(&mut scene, WIDTH, HEIGHT, secs(14));
    let green = mean_color(&after);
    assert!(green[1] > 3.0 * green[0].max(green[2]), "{green:?}");
    render_frame(&mut scene, WIDTH, HEIGHT, secs(20));
    let third = render_frame(&mut scene, WIDTH, HEIGHT, secs(24));
    assert_eq!(scene.status().as_deref(), Some("Slide 3/3: c_blue.png"));
    let blue = mean_color(&third);
    assert!(blue[2] > 3.0 * blue[0].max(blue[1]), "{blue:?}");
    render_frame(&mut scene, WIDTH, HEIGHT, secs(30));
    let wrapped = render_frame(&mut scene, WIDTH, HEIGHT, secs(34));
    assert_eq!(scene.status().as_deref(), Some("Slide 1/3: a_red.png"));
    assert!(mean_color(&wrapped)[0] > 3.0 * mean_color(&wrapped)[2].max(1.0));
}

#[test]
fn shuffle_with_a_seed_is_repeatable_and_a_permutation() {
    let directory = three_color_folder();
    let mut settings = folder_settings(directory.path());
    settings.order = SlideOrder::Shuffle;
    settings.shuffle_seed = 7;
    let first = loaded_slideshow(directory.path(), settings.clone());
    let second = loaded_slideshow(directory.path(), settings.clone());
    assert_eq!(first.order, second.order);
    let mut sorted = first.order.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2]);
    assert_eq!(first.order, build_order(3, SlideOrder::Shuffle, 7));
    settings.shuffle_seed = 0;
    let auto = loaded_slideshow(directory.path(), settings);
    let mut auto_sorted = auto.order.clone();
    auto_sorted.sort_unstable();
    assert_eq!(
        auto_sorted,
        vec![0, 1, 2],
        "seed 0 still yields a permutation"
    );
}

#[test]
fn unreadable_files_are_skipped_and_reported() {
    let directory = tempfile::tempdir().expect("tempdir");
    write_png(&directory.path().join("a.png"), [255, 255, 255]);
    std::fs::write(directory.path().join("b.png"), b"corrupt").expect("write");
    write_png(&directory.path().join("c.png"), [255, 255, 255]);
    let mut scene = scene_for(
        &folder_settings(directory.path()),
        &directory.path().join("cache"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    // Start slide 1, then let its time pass so the scheduler must skip b.png.
    settle(&mut scene, |scene| {
        scene.scheduler.current().is_some() && scene.images.len() == 2
    });
    while scene.failed.is_empty() {
        render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    render_frame(&mut scene, WIDTH, HEIGHT, secs(11));
    let status = scene.status().expect("status");
    assert!(status.starts_with("Slide 3/3: c.png"), "{status}");
    assert!(status.contains("1 unreadable"), "{status}");
}

#[test]
fn errors_are_reported_in_the_status_line() {
    let directory = tempfile::tempdir().expect("tempdir");
    let missing = ImagesSettings {
        source: ImageSource::Local(directory.path().join("gone.png")),
        ..ImagesSettings::default()
    };
    let mut scene = scene_for(&missing, directory.path());
    settle(&mut scene, |scene| !scene.failed.is_empty());
    let rendered = render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
    assert!(scene.status().expect("status").contains("Cannot read"));
    assert_eq!(rendered.lit_dots(), 0);
    assert_eq!(scene.frames_per_second(), 1, "nothing to animate");

    let empty = ImagesSettings {
        mode: ImagesMode::Folders,
        folders: directory.path().to_string_lossy().into_owned(),
        ..ImagesSettings::default()
    };
    let mut scene = scene_for(&empty, directory.path());
    settle(&mut scene, |scene| scene.list_error.is_some());
    assert!(scene.status().expect("status").contains("No images found"));

    let blank = ImagesSettings {
        source: ImageSource::Local(Default::default()),
        ..ImagesSettings::default()
    };
    let mut scene = scene_for(&blank, directory.path());
    settle(&mut scene, |scene| scene.list_error.is_some());
    assert_eq!(scene.status().as_deref(), Some("Choose an image file"));
}

#[test]
fn hostile_sizes_never_crash_the_scene() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("huge.png");
    std::fs::write(&path, png_bytes(20_000, 16, |_, _| [200, 200, 200])).expect("write");
    let settings = ImagesSettings {
        source: ImageSource::Local(path),
        ..ImagesSettings::default()
    };
    let mut scene = scene_for(&settings, directory.path());
    settle(&mut scene, |scene| !scene.failed.is_empty());
    let rendered = render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
    assert!(scene.status().expect("status").contains("too large"));
    assert_eq!(rendered.lit_dots(), 0);
}

#[test]
fn large_images_are_downscaled_to_the_decode_target() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("big.png");
    std::fs::write(
        &path,
        png_bytes(3000, 1500, |x, _| [(x / 12) as u8, 90, 90]),
    )
    .expect("write");
    let settings = ImagesSettings {
        source: ImageSource::Local(path),
        ..ImagesSettings::default()
    };
    let mut scene = scene_for(&settings, directory.path());
    settle(&mut scene, |scene| scene.images.len() == 1);
    let image = scene.images.get(0).cloned().expect("decoded");
    assert!(
        image.width <= 1024 && image.height <= 576,
        "{}x{}",
        image.width,
        image.height
    );
    assert!((image.aspect() - 2.0).abs() < 0.02, "aspect kept");
}

#[test]
fn transparent_images_render_over_black() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("clear.png");
    std::fs::write(&path, png_rgba_bytes(16, 16, [255, 255, 255, 0])).expect("write");
    let settings = ImagesSettings {
        source: ImageSource::Local(path),
        motion: Motion::None,
        ..vivid_full()
    };
    let mut scene = scene_for(&settings, directory.path());
    let rendered = settle(&mut scene, |scene| scene.scheduler.current().is_some());
    assert_eq!(rendered.lit_dots(), 0);
}

#[test]
fn fit_mode_shows_borders_and_stretch_fills() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("square.png");
    std::fs::write(&path, png_bytes(50, 50, |_, _| [255, 255, 255])).expect("write");
    let make = |fit| ImagesSettings {
        source: ImageSource::Local(path.clone()),
        motion: Motion::None,
        fit,
        ..vivid_full()
    };
    let mut fit_scene = scene_for(&make(FitMode::Fit), directory.path());
    let fit = settle(&mut fit_scene, |scene| scene.scheduler.current().is_some());
    let mut stretch_scene = scene_for(&make(FitMode::Stretch), directory.path());
    let stretch = settle(&mut stretch_scene, |scene| {
        scene.scheduler.current().is_some()
    });
    assert!(stretch.lit_dots() > fit.lit_dots() + 200);
    assert_eq!(fit.cell_colors[0], [0, 0, 0], "left border is black");
    assert_ne!(stretch.cell_colors[0], [0, 0, 0]);
    let width = fit.raster.width;
    assert_eq!(
        fit.raster.dots[width / 2 + 30],
        0.0,
        "border column stays dark"
    );
}

#[test]
fn dropping_the_scene_stops_the_worker_quickly() {
    let directory = three_color_folder();
    let mut scene = scene_for(
        &folder_settings(directory.path()),
        &directory.path().join("cache"),
    );
    render_frame(&mut scene, WIDTH, HEIGHT, Duration::ZERO);
    let started = Instant::now();
    drop(scene);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn resizing_the_frame_keeps_rendering() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("p.png");
    write_png(&path, [180, 180, 60]);
    let settings = ImagesSettings {
        source: ImageSource::Local(path),
        motion: Motion::ZoomIn,
        ..vivid_full()
    };
    let mut scene = scene_for(&settings, directory.path());
    settle(&mut scene, |scene| scene.scheduler.current().is_some());
    for (width, height) in [(20, 5), (80, 24), (300, 90), (1, 1)] {
        let rendered = render_frame(&mut scene, width, height, secs(3));
        assert_eq!(
            rendered.cell_colors.len(),
            usize::from(width) * usize::from(height)
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while scene.images.is_empty() {
        render_frame(&mut scene, 300, 90, secs(4));
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A synthetic sunset used to judge the look; run with `-- --nocapture`.
fn sunset_png() -> Vec<u8> {
    png_bytes(480, 270, |x, y| {
        let (fx, fy) = (x as f32 / 479.0, y as f32 / 269.0);
        let sun =
            (((fx - 0.62).powi(2) + ((fy - 0.55) * 0.5625 * 2.0).powi(2)).sqrt() < 0.12) as u8;
        let hill = fy > 0.72 + 0.06 * (fx * 9.0).sin();
        if hill {
            return [12, 18, 30];
        }
        if sun == 1 {
            return [255, 236, 170];
        }
        let sky = fy / 0.75;
        [
            (40.0 + 215.0 * sky.powf(1.4)) as u8,
            (30.0 + 120.0 * sky.powf(1.8)) as u8,
            (110.0 - 40.0 * sky) as u8,
        ]
    })
}

#[test]
fn braille_preview_of_a_synthetic_sunset() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("sunset.png");
    std::fs::write(&path, sunset_png()).expect("write");
    for (name, preset) in [
        ("dimmed", ImagePreset::Dimmed),
        ("vivid", ImagePreset::Vivid),
    ] {
        let settings = ImagesSettings {
            source: ImageSource::Local(path.clone()),
            preset,
            motion: Motion::None,
            ..ImagesSettings::default()
        };
        let mut scene = scene_for(&settings, directory.path());
        let rendered = settle(&mut scene, |scene| scene.scheduler.current().is_some());
        println!("--- {name} (density 100, ordered dither) ---");
        for line in rendered
            .braille_lines(100, DitherMode::Ordered)
            .iter()
            .take(20)
        {
            println!("{line}");
        }
        let lines = rendered.braille_lines(100, DitherMode::Ordered);
        assert!(lines.iter().any(|line| line.chars().any(|c| c != ' ')));
    }
}
