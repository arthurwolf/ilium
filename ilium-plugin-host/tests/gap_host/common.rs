//! Shared helpers of the host-gap lane: the manual-page reader, the rig shortcuts and the
//! requirement-text assertion. Everything uses the `ilium_plugin_host::testing::ops` rig and the
//! items named in API_GAPS.md; hooks that do not exist yet are called anyway and recorded.

pub use ilium_plugin_host::cli::CliOutput;
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

/// Installs a spec non-interactively and asserts exit code 0.
pub fn install_ok(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"]);
    assert_eq!(
        output.exit_code,
        0,
        "install of {} must succeed: {}",
        spec.id(),
        output.stderr
    );
    output
}
