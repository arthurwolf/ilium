//! Measure the Wind scene's actual software frame path.
//! Usage: wind_performance [--frames N] [--width N] [--height N] [--dots N] [--fps N] [--occupied] [--merge-dots]

#[path = "../tests/support/mod.rs"]
mod ambient_fixture;

use ilium_ambient::debug::render_frame;
use ilium_ambient::scene::OccupancyMask;
use ilium_ambient::{AmbientKind, AmbientSettings, DitherMode, SceneEnv};
use std::time::{Duration, Instant};

fn main() {
    let mut frames = 300_u32;
    let mut width = 160_u16;
    let mut height = 50_u16;
    let mut dots = None;
    let mut fps = 20_u32;
    let mut occupied = false;
    let mut merge_dots = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        if flag == "--occupied" {
            occupied = true;
            continue;
        }
        if flag == "--merge-dots" {
            merge_dots = true;
            continue;
        }
        let value = arguments.next().unwrap_or_default();
        match flag.as_str() {
            "--frames" => frames = value.parse().expect("invalid frame count"),
            "--width" => width = value.parse().expect("invalid width"),
            "--height" => height = value.parse().expect("invalid height"),
            "--dots" => dots = Some(value.parse().expect("invalid dot count")),
            "--fps" => fps = value.parse().expect("invalid frame rate"),
            _ => panic!("unknown option: {flag}"),
        }
    }
    assert!(frames > 0 && frames <= 100_000 && fps > 0);
    let fixture = ambient_fixture::ResourcesFixture::new().expect("resources fixture");
    let environment = SceneEnv::for_test(
        std::env::temp_dir().join("ilium-wind-performance"),
        fixture.resources,
    );
    let settings = AmbientSettings::default();
    let mut settings = settings;
    if let Some(dot_count) = dots {
        settings.wind.dot_count = dot_count;
    }
    settings.wind.frame_rate = fps;
    settings.wind.merge_dots = merge_dots;
    let mut scene = settings.create_scene(AmbientKind::Wind, &environment);
    if occupied {
        let mask = OccupancyMask::from_fn(width, height, |column, row| {
            (usize::from(column) / 8 + usize::from(row) / 3) % 5 == 0
        });
        scene.occupancy(&mask);
    }
    let mut checksum = 0_u64;
    let mut elapsed = Vec::with_capacity(frames as usize);
    let mut end_to_end_elapsed = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        let started = Instant::now();
        let rendered = render_frame(
            scene.as_mut(),
            width,
            height,
            Duration::from_secs_f64(f64::from(index + 1) / f64::from(fps)),
        );
        elapsed.push(started.elapsed().as_nanos() as u64);
        let braille_lines = rendered.braille_lines(60, DitherMode::Ordered);
        end_to_end_elapsed.push(started.elapsed().as_nanos() as u64);
        checksum = checksum.wrapping_add(
            braille_lines
                .iter()
                .flat_map(|line| line.bytes())
                .map(u64::from)
                .sum::<u64>(),
        );
    }
    elapsed.sort_unstable();
    end_to_end_elapsed.sort_unstable();
    let total: u128 = elapsed.iter().map(|value| u128::from(*value)).sum();
    let mean = total as f64 / f64::from(frames);
    let median = elapsed[(frames as usize - 1) / 2];
    let p95 = elapsed[((frames as usize - 1) * 95) / 100];
    let end_to_end_total: u128 = end_to_end_elapsed
        .iter()
        .map(|value| u128::from(*value))
        .sum();
    let end_to_end_mean = end_to_end_total as f64 / f64::from(frames);
    let end_to_end_median = end_to_end_elapsed[(frames as usize - 1) / 2];
    let end_to_end_p95 = end_to_end_elapsed[((frames as usize - 1) * 95) / 100];
    println!(
        "{}",
        serde_json::json!({
            "type": "result",
            "scene": "wind",
            "width": width,
            "height": height,
            "frames": frames,
            "dots": settings.wind.dot_count,
            "fps": fps,
            "occupied": occupied,
            "merge_dots": merge_dots,
            "mean_render_ns": mean,
            "median_render_ns": median,
            "min_render_ns": elapsed[0],
            "p95_render_ns": p95,
            "mean_render_and_encode_ns": end_to_end_mean,
            "median_render_and_encode_ns": end_to_end_median,
            "p95_render_and_encode_ns": end_to_end_p95,
            "checksum": checksum,
            "scope": "render_ns measures Wind rendering; render_and_encode_ns also includes Braille conversion; checksum scan and terminal I/O are excluded"
        })
    );
}
