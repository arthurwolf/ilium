//! What triggers an agent's own (native) compaction, as the user configured
//! it, compared with the remote-compaction threshold.
//!
//! Claude Code and Codex compact by themselves once the context reaches a
//! trigger that settings or environment variables can move. When that trigger
//! sits at or below the remote threshold, native compaction always fires
//! first and the remote one never runs; the settings tab warns about it.
//!
//! Detection is read-only. Precedence, highest first: the disable variables,
//! `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, a
//! project-level file, the user-level file, the CLI default. Profile
//! overrides of Codex are listed but not resolved (they apply only to
//! sessions that select the profile).

use std::path::PathBuf;

use ilium_compaction_analysis::semantics::AgentSemantics;
use ilium_compaction_analysis::AgentKind;

use crate::app::App;

use crate::agent_config_writer::{
    read_current, read_overrides, AgentConfigTarget, ApplyWarning, ConfigPaths,
    CLAUDE_AUTO_COMPACT_ENV,
};

pub const CLAUDE_PERCENT_ENV: &str = "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE";
const CLAUDE_DISABLE_ENVIRONMENT: [&str; 2] = ["DISABLE_AUTO_COMPACT", "DISABLE_COMPACT"];

/// Where the value in force comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeSource {
    Environment(String),
    ProjectFile(PathBuf),
    UserFile(PathBuf),
    CliDefault,
}

impl NativeSource {
    pub fn describe(&self) -> String {
        match self {
            Self::Environment(variable) => format!("environment variable {variable}"),
            Self::ProjectFile(path) => format!("project file {}", path.display()),
            Self::UserFile(path) => format!("user file {}", path.display()),
            Self::CliDefault => "the CLI default".to_owned(),
        }
    }
}

/// What the agent was told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLimit {
    /// The CLI default applies.
    Default,
    /// The key or variable, in tokens (`autoCompactWindow`, Codex limit).
    Setting(u64),
    /// A percentage of the window (`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`).
    PercentOfWindow(u8),
    /// Native automatic compaction is switched off.
    Disabled,
}

/// One agent's detected native trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTrigger {
    pub target: AgentConfigTarget,
    pub limit: NativeLimit,
    pub source: NativeSource,
    /// Other places that set the key and apply only in some sessions.
    pub notes: Vec<String>,
}

/// A trigger resolved against one window, next to the remote threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeEvaluation {
    pub window_tokens: u64,
    /// Where the CLI compacts with nothing configured; `None` when the CLI
    /// default is not derivable.
    pub default_trigger_tokens: u64,
    /// Where the CLI compacts now; `None` while native compaction is off.
    pub effective_trigger_tokens: Option<u64>,
    pub remote_trigger_tokens: u64,
    /// The native trigger is at or below the remote one: remote never runs.
    pub is_remote_shadowed: bool,
}

impl NativeEvaluation {
    pub fn effective_percent(&self) -> Option<f64> {
        self.effective_trigger_tokens
            .map(|tokens| percent_of(tokens, self.window_tokens))
    }

    pub fn default_percent(&self) -> f64 {
        percent_of(self.default_trigger_tokens, self.window_tokens)
    }

    /// The highest whole-percent threshold that still triggers first, or
    /// `None` when the native trigger cannot be undercut or is off.
    pub fn highest_working_percent(&self) -> Option<u8> {
        let tokens = self.effective_trigger_tokens?;
        let percent = (tokens.saturating_mul(100) / self.window_tokens.max(1)).saturating_sub(1);
        (percent >= 1).then(|| u8::try_from(percent.min(100)).unwrap_or(100))
    }
}

fn percent_of(tokens: u64, window_tokens: u64) -> f64 {
    tokens as f64 * 100.0 / window_tokens.max(1) as f64
}

pub fn agent_kind(target: AgentConfigTarget) -> AgentKind {
    match target {
        AgentConfigTarget::ClaudeAutoCompactWindow => AgentKind::ClaudeCode,
        AgentConfigTarget::CodexAutoCompactTokenLimit => AgentKind::Codex,
    }
}

/// Window assumed when no pane reports its own.
pub fn default_window_tokens(target: AgentConfigTarget) -> u64 {
    u64::from(agent_kind(target).profile().default_context_window_tokens)
}

fn is_truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

fn parse_tokens(text: &str) -> Option<u64> {
    text.trim().trim_matches('"').replace('_', "").parse().ok()
}

/// Reads the configuration in force for `target`. `environment` looks up one
/// variable of Ilium's own environment.
pub fn detect(
    target: AgentConfigTarget,
    paths: &ConfigPaths,
    environment: &dyn Fn(&str) -> Option<String>,
) -> NativeTrigger {
    let mut notes = Vec::new();
    let overrides = read_overrides(target, paths);
    for warning in &overrides {
        if let ApplyWarning::ProfileOverride { profile, value, .. } = warning {
            notes.push(format!(
                "profile \"{profile}\" sets {value}; it applies only when that profile is selected"
            ));
        }
    }
    let environment_source = |variable: &str| NativeSource::Environment(variable.to_owned());

    if target == AgentConfigTarget::ClaudeAutoCompactWindow {
        for variable in CLAUDE_DISABLE_ENVIRONMENT {
            if environment(variable).is_some_and(|value| is_truthy(&value)) {
                return NativeTrigger {
                    target,
                    limit: NativeLimit::Disabled,
                    source: environment_source(variable),
                    notes,
                };
            }
        }
        if let Some(percent) = environment(CLAUDE_PERCENT_ENV)
            .and_then(|value| parse_tokens(&value))
            .filter(|percent| (1..=100).contains(percent))
        {
            return NativeTrigger {
                target,
                limit: NativeLimit::PercentOfWindow(percent as u8),
                source: environment_source(CLAUDE_PERCENT_ENV),
                notes,
            };
        }
        let from_environment = paths
            .environment
            .get(CLAUDE_AUTO_COMPACT_ENV)
            .cloned()
            .or_else(|| environment(CLAUDE_AUTO_COMPACT_ENV))
            .and_then(|value| parse_tokens(&value));
        if let Some(value) = from_environment {
            return NativeTrigger {
                target,
                limit: NativeLimit::Setting(value),
                source: environment_source(CLAUDE_AUTO_COMPACT_ENV),
                notes,
            };
        }
    }
    for warning in &overrides {
        let ApplyWarning::ProjectOverride { path, value } = warning else {
            continue;
        };
        if let Some(tokens) = parse_tokens(value) {
            return NativeTrigger {
                target,
                limit: NativeLimit::Setting(tokens),
                source: NativeSource::ProjectFile(path.clone()),
                notes,
            };
        }
    }
    if let Ok(current) = read_current(target, paths) {
        if let Some(tokens) = current.value {
            return NativeTrigger {
                target,
                limit: NativeLimit::Setting(tokens),
                source: NativeSource::UserFile(current.path),
                notes,
            };
        }
    }
    NativeTrigger {
        target,
        limit: NativeLimit::Default,
        source: NativeSource::CliDefault,
        notes,
    }
}

/// Resolves `trigger` against a window and the remote threshold.
pub fn evaluate(
    trigger: &NativeTrigger,
    window_tokens: u64,
    threshold_percent: u8,
) -> NativeEvaluation {
    let semantics = AgentSemantics::default_for(agent_kind(trigger.target));
    let window = u32::try_from(window_tokens).unwrap_or(u32::MAX);
    let (default_trigger, from_setting): (u64, Box<dyn Fn(u64) -> u64>) = match semantics {
        AgentSemantics::Claude(_) => (
            u64::from(semantics.setting_to_trigger(window)),
            Box::new(move |setting| {
                u64::from(semantics.setting_to_trigger(u32::try_from(setting).unwrap_or(u32::MAX)))
            }),
        ),
        AgentSemantics::Codex(_) => {
            let codex = AgentSemantics::codex_with_window(window);
            (
                u64::from(codex.cli_defaults(window).trigger_tokens),
                Box::new(move |setting| {
                    u64::from(codex.setting_to_trigger(u32::try_from(setting).unwrap_or(u32::MAX)))
                }),
            )
        }
    };
    let effective = match trigger.limit {
        NativeLimit::Default => Some(default_trigger),
        NativeLimit::Setting(setting) => Some(from_setting(setting)),
        NativeLimit::PercentOfWindow(percent) => Some(window_tokens * u64::from(percent) / 100),
        NativeLimit::Disabled => None,
    };
    let remote = window_tokens * u64::from(threshold_percent) / 100;
    NativeEvaluation {
        window_tokens,
        default_trigger_tokens: default_trigger,
        effective_trigger_tokens: effective,
        remote_trigger_tokens: remote,
        is_remote_shadowed: effective.is_some_and(|tokens| tokens <= remote),
    }
}

/// Both agents' detected triggers, refreshed when the tab opens.
#[derive(Debug, Clone, Default)]
pub struct NativeSnapshot {
    pub triggers: Vec<NativeTrigger>,
    pub is_visible: bool,
}

impl App {
    /// Re-reads the native triggers on every hidden -> visible transition of
    /// the Remote compaction tab.
    pub fn remote_compaction_sync_native(&mut self, is_visible: bool) {
        if is_visible && !self.remote_compaction_native.is_visible {
            self.refresh_remote_compaction_native();
        }
        self.remote_compaction_native.is_visible = is_visible;
    }

    pub fn refresh_remote_compaction_native(&mut self) {
        let environment = |name: &str| std::env::var(name).ok();
        self.remote_compaction_native.triggers = match self.optimization_config_paths() {
            Ok(paths) => [
                AgentConfigTarget::ClaudeAutoCompactWindow,
                AgentConfigTarget::CodexAutoCompactTokenLimit,
            ]
            .into_iter()
            .map(|target| detect(target, &paths, &environment))
            .collect(),
            Err(_) => Vec::new(),
        };
    }
}

/// Human text for the configured value.
pub fn describe_limit(limit: NativeLimit) -> String {
    match limit {
        NativeLimit::Default => "not set".to_owned(),
        NativeLimit::Setting(tokens) => format!("{tokens} tokens"),
        NativeLimit::PercentOfWindow(percent) => format!("{percent}% of the window"),
        NativeLimit::Disabled => "automatic compaction disabled".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(target: AgentConfigTarget, limit: NativeLimit) -> NativeTrigger {
        NativeTrigger {
            target,
            limit,
            source: NativeSource::CliDefault,
            notes: Vec::new(),
        }
    }

    #[test]
    fn claude_default_fires_33k_below_the_window() {
        let claude = trigger(
            AgentConfigTarget::ClaudeAutoCompactWindow,
            NativeLimit::Default,
        );
        let result = evaluate(&claude, 200_000, 65);
        assert_eq!(result.default_trigger_tokens, 167_000);
        assert_eq!(result.effective_trigger_tokens, Some(167_000));
        assert!(!result.is_remote_shadowed);
    }

    #[test]
    fn codex_default_is_ninety_percent() {
        let codex = trigger(
            AgentConfigTarget::CodexAutoCompactTokenLimit,
            NativeLimit::Default,
        );
        let result = evaluate(&codex, 272_000, 65);
        assert_eq!(result.default_trigger_tokens, 244_800);
        assert!(!result.is_remote_shadowed);
    }

    #[test]
    fn a_low_setting_shadows_the_remote_threshold() {
        let claude = trigger(
            AgentConfigTarget::ClaudeAutoCompactWindow,
            NativeLimit::Setting(130_000),
        );
        let result = evaluate(&claude, 200_000, 65);
        assert_eq!(result.effective_trigger_tokens, Some(97_000));
        assert!(result.is_remote_shadowed, "97K <= 130K remote trigger");
        assert_eq!(result.highest_working_percent(), Some(47));
    }

    #[test]
    fn codex_limit_above_the_clamp_is_reduced_to_ninety_percent() {
        let codex = trigger(
            AgentConfigTarget::CodexAutoCompactTokenLimit,
            NativeLimit::Setting(999_999),
        );
        let result = evaluate(&codex, 272_000, 65);
        assert_eq!(result.effective_trigger_tokens, Some(244_800));
    }

    #[test]
    fn percent_override_and_disable_resolve() {
        let percent = trigger(
            AgentConfigTarget::ClaudeAutoCompactWindow,
            NativeLimit::PercentOfWindow(50),
        );
        assert!(evaluate(&percent, 200_000, 65).is_remote_shadowed);
        let disabled = trigger(
            AgentConfigTarget::ClaudeAutoCompactWindow,
            NativeLimit::Disabled,
        );
        let result = evaluate(&disabled, 200_000, 88);
        assert_eq!(result.effective_trigger_tokens, None);
        assert!(!result.is_remote_shadowed);
        assert_eq!(result.highest_working_percent(), None);
    }

    #[test]
    fn equal_triggers_count_as_shadowed() {
        let codex = trigger(
            AgentConfigTarget::CodexAutoCompactTokenLimit,
            NativeLimit::Setting(130_000),
        );
        assert!(evaluate(&codex, 200_000, 65).is_remote_shadowed);
    }

    #[test]
    fn detect_prefers_environment_then_user_file() {
        let directory = std::env::temp_dir().join(format!("ilium-native-{}", std::process::id()));
        let claude_dir = directory.join("claude");
        let codex_dir = directory.join("codex");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            "{\"autoCompactWindow\": 250000}",
        )
        .unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            "model_auto_compact_token_limit = 200000\n",
        )
        .unwrap();
        let paths = ConfigPaths::with_roots(claude_dir, codex_dir, directory.join("lock"));
        let none = |_: &str| None;
        let from_file = detect(AgentConfigTarget::ClaudeAutoCompactWindow, &paths, &none);
        assert_eq!(from_file.limit, NativeLimit::Setting(250_000));
        assert!(matches!(from_file.source, NativeSource::UserFile(_)));
        let codex = detect(AgentConfigTarget::CodexAutoCompactTokenLimit, &paths, &none);
        assert_eq!(codex.limit, NativeLimit::Setting(200_000));

        let environment = |name: &str| (name == CLAUDE_PERCENT_ENV).then(|| "40".to_owned());
        let overridden = detect(
            AgentConfigTarget::ClaudeAutoCompactWindow,
            &paths,
            &environment,
        );
        assert_eq!(overridden.limit, NativeLimit::PercentOfWindow(40));
        let disabled = |name: &str| (name == "DISABLE_AUTO_COMPACT").then(|| "1".to_owned());
        let off = detect(
            AgentConfigTarget::ClaudeAutoCompactWindow,
            &paths,
            &disabled,
        );
        assert_eq!(off.limit, NativeLimit::Disabled);
        std::fs::remove_file(directory.join("claude").join("settings.json")).unwrap();
        std::fs::remove_file(directory.join("codex").join("config.toml")).unwrap();
        std::fs::remove_dir(directory.join("claude")).unwrap();
        std::fs::remove_dir(directory.join("codex")).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
}
