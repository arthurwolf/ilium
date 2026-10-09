//! Compare the native Carpet scene with its packaged V8 implementation.
//! Measures render/pack wall time and Braille differences for every mode whose
//! inputs can be held locally. Lichess TV is reported as unmeasured because the
//! native and guest implementations need an authenticated live game source.
use ilium_ambient::CarpetSettings;
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
    AnimationFrame, AnimationKind, AnimationSettings, DitherMode,
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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MIB: usize = 1024 * 1024;
const MEASURED_MODES: [i32; 8] = [0, 1, 2, 3, 5, 6, 7, 8];
// Bind each timing record to the complete native Carpet/Braille path, not
// only this harness. Embed bytes so later source edits cannot rewrite results.
const NATIVE_BASELINES: [(&str, &[u8]); 20] = [
    (
        "ilium-client/src/background_animation/mod.rs",
        include_bytes!("../src/background_animation/mod.rs"),
    ),
    (
        "ilium-client/src/background_animation/scenes.rs",
        include_bytes!("../src/background_animation/scenes.rs"),
    ),
    (
        "ilium-client/src/background_animation/raster.rs",
        include_bytes!("../src/background_animation/raster.rs"),
    ),
    (
        "ilium-client/src/background_animation/host.rs",
        include_bytes!("../src/background_animation/host.rs"),
    ),
    (
        "ilium-ambient/src/registry.rs",
        include_bytes!("../../ilium-ambient/src/registry.rs"),
    ),
    (
        "ilium-ambient/src/scene.rs",
        include_bytes!("../../ilium-ambient/src/scene.rs"),
    ),
    (
        "ilium-ambient/src/resources.rs",
        include_bytes!("../../ilium-ambient/src/resources.rs"),
    ),
    (
        "ilium-ambient/src/lib.rs",
        include_bytes!("../../ilium-ambient/src/lib.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/mod.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/mod.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/chess.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/chess.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/chess_engine.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/chess_engine.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/envelope_tree.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/envelope_tree.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/model.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/model.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/render.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/render.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/settings.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/settings.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/simulations.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/simulations.rs"),
    ),
    (
        "ilium-ambient/src/scenes/carpet/snake_planner.rs",
        include_bytes!("../../ilium-ambient/src/scenes/carpet/snake_planner.rs"),
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
const V8_RUNTIME_SOURCES: [(&str, &[u8]); 5] = [
    (
        "ilium-animation-js/src/engine.rs",
        include_bytes!("../../ilium-animation-js/src/engine.rs"),
    ),
    (
        "ilium-animation-js/src/manifest.rs",
        include_bytes!("../../ilium-animation-js/src/manifest.rs"),
    ),
    (
        "ilium-animation-js/src/package.rs",
        include_bytes!("../../ilium-animation-js/src/package.rs"),
    ),
    (
        "ilium-animation-js/src/settings.rs",
        include_bytes!("../../ilium-animation-js/src/settings.rs"),
    ),
    (
        "ilium-animation-js/src/surface.rs",
        include_bytes!("../../ilium-animation-js/src/surface.rs"),
    ),
];

fn invalid(message: &str) -> Box<dyn Error> {
    std::io::Error::other(message.to_owned()).into()
}
fn emit(value: Value) {
    println!("{value}");
}
fn resolve_package_path(value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}
fn digest(bytes: &[u8]) -> String {
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
            json!({"type":"result","usage":"v8_carpet_performance --package PATH --sha256 HEX --width CELLS --height CELLS --fps INTEGER --warmup COUNT --frames COUNT","measured_modes":MEASURED_MODES,"unmeasured_modes":{"4":"Lichess TV requires an authenticated native game-source snapshot"},"bounds":{"width":[1,320],"height":[1,120],"fps":[1,30],"warmup":[0,1000],"frames":[1,10000]}}),
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
        return Err(invalid("all documented flags are required exactly once"));
    }
    let mut flags = BTreeMap::new();
    for pair in args.chunks_exact(2) {
        if !names.contains(&pair[0].as_str())
            || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(invalid("unknown or duplicate flag"));
        }
    }
    let get = |name| {
        flags
            .get(name)
            .copied()
            .ok_or_else(|| invalid("missing flag"))
    };
    let number = |name, minimum, maximum| -> Result<u64> {
        let value = get(name)?.parse::<u64>()?;
        if !(minimum..=maximum).contains(&value) {
            return Err(invalid("flag outside documented bounds"));
        }
        Ok(value)
    };
    let package = resolve_package_path(get("--package")?);
    let digest = get("--sha256")?.to_owned();
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
    {
        return Err(invalid("lowercase SHA256 required"));
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
fn plane<'a>(output: &'a RenderOutput, name: &str, bytes: usize) -> Result<&'a [u8]> {
    let value = output
        .planes
        .get(name)
        .ok_or_else(|| invalid("missing output plane"))?;
    if value.len() != bytes {
        return Err(invalid("output plane byte count mismatch"));
    }
    Ok(value)
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
fn stats(values: &mut [f64]) -> Value {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n.is_multiple_of(2) {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    } else {
        values[n / 2]
    };
    json!({"count":n,"median_ns":median,"p95_ns":values[(n * 95).div_ceil(100) - 1],"minimum_ns":values[0],"maximum_ns":values[n - 1]})
}
fn glyph_masks(frame: &AnimationFrame, width: u16, height: u16) -> Result<Vec<u8>> {
    let mut masks = Vec::with_capacity(usize::from(width) * usize::from(height));
    for y in 0..height {
        for x in 0..width {
            let glyph = frame.glyph(x, y);
            if glyph == ' ' {
                masks.push(0);
                continue;
            }
            let value = u32::from(glyph);
            if !(0x2800..=0x28ff).contains(&value) {
                return Err(invalid("Carpet emitted a non-Braille glyph"));
            }
            masks.push((value - 0x2800) as u8);
        }
    }
    Ok(masks)
}
fn measure(options: &Options, package: &Arc<Package>, quota: &QuotaGroup, mode: i32) -> Result<()> {
    let carpet = CarpetSettings {
        mode,
        ..CarpetSettings::default()
    };
    let settings = validate_settings(
        &package.manifest().settings,
        &serde_json::to_value(&carpet)?,
    )?;
    if settings != serde_json::to_value(&carpet)? {
        return Err(invalid("native and package Carpet settings differ"));
    }
    let native_settings = AnimationSettings {
        enabled: true,
        kind: AnimationKind::Carpet,
        density_percent: 60,
        dither: DitherMode::Ordered,
        ambient: ilium_ambient::AmbientSettings {
            carpet,
            ..Default::default()
        },
        ..AnimationSettings::default()
    }
    .normalized();
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
    let mut engine = Engine::new(Arc::clone(package), limits, quota.clone())?;
    engine.install_bootstrap(TRUSTED_BOOTSTRAP)?;
    // Match the offline Beach harness without granting external service rights.
    engine.bind_service_authority(
        package.digest(),
        ServiceAuthority {
            instance_id: 1,
            plan_generation: 1,
            authorization_epoch: 1,
        },
    )?;
    engine.load()?;
    let viewport = json!({"cell_width":options.width,"cell_height":options.height,"dot_width":u32::from(options.width)*2,"dot_height":u32::from(options.height)*4,"revision":1});
    let environment = json!({"viewport":viewport,"available":{"pointer":true,"audio":false,"gpu":false,"location":false}});
    let plan = engine.plan(&settings, AnimationMode::Live, &environment)?;
    if plan["output"] != json!({"mode":"pixels","format":"gray32","update":"replace"}) {
        return Err(invalid("unexpected Carpet output plan"));
    }
    if engine.start_create(&settings, &plan)? != CreateState::Ready
        || !engine.take_requests()?.is_empty()
    {
        return Err(invalid(
            "offline Carpet mode requested an unprovided native service",
        ));
    }
    let mut surface = Surface::new(1, 1, shape)?;
    let mut native = AnimationFrame::default();
    let pointer = (mode == 0).then_some([0.7, 0.4]);
    native.pointer(pointer);
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
        let seconds = sequence as f64 / f64::from(options.fps);
        let previous = (sequence - 1) as f64 / f64::from(options.fps);
        let elapsed = Duration::from_secs_f64(seconds);
        let native_begin = Instant::now();
        native.render(&native_settings, options.width, options.height, elapsed);
        let native_masks = glyph_masks(&native, options.width, options.height)?;
        let native_ns = native_begin.elapsed().as_nanos() as f64;

        let begin = Instant::now();
        let seed = surface.begin(sequence)?;
        let Data::F32(data) = seed.data else {
            return Err(invalid("unexpected Carpet seed format"));
        };
        let binary = BTreeMap::from([(
            "work_data".into(),
            data.iter().flat_map(|v| v.to_ne_bytes()).collect(),
        )]);
        engine.seed_frame(&json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]}}), &seeded, &binary)?;
        let pointer_input = if mode == 0 {
            json!({"revision":1,"available":true,"x":0.7,"y":0.4,"inside":true,"captured_at_ms":0})
        } else {
            Value::Null
        };
        let clock_input = if matches!(mode, 7 | 8) {
            json!({"revision":1,"available":true,"epoch_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64,"timezone":"UTC"})
        } else {
            Value::Null
        };
        let mut inputs = serde_json::Map::new();
        if !pointer_input.is_null() {
            inputs.insert("pointer".into(), pointer_input);
        }
        if !clock_input.is_null() {
            inputs.insert("clock".into(), clock_input);
        }
        let context = json!({"viewport":viewport,"time":seconds,"wall":seconds,"delta":seconds-previous,"wall_delta":seconds-previous,
            "settings":settings,"visible":true,"inputs":inputs,"render_policy":{"can_skip_occluded":false},
            "_ilium_frame":{"key":seed.key,"shape":seed.shape}});
        let render_begin = Instant::now();
        let output = engine.render(&context, &returned)?;
        let render_ns = render_begin.elapsed().as_nanos() as f64;
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
                                v * (f32::from(native_settings.density_percent) / 100.0)
                                    > ilium_ambient::raster::threshold(x, y, native_settings.dither)
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
            return Err(invalid("Surface rejected Carpet frame"));
        }
        let js_masks = packed.ok_or_else(|| invalid("accepted Carpet frame was not packed"))?;
        let js_ns = begin.elapsed().as_nanos() as f64;
        if native_masks.len() != js_masks.len() {
            return Err(invalid("native/V8 Carpet mask lengths differ"));
        }
        if index >= options.warmup {
            native_times.push(native_ns);
            js_times.push(js_ns);
            js_render_times.push(render_ns);
            for (&native_mask, &js_mask) in native_masks.iter().zip(&js_masks) {
                changed_bits += u64::from((native_mask ^ js_mask).count_ones());
                compared_bits += 8;
                changed_cells += u64::from(native_mask != js_mask);
            }
        }
    }
    let native_stats = stats(&mut native_times);
    let js_stats = stats(&mut js_times);
    emit(
        json!({"type":"result","case":format!("carpet-{mode}"),"mode":mode,"status":"measured_pending_fidelity_review",
        "package_sha256":package.digest(),"native_render_pack_collect":native_stats,
        "v8_seed_render_accept_pack_collect":js_stats,"v8_render_only":stats(&mut js_render_times),
        "raw_median_pipeline_ratio":js_stats["median_ns"].as_f64().zip(native_stats["median_ns"].as_f64()).map(|(j,n)|j/n),
        "changed_braille_bits":changed_bits,"compared_braille_bits":compared_bits,"changed_cells":changed_cells,
        "input_fixture":{"pointer":"normalized (0.7, 0.4) for mode 0","clock":"current UTC snapshot for modes 7 and 8","remote_source":"none; mode 4 excluded"},
        "excludes":["protected helper IPC","permission broker","UI composition","terminal emission"]}),
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
    initialize_engine(quota.clone(), 1)?;
    let _package_storage = quota
        .reserve_external_storage(160 * MIB)
        .map_err(|reason| invalid(&format!("package admission: {reason:?}")))?;
    let mut file = std::fs::File::open(&options.package)?;
    let size = file.metadata()?.len();
    let limits = PackageLimits::default();
    if size > limits.archive_bytes {
        return Err(invalid("package archive exceeds bound"));
    }
    let mut archive = Vec::with_capacity(size as usize + 1);
    file.by_ref().take(size + 1).read_to_end(&mut archive)?;
    if archive.len() as u64 != size {
        return Err(invalid("archive changed size during read"));
    }
    let package = Arc::new(Package::from_bytes(&archive, limits)?);
    if package.digest() != options.digest || package.manifest().id != "carpet" {
        return Err(invalid(
            "archive identity differs from explicit pinned Carpet package",
        ));
    }
    let native_source_sha256: BTreeMap<_, _> = NATIVE_BASELINES
        .iter()
        .map(|(path, bytes)| (*path, digest(bytes)))
        .collect();
    let v8_runtime_source_sha256: BTreeMap<_, _> = V8_RUNTIME_SOURCES
        .iter()
        .map(|(path, bytes)| (*path, digest(bytes)))
        .collect();
    emit(
        json!({"type":"manifest","benchmark":"v8_carpet_performance",
        "benchmark_source_sha256":digest(include_bytes!("v8_carpet_performance.rs")),
        "bootstrap_sha256":digest(TRUSTED_BOOTSTRAP.as_bytes()),"package":options.package,
        "native_source_sha256":native_source_sha256,"v8_runtime_source_sha256":v8_runtime_source_sha256,
        "package_sha256":package.digest(),"width_cells":options.width,"height_cells":options.height,
        "fps":options.fps,"warmup":options.warmup,"frames":options.frames,
        "native_boundary":"public AnimationFrame::render plus glyph collection",
        "v8_boundary":"trusted bootstrap, seeded binary views, Surface acceptance and native pack",
        "excluded_mode":{"4":"Lichess TV requires a matching live game-source snapshot; no network source is fabricated"},
        "clock":"modes 7 and 8 receive live UTC snapshots; exact native/V8 wall-clock identity is not guaranteed",
        "excludes":["helper IPC","permission acquisition","UI composition","terminal emission"],"cpu_time_ns":null}),
    );
    for mode in MEASURED_MODES {
        measure(&options, &package, &quota, mode)?;
    }
    emit(
        json!({"type":"summary","status":"measured_pending_fidelity_review",
        "measured_modes":MEASURED_MODES,"excluded_modes":[4],
        "retained_worker_threads":quota.snapshot().worker_threads,
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

#[cfg(test)]
mod package_path_tests {
    use super::{digest, resolve_package_path};
    use std::path::PathBuf;

    #[test]
    fn relative_package_path_loads_the_bundled_carpet_archive_from_the_snapshot() {
        let archive_path =
            resolve_package_path("../ilium-animation-js/assets/packages/carpet-1.0.0.iliumanim");
        let archive = std::fs::read(archive_path).expect("bundled Carpet archive exists");

        assert_eq!(
            digest(&archive),
            "5ef31ca8cee61fd7f0c077128419f81dbea59ef516813453532a98e976d8daf9"
        );
    }

    #[test]
    fn absolute_package_path_is_preserved() {
        let path = PathBuf::from("/tmp/benchmark-package.iliumanim");

        assert_eq!(resolve_package_path(path.to_str().unwrap()), path);
    }
}

#[cfg(test)]
mod tests {
    use super::{digest, NATIVE_BASELINES};
    use std::collections::BTreeMap;

    #[test]
    fn native_benchmark_manifest_binds_scene_dispatch_and_palette_construction() {
        let hashes: BTreeMap<_, _> = NATIVE_BASELINES
            .iter()
            .map(|(path, bytes)| (*path, digest(bytes)))
            .collect();

        for path in [
            "ilium-client/src/background_animation/host.rs",
            "ilium-ambient/src/registry.rs",
            "ilium-ambient/src/scene.rs",
            "ilium-ambient/src/resources.rs",
            "ilium-ambient/src/lib.rs",
        ] {
            let hash = hashes
                .get(path)
                .unwrap_or_else(|| panic!("benchmark source manifest omits {path}"));
            assert_eq!(hash.len(), 64, "invalid SHA-256 for {path}");
        }
    }
}
