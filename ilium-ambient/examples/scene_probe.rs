//! Render one ambient scene headlessly and print JSONL frames.
//! Usage: scene_probe --kind pipes|stars|night_lights|clouds|video|spectrum|images
//!        [--width N] [--height N] [--times SECONDS_CSV] [--wait-ms N] [--density PCT] [--settings-json JSON]
//!        [--png-output NEW_DIR] writes one PNG per frame, with JSONL artifact paths.
//! Each output line is {"type":"frame","kind":..,"time":..,"status":..,"lines":[..]}.

#[path = "../tests/support/mod.rs"]
mod ambient_fixture;

use ilium_ambient::debug::render_frame;
#[cfg(test)]
use ilium_ambient::Raster;
use ilium_ambient::{AmbientKind, AmbientSettings, DitherMode, SceneEnv};
use image::ImageEncoder;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Use the same unquantized coverage and Ordered mask as the JSONL Braille.
fn rendered_rgb(rendered: &ilium_ambient::debug::Rendered, density_percent: u16) -> Vec<u8> {
    let width = usize::from(rendered.width) * 2;
    let height = usize::from(rendered.height) * 4;
    let density = f32::from(density_percent) / 100.0;
    let mut pixels = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let coverage = rendered.raster.dots[y * rendered.raster.width + x];
            let lit =
                coverage * density > ilium_ambient::raster::threshold(x, y, DitherMode::Ordered);
            let color = if lit {
                rendered.cell_colors[(y / 4) * usize::from(rendered.width) + x / 2]
            } else {
                [8, 12, 14]
            };
            pixels.extend_from_slice(&color);
        }
    }
    pixels
}

fn write_png(
    path: &Path,
    rendered: &ilium_ambient::debug::Rendered,
    density: u16,
) -> Result<(), String> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("create {}: {error}", path.display()))?;
    image::codecs::png::PngEncoder::new(file)
        .write_image(
            &rendered_rgb(rendered, density),
            u32::from(rendered.width) * 2,
            u32::from(rendered.height) * 4,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|error| format!("encode {}: {error}", path.display()))
}

fn run() -> Result<(), String> {
    let resources_fixture = ambient_fixture::ResourcesFixture::new()?;
    let mut kind = None;
    let (mut width, mut height, mut wait_ms) = (100_u16, 30_u16, 0_u64);
    let mut times = vec![0.0_f64, 5.0];
    let mut settings_json = None;
    let mut png_output = None;
    let mut density = 60_u16;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--kind" => {
                kind = serde_json::from_value::<AmbientKind>(serde_json::Value::String(value)).ok()
            }
            "--width" => width = value.parse().map_err(|_| "bad width")?,
            "--height" => height = value.parse().map_err(|_| "bad height")?,
            "--settings-json" => settings_json = Some(value),
            "--png-output" => png_output = Some(PathBuf::from(value)),
            "--density" => density = value.parse().map_err(|_| "bad density")?,
            "--wait-ms" => wait_ms = value.parse().map_err(|_| "bad wait")?,
            "--times" => {
                times = value
                    .split(',')
                    .map(|part| part.parse::<f64>().map_err(|_| "bad time"))
                    .collect::<Result<_, _>>()?
            }
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    let kind = kind.ok_or("--kind is required")?;
    let cache = std::env::temp_dir().join("ilium-scene-probe");
    let env = SceneEnv::for_test(cache, resources_fixture.resources.clone());
    let settings: AmbientSettings = match settings_json {
        Some(text) => serde_json::from_str(&text).map_err(|error| error.to_string())?,
        None => AmbientSettings::default(),
    };
    let png_output = png_output
        .map(|directory| {
            if width == 0 || height == 0 {
                return Err("PNG output requires nonzero width and height".to_owned());
            }
            // Refuse an existing directory to preserve earlier capture evidence.
            std::fs::create_dir(&directory).map_err(|error| {
                format!("create PNG directory {}: {error}", directory.display())
            })?;
            std::fs::canonicalize(&directory)
                .map_err(|error| format!("resolve PNG directory {}: {error}", directory.display()))
        })
        .transpose()?;
    let mut scene = settings.create_scene(kind, &env);
    for (index, time) in times.into_iter().enumerate() {
        if wait_ms > 0 {
            std::thread::sleep(Duration::from_millis(wait_ms));
        }
        let rendered = render_frame(scene.as_mut(), width, height, Duration::from_secs_f64(time));
        println!(
            "{}",
            serde_json::json!({
                "type": "frame",
                "kind": format!("{kind:?}"),
                "time": time,
                "status": scene.status(),
                "lines": rendered.braille_lines(density, DitherMode::Ordered),
            })
        );
        if let Some(directory) = &png_output {
            let path = directory.join(format!("frame-{index:05}.png"));
            write_png(&path, &rendered, density)?;
            println!(
                "{}",
                serde_json::json!({
                    "type": "artifact", "path": path, "format": "png",
                    "frame_index": index, "time": time,
                    "width": u32::from(width) * 2, "height": u32::from(height) * 4,
                    "density_percent": density, "dither": "ordered",
                    "background_rgb": [8, 12, 14],
                })
            );
        }
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        println!("{}", serde_json::json!({"type":"error","message":message}));
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored_fixture() -> ilium_ambient::debug::Rendered {
        let mut raster = Raster::default();
        raster.resize(6, 8);
        for (index, value) in raster.dots.iter_mut().enumerate() {
            let threshold =
                ilium_ambient::raster::threshold(index % 6, index / 6, DitherMode::Ordered);
            *value = match index % 5 {
                0 => 0.0,
                1 => 1.0,
                2 => threshold,
                3 => (threshold - 0.0001).max(0.0),
                _ => (threshold + 0.0001).min(1.0),
            };
        }
        ilium_ambient::debug::Rendered {
            width: 3,
            height: 2,
            raster,
            cell_colors: vec![
                [180, 120, 60],
                [30, 150, 210],
                [220, 40, 70],
                [90, 200, 45],
                [130, 65, 190],
                [240, 210, 100],
            ],
        }
    }

    #[test]
    fn rgb_mask_matches_every_braille_bit_across_densities_and_cell_boundaries() {
        const BITS: [[u32; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let rendered = colored_fixture();
        let mut saw_lit = false;
        let mut saw_dark = false;
        for density in [0, 25, 60, 100, 200] {
            let pixels = rendered_rgb(&rendered, density);
            assert_eq!(pixels.len(), 6 * 8 * 3);
            let cells: Vec<Vec<char>> = rendered
                .braille_lines(density, DitherMode::Ordered)
                .iter()
                .map(|line| line.chars().collect())
                .collect();
            for (index, pixel) in pixels.chunks_exact(3).enumerate() {
                let (x, y) = (index % 6, index / 6);
                let glyph = cells[y / 4][x / 2];
                let mask = if glyph == ' ' {
                    0
                } else {
                    u32::from(glyph) - 0x2800
                };
                let lit = mask & BITS[y % 4][x % 2] != 0;
                saw_lit |= lit;
                saw_dark |= !lit;
                let expected = if lit {
                    rendered.cell_colors[(y / 4) * 3 + x / 2]
                } else {
                    [8, 12, 14]
                };
                assert_eq!(
                    pixel, &expected,
                    "density={density}, dot=({x},{y}), glyph={glyph:?}"
                );
            }
        }
        assert!(saw_lit && saw_dark);
    }

    #[test]
    fn exact_threshold_is_dark_and_partial_coverage_above_it_is_lit() {
        let mut rendered = colored_fixture();
        for (index, value) in rendered.raster.dots.iter_mut().enumerate() {
            *value = ilium_ambient::raster::threshold(index % 6, index / 6, DitherMode::Ordered);
        }
        assert!(rendered_rgb(&rendered, 100)
            .chunks_exact(3)
            .all(|pixel| pixel == [8, 12, 14]));
        for value in &mut rendered.raster.dots {
            *value += 0.0001;
        }
        for (index, pixel) in rendered_rgb(&rendered, 100).chunks_exact(3).enumerate() {
            let (x, y) = (index % 6, index / 6);
            assert_eq!(pixel, &rendered.cell_colors[(y / 4) * 3 + x / 2]);
        }
    }

    #[test]
    fn encoded_png_preserves_pixels_dimensions_and_existing_output() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("frame-00000.png");
        let rendered = colored_fixture();
        write_png(&path, &rendered, 60).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (6, 8));
        assert_eq!(decoded.as_raw(), &rendered_rgb(&rendered, 60));
        assert!(write_png(&path, &rendered, 100).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn png_pixels_use_the_frame_cell_color_only_for_emitted_dots() {
        let mut raster = Raster::default();
        raster.resize(2, 4);
        raster.dots[0] = 1.0;
        let rendered = ilium_ambient::debug::Rendered {
            width: 1,
            height: 1,
            raster,
            cell_colors: vec![[180, 120, 60]],
        };

        let pixels = rendered_rgb(&rendered, 100);

        assert_eq!(&pixels[0..3], &[180, 120, 60]);
        assert_eq!(&pixels[3..6], &[8, 12, 14]);
        assert_eq!(pixels.len(), 2 * 4 * 3);
    }
}
