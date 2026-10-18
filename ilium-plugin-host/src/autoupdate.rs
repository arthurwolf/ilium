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
    pub fn from_toml(_text: &str) -> Result<AutoUpdateConfig, Diagnostic> {
        unimplemented!("host::autoupdate::AutoUpdateConfig::from_toml")
    }

    /// Applies the system policy mode when set (PKG-305).
    pub fn with_policy(self, _policy_mode: Option<AutoUpdateMode>) -> AutoUpdateConfig {
        unimplemented!("host::autoupdate::AutoUpdateConfig::with_policy")
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
pub fn schedule(_config: &AutoUpdateConfig) -> Schedule {
    unimplemented!("host::autoupdate::schedule")
}

/// Whether a check may use the network (PKG-300, 302).
pub fn should_check_network(_config: &AutoUpdateConfig) -> bool {
    unimplemented!("host::autoupdate::should_check_network")
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
    /// Installs silently.
    Install,
    /// Shows a notice.
    Notice(Notice),
}

/// Decides between a silent install and a notice (PKG-303, 304).
pub fn decide(_config: &AutoUpdateConfig, _candidate: &UpdateCandidate) -> AutoAction {
    unimplemented!("host::autoupdate::decide")
}

/// How long `Later` hides a notice (PKG-304).
pub fn later_hides_for() -> Duration {
    unimplemented!("host::autoupdate::later_hides_for")
}
