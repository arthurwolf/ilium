//! Builds the core plugins to WebAssembly components and writes the catalogue of this crate.
//!
//! One nested `cargo build --release --target wasm32-wasip2` per side (server, client) over
//! every crate under `plugins/core/` whose `manifest.json` has that side, with the crate
//! feature `component-<side>`. Both runs share one target directory outside this build's own,
//! so the SDK and its dependencies are compiled once and cargo's fingerprints make a rerun
//! without changes a freshness check. The script itself reruns only when `plugins/`, `wit/`,
//! an SDK crate, `Cargo.lock` or one of its environment variables changes.
//!
//! The nested cargo uses the workspace `Cargo.lock` (with `--locked` unless the settings say
//! otherwise) and needs nothing the outer build did not already need, except the
//! `wasm32-wasip2` standard library (pinned in `rust-toolchain.toml`) and the wasm32-only
//! crates of the lock file in the registry cache.
//!
//! Settings come from `bundle-build.conf` beside this script (keys `mode`, `locked`,
//! `offline`, `target-dir`, described in that file), so a build that cannot pass environment
//! variables is still configurable from the source tree. An environment variable, when set
//! and not empty, overrides its key:
//!   ILIUM_PLUGIN_BUNDLE_MODE        skip | lenient | strict (see `Mode`).
//!   ILIUM_PLUGIN_BUNDLE_LOCKED      0 drops --locked from the nested cargo.
//!   ILIUM_PLUGIN_BUNDLE_TARGET_DIR  target directory of the nested builds.
//!   ILIUM_PLUGIN_BUNDLE_OFFLINE     1 passes --offline to the nested cargo (also implied by
//!                                   CARGO_NET_OFFLINE=true).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// `ilium:api-version` of the components: major, minor, patch.
const API_VERSION: [u16; 3] = [0, 1, 0];
const API_VERSION_SECTION: &str = "ilium:api-version";
const WASM_TARGET: &str = "wasm32-wasip2";
/// Sides built as components, in build order.
const BUILT_SIDES: &[&str] = &["server", "client"];
/// Sides a manifest can declare.
const ALL_SIDES: &[&str] = &["server", "client", "scene"];
/// Sides without component glue in the SDK yet; their components are reported as pending,
/// never as missing.
const PENDING_SIDES: &[&str] = &["scene"];
/// The settings file, beside this script.
const SETTINGS_FILE: &str = "bundle-build.conf";
/// The keys of the settings file.
const SETTINGS_KEYS: &[&str] = &["mode", "locked", "offline", "target-dir"];
/// The mode when neither the settings file nor ILIUM_PLUGIN_BUNDLE_MODE names one: an
/// ordinary workspace build does not pay for the wasm build.
const DEFAULT_MODE: &str = "skip";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Build nothing; the catalogue lists the plugins and their manifests without components.
    Skip,
    /// Build what compiles; a component that does not is listed in `MISSING`.
    Lenient,
    /// Build everything; a component that does not compile fails this build.
    Strict,
}

/// What the settings file and the environment ask of the nested builds.
struct Settings {
    mode: Mode,
    /// Pass `--locked`: the nested cargo refuses to change `Cargo.lock`.
    locked: bool,
    /// Pass `--offline`.
    offline: bool,
    /// The target directory override, when one is named.
    target_directory: Option<PathBuf>,
}

struct Plugin {
    directory: String,
    package: String,
    id: String,
    manifest_path: PathBuf,
    sides: Vec<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ilium-plugin-bundle: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let manifest_directory = PathBuf::from(required_env("CARGO_MANIFEST_DIR")?);
    let out_directory = PathBuf::from(required_env("OUT_DIR")?);
    let root = manifest_directory
        .parent()
        .ok_or("the crate directory has no parent")?
        .to_path_buf();
    let settings_path = manifest_directory.join(SETTINGS_FILE);
    println!("cargo:rerun-if-changed={}", settings_path.display());
    let settings = settings(&settings_path, &root)?;
    let mode = settings.mode;

    for tracked in [
        "plugins",
        "wit",
        "ilium-plugin-sdk",
        "ilium-plugin-sdk-core",
        "ilium-plugin-sdk-macros",
        "Cargo.lock",
        "rust-toolchain.toml",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(tracked).display());
    }
    for variable in [
        "ILIUM_PLUGIN_BUNDLE_MODE",
        "ILIUM_PLUGIN_BUNDLE_LOCKED",
        "ILIUM_PLUGIN_BUNDLE_TARGET_DIR",
        "ILIUM_PLUGIN_BUNDLE_OFFLINE",
        "CARGO_NET_OFFLINE",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }

    let plugins = discover(&root.join("plugins").join("core"))?;
    let mut components: BTreeMap<(String, String), PathBuf> = BTreeMap::new();
    if mode != Mode::Skip {
        let target_directory = match &settings.target_directory {
            Some(directory) => directory.clone(),
            None => nested_target_directory(&out_directory)?,
        };
        for side in BUILT_SIDES {
            let built = build_side(&root, &target_directory, &plugins, side, &settings)?;
            for (package, artifact) in built {
                let Some(plugin) = plugins.iter().find(|plugin| plugin.package == package) else {
                    continue;
                };
                let stamped = out_directory.join(format!("{}.{side}.wasm", plugin.directory));
                stamp(&artifact, &stamped)?;
                components.insert((plugin.directory.clone(), (*side).to_owned()), stamped);
            }
        }
    }

    let mut missing = Vec::new();
    let mut pending = Vec::new();
    for plugin in &plugins {
        for side in &plugin.sides {
            if components.contains_key(&(plugin.directory.clone(), side.clone())) {
                continue;
            }
            if PENDING_SIDES.contains(&side.as_str()) {
                pending.push((plugin.directory.clone(), side.clone()));
            } else {
                missing.push((plugin.directory.clone(), side.clone()));
            }
        }
    }
    if mode == Mode::Strict && !missing.is_empty() {
        return Err(format!(
            "{} component(s) were not built: {}",
            missing.len(),
            describe(&missing)
        ));
    }
    if mode == Mode::Lenient && !missing.is_empty() {
        println!(
            "cargo:warning=ilium-plugin-bundle: {} component(s) were not built: {}",
            missing.len(),
            describe(&missing)
        );
    }

    let catalogue = catalogue(mode, &plugins, &components, &missing, &pending);
    let catalogue_path = out_directory.join("catalogue.rs");
    std::fs::write(&catalogue_path, catalogue)
        .map_err(|error| format!("cannot write {}: {error}", catalogue_path.display()))
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set"))
}

/// The `key = value` lines of the settings file; a missing file is an empty map.
fn settings_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let mut values = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "{}:{}: expected `key = value`",
                path.display(),
                index + 1
            ));
        };
        let key = key.trim();
        if !SETTINGS_KEYS.contains(&key) {
            return Err(format!(
                "{}:{}: unknown key `{key}`",
                path.display(),
                index + 1
            ));
        }
        values.insert(key.to_owned(), value.trim().to_owned());
    }
    Ok(values)
}

/// The settings: each environment variable when set and not empty, else its key of the
/// settings file, else the default.
fn settings(path: &Path, root: &Path) -> Result<Settings, String> {
    let file = settings_file(path)?;
    let pick = |variable: &str, key: &str| -> Option<(String, String)> {
        match std::env::var(variable) {
            Ok(value) if !value.is_empty() => Some((variable.to_owned(), value)),
            _ => file
                .get(key)
                .filter(|value| !value.is_empty())
                .map(|value| (format!("`{key}` in {SETTINGS_FILE}"), value.clone())),
        }
    };
    let boolean = |variable: &str, key: &str, default: bool| -> Result<bool, String> {
        match pick(variable, key) {
            None => Ok(default),
            Some((origin, value)) => match value.as_str() {
                "1" | "true" | "yes" => Ok(true),
                "0" | "false" | "no" => Ok(false),
                other => Err(format!("{origin} must be true or false, not `{other}`")),
            },
        }
    };
    let mode = match pick("ILIUM_PLUGIN_BUNDLE_MODE", "mode") {
        None => DEFAULT_MODE.to_owned(),
        Some((_, value)) => value,
    };
    let mode = match mode.as_str() {
        "skip" => Mode::Skip,
        "lenient" => Mode::Lenient,
        "strict" => Mode::Strict,
        other => {
            return Err(format!(
                "the bundle mode must be skip, lenient or strict, not `{other}`"
            ))
        }
    };
    let cargo_offline =
        std::env::var("CARGO_NET_OFFLINE").is_ok_and(|value| value == "true" || value == "1");
    let target_directory =
        pick("ILIUM_PLUGIN_BUNDLE_TARGET_DIR", "target-dir").map(|(_, value)| {
            let directory = PathBuf::from(value);
            if directory.is_absolute() {
                directory
            } else {
                root.join(directory)
            }
        });
    Ok(Settings {
        mode,
        locked: boolean("ILIUM_PLUGIN_BUNDLE_LOCKED", "locked", true)?,
        offline: boolean("ILIUM_PLUGIN_BUNDLE_OFFLINE", "offline", false)? || cargo_offline,
        target_directory,
    })
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Skip => "skip",
        Mode::Lenient => "lenient",
        Mode::Strict => "strict",
    }
}

fn describe(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(directory, side)| format!("{directory}/{side}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every crate under `plugins/core/`, in directory order.
fn discover(core: &Path) -> Result<Vec<Plugin>, String> {
    let mut directories = Vec::new();
    let entries = std::fs::read_dir(core)
        .map_err(|error| format!("cannot list {}: {error}", core.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot list {}: {error}", core.display()))?;
        if entry.path().join("Cargo.toml").is_file() {
            directories.push(entry.path());
        }
    }
    directories.sort();
    let mut plugins = Vec::new();
    for directory in directories {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no UTF-8 name", directory.display()))?
            .to_owned();
        let cargo_toml = read(&directory.join("Cargo.toml"))?;
        let package = package_name(&cargo_toml)
            .ok_or_else(|| format!("plugins/core/{name}/Cargo.toml has no package name"))?;
        let manifest_path = directory.join("manifest.json");
        let manifest: serde_json::Value = serde_json::from_str(&read(&manifest_path)?)
            .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
        let id = manifest
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("{} has no id", manifest_path.display()))?
            .to_owned();
        let sides = ALL_SIDES
            .iter()
            .filter(|side| {
                manifest
                    .get(**side)
                    .is_some_and(serde_json::Value::is_object)
            })
            .map(|side| (*side).to_owned())
            .collect();
        plugins.push(Plugin {
            directory: name,
            package,
            id,
            manifest_path,
            sides,
        });
    }
    Ok(plugins)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// The `name` of the `[package]` table.
fn package_name(cargo_toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_owned());
        }
    }
    None
}

/// Where the nested builds keep their output when no directory is named: `plugin-wasm`
/// beside the profile directory of the outer build (never inside it, which the outer cargo
/// has locked).
fn nested_target_directory(out_directory: &Path) -> Result<PathBuf, String> {
    // OUT_DIR is <target>[/<triple>]/<profile>/build/<package>-<hash>/out.
    let profile_directory = out_directory
        .ancestors()
        .find(|ancestor| {
            ancestor
                .parent()
                .is_some_and(|parent| parent.join("build") == *ancestor)
        })
        .and_then(Path::parent)
        .ok_or_else(|| format!("unexpected OUT_DIR layout: {}", out_directory.display()))?;
    let beside = profile_directory
        .parent()
        .ok_or_else(|| format!("unexpected OUT_DIR layout: {}", out_directory.display()))?;
    Ok(beside.join("plugin-wasm"))
}

/// Builds one side of every plugin that has it. Returns (package name, component path) of
/// each component cargo reports, fresh or rebuilt; a plugin that failed is simply absent.
fn build_side(
    root: &Path,
    target_directory: &Path,
    plugins: &[Plugin],
    side: &str,
    settings: &Settings,
) -> Result<Vec<(String, PathBuf)>, String> {
    let mode = settings.mode;
    let selected: Vec<&Plugin> = plugins
        .iter()
        .filter(|plugin| plugin.sides.iter().any(|candidate| candidate == side))
        .collect();
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .arg("build")
        .arg("--release")
        .arg("--lib")
        .arg("--target")
        .arg(WASM_TARGET)
        .arg("--target-dir")
        .arg(target_directory)
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .arg("--message-format")
        .arg("json-render-diagnostics");
    if mode == Mode::Lenient {
        command.arg("--keep-going");
    }
    if settings.locked {
        command.arg("--locked");
    }
    if settings.offline {
        command.arg("--offline");
    }
    let mut features = Vec::new();
    for plugin in &selected {
        command.arg("-p").arg(&plugin.package);
        features.push(format!("{}/component-{side}", plugin.package));
    }
    command.arg("--features").arg(features.join(","));
    scrub_environment(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let output = command.output().map_err(|error| {
        format!("cannot start the nested cargo for the {side} components: {error}")
    })?;
    let mut built = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let Some(name) = message
            .get("target")
            .and_then(|target| target.get("name"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        // The target name of a library is the package name, with `-` or `_` by cargo version.
        let Some(plugin) = selected
            .iter()
            .find(|plugin| plugin.package == name || plugin.package.replace('-', "_") == name)
        else {
            continue;
        };
        let component = message
            .get("filenames")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .find(|file| file.ends_with(".wasm"));
        if let Some(component) = component {
            built.push((plugin.package.clone(), PathBuf::from(component)));
        }
    }
    if !output.status.success() && mode == Mode::Strict {
        return Err(format!(
            "the nested cargo build of the {side} components failed ({}); its diagnostics are above",
            output.status
        ));
    }
    Ok(built)
}

/// Removes what cargo set for this build script and would misdirect a nested build for
/// another target: flags of the host target, this package's identity, the outer target
/// directory. Compiler wrappers, the toolchain, the jobserver and network settings stay.
fn scrub_environment(command: &mut Command) {
    const EXACT: &[&str] = &[
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_MANIFEST_LINKS",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "OUT_DIR",
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "NUM_JOBS",
        "RUSTC_LINKER",
    ];
    const PREFIXES: &[&str] = &["CARGO_PKG_", "CARGO_CFG_", "CARGO_FEATURE_", "DEP_"];
    for (name, _) in std::env::vars_os() {
        let Some(name) = name.to_str() else {
            continue;
        };
        if EXACT.contains(&name) || PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
            command.env_remove(name);
        }
    }
}

/// Copies a component and appends the top-level custom section `ilium:api-version`
/// (three big-endian u16), which the host requires of every component.
fn stamp(source: &Path, destination: &Path) -> Result<(), String> {
    let mut bytes = std::fs::read(source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let mut payload = Vec::new();
    leb128(&mut payload, API_VERSION_SECTION.len());
    payload.extend_from_slice(API_VERSION_SECTION.as_bytes());
    for part in API_VERSION {
        payload.extend_from_slice(&part.to_be_bytes());
    }
    bytes.push(0); // custom section id
    leb128(&mut bytes, payload.len());
    bytes.extend_from_slice(&payload);
    std::fs::write(destination, bytes)
        .map_err(|error| format!("cannot write {}: {error}", destination.display()))
}

fn leb128(output: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            output.push(byte);
            return;
        }
        output.push(byte | 0x80);
    }
}

fn catalogue(
    mode: Mode,
    plugins: &[Plugin],
    components: &BTreeMap<(String, String), PathBuf>,
    missing: &[(String, String)],
    pending: &[(String, String)],
) -> String {
    let mut text = String::from("// Written by build.rs of ilium-plugin-bundle.\n\n");
    // Writing to a String cannot fail; the results are ignored for that reason.
    let _ = writeln!(
        text,
        "/// How the build script ran: `skip`, `lenient` or `strict`."
    );
    let _ = writeln!(text, "pub const MODE: &str = {:?};", mode_name(mode));
    let _ = writeln!(
        text,
        "/// True when every component with SDK glue was built."
    );
    let _ = writeln!(
        text,
        "pub const COMPLETE: bool = {};",
        mode != Mode::Skip && missing.is_empty()
    );
    let _ = writeln!(text, "/// Components that should exist and were not built.");
    let _ = writeln!(text, "pub static MISSING: &[Absent] = &{};", pairs(missing));
    let _ = writeln!(
        text,
        "/// Components whose side has no component glue in the SDK yet."
    );
    let _ = writeln!(text, "pub static PENDING: &[Absent] = &{};", pairs(pending));
    let _ = writeln!(text, "/// Every core plugin, in directory order.");
    let _ = writeln!(text, "pub static PLUGINS: &[BundledPlugin] = &[");
    for plugin in plugins {
        let _ = writeln!(text, "    BundledPlugin {{");
        let _ = writeln!(text, "        id: {:?},", plugin.id);
        let _ = writeln!(text, "        directory: {:?},", plugin.directory);
        let _ = writeln!(
            text,
            "        manifest: include_str!({:?}),",
            plugin.manifest_path.display().to_string()
        );
        for side in ALL_SIDES {
            let value = match components.get(&(plugin.directory.clone(), (*side).to_owned())) {
                Some(path) => format!("Some(include_bytes!({:?}))", path.display().to_string()),
                None => "None".to_owned(),
            };
            let _ = writeln!(text, "        {side}: {value},");
        }
        let _ = writeln!(text, "    }},");
    }
    let _ = writeln!(text, "];");
    text
}

fn pairs(entries: &[(String, String)]) -> String {
    let mut text = String::from("[");
    for (directory, side) in entries {
        let _ = write!(text, "({directory:?}, {side:?}), ");
    }
    text.push(']');
    text
}
