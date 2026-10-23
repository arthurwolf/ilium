//! The `ilium plugin` command line against a registry model (PKG-801, PKG-823, PKG-824).

use crate::error::PluginError;
use crate::registry::Registry;

/// What a command prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    /// The text on standard output.
    pub text: String,
}

/// Runs `args`, for example `["plugin", "yank", "demo@1.0.0", "--reason", "broken"]`, against
/// `registry` as the owner of `signer_key`.
pub fn run(
    _registry: &mut Registry,
    _signer_key: &str,
    _args: &[&str],
) -> Result<CliOutput, PluginError> {
    unimplemented!("api::cli::run")
}
