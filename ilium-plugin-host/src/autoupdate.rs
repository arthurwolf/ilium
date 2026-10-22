//! Automatic updates: the `[plugins.auto_update]` config, the check schedule and the decision
//! between a silent install and a notice (install-and-update.md, PKG-300..305).

use std::time::Duration;

use ilium_plugin_api::Diagnostic;

/// The auto-update mode (PKG-300).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoUpdateMode {
    /// Never checks.
    Off,
    /// Checks and shows a notice.
    Notify,
    /// Checks and installs silently when safe.
    Install,
}

/// The auto-update config (PKG-300..302).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoUpdateConfig {
    /// The mode.
    pub mode: AutoUpdateMode,
    /// Whether the operating system reports a metered connection.
    pub os_metered: bool,
    /// Hours between checks, 1..=168.
    pub interval_hours: u32,
    /// Whether the config refuses metered connections.
    pub skip_metered: bool,
}

impl AutoUpdateConfig {
    /// Parses the config TOML; the interval must lie in 1..=168 (PKG-300, 301).
    pub fn from_toml(text: &str) -> Result<AutoUpdateConfig, Diagnostic> {
        #[derive(serde::Deserialize, Default)]
        struct Root {
            #[serde(default)]
            plugins: Config,
        }
        #[derive(serde::Deserialize)]
        #[serde(default)]
        struct Config {
            auto_update: String,
            auto_update_interval_hours: u32,
            metered: bool,
        }
        impl Default for Config {
            fn default() -> Self {
                Self { auto_update: "off".to_owned(), auto_update_interval_hours: 24, metered: false }
            }
        }
        let config = toml::from_str::<Root>(text)
            .map_err(|error| config_error("", error.to_string()))?.plugins;
        let mode = match config.auto_update.as_str() {
            "off" => AutoUpdateMode::Off,
            "notify" => AutoUpdateMode::Notify,
            "install" => AutoUpdateMode::Install,
            _ => return Err(config_error("/plugins/auto_update", "Expected off, notify or install")),
        };
        if !(1..=168).contains(&config.auto_update_interval_hours) {
            return Err(config_error("/plugins/auto_update_interval_hours", "Expected an interval from 1 to 168 hours"));
        }
        Ok(Self { mode, os_metered: false, interval_hours: config.auto_update_interval_hours, skip_metered: config.metered })
    }

    /// Applies the system policy mode when set (PKG-305).
    pub fn with_policy(mut self, policy_mode: Option<AutoUpdateMode>) -> AutoUpdateConfig {
        if let Some(mode) = policy_mode {
            self.mode = mode;
        }
        self
    }
}

/// When checks run (PKG-301).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Whether a check runs at start.
    pub at_start: bool,
    /// Time between checks.
    pub interval: Duration,
    /// The largest random delay added to a check.
    pub max_jitter: Duration,
}

/// The check schedule of a config (PKG-301).
pub fn schedule(config: &AutoUpdateConfig) -> Schedule {
    Schedule {
        at_start: config.mode != AutoUpdateMode::Off,
        interval: Duration::from_secs(u64::from(config.interval_hours) * 3600),
        max_jitter: Duration::from_secs(3600),
    }
}

/// Whether a check may use the network (PKG-300, 302).
pub fn should_check_network(config: &AutoUpdateConfig) -> bool {
    config.mode != AutoUpdateMode::Off && !config.os_metered && !config.skip_metered
}

/// An update found by a check (PKG-303, 304).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCandidate {
    /// The plugin id.
    pub plugin: String,
    /// The installed version.
    pub old_version: String,
    /// The available version.
    pub new_version: String,
    /// Grants the new version adds.
    pub new_grants: Vec<String>,
    /// Whether the new version adds `replaces`.
    pub adds_replaces: bool,
    /// Whether the signing key changed.
    pub key_changed: bool,
    /// Whether the version is on the selected channel.
    pub channel_selected: bool,
    /// The changelog URL.
    pub changelog_url: String,
}

/// A button of an update notice (PKG-304).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    /// Installs the update.
    Update,
    /// Hides the notice for a while.
    Later,
    /// Skips this version.
    SkipThisVersion,
}

/// An update notice (PKG-304).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The plugin id.
    pub plugin: String,
    /// The installed version.
    pub old_version: String,
    /// The available version.
    pub new_version: String,
    /// Grants the new version adds.
    pub new_grants: Vec<String>,
    /// The changelog URL.
    pub changelog_url: String,
    /// The buttons.
    pub actions: Vec<NoticeAction>,
}

/// What a check does with a candidate (PKG-303, 304).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoAction {
    /// Automatic update processing is disabled or suppressed on a metered connection.
    None,
    /// Installs silently.
    Install,
    /// Shows a notice.
    Notice(Notice),
}

/// Decides between a silent install and a notice (PKG-303, 304).
pub fn decide(config: &AutoUpdateConfig, candidate: &UpdateCandidate) -> AutoAction {
    if !should_check_network(config) {
        return AutoAction::None;
    }
    if config.mode == AutoUpdateMode::Install
        && candidate.new_grants.is_empty()
        && !candidate.adds_replaces
        && !candidate.key_changed
        && candidate.channel_selected
    {
        return AutoAction::Install;
    }
    AutoAction::Notice(Notice {
        plugin: candidate.plugin.clone(),
        old_version: candidate.old_version.clone(),
        new_version: candidate.new_version.clone(),
        new_grants: candidate.new_grants.clone(),
        changelog_url: candidate.changelog_url.clone(),
        actions: vec![NoticeAction::Update, NoticeAction::Later, NoticeAction::SkipThisVersion],
    })
}

/// How long `Later` hides a notice (PKG-304).
pub fn later_hides_for() -> Duration {
    Duration::from_secs(7 * 24 * 3600)
}

fn config_error(pointer: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        ilium_plugin_api::Code::from_static(ilium_plugin_api::codes::config::PARSE_ERROR),
        pointer,
        message,
    )
}
