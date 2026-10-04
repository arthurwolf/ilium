//! Render one ambient scene headlessly and print JSONL frames.
//! Usage: scene_probe --kind pipes|stars|night_lights|clouds|video|spectrum|images
//!        [--width N] [--height N] [--times SECONDS_CSV] [--wait-ms N] [--density PCT] [--settings-json JSON]
//! Each output line is {"type":"frame","kind":..,"time":..,"status":..,"lines":[..]}.

#[path = "../tests/support/mod.rs"]
mod ambient_fixture;

use ilium_ambient::debug::render_frame;
use ilium_ambient::{AmbientKind, AmbientSettings, DitherMode, SceneEnv};
use std::time::Duration;

fn run() -> Result<(), String> {
    let resources_fixture = ambient_fixture::ResourcesFixture::new()?;
    let mut kind = None;
    let (mut width, mut height, mut wait_ms) = (100_u16, 30_u16, 0_u64);
    let mut times = vec![0.0_f64, 5.0];
    let mut settings_json = None;
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
    let mut scene = settings.create_scene(kind, &env);
    for time in times {
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
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        println!("{}", serde_json::json!({"type":"error","message":message}));
        std::process::exit(2);
    }
}
