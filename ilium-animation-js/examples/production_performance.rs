//! Trusted, in-process native/V8 frame comparison; JSONL stdout.
//! This deliberately excludes protected-helper IPC, UI composition and terminal
//! output. Logical Surface acceptance never certifies terminal presentation.
//! Native Beach lacks a public factory in this crate; TV lacks an injected
//! native feed; automatic chess lacks a deterministic native completion barrier.
//! Beach/TV are BLOCKED; automatic chess is measured diagnostically with
//! parity blocked. No case is replaced with copied algorithms.
#[cfg(all(feature = "v8-runtime", feature = "native-host"))]
fn main() {
    if let Err(error) = benchmark::run() {
        println!(
            "{}",
            serde_json::json!({"type":"error","message":error.to_string()})
        );
        std::process::exit(1);
    }
}
#[cfg(not(all(feature = "v8-runtime", feature = "native-host")))]
fn main() {
    println!(
        "{}",
        serde_json::json!({"type":"error","message":"production_performance requires v8-runtime and native-host"})
    );
    std::process::exit(1);
}
#[cfg(all(feature = "v8-runtime", feature = "native-host"))]
mod benchmark {
    use ilium_ambient::{
        resources::AmbientResources, AmbientKind, AmbientSettings, Frame, Raster, SceneEnv,
        SceneSettings,
    };
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
            SurfaceError, Update,
        },
        TRUSTED_BOOTSTRAP,
    };
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        ShutdownMode, StorageAdmission,
    };
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{
        collections::{BTreeMap, BTreeSet},
        error::Error,
        io::Read,
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant, SystemTime},
    };
    type Result<T> = std::result::Result<T, Box<dyn Error>>;
    const MIB: usize = 1024 * 1024;
    const CASES: [&str; 11] = [
        "beach-classic",
        "beach-rich",
        "carpet-0",
        "carpet-1",
        "carpet-2",
        "carpet-3",
        "carpet-4",
        "carpet-5",
        "carpet-6",
        "carpet-7",
        "carpet-8",
    ];
    // Exact compiled native originals, including the blocked Beach boundary.
    const BASELINES: [(&str, &str); 9] = [
        (
            "ilium-client/src/background_animation/shoreline.rs",
            include_str!("../../ilium-client/src/background_animation/shoreline.rs"),
        ),
        (
            "ilium-client/src/background_animation/scenes.rs",
            include_str!("../../ilium-client/src/background_animation/scenes.rs"),
        ),
        (
            "ilium-ambient/src/scenes/carpet/mod.rs",
            include_str!("../../ilium-ambient/src/scenes/carpet/mod.rs"),
        ),
        (
            "ilium-ambient/src/scenes/carpet/settings.rs",
            include_str!("../../ilium-ambient/src/scenes/carpet/settings.rs"),
        ),
        (
            "ilium-ambient/src/scenes/carpet/render.rs",
            include_str!("../../ilium-ambient/src/scenes/carpet/render.rs"),
        ),
        (
            "ilium-ambient/src/scenes/carpet/simulations.rs",
            include_str!("../../ilium-ambient/src/scenes/carpet/simulations.rs"),
        ),
        (
            "ilium-ambient/src/scenes/carpet/chess.rs",
            include_str!("../../ilium-ambient/src/scenes/carpet/chess.rs"),
        ),
        (
            "ilium-ambient/src/raster.rs",
            include_str!("../../ilium-ambient/src/raster.rs"),
        ),
        (
            "ilium-ambient/src/scene.rs",
            include_str!("../../ilium-ambient/src/scene.rs"),
        ),
    ];
    fn fail(message: &str) -> Box<dyn Error> {
        std::io::Error::other(message.to_owned()).into()
    }
    fn emit(value: Value) {
        println!("{value}");
    }
    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    struct Options {
        beach: PathBuf,
        carpet: PathBuf,
        beach_sha: String,
        carpet_sha: String,
        width: u32,
        height: u32,
        fps: u32,
        warmup: usize,
        frames: usize,
        epoch_ms: u64,
        cases: Vec<String>,
    }
    fn options() -> Result<Option<Options>> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args == ["--help"] {
            emit(
                json!({"type":"result","usage":"production_performance --beach-package ABS --beach-sha256 HEX --carpet-package ABS --carpet-sha256 HEX --width CELLS --height CELLS --fps INTEGER --warmup COUNT --frames COUNT --civil-epoch-ms INTEGER --cases all|comma-separated-case-ids","cases":CASES,"bounds":{"width":[1,320],"height":[1,120],"fps":[1,30],"warmup":[0,1000],"frames":[1,10000]},"features":["v8-runtime","native-host"]}),
            );
            return Ok(None);
        }
        let allowed = [
            "--beach-package",
            "--beach-sha256",
            "--carpet-package",
            "--carpet-sha256",
            "--width",
            "--height",
            "--fps",
            "--warmup",
            "--frames",
            "--civil-epoch-ms",
            "--cases",
        ];
        if args.len() != allowed.len() * 2 {
            return Err(fail("every documented flag is required exactly once"));
        }
        let mut flags = BTreeMap::new();
        for pair in args.chunks_exact(2) {
            if !allowed.contains(&pair[0].as_str())
                || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
            {
                return Err(fail("unknown or duplicate flag"));
            }
        }
        let get = |name: &str| flags.get(name).copied().ok_or_else(|| fail("missing flag"));
        let number = |name: &str, minimum: u64, maximum: u64| -> Result<u64> {
            let value = get(name)?.parse::<u64>()?;
            if value < minimum || value > maximum {
                return Err(fail("numeric flag outside declared bounds"));
            }
            Ok(value)
        };
        let path = |name: &str| -> Result<PathBuf> {
            let path = PathBuf::from(get(name)?);
            if !path.is_absolute() {
                return Err(fail("package paths must be absolute"));
            }
            Ok(path)
        };
        let sha = |name: &str| -> Result<String> {
            let value = get(name)?;
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(fail("SHA256 must be 64 lowercase hexadecimal characters"));
            }
            Ok(value.into())
        };
        let requested = get("--cases")?;
        let cases: Vec<String> = if requested == "all" {
            CASES.iter().map(|name| (*name).into()).collect()
        } else {
            requested.split(',').map(str::to_owned).collect()
        };
        let mut seen = BTreeSet::new();
        if cases
            .iter()
            .any(|name| !CASES.contains(&name.as_str()) || !seen.insert(name))
        {
            return Err(fail("unknown or duplicate case; order is explicit"));
        }
        Ok(Some(Options {
            beach: path("--beach-package")?,
            carpet: path("--carpet-package")?,
            beach_sha: sha("--beach-sha256")?,
            carpet_sha: sha("--carpet-sha256")?,
            width: number("--width", 1, 320)? as u32,
            height: number("--height", 1, 120)? as u32,
            fps: number("--fps", 1, 30)? as u32,
            warmup: number("--warmup", 0, 1000)? as usize,
            frames: number("--frames", 1, 10000)? as usize,
            epoch_ms: number("--civil-epoch-ms", 0, 4_102_444_800_000)?,
            cases,
        }))
    }
    struct Loaded {
        package: Arc<Package>,
        _storage: StorageAdmission,
    }
    fn load(path: &Path, expected: &str, id: &str, quota: &QuotaGroup) -> Result<Loaded> {
        let limits = PackageLimits::default();
        let file = std::fs::File::open(path)?;
        let length = file.metadata()?.len();
        if length > limits.archive_bytes {
            return Err(fail("archive exceeds loader bound"));
        }
        // Cover both archive scratch and all expanded payload/metadata before IO.
        let storage = quota
            .reserve_external_storage(
                (limits.archive_bytes + limits.expanded_bytes) as usize + 8 * MIB,
            )
            .map_err(|reason| fail(&format!("package admission: {reason:?}")))?;
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(limits.archive_bytes + 1)
            .read_to_end(&mut bytes)?;
        let archive_sha = digest(&bytes);
        if archive_sha != expected {
            return Err(fail("archive differs from explicitly pinned SHA256"));
        }
        let package = Arc::new(Package::from_bytes(&bytes, limits)?);
        if package.manifest().id != id {
            return Err(fail("package id does not match selected native baseline"));
        }
        emit(
            json!({"type":"artifact","path":path,"archive_sha256":archive_sha,"package_digest":package.digest(),"canonical_manifest_sha256":digest(&serde_json::to_vec(package.manifest())?),"manifest":package.manifest()}),
        );
        Ok(Loaded {
            package,
            _storage: storage,
        })
    }
    pub fn run() -> Result<()> {
        let Some(options) = options()? else {
            return Ok(());
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 16,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 64 * MIB,
            result_bytes: 64 * MIB,
            worker_threads: 16,
            worker_bytes: 1536 * MIB,
        });
        initialize_engine(quota.clone(), 1)?;
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: MIB,
        };
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let mut execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: disabled,
                service: disabled,
            },
        )?;
        let client = execution
            .client(ClientLimits {
                jobs: 16,
                service_jobs: 0,
                input_bytes: 64 * MIB,
                result_bytes: 64 * MIB,
            })
            .map_err(|reason| fail(&format!("finite client: {reason:?}")))?;
        let env = SceneEnv::for_test(
            std::env::temp_dir().join("ilium-performance-unused-cache"),
            AmbientResources::new(client),
        );
        // Selected supported native scenes perform no filesystem/network capture.
        let _owner = quota
            .reserve_external_worker(1, 4 * MIB)
            .map_err(|reason| fail(&format!("owner admission: {reason:?}")))?;
        let beach = load(&options.beach, &options.beach_sha, "beach", &quota)?;
        let carpet = load(&options.carpet, &options.carpet_sha, "carpet", &quota)?;
        let baselines: BTreeMap<_, _> = BASELINES
            .iter()
            .map(|(path, source)| (*path, digest(source.as_bytes())))
            .collect();
        emit(
            json!({"type":"manifest","benchmark":"production_performance","benchmark_source_sha256":digest(include_bytes!("production_performance.rs")),"bootstrap_sha256":digest(TRUSTED_BOOTSTRAP.as_bytes()),"native_baselines":baselines,"width_cells":options.width,"height_cells":options.height,"fps":options.fps,"warmup":options.warmup,"measured_frames":options.frames,"civil_epoch_ms":options.epoch_ms,"ordered_cases":options.cases,"root_limits":format!("{:?}",quota.snapshot().limits),"scope":"trusted in-process native scene and embedded V8 render, binary handoff and logical Surface acceptance","excludes":["protected helper IPC","permission broker acquisition","UI composition","terminal emission","package startup timing"],"cpu_time_ns":null,"cpu_time_limitation":"No portable platform CPU-time API is available; durations use monotonic elapsed wall time, never claimed as CPU time.","pack_policy":"identity tone, fixed 0.5 threshold; no protected source owner or terminal credit"}),
        );
        let mut blocked = 0;
        let mut measured = 0;
        let mut async_parity_blocked = 0;
        for case in &options.cases {
            let reason=match case.as_str() {
                "beach-classic"|"beach-rich"=>Some("native client PreparedScene/shoreline renderers are private; this crate has neither a public native Beach factory nor a client dependency"),
                "carpet-4"=>Some("native Carpet TV starts LiveTv directly; no public injected deterministic feed exists, and this benchmark never acquires real system/network data"),
                _=>None,
            };
            if let Some(reason) = reason {
                blocked += 1;
                emit(
                    json!({"type":"result","case":case,"status":"BLOCKED","reason":reason,"package_digest":if case.starts_with("beach"){beach.package.digest()}else{carpet.package.digest()}}),
                );
                continue;
            }
            measure(case, &options, &carpet.package, &env, &quota)?;
            measured += 1;
            if case == "carpet-3" {
                async_parity_blocked += 1;
            }
        }
        drop(carpet);
        drop(beach);
        drop(env);
        execution.request_shutdown(ShutdownMode::Cancel);
        let retired = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .map_err(|reason| fail(&format!("finite bank teardown: {reason:?}")))?;
        if retired.remaining_workers != 0 {
            return Err(fail(
                "native finite bank did not retire before benchmark teardown deadline",
            ));
        }
        drop(execution);
        emit(
            json!({"type":"summary","measured_cases":measured,"blocked_cases":blocked,"async_parity_blocked_measured_cases":async_parity_blocked,"status":if blocked==0 && async_parity_blocked==0{"measured"}else{"partial"},"retained_worker_threads":quota.snapshot().worker_threads,"retained_worker_bytes":quota.snapshot().worker_bytes,"retained_note":"process-global V8 platform remains initialized; this is not a zero-resource claim"}),
        );
        Ok(())
    }
    fn specification(name: &str, kind: TypedArrayKind, elements: usize) -> ArraySpec {
        ArraySpec {
            name: name.into(),
            kind,
            elements,
        }
    }
    fn f32_bytes(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect()
    }
    fn decode_f32(bytes: &[u8], count: usize) -> Result<Vec<f32>> {
        if bytes.len() != count * 4 {
            return Err(fail("sealed gray32 byte shape mismatch"));
        }
        Ok(bytes
            .chunks_exact(4)
            .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .collect())
    }
    fn plane<'a>(output: &'a RenderOutput, name: &str, bytes: usize) -> Result<&'a [u8]> {
        let values = output
            .planes
            .get(name)
            .ok_or_else(|| fail("sealed output plane missing"))?;
        if values.len() != bytes {
            return Err(fail("sealed output plane byte shape mismatch"));
        }
        Ok(values)
    }
    fn returned_planes(output: &RenderOutput, samples: usize) -> Result<Planes> {
        Ok(Planes {
            data: Data::F32(decode_f32(plane(output, "data", samples * 4)?, samples)?),
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
    fn commit(
        surface: &mut Surface,
        metadata: FrameMeta,
        planes: Planes,
        quota: &QuotaGroup,
    ) -> Result<()> {
        let outcome =
            surface.finish_with(metadata, planes, &mut NoNativeRenderer, |candidate, _| {
                let layout = candidate.shape().layout()?;
                let _stage = quota
                    .reserve_external_storage(layout.dots * 16 + layout.cells * 32 + 1024)
                    .map_err(|_| SurfaceError::Capacity)?;
                let packed = candidate.pack(|v, _, _| v, |v, _, _| v >= 0.5)?;
                std::hint::black_box(packed);
                Ok(())
            })?;
        if !outcome.accepted {
            return Err(fail("logical surface transaction was not accepted"));
        }
        Ok(())
    }
    fn stats(values: &mut [f64]) -> Value {
        values.sort_by(f64::total_cmp);
        let count = values.len();
        let median = if count.is_multiple_of(2) {
            (values[count / 2 - 1] + values[count / 2]) / 2.0
        } else {
            values[count / 2]
        };
        let p95 = ((count * 95).div_ceil(100)).saturating_sub(1);
        json!({"median_ns":median,"p95_ns":values[p95],"minimum_ns":values[0],"maximum_ns":values[count-1],"count":count})
    }
    fn measure(
        case: &str,
        options: &Options,
        package: &Arc<Package>,
        env: &SceneEnv,
        quota: &QuotaGroup,
    ) -> Result<()> {
        let mode = case
            .strip_prefix("carpet-")
            .ok_or_else(|| fail("unexpected measurement case"))?
            .parse::<i32>()?;
        let mut ambient = AmbientSettings::default();
        ambient.carpet.mode = mode;
        ambient.carpet.fps = options.fps as i32;
        ambient.carpet = ambient.carpet.normalized();
        if ambient.carpet.fps != options.fps as i32 {
            return Err(fail("requested FPS is changed by native normalization"));
        }
        let settings = validate_settings(
            &package.manifest().settings,
            &serde_json::to_value(&ambient.carpet)?,
        )?;
        if settings != serde_json::to_value(&ambient.carpet)? {
            return Err(fail("package/native normalized settings are different"));
        }
        let shape = Shape {
            cell_width: options.width,
            cell_height: options.height,
            mode: Mode::Pixels,
            format: Format::Gray32,
            update: Update::Replace,
            cell_rgb: false,
            colour_space: ColourSpace::Srgb,
        };
        let layout = shape.layout()?;
        if layout.handoff_bytes > EngineLimits::default().frame_bytes {
            return Err(fail("frame exceeds unchanged engine frame budget"));
        }
        // Covers native scene retained caches, both Surface stores and every
        // simultaneous candidate/seed/copied plane/JSON/hash/raster allocation.
        let _storage = quota
            .reserve_external_storage(
                layout.canonical_bytes * 8
                    + layout.handoff_bytes * 8
                    + layout.dots * 96
                    + 32 * MIB
                    + options.frames * 64,
            )
            .map_err(|reason| fail(&format!("case admission: {reason:?}")))?;
        let preparation = Instant::now();
        let mut native = ambient.create_scene(AmbientKind::Carpet, env);
        let native_preparation_ns = preparation.elapsed().as_nanos() as f64;
        let mut raster = Raster::default();
        raster.resize(options.width as usize * 2, options.height as usize * 4);
        let mut colors = vec![[0; 3]; layout.cells];
        let mut native_surface = Surface::new(2, 1, shape)?;
        let mut js_surface = Surface::new(1, 1, shape)?;
        let js_preparation = Instant::now();
        let mut limits = EngineLimits::default();
        limits.render_ms = limits.render_ms.min(package.manifest().limits.render_ms);
        limits.preparation_ms = limits
            .preparation_ms
            .min(package.manifest().limits.preparation_ms);
        let mut engine = Engine::new(Arc::clone(package), limits, quota.clone())?;
        engine.install_bootstrap(TRUSTED_BOOTSTRAP)?;
        engine.load()?;
        let environment = json!({"viewport":{"cell_width":options.width,"cell_height":options.height,"dot_width":options.width*2,"dot_height":options.height*4,"revision":1},"available":{"pointer":mode==0,"audio":false,"gpu":false,"location":false}});
        let plan = engine.plan(&settings, AnimationMode::Live, &environment)?;
        if plan["output"] != json!({"mode":"pixels","format":"gray32","update":"replace"}) {
            return Err(fail(
                "package output is not the exact compared gray32 replace contract",
            ));
        }
        if engine.start_create(&settings, &plan)? != CreateState::Ready
            || !engine.take_requests()?.is_empty()
        {
            return Err(fail(
                "benchmark scene attempted external acquisition or asynchronous preparation",
            ));
        }
        let js_preparation_ns = js_preparation.elapsed().as_nanos() as f64;
        let returned = [
            specification("work_data", TypedArrayKind::F32, layout.elements),
            specification("data", TypedArrayKind::F32, layout.elements),
            specification("work_touch", TypedArrayKind::U8, layout.samples),
            specification("touch", TypedArrayKind::U8, layout.samples),
            specification("work_order", TypedArrayKind::U32, layout.samples),
            specification("order", TypedArrayKind::U32, layout.samples),
        ];
        let seed_spec = [specification(
            "work_data",
            TypedArrayKind::F32,
            layout.elements,
        )];
        let mut native_render = Vec::with_capacity(options.frames);
        let mut native_full = Vec::with_capacity(options.frames);
        let mut js_render = Vec::with_capacity(options.frames);
        let mut js_full = Vec::with_capacity(options.frames);
        let mut native_hash = Sha256::new();
        let mut js_hash = Sha256::new();
        let mut input_hash = Sha256::new();
        let mut max_error = 0.0_f64;
        let mut squared_error = 0.0;
        let mut compared = 0_u64;
        let mut compared_cells = 0_u64;
        let mut different_braille_cells = 0_u64;
        let mut different_braille_dots = 0_u64;
        let mut native_mask_hash = Sha256::new();
        let mut js_mask_hash = Sha256::new();
        let mut thinking_frames = 0_u64;
        let mut native_status_transitions = 0_u64;
        let mut last_native_status: Option<String> = None;
        for index in 0..options.warmup + options.frames {
            let sequence = index as u64 + 1;
            let time = Duration::from_secs_f64(index as f64 / f64::from(options.fps));
            let seconds = time.as_secs_f64();
            let previous = if index == 0 {
                0.0
            } else {
                Duration::from_secs_f64((index - 1) as f64 / f64::from(options.fps)).as_secs_f64()
            };
            let delta = seconds - previous;
            let civil_ms = options.epoch_ms + (seconds * 1000.0).round() as u64;
            let now = SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_millis(civil_ms))
                .ok_or_else(|| fail("civil time overflow"))?;
            // A fixed, host-owned normalized pointer is sufficient to exercise
            // pointer projection while avoiding float trajectory divergence.
            native.pointer((mode == 0).then_some([0.5, 0.5]));
            let mut inputs = json!({});
            if mode == 0 {
                inputs["pointer"] = json!({"x":0.5,"y":0.5,"inside":true});
            }
            if mode == 7 || mode == 8 {
                inputs["clock"] = json!({"epoch_ms":civil_ms,"timezone":"UTC"});
            }
            let mut context = json!({"viewport":environment["viewport"],"time":seconds,"wall":seconds,"delta":delta,"wall_delta":delta,"settings":settings,"visible":true,"inputs":inputs,"render_policy":{"can_skip_occluded":false}});
            input_hash.update(serde_json::to_vec(&context)?);
            // Alternate first execution to reduce systematic hot-cache order bias.
            let native_frame = |native: &mut dyn ilium_ambient::Scene,
                                raster: &mut Raster,
                                colors: &mut Vec<[u8; 3]>,
                                surface: &mut Surface|
             -> Result<(f64, f64)> {
                let full = Instant::now();
                let render = Instant::now();
                raster.dots.fill(0.0);
                raster.owner_ids.fill(0);
                native.render(&mut Frame {
                    raster: &mut *raster,
                    cell_colors: &mut *colors,
                    width: options.width as u16,
                    height: options.height as u16,
                    time,
                    wall: time,
                    now,
                });
                let render_ns = render.elapsed().as_nanos() as f64;
                let seed = surface.begin(sequence)?;
                commit(
                    surface,
                    FrameMeta {
                        wire_version: 1,
                        key: seed.key,
                        shape,
                        presented: true,
                        error: None,
                        commands: vec![],
                    },
                    Planes {
                        data: Data::F32(raster.dots.clone()),
                        touch: vec![1; layout.samples],
                        order: vec![1; layout.samples],
                        cell_rgb: None,
                        colour_touch: None,
                        colour_order: None,
                    },
                    quota,
                )?;
                Ok((render_ns, full.elapsed().as_nanos() as f64))
            };
            let js_frame = |engine: &mut Engine,
                            surface: &mut Surface,
                            context: &mut Value|
             -> Result<(f64, f64)> {
                let full = Instant::now();
                let seed = surface.begin(sequence)?;
                let Data::F32(data) = seed.data else {
                    return Err(fail("unexpected seed format"));
                };
                let metadata = json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,"invalid_rects":seed.invalid_rects,"input_specs":[]}});
                let binary = BTreeMap::from([("work_data".into(), f32_bytes(&data))]);
                engine.seed_frame(&metadata, &seed_spec, &binary)?;
                context["_ilium_frame"] = json!({"key":seed.key,"shape":seed.shape});
                let render = Instant::now();
                let output = engine.render(context, &returned)?;
                let render_ns = render.elapsed().as_nanos() as f64;
                let meta = FrameMeta::parse(&serde_json::to_vec(&output.metadata)?)?;
                let sealed = returned_planes(&output, layout.samples)?;
                let committed = commit(surface, meta, sealed, quota);
                engine.accept_frame(committed.is_ok())?;
                committed?;
                Ok((render_ns, full.elapsed().as_nanos() as f64))
            };
            let (n, j) = if index.is_multiple_of(2) {
                let n = native_frame(
                    native.as_mut(),
                    &mut raster,
                    &mut colors,
                    &mut native_surface,
                )?;
                let j = js_frame(&mut engine, &mut js_surface, &mut context)?;
                (n, j)
            } else {
                let j = js_frame(&mut engine, &mut js_surface, &mut context)?;
                let n = native_frame(
                    native.as_mut(),
                    &mut raster,
                    &mut colors,
                    &mut native_surface,
                )?;
                (n, j)
            };
            if mode == 3 {
                let status = native.status();
                if status.as_deref() == Some("Automatic chess: thinking") {
                    thinking_frames += 1;
                }
                if status != last_native_status {
                    native_status_transitions += 1;
                    last_native_status = status;
                }
            }
            if index < options.warmup {
                continue;
            }
            native_render.push(n.0);
            native_full.push(n.1);
            js_render.push(j.0);
            js_full.push(j.1);
            let Data::F32(js_pixels) = js_surface.snapshot().data() else {
                return Err(fail("unexpected accepted canonical format"));
            };
            if js_pixels.len() != raster.dots.len() {
                return Err(fail("accepted differential shape mismatch"));
            }
            // Compare the actual accepted Surface pack, outside timed render
            // regions. A tiny float error can still cross a binary threshold.
            let comparison_bytes = layout
                .cells
                .checked_mul(std::mem::size_of::<u8>() + std::mem::size_of::<Option<[u8; 3]>>())
                .and_then(|bytes| {
                    bytes.checked_add(std::mem::size_of_val(native_surface.snapshot().owners()))
                })
                .and_then(|bytes| bytes.checked_mul(2))
                .and_then(|bytes| bytes.checked_add(4096))
                .ok_or_else(|| fail("packed parity storage overflow"))?;
            let _comparison_storage = quota
                .reserve_external_storage(comparison_bytes)
                .map_err(|_| fail("packed parity storage admission refused"))?;
            let native_packed = native_surface
                .snapshot()
                .pack(|value, _, _| value, |value, _, _| value >= 0.5)?;
            let js_packed = js_surface
                .snapshot()
                .pack(|value, _, _| value, |value, _, _| value >= 0.5)?;
            if native_packed.masks.len() != layout.cells || js_packed.masks.len() != layout.cells {
                return Err(fail("accepted packed differential shape mismatch"));
            }
            native_mask_hash.update(&native_packed.masks);
            js_mask_hash.update(&js_packed.masks);
            for (&native_mask, &js_mask) in native_packed.masks.iter().zip(&js_packed.masks) {
                compared_cells += 1;
                different_braille_cells += u64::from(native_mask != js_mask);
                different_braille_dots += u64::from((native_mask ^ js_mask).count_ones());
            }
            for (&native_value, &js_value) in raster.dots.iter().zip(js_pixels) {
                if !native_value.is_finite() || !js_value.is_finite() {
                    return Err(fail("nonfinite compared pixel"));
                }
                native_hash.update(native_value.to_ne_bytes());
                js_hash.update(js_value.to_ne_bytes());
                let error = f64::from(native_value) - f64::from(js_value);
                max_error = max_error.max(error.abs());
                squared_error += error * error;
                compared += 1;
            }
        }
        engine.dispose()?;
        drop(engine);
        drop(native);
        // Timings remain diagnostic until differential output is qualified.
        // This emits no synthetic CPU measurement or unqualified speed ratio.
        emit(
            json!({"type":"result","case":case,"status":if mode==3{"diagnostic_parity_blocked"}else{"measured"},"settings":settings,"native_preparation_elapsed_ns":native_preparation_ns,"v8_preparation_elapsed_ns":js_preparation_ns,"native_async_search":if mode==3{json!({"thinking_frames_including_warmup":thinking_frames,"status_transitions":native_status_transitions,"last_status":last_native_status,"search_worker_cpu_time_ns":null,"completion_wait_elapsed_ns":null,"qualification":"decision and body equality must be established before interpreting any ratio; no public native completion barrier or decision snapshot exists"})}else{Value::Null},"package_digest":package.digest(),"native_scene_render_elapsed":stats(&mut native_render),"native_plus_common_surface_elapsed":stats(&mut native_full),"v8_render_and_binary_handoff_elapsed":stats(&mut js_render),"v8_seed_render_and_accepted_surface_elapsed":stats(&mut js_full),"cpu_time_ns":null,"clock":"monotonic elapsed wall time","native_full_adapter":"native Raster copied into common gray32 Surface, validated and packed; this adapter is benchmark-only","v8_full_adapter":"binary seed, production facade render, detach/copy, sealed-plane validation, Surface validation/pack, acceptance microtasks","packed_differential":{"compared_cells":compared_cells,"different_cells":different_braille_cells,"different_dots":different_braille_dots,"native_masks_sha256":format!("{:x}",native_mask_hash.finalize()),"v8_masks_sha256":format!("{:x}",js_mask_hash.finalize()),"policy":"actual accepted Surface pack; identity tone; fixed0.5 threshold","scope":"pre-compositor Braille masks only; no terminal emission/source-authority proof","status":if mode==3{"blocked_async_decision_equality"}else if different_braille_cells==0{"exact_masks"}else{"different_masks"}},"differential":{"compared_scalars":compared,"max_abs_error":max_error,"rmse":(squared_error/compared as f64).sqrt(),"native_sha256":format!("{:x}",native_hash.finalize()),"v8_sha256":format!("{:x}",js_hash.finalize()),"status":if mode==3{"blocked_async_decision_equality"}else if max_error==0.0{"exact"}else{"requires_review"}},"input_sha256":format!("{:x}",input_hash.finalize()),"cold_preparation_included":false}),
        );
        Ok(())
    }
}
