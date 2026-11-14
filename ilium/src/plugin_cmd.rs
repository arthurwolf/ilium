//! `ilium plugin ...` forwarding, the plugin start-mode switches (reference-cli.md) and the
//! catalogue of bundled plugins the client side of this binary runs.
//!
//! `ilium plugin list|enable|disable` work with and without a running server: the bundled
//! session backend below edits `installed.json` directly when no server serves the project,
//! and asks the live server otherwise, so a running plugin stops or starts at once.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_plugin_host::cli::{
    SessionBackend, SessionPlugin, SessionPluginDetails, SessionRequest, SessionResponse,
};
use ilium_plugin_host::runtime::{CataloguePlugin, ESSENTIAL_PLUGINS};
use ilium_plugin_host::telemetry::{HourBucket, InspectNode, MemoryView, TraceRecord};
use ilium_plugin_host::{registry, wiring};

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
    let is_set = |value: Option<&str>| value.is_some_and(|text| !text.is_empty() && text != "0");
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
    let config_dir = project_dirs.as_ref().map_or_else(
        || fallback.join("config"),
        |dirs| dirs.config_dir().to_path_buf(),
    );
    let data_dir = project_dirs.as_ref().map_or_else(
        || fallback.join("data"),
        |dirs| dirs.data_dir().to_path_buf(),
    );
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

/// The bundled plugins with the components of one side (`"server"` or `"client"`).
pub fn bundled_catalogue(side: &str) -> Vec<CataloguePlugin> {
    wiring::catalogue(
        ilium_plugin_bundle::plugins()
            .iter()
            .map(|plugin| (plugin.id, plugin.manifest, plugin.component(side))),
    )
}

/// What the interactive client needs to run plugins, with the start mode the environment
/// asks for (`main` exports `--no-plugins` and `--safe-plugins` into it before any thread
/// starts, which is also how the spawned server learns the mode).
pub fn client_plugin_start() -> Option<ilium_client::plugin_host::host::ClientPluginStart> {
    let mode = wiring::start_mode_from_env();
    // No host at all: the client then also tells the server it has no plugins.
    if mode == StartMode::NoPlugins {
        return None;
    }
    let directories = directories::ProjectDirs::from("", "", "ilium")?;
    Some(ilium_client::plugin_host::host::ClientPluginStart {
        catalogue: bundled_catalogue("client"),
        data_dir: directories.data_dir().to_path_buf(),
        mode,
        help: ilium_plugin_bundle::plugins()
            .iter()
            .flat_map(|plugin| {
                plugin
                    .help
                    .iter()
                    .map(|topic| ilium_client::plugin_host::host::PluginHelpTopic {
                        plugin: plugin.id.to_owned(),
                        id: topic.id.to_owned(),
                        title: topic.title.to_owned(),
                        markdown: topic.markdown.to_owned(),
                    })
            })
            .collect(),
        schemas: ilium_plugin_bundle::plugins()
            .iter()
            .flat_map(|plugin| plugin.schemas)
            .map(|schema| (schema.service.to_owned(), schema.text.to_owned()))
            .collect(),
        scenes: bundled_catalogue("scene"),
    })
}

/// How long the CLI waits for a live server to confirm an enable or disable.
const LIVE_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(10);

/// The live server of a project session, when one is running.
struct LiveServer {
    socket_path: PathBuf,
    session_name: String,
    /// The CLI's runtime; requests block on it from the blocking thread the command runs on.
    runtime: tokio::runtime::Handle,
}

impl LiveServer {
    /// Asks the server to enable or disable a plugin and waits for the state it reports.
    /// `Ok` carries the state name; a refusal or a lost connection is an error code.
    fn set_enabled(&self, plugin: &str, enabled: bool) -> Result<String, String> {
        use ilium_ipc::plugin::{ControlBody, EnvelopeBody, PluginEnvelope};
        self.runtime.block_on(async {
            let mut connection = ilium_client::connection::Connection::connect(
                &self.socket_path,
                self.session_name.clone(),
            )
            .await
            .map_err(|error| {
                tracing::warn!(%error, "ilium plugin: the live server did not answer");
                "session.unreachable".to_owned()
            })?;
            connection
                .requests
                .send(ilium_ipc::ClientRequest::Plugin(PluginEnvelope {
                    plugin: plugin.to_owned(),
                    seq: 1,
                    body: EnvelopeBody::Control(ControlBody::SetEnabled {
                        plugin: plugin.to_owned(),
                        enabled,
                    }),
                }))
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "ilium plugin: the live server did not answer");
                    "session.unreachable".to_owned()
                })?;
            let confirmation = async {
                while let Some(event) = connection.events.recv().await {
                    let (event, _retention) = event.into_parts();
                    // The server answers a session-name mismatch with an error and
                    // nothing else.
                    if let ilium_ipc::ServerEvent::Error { message } = &event {
                        tracing::warn!(%message, "ilium plugin: the live server refused");
                        return Err("session.not-found".to_owned());
                    }
                    let ilium_ipc::ServerEvent::Plugin(PluginEnvelope {
                        body: EnvelopeBody::Control(ControlBody::PluginState { plugin: id, state }),
                        ..
                    }) = event
                    else {
                        continue;
                    };
                    if id != plugin {
                        continue;
                    }
                    if let Some(code) = state.strip_prefix("refused:") {
                        return Err(code.to_owned());
                    }
                    // The attach snapshot also reports the state the plugin had before;
                    // only a state on the requested side of the switch is the answer.
                    if wiring::is_disabled_state(&state) != enabled {
                        return Ok(state);
                    }
                }
                Err("session.unreachable".to_owned())
            };
            tokio::time::timeout(LIVE_CONFIRMATION_TIMEOUT, confirmation)
                .await
                .unwrap_or_else(|_| Err("session.timeout".to_owned()))
        })
    }
}

/// The session backend of `ilium plugin list|enable|disable` over the bundled catalogue.
struct BundledSession {
    data_dir: PathBuf,
    catalogue: Vec<CataloguePlugin>,
    live: Option<LiveServer>,
}

impl BundledSession {
    fn details(&self) -> Result<Vec<SessionPluginDetails>, String> {
        let installed = wiring::installed_path(&self.data_dir);
        let disabled = registry::disabled_plugin_ids(&installed)
            .map_err(|error| error.code().as_str().to_owned())?;
        // Without a server nothing runs, so the list reports the stored choice only.
        let observed = std::collections::BTreeMap::new();
        Ok(wiring::plugin_rows(&self.catalogue, &disabled, &observed)
            .into_iter()
            .map(|row| {
                let is_disabled = wiring::is_disabled_state(&row.state);
                SessionPluginDetails {
                    id: row.id,
                    version: row.version,
                    tier: "core".to_owned(),
                    state: if is_disabled {
                        "disabled".to_owned()
                    } else if self.live.is_some() {
                        "running".to_owned()
                    } else {
                        // Enabled with no server to run it.
                        "stopped".to_owned()
                    },
                    dev: false,
                    signed_by: None,
                    grants: Vec::new(),
                    failure_count: 0,
                    data_exists: false,
                    cache_exists: false,
                    storage_buckets: 0,
                    settings_values: 0,
                    log_kept: false,
                }
            })
            .collect())
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> Result<SessionResponse, String> {
        let Some(plugin) = self.catalogue.iter().find(|plugin| plugin.id == id) else {
            return Err("plugin.not-found".to_owned());
        };
        if !enabled && ESSENTIAL_PLUGINS.contains(&id) {
            return Err("plugin.essential".to_owned());
        }
        match &self.live {
            // The server persists the choice itself, in the same store.
            Some(live) => live.set_enabled(id, enabled).map(|_| SessionResponse::Done),
            None => registry::set_plugin_enabled(
                &wiring::installed_path(&self.data_dir),
                id,
                &wiring::manifest_version(&plugin.manifest),
                enabled,
            )
            .map(|_| SessionResponse::Done)
            .map_err(|error| error.code().as_str().to_owned()),
        }
    }
}

impl SessionBackend for BundledSession {
    fn request(&self, request: SessionRequest) -> Result<SessionResponse, String> {
        match request {
            SessionRequest::Connect => Ok(SessionResponse::Done),
            SessionRequest::List => self.details().map(SessionResponse::Plugins),
            SessionRequest::Describe { id } => self
                .details()?
                .into_iter()
                .find(|plugin| plugin.id == id)
                .map(SessionResponse::Plugin)
                .ok_or_else(|| "plugin.not-found".to_owned()),
            SessionRequest::Enable { id } => self.set_enabled(&id, true),
            SessionRequest::Disable { id } => self.set_enabled(&id, false),
            _ => Err("session.operation-unsupported".to_owned()),
        }
    }

    fn plugin(&self, id: &str) -> Option<SessionPlugin> {
        self.catalogue
            .iter()
            .any(|plugin| plugin.id == id)
            .then_some(SessionPlugin {
                dev: false,
                running: self.live.is_some(),
            })
    }

    fn trace_records(&self, _id: &str) -> Vec<TraceRecord> {
        Vec::new()
    }

    fn last_tree(&self, _id: &str, _target: &str) -> Option<InspectNode> {
        None
    }

    fn profile_exports(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }

    fn profile_calls(
        &self,
        _id: &str,
        _duration_secs: u32,
    ) -> Vec<ilium_plugin_host::cli::ProfileCall> {
        Vec::new()
    }

    fn memory_views(&self, _id: &str) -> Vec<MemoryView> {
        Vec::new()
    }

    fn counters(&self, _id: &str) -> Vec<(ilium_plugin_api::Side, Vec<HourBucket>)> {
        Vec::new()
    }
}

/// Runs `ilium plugin <args...>` for the project in `cwd`, prints JSONL to stdout and text to
/// stderr, returns the exit code. Call it from a blocking thread, never from an async task:
/// reaching a live server blocks on `runtime`.
pub fn run_plugin_command_in(args: &[String], cwd: &Path, runtime: tokio::runtime::Handle) -> i32 {
    let mut environment = real_cli_env();
    // Only the three commands this backend fully serves get it; every other command keeps
    // the behaviour it has without a session.
    if matches!(
        args.first().map(String::as_str),
        Some("list" | "enable" | "disable")
    ) {
        let live =
            crate::session::resolve_project_session(cwd, crate::session::DEFAULT_SESSION_NAME)
                .ok()
                .filter(|session| crate::session::is_session_live(&session.socket_path))
                .map(|session| LiveServer {
                    socket_path: session.socket_path.clone(),
                    session_name: session.name.clone(),
                    runtime: runtime.clone(),
                });
        environment.session = Some(Arc::new(BundledSession {
            data_dir: environment.data_dir.clone(),
            catalogue: bundled_catalogue("client"),
            live,
        }));
    }
    print_output(ilium_plugin_host::cli::run(args, &environment))
}

/// Runs `ilium plugin <args...>` without a session, prints JSONL to stdout and text to stderr,
/// returns the exit code.
pub fn run_plugin_command(args: &[String]) -> i32 {
    print_output(ilium_plugin_host::cli::run(args, &real_cli_env()))
}

fn print_output(output: ilium_plugin_host::cli::CliOutput) -> i32 {
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
