//! Actual native client Beach consumer versus embedded V8 and Surface packing.
//! This qualification harness creates no execution bank and acquires no inputs.
//! It excludes helper isolation/IPC, UI composition and terminal emission.
use ilium_animation_js::{
    engine::{
        initialize_engine, ArraySpec, CreateState, Engine, EngineLimits, RenderOutput,
        ServiceAuthority, TypedArrayKind,
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
    AnimationFrame, AnimationKind, AnimationSettings, ShorelineSettings, ShorelineStyle,
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
// These bytes are embedded at compilation, not read from a potentially newer
// worktree when the benchmark runs.
const BASELINES: [(&str, &[u8]); 6] = [
    (
        "ilium-client/src/background_animation/mod.rs",
        include_bytes!("../src/background_animation/mod.rs"),
    ),
    (
        "ilium-client/src/background_animation/scenes.rs",
        include_bytes!("../src/background_animation/scenes.rs"),
    ),
    (
        "ilium-client/src/background_animation/shoreline.rs",
        include_bytes!("../src/background_animation/shoreline.rs"),
    ),
    (
        "ilium-ambient/src/raster.rs",
        include_bytes!("../../ilium-ambient/src/raster.rs"),
    ),
    (
        "ilium-ambient/src/dither.rs",
        include_bytes!("../../ilium-ambient/src/dither.rs"),
    ),
    (
        "ilium-ambient/src/style.rs",
        include_bytes!("../../ilium-ambient/src/style.rs"),
    ),
];
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn invalid(message: &str) -> Box<dyn Error> {
    std::io::Error::other(message.to_owned()).into()
}
fn emit(value: Value) {
    println!("{value}");
}
struct Options {
    package: PathBuf,
    digest: String,
    width: u16,
    height: u16,
    fps: u32,
    warmup: usize,
    frames: usize,
    diagnostic_render_ms: Option<u64>,
}
fn options() -> Result<Option<Options>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments == ["--help"] {
        emit(
            json!({"type":"result","usage":"v8_beach_performance --package ABS --sha256 HEX --width CELLS --height CELLS --fps INTEGER --warmup COUNT --frames COUNT [--diagnostic-render-ms INTEGER]","cases":["beach-classic","beach-rich"],"bounds":{"width":[1,320],"height":[1,120],"fps":[1,30],"warmup":[0,1000],"frames":[1,10000],"diagnostic_render_ms":[1,10000]},"diagnostic_warning":"An explicit diagnostic budget does not qualify production deadlines"}),
        );
        return Ok(None);
    }
    let allowed = [
        "--package",
        "--sha256",
        "--width",
        "--height",
        "--fps",
        "--warmup",
        "--frames",
    ];
    if arguments.len() != allowed.len() * 2 && arguments.len() != (allowed.len() + 1) * 2 {
        return Err(invalid("every documented flag is required exactly once"));
    }
    let mut flags = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if (!allowed.contains(&pair[0].as_str()) && pair[0] != "--diagnostic-render-ms")
            || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(invalid("unknown or duplicate flag"));
        }
    }
    let get = |key: &str| {
        flags
            .get(key)
            .copied()
            .ok_or_else(|| invalid("missing flag"))
    };
    let number = |key: &str, minimum: u64, maximum: u64| -> Result<u64> {
        let value = get(key)?.parse::<u64>()?;
        if !(minimum..=maximum).contains(&value) {
            return Err(invalid("numeric flag outside declared bounds"));
        }
        Ok(value)
    };
    let package = PathBuf::from(get("--package")?);
    let digest = get("--sha256")?.to_owned();
    if !package.is_absolute()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
    {
        return Err(invalid(
            "absolute package path and lowercase SHA256 required",
        ));
    }
    Ok(Some(Options {
        package,
        digest,
        width: number("--width", 1, 320)? as u16,
        height: number("--height", 1, 120)? as u16,
        fps: number("--fps", 1, 30)? as u32,
        warmup: number("--warmup", 0, 1000)? as usize,
        frames: number("--frames", 1, 10000)? as usize,
        diagnostic_render_ms: if flags.contains_key("--diagnostic-render-ms") {
            Some(number("--diagnostic-render-ms", 1, 10000)?)
        } else {
            None
        },
    }))
}
fn spec(name: &str, kind: TypedArrayKind, elements: usize) -> ArraySpec {
    ArraySpec {
        name: name.into(),
        kind,
        elements,
    }
}
fn plane<'a>(output: &'a RenderOutput, name: &str, count: usize) -> Result<&'a [u8]> {
    let bytes = output
        .planes
        .get(name)
        .ok_or_else(|| invalid("missing sealed plane"))?;
    if bytes.len() != count {
        return Err(invalid("sealed plane shape mismatch"));
    }
    Ok(bytes)
}
fn planes(output: &RenderOutput, samples: usize) -> Result<Planes> {
    Ok(Planes {
        data: Data::F32(
            plane(output, "data", samples * 4)?
                .chunks_exact(4)
                .map(|v| f32::from_ne_bytes([v[0], v[1], v[2], v[3]]))
                .collect(),
        ),
        touch: plane(output, "touch", samples)?.to_vec(),
        order: plane(output, "order", samples * 4)?
            .chunks_exact(4)
            .map(|v| u32::from_ne_bytes([v[0], v[1], v[2], v[3]]))
            .collect(),
        cell_rgb: None,
        colour_touch: None,
        colour_order: None,
    })
}
fn stats(samples: &mut [f64]) -> Value {
    samples.sort_by(f64::total_cmp);
    let count = samples.len();
    let median = if count.is_multiple_of(2) {
        (samples[count / 2 - 1] + samples[count / 2]) / 2.
    } else {
        samples[count / 2]
    };
    json!({"median_ns":median,"p95_ns":samples[(count*95).div_ceil(100)-1],"minimum_ns":samples[0],"maximum_ns":samples[count-1],"count":count})
}
fn measure(
    options: &Options,
    package: &Arc<Package>,
    quota: &QuotaGroup,
    style: ShorelineStyle,
) -> Result<()> {
    let shoreline = ShorelineSettings {
        style,
        ..ShorelineSettings::default()
    }
    .normalized();
    let native_settings = AnimationSettings {
        kind: AnimationKind::Shoreline,
        shoreline,
        ..AnimationSettings::default()
    }
    .normalized();
    if native_settings.dither.is_error_diffusion() {
        return Err(invalid(
            "default native dither requires a different packing adapter",
        ));
    }
    let settings = validate_settings(
        &package.manifest().settings,
        &serde_json::to_value(shoreline)?,
    )?;
    if settings != serde_json::to_value(shoreline)? {
        return Err(invalid("native/package normalized settings mismatch"));
    }
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
    let mut limits = EngineLimits::default();
    limits.render_ms = limits.render_ms.min(package.manifest().limits.render_ms);
    // Diagnostic only: measure frames that the production deadline refuses.
    // This isolated example does not change the helper or manifest policy.
    if let Some(milliseconds) = options.diagnostic_render_ms {
        limits.render_ms = milliseconds;
    }
    limits.preparation_ms = limits
        .preparation_ms
        .min(package.manifest().limits.preparation_ms);
    if layout.handoff_bytes > limits.frame_bytes {
        return Err(invalid("frame exceeds unchanged engine budget"));
    }
    // Native SceneCache/raster, Surface, binary copies, packing, timing arrays and
    // current/candidate masks remain under this original finite root admission.
    let _storage = quota
        .reserve_external_storage(
            layout.canonical_bytes * 8
                + layout.handoff_bytes * 8
                + layout.dots * 96
                + 32 * MIB
                + options.frames * 64,
        )
        .map_err(|reason| invalid(&format!("case admission: {reason:?}")))?;
    let mut native = AnimationFrame::default();
    let mut surface = Surface::new(1, 1, shape)?;
    let preparation = Instant::now();
    let mut engine = Engine::new(Arc::clone(package), limits, quota.clone())?;
    engine.install_bootstrap(TRUSTED_BOOTSTRAP)?;
    // Match the native fixture authority used by the Carpet comparison. This
    // binds the embedded activation; it grants no external service access.
    engine.bind_service_authority(
        package.digest(),
        ServiceAuthority {
            instance_id: 1,
            plan_generation: 1,
            authorization_epoch: 1,
        },
    )?;
    engine.load()?;
    let environment = json!({"viewport":{"cell_width":options.width,"cell_height":options.height,"dot_width":layout.width,"dot_height":layout.height,"revision":1},"available":{"pointer":false,"audio":false,"gpu":false,"location":false}});
    let plan = engine.plan(&settings, AnimationMode::Live, &environment)?;
    if plan["output"] != json!({"mode":"pixels","format":"gray32","update":"replace"})
        || engine.start_create(&settings, &plan)? != CreateState::Ready
        || !engine.take_requests()?.is_empty()
    {
        return Err(invalid("unexpected package plan or host acquisition"));
    }
    let js_preparation_ns = preparation.elapsed().as_nanos();
    // Include native actual prepared geometry and first packing in cold setup.
    let preparation = Instant::now();
    native.render(
        &native_settings,
        options.width,
        options.height,
        Duration::ZERO,
    );
    let native_preparation_ns = preparation.elapsed().as_nanos();
    let returned = [
        spec("work_data", TypedArrayKind::F32, layout.elements),
        spec("data", TypedArrayKind::F32, layout.elements),
        spec("work_touch", TypedArrayKind::U8, layout.samples),
        spec("touch", TypedArrayKind::U8, layout.samples),
        spec("work_order", TypedArrayKind::U32, layout.samples),
        spec("order", TypedArrayKind::U32, layout.samples),
    ];
    let seeded = [spec("work_data", TypedArrayKind::F32, layout.elements)];
    let mut native_times = Vec::with_capacity(options.frames);
    let mut js_times = Vec::with_capacity(options.frames);
    let mut js_render_times = Vec::with_capacity(options.frames);
    let mut changed_bits = 0_u64;
    let mut compared_bits = 0_u64;
    let mut changed_cells = 0_u64;
    for index in 0..options.warmup + options.frames {
        let sequence = index as u64 + 1;
        let time = Duration::from_secs_f64(sequence as f64 / f64::from(options.fps));
        let seconds = time.as_secs_f64();
        let previous =
            Duration::from_secs_f64((sequence - 1) as f64 / f64::from(options.fps)).as_secs_f64();
        let native_frame = |native: &mut AnimationFrame| -> Result<(f64, Vec<u8>)> {
            let begin = Instant::now();
            native.render(&native_settings, options.width, options.height, time);
            let mut masks = Vec::with_capacity(layout.cells);
            for y in 0..options.height {
                for x in 0..options.width {
                    let glyph = native.glyph(x, y);
                    let mask = if glyph == ' ' {
                        0
                    } else {
                        u8::try_from(
                            u32::from(glyph)
                                .checked_sub(0x2800)
                                .ok_or_else(|| invalid("non-Braille native glyph"))?,
                        )?
                    };
                    masks.push(mask);
                }
            }
            Ok((begin.elapsed().as_nanos() as f64, masks))
        };
        let js_frame = |engine: &mut Engine,
                        surface: &mut Surface|
         -> Result<(f64, f64, Vec<u8>)> {
            let begin = Instant::now();
            let seed = surface.begin(sequence)?;
            let Data::F32(data) = seed.data else {
                return Err(invalid("unexpected seed format"));
            };
            let binary = BTreeMap::from([(
                "work_data".into(),
                data.iter().flat_map(|v| v.to_ne_bytes()).collect(),
            )]);
            engine.seed_frame(&json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]}}),&seeded,&binary)?;
            let context = json!({"viewport":environment["viewport"],"time":seconds,"wall":seconds,"delta":seconds-previous,"wall_delta":seconds-previous,
                "settings":settings,"visible":true,"inputs":{},"render_policy":{"can_skip_occluded":false},"_ilium_frame":{"key":seed.key,"shape":seed.shape}});
            let render = Instant::now();
            let output = engine.render(&context, &returned)?;
            let render_ns = render.elapsed().as_nanos() as f64;
            let metadata = FrameMeta::parse(&serde_json::to_vec(&output.metadata)?)?;
            let sealed = planes(&output, layout.samples)?;
            let mut packed = None;
            let accepted =
                surface.finish_with(metadata, sealed, &mut NoNativeRenderer, |snapshot, _| {
                    packed = Some(
                        snapshot
                            .pack(
                                |v, _, _| native_settings.appearance.shape_dot(v),
                                |v, x, y| {
                                    v * (f32::from(native_settings.density_percent) / 100.)
                                        > ilium_ambient::raster::threshold(
                                            x,
                                            y,
                                            native_settings.dither,
                                        )
                                },
                            )?
                            .masks,
                    );
                    Ok(())
                });
            let success = accepted.as_ref().is_ok_and(|outcome| outcome.accepted);
            engine.accept_frame(success)?;
            accepted?;
            if !success {
                return Err(invalid("Surface rejected frame"));
            }
            Ok((
                begin.elapsed().as_nanos() as f64,
                render_ns,
                packed.ok_or_else(|| invalid("accepted frame without native pack"))?,
            ))
        };
        // Alternate order rather than always warming the native side first.
        let (n, j) = if index.is_multiple_of(2) {
            let n = native_frame(&mut native)?;
            let j = js_frame(&mut engine, &mut surface)?;
            (n, j)
        } else {
            let j = js_frame(&mut engine, &mut surface)?;
            let n = native_frame(&mut native)?;
            (n, j)
        };
        if n.1.len() != j.2.len() {
            return Err(invalid("packed native/V8 dimensions mismatch"));
        }
        if index == 0 {
            emit(
                json!({"type":"progress","case":if style==ShorelineStyle::Classic{"beach-classic"}else{"beach-rich"},"first_v8_render_ns":j.1,"first_v8_pipeline_ns":j.0,"diagnostic_render_ms":options.diagnostic_render_ms,"production_deadline_qualified":options.diagnostic_render_ms.is_none()}),
            );
        }
        if index < options.warmup {
            continue;
        }
        native_times.push(n.0);
        js_times.push(j.0);
        js_render_times.push(j.1);
        for (&native_mask, &js_mask) in n.1.iter().zip(&j.2) {
            changed_bits += u64::from((native_mask ^ js_mask).count_ones());
            compared_bits += 8;
            changed_cells += u64::from(native_mask != js_mask);
        }
    }
    let native_stats = stats(&mut native_times);
    let js_stats = stats(&mut js_times);
    emit(
        json!({"type":"result","case":if style==ShorelineStyle::Classic{"beach-classic"}else{"beach-rich"},"status":"measured_pending_fidelity_review",
        "package_digest":package.digest(),"settings":settings,"native_client_render_pack_collect":native_stats,"v8_seed_render_accept_pack_collect":js_stats,
        "v8_render_detach_only":stats(&mut js_render_times),"raw_median_pipeline_ratio":js_stats["median_ns"].as_f64().zip(native_stats["median_ns"].as_f64()).map(|(j,n)|j/n),
        "native_cold_first_frame_ns":native_preparation_ns as u64,"v8_load_create_ns":js_preparation_ns as u64,"cold_setup_boundaries_are_different":true,
        "changed_braille_bits":changed_bits,"compared_braille_bits":compared_bits,"changed_cells":changed_cells,"fidelity_bit_difference_fraction":changed_bits as f64/compared_bits as f64,
        "pixel_fidelity":"covered separately by native source golden fields; this actual-consumer harness compares packed Braille",
        "cpu_time_ns":null,"performance_acceptance":"requires review of fidelity differences and separate protected-helper/UI measurements"}),
    );
    engine.dispose()?;
    native.release_hosts();
    Ok(())
}
fn run() -> Result<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 16,
        worker_bytes: 2304 * MIB,
    });
    // Production V8 platform must precede other workers, even in a benchmark.
    initialize_engine(quota.clone(), 1)?;
    let _package_storage = quota
        .reserve_external_storage(160 * MIB)
        .map_err(|reason| invalid(&format!("package admission: {reason:?}")))?;
    let mut file = std::fs::File::open(&options.package)?;
    let limits = PackageLimits::default();
    let size = file.metadata()?.len();
    if size > limits.archive_bytes {
        return Err(invalid("package archive exceeds bound"));
    }
    let mut archive = Vec::with_capacity(size as usize + 1);
    file.by_ref().take(size + 1).read_to_end(&mut archive)?;
    if archive.len() as u64 != size {
        return Err(invalid("archive changed size during read"));
    }
    let package = Arc::new(Package::from_bytes(&archive, limits)?);
    if package.digest() != options.digest || package.manifest().id != "beach" {
        return Err(invalid(
            "archive identity differs from explicit pinned Beach package",
        ));
    }
    let baselines: BTreeMap<_, _> = BASELINES
        .iter()
        .map(|(path, bytes)| (*path, digest(bytes)))
        .collect();
    emit(
        json!({"type":"manifest","benchmark":"v8_beach_performance","benchmark_source_sha256":digest(include_bytes!("v8_beach_performance.rs")),"bootstrap_sha256":digest(TRUSTED_BOOTSTRAP.as_bytes()),"native_baselines":baselines,"package":options.package,"package_sha256":package.digest(),"width_cells":options.width,"height_cells":options.height,
        "fps":options.fps,"warmup":options.warmup,"frames":options.frames,"diagnostic_render_ms":options.diagnostic_render_ms,"production_deadline_qualified":options.diagnostic_render_ms.is_none(),"clock":"host monotonic chosen fixed simulation timestamps",
        "native_boundary":"actual public AnimationFrame::render with original SceneCache and native pack, then public glyph collection",
        "v8_boundary":"trusted bootstrap, seeded binary views, render/detach, Surface acceptance and exact native default appearance/threshold pack",
        "excludes":["protected helper isolation/IPC","permission acquisition","UI composition","terminal emission"],"root":"isolated finite qualification root16 workers/2304MiB; not a client coexistence claim","cpu_time_ns":null}),
    );
    measure(&options, &package, &quota, ShorelineStyle::Classic)?;
    measure(&options, &package, &quota, ShorelineStyle::Rich)?;
    emit(
        json!({"type":"summary","measured_cases":2,"status":"measured_pending_fidelity_review","root_retained_worker_threads":quota.snapshot().worker_threads,
        "retention":"process-global V8 platform remains initialized until process exit"}),
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        emit(json!({"type":"error","message":error.to_string()}));
        std::process::exit(1);
    }
}
