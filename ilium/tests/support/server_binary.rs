//! Shared helper for end-to-end tests that spawn the `ilium-server` executable.
//!
//! Cargo only builds the binaries of the package under test for integration tests, and
//! `ilium-server` lives in another package, so it is built on demand into the same target
//! directory. Each integration test file includes this module with `#[path]`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Locates the `ilium-server` executable beside this package's `ilium` binary. The server is built
/// on demand into the same target directory; a build that fails surfaces its stderr rather than a
/// missing-file assertion.
pub fn ensure_server_binary() -> PathBuf {
    let server_binary = Path::new(env!("CARGO_BIN_EXE_ilium")).with_file_name("ilium-server");
    if server_binary.is_file() {
        return server_binary;
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let output = Command::new(cargo)
        .current_dir(&workspace_root)
        .args([
            "build",
            "-p",
            "ilium-server",
            "--bin",
            "ilium-server",
            "--message-format=json",
        ])
        .output()
        .expect("run cargo build for ilium-server");
    assert!(
        output.status.success(),
        "cargo build of ilium-server failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Cargo reports the exact executable it wrote; the target folder seen by this test
    // process is not guaranteed to be the one the nested build used.
    let built_binary = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["target"]["name"] == "ilium-server"
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| {
            panic!(
                "cargo reported no executable for ilium-server:\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert!(
        built_binary.is_file(),
        "cargo reported {} but the file is missing",
        built_binary.display()
    );
    built_binary
}
