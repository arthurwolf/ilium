//! `ilium plugin build` (reference-plugin-cli.md DEV-050..055).
//!
//! The command detects the project language, checks the toolchain, runs the builder stages per
//! declared side and writes `dist/<side>.wasm` plus `dist/build.json`. `--watch` rebuilds after
//! debounced changes taken from the injected event script and never contacts a session.
use super::{dev::Debouncer, dev::DEBOUNCE, offline, CliEnv, CliOutput, DevEvent};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SIDES: [&str; 3] = ["server", "client", "scene"];

struct Options {
    project: PathBuf,
    side: Option<String>,
    debug: bool,
    watch: bool,
    locked: bool,
    offline: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    TypeScript,
}

struct Failure {
    code: &'static str,
    message: String,
    exit: i32,
}

fn fail(code: &'static str, message: impl Into<String>, exit: i32) -> Failure {
    Failure {
        code,
        message: message.into(),
        exit,
    }
}

fn parse(args: &[String], env: &CliEnv) -> Result<Options, CliOutput> {
    let usage = |code: &str, message: &str| offline::failure(code, message, 2);
    let mut project = None;
    let (mut side, mut debug, mut release) = (None, false, false);
    let (mut watch, mut locked, mut offline_flag) = (false, false, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--debug" => debug = true,
            "--release" => release = true,
            "--watch" => watch = true,
            "--locked" => locked = true,
            "--offline" => offline_flag = true,
            "--side" => match iter.next() {
                Some(value) => side = Some(value.clone()),
                None => return Err(usage("usage.missing-argument", "--side needs a value")),
            },
            flag if flag.starts_with('-') => {
                return Err(usage("usage.unknown-flag", "Unsupported flag"));
            }
            _ if project.is_none() => project = Some(arg.clone()),
            _ => return Err(usage("usage.argument-invalid", "Unexpected argument")),
        }
    }
    if debug && release {
        return Err(usage(
            "usage.conflict",
            "--debug and --release cannot be combined",
        ));
    }
    if let Some(side) = &side {
        if !SIDES.contains(&side.as_str()) {
            return Err(usage("usage.invalid-value", "Unknown side"));
        }
    }
    let project = project.map_or_else(|| env.cwd.clone(), |path| env.cwd.join(path));
    let offline_env = env
        .env
        .get("ILIUM_PLUGIN_OFFLINE")
        .is_some_and(|value| !value.is_empty() && value != "0");
    Ok(Options {
        project,
        side,
        debug,
        watch,
        locked,
        offline: offline_flag || offline_env,
    })
}

fn detect_language(project: &Path) -> Result<Language, Failure> {
    match (
        project.join("Cargo.toml").is_file(),
        project.join("package.json").is_file(),
    ) {
        (true, false) => Ok(Language::Rust),
        (false, true) => Ok(Language::TypeScript),
        (true, true) => Err(fail(
            "toolchain.language-ambiguous",
            "both Cargo.toml and package.json exist",
            4,
        )),
        (false, false) => Err(fail(
            "toolchain.language-unknown",
            "neither Cargo.toml nor package.json exists",
            4,
        )),
    }
}

fn declared_sides(project: &Path) -> Result<Vec<String>, Failure> {
    let text = std::fs::read_to_string(project.join("manifest.json"))
        .map_err(|error| fail("manifest.file-unreadable", error.to_string(), 1))?;
    let manifest: Value = serde_json::from_str(&text)
        .map_err(|error| fail("manifest.schema", error.to_string(), 1))?;
    Ok(SIDES
        .iter()
        .filter(|side| manifest.get(**side).is_some_and(|value| !value.is_null()))
        .map(|side| (*side).to_owned())
        .collect())
}

fn check_toolchain(language: Language, env: &CliEnv) -> Result<Vec<(String, String)>, Failure> {
    let Some(probe) = env.probe.as_ref() else {
        return Ok(Vec::new());
    };
    match language {
        Language::Rust => {
            let version = probe
                .tool_version("rustc")
                .ok_or_else(|| fail("toolchain.rustc-missing", "rustc was not found", 4))?;
            if !probe.rust_target_installed("wasm32-wasip2") {
                return Err(fail(
                    "toolchain.rust-target",
                    "the wasm32-wasip2 target is not installed; run rustup target add wasm32-wasip2",
                    4,
                ));
            }
            Ok(vec![("rustc".into(), version)])
        }
        Language::TypeScript => {
            let version = probe
                .tool_version("bun")
                .ok_or_else(|| fail("toolchain.bun-missing", "bun was not found", 4))?;
            Ok(vec![("bun".into(), version)])
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One full build of the selected sides. Returns the records it produced.
fn build_once(
    options: &Options,
    language: Language,
    sides: &[String],
    toolchain: &[(String, String)],
    env: &CliEnv,
) -> Result<Vec<Value>, Failure> {
    let builder = env
        .builder
        .as_ref()
        .ok_or_else(|| fail("command.context-required", "no build runner is connected", 1))?;
    let stages: [(&str, &str); 2] = match language {
        Language::Rust => [("compile", "build.compile-failed"), ("component", "build.component-failed")],
        Language::TypeScript => [("bundle", "build.bundle-failed"), ("component", "build.component-failed")],
    };
    let dist = options.project.join("dist");
    std::fs::create_dir_all(&dist)
        .map_err(|error| fail("build.write-failed", error.to_string(), 1))?;
    let mut records = Vec::new();
    let mut entries = Vec::new();
    for side in sides {
        let mut component = Vec::new();
        for (stage, code) in stages {
            records.push(json!({"type":"progress","step":stage,"message":format!("{stage} {side}")}));
            component = builder
                .run_stage(&options.project, stage, side)
                .map_err(|message| fail(code, message, 1))?;
        }
        records.push(json!({"type":"progress","step":"api-version","message":format!("checking the plugin API version of {side}")}));
        let name = format!("{side}.wasm");
        std::fs::write(dist.join(&name), &component)
            .map_err(|error| fail("build.write-failed", error.to_string(), 1))?;
        let sha256 = hex(&Sha256::digest(&component));
        records.push(json!({"type":"artifact","kind":"component",
            "path":format!("dist/{name}"),"bytes":component.len(),"sha256":sha256}));
        if language == Language::TypeScript && options.debug {
            let map = format!("{name}.map");
            let body = json!({"version":3,"file":name,"sources":["src/index.ts"],"mappings":""})
                .to_string();
            std::fs::write(dist.join(&map), &body)
                .map_err(|error| fail("build.write-failed", error.to_string(), 1))?;
            records.push(json!({"type":"artifact","kind":"source-map",
                "path":format!("dist/{map}"),"bytes":body.len(),
                "sha256":hex(&Sha256::digest(body.as_bytes()))}));
        }
        entries.push(json!({"name":side,"bytes":component.len(),"sha256":sha256}));
    }
    let input_hash = super::project_inputs::project_input_hash(&options.project)
        .map_err(|message| fail("build.input-unreadable", message, 1))?;
    let toolchain: serde_json::Map<String, Value> = toolchain
        .iter()
        .map(|(name, version)| (name.clone(), Value::String(version.clone())))
        .collect();
    let info = json!({
        "profile": if options.debug { "debug" } else { "release" },
        "api": "0.1.0",
        "input_hash": input_hash,
        "sides": entries,
        "toolchain": toolchain,
        "cached": false,
    });
    let mut text = serde_json::to_string_pretty(&info).unwrap_or_default();
    text.push('\n');
    std::fs::write(dist.join("build.json"), &text)
        .map_err(|error| fail("build.write-failed", error.to_string(), 1))?;
    records.push(json!({"type":"artifact","kind":"build-info","path":"dist/build.json",
        "bytes":text.len(),"sha256":hex(&Sha256::digest(text.as_bytes()))}));
    Ok(records)
}

/// One build of every declared side for `ilium plugin dev`: the records it produced, or the
/// failure code and message (the compiler text for a failed stage).
pub(super) fn build_for_dev(
    project: &Path,
    release: bool,
    env: &CliEnv,
) -> Result<Vec<Value>, (&'static str, String)> {
    let options = Options {
        project: project.to_path_buf(),
        side: None,
        debug: !release,
        watch: false,
        locked: false,
        offline: false,
    };
    let run = || -> Result<Vec<Value>, Failure> {
        let language = detect_language(project)?;
        let sides = declared_sides(project)?;
        let toolchain = check_toolchain(language, env)?;
        build_once(&options, language, &sides, &toolchain, env)
    };
    run().map_err(|failure| (failure.code, failure.message))
}

fn error_record(failure: &Failure) -> Value {
    json!({"type":"error","code":failure.code,"message":failure.message})
}

/// Runs `ilium plugin build`.
pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let options = match parse(args, env) {
        Ok(options) => options,
        Err(output) => return output,
    };
    match execute(&options, env) {
        Ok(output) => output,
        Err(failure) => offline::failure(failure.code, failure.message, failure.exit),
    }
}

fn execute(options: &Options, env: &CliEnv) -> Result<CliOutput, Failure> {
    let language = detect_language(&options.project)?;
    let declared = declared_sides(&options.project)?;
    let sides: Vec<String> = match &options.side {
        Some(side) if declared.contains(side) => vec![side.clone()],
        Some(side) => {
            return Err(fail(
                "build.side-undeclared",
                format!("the manifest does not declare the {side} side"),
                1,
            ))
        }
        None => declared,
    };
    let toolchain = check_toolchain(language, env)?;
    if options.locked {
        let lockfile = match language {
            Language::Rust => "Cargo.lock",
            Language::TypeScript => "bun.lock",
        };
        if !options.project.join(lockfile).is_file() {
            return Err(fail(
                "build.lockfile-missing",
                format!("{lockfile} is missing"),
                1,
            ));
        }
        if env
            .builder
            .as_ref()
            .is_some_and(|builder| builder.lockfile_would_change(&options.project))
        {
            return Err(fail(
                "build.lockfile-outdated",
                format!("building would change {lockfile}"),
                1,
            ));
        }
    }
    if options.offline {
        if let Some(name) = env
            .builder
            .as_ref()
            .and_then(|builder| builder.missing_offline_dependency(&options.project))
        {
            return Err(fail(
                "build.offline-missing",
                format!("dependency {name} is not in the offline cache"),
                1,
            ));
        }
    }
    let mut records = Vec::new();
    let first = build_once(options, language, &sides, &toolchain, env);
    if !options.watch {
        records.extend(first?);
        return Ok(offline::finish(records, 0));
    }
    match first {
        Ok(built) => records.extend(built),
        Err(failure) => records.push(error_record(&failure)),
    }
    let mut script: Vec<&DevEvent> = env.dev_script.iter().collect();
    script.sort_by_key(|event| event_time(event));
    let mut debouncer = Debouncer::new(Duration::from_millis(DEBOUNCE.as_millis() as u64));
    for event in script {
        let at = event_time(event);
        if debouncer.due(at) > 0 {
            match build_once(options, language, &sides, &toolchain, env) {
                Ok(built) => records.extend(built),
                Err(failure) => records.push(error_record(&failure)),
            }
        }
        match event {
            DevEvent::Change { .. } | DevEvent::WriteFile { .. } => debouncer.change(at),
            DevEvent::Interrupt { .. } => break,
            _ => {}
        }
    }
    Ok(offline::finish(records, 130))
}

fn event_time(event: &DevEvent) -> u64 {
    match event {
        DevEvent::Change { at_ms, .. }
        | DevEvent::WriteFile { at_ms, .. }
        | DevEvent::Log { at_ms, .. }
        | DevEvent::Calls { at_ms, .. }
        | DevEvent::Interrupt { at_ms } => *at_ms,
    }
}
