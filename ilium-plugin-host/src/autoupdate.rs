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
                Self {
                    auto_update: "off".to_owned(),
                    auto_update_interval_hours: 24,
                    metered: false,
                }
            }
        }
        let config = toml::from_str::<Root>(text)
            .map_err(|error| config_error("", error.to_string()))?
            .plugins;
        let mode = match config.auto_update.as_str() {
            "off" => AutoUpdateMode::Off,
            "notify" => AutoUpdateMode::Notify,
            "install" => AutoUpdateMode::Install,
            _ => {
                return Err(config_error(
                    "/plugins/auto_update",
                    "Expected off, notify or install",
                ))
            }
        };
        if !(1..=168).contains(&config.auto_update_interval_hours) {
            return Err(config_error(
                "/plugins/auto_update_interval_hours",
                "Expected an interval from 1 to 168 hours",
            ));
        }
        Ok(Self {
            mode,
            os_metered: false,
            interval_hours: config.auto_update_interval_hours,
            skip_metered: config.metered,
        })
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
        actions: vec![
            NoticeAction::Update,
            NoticeAction::Later,
            NoticeAction::SkipThisVersion,
        ],
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ilium_plugin_api::{codes, Code, Diagnostic, Severity};

    use super::{
        decide, later_hides_for, schedule, should_check_network, AutoAction, AutoUpdateConfig,
        AutoUpdateMode, NoticeAction, UpdateCandidate,
    };

    /// A config with the given mode and OS metered flag; other fields at their defaults.
    fn config_with(mode: AutoUpdateMode, os_metered: bool) -> AutoUpdateConfig {
        AutoUpdateConfig {
            mode,
            os_metered,
            interval_hours: 24,
            skip_metered: false,
        }
    }

    /// An update that needs no new grants, adds no `replaces`, keeps the key and stays on the
    /// selected channel: the only kind of update that auto-install may apply silently (PKG-302).
    fn clean_candidate() -> UpdateCandidate {
        UpdateCandidate {
            plugin: "com.example.notes".to_string(),
            old_version: "1.0.0".to_string(),
            new_version: "1.1.0".to_string(),
            new_grants: Vec::new(),
            adds_replaces: false,
            key_changed: false,
            channel_selected: true,
            changelog_url: "https://example.com/notes/changelog".to_string(),
        }
    }

    /// Unwraps the error of a config parse, failing the test when parsing succeeded.
    fn expect_config_error(result: Result<AutoUpdateConfig, Diagnostic>) -> Diagnostic {
        match result {
            Ok(config) => panic!("expected a config error, got {config:?}"),
            Err(diagnostic) => diagnostic,
        }
    }

    /// REQ: PKG-300
    #[test]
    fn empty_config_text_yields_auto_update_off_with_daily_interval() {
        let config = AutoUpdateConfig::from_toml("").expect("empty config must parse");
        assert_eq!(config.mode, AutoUpdateMode::Off);
        assert_eq!(config.interval_hours, 24);
        assert!(!config.os_metered);
    }

    /// REQ: PKG-300
    #[test]
    fn from_toml_reads_notify_mode() {
        let config = AutoUpdateConfig::from_toml("[plugins]\nauto_update = \"notify\"\n")
            .expect("notify must parse");
        assert_eq!(config.mode, AutoUpdateMode::Notify);
    }

    /// REQ: PKG-300
    #[test]
    fn from_toml_reads_install_mode() {
        let config = AutoUpdateConfig::from_toml("[plugins]\nauto_update = \"install\"\n")
            .expect("install must parse");
        assert_eq!(config.mode, AutoUpdateMode::Install);
    }

    /// REQ: PKG-300
    #[test]
    fn from_toml_rejects_unknown_mode_value() {
        let diagnostic = expect_config_error(AutoUpdateConfig::from_toml(
            "[plugins]\nauto_update = \"always\"\n",
        ));
        assert_eq!(diagnostic.severity, Severity::Error);
    }

    /// REQ: PKG-301
    #[test]
    fn from_toml_reads_interval_hours() {
        let config = AutoUpdateConfig::from_toml("[plugins]\nauto_update_interval_hours = 6\n")
            .expect("interval 6 must parse");
        assert_eq!(config.interval_hours, 6);
    }

    /// REQ: PKG-301
    #[test]
    fn from_toml_accepts_interval_at_both_bounds() {
        let lower = AutoUpdateConfig::from_toml("[plugins]\nauto_update_interval_hours = 1\n")
            .expect("interval 1 is the lower bound");
        let upper = AutoUpdateConfig::from_toml("[plugins]\nauto_update_interval_hours = 168\n")
            .expect("interval 168 is the upper bound");
        assert_eq!(lower.interval_hours, 1);
        assert_eq!(upper.interval_hours, 168);
    }

    /// REQ: PKG-301
    #[test]
    fn from_toml_rejects_interval_zero() {
        let diagnostic = expect_config_error(AutoUpdateConfig::from_toml(
            "[plugins]\nauto_update_interval_hours = 0\n",
        ));
        assert_eq!(diagnostic.severity, Severity::Error);
    }

    /// REQ: PKG-301
    #[test]
    fn from_toml_rejects_interval_above_168() {
        let diagnostic = expect_config_error(AutoUpdateConfig::from_toml(
            "[plugins]\nauto_update_interval_hours = 169\n",
        ));
        assert_eq!(diagnostic.severity, Severity::Error);
    }

    /// REQ: PKG-300
    #[test]
    fn from_toml_refuses_unknown_plugins_key_with_config_unknown_key() {
        let diagnostic = expect_config_error(AutoUpdateConfig::from_toml(
            "[plugins]\nauto_update_frequency = \"daily\"\n",
        ));
        assert_eq!(
            diagnostic.code,
            Code::from_static(codes::config::UNKNOWN_KEY)
        );
    }

    /// REQ: COR-090
    #[test]
    fn from_toml_unparseable_text_yields_config_parse_error() {
        let diagnostic =
            expect_config_error(AutoUpdateConfig::from_toml("[plugins\nauto_update ="));
        assert_eq!(
            diagnostic.code,
            Code::from_static(codes::config::PARSE_ERROR)
        );
    }

    /// REQ: PKG-305
    #[test]
    fn with_policy_none_keeps_the_configured_mode() {
        let config = config_with(AutoUpdateMode::Install, false).with_policy(None);
        assert_eq!(config.mode, AutoUpdateMode::Install);
    }

    /// REQ: PKG-305
    #[test]
    fn with_policy_overrides_the_user_mode() {
        let config =
            config_with(AutoUpdateMode::Notify, false).with_policy(Some(AutoUpdateMode::Off));
        assert_eq!(config.mode, AutoUpdateMode::Off);
    }

    /// REQ: PKG-305
    #[test]
    fn with_policy_off_locks_an_install_mode_to_off() {
        let config =
            config_with(AutoUpdateMode::Install, false).with_policy(Some(AutoUpdateMode::Off));
        assert_eq!(config.mode, AutoUpdateMode::Off);
    }

    /// REQ: PKG-301
    #[test]
    fn schedule_checks_at_start_every_interval_with_one_hour_jitter() {
        let planned = schedule(&config_with(AutoUpdateMode::Install, false));
        assert!(planned.at_start);
        assert_eq!(planned.interval, Duration::from_secs(24 * 3600));
        assert_eq!(planned.max_jitter, Duration::from_secs(3600));
    }

    /// REQ: PKG-301
    #[test]
    fn schedule_interval_follows_configured_hours_at_both_bounds() {
        let mut hourly = config_with(AutoUpdateMode::Notify, false);
        hourly.interval_hours = 1;
        let mut weekly = config_with(AutoUpdateMode::Notify, false);
        weekly.interval_hours = 168;
        assert_eq!(schedule(&hourly).interval, Duration::from_secs(3600));
        assert_eq!(schedule(&weekly).interval, Duration::from_secs(168 * 3600));
    }

    /// REQ: PKG-300
    #[test]
    fn off_mode_never_uses_the_network() {
        assert!(!should_check_network(&config_with(
            AutoUpdateMode::Off,
            false
        )));
    }

    /// REQ: PKG-300
    #[test]
    fn notify_and_install_modes_may_use_the_network_on_an_unmetered_connection() {
        assert!(should_check_network(&config_with(
            AutoUpdateMode::Notify,
            false
        )));
        assert!(should_check_network(&config_with(
            AutoUpdateMode::Install,
            false
        )));
    }

    /// REQ: PKG-303
    #[test]
    fn metered_connection_blocks_checks_in_both_active_modes() {
        assert!(!should_check_network(&config_with(
            AutoUpdateMode::Notify,
            true
        )));
        assert!(!should_check_network(&config_with(
            AutoUpdateMode::Install,
            true
        )));
    }

    /// REQ: PKG-302
    #[test]
    fn clean_candidate_in_install_mode_installs_silently() {
        let action = decide(
            &config_with(AutoUpdateMode::Install, false),
            &clean_candidate(),
        );
        assert_eq!(action, AutoAction::Install);
    }

    /// REQ: PKG-302
    #[test]
    fn install_mode_with_new_grants_produces_a_notice() {
        let mut candidate = clean_candidate();
        candidate.new_grants = vec!["fs.read:project".to_string()];
        let action = decide(&config_with(AutoUpdateMode::Install, false), &candidate);
        assert!(matches!(action, AutoAction::Notice(_)));
    }

    /// REQ: PKG-302
    #[test]
    fn install_mode_with_added_replaces_produces_a_notice() {
        let mut candidate = clean_candidate();
        candidate.adds_replaces = true;
        let action = decide(&config_with(AutoUpdateMode::Install, false), &candidate);
        assert!(matches!(action, AutoAction::Notice(_)));
    }

    /// REQ: PKG-302
    #[test]
    fn install_mode_with_changed_publisher_key_produces_a_notice() {
        let mut candidate = clean_candidate();
        candidate.key_changed = true;
        let action = decide(&config_with(AutoUpdateMode::Install, false), &candidate);
        assert!(matches!(action, AutoAction::Notice(_)));
    }

    /// REQ: PKG-302
    #[test]
    fn install_mode_with_unselected_channel_produces_a_notice() {
        let mut candidate = clean_candidate();
        candidate.channel_selected = false;
        let action = decide(&config_with(AutoUpdateMode::Install, false), &candidate);
        assert!(matches!(action, AutoAction::Notice(_)));
    }

    /// REQ: PKG-300
    #[test]
    fn notify_mode_shows_a_notice_even_for_a_clean_candidate() {
        let action = decide(
            &config_with(AutoUpdateMode::Notify, false),
            &clean_candidate(),
        );
        assert!(matches!(action, AutoAction::Notice(_)));
    }

    /// REQ: PKG-304
    #[test]
    fn notice_copies_candidate_fields_and_offers_update_later_and_skip() {
        let mut candidate = clean_candidate();
        candidate.new_grants = vec!["net.http:example.com".to_string()];
        let action = decide(&config_with(AutoUpdateMode::Notify, false), &candidate);
        let AutoAction::Notice(notice) = action else {
            panic!("expected a notice, got {action:?}");
        };
        assert_eq!(notice.plugin, "com.example.notes");
        assert_eq!(notice.old_version, "1.0.0");
        assert_eq!(notice.new_version, "1.1.0");
        assert_eq!(notice.new_grants, vec!["net.http:example.com".to_string()]);
        assert_eq!(notice.changelog_url, "https://example.com/notes/changelog");
        assert_eq!(
            notice.actions,
            vec![
                NoticeAction::Update,
                NoticeAction::Later,
                NoticeAction::SkipThisVersion
            ]
        );
    }

    /// REQ: PKG-304
    #[test]
    fn notice_for_update_without_grants_has_an_empty_grant_list() {
        let action = decide(
            &config_with(AutoUpdateMode::Notify, false),
            &clean_candidate(),
        );
        let AutoAction::Notice(notice) = action else {
            panic!("expected a notice, got {action:?}");
        };
        assert!(notice.new_grants.is_empty());
    }

    /// REQ: PKG-304
    #[test]
    fn later_hides_a_notice_for_seven_days() {
        assert_eq!(later_hides_for(), Duration::from_secs(7 * 24 * 3600));
    }
}
