//! Every `ilium plugin ...` subcommand as a pure function of arguments and environment
//! (reference-cli.md). The binary forwards here.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::registry_client::Fetcher;

/// The environment a command runs in (injected so tests need no real directories).
#[derive(Clone)]
pub struct CliEnv {
    /// Data directory.
    pub data_dir: PathBuf,
    /// State directory.
    pub state_dir: PathBuf,
    /// Config directory.
    pub config_dir: PathBuf,
    /// Working directory.
    pub cwd: PathBuf,
    /// The current time.
    pub now: SystemTime,
    /// Whether the command runs in CI (no prompts).
    pub is_ci: bool,
    /// How the command reaches registries.
    pub fetcher: std::sync::Arc<dyn Fetcher>,
}

/// What a command produced: JSONL records on stdout, text on stderr, an exit code.
#[derive(Debug, Clone, PartialEq)]
pub struct CliOutput {
    /// Exit code (reference-cli.md exit codes).
    pub exit_code: i32,
    /// One JSON object per stdout line.
    pub stdout_jsonl: Vec<serde_json::Value>,
    /// Human text for stderr.
    pub stderr: String,
}

/// Runs `ilium plugin <args...>`.
pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    unimplemented!("host::cli::run")
}
