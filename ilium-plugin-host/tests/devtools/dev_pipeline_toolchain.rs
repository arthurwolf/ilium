//! Toolchain minimums checked by `ilium plugin doctor` before a build starts (DEV-883).

use std::path::Path;
use std::sync::Arc;

use ilium_plugin_host::cli::ToolProbe;

use crate::common::*;

/// A probe that reports every tool at version 0.0.1, which is below any minimum the manual gives.
struct OldToolsProbe;

impl ToolProbe for OldToolsProbe {
    fn tool_version(&self, _tool: &str) -> Option<String> {
        Some("0.0.1".to_owned())
    }

    fn rust_target_installed(&self, _target: &str) -> bool {
        true
    }

    fn free_disk_bytes(&self, _dir: &Path) -> u64 {
        u64::MAX
    }
}

/// REQ: DEV-883
#[test]
fn doctor_fails_with_toolchain_too_old_naming_the_found_version() {
    let root = scratch();
    let env = env(root.path()).with_probe(Arc::new(OldToolsProbe));
    let output = run_in_project(&env, "doctor", &[]);
    assert_eq!(error_code(&output).as_deref(), Some("toolchain.too-old"));
    let error = records_of(&output, "error")
        .into_iter()
        .next()
        .expect("an error record");
    assert!(
        error.to_string().contains("0.0.1"),
        "the error names the found version: {error}"
    );
}
