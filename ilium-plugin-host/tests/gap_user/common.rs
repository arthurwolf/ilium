//! Shared helpers of the user-gap lane: the rig shortcuts and the manual-page reader. This test
//! target is separate from `lifecycle_ops`, so it carries its own copy of the small helpers.

use std::path::Path;
use std::time::Duration;

use ilium_plugin_host::cli::CliOutput;
pub use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

/// The plugin id most tests install.
pub const NOTES: &str = "com.example.notes";
/// A second, unrelated plugin id.
pub const OTHER: &str = "com.example.other";

/// A fresh rig with empty data, state, config and cache directories.
pub fn rig() -> OpsRig {
    OpsRig::new()
}

/// A package spec with the given id and version.
pub fn pkg(id: &str, version: &str) -> PackageSpec {
    PackageSpec::new(id, version)
}

/// Whole seconds as a `Duration`, for advancing the virtual clock.
pub fn secs(count: u64) -> Duration {
    Duration::from_secs(count)
}

/// The text of one manual page (`docs/plugins-manual/pages/<name>.md`).
pub fn manual_page(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the host crate sits inside the workspace")
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Installs a spec non-interactively: `install <file> --yes --allow-unsigned`.
pub fn install(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"])
}

/// Installs a spec and asserts exit code 0.
pub fn install_ok(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let output = install(rig, spec);
    assert_eq!(output.exit_code, 0, "install failed: {}", output.stderr);
    output
}
