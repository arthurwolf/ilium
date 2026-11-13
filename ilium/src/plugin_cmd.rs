//! `ilium plugin ...` forwarding and the plugin start-mode switches (reference-cli.md).
//! Skeleton: not wired into clap yet, so `ilium --help` and the existing CLI tests stay unchanged.

use ilium_plugin_host::cli::CliEnv;
use ilium_plugin_host::lifecycle::StartMode;

/// The environment variable that starts only core plugins.
pub const SAFE_PLUGINS_ENV: &str = "ILIUM_SAFE_PLUGINS";
/// The environment variable that starts no plugins.
pub const NO_PLUGINS_ENV: &str = "ILIUM_NO_PLUGINS";

/// Decides the start mode from the flags and environment values (`--no-plugins` beats
/// `--safe-plugins`; a variable counts as set when it is non-empty and not `0`).
pub fn parse_start_mode(
    _safe_plugins_flag: bool,
    _no_plugins_flag: bool,
    _safe_plugins_env: Option<&str>,
    _no_plugins_env: Option<&str>,
) -> StartMode {
    unimplemented!("cli::plugin_cmd::parse_start_mode")
}

/// The real environment for a command: directories, working directory, clock and fetcher.
pub fn real_cli_env() -> CliEnv {
    unimplemented!("cli::plugin_cmd::real_cli_env")
}

/// Runs `ilium plugin <args...>`, prints JSONL to stdout and text to stderr, returns the exit code.
pub fn run_plugin_command(args: &[String]) -> i32 {
    let output = ilium_plugin_host::cli::run(args, &real_cli_env());
    unimplemented!(
        "cli::plugin_cmd::run_plugin_command: print output ({})",
        output.exit_code
    )
}
