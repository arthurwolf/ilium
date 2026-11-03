//! Shared helpers of the SECU containment tests: a CLI environment rooted in a scratch
//! directory, a state snapshot for "no change" assertions, and the audit and list readers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_plugin_host::cli::{run_cli, CliEnv, CliRunOutput, MemorySession};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};

/// The plugin id used by every SECU containment test.
pub const PLUGIN: &str = "acme.notes";

/// A fetcher that must never be reached: kill, unkill and listing read only local state.
pub struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        panic!("containment commands never fetch")
    }
}

/// A CLI environment whose directories all live under `root`. `with_session` attaches a
/// running session; without it the commands that need one must report that none runs.
pub fn env(root: &Path, with_session: bool) -> CliEnv {
    let env = CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        root.to_path_buf(),
        SystemTime::UNIX_EPOCH + Duration::from_secs(120),
        true,
        Arc::new(NoNetwork),
    );
    if with_session {
        env.with_session(MemorySession::new())
    } else {
        env
    }
}

/// Splits literal words into the owned argument list `run_cli` takes.
pub fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Runs `ilium plugin <words>` in the given environment.
pub fn plugin(env: &CliEnv, values: &[&str]) -> CliRunOutput {
    run_cli(env, &words(values))
}

/// Every file under `dir` with its bytes, keyed by path relative to `dir`.
pub fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(dir, dir, &mut files);
    files
}

fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if let Ok(bytes) = std::fs::read(&path) {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            files.insert(relative, bytes);
        }
    }
}

/// The audit log as `ilium plugin audit` prints it.
pub fn audit_text(env: &CliEnv) -> String {
    plugin(env, &["audit"]).stdout
}

/// The line of `ilium plugin list` that names the plugin, or an empty string when absent.
pub fn row(env: &CliEnv, plugin_id: &str) -> String {
    plugin(env, &["list"])
        .stdout
        .lines()
        .find(|line| line.contains(plugin_id))
        .unwrap_or_default()
        .to_owned()
}
