//! Actual packaged-animation/helper check. JSONL stdout; no network acquisition.
//! Synthetic pointer, clock and TV inputs verify transport/rendering only.
#[cfg(feature = "v8-runtime")]
fn main() {
    if let Err(error) = check::run() {
        println!(
            "{}",
            serde_json::json!({"type":"error","message":error.to_string()})
        );
        std::process::exit(1);
    }
}
#[cfg(not(feature = "v8-runtime"))]
fn main() {
    println!(
        "{}",
        serde_json::json!({"type":"error","message":"v8-runtime feature required"})
    );
    std::process::exit(1);
}
#[cfg(feature = "v8-runtime")]
mod check {
    use ilium_animation_js::{
        engine::{ArraySpec, CreateState, ServiceAuthority, TypedArrayKind},
        helper::{HelperAuthority, HelperLimits, HelperPlayback, HelperSession},
        manifest::AnimationMode,
        package::{Package, PackageLimits},
        settings::validate_settings,
        TRUSTED_BOOTSTRAP,
    };
    use ilium_execution::{QuotaGroup, QuotaLimits};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, error::Error, io::Read, path::Path};
    type Result<T> = std::result::Result<T, Box<dyn Error>>;
    const WIDTH: usize = 48;
    const HEIGHT: usize = 16;
    const DOTS: usize = WIDTH * HEIGHT * 8;
    fn fail(message: &str) -> Box<dyn Error> {
        std::io::Error::other(message.to_owned()).into()
    }
    fn load(path: &str, expected: &str, id: &str) -> Result<(Vec<u8>, Package)> {
        if !Path::new(path).is_absolute()
            || expected.len() != 64
            || !expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(fail("archive needs an absolute path and lowercase SHA256"));
        }
        let limits = PackageLimits::default();
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > limits.archive_bytes {
            return Err(fail("archive limit"));
        }
        let mut bytes = Vec::new();
        file.take(limits.archive_bytes + 1)
            .read_to_end(&mut bytes)?;
        if format!("{:x}", Sha256::digest(&bytes)) != expected {
            return Err(fail("archive SHA256 differs"));
        }
        let package = Package::from_bytes(&bytes, limits)?;
        if package.manifest().id != id {
            return Err(fail("unexpected package identity"));
        }
        Ok((bytes, package))
    }
    pub fn run() -> Result<()> {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        let flags = [
            "--helper",
            "--beach-package",
            "--beach-sha256",
            "--carpet-package",
            "--carpet-sha256",
        ];
        if arguments == ["--help"] {
            println!(
                "{}",
                json!({"type":"result","required_flags":flags,"dimensions_cells":[WIDTH,HEIGHT],"frames_per_choice":3,"scope":"real packaged helper transport; synthetic inputs; no permission acquisition, native parity or terminal publication"})
            );
            return Ok(());
        }
        if arguments.len() != flags.len() * 2 {
            return Err(fail("all five flags required exactly once"));
        }
        let mut options = BTreeMap::new();
        for pair in arguments.chunks_exact(2) {
            if !flags.contains(&pair[0].as_str())
                || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
            {
                return Err(fail("unknown or duplicate flag"));
            }
        }
        let get = |name| {
            options
                .get(name)
                .copied()
                .ok_or_else(|| fail("missing flag"))
        };
        let executable = Path::new(get("--helper")?);
        if !executable.is_absolute() {
            return Err(fail("helper path must be absolute"));
        }
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 32,
            worker_bytes: 1024 * 1024 * 1024,
        });
        let package_limits = PackageLimits::default();
        let _storage = quota
            .reserve_external_storage(
                2 * (package_limits.archive_bytes + package_limits.expanded_bytes) as usize
                    + 32 * 1024 * 1024,
            )
            .map_err(|reason| fail(&format!("package/frame admission: {reason:?}")))?;
        let beach = load(get("--beach-package")?, get("--beach-sha256")?, "beach")?;
        let carpet = load(get("--carpet-package")?, get("--carpet-sha256")?, "carpet")?;
        println!(
            "{}",
            json!({"type":"manifest","helper":executable,"beach_digest":beach.1.digest(),"carpet_digest":carpet.1.digest(),"synthetic_inputs":true,"scope":"actual RAM packages and protected helper; not grants, live TV, native parity or terminal credit"})
        );
        let mut choices = vec![
            ("beach-classic".to_owned(), true, json!({"style":"classic"})),
            ("beach-rich".to_owned(), true, json!({"style":"rich"})),
        ];
        for mode in 0..=8 {
            choices.push((format!("carpet-{mode}"), false, json!({"mode":mode})));
        }
        let mut passed = 0;
        let mut refused = 0;
        for (choice, is_beach, authored) in choices {
            let (archive, package) = if is_beach { &beach } else { &carpet };
            let settings = validate_settings(&package.manifest().settings, &authored)?;
            for mode in [AnimationMode::Live, AnimationMode::PreRendered] {
                let instance_id = passed + refused + 1;
                let mut helper = HelperSession::launch(
                    executable,
                    archive,
                    TRUSTED_BOOTSTRAP,
                    HelperAuthority {
                        package_digest: package.digest().into(),
                        instance_id,
                        plan_generation: 1,
                        authorization_epoch: 1,
                    },
                    HelperLimits::default(),
                    quota.clone(),
                    HelperPlayback {
                        mode: mode.clone(),
                        ambient_seed: 0,
                    },
                )?;
                helper.bind_service_authority(ServiceAuthority {
                    instance_id,
                    plan_generation: 1,
                    authorization_epoch: 1,
                })?;
                let viewport = json!({"cell_width":WIDTH,"cell_height":HEIGHT,"dot_width":WIDTH*2,"dot_height":HEIGHT*4,"revision":1});
                let environment = json!({"viewport":viewport,"available":{"pointer":true,"audio":false,"gpu":false,"location":false}});
                let plan = helper.plan(&settings, mode.clone(), &environment)?;
                let expected_refusal = !is_beach
                    && mode == AnimationMode::PreRendered
                    && [0, 4, 7, 8].contains(
                        &settings["mode"]
                            .as_i64()
                            .ok_or_else(|| fail("missing normalized mode"))?,
                    );
                if expected_refusal {
                    if plan["mode_unavailable_reason"]
                        .as_str()
                        .is_none_or(|reason| reason.is_empty())
                    {
                        return Err(fail("live input mode failed to refuse pre-rendering"));
                    }
                    helper.dispose()?;
                    refused += 1;
                    println!(
                        "{}",
                        json!({"type":"result","choice":choice,"mode":mode,"status":"expected_refusal"})
                    );
                    continue;
                }
                if plan["output"] != json!({"mode":"pixels","format":"gray32","update":"replace"}) {
                    return Err(fail("unexpected output contract"));
                }
                let mut creation = helper.start_create(&settings, &plan)?;
                for _ in 0..8 {
                    if !helper.take_requests().is_empty() {
                        return Err(fail("unexpected external acquisition"));
                    }
                    if creation == CreateState::Ready {
                        break;
                    }
                    creation = helper.pump()?;
                }
                if creation != CreateState::Ready {
                    return Err(fail("package creation did not finish"));
                }
                let shape = json!({"cell_width":WIDTH,"cell_height":HEIGHT,"mode":"pixels","format":"gray32","update":"replace","cell_rgb":false,"colour_space":"srgb"});
                let seeds = [ArraySpec {
                    name: "work_data".into(),
                    kind: TypedArrayKind::F32,
                    elements: DOTS,
                }];
                let output = [
                    ("work_data", TypedArrayKind::F32),
                    ("data", TypedArrayKind::F32),
                    ("work_touch", TypedArrayKind::U8),
                    ("touch", TypedArrayKind::U8),
                    ("work_order", TypedArrayKind::U32),
                    ("order", TypedArrayKind::U32),
                ]
                .map(|(name, kind)| ArraySpec {
                    name: name.into(),
                    kind,
                    elements: DOTS,
                });
                let planes = BTreeMap::from([("work_data".into(), vec![0; DOTS * 4])]);
                let mut hash = Sha256::new();
                let mut has_ink = false;
                for sequence in 1..=3_u64 {
                    let key = json!({"instance_id":instance_id.to_string(),"revision":"1","base_version":(sequence-1).to_string(),"sequence":sequence.to_string()});
                    helper.seed_frame(&json!({"frame":{"key":key,"shape":shape,"reset":true,"invalid_rects":[],"input_specs":[]}}),&seeds,&planes)?;
                    let seconds = (sequence - 1) as f64 / 20.0;
                    let inputs = if is_beach {
                        json!({})
                    } else {
                        match settings["mode"].as_i64() {
                            Some(0) => json!({"pointer":{"x":0.5,"y":0.5,"inside":true}}),
                            Some(4) => {
                                json!({"chess":{"available":true,"revision":1,"game_id":"synthetic-helper-fixture","fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","moves":[],"white":"synthetic fixture","black":"synthetic fixture","state":"playing"}})
                            }
                            Some(7 | 8) => {
                                json!({"clock":{"epoch_ms":1_800_000_000_000_u64+50*(sequence-1),"timezone":"UTC"}})
                            }
                            _ => json!({}),
                        }
                    };
                    let render_started = std::time::Instant::now();
                    let frame = helper.render(&json!({"viewport":viewport,"time":seconds,"wall":seconds,"delta":if sequence==1{0.0}else{0.05},"wall_delta":if sequence==1{0.0}else{0.05},"settings":settings,"visible":true,"inputs":inputs,"render_policy":{"can_skip_occluded":false},"_ilium_frame":{"key":key,"shape":shape}}),&output).map_err(|error| { eprintln!("{}",json!({"type":"error","choice":choice,"mode":mode,"sequence":sequence,"render_wall_ms":render_started.elapsed().as_secs_f64()*1000.0,"message":error.to_string()})); error })?;
                    if frame.metadata["presented"] != true || frame.metadata["error"] != Value::Null
                    {
                        return Err(fail("packaged frame was not presented successfully"));
                    }
                    let data = frame
                        .planes
                        .get("data")
                        .ok_or_else(|| fail("missing gray32 plane"))?;
                    if data.len() != DOTS * 4 {
                        return Err(fail("gray32 byte shape"));
                    }
                    for bytes in data.chunks_exact(4) {
                        let value = f32::from_ne_bytes(bytes.try_into()?);
                        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                            return Err(fail("invalid gray32 sample"));
                        }
                        has_ink |= value > 0.0;
                    }
                    let touch = frame
                        .planes
                        .get("touch")
                        .ok_or_else(|| fail("missing touch plane"))?;
                    if touch.len() != DOTS || touch.iter().any(|value| *value != 1) {
                        return Err(fail("replace frame did not touch every sample"));
                    }
                    hash.update(data);
                    if !helper.take_requests().is_empty() {
                        return Err(fail("render attempted external acquisition"));
                    }
                    helper.accept_frame(true)?;
                }
                helper.dispose()?;
                if !has_ink {
                    return Err(fail("animation rendered only blank frames"));
                }
                passed += 1;
                println!(
                    "{}",
                    json!({"type":"result","choice":choice,"mode":mode,"status":"passed","frames":3,"data_sha256":format!("{:x}",hash.finalize()),"synthetic_inputs":true})
                );
            }
        }
        if passed != 18 || refused != 4 {
            return Err(fail("incomplete choice/mode inventory"));
        }
        println!(
            "{}",
            json!({"type":"summary","rendered_choices":passed,"expected_refusals":refused,"frames":passed*3,"status":"passed","scope":"helper rendering only; no broker acquisition, fidelity, performance or terminal publication claim"})
        );
        Ok(())
    }
}
