//! OS sandbox setup through `ilium-platform` (seccomp and landlock on Linux, the platform
//! equivalents elsewhere); reference-security.md.

use crate::HelperError;

/// What the helper may touch once sandboxed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SandboxPolicy {
    /// Directories the helper may read.
    pub read_paths: Vec<std::path::PathBuf>,
    /// Directories the helper may write.
    pub write_paths: Vec<std::path::PathBuf>,
    /// Whether outbound network access is allowed.
    pub allow_network: bool,
}

/// Applies the policy to the current process; it cannot be undone.
pub fn apply(policy: &SandboxPolicy) -> Result<(), HelperError> {
    unimplemented!("helper::sandbox::apply")
}
