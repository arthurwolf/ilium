//! Measures Wind through the client's reusable raster and Braille packing path.
//! UI composition and terminal emission are outside this timing boundary.
#[path = "../../ilium-ambient/tests/support/mod.rs"]
mod ambient_fixture;

use ilium_ambient::OccupancyMask;
use ilium_client::background_animation::{AnimationFrame, AnimationKind, AnimationSettings};
use image::{GrayImage, Luma};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const SOURCES: [&str; 12] = [
    "ilium-client/examples/wind_client_performance.rs",
    "ilium-client/src/background_animation/mod.rs",
    "ilium-client/src/background_animation/host.rs",
    "ilium-ambient/src/scenes/wind/sim.rs",
    "ilium-ambient/src/scenes/wind/flow.rs",
    "ilium-ambient/src/scenes/wind/mod.rs",
    "ilium-ambient/src/scenes/wind/simd.rs",
    "ilium-ambient/src/scenes/wind/settings.rs",
    "ilium-ambient/src/scene.rs",
    "ilium-ambient/src/raster.rs",
    "ilium-ambient/src/dither.rs",
    "ilium-ambient/src/style.rs",
];
const OPTIONAL_SOURCES: [&str; 1] = ["ilium-ambient/src/scenes/wind/simd.rs"];

fn hash_source_files(
    source_root: &Path,
    source_paths: &[&str],
    optional_paths: &[&str],
) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for source_path in source_paths {
        let path = source_root.join(source_path);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && optional_paths.contains(source_path) =>
            {
                continue;
            }
            Err(error) => {
                return Err(
                    format!("cannot read benchmark source {}: {error}", path.display()).into(),
                );
            }
        };
        hashes.insert(
            (*source_path).to_owned(),
            format!("{:x}", Sha256::digest(bytes)),
        );
    }
    Ok(hashes)
}

#[derive(Clone)]
struct Options {
    width: u16,
    height: u16,
    fps: u32,
    warmup: u32,
    frames: u32,
    dots: Option<u32>,
    occupied: bool,
    merge_dots: bool,
    pointer: Option<[f32; 2]>,
    paired_pointer: bool,
    capture_dir: Option<PathBuf>,
    capture_subdir: Option<PathBuf>,
}

fn validate_capture_subdirectory(path: &std::path::Path) -> Result<PathBuf> {
    use std::path::Component;

    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(
            "capture subdirectory must be a non-empty relative path without traversal".into(),
        );
    }
    Ok(path.to_path_buf())
}

fn resolve_capture_directory(options: &Options) -> Result<Option<PathBuf>> {
    if let Some(directory) = &options.capture_dir {
        return Ok(Some(directory.clone()));
    }
    let Some(subdirectory) = &options.capture_subdir else {
        return Ok(None);
    };
    let target_directory = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(directory) => PathBuf::from(directory),
        None => std::env::current_exe()?
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::parent)
            .ok_or("cannot locate Cargo target directory for captures")?
            .to_path_buf(),
    };
    if !target_directory.is_absolute() {
        return Err("Cargo target directory must be absolute".into());
    }
    Ok(Some(target_directory.join(subdirectory)))
}

fn parse_pointer_position(value: &str) -> Result<[f32; 2]> {
    let (x, y) = value
        .split_once(',')
        .ok_or("pointer position must have the form X,Y")?;
    let position = [x.parse::<f32>()?, y.parse::<f32>()?];
    if !position
        .iter()
        .all(|coordinate| coordinate.is_finite() && (0.0..=1.0).contains(coordinate))
    {
        return Err("pointer coordinates must be finite and between 0 and 1".into());
    }
    Ok(position)
}

fn pointer_workloads(
    paired_pointer: bool,
    pointer: Option<[f32; 2]>,
) -> Result<Vec<(&'static str, Option<[f32; 2]>)>> {
    if paired_pointer && pointer.is_some() {
        return Err("--paired-pointer cannot be combined with --pointer".into());
    }
    if paired_pointer {
        Ok(vec![("no-pointer", None), ("pointer", Some([0.5, 0.5]))])
    } else {
        Ok(vec![("single", pointer)])
    }
}

fn checksum(frame: &AnimationFrame, width: u16, height: u16) -> u64 {
    let mut checksum = 0xcbf2_9ce4_8422_2325_u64;
    for row in 0..height {
        for column in 0..width {
            checksum ^= u64::from(frame.glyph(column, row) as u32);
            checksum = checksum.wrapping_mul(0x100_0000_01b3);
        }
    }
    checksum
}

fn wind_glyph_bits(glyph: char) -> Option<u8> {
    match glyph {
        ' ' => Some(0),
        '\u{2800}'..='\u{28ff}' => Some((glyph as u32 - 0x2800) as u8),
        // Merged-dot density glyphs are represented at Braille-cell resolution
        // for benchmark captures; the terminal renderer retains the real glyph.
        '\u{2022}' => Some(0b0011_0110),
        '\u{25cf}' => Some(0b1111_1111),
        '\u{25c9}' => Some(0b1100_1001),
        _ => None,
    }
}

fn save_braille_png(
    frame: &AnimationFrame,
    width: u16,
    height: u16,
    path: &std::path::Path,
) -> Result<()> {
    const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
    let mut image = GrayImage::new(u32::from(width) * 2, u32::from(height) * 4);
    for row in 0..height {
        for column in 0..width {
            let glyph = frame.glyph(column, row);
            let bits = wind_glyph_bits(glyph)
                .ok_or_else(|| format!("unexpected Wind capture glyph: {glyph:?}"))?;
            for (dot_row, bit_row) in BITS.iter().enumerate() {
                for (dot_column, bit) in bit_row.iter().enumerate() {
                    if bits & *bit != 0 {
                        image.put_pixel(
                            u32::from(column) * 2 + dot_column as u32,
                            u32::from(row) * 4 + dot_row as u32,
                            Luma([255]),
                        );
                    }
                }
            }
        }
    }
    image.save(path)?;
    Ok(())
}

fn parse_options() -> Result<Option<Options>> {
    let mut options = Options {
        width: 160,
        height: 50,
        fps: 30,
        warmup: 120,
        frames: 3000,
        dots: None,
        occupied: true,
        merge_dots: false,
        pointer: None,
        paired_pointer: false,
        capture_dir: None,
        capture_subdir: None,
    };
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        if flag == "--help" {
            println!(
                "{}",
                json!({
                    "type": "help",
                    "usage": "wind_client_performance [--width N] [--height N] [--fps N] [--warmup N] [--frames N] [--dots 10|20000|50000] [--occupied|--no-occupied] [--merge-dots] [--pointer X,Y|--paired-pointer] [--capture-dir ABS|--capture-subdir REL]",
                    "defaults": {"width":160,"height":50,"fps":30,"warmup":120,"frames":3000,"dots":[20000,50000],"occupied":true,"merge_dots":false},
                    "note": "--dots 10 is a sparse baseline for fixed scene and raster-packing cost; it is not a target workload"
                })
            );
            return Ok(None);
        }
        match flag.as_str() {
            "--occupied" => options.occupied = true,
            "--no-occupied" => options.occupied = false,
            "--merge-dots" => options.merge_dots = true,
            "--paired-pointer" => options.paired_pointer = true,
            _ => {
                let value = arguments.next().ok_or("flag requires a value")?;
                match flag.as_str() {
                    "--width" => options.width = value.parse()?,
                    "--height" => options.height = value.parse()?,
                    "--fps" => options.fps = value.parse()?,
                    "--warmup" => options.warmup = value.parse()?,
                    "--frames" => options.frames = value.parse()?,
                    "--dots" => options.dots = Some(value.parse()?),
                    "--pointer" => options.pointer = Some(parse_pointer_position(&value)?),
                    "--capture-dir" => options.capture_dir = Some(PathBuf::from(value)),
                    "--capture-subdir" => {
                        options.capture_subdir = Some(validate_capture_subdirectory(
                            PathBuf::from(value).as_path(),
                        )?)
                    }
                    _ => return Err(format!("unknown option: {flag}").into()),
                }
            }
        }
    }
    if options.width == 0
        || options.height == 0
        || options.width > 320
        || options.height > 120
        || !(1..=60).contains(&options.fps)
        || options.warmup > 1000
        || !(1..=10000).contains(&options.frames)
        || options
            .dots
            .is_some_and(|dots| ![10, 20_000, 50_000].contains(&dots))
    {
        return Err(
            "dimensions, fps, frame counts, or dot count are outside the documented bounds".into(),
        );
    }
    if options
        .capture_dir
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err("capture directory must be an absolute path".into());
    }
    if options.capture_dir.is_some() && options.capture_subdir.is_some() {
        return Err("choose only one capture directory option".into());
    }
    pointer_workloads(options.paired_pointer, options.pointer)?;
    Ok(Some(options))
}

fn measure(
    options: &Options,
    dots: u32,
    resources: &ilium_ambient::resources::AmbientResources,
) -> Result<()> {
    let mut settings = AnimationSettings {
        enabled: true,
        kind: AnimationKind::Wind,
        ..AnimationSettings::default()
    };
    settings.ambient.wind.seed = 1;
    settings.ambient.wind.dot_count = dots;
    settings.ambient.wind.frame_rate = options.fps;
    settings.ambient.wind.merge_dots = options.merge_dots;

    let mut frame = AnimationFrame::default();
    frame.configure_resources(resources.clone());
    frame.pointer(options.pointer);
    if options.occupied {
        let mask = OccupancyMask::from_fn(options.width, options.height, |column, row| {
            (usize::from(column) / 8 + usize::from(row) / 3) % 5 == 0
        });
        frame.set_occupancy(Some(Arc::new(mask)), 1);
    }

    let mut samples = Vec::with_capacity(options.frames as usize);
    let mut frame_checksum = 0_u64;
    let capture_frames = [
        options.warmup,
        options.warmup + options.frames / 2,
        options.warmup + options.frames - 1,
    ];
    let mut captures = Vec::new();
    for index in 0..(options.warmup + options.frames) {
        let elapsed =
            Duration::from_nanos((u64::from(index + 1) * 1_000_000_000) / u64::from(options.fps));
        let started = Instant::now();
        frame.render(&settings, options.width, options.height, elapsed);
        let render_ns = started.elapsed().as_nanos() as u64;
        frame_checksum = checksum(&frame, options.width, options.height);
        if index >= options.warmup {
            samples.push(render_ns);
        }
        if capture_frames.contains(&index) {
            if let Some(directory) = &options.capture_dir {
                let output = directory.join(format!(
                    "wind-{dots}-frame-{:05}.png",
                    index - options.warmup
                ));
                save_braille_png(&frame, options.width, options.height, &output)?;
                captures.push(output.display().to_string());
            }
        }
    }
    samples.sort_unstable();
    let total: u128 = samples.iter().map(|sample| u128::from(*sample)).sum();
    let count = samples.len();
    let source_hashes = hash_source_files(&std::env::current_dir()?, &SOURCES, &OPTIONAL_SOURCES)?;
    println!(
        "{}",
        json!({
            "type":"result",
            "scene":"wind",
            "path":"ilium-client::AnimationFrame::render",
            "width":options.width,
            "height":options.height,
            "fps":options.fps,
            "dots":dots,
            "warmup_frames":options.warmup,
            "frames":count,
            "occupied":options.occupied,
            "merge_dots":options.merge_dots,
            "pointer":options.pointer,
            "mean_render_and_pack_ns":total as f64 / count as f64,
            "median_render_and_pack_ns":samples[count / 2],
            "p95_render_and_pack_ns":samples[(count - 1) * 95 / 100],
            "checksum":frame_checksum,
            "captures":captures,
            "source_hashes":source_hashes,
            "scope":"client reusable raster clear, Wind scene render, and Braille cell packing; checksum scan, UI composition, and terminal I/O are excluded"
        })
    );
    Ok(())
}

fn run() -> Result<()> {
    let Some(mut options) = parse_options()? else {
        return Ok(());
    };
    options.capture_dir = resolve_capture_directory(&options)?;
    let fixture = ambient_fixture::ResourcesFixture::new()?;
    if let Some(directory) = &options.capture_dir {
        std::fs::create_dir_all(directory)?;
    }
    let dots = options
        .dots
        .map_or_else(|| vec![20_000, 50_000], |dots| vec![dots]);
    for dot_count in dots {
        for (label, pointer) in pointer_workloads(options.paired_pointer, options.pointer)? {
            let mut workload = options.clone();
            workload.pointer = pointer;
            workload.paired_pointer = false;
            if options.paired_pointer {
                workload.capture_dir = options
                    .capture_dir
                    .as_ref()
                    .map(|directory| directory.join(label));
                if let Some(directory) = &workload.capture_dir {
                    fs::create_dir_all(directory)?;
                }
            }
            measure(&workload, dot_count, &fixture.resources)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        hash_source_files, parse_pointer_position, pointer_workloads,
        validate_capture_subdirectory, wind_glyph_bits,
    };
    use std::fs;
    use std::path::Path;

    #[test]
    fn parses_normalized_pointer_coordinates() {
        assert_eq!(parse_pointer_position("0.5,0.25").unwrap(), [0.5, 0.25]);
        assert_eq!(parse_pointer_position("0,1").unwrap(), [0.0, 1.0]);
    }

    #[test]
    fn rejects_malformed_non_finite_and_out_of_range_pointer_coordinates() {
        for value in [
            "0.5",
            "0.2,0.3,0.4",
            "NaN,0.5",
            "0.5,inf",
            "-0.1,0.5",
            "0.5,1.1",
        ] {
            assert!(parse_pointer_position(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn paired_pointer_workload_runs_unforced_and_center_forced_cases() {
        assert_eq!(
            pointer_workloads(true, None).unwrap(),
            vec![("no-pointer", None), ("pointer", Some([0.5, 0.5]))]
        );
        assert!(pointer_workloads(true, Some([0.25, 0.75])).is_err());
        assert_eq!(
            pointer_workloads(false, Some([0.25, 0.75])).unwrap(),
            vec![("single", Some([0.25, 0.75]))]
        );
    }

    #[test]
    fn accepts_only_relative_capture_subdirectories_without_parent_traversal() {
        assert!(validate_capture_subdirectory(Path::new("wind-captures/run-1")).is_ok());
        for path in ["", ".", "../outside", "wind/../../outside", "/tmp/captures"] {
            assert!(
                validate_capture_subdirectory(Path::new(path)).is_err(),
                "accepted unsafe capture subdirectory {path:?}"
            );
        }
    }

    #[test]
    fn maps_merged_wind_density_glyphs_to_capture_pixels() {
        assert_eq!(wind_glyph_bits('•'), Some(0b0011_0110));
        assert_eq!(wind_glyph_bits('●'), Some(0b1111_1111));
        assert_eq!(wind_glyph_bits('◉'), Some(0b1100_1001));
        assert_eq!(wind_glyph_bits(' '), Some(0));
        assert_eq!(wind_glyph_bits('x'), None);
    }

    #[test]
    fn source_manifest_hashes_existing_files_and_omits_absent_optional_files() {
        let root = std::env::temp_dir().join(format!(
            "ilium-wind-source-hash-test-{}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create exact temporary test directory");
        fs::write(root.join("present.rs"), b"baseline source").expect("write source fixture");

        let hashes = hash_source_files(&root, &["present.rs", "optional.rs"], &["optional.rs"])
            .expect("hash source manifest");
        assert!(hashes.contains_key("present.rs"));
        assert!(!hashes.contains_key("optional.rs"));
        assert!(hash_source_files(&root, &["missing.rs"], &[]).is_err());

        fs::remove_file(root.join("present.rs")).expect("remove test-owned source fixture");
        fs::remove_dir(root).expect("remove empty test-owned directory");
    }
}
