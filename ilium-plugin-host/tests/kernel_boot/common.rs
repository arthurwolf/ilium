//! Shared helpers of the kernel-boot lane for the host crate: workspace and manual access,
//! the crate edge checker (HST-001..005) and a recording native guest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::PluginError;
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::NativeGuest;

/// The workspace root (the parent of this crate).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the host crate sits inside the workspace")
        .to_path_buf()
}

/// Reads a text file or fails the test with the path.
pub fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The text of one manual page (`docs/plugins-manual/pages/<name>.md`).
pub fn manual_page(name: &str) -> String {
    read_text(
        &workspace_root()
            .join("docs/plugins-manual/pages")
            .join(format!("{name}.md")),
    )
}

/// Every `*.rs` file under a directory, as (path relative to the directory, text).
pub fn rust_sources(dir: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                let relative = path
                    .strip_prefix(dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((relative, read_text(&path)));
            }
        }
    }
    found.sort();
    found
}

/// One workspace crate as the edge checker sees it: `[dependencies]` only.
#[derive(Debug, Clone)]
pub struct CrateManifest {
    pub dir: String,
    pub name: String,
    pub dependencies: BTreeSet<String>,
}

fn collect_dependency_names(table: Option<&toml::Value>, out: &mut BTreeSet<String>) {
    let Some(table) = table.and_then(|value| value.as_table()) else {
        return;
    };
    for (key, spec) in table {
        let real_name = spec
            .get("package")
            .and_then(|value| value.as_str())
            .unwrap_or(key.as_str());
        out.insert(real_name.to_owned());
    }
}

/// Parses the text of a `Cargo.toml`; `[dependencies]` and target-specific dependencies count,
/// `[dev-dependencies]` and `tests/` are exempt (test plan section 1.2).
pub fn parse_crate_manifest(dir: &str, text: &str) -> CrateManifest {
    let value: toml::Value = text.parse().expect("a valid Cargo.toml");
    let name = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
        .unwrap_or("")
        .to_owned();
    let mut dependencies = BTreeSet::new();
    collect_dependency_names(value.get("dependencies"), &mut dependencies);
    if let Some(targets) = value.get("target").and_then(|target| target.as_table()) {
        for (_, section) in targets {
            collect_dependency_names(section.get("dependencies"), &mut dependencies);
        }
    }
    CrateManifest {
        dir: dir.to_owned(),
        name,
        dependencies,
    }
}

/// Every member crate of the workspace, with `dir/*` member globs expanded.
pub fn workspace_crates() -> Vec<CrateManifest> {
    let root = workspace_root();
    let root_text = read_text(&root.join("Cargo.toml"));
    let root_value: toml::Value = root_text.parse().expect("a valid workspace Cargo.toml");
    let members = root_value
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(|members| members.as_array())
        .expect("workspace.members");
    let mut dirs: Vec<String> = Vec::new();
    for member in members {
        let pattern = member.as_str().expect("member is a string");
        if let Some(parent) = pattern.strip_suffix("/*") {
            let Ok(entries) = std::fs::read_dir(root.join(parent)) else {
                continue;
            };
            let mut children: Vec<String> = entries
                .flatten()
                .filter(|entry| entry.path().join("Cargo.toml").is_file())
                .map(|entry| format!("{parent}/{}", entry.file_name().to_string_lossy()))
                .collect();
            children.sort();
            dirs.extend(children);
        } else {
            dirs.push(pattern.to_owned());
        }
    }
    dirs.into_iter()
        .filter(|dir| root.join(dir).join("Cargo.toml").is_file())
        .map(|dir| {
            let text = read_text(&root.join(&dir).join("Cargo.toml"));
            parse_crate_manifest(&dir, &text)
        })
        .collect()
}

/// Wasmtime, V8, tokio and the other runtime and I/O crates `ilium-plugin-api` must not use.
pub const API_FORBIDDEN_CRATES: [&str; 14] = [
    "wasmtime",
    "wasmtime-wasi",
    "v8",
    "rusty_v8",
    "tokio",
    "async-std",
    "smol",
    "mio",
    "socket2",
    "reqwest",
    "hyper",
    "ureq",
    "tungstenite",
    "notify",
];

/// The crates of the SDK family (test plan section 1.2).
pub const SDK_FAMILY: [&str; 4] = [
    "ilium-plugin-sdk",
    "ilium-plugin-sdk-core",
    "ilium-plugin-sdk-macros",
    "ilium-plugin-testkit",
];

/// The `ilium-*` crates the host may depend on (HST edge table).
pub const HOST_ALLOWED_ILIUM: [&str; 4] = [
    "ilium-plugin-api",
    "ilium-execution",
    "ilium-platform",
    "ilium-logging",
];

/// Returns `"from -> to"` for every forbidden edge of the table in reference-host-architecture.md.
pub fn forbidden_edges(crates: &[CrateManifest]) -> Vec<String> {
    let mut found = Vec::new();
    for krate in crates {
        let mut bad: Vec<&String> = Vec::new();
        match krate.name.as_str() {
            "ilium-plugin-api" => {
                for dependency in &krate.dependencies {
                    if dependency.starts_with("ilium")
                        || API_FORBIDDEN_CRATES.contains(&dependency.as_str())
                    {
                        bad.push(dependency);
                    }
                }
            }
            "ilium-plugin-host" => {
                for dependency in &krate.dependencies {
                    if dependency.starts_with("ilium")
                        && !HOST_ALLOWED_ILIUM.contains(&dependency.as_str())
                    {
                        bad.push(dependency);
                    }
                }
            }
            "ilium-ipc" => {
                for dependency in &krate.dependencies {
                    if dependency == "ilium-plugin-host" || dependency == "wasmtime" {
                        bad.push(dependency);
                    }
                }
            }
            "ilium-core" | "ilium-detect" | "ilium-pty" => {
                for dependency in &krate.dependencies {
                    if dependency.starts_with("ilium-plugin") {
                        bad.push(dependency);
                    }
                }
            }
            name if SDK_FAMILY.contains(&name) => {
                for dependency in &krate.dependencies {
                    if dependency.starts_with("ilium") && !SDK_FAMILY.contains(&dependency.as_str())
                    {
                        bad.push(dependency);
                    }
                }
            }
            _ if krate.dir.starts_with("plugins/") => {
                for dependency in &krate.dependencies {
                    if dependency.starts_with("ilium") && dependency != "ilium-plugin-sdk" {
                        bad.push(dependency);
                    }
                }
            }
            _ => {}
        }
        for dependency in bad {
            found.push(format!("{} -> {}", krate.name, dependency));
        }
    }
    found
}

/// A synthetic crate for edge-checker tests.
pub fn synthetic_crate(dir: &str, name: &str, dependencies: &[&str]) -> CrateManifest {
    CrateManifest {
        dir: dir.to_owned(),
        name: name.to_owned(),
        dependencies: dependencies
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
    }
}

/// What a [`RecordingGuest`] saw.
#[derive(Debug, Default, Clone)]
pub struct GuestLog {
    pub activations: Vec<Activation>,
    pub batches: Vec<Vec<Event>>,
    pub deactivations: Vec<DeactivationReason>,
}

/// A native guest that records its calls and returns no submissions.
pub struct RecordingGuest {
    pub log: Arc<Mutex<GuestLog>>,
}

impl RecordingGuest {
    /// A guest and the shared log it writes to.
    pub fn new() -> (RecordingGuest, Arc<Mutex<GuestLog>>) {
        let log = Arc::new(Mutex::new(GuestLog::default()));
        (
            RecordingGuest {
                log: Arc::clone(&log),
            },
            log,
        )
    }
}

impl NativeGuest for RecordingGuest {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        self.log
            .lock()
            .expect("log lock")
            .activations
            .push(activation);
        Ok(())
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        self.log
            .lock()
            .expect("log lock")
            .deactivations
            .push(reason);
        None
    }

    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.log.lock().expect("log lock").batches.push(batch);
        Ok(Vec::new())
    }
}

/// The index of the first trace entry equal to `marker`.
pub fn position_of(trace: &[String], marker: &str) -> Option<usize> {
    trace.iter().position(|entry| entry == marker)
}
