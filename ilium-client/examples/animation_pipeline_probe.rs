//! Synthetic, deterministic animation stage benchmark. JSONL only; no real project data.
#[path = "../../ilium-ambient/tests/support/mod.rs"]
mod ambient_fixture;

use ilium_client::{
    app::App,
    background_animation::{
        AnimationFrame, AnimationKind, AnimationLoopCache, AnimationSettings, ShorelineStyle,
    },
    background_composition,
    config::MotionLevel,
};
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    Terminal, TerminalOptions, Viewport,
};
use std::{
    cell::Cell,
    io::{self, Write},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
struct OutputCount {
    bytes: Rc<Cell<usize>>,
    writes: Rc<Cell<usize>>,
}
impl Write for OutputCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.set(self.bytes.get() + bytes.len());
        self.writes.set(self.writes.get() + 1);
        std::hint::black_box(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn summarize(
    stage: &str,
    kind: AnimationKind,
    width: u16,
    height: u16,
    density: u16,
    mut samples: Vec<f64>,
    extra: serde_json::Value,
) {
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    samples.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"type":"result", "stage":stage,"scene":kind,"width":width,"height":height,"density_percent":density,"foreground":extra["foreground"],"shoreline_style":extra["shoreline_style"],"samples":samples.len(),"mean_us":mean,"median_us":samples[samples.len()/2],"p95_us":samples[(samples.len()-1)*95/100],"extra":extra})
    );
}
/// Synthetic foregrounds exercise safe-space detection, styling and wide
/// continuations. Create the template outside timed composition; its copy is
/// included in the directly timed display pipeline for nonempty fixtures.
fn foreground_fixture(area: Rect, selected: &str) -> Buffer {
    let mut buffer = Buffer::empty(area);
    for row in (0..area.height).step_by(4) {
        let text = match selected {
            "ascii" => "build: step 42/100    terminal output    input ready > ",
            "wide" => "界🙂e\u{301} terminal 界🙂e\u{301} clipped boundary",
            _ => continue,
        };
        buffer.set_stringn(
            area.x,
            area.y + row,
            text,
            usize::from(area.width),
            Style::default(),
        );
    }
    if selected == "styled" {
        for row in (0..area.height).step_by(4) {
            buffer.set_style(
                Rect::new(area.x, area.y + row, area.width, 1),
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::UNDERLINED),
            );
        }
    }
    buffer
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let resources = ambient_fixture::ResourcesFixture::new()?;
    let (mut width, mut height, mut frames, mut selected) =
        (160_u16, 50_u16, 300_u32, String::from("shoreline"));
    let mut density = 60_u16;
    let mut foreground = String::from("empty");
    let mut shoreline_style = String::from("rich");
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--help" {
            println!(
                "{}",
                serde_json::json!({"type":"help","usage":"animation_pipeline_probe --width N --height N --frames N --scene shoreline|all|SCENE_JSON_NAME --density 25..100 --foreground empty|ascii|styled|wide --shoreline-style rich|classic"})
            );
            return Ok(());
        }
        let value = args.next().ok_or("flag requires value")?;
        match flag.as_str() {
            "--width" => width = value.parse()?,
            "--height" => height = value.parse()?,
            "--frames" => frames = value.parse()?,
            "--scene" => selected = value,
            "--density" => density = value.parse()?,
            "--foreground" => foreground = value,
            "--shoreline-style" => shoreline_style = value,
            _ => return Err(format!("unknown flag {flag}").into()),
        }
    }
    if width == 0 || height == 0 || width > 500 || height > 200 || !(10..=10000).contains(&frames) {
        return Err("require dimensions1..500x1..200 and frames10..10000".into());
    }
    if !(25..=100).contains(&density) {
        return Err("density must be25..100".into());
    }
    if !["empty", "ascii", "styled", "wide"].contains(&foreground.as_str()) {
        return Err("foreground must be empty|ascii|styled|wide".into());
    }
    if !["rich", "classic"].contains(&shoreline_style.as_str()) {
        return Err("shoreline style must be rich|classic".into());
    }
    let summarize = |stage, kind, width, height, density, samples, mut extra: serde_json::Value| {
        extra["foreground"] = serde_json::Value::String(foreground.clone());
        extra["shoreline_style"] = serde_json::Value::String(shoreline_style.clone());
        summarize(stage, kind, width, height, density, samples, extra);
    };
    let kinds: Vec<_> = AnimationKind::ALL
        .into_iter()
        .take(10)
        .filter(|kind| {
            selected == "all"
                || serde_json::to_value(kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .as_deref()
                    == Some(selected.as_str())
        })
        .collect();
    if kinds.is_empty() {
        return Err("scene must name a built-in monochrome scene".into());
    }
    for kind in kinds {
        let mut settings = AnimationSettings {
            enabled: true,
            kind,
            loop_seconds: 1,
            density_percent: density,
            ..Default::default()
        };
        settings.shoreline.style = if shoreline_style == "classic" {
            ShorelineStyle::Classic
        } else {
            ShorelineStyle::Rich
        };
        let mut field = AnimationFrame::default();
        field.configure_resources(resources.resources.clone());
        let cold = Instant::now();
        field.render(&settings, width, height, Duration::ZERO);
        println!(
            "{}",
            serde_json::json!({"type":"result","stage":"cold_render","scene":kind,"width":width,"height":height,"density_percent":density,"foreground":foreground,"shoreline_style":shoreline_style,"us":cold.elapsed().as_secs_f64()*1e6})
        );
        let mut samples = Vec::new();
        for n in 1..=frames {
            let start = Instant::now();
            field.render(
                &settings,
                width,
                height,
                Duration::from_nanos(u64::from(n) * 1_000_000_000 / 30),
            );
            samples.push(start.elapsed().as_secs_f64() * 1e6);
            std::hint::black_box(field.glyph(width / 2, height / 2));
        }
        summarize(
            "render_and_pack",
            kind,
            width,
            height,
            density,
            samples,
            serde_json::json!({"clock_fps":30}),
        );
        // Pin scene geometry and alternate only density, forcing a repack
        // without scene simulation. Measure the requested-density call only.
        let mut alternate = settings.clone();
        alternate.density_percent = if density == 100 { 99 } else { 100 };
        field.render(&settings, width, height, Duration::ZERO);
        let mut repack = Vec::new();
        for _ in 0..frames {
            field.render(&alternate, width, height, Duration::ZERO);
            let start = Instant::now();
            field.render(&settings, width, height, Duration::ZERO);
            repack.push(start.elapsed().as_secs_f64() * 1e6);
            std::hint::black_box(field.glyph(width / 2, height / 2));
        }
        summarize(
            "repack_only",
            kind,
            width,
            height,
            density,
            repack,
            serde_json::json!({"scope":"same geometry; requested-density repack plus normalization; alternate density outside timer"}),
        );
        let mut app = App::new(
            "synthetic-benchmark".into(),
            PathBuf::from("/synthetic-animation-benchmark"),
        );
        app.animation_frame
            .configure_resources(resources.resources.clone());
        app.animation_settings = settings.clone();
        app.ui_settings.motion_level = MotionLevel::Full;
        let area = Rect::new(0, 0, width, height);
        let foreground_template = foreground_fixture(area, &foreground);
        app.layout.screen_area = area;
        app.layout.pane_area = area;
        // This component benchmark has its own cache; it does not expose or
        // mutate the application's worker-owned scene state.
        let mut cache = AnimationLoopCache::new(resources.resources.clone());
        let build = Instant::now();
        while !cache.step(&settings, width, height, 8) {
            if build.elapsed() > Duration::from_secs(120) {
                return Err("cache build exceeded120seconds".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        println!(
            "{}",
            serde_json::json!({"type":"result","stage":"cache_build","scene":kind,"width":width,"height":height,"density_percent":density,"foreground":foreground,"shoreline_style":shoreline_style,"loop_seconds":1,"us":build.elapsed().as_secs_f64()*1e6,"resident_bytes":cache.status().resident_bytes,"scope":"isolated cache component"})
        );
        let mut samples = Vec::new();
        for n in 0..frames {
            let start = Instant::now();
            std::hint::black_box(cache.copy_frame_into(
                Duration::from_nanos(u64::from(n) * 1_000_000_000 / 30),
                &mut field,
            ));
            samples.push(start.elapsed().as_secs_f64() * 1e6);
            std::hint::black_box(field.glyph(0, 0));
        }
        summarize(
            "ready_ram_copy",
            kind,
            width,
            height,
            density,
            samples,
            serde_json::json!({"scope":"isolated cache component"}),
        );
        drop(cache);
        let counter = OutputCount::default();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(counter.clone()),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;
        let (mut compose, mut display, mut buffer_setup, mut pipeline) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut worker_completion = Vec::with_capacity(frames as usize - 1);
        for n in 0..frames {
            let elapsed = Duration::from_nanos(u64::from(n) * 1_000_000_000 / 30);
            let preparation = Instant::now();
            app.animation_frame
                .request(&settings, width, height, elapsed, None)
                .map_err(|error| format!("animation worker rejected request: {error:?}"))?;
            // Only this synthetic harness waits. Production composition must
            // remain nonblocking and may use the latest completed frame.
            while !app.animation_frame.collect() {
                if preparation.elapsed() > Duration::from_secs(120) {
                    return Err("animation worker completion exceeded120seconds".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let completion_us = preparation.elapsed().as_secs_f64() * 1e6;
            println!(
                "{}",
                serde_json::json!({"type":"result","stage":"worker_completion","scene":kind,"frame":n,"us":completion_us,"scope":"request to collected worker result; 1ms harness polling"})
            );
            if n > 0 {
                worker_completion.push(completion_us);
            }
            let pipeline_start = Instant::now();
            let mut buffer = if foreground == "empty" {
                Buffer::empty(area)
            } else {
                foreground_template.clone()
            };
            let start = Instant::now();
            background_composition::compose(&mut buffer, &mut app, elapsed);
            let presentation = background_composition::capture_final(&buffer, area, &mut app);
            let composition_elapsed = start.elapsed();
            let start = Instant::now();
            let mut setup = Duration::ZERO;
            terminal.draw(|frame| {
                let t = Instant::now();
                frame.buffer_mut().clone_from(&buffer);
                setup = t.elapsed();
            })?;
            if let Some(presentation) = presentation {
                // This synthetic native-scene benchmark does not issue replay flush proofs.
                app.animation_frame.acknowledge(presentation, None);
            }
            let total = start.elapsed();
            let pipeline_elapsed = pipeline_start.elapsed();
            if n == 0 {
                println!(
                    "{}",
                    serde_json::json!({"type":"result","stage":"cold_display_pipeline","scene":kind,"width":width,"height":height,"density_percent":density,"foreground":foreground,"shoreline_style":shoreline_style,"us":pipeline_elapsed.as_secs_f64()*1e6,"bytes":counter.bytes.get(),"writes":counter.writes.get()})
                );
                counter.bytes.set(0);
                counter.writes.set(0);
                continue;
            }
            compose.push(composition_elapsed.as_secs_f64() * 1e6);
            pipeline.push(pipeline_elapsed.as_secs_f64() * 1e6);
            display.push(total.saturating_sub(setup).as_secs_f64() * 1e6);
            buffer_setup.push(setup.as_secs_f64() * 1e6);
            std::hint::black_box(terminal.backend());
        }
        summarize(
            "worker_completion",
            kind,
            width,
            height,
            density,
            worker_completion,
            serde_json::json!({"scope":"request to collected worker result; 1ms harness polling; excludes PTY, terminal emulator, CPU/RSS"}),
        );
        summarize(
            "ready_pipeline_total",
            kind,
            width,
            height,
            density,
            pipeline,
            serde_json::json!({"fixture":foreground,"sink":"counting memory writer; excludesPTY/terminal emulator","scope":"direct timer around allocation,composition,buffer copy,diff,ANSI"}),
        );
        summarize(
            "ready_composition",
            kind,
            width,
            height,
            density,
            compose,
            serde_json::json!({"fixture":foreground}),
        );
        summarize(
            "diff_and_ansi",
            kind,
            width,
            height,
            density,
            display,
            serde_json::json!({"sink":"counting memory writer; excludesPTY/terminal emulator","bytes":counter.bytes.get(),"writes":counter.writes.get()}),
        );
        summarize(
            "display_buffer_copy",
            kind,
            width,
            height,
            density,
            buffer_setup,
            serde_json::json!({"fixture":"precomputed buffer clone"}),
        );
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        println!(
            "{}",
            serde_json::json!({"type":"error","message":error.to_string()})
        );
        std::process::exit(1);
    }
}
