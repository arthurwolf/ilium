//! Server-wide configuration, loaded from `~/.config/ilium/config.toml`
//! (see `CLAUDE.md`'s "Config & data locations"): the detection loop's
//! adaptive poll cadence (ARCHITECTURE.md "Poll cadence"), user-configured
//! agent detection signatures (ARCHITECTURE.md M5), and the desktop
//! notification toggle (ARCHITECTURE.md M5, `Working -> Done` notifications).
//! Keybinding and theme
//! config live client-side (`ilium-client/src/config.rs`) since this
//! crate never touches rendering or input dispatch.

use std::borrow::Cow;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use ilium_core::AgentClass;
use ilium_detect::AgentSignature;
use ilium_sound::SoundSettings;
use serde::Deserialize;

use crate::error::{ConfigLoadError, ServerError};

/// How often the detection loop re-checks a pane, per its last-observed
/// activity. Values below a few hundred milliseconds are clamped up at
/// load time (see [`DetectionConfig::from_raw`]) -- the detection loop
/// does real work (a `sysinfo` refresh, a screen-text scan) on every due
/// pane, and a misconfigured near-zero interval would turn "adaptive
/// backoff" into "busy loop."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectionConfig {
    /// Poll interval for panes last classified as `Working` or
    /// `WaitingApproval` -- both are states a prompt user actively wants a
    /// quick transition out of (`Working -> Done`, or `WaitingApproval`
    /// resolving once they answer). `WaitingApproval` in particular must
    /// not share the slow tier below: it is by definition a state the user
    /// is expected to act on imminently, so any answer -- or any
    /// classification that flagged it in error on a single transient
    /// screen -- should self-correct within one fast interval, not linger
    /// for up to `idle_poll_interval` after the screen has already moved
    /// on. See `crate::detection::interval_for`.
    pub working_poll_interval: Duration,
    /// Poll interval for panes last classified as `Idle`, `Done`, or plain
    /// shells with no agent detected -- none of those change on their own
    /// between polls, so polling slowly is both correct and cheap.
    pub idle_poll_interval: Duration,
    /// Whether the detection loop auto-answers known one-time interstitial
    /// dialogs (`ilium_detect::interstitial_prompt_response`) by writing the
    /// registered key straight into the pty -- currently just Claude Code's
    /// "resume full session" prompt. Defaults on: the registry only contains
    /// dialogs verified safe to answer the same way every time, and the
    /// per-pid latch (`TerminalPaneRuntime::auto_answered_interstitial_prompt_for_pid`)
    /// bounds it to exactly one keystroke per agent process.
    pub auto_answer_interstitial_prompts: bool,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            working_poll_interval: Duration::from_secs(10),
            idle_poll_interval: Duration::from_secs(45),
            auto_answer_interstitial_prompts: true,
        }
    }
}

/// Whether a pane's `Working -> Done`/`Idle` transition fires a desktop
/// notification (ARCHITECTURE.md M5). A separate table from `[detection]` -- polling
/// cadence and "should this ever pop a notification" are independent
/// concerns a user may want to tune separately (e.g. keep fast polling but
/// disable notifications on a headless box with no notification daemon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationsConfig {
    pub enabled: bool,
}

/// Whether this process writes its instrumented events to the session's
/// timestamped `/tmp/.ilium/...` file. It is deliberately disabled unless the
/// user opts in through the Debug settings tab or `[debug]` config table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DebugConfig {
    pub file_logging_enabled: bool,
}

/// Loopback-only HTTP automation settings. The listener deliberately never
/// accepts remote connections: prompt submission controls local coding agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpApiConfig {
    pub port: u16,
}

impl Default for HttpApiConfig {
    fn default() -> Self {
        Self { port: 8872 }
    }
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Every table `config.toml` may contain, already validated. What
/// [`load`] returns; a config file that fails to load falls back to
/// [`ServerConfig::default`] (every field's own default) at the call site
/// rather than this crate hardcoding a fallback here.
///
/// No `PartialEq`/`Eq`/`Copy` here (unlike its sub-fields): `custom_signatures`
/// holds `ilium_detect::AgentSignature`, whose `class_of` is a `fn`
/// pointer -- comparing those is documented as unreliable, so
/// `AgentSignature` deliberately doesn't derive equality either (see its
/// own doc comment). Nothing needs whole-`ServerConfig` equality; tests
/// compare the individual fields that do support it instead.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub detection: DetectionConfig,
    pub notifications: NotificationsConfig,
    pub sound: SoundSettings,
    /// User-configured agent signatures (`[[detection.custom_signatures]]`)
    /// to check alongside `ilium-detect`'s built-in registry -- see
    /// `ilium_detect::identify_agent_with_extra`, the registry extension
    /// point this list feeds.
    pub custom_signatures: Vec<AgentSignature>,
    pub session_recovery: SessionRecoveryConfig,
    /// Whether native session JSON files are copied to the project-local
    /// rolling backup store. Missing config defaults to enabled.
    pub session_backups_enabled: bool,
    pub debug: DebugConfig,
    pub http_api: HttpApiConfig,
    pub agent_debug_menu_enabled: bool,
    /// Whether the server accepts `SetPaneProgressMonitor` at all -- see
    /// `ilium-server`'s progress-monitor loop. Defaults on: the command it
    /// runs is agent-authored, but the agent already has equivalent shell
    /// access in the same pane, so this gates an unattended *recurring*
    /// execution rather than a new privilege. Live-toggleable from the
    /// client's Settings tab without a server restart -- see
    /// `ClientRequest::UpdateProgressMonitorEnabled`.
    pub progress_monitor_enabled: bool,
}

impl Default for ServerConfig {
    /// Hand-written rather than `#[derive(Default)]`: a bare `bool` field
    /// defaults to `false` under the derive, which is wrong for
    /// `progress_monitor_enabled` (defaults on -- see its own doc comment).
    /// Every other field's own `Default` impl already gives the value
    /// [`load`] would produce for a missing config file, so this mirrors
    /// that field for field.
    fn default() -> Self {
        Self {
            detection: DetectionConfig::default(),
            notifications: NotificationsConfig::default(),
            sound: SoundSettings::default(),
            custom_signatures: Vec::new(),
            session_recovery: SessionRecoveryConfig::default(),
            session_backups_enabled: true,
            debug: DebugConfig::default(),
            http_api: HttpApiConfig::default(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionRecoveryConfig {
    #[default]
    RestoreAutomatically,
    AskBeforeRestore,
    StartFresh,
}

/// The on-disk shape of `config.toml`. Kept separate from [`ServerConfig`]
/// (which uses `Duration`, has no serde impl by design, and enforces the
/// clamp/default invariants) so a partially-specified or out-of-range
/// config file can be validated in one place ([`DetectionConfig::from_raw`],
/// [`NotificationsConfig::from_raw`], [`RawCustomSignature::validate`])
/// rather than every field needing its own serde validator.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    detection: RawDetectionConfig,
    #[serde(default)]
    notifications: RawNotificationsConfig,
    #[serde(default)]
    sound: SoundSettings,
    #[serde(default)]
    session: RawSessionConfig,
    #[serde(default)]
    debug: RawDebugConfig,
    #[serde(default, rename = "api")]
    http_api: RawHttpApiConfig,
    #[serde(default)]
    ui: RawUiConfig,
}

#[derive(Debug, Default, Deserialize)]
struct RawDebugConfig {
    file_logging_enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawHttpApiConfig {
    port: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
struct RawUiConfig {
    agent_debug_menu_enabled: Option<bool>,
    progress_monitor_enabled: Option<bool>,
}
#[derive(Debug, Default, Deserialize)]
struct RawSessionConfig {
    recovery_policy: Option<String>,
    backups_enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDetectionConfig {
    working_poll_seconds: Option<u64>,
    idle_poll_seconds: Option<u64>,
    auto_answer_interstitial_prompts: Option<bool>,
    /// `[[detection.custom_signatures]]` -- an array of tables, each one
    /// process-name pattern plus the `AgentClass` it should resolve to.
    #[serde(default)]
    custom_signatures: Vec<RawCustomSignature>,
}

#[derive(Debug, Default, Deserialize)]
struct RawNotificationsConfig {
    enabled: Option<bool>,
}

/// One `[[detection.custom_signatures]]` entry as read from TOML, before
/// validation. `agent_class` is a free-form string rather than a serde enum
/// so an unrecognized value produces this crate's own
/// [`ConfigLoadError::InvalidCustomSignature`] (naming the exact allowed
/// values) instead of serde's generic "unknown variant" message.
#[derive(Debug, Deserialize)]
struct RawCustomSignature {
    /// Lowercase substring matched against a process name -- same
    /// semantics as `AgentSignature::name_substring`.
    process_name: String,
    /// One of `"claude"`, `"codex"`, `"antigravity"`, or `"other"`.
    /// `"antimatter"` is accepted as an alias for `"antigravity"`.
    /// `"other"` resolves to
    /// `AgentClass::Other(<the process name that actually matched>)`,
    /// mirroring how the built-in `opencode`/`aider` signatures behave --
    /// there is no separate "fixed label" field, since `class_of` is a
    /// plain non-capturing `fn` pointer (see `AgentSignature`'s doc
    /// comment) and a fixed label would require a closure that captures
    /// per-entry config data instead.
    agent_class: String,
}

impl RawCustomSignature {
    fn validate(self) -> Result<AgentSignature, ConfigLoadError> {
        let name_substring = self.process_name.trim().to_lowercase();
        if name_substring.is_empty() {
            return Err(ConfigLoadError::InvalidCustomSignature(
                "process_name must not be empty".to_string(),
            ));
        }

        let class_of: fn(&str) -> AgentClass = match self.agent_class.trim().to_lowercase().as_str()
        {
            "claude" => |_matched_name: &str| AgentClass::Claude,
            "codex" => |_matched_name: &str| AgentClass::Codex,
            "antigravity" | "antimatter" => |_matched_name: &str| AgentClass::Antigravity,
            "other" => |matched_name: &str| AgentClass::Other(matched_name.to_string()),
            other => {
                return Err(ConfigLoadError::InvalidCustomSignature(format!(
                    "unknown agent_class {other:?} (expected \"claude\", \"codex\", \"antigravity\", or \"other\")"
                )))
            }
        };

        Ok(AgentSignature {
            name_substring: Cow::Owned(name_substring),
            class_of,
        })
    }
}

/// The smallest interval a misconfigured `config.toml` is allowed to
/// request -- see [`DetectionConfig`]'s doc comment.
const MINIMUM_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Fully validated live settings that can be installed into the detector.
#[derive(Debug)]
pub struct ValidatedAgentDetectionSettings {
    pub detection: DetectionConfig,
    pub custom_signatures: Vec<AgentSignature>,
}

/// Converts the current effective detector values into the stable IPC form.
/// The minimum interval is represented as zero seconds because the config
/// format itself uses integer seconds and the loader clamps zero to 500 ms.
pub fn agent_detection_settings(
    detection: &DetectionConfig,
    custom_signatures: &[AgentSignature],
) -> ilium_ipc::AgentDetectionSettings {
    ilium_ipc::AgentDetectionSettings {
        working_poll_seconds: detection.working_poll_interval.as_secs(),
        idle_poll_seconds: detection.idle_poll_interval.as_secs(),
        custom_signatures: custom_signatures
            .iter()
            .map(|signature| ilium_ipc::CustomAgentSignature {
                name_substring: signature.name_substring.to_string(),
                class: (signature.class_of)(&signature.name_substring),
            })
            .collect(),
    }
}

/// Validates an IPC settings value and builds the exact runtime representation.
/// A zero poll interval is accepted as the round-trip sentinel for 500 ms.
pub fn validate_agent_detection_settings(
    settings: &ilium_ipc::AgentDetectionSettings,
) -> Result<ValidatedAgentDetectionSettings, ilium_ipc::AgentDetectionSettingsError> {
    let working_poll_interval = validate_live_poll_interval(settings.working_poll_seconds)?;
    let idle_poll_interval = validate_live_poll_interval(settings.idle_poll_seconds)?;
    let mut custom_signatures = Vec::with_capacity(settings.custom_signatures.len());

    for signature in &settings.custom_signatures {
        let name_substring = signature.name_substring.trim().to_lowercase();
        if name_substring.is_empty() {
            return Err(ilium_ipc::AgentDetectionSettingsError {
                message: "custom signature name_substring must not be empty".to_string(),
            });
        }
        let class_of: fn(&str) -> AgentClass = match &signature.class {
            AgentClass::Claude => |_matched_name: &str| AgentClass::Claude,
            AgentClass::Codex => |_matched_name: &str| AgentClass::Codex,
            AgentClass::Antigravity => |_matched_name: &str| AgentClass::Antigravity,
            AgentClass::Other(_) => {
                |matched_name: &str| AgentClass::Other(matched_name.to_string())
            }
        };
        custom_signatures.push(AgentSignature {
            name_substring: Cow::Owned(name_substring),
            class_of,
        });
    }

    Ok(ValidatedAgentDetectionSettings {
        detection: DetectionConfig {
            working_poll_interval,
            idle_poll_interval,
            // The settings UI intentionally does not own this input-automation
            // behavior, so preserve its server-configured value on live edits.
            auto_answer_interstitial_prompts: DetectionConfig::default()
                .auto_answer_interstitial_prompts,
        },
        custom_signatures,
    })
}

fn validate_live_poll_interval(
    seconds: u64,
) -> Result<Duration, ilium_ipc::AgentDetectionSettingsError> {
    let interval = Duration::from_secs(seconds).max(MINIMUM_POLL_INTERVAL);
    if std::time::Instant::now().checked_add(interval).is_none() {
        return Err(ilium_ipc::AgentDetectionSettingsError {
            message: "poll interval is too large for the platform timer".to_string(),
        });
    }
    Ok(interval)
}

/// Merges only detector-owned keys into `config.toml` and atomically publishes
/// the result. The lock covers read, merge, and rename so concurrent detector
/// settings writes cannot replace one another with stale values.
pub fn save_agent_detection_settings(
    config_dir: &Path,
    settings: &ilium_ipc::AgentDetectionSettings,
) -> Result<(), ilium_ipc::AgentDetectionSettingsError> {
    use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};

    let path = config_dir.join("config.toml");
    let lock_path = config_dir.join(".agent-detection-settings.lock");
    let _lock = ExclusiveFileLock::acquire(&lock_path).map_err(|error| {
        ilium_ipc::AgentDetectionSettingsError {
            message: format!("could not lock config file: {error}"),
        }
    })?;
    let mut document = match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str::<toml::Value>(&contents).map_err(|error| {
            ilium_ipc::AgentDetectionSettingsError {
                message: format!("could not parse config.toml: {error}"),
            }
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            toml::Value::Table(toml::value::Table::new())
        }
        Err(error) => {
            return Err(ilium_ipc::AgentDetectionSettingsError {
                message: format!("could not read config.toml: {error}"),
            });
        }
    };
    let root = document
        .as_table_mut()
        .ok_or_else(|| ilium_ipc::AgentDetectionSettingsError {
            message: "config.toml root must be a table".to_string(),
        })?;
    let detection = root
        .entry("detection".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .ok_or_else(|| ilium_ipc::AgentDetectionSettingsError {
            message: "config.toml [detection] entry must be a table".to_string(),
        })?;
    let working_poll_seconds = i64::try_from(settings.working_poll_seconds).map_err(|_| {
        ilium_ipc::AgentDetectionSettingsError {
            message: "working_poll_seconds exceeds the supported TOML integer range".to_string(),
        }
    })?;
    let idle_poll_seconds = i64::try_from(settings.idle_poll_seconds).map_err(|_| {
        ilium_ipc::AgentDetectionSettingsError {
            message: "idle_poll_seconds exceeds the supported TOML integer range".to_string(),
        }
    })?;
    detection.insert(
        "working_poll_seconds".to_string(),
        toml::Value::Integer(working_poll_seconds),
    );
    detection.insert(
        "idle_poll_seconds".to_string(),
        toml::Value::Integer(idle_poll_seconds),
    );
    detection.insert(
        "custom_signatures".to_string(),
        toml::Value::Array(
            settings
                .custom_signatures
                .iter()
                .map(|signature| {
                    let mut table = toml::value::Table::new();
                    table.insert(
                        "process_name".to_string(),
                        toml::Value::String(signature.name_substring.trim().to_lowercase()),
                    );
                    table.insert(
                        "agent_class".to_string(),
                        toml::Value::String(match &signature.class {
                            AgentClass::Claude => "claude".to_string(),
                            AgentClass::Codex => "codex".to_string(),
                            AgentClass::Antigravity => "antigravity".to_string(),
                            AgentClass::Other(_) => "other".to_string(),
                        }),
                    );
                    toml::Value::Table(table)
                })
                .collect(),
        ),
    );

    let serialized = toml::to_string_pretty(&document).map_err(|error| {
        ilium_ipc::AgentDetectionSettingsError {
            message: format!("could not serialize config.toml: {error}"),
        }
    })?;
    secure_fs::create_private_directory(config_dir).map_err(|error| {
        ilium_ipc::AgentDetectionSettingsError {
            message: format!("could not prepare config directory: {error}"),
        }
    })?;
    let temporary_path = config_dir.join(format!(
        ".config.toml.agent-detection-{}.tmp",
        std::process::id()
    ));
    let mut temporary_file_created = false;
    let write_result = (|| -> std::io::Result<()> {
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        temporary_file_created = true;
        file.write_all(serialized.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary_path, &path)?;
        secure_fs::restrict_file_to_owner(&path)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        if temporary_file_created {
            let _ = std::fs::remove_file(&temporary_path);
        }
        return Err(ilium_ipc::AgentDetectionSettingsError {
            message: format!("could not publish config.toml: {error}"),
        });
    }
    Ok(())
}

impl DetectionConfig {
    /// Borrows rather than consumes `raw` -- `load` still needs
    /// `raw.detection.custom_signatures` (owned, to validate into
    /// `AgentSignature`s) after this call.
    fn from_raw(raw: &RawDetectionConfig) -> Self {
        let defaults = Self::default();
        Self {
            working_poll_interval: raw
                .working_poll_seconds
                .map(Duration::from_secs)
                .unwrap_or(defaults.working_poll_interval)
                .max(MINIMUM_POLL_INTERVAL),
            idle_poll_interval: raw
                .idle_poll_seconds
                .map(Duration::from_secs)
                .unwrap_or(defaults.idle_poll_interval)
                .max(MINIMUM_POLL_INTERVAL),
            auto_answer_interstitial_prompts: raw
                .auto_answer_interstitial_prompts
                .unwrap_or(defaults.auto_answer_interstitial_prompts),
        }
    }
}

impl NotificationsConfig {
    fn from_raw(raw: RawNotificationsConfig) -> Self {
        let defaults = Self::default();
        Self {
            enabled: raw.enabled.unwrap_or(defaults.enabled),
        }
    }
}

/// Parses `[session] recovery_policy` strictly -- an unrecognized value is a
/// [`ConfigLoadError::InvalidSessionRecoveryPolicy`], not a silent fallback
/// to the default, since this string controls a crash-recovery safety gate
/// (see [`ConfigLoadError::InvalidSessionRecoveryPolicy`]'s doc comment).
/// Mirrors `ilium_client`'s identical `parse_session_recovery_policy` for
/// the client-side copy of this same setting.
fn parse_session_recovery_policy(value: &str) -> Result<SessionRecoveryConfig, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "restore_automatically" => Ok(SessionRecoveryConfig::RestoreAutomatically),
        "ask_before_restore" => Ok(SessionRecoveryConfig::AskBeforeRestore),
        "start_fresh" => Ok(SessionRecoveryConfig::StartFresh),
        _ => Err(ConfigLoadError::InvalidSessionRecoveryPolicy(
            value.to_string(),
        )),
    }
}

/// Loads `<config_dir>/config.toml`. A missing file is not an error --
/// most sessions never create one -- and loads as [`ServerConfig::default`].
/// A file that exists but fails to read or parse *is* a
/// [`ServerError::ConfigLoad`]; the caller (`main`) logs it and falls back
/// to defaults rather than refusing to start the server over a typo in an
/// optional config file.
pub fn load(config_dir: &Path) -> Result<ServerConfig, ServerError> {
    let path = config_dir.join("config.toml");
    // A single `read_to_string` (rather than an `exists()` check followed by
    // a separate read) avoids two failure modes a split check/read has: the
    // TOCTOU window where the file is removed between the two calls, and
    // `exists()` folding every stat error -- including a permission problem
    // on the file or a parent directory -- into "missing," which would boot
    // silently on defaults instead of surfacing a real `ConfigLoadError::Read`.
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ServerConfig::default());
        }
        Err(source) => {
            return Err(ServerError::ConfigLoad {
                path,
                source: Box::new(ConfigLoadError::Read(source)),
            });
        }
    };
    let raw: RawConfig = toml::from_str(&contents).map_err(|source| ServerError::ConfigLoad {
        path: path.clone(),
        source: Box::new(ConfigLoadError::Parse(source)),
    })?;

    let detection = DetectionConfig::from_raw(&raw.detection);
    let custom_signatures = raw
        .detection
        .custom_signatures
        .into_iter()
        .map(RawCustomSignature::validate)
        .collect::<Result<Vec<_>, ConfigLoadError>>()
        .map_err(|source| ServerError::ConfigLoad {
            path: path.clone(),
            source: Box::new(source),
        })?;

    let session_recovery = raw
        .session
        .recovery_policy
        .as_deref()
        .map(parse_session_recovery_policy)
        .transpose()
        .map_err(|source| ServerError::ConfigLoad {
            path: path.clone(),
            source: Box::new(source),
        })?
        .unwrap_or_default();

    let http_api_port = raw.http_api.port.unwrap_or(HttpApiConfig::default().port);
    if http_api_port == 0 {
        return Err(ServerError::ConfigLoad {
            path,
            source: Box::new(ConfigLoadError::InvalidHttpApiPort),
        });
    }

    Ok(ServerConfig {
        detection,
        notifications: NotificationsConfig::from_raw(raw.notifications),
        sound: raw.sound,
        custom_signatures,
        session_recovery,
        session_backups_enabled: raw.session.backups_enabled.unwrap_or(true),
        debug: DebugConfig {
            file_logging_enabled: raw.debug.file_logging_enabled.unwrap_or(false),
        },
        http_api: HttpApiConfig {
            port: http_api_port,
        },
        agent_debug_menu_enabled: raw.ui.agent_debug_menu_enabled.unwrap_or(false),
        progress_monitor_enabled: raw.ui.progress_monitor_enabled.unwrap_or(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-server-config-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn missing_config_file_loads_defaults() {
        let dir = scratch_dir();
        let config = load(&dir).expect("missing file is not an error");
        // `ServerConfig` doesn't derive `PartialEq` (see its doc comment --
        // `AgentSignature::class_of` is a `fn` pointer), so defaults are
        // asserted field by field instead of via one struct comparison.
        assert_eq!(config.detection, DetectionConfig::default());
        assert_eq!(config.notifications, NotificationsConfig::default());
        assert_eq!(config.sound, SoundSettings::default());
        assert!(config.custom_signatures.is_empty());
        assert!(!config.debug.file_logging_enabled);
        assert_eq!(config.http_api, HttpApiConfig::default());
        assert!(!config.agent_debug_menu_enabled);
        assert!(config.progress_monitor_enabled);
        assert_eq!(
            config.detection.working_poll_interval,
            Duration::from_secs(10)
        );
    }

    #[test]
    fn progress_monitor_can_be_disabled_explicitly() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[ui]\nprogress_monitor_enabled = false\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid UI config should load");
        assert!(!config.progress_monitor_enabled);
    }

    #[test]
    fn agent_debug_capture_can_be_enabled_explicitly() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[ui]\nagent_debug_menu_enabled = true\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid UI config should load");
        assert!(config.agent_debug_menu_enabled);
    }

    #[test]
    fn debug_file_logging_can_be_enabled_explicitly() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[debug]\nfile_logging_enabled = true\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid debug config should load");
        assert!(config.debug.file_logging_enabled);
    }

    #[test]
    fn http_api_port_can_be_configured() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[api]\nport = 19072\n").unwrap();

        let config = load(&dir).expect("valid API config should load");

        assert_eq!(config.http_api.port, 19072);
    }

    #[test]
    fn valid_config_file_overrides_defaults() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[detection]\nworking_poll_seconds = 3\nidle_poll_seconds = 60\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(
            config.detection.working_poll_interval,
            Duration::from_secs(3)
        );
        assert_eq!(config.detection.idle_poll_interval, Duration::from_secs(60));
    }

    #[test]
    fn partially_specified_config_keeps_the_other_default() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[detection]\nworking_poll_seconds = 2\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(
            config.detection.working_poll_interval,
            Duration::from_secs(2)
        );
        assert_eq!(
            config.detection.idle_poll_interval,
            DetectionConfig::default().idle_poll_interval
        );
    }

    #[test]
    fn a_near_zero_interval_is_clamped_up_to_the_minimum() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[detection]\nworking_poll_seconds = 0\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(
            config.detection.working_poll_interval,
            MINIMUM_POLL_INTERVAL
        );
    }

    #[test]
    fn notifications_default_to_enabled() {
        let dir = scratch_dir();
        let config = load(&dir).expect("missing file is not an error");
        assert!(config.notifications.enabled);
    }

    #[test]
    fn notifications_can_be_disabled_via_config_file() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[notifications]\nenabled = false\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert!(!config.notifications.enabled);
        // The `[detection]` table was left unspecified entirely -- absence
        // of that whole table must not disturb its own defaults.
        assert_eq!(config.detection, DetectionConfig::default());
    }

    #[test]
    fn sound_settings_are_loaded_with_partial_event_defaults() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            concat!(
                "[sound]\n",
                "source = \"sound_file\"\n",
                "file = \"/usr/share/sounds/example.oga\"\n",
                "\n",
                "[sound.events]\n",
                "approval_required = true\n",
            ),
        )
        .unwrap();

        let config = load(&dir).expect("valid sound config should load");
        assert_eq!(config.sound.source, ilium_sound::SoundSourceKind::SoundFile);
        assert_eq!(
            config.sound.file,
            Some(std::path::PathBuf::from("/usr/share/sounds/example.oga"))
        );
        assert!(config.sound.events.agent_finished);
        assert!(config.sound.events.approval_required);
        assert!(!config.sound.events.agent_started);
    }

    #[test]
    fn malformed_toml_is_a_config_load_error_not_a_panic() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "not valid [ toml").unwrap();

        let result = load(&dir);
        assert!(matches!(result, Err(ServerError::ConfigLoad { .. })));
    }

    #[test]
    fn custom_signatures_default_to_empty() {
        let dir = scratch_dir();
        let config = load(&dir).expect("missing file is not an error");
        assert!(config.custom_signatures.is_empty());
    }

    #[test]
    fn custom_signatures_are_parsed_and_validated_into_agent_signatures() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            concat!(
                "[[detection.custom_signatures]]\n",
                "process_name = \"MyTool\"\n",
                "agent_class = \"Other\"\n",
                "\n",
                "[[detection.custom_signatures]]\n",
                "process_name = \"myclaudefork\"\n",
                "agent_class = \"claude\"\n",
            ),
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(config.custom_signatures.len(), 2);

        // `process_name` is lowercased at load time, matching
        // `AgentSignature::name_substring`'s documented lowercase-only
        // contract.
        assert_eq!(
            config.custom_signatures[0].name_substring.as_ref(),
            "mytool"
        );
        assert_eq!(
            (config.custom_signatures[0].class_of)("mytool"),
            AgentClass::Other("mytool".to_string())
        );
        assert_eq!(
            config.custom_signatures[1].name_substring.as_ref(),
            "myclaudefork"
        );
        assert_eq!(
            (config.custom_signatures[1].class_of)("myclaudefork"),
            AgentClass::Claude
        );
    }

    #[test]
    fn detection_settings_round_trip_keeps_the_minimum_interval_sentinel_and_other_tables() {
        let dir = scratch_dir();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[detection]\nworking_poll_seconds = 10\nidle_poll_seconds = 45\nauto_answer_interstitial_prompts = false\n\n[voice]\nlocale = \"en\"\n",
        )
        .expect("write starting config");
        let settings = ilium_ipc::AgentDetectionSettings {
            // Zero is the stable wire/config sentinel for the effective
            // 500 ms minimum. The UI can label this value explicitly.
            working_poll_seconds: 0,
            idle_poll_seconds: 23,
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "my-agent".to_string(),
                class: AgentClass::Codex,
            }],
        };

        save_agent_detection_settings(&dir, &settings).expect("save detection settings");

        let contents = std::fs::read_to_string(&path).expect("read saved config");
        let document: toml::Value = toml::from_str(&contents).expect("parse saved config");
        assert_eq!(document["voice"]["locale"].as_str(), Some("en"));
        assert_eq!(
            document["detection"]["auto_answer_interstitial_prompts"].as_bool(),
            Some(false)
        );
        assert_eq!(
            document["detection"]["working_poll_seconds"].as_integer(),
            Some(0)
        );
        assert_eq!(
            document["detection"]["idle_poll_seconds"].as_integer(),
            Some(23)
        );
        assert_eq!(
            document["detection"]["custom_signatures"][0]["process_name"].as_str(),
            Some("my-agent")
        );
        assert_eq!(
            document["detection"]["custom_signatures"][0]["agent_class"].as_str(),
            Some("codex")
        );
    }

    #[test]
    fn detection_settings_reject_empty_custom_signature_names() {
        let settings = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 10,
            idle_poll_seconds: 45,
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "  ".to_string(),
                class: AgentClass::Claude,
            }],
        };

        let error = validate_agent_detection_settings(&settings).expect_err("empty name rejected");

        assert!(error.message.contains("name_substring"));
    }

    #[test]
    fn detection_settings_zero_sentinel_maps_to_the_500_ms_minimum() {
        let settings = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 0,
            idle_poll_seconds: 0,
            custom_signatures: Vec::new(),
        };

        let accepted = validate_agent_detection_settings(&settings)
            .expect("zero is the explicit 500 ms minimum sentinel");

        assert_eq!(
            accepted.detection.working_poll_interval,
            MINIMUM_POLL_INTERVAL
        );
        assert_eq!(accepted.detection.idle_poll_interval, MINIMUM_POLL_INTERVAL);
    }

    #[test]
    fn detection_settings_reject_intervals_that_overflow_platform_timers() {
        let settings = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: u64::MAX,
            idle_poll_seconds: 45,
            custom_signatures: Vec::new(),
        };

        let error = validate_agent_detection_settings(&settings)
            .expect_err("unrepresentable timer deadline rejected");

        assert!(error.message.contains("platform timer"));
    }

    #[test]
    fn a_custom_signature_with_an_unknown_agent_class_is_a_config_load_error() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            concat!(
                "[[detection.custom_signatures]]\n",
                "process_name = \"mytool\"\n",
                "agent_class = \"not-a-real-class\"\n",
            ),
        )
        .unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ServerError::ConfigLoad { ref source, .. })
                if matches!(**source, ConfigLoadError::InvalidCustomSignature(_))
        ));
    }

    #[test]
    fn an_unrecognized_session_recovery_policy_is_a_config_load_error() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[session]\nrecovery_policy = \"resore_automatically\"\n",
        )
        .unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ServerError::ConfigLoad { ref source, .. })
                if matches!(**source, ConfigLoadError::InvalidSessionRecoveryPolicy(_))
        ));
    }

    #[test]
    fn session_recovery_policy_accepts_all_three_documented_values() {
        for (value, expected) in [
            (
                "restore_automatically",
                SessionRecoveryConfig::RestoreAutomatically,
            ),
            (
                "ask_before_restore",
                SessionRecoveryConfig::AskBeforeRestore,
            ),
            ("start_fresh", SessionRecoveryConfig::StartFresh),
        ] {
            let dir = scratch_dir();
            std::fs::write(
                dir.join("config.toml"),
                format!("[session]\nrecovery_policy = \"{value}\"\n"),
            )
            .unwrap();

            let config = load(&dir).expect("documented recovery_policy value should load");
            assert_eq!(config.session_recovery, expected);
        }
    }

    #[test]
    fn a_custom_signature_with_an_empty_process_name_is_a_config_load_error() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            concat!(
                "[[detection.custom_signatures]]\n",
                "process_name = \"   \"\n",
                "agent_class = \"other\"\n",
            ),
        )
        .unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ServerError::ConfigLoad { ref source, .. })
                if matches!(**source, ConfigLoadError::InvalidCustomSignature(_))
        ));
    }

    #[test]
    fn session_backups_default_on_and_can_be_disabled() {
        let directory = tempfile::tempdir().expect("config directory");
        assert!(
            load(directory.path())
                .expect("absent config")
                .session_backups_enabled
        );

        std::fs::write(
            directory.path().join("config.toml"),
            "[session]\nrecovery_policy = \"ask_before_restore\"\nbackups_enabled = false\n",
        )
        .expect("write config");
        let config = load(directory.path()).expect("valid config");
        assert!(!config.session_backups_enabled);
        assert_eq!(
            config.session_recovery,
            SessionRecoveryConfig::AskBeforeRestore
        );
    }
}
