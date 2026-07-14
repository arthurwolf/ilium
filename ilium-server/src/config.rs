//! Server-wide configuration, loaded from `~/.config/ilium/config.toml`
//! (see `CLAUDE.md`'s "Config & data locations"): the detection loop's
//! adaptive poll cadence (README "Poll cadence"), user-configured agent
//! detection signatures (README M5), and the desktop notification toggle
//! (README M5, `Working -> Done` notifications). Keybinding and theme
//! config live client-side (`ilium-client/src/config.rs`) since this
//! crate never touches rendering or input dispatch.

use std::borrow::Cow;
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
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            working_poll_interval: Duration::from_secs(5),
            idle_poll_interval: Duration::from_secs(45),
        }
    }
}

/// Whether a pane's `Working -> Done`/`Idle` transition fires a desktop
/// notification (README M5). A separate table from `[detection]` -- polling
/// cadence and "should this ever pop a notification" are independent
/// concerns a user may want to tune separately (e.g. keep fast polling but
/// disable notifications on a headless box with no notification daemon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationsConfig {
    pub enabled: bool,
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
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    pub detection: DetectionConfig,
    pub notifications: NotificationsConfig,
    pub sound: SoundSettings,
    /// User-configured agent signatures (`[[detection.custom_signatures]]`)
    /// to check alongside `ilium-detect`'s built-in registry -- see
    /// `ilium_detect::identify_agent_with_extra`, the registry extension
    /// point this list feeds.
    pub custom_signatures: Vec<AgentSignature>,
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
}

#[derive(Debug, Default, Deserialize)]
struct RawDetectionConfig {
    working_poll_seconds: Option<u64>,
    idle_poll_seconds: Option<u64>,
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

/// Loads `<config_dir>/config.toml`. A missing file is not an error --
/// most sessions never create one -- and loads as [`ServerConfig::default`].
/// A file that exists but fails to read or parse *is* a
/// [`ServerError::ConfigLoad`]; the caller (`main`) logs it and falls back
/// to defaults rather than refusing to start the server over a typo in an
/// optional config file.
pub fn load(config_dir: &Path) -> Result<ServerConfig, ServerError> {
    let path = config_dir.join("config.toml");
    if !path.exists() {
        return Ok(ServerConfig::default());
    }
    let contents = std::fs::read_to_string(&path).map_err(|source| ServerError::ConfigLoad {
        path: path.clone(),
        source: ConfigLoadError::Read(source),
    })?;
    let raw: RawConfig = toml::from_str(&contents).map_err(|source| ServerError::ConfigLoad {
        path: path.clone(),
        source: ConfigLoadError::Parse(source),
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
            source,
        })?;

    Ok(ServerConfig {
        detection,
        notifications: NotificationsConfig::from_raw(raw.notifications),
        sound: raw.sound,
        custom_signatures,
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
            Err(ServerError::ConfigLoad {
                source: ConfigLoadError::InvalidCustomSignature(_),
                ..
            })
        ));
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
            Err(ServerError::ConfigLoad {
                source: ConfigLoadError::InvalidCustomSignature(_),
                ..
            })
        ));
    }
}
