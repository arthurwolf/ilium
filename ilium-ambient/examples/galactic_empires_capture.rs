//! Capture actual dithered galaxy dot output as PNG and Braille JSONL.
//! --output-dir PATH [--width N] [--height N] [--times SECONDS_CSV]
//! [--settings-json JSON] [--step-seconds N]
use ilium_ambient::{
    debug::render_frame, raster::threshold, AmbientKind, AmbientSettings, DitherMode, SceneEnv,
};
use std::{path::PathBuf, time::Duration};

fn run() -> Result<(), String> {
    let mut output = None;
    let (mut width, mut height) = (120_u16, 40_u16);
    let mut times = vec![0.0_f64, 30.0, 120.0, 300.0];
    let mut settings = AmbientSettings::default();
    let mut step = 0.5_f64;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--output-dir" => output = Some(PathBuf::from(value)),
            "--width" => width = value.parse().map_err(|_| "bad width")?,
            "--height" => height = value.parse().map_err(|_| "bad height")?,
            "--times" => {
                times = value
                    .split(',')
                    .map(|part| part.parse().map_err(|_| "bad time".to_owned()))
                    .collect::<Result<_, _>>()?
            }
            "--settings-json" => {
                settings = serde_json::from_str(&value).map_err(|error| error.to_string())?
            }
            "--step-seconds" => step = value.parse().map_err(|_| "bad step")?,
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    if width == 0
        || height == 0
        || !step.is_finite()
        || step <= 0.0
        || times.iter().any(|time| !time.is_finite() || *time < 0.0)
    {
        return Err("positive dimensions/step and finite nonnegative times required".to_owned());
    }
    let output = output.ok_or("--output-dir required")?;
    std::fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    times.sort_by(f64::total_cmp);
    let mut scene = settings.create_scene(
        AmbientKind::GalacticEmpires,
        &SceneEnv::for_test(output.join("unused-cache")),
    );
    let mut clock = 0.0;
    for (index, time) in times.into_iter().enumerate() {
        while clock + step < time {
            clock += step;
            render_frame(scene.as_mut(), 1, 1, Duration::from_secs_f64(clock));
        }
        clock = time;
        let frame = render_frame(scene.as_mut(), width, height, Duration::from_secs_f64(time));
        let mut pixels = image::RgbImage::new(u32::from(width) * 8, u32::from(height) * 16);
        for y in 0..frame.raster.height {
            for x in 0..frame.raster.width {
                let intensity = frame.raster.dots[y * frame.raster.width + x];
                if intensity * 0.6 <= threshold(x, y, DitherMode::Ordered) {
                    continue;
                }
                let color = frame.cell_colors[(y / 4) * usize::from(width) + x / 2];
                for py in 0..3 {
                    for px in 0..3 {
                        pixels.put_pixel(x as u32 * 4 + px, y as u32 * 4 + py, image::Rgb(color));
                    }
                }
            }
        }
        let path = output.join(format!("frame-{index:03}.png"));
        pixels.save(&path).map_err(|error| error.to_string())?;
        println!(
            "{}",
            serde_json::json!({"type":"artifact","time":time,"path":path,"width":width,"height":height,"settings":settings.galactic_empires,"density_percent":60,"dither":"ordered","status":scene.status(),"lines":frame.braille_lines(60,DitherMode::Ordered)})
        );
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        println!("{}", serde_json::json!({"type":"error","error":error}));
        std::process::exit(1);
    }
}
