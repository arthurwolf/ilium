//! Shared helpers of the kernel-boot lane for the ilium crate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

/// The workspace root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The text of one manual page.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Runs the `ilium` binary with isolated state and a hard time bound.
pub fn run_ilium(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ilium"));
    command
        .args(args)
        .env("XDG_DATA_HOME", scratch.path().join("data"))
        .env("XDG_CONFIG_HOME", scratch.path().join("config"))
        .env("XDG_RUNTIME_DIR", scratch.path())
        .env_remove("ILIUM_SAFE_PLUGINS")
        .env_remove("ILIUM_NO_PLUGINS");
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ilium");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().expect("poll child").is_some() {
            return child.wait_with_output().expect("collect output");
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return child.wait_with_output().expect("collect output after kill");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
