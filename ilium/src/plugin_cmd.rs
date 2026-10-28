//! `ilium plugin ...` forwarding and the plugin start-mode switches (reference-cli.md).
//! Skeleton: not wired into clap yet, so `ilium --help` and the existing CLI tests stay unchanged.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use ilium_plugin_api::Platform;
use ilium_plugin_host::cli::CliEnv;
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};

/// The environment variable that starts only core plugins.
pub const SAFE_PLUGINS_ENV: &str = "ILIUM_SAFE_PLUGINS";
/// The environment variable that starts no plugins.
pub const NO_PLUGINS_ENV: &str = "ILIUM_NO_PLUGINS";

/// Decides the start mode from the flags and environment values (`--no-plugins` beats
/// `--safe-plugins`; a variable counts as set when it is non-empty and not `0`).
pub fn parse_start_mode(
    safe_plugins_flag: bool,
    no_plugins_flag: bool,
    safe_plugins_env: Option<&str>,
    no_plugins_env: Option<&str>,
) -> StartMode {
    fn is_set(value: Option<&str>) -> bool {
        value.is_some_and(|text| !text.is_empty() && text != "0")
    }
    if no_plugins_flag || is_set(no_plugins_env) {
        StartMode::NoPlugins
    } else if safe_plugins_flag || is_set(safe_plugins_env) {
        StartMode::SafePlugins
    } else {
        StartMode::Normal
    }
}

/// Fetcher used until an HTTP client is wired in: every request reports the network as
/// unreachable, so registry commands fall back to the offline cache.
struct UnreachableFetcher;

impl Fetcher for UnreachableFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

/// The real environment for a command: directories, working directory, clock and fetcher.
pub fn real_cli_env() -> CliEnv {
    let project_dirs = directories::ProjectDirs::from("", "", "ilium");
    let fallback = std::env::temp_dir().join("ilium");
    let config_dir = project_dirs
        .as_ref()
        .map_or_else(|| fallback.join("config"), |dirs| dirs.config_dir().to_path_buf());
    let data_dir = project_dirs
        .as_ref()
        .map_or_else(|| fallback.join("data"), |dirs| dirs.data_dir().to_path_buf());
    let state_dir = project_dirs.as_ref().map_or_else(
        || fallback.join("state"),
        |dirs| {
            dirs.state_dir()
                .unwrap_or_else(|| dirs.data_local_dir())
                .to_path_buf()
        },
    );
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let is_ci = std::env::var("CI").is_ok_and(|value| !value.is_empty() && value != "0");
    let mut environment = CliEnv::new(
        data_dir,
        state_dir,
        config_dir,
        cwd,
        SystemTime::now(),
        is_ci,
        Arc::new(UnreachableFetcher),
    );
    environment.platform = current_platform();
    for (name, value) in std::env::vars() {
        if name.starts_with("ILIUM_") {
            environment.env.insert(name, value);
        }
    }
    environment
}

/// Runs `ilium plugin <args...>`, prints JSONL to stdout and text to stderr, returns the exit code.
pub fn run_plugin_command(args: &[String]) -> i32 {
    let output = ilium_plugin_host::cli::run(args, &real_cli_env());
    let mut stdout = std::io::stdout().lock();
    let printed = if output.stdout_raw.is_empty() {
        stdout.write_all(output.stdout_text.as_bytes())
    } else {
        stdout.write_all(output.stdout_raw.as_bytes())
    };
    let flushed = printed.and_then(|()| stdout.flush());
    if let Err(error) = flushed {
        tracing::warn!("ilium plugin: writing stdout failed: {error}");
    }
    if !output.stderr.is_empty() {
        let mut stderr = std::io::stderr().lock();
        if let Err(error) = stderr.write_all(output.stderr.as_bytes()) {
            tracing::warn!("ilium plugin: writing stderr failed: {error}");
        }
    }
    output.exit_code
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: HST-033
    #[test]
    fn hst_033_the_safe_flag_alone_selects_safe_plugins() {
        assert_eq!(
            parse_start_mode(true, false, None, None),
            StartMode::SafePlugins
        );
    }

    /// REQ: HST-033
    #[test]
    fn hst_033_the_no_plugins_flag_alone_selects_no_plugins() {
        assert_eq!(
            parse_start_mode(false, true, None, None),
            StartMode::NoPlugins
        );
    }

    /// REQ: HST-033
    #[test]
    fn hst_033_no_flag_and_no_variable_selects_normal() {
        assert_eq!(
            parse_start_mode(false, false, None, None),
            StartMode::Normal
        );
    }

    /// REQ: HST-043
    #[test]
    fn hst_043_no_plugins_flag_beats_safe_plugins_flag() {
        assert_eq!(
            parse_start_mode(true, true, None, None),
            StartMode::NoPlugins
        );
    }

    /// REQ: HST-043
    #[test]
    fn hst_043_no_plugins_variable_beats_safe_flag_as_the_strictest_mode() {
        assert_eq!(
            parse_start_mode(true, false, None, Some("1")),
            StartMode::NoPlugins
        );
    }

    /// REQ: HST-033
    #[test]
    fn hst_033_a_non_zero_safe_variable_selects_safe_plugins() {
        assert_eq!(
            parse_start_mode(false, false, Some("1"), None),
            StartMode::SafePlugins
        );
    }

    /// REQ: HST-033
    #[test]
    fn hst_033_empty_or_zero_variables_count_as_unset() {
        assert_eq!(
            parse_start_mode(false, false, Some(""), None),
            StartMode::Normal
        );
        assert_eq!(
            parse_start_mode(false, false, Some("0"), None),
            StartMode::Normal
        );
        assert_eq!(
            parse_start_mode(false, false, None, Some("0")),
            StartMode::Normal
        );
    }

    /// REQ: DEV-003
    #[test]
    fn dev_003_help_prints_usage_and_exits_with_zero() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_plugin_command(&args), 0);
    }

    /// REQ: DEV-010
    #[test]
    fn dev_010_unknown_subcommand_is_a_usage_error_with_exit_code_two() {
        let args = vec!["no-such-subcommand".to_string()];
        assert_eq!(run_plugin_command(&args), 2);
    }

    /// REQ: DEV-003
    #[test]
    fn dev_003_real_cli_env_uses_the_current_working_directory() {
        let environment = real_cli_env();
        assert_eq!(
            environment.cwd,
            std::env::current_dir().expect("the test process has a working directory")
        );
    }
}
