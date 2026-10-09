//! Capture frames from the production saved-world scene for visual acceptance.
//!
//! The output is the scene's actual Braille-dot raster and cell colors expanded
//! to pixels. Each frame carries the renderer's saved-world source and material
//! coverage receipt; no map files are written.

use ilium_ambient::{
    minecraft::{
        saved_runtime::SavedRuntime,
        saved_scene::{PinnedSceneSource, SavedScene},
        settings::WorldSource,
    },
    raster::{self, DitherMode, Raster},
    registry::AmbientSettings,
    scene::{Frame, SavedWorldFrameEvidence, Scene, SceneEnv},
    voxel_landscape::{
        assets::identity::{AssetPath, Digest256},
        pack_profiles::FULL_PACKS,
    },
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

struct Arguments {
    saves_root: PathBuf,
    world: String,
    native_jar: PathBuf,
    output: PathBuf,
    pack: Option<(usize, PathBuf)>,
    pack_root: Option<String>,
    width: u16,
    height: u16,
    zoom: u16,
    deadline: Duration,
    frame_count: usize,
    frame_interval: Duration,
}

fn parse_arguments() -> Result<Arguments, String> {
    parse_arguments_from(std::env::args().skip(1))
}

fn parse_arguments_from(mut arguments: impl Iterator<Item = String>) -> Result<Arguments, String> {
    let mut values = BTreeMap::<String, String>::new();
    while let Some(key) = arguments.next() {
        if ![
            "--saves-root",
            "--world",
            "--native-jar",
            "--output",
            "--pack-profile",
            "--pack-path",
            "--pack-root",
            "--width",
            "--height",
            "--zoom",
            "--deadline-seconds",
            "--frames",
            "--frame-interval-seconds",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unknown argument {key}"));
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?;
        if values.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate argument {key}"));
        }
    }

    let required = |key: &str| values.get(key).ok_or_else(|| format!("{key} is required"));
    let saves_root = PathBuf::from(required("--saves-root")?);
    let world = required("--world")?.clone();
    let native_jar = PathBuf::from(required("--native-jar")?);
    let output = PathBuf::from(required("--output")?);
    if !saves_root.is_absolute()
        || !saves_root.is_dir()
        || !native_jar.is_absolute()
        || !native_jar.is_file()
        || !output.is_absolute()
        || output.exists()
    {
        return Err(
            "existing absolute save root and Java archive, and a new absolute output path are required"
                .into(),
        );
    }
    let world_path = Path::new(&world);
    if world_path.components().count() != 1
        || !matches!(world_path.components().next(), Some(Component::Normal(_)))
    {
        return Err("--world must name one direct save folder".into());
    }

    let parse = |key: &str, default: &str| -> Result<u64, String> {
        values.get(key).map_or_else(
            || Ok(default.parse().expect("valid default")),
            |v| v.parse().map_err(|_| format!("invalid value for {key}")),
        )
    };
    let width = parse("--width", "128")? as u16;
    let height = parse("--height", "40")? as u16;
    let zoom = parse("--zoom", "100")? as u16;
    let deadline = parse("--deadline-seconds", "900")?;
    let frame_count = parse("--frames", "6")? as usize;
    let frame_interval = parse("--frame-interval-seconds", "16")?;
    if !(16..=240).contains(&width)
        || !(8..=120).contains(&height)
        || !(25..=400).contains(&zoom)
        || !(10..=1800).contains(&deadline)
        || !(2..=12).contains(&frame_count)
        || !(1..=120).contains(&frame_interval)
    {
        return Err(
            "capture dimensions, zoom, deadline, frame count, or interval is out of range".into(),
        );
    }

    let profile = values
        .get("--pack-profile")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| "invalid --pack-profile".to_owned())
        })
        .transpose()?;
    let pack_path = values.get("--pack-path").map(PathBuf::from);
    let pack = match (profile, pack_path) {
        (None, None) => None,
        (Some(index), Some(path))
            if index < FULL_PACKS.len() && path.is_absolute() && path.is_file() =>
        {
            Some((index, path))
        }
        _ => {
            return Err(
                "selected-pack capture requires a valid profile and absolute archive".into(),
            )
        }
    };
    let pack_root = values
        .get("--pack-root")
        .map(|root| root.trim())
        .filter(|root| !root.is_empty());
    let pack_root = match pack_root {
        None => None,
        Some(_) if pack.is_none() => {
            return Err("--pack-root requires a selected pack profile and archive".into())
        }
        Some(root)
            if root.len() > 512
                || root.chars().any(char::is_control)
                || AssetPath::parse(root).is_err() =>
        {
            return Err("--pack-root must be a safe relative asset folder".into())
        }
        Some(root) => Some(root.to_owned()),
    };

    Ok(Arguments {
        saves_root,
        world,
        native_jar,
        output,
        pack,
        pack_root,
        width,
        height,
        zoom,
        deadline: Duration::from_secs(deadline),
        frame_count,
        frame_interval: Duration::from_secs(frame_interval),
    })
}

fn emit(value: Value) {
    println!("{value}");
}

fn render_metadata(width: u16, height: u16, zoom: u16) -> Value {
    json!({
        "viewport_cells": {"width": width, "height": height},
        "viewport_pixels": {
            "width": usize::from(width) * 2,
            "height": usize::from(height) * 4,
        },
        "zoom_percent": zoom,
    })
}

fn frame_evidence_matches(
    evidence: &SavedWorldFrameEvidence,
    native_digest: Digest256,
    selected_digest: Option<Digest256>,
) -> bool {
    evidence.is_qualified()
        && evidence.source_profile == ilium_ambient::minecraft::native_assets::PROFILE
        && evidence.native_archive_sha256 == native_digest
        && evidence.selected_archive_sha256 == selected_digest
}

fn paint_png(
    raster: &Raster,
    cell_colors: &[[u8; 3]],
    width: u16,
    path: &Path,
) -> Result<(usize, Digest256), Box<dyn std::error::Error>> {
    let pixel_width = raster.width;
    let pixel_height = raster.height;
    let mut pixels = Vec::with_capacity(pixel_width * pixel_height * 3);
    let mut lit_dots = 0;
    for y in 0..pixel_height {
        for x in 0..pixel_width {
            let index = y * pixel_width + x;
            let threshold = raster::threshold(x, y, DitherMode::Ordered);
            let color = if raster.dots[index] > threshold {
                lit_dots += 1;
                let cell = (y / 4) * usize::from(width) + (x / 2);
                cell_colors[cell]
            } else {
                [0, 0, 0]
            };
            pixels.extend_from_slice(&color);
        }
    }
    let image = image::RgbImage::from_raw(
        u32::try_from(pixel_width)?,
        u32::try_from(pixel_height)?,
        pixels,
    )
    .ok_or("invalid raster dimensions")?;
    image.save(path)?;
    let digest = Digest256::of(&fs::read(path)?);
    Ok((lit_dots, digest))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments().map_err(|error| format!("arguments: {error}"))?;
    fs::create_dir(&arguments.output)?;
    let save_root = Arc::new(ilium_platform::secure_fs::NoFollowDirectory::open_root(
        &arguments.saves_root,
    )?);
    let pinned_root = Arc::new(ilium_platform::animation_files::PinnedDirectory::from_host(
        save_root,
    )?);
    let selected_directory = pinned_root.child(&arguments.world, false)?;
    let save_identity = format!("{:?}", selected_directory.identity());
    let native_jar = Arc::new(ilium_platform::animation_files::PinnedFile::from_host(
        fs::File::open(&arguments.native_jar)?,
    )?);
    let native_digest = Digest256::of(&fs::read(&arguments.native_jar)?);
    fs::create_dir_all(arguments.output.join("history"))?;

    let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 4,
        jobs: 8,
        service_jobs: 0,
        input_bytes: 512 * 1024 * 1024,
        result_bytes: 512 * 1024 * 1024,
        worker_threads: 16,
        worker_bytes: 2304 * 1024 * 1024,
    });
    let lane = ilium_execution::LaneConfig {
        threads: 1,
        queue_slots: 4,
        priority: None,
        resident_bytes_per_thread: 1024 * 1024,
    };
    let mut execution = ilium_execution::Execution::start(
        quota.clone(),
        ilium_execution::ExecutionConfig {
            cpu: lane,
            io: lane,
            service: ilium_execution::LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            },
        },
    )?;
    let client = execution
        .client(ilium_execution::ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 512 * 1024 * 1024,
        })
        .map_err(|reason| {
            std::io::Error::other(format!("capture client admission failed: {reason:?}"))
        })?;
    let env = SceneEnv::for_test(
        arguments.output.join("cache"),
        ilium_ambient::resources::AmbientResources::new(client),
    );

    let mut settings = AmbientSettings::default();
    settings.voxel_landscape.saved_maps.source = WorldSource::SavedMaps;
    settings.voxel_landscape.saved_maps.saves_folder =
        arguments.saves_root.to_string_lossy().into_owned();
    settings.voxel_landscape.zoom_percent = i32::from(arguments.zoom);
    let (profile_id, selected_digest) = if let Some((index, path)) = &arguments.pack {
        settings.voxel_landscape.pack_profile = *index;
        settings.voxel_landscape.pack_path = path.to_string_lossy().into_owned();
        settings.voxel_landscape.pack_root = arguments.pack_root.clone().unwrap_or_default();
        (FULL_PACKS[*index].id, Some(Digest256::of(&fs::read(path)?)))
    } else {
        settings.voxel_landscape.pack_path.clear();
        settings.voxel_landscape.pack_root.clear();
        ("java-default", None)
    };
    let source = PinnedSceneSource {
        root_label: arguments.saves_root.clone(),
        selected_world: Some(arguments.saves_root.join(&arguments.world)),
        selected_identity: Some(selected_directory.identity()),
        root: pinned_root,
        native_jar,
        history_storage: arguments.output.join("history"),
        history_root: None,
        stop: None,
        runtime: Arc::new(SavedRuntime::new()),
    };
    let mut scene = SavedScene::new_pinned(source, &settings.voxel_landscape, &env)?;
    let mut raster = Raster::default();
    let mut cell_colors =
        vec![[0_u8; 3]; usize::from(arguments.width) * usize::from(arguments.height)];
    let started = Instant::now();
    let mut attempts = 0_u64;
    let mut captured = Vec::with_capacity(arguments.frame_count);
    let mut last_progress = Instant::now();
    emit(json!({
        "type": "progress",
        "stage": "preparing saved world and selected textures",
        "world": arguments.world,
        "pack": profile_id,
        "elapsed_seconds": 0,
        "message": "Preparing a pinned saved-world scene and its selected texture pack"
    }));

    while captured.len() < arguments.frame_count {
        let elapsed = started.elapsed();
        raster.resize(
            usize::from(arguments.width) * 2,
            usize::from(arguments.height) * 4,
        );
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut cell_colors,
            width: arguments.width,
            height: arguments.height,
            time: elapsed,
            wall: elapsed,
            now: UNIX_EPOCH + elapsed,
        });
        attempts += 1;

        if let Some(evidence) = scene.saved_world_frame_evidence() {
            let evidence_matches =
                frame_evidence_matches(&evidence, native_digest, selected_digest);
            if !evidence_matches {
                return Err(
                    format!("frame asset receipt failed qualification: {evidence:?}").into(),
                );
            }
            let next_frame_at = arguments.frame_interval * captured.len() as u32;
            if elapsed >= next_frame_at {
                let image_path = arguments
                    .output
                    .join(format!("frame-{:03}.png", captured.len()));
                let (lit_dots, image_digest) =
                    paint_png(&raster, &cell_colors, arguments.width, &image_path)?;
                if lit_dots == 0 {
                    return Err("qualified saved-world frame emitted no visible raster dots".into());
                }
                let receipt = json!({
                    "type": "frame",
                    "index": captured.len(),
                    "elapsed_seconds": elapsed.as_secs(),
                    "render": render_metadata(arguments.width, arguments.height, arguments.zoom),
                    "image": image_path,
                    "image_sha256": image_digest,
                    "lit_dots": lit_dots,
                    "save_identity": save_identity,
                    "asset_evidence": evidence,
                });
                fs::write(
                    arguments
                        .output
                        .join(format!("frame-{:03}.json", captured.len())),
                    serde_json::to_vec_pretty(&receipt)?,
                )?;
                emit(receipt.clone());
                captured.push(receipt);
            }
        }
        if last_progress.elapsed() >= Duration::from_secs(3) {
            emit(json!({
                "type": "progress",
                "stage": "waiting for or rendering saved-world frames",
                "world": arguments.world,
                "pack": profile_id,
                "attempts": attempts,
                "frames": captured.len(),
                "frames_requested": arguments.frame_count,
                "elapsed_seconds": elapsed.as_secs(),
                "scene_status": scene.status(),
            }));
            last_progress = Instant::now();
        }
        if elapsed >= arguments.deadline {
            return Err(format!(
                "saved-world frame deadline expired after {} seconds; captured {}/{} frames; status={:?}",
                arguments.deadline.as_secs(),
                captured.len(),
                arguments.frame_count,
                scene.status()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(100));
    }

    let unique_images = captured
        .iter()
        .filter_map(|frame| frame.get("image_sha256").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    if unique_images.len() < 2 {
        return Err("the camera did not produce two visually distinct saved-world frames".into());
    }
    let manifest = json!({
        "type": "result",
        "world": arguments.world,
        "source_profile": profile_id,
        "render": render_metadata(arguments.width, arguments.height, arguments.zoom),
        "pack_archive": arguments.pack.as_ref().map(|(_, path)| path),
        "native_archive": arguments.native_jar,
        "save_identity": save_identity,
        "frames_requested": arguments.frame_count,
        "frames_captured": captured.len(),
        "distinct_images": unique_images.len(),
        "capture_attempts": attempts,
        "elapsed_seconds": started.elapsed().as_secs(),
        "frames": captured,
        "save_files_written": false,
    });
    fs::write(
        arguments.output.join("capture.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    emit(manifest);

    drop(scene);
    drop(env);
    execution.request_shutdown(ilium_execution::ShutdownMode::Cancel);
    execution
        .join_until_background(Instant::now() + Duration::from_secs(15))
        .map_err(|error| format!("capture worker shutdown: {error:?}"))?;
    emit(json!({
        "type": "result",
        "probe_workers_joined": true,
        "quota": format!("{:?}", quota.snapshot()),
    }));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        emit(json!({ "type": "error", "message": error.to_string() }));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_render_metadata_records_cell_pixel_dimensions_and_zoom() {
        let render = render_metadata(80, 24, 150);

        assert_eq!(render["viewport_cells"]["width"], 80);
        assert_eq!(render["viewport_cells"]["height"], 24);
        assert_eq!(render["viewport_pixels"]["width"], 160);
        assert_eq!(render["viewport_pixels"]["height"], 96);
        assert_eq!(render["zoom_percent"], 150);
    }

    #[test]
    fn frame_evidence_matches_java_default_and_selected_texture_packs() {
        let native_digest = Digest256::of(b"native archive");
        let selected_digest = Digest256::of(b"selected archive");
        let evidence = SavedWorldFrameEvidence {
            bank_epoch: Digest256::of(b"bank epoch"),
            source_profile: ilium_ambient::minecraft::native_assets::PROFILE,
            native_archive_sha256: native_digest,
            selected_archive_sha256: None,
            required_materials: 1,
            required_materials_satisfied: 1,
            visible_pixels: 1,
        };

        assert!(frame_evidence_matches(&evidence, native_digest, None,));
        assert!(!frame_evidence_matches(
            &evidence,
            Digest256::of(b"other native archive"),
            None,
        ));

        let selected_evidence = SavedWorldFrameEvidence {
            selected_archive_sha256: Some(selected_digest),
            ..evidence
        };
        assert!(frame_evidence_matches(
            &selected_evidence,
            native_digest,
            Some(selected_digest),
        ));
        assert!(!frame_evidence_matches(
            &selected_evidence,
            native_digest,
            None,
        ));
    }

    fn arguments(directory: &Path, selected_pack: bool, root: Option<&str>) -> Vec<String> {
        let saves = directory.join("saves");
        let archive = directory.join("pack.zip");
        let jar = directory.join("minecraft.jar");
        fs::create_dir_all(&saves).unwrap();
        fs::write(&jar, b"placeholder java archive").unwrap();
        fs::write(&archive, b"placeholder texture archive").unwrap();
        let mut values = vec![
            "--saves-root".into(),
            saves.to_string_lossy().into_owned(),
            "--world".into(),
            "Example".into(),
            "--native-jar".into(),
            jar.to_string_lossy().into_owned(),
            "--output".into(),
            directory.join("capture").to_string_lossy().into_owned(),
        ];
        if selected_pack {
            values.extend([
                "--pack-profile".into(),
                "0".into(),
                "--pack-path".into(),
                archive.to_string_lossy().into_owned(),
            ]);
        }
        if let Some(root) = root {
            values.extend(["--pack-root".into(), root.into()]);
        }
        values
    }

    #[test]
    fn capture_arguments_preserve_selected_pack_internal_root() {
        let directory = tempfile::tempdir().unwrap();
        let parsed = parse_arguments_from(
            arguments(
                directory.path(),
                true,
                Some("VoxelAssets/GoodVibes/minecraft"),
            )
            .into_iter(),
        )
        .unwrap();

        assert_eq!(
            parsed.pack_root.as_deref(),
            Some("VoxelAssets/GoodVibes/minecraft")
        );
    }

    #[test]
    fn capture_arguments_reject_unsafe_and_default_pack_roots() {
        let directory = tempfile::tempdir().unwrap();
        let unsafe_root =
            parse_arguments_from(arguments(directory.path(), true, Some("../outside")).into_iter());
        assert!(unsafe_root.is_err());

        let default_root = parse_arguments_from(
            arguments(directory.path(), false, Some("assets/minecraft")).into_iter(),
        );
        assert!(default_root.is_err());
    }
}
