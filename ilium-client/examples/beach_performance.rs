//! Real client Beach renderer versus embedded V8; JSONL stdout.
//! Elapsed wall time, not CPU time. Excludes helper IPC and terminal composition.
use ilium_animation_js::{
    engine::{
        initialize_engine, ArraySpec, CreateState, Engine, EngineLimits, RenderOutput,
        TypedArrayKind,
    },
    manifest::AnimationMode,
    package::{Package, PackageLimits},
    settings::validate_settings,
    surface::{
        ColourSpace, Data, Format, FrameMeta, Mode, NoNativeRenderer, Planes, Shape, Surface,
        Update,
    },
    TRUSTED_BOOTSTRAP,
};
use ilium_client::background_animation::{
    AnimationFrame, AnimationKind, AnimationSettings, DitherMode, ShorelineStyle,
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    io::Read,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MIB: usize = 1024 * 1024;
fn fail(message: &str) -> Box<dyn Error> {
    std::io::Error::other(message.to_owned()).into()
}
fn emit(value: Value) {
    println!("{value}");
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct Options {
    package: PathBuf,
    digest: String,
    width: u16,
    height: u16,
    fps: u32,
    warmup: usize,
    frames: usize,
}
fn options() -> Result<Option<Options>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        emit(
            json!({"type":"result","usage":"beach_performance --package ABS --sha256 HEX --width CELLS --height CELLS --fps INTEGER --warmup COUNT --frames COUNT","cases":["classic","rich"],"bounds":{"width":[1,320],"height":[1,120],"fps":[1,30],"warmup":[0,1000],"frames":[1,10000]}}),
        );
        return Ok(None);
    }
    let names = [
        "--package",
        "--sha256",
        "--width",
        "--height",
        "--fps",
        "--warmup",
        "--frames",
    ];
    if args.len() != names.len() * 2 {
        return Err(fail("all documented flags are required exactly once"));
    }
    let mut flags = BTreeMap::new();
    for pair in args.chunks_exact(2) {
        if !names.contains(&pair[0].as_str())
            || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(fail("unknown or duplicate flag"));
        }
    }
    let get = |name| flags.get(name).copied().ok_or_else(|| fail("missing flag"));
    let number = |name, low, high| -> Result<u64> {
        let value = get(name)?.parse::<u64>()?;
        if !(low..=high).contains(&value) {
            return Err(fail("flag outside documented bounds"));
        }
        Ok(value)
    };
    let package = PathBuf::from(get("--package")?);
    if !package.is_absolute() {
        return Err(fail("package path must be absolute"));
    }
    let digest = get("--sha256")?.to_ascii_lowercase();
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail("invalid SHA256"));
    }
    Ok(Some(Options {
        package,
        digest,
        width: number("--width", 1, 320)? as u16,
        height: number("--height", 1, 120)? as u16,
        fps: number("--fps", 1, 30)? as u32,
        warmup: number("--warmup", 0, 1000)? as usize,
        frames: number("--frames", 1, 10000)? as usize,
    }))
}
fn spec(name: &str, kind: TypedArrayKind, elements: usize) -> ArraySpec {
    ArraySpec {
        name: name.into(),
        kind,
        elements,
    }
}
fn plane<'a>(output: &'a RenderOutput, name: &str, length: usize) -> Result<&'a [u8]> {
    let bytes = output
        .planes
        .get(name)
        .ok_or_else(|| fail("missing output plane"))?;
    if bytes.len() != length {
        return Err(fail("output plane byte count mismatch"));
    }
    Ok(bytes)
}
fn planes(output: &RenderOutput, samples: usize) -> Result<Planes> {
    Ok(Planes {
        data: Data::F32(
            plane(output, "data", samples * 4)?
                .chunks_exact(4)
                .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                .collect(),
        ),
        touch: plane(output, "touch", samples)?.to_vec(),
        order: plane(output, "order", samples * 4)?
            .chunks_exact(4)
            .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        cell_rgb: None,
        colour_touch: None,
        colour_order: None,
    })
}
fn stats(values: &mut [f64]) -> Value {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n.is_multiple_of(2) {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    } else {
        values[n / 2]
    };
    json!({"count":n,"median_ns":median,"p95_ns":values[(n*95).div_ceil(100)-1],"minimum_ns":values[0],"maximum_ns":values[n-1]})
}
fn masks(frame: &AnimationFrame, options: &Options) -> Result<Vec<u8>> {
    (0..options.height)
        .flat_map(|y| (0..options.width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let glyph = frame.glyph(x, y);
            if glyph == ' ' {
                return Ok(0);
            }
            let value = glyph as u32;
            if !(0x2800..=0x28ff).contains(&value) {
                return Err(fail("Beach emitted a non-Braille glyph"));
            }
            Ok((value - 0x2800) as u8)
        })
        .collect()
}
fn measure(
    options: &Options,
    package: &Arc<Package>,
    quota: &QuotaGroup,
    style: ShorelineStyle,
) -> Result<()> {
    let preparation = Instant::now();
    let mut settings = AnimationSettings {
        enabled: true,
        kind: AnimationKind::Shoreline,
        speed_percent: 100,
        density_percent: 60,
        dither: DitherMode::Ordered,
        ..AnimationSettings::default()
    };
    settings.shoreline.style = style;
    let settings = settings.normalized();
    if !settings.appearance.pattern_is_neutral() {
        return Err(fail("native pattern tone must be neutral"));
    }
    let raw = serde_json::to_value(settings.shoreline)?;
    let normalized = validate_settings(&package.manifest().settings, &raw)?;
    if normalized != raw {
        return Err(fail("native and package normalized settings differ"));
    }
    let mut native = AnimationFrame::default();
    let native_preparation_ns = preparation.elapsed().as_nanos();
    let shape = Shape {
        cell_width: u32::from(options.width),
        cell_height: u32::from(options.height),
        mode: Mode::Pixels,
        format: Format::Gray32,
        update: Update::Replace,
        cell_rgb: false,
        colour_space: ColourSpace::Srgb,
    };
    let layout = shape.layout()?;
    let _storage = quota
        .reserve_external_storage(
            layout.canonical_bytes * 8
                + layout.handoff_bytes * 8
                + layout.dots * 96
                + 32 * MIB
                + options.frames * 32,
        )
        .map_err(|_| fail("case storage refused"))?;
    let preparation = Instant::now();
    let mut limits = EngineLimits::default();
    limits.render_ms = limits.render_ms.min(package.manifest().limits.render_ms);
    limits.preparation_ms = limits
        .preparation_ms
        .min(package.manifest().limits.preparation_ms);
    let mut engine = Engine::new(Arc::clone(package), limits, quota.clone())?;
    engine.install_bootstrap(TRUSTED_BOOTSTRAP)?;
    engine.load()?;
    let viewport = json!({"cell_width":options.width,"cell_height":options.height,"dot_width":u32::from(options.width)*2,"dot_height":u32::from(options.height)*4,"revision":1});
    let plan = engine.plan(
        &normalized,
        AnimationMode::Live,
        &json!({"viewport":viewport,"available":{}}),
    )?;
    if plan["output"] != json!({"mode":"pixels","format":"gray32","update":"replace"}) {
        return Err(fail("unexpected Beach output plan"));
    }
    if engine.start_create(&normalized, &plan)? != CreateState::Ready
        || !engine.take_requests()?.is_empty()
    {
        return Err(fail("Beach requested asynchronous or external preparation"));
    }
    let mut surface = Surface::new(1, 1, shape)?;
    let v8_preparation_ns = preparation.elapsed().as_nanos();
    let returned = [
        spec("work_data", TypedArrayKind::F32, layout.elements),
        spec("data", TypedArrayKind::F32, layout.elements),
        spec("work_touch", TypedArrayKind::U8, layout.samples),
        spec("touch", TypedArrayKind::U8, layout.samples),
        spec("work_order", TypedArrayKind::U32, layout.samples),
        spec("order", TypedArrayKind::U32, layout.samples),
    ];
    let seed_specs = [spec("work_data", TypedArrayKind::F32, layout.elements)];
    let mut native_times = Vec::with_capacity(options.frames);
    let mut js_times = Vec::with_capacity(options.frames);
    let mut different_cells = 0_u64;
    let mut different_dots = 0_u64;
    let mut native_hash = Sha256::new();
    let mut js_hash = Sha256::new();
    let mut input_hash = Sha256::new();
    for index in 0..options.warmup + options.frames {
        let time = Duration::from_secs_f64(index as f64 / f64::from(options.fps));
        let previous = if index == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((index - 1) as f64 / f64::from(options.fps))
        };
        let mut context = json!({"viewport":viewport,"time":time.as_secs_f64(),"wall":time.as_secs_f64(),"delta":(time-previous).as_secs_f64(),"wall_delta":(time-previous).as_secs_f64(),"settings":normalized,"visible":true,"inputs":{},"render_policy":{"can_skip_occluded":false}});
        input_hash.update(serde_json::to_vec(&context)?);
        let native_step = |native: &mut AnimationFrame| {
            let start = Instant::now();
            native.render(&settings, options.width, options.height, time);
            std::hint::black_box(&*native);
            start.elapsed().as_nanos() as f64
        };
        let js_step = |engine: &mut Engine,
                       surface: &mut Surface,
                       context: &mut Value|
         -> Result<(f64, Vec<u8>)> {
            let start = Instant::now();
            let seed = surface.begin(index as u64 + 1)?;
            let Data::F32(data) = seed.data else {
                return Err(fail("unexpected seed format"));
            };
            let metadata = json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]}});
            let binary = BTreeMap::from([(
                "work_data".into(),
                data.iter().flat_map(|v| v.to_ne_bytes()).collect(),
            )]);
            engine.seed_frame(&metadata, &seed_specs, &binary)?;
            context["_ilium_frame"] = json!({"key":seed.key,"shape":seed.shape});
            let output = engine.render(context, &returned)?;
            let meta = FrameMeta::parse(&serde_json::to_vec(&output.metadata)?)?;
            let mut packed = None;
            let result = surface.finish_with(
                meta,
                planes(&output, layout.samples)?,
                &mut NoNativeRenderer,
                |candidate, _| {
                    // Identical production ordered dither, strict comparison and density.
                    packed = Some(
                        candidate
                            .pack(
                                |v, _, _| v,
                                |v, x, y| {
                                    v * 0.6
                                        > ilium_ambient::raster::threshold(
                                            x,
                                            y,
                                            DitherMode::Ordered,
                                        )
                                },
                            )?
                            .masks,
                    );
                    Ok(())
                },
            );
            let accepted = result.as_ref().is_ok_and(|r| r.accepted);
            engine.accept_frame(accepted)?;
            result?;
            if !accepted {
                return Err(fail("V8 frame transaction refused"));
            }
            Ok((
                start.elapsed().as_nanos() as f64,
                packed.ok_or_else(|| fail("accepted output was not packed"))?,
            ))
        };
        let (native_ns, (js_ns, js_masks)) = if index.is_multiple_of(2) {
            let n = native_step(&mut native);
            let j = js_step(&mut engine, &mut surface, &mut context)?;
            (n, j)
        } else {
            let j = js_step(&mut engine, &mut surface, &mut context)?;
            let n = native_step(&mut native);
            (n, j)
        };
        if index == 0 {
            emit(
                json!({"type":"result","stage":"first_frame","style":style,"native_render_and_pack_ns":native_ns,"v8_seed_render_handoff_accept_pack_ns":js_ns,"included_in_distribution":options.warmup==0,"cold_state":"not guaranteed"}),
            );
        }
        if index < options.warmup {
            continue;
        }
        native_times.push(native_ns);
        js_times.push(js_ns);
        let native_masks = masks(&native, options)?;
        if native_masks.len() != js_masks.len() {
            return Err(fail("packed shape mismatch"));
        }
        native_hash.update(&native_masks);
        js_hash.update(&js_masks);
        for (n, j) in native_masks.iter().zip(&js_masks) {
            different_cells += u64::from(n != j);
            different_dots += u64::from((n ^ j).count_ones());
        }
    }
    engine.dispose()?;
    let native_stats = stats(&mut native_times);
    let js_stats = stats(&mut js_times);
    emit(
        json!({"type":"result","stage":"warm_frames","style":style,"settings":normalized,"native_construction_ns":native_preparation_ns,"v8_construction_ns":v8_preparation_ns,"native_render_and_pack_elapsed":native_stats,"v8_seed_render_handoff_accept_pack_elapsed":js_stats,"clock":"monotonic elapsed wall time","cpu_time_ns":null,"comparison_scope":"native public packed-frame pipeline versus V8 seed/render/binary/accepted Surface pack; no UI/helper/terminal","pack_policy":{"dither":"ordered","density_percent":60,"neutral_pattern_tone":true,"comparison":"strict greater than shared native threshold"},"packed_differential":{"compared_cells":options.frames*layout.cells,"different_cells":different_cells,"different_dots":different_dots,"native_sha256":format!("{:x}",native_hash.finalize()),"v8_sha256":format!("{:x}",js_hash.finalize()),"status":if different_cells==0{"exact_masks"}else{"requires_fidelity_review"}},"v8_to_native_median_ratio":if different_cells==0 {Some(js_stats["median_ns"].as_f64().ok_or_else(||fail("missing median"))?/native_stats["median_ns"].as_f64().ok_or_else(||fail("missing median"))?)} else {None},"input_sha256":format!("{:x}",input_hash.finalize()),"first_frame_in_distribution":options.warmup==0}),
    );
    Ok(())
}
fn run() -> Result<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 16,
        jobs: 16,
        service_jobs: 0,
        input_bytes: 64 * MIB,
        result_bytes: 64 * MIB,
        worker_threads: 32,
        worker_bytes: 1536 * MIB,
    });
    let limits = PackageLimits::default();
    let _package_storage = quota
        .reserve_external_storage((limits.archive_bytes + limits.expanded_bytes) as usize + 8 * MIB)
        .map_err(|_| fail("package storage refused"))?;
    let started = Instant::now();
    let file = std::fs::File::open(&options.package)?;
    let size = file.metadata()?.len();
    if size > limits.archive_bytes {
        return Err(fail("archive exceeds loader limit"));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(limits.archive_bytes + 1)
        .read_to_end(&mut bytes)?;
    if sha(&bytes) != options.digest {
        return Err(fail("archive differs from pinned SHA256"));
    }
    let package = Arc::new(Package::from_bytes(&bytes, limits)?);
    if package.manifest().id != "beach" {
        return Err(fail("selected package is not Beach"));
    }
    emit(
        json!({"type":"result","stage":"archive_loading","path":options.package,"archive_sha256":options.digest,"package_digest":package.digest(),"elapsed_ns":started.elapsed().as_nanos(),"filesystem_cache":"uncontrolled","includes":["open/read","SHA256","RAM expansion and validation"],"excludes":["V8 initialization","module evaluation","scene creation"]}),
    );
    let started = Instant::now();
    initialize_engine(quota.clone(), 1)?;
    emit(
        json!({"type":"result","stage":"process_global_v8_initialization","elapsed_ns":started.elapsed().as_nanos()}),
    );
    let _owner = quota
        .reserve_external_worker(1, 4 * MIB)
        .map_err(|_| fail("owner admission refused"))?;
    emit(
        json!({"type":"manifest","benchmark":"beach_performance","source_sha256":sha(include_bytes!("beach_performance.rs")),"bootstrap_sha256":sha(TRUSTED_BOOTSTRAP.as_bytes()),"native_shoreline_sha256":sha(include_bytes!("../src/background_animation/shoreline.rs")),"native_scenes_sha256":sha(include_bytes!("../src/background_animation/scenes.rs")),"native_frame_sha256":sha(include_bytes!("../src/background_animation/mod.rs")),"native_threshold_sha256":sha(include_bytes!("../../ilium-ambient/src/dither.rs")),"width_cells":options.width,"height_cells":options.height,"fps":options.fps,"warmup":options.warmup,"measured_frames":options.frames,"execution_order":"alternating native/V8 first","excludes":["protected helper IPC","permission broker","UI composition","terminal emission"],"no_terminal_credit":true}),
    );
    for style in [ShorelineStyle::Classic, ShorelineStyle::Rich] {
        measure(&options, &package, &quota, style)?;
    }
    emit(
        json!({"type":"summary","measured_cases":2,"performance_qualification":"inspect packed differential before interpreting ratios; wall-time measurements only"}),
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        emit(json!({"type":"error","message":error.to_string()}));
        std::process::exit(1);
    }
}
