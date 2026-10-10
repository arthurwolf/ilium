use std::path::PathBuf;

/// Resolve the matching helper built with this integration-test target.
/// Explicit overrides remain useful for qualification against a pinned helper.
pub fn helper_path() -> PathBuf {
    std::env::var_os("ILIUM_ANIMATION_HELPER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_ilium-animation-helper")))
}
