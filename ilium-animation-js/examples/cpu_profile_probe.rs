//! Captures one real package render with V8 Inspector. JSONL stdout.
//!
//! This diagnostic uses the same deliberately small host facade as
//! `engine_probe`; it does not grant permissions or reproduce helper IPC.
#[cfg(feature = "diagnostic-profiler")]
fn main() {
    if let Err(error) = run() {
        println!(
            "{}",
            serde_json::json!({"type":"error","message":error.to_string()})
        );
        std::process::exit(1);
    }
}
#[cfg(not(feature = "diagnostic-profiler"))]
fn main() {
    println!(
        "{}",
        serde_json::json!({"type":"error","message":"cpu_profile_probe requires diagnostic-profiler feature"})
    );
    std::process::exit(1);
}
#[cfg(feature = "diagnostic-profiler")]
fn run() -> ilium_animation_js::error::Result<()> {
    use ilium_animation_js::{
        engine::{
            initialize_engine, ArraySpec, CreateState, Engine, EngineLimits, ServiceAuthority,
            TypedArrayKind,
        },
        error::AnimationError,
        manifest::AnimationMode,
        package::{Package, PackageLimits},
        settings::validate_settings,
    };
    use ilium_execution::{QuotaGroup, QuotaLimits};
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, io::Read, sync::Arc, time::Instant};
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let fail = |message: &str| AnimationError::Runtime(message.into());
    if arguments.iter().any(|argument| argument == "--help") {
        println!(
            "{}",
            serde_json::json!({"type":"result","usage":"cpu_profile_probe --package PATH --sha256 HEX --mode live|pre_rendered --width CELLS --height CELLS --frames 1 --cpu-profile true [--settings JSON]","profile_frame_count":1,"profile_bound_bytes":8388608,"limitations":"diagnostic host facade; no helper IPC, permission grants or production timing"})
        );
        return Ok(());
    }
    if arguments.len() % 2 != 0 {
        return Err(fail("each CLI flag requires one value"));
    }
    let mut flags = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if ![
            "--package",
            "--sha256",
            "--mode",
            "--width",
            "--height",
            "--frames",
            "--cpu-profile",
            "--settings",
        ]
        .contains(&pair[0].as_str())
            || flags.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err(fail("unknown/duplicate CLI flag"));
        }
    }
    let path = flags
        .get("--package")
        .ok_or_else(|| fail("--package is required"))?;
    if !std::path::Path::new(path).is_absolute() {
        return Err(fail("--package must be an absolute path"));
    }
    let parse = |flag: &str, maximum: usize| -> ilium_animation_js::error::Result<usize> {
        let number = flags
            .get(flag)
            .ok_or_else(|| fail("missing numeric flag"))?
            .parse::<usize>()
            .map_err(|_| fail("invalid numeric flag"))?;
        if number == 0 || number > maximum {
            return Err(fail("numeric flag outside bounds"));
        }
        Ok(number)
    };
    let width = parse("--width", 480)?;
    let height = parse("--height", 160)?;
    let frames = parse("--frames", 3600)?;
    if flags.get("--cpu-profile").map(String::as_str) != Some("true") || frames != 1 {
        return Err(fail("--cpu-profile true requires exactly one frame"));
    }
    let mode = match flags.get("--mode").map(String::as_str) {
        Some("live") => AnimationMode::Live,
        Some("pre_rendered") => AnimationMode::PreRendered,
        _ => return Err(fail("--mode must be live or pre_rendered")),
    };
    let limits = PackageLimits::default();
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limits.archive_bytes {
        return Err(fail("package archive budget exceeded"));
    }
    let mut bytes = Vec::new();
    file.take(limits.archive_bytes + 1)
        .read_to_end(&mut bytes)?;
    let expected_digest = flags
        .get("--sha256")
        .ok_or_else(|| fail("--sha256 is required"))?;
    if expected_digest.len() != 64
        || !expected_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || format!("{:x}", Sha256::digest(&bytes)) != *expected_digest
    {
        return Err(fail("package SHA256 does not match --sha256"));
    }
    let package = Arc::new(Package::from_bytes(&bytes, limits)?);
    let supplied = flags
        .get("--settings")
        .map_or(Ok(serde_json::json!({})), |json| serde_json::from_str(json))?;
    let settings = validate_settings(&package.manifest().settings, &supplied)?;
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 16,
        jobs: 16,
        service_jobs: 16,
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 32 * 1024 * 1024,
        worker_threads: 8,
        worker_bytes: 512 * 1024 * 1024,
    });
    initialize_engine(quota.clone(), 1)?;
    let _main_worker = quota
        .reserve_external_worker(1, 4 * 1024 * 1024)
        .map_err(|error| AnimationError::Budget(format!("probe worker: {error:?}")))?;
    let started = Instant::now();
    let mut engine = Engine::new(Arc::clone(&package), EngineLimits::default(), quota.clone())?;
    engine.install_bootstrap(PROBE_BOOTSTRAP)?;
    // Probe-local native activation; external rights remain ungranted.
    engine.bind_service_authority(
        package.digest(),
        ServiceAuthority {
            instance_id: 1,
            plan_generation: 1,
            authorization_epoch: 1,
        },
    )?;
    engine.load()?;
    let environment = serde_json::json!({"viewport":{"cell_width":width,"cell_height":height,"dot_width":width*2,"dot_height":height*4,"revision":1},"available":{"pointer":false,"audio":false,"gpu":false,"location":false}});
    let plan = engine.plan(&settings, mode, &environment)?;
    if plan.get("mode_unavailable_reason").is_some() {
        return Err(fail("selected settings do not support requested mode"));
    }
    if plan
        .pointer("/output/format")
        .and_then(serde_json::Value::as_str)
        != Some("gray32")
    {
        return Err(fail(
            "probe currently expects gray32 output; select corresponding package plan",
        ));
    }
    let mut accepted = plan.clone();
    if let Some(object) = accepted.as_object_mut() {
        object.insert("permissions".into(), serde_json::json!([]));
    }
    let mut creation = engine.start_create(&settings, &accepted)?;
    while creation == CreateState::Pending {
        let requests = engine.take_requests()?;
        if requests.is_empty() {
            creation = engine.pump()?;
            if creation == CreateState::Pending {
                return Err(fail("probe has no external acquisition provider"));
            }
        } else {
            for request in requests {
                engine.complete_request(request.id,&serde_json::json!({"ok":false,"error":{"code":"unavailable","message":"probe has no external acquisition provider"}}))?;
            }
            creation = engine.pump()?;
        }
    }
    println!(
        "{}",
        serde_json::json!({"type":"progress","phase":"prepared","package_id":package.manifest().id,"package_digest":package.digest(),"preparation_ms":started.elapsed().as_secs_f64()*1000.0,"plan":plan})
    );
    let arrays = [ArraySpec {
        name: "gray".into(),
        kind: TypedArrayKind::F32,
        elements: width * height * 8,
    }];
    let mut milliseconds = Vec::new();
    let fps = plan
        .get("fps")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(20.0)
        .clamp(1.0, 60.0);
    for index in 0..frames {
        let context = serde_json::json!({"viewport":environment["viewport"],"time":index as f64/fps,"delta":if index==0{0.0}else{1.0/fps},"wall":index as f64/fps,"wall_delta":if index==0{0.0}else{1.0/fps},"settings":settings,"visible":true,"inputs":{},"surface":{"revision":index,"reset":index==0,"invalid_rects":[]},"render_policy":{"can_skip_occluded":false}});
        let began = Instant::now();
        let (frame, cpu_profile) = engine.render_with_cpu_profile(&context, &arrays)?;
        let elapsed = began.elapsed().as_secs_f64() * 1000.0;
        milliseconds.push(elapsed);
        let pixels = frame
            .planes
            .get("gray")
            .ok_or_else(|| fail("gray plane missing"))?;
        let mut minimum = f32::INFINITY;
        let mut maximum = f32::NEG_INFINITY;
        let mut sum = 0.0_f64;
        for bytes in pixels.chunks_exact(4) {
            let value = f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(fail("nonfinite/out-of-range gray32 pixel"));
            }
            minimum = minimum.min(value);
            maximum = maximum.max(value);
            sum += value as f64;
        }
        println!(
            "{}",
            serde_json::json!({"type":"result","frame":index,"instrumented_render_ms":elapsed,"gray_min":minimum,"gray_max":maximum,"gray_sum":sum,"sha256":format!("{:x}",Sha256::digest(pixels)),"cpu_profile":cpu_profile})
        );
        if frame
            .metadata
            .get("submitted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
        {
            engine.accept_frame(true)?;
        }
    }
    let total: f64 = milliseconds.iter().sum();
    milliseconds.sort_by(f64::total_cmp);
    let median = milliseconds[milliseconds.len() / 2];
    let maximum = *milliseconds
        .last()
        .ok_or_else(|| fail("no frame measurements"))?;
    engine.dispose()?;
    drop(engine);
    let snapshot = quota.snapshot();
    println!(
        "{}",
        serde_json::json!({"type":"summary","frames":frames,"width_cells":width,"height_cells":height,"mean_instrumented_render_ms":total/frames as f64,"median_instrumented_render_ms":median,"max_instrumented_render_ms":maximum,"timing_valid_for_performance_comparison":false,"retained_worker_threads":snapshot.worker_threads,"retained_worker_bytes":snapshot.worker_bytes,"status":"done"})
    );
    Ok(())
}
#[cfg(feature = "v8-runtime")]
const PROBE_BOOTSTRAP: &str = r#"
'use strict';
globalThis.__ilium_host=Object.freeze({permissions:Object.freeze({has:()=>false,get:()=>undefined}),status:Object.freeze({log(){},progress(){}})});
globalThis.__ilium_make_frame=context=>{const viewport=context.viewport;const gray=new Float32Array(viewport.dot_width*viewport.dot_height);return {cell_width:viewport.cell_width,cell_height:viewport.cell_height,dot_width:viewport.dot_width,dot_height:viewport.dot_height,viewport_revision:viewport.revision,gray,pixels:{data:gray},present(){if(this.submitted)throw Error('duplicate present');this.submitted=true;}};};
globalThis.__ilium_finish_frame=frame=>({metadata:{submitted:!!frame.submitted},planes:{gray:frame.gray}});
globalThis.__ilium_accept_frame=()=>{};
"#;
