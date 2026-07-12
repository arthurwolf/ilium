//! Server-wide configuration, loaded from `~/.config/illium/config.toml`
//! (see `CLAUDE.md`'s "Config & data locations"): the detection loop's
//! adaptive poll cadence (README "Poll cadence") and the desktop
//! notification toggle (README M5, `Working -> Done` notifications). Other
//! config (keybindings, detection signatures, theme) is explicitly listed
//! under README milestone M5 and out of scope for this crate today.

use std::path::Path;
use std::time::Duration;

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
    /// Poll interval for panes last classified as `Working` -- the state
    /// where a prompt user most wants a quick `Working -> Done` transition
    /// to show up.
    pub working_poll_interval: Duration,
    /// Poll interval for panes last classified as `Idle`, `Done`,
    /// `WaitingApproval`, or plain shells with no agent detected -- nothing
    /// is changing, so polling slowly is both correct and cheap.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServerConfig {
    pub detection: DetectionConfig,
    pub notifications: NotificationsConfig,
}

/// The on-disk shape of `config.toml`. Kept separate from [`ServerConfig`]
/// (which uses `Duration`, has no serde impl by design, and enforces the
/// clamp/default invariants) so a partially-specified or out-of-range
/// config file can be validated in one place ([`DetectionConfig::from_raw`],
/// [`NotificationsConfig::from_raw`]) rather than every field needing its
/// own serde validator.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    detection: RawDetectionConfig,
    #[serde(default)]
    notifications: RawNotificationsConfig,
}

#[derive(Debug, Default, Deserialize)]
struct RawDetectionConfig {
    working_poll_seconds: Option<u64>,
    idle_poll_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawNotificationsConfig {
    enabled: Option<bool>,
}

/// The smallest interval a misconfigured `config.toml` is allowed to
/// request -- see [`DetectionConfig`]'s doc comment.
const MINIMUM_POLL_INTERVAL: Duration = Duration::from_millis(500);

impl DetectionConfig {
    fn from_raw(raw: RawDetectionConfig) -> Self {
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
    Ok(ServerConfig {
        detection: DetectionConfig::from_raw(raw.detection),
        notifications: NotificationsConfig::from_raw(raw.notifications),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-server-config-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn missing_config_file_loads_defaults() {
        let dir = scratch_dir();
        let config = load(&dir).expect("missing file is not an error");
        assert_eq!(config, ServerConfig::default());
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
    fn malformed_toml_is_a_config_load_error_not_a_panic() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "not valid [ toml").unwrap();

        let result = load(&dir);
        assert!(matches!(result, Err(ServerError::ConfigLoad { .. })));
    }
}
