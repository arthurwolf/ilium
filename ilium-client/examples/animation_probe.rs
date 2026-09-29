//! Release-mode scene cost and deterministic raster receipts.
//! Usage: animation_probe [--frames N] [--width N] [--height N] [--output-dir PATH]
//! [--sample-times SECONDS_CSV] [--settings-file PATH]

use ilium_client::background_animation::{AnimationFrame, AnimationKind, AnimationSettings};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn run() -> Result<(), String> {
    let mut frames = 120_u64;
    let mut width = 160_u16;
    let mut height = 50_u16;
    let mut output_dir = None;
    let mut sample_times = vec![0.0_f64, 8.0];
    let mut settings_file = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        if flag == "--help" {
            println!(
                "{}",
                serde_json::json!({"type":"help", "usage":"animation_probe [--frames N] [--width N] [--height N] [--output-dir PATH] [--sample-times SECONDS_CSV] [--settings-file PATH]"})
            );
            return Ok(());
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("Missing value for {flag}"))?;
        match flag.as_str() {
            "--frames" => frames = value.parse().map_err(|_| "Invalid frame count")?,
            "--width" => width = value.parse().map_err(|_| "Invalid width")?,
            "--height" => height = value.parse().map_err(|_| "Invalid height")?,
            "--output-dir" => output_dir = Some(PathBuf::from(value)),
            "--sample-times" => {
                sample_times = value
                    .split(',')
                    .map(|part| part.parse::<f64>().map_err(|_| "Invalid sample time"))
                    .collect::<Result<Vec<_>, _>>()?;
                if sample_times.is_empty()
                    || sample_times.len() > 120
                    || sample_times
                        .iter()
                        .any(|time| !time.is_finite() || *time < 0.0 || *time > 86400.0)
                {
                    return Err("Require 1..120 finite sample times in 0..86400 seconds".into());
                }
            }
            "--settings-file" => settings_file = Some(PathBuf::from(value)),
            _ => return Err(format!("Unknown flag {flag}")),
        }
    }
    if frames == 0 || frames > 100_000 || width == 0 || height == 0 || width > 1000 || height > 500
    {
        return Err("Require 1..100000 frames, 1..1000 columns, 1..500 rows".into());
    }
    if let Some(directory) = &output_dir {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    let baseline_settings = if let Some(path) = settings_file {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        serde_json::from_slice::<AnimationSettings>(&bytes)
            .map_err(|error| error.to_string())?
            .normalized()
    } else {
        AnimationSettings::default()
    };
    for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
        let mut frame = AnimationFrame::default();
        let settings = AnimationSettings {
            kind,
            ..baseline_settings
        };
        let initial_started = Instant::now();
        frame.render(&settings, width, height, Duration::ZERO);
        let initial_render_us = initial_started.elapsed().as_secs_f64() * 1_000_000.0;
        let mut previous = Vec::with_capacity(usize::from(width) * usize::from(height));
        for y in 0..height {
            for x in 0..width {
                previous.push(frame.glyph(x, y));
            }
        }
        let mut times = Vec::with_capacity(frames as usize);
        let mut changed_cells = 0_u64;
        for number in 1..=frames {
            let started = Instant::now();
            frame.render(
                &settings,
                width,
                height,
                Duration::from_nanos((number * 1_000_000_000).div_ceil(12)),
            );
            times.push(started.elapsed().as_secs_f64() * 1_000_000.0);
            for y in 0..height {
                for x in 0..width {
                    let cell = usize::from(y) * usize::from(width) + usize::from(x);
                    let glyph = frame.glyph(x, y);
                    changed_cells += u64::from(previous[cell] != glyph);
                    previous[cell] = glyph;
                }
            }
        }
        let mean = times.iter().sum::<f64>() / frames as f64;
        times.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({
                "type":"result", "scene":kind, "width":width, "height":height,
                "frames":frames, "renderer_mean_us":mean, "initial_render_us": initial_render_us,
                "renderer_p95_us":times[(times.len() - 1) * 95 / 100],
                "mean_changed_cells": changed_cells as f64 / frames as f64,
                "estimated_glyph_bytes_per_second_at_12fps": changed_cells as f64 / frames as f64 * 36.0,
                "scope":"scene renderer only; excludes UI, ANSI writes and terminal emulator"
            })
        );
        if let Some(directory) = &output_dir {
            for &seconds in &sample_times {
                frame.render(&settings, width, height, Duration::from_secs_f64(seconds));
                let rows: Vec<String> = (0..height)
                    .map(|y| (0..width).map(|x| frame.glyph(x, y)).collect())
                    .collect();
                let path = directory.join(format!("raster-{:02}-{seconds:06.3}s.json", index + 1));
                let receipt = serde_json::json!({"type":"artifact", "scene":kind,
                    "label":kind.label(),"seconds":seconds,"width":width,"height":height,"settings":settings,"rows":rows});
                std::fs::write(
                    &path,
                    serde_json::to_vec(&receipt).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                println!(
                    "{}",
                    serde_json::json!({"type":"artifact","path":path,"scene":kind})
                );
            }
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            println!("{}", serde_json::json!({"type":"error","message":message}));
            std::process::ExitCode::FAILURE
        }
    }
}
