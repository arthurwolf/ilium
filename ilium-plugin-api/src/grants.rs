//! Capability grants and the grant table (reference-capabilities.md).

use serde::{Deserialize, Serialize};

use crate::error::UnknownName;
use crate::manifest::Diagnostic;
use crate::side::Side;
use crate::tier::Tier;

/// One requested capability, for example `tree.read` or `http:api.example.com`
/// (reference-capabilities.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Grant(String);

/// Risk class of a grant: normal grants are low, `H` grants are high (reference-capabilities.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Risk {
    /// Low risk (reference-capabilities.md).
    Low,
    /// Medium risk (reference-capabilities.md).
    Medium,
    /// High risk, asked for once at first activation for T1 (reference-capabilities.md).
    High,
}

impl Risk {
    /// Every risk class, lowest first (reference-capabilities.md).
    pub const ALL: [Risk; 3] = [Risk::Low, Risk::Medium, Risk::High];

    /// `low`, `medium` or `high` (reference-capabilities.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

impl std::fmt::Display for Risk {
    /// `low`, `medium` or `high` (reference-capabilities.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Risk {
    type Err = UnknownName;

    /// Parses `low`, `medium` or `high` (reference-capabilities.md).
    fn from_str(text: &str) -> Result<Risk, UnknownName> {
        Risk::ALL
            .into_iter()
            .find(|risk| risk.as_str() == text)
            .ok_or_else(|| UnknownName::new("risk", text))
    }
}

/// One row of the grant table (reference-capabilities.md "Capability table").
#[derive(Debug, Clone)]
pub struct GrantInfo {
    /// The grant family, for example `http` or `tree.read`.
    pub family: &'static str,
    /// Whether the grant takes an argument after `:`.
    pub takes_argument: bool,
    /// Risk class.
    pub risk: Risk,
    /// Tiers that may request it.
    pub tiers: &'static [Tier],
    /// Sides that may request it.
    pub sides: &'static [Side],
    /// The unstable feature that gates it, if any (ABI-022).
    pub unstable: Option<&'static str>,
    /// The WIT interfaces the grant unlocks (ABI-023).
    pub world_interfaces: &'static [&'static str],
}

/// Parsing and display go through the same validation and text as `Grant::parse` and `as_str`,
/// so `text.parse::<Grant>()` and `grant.to_string()` round-trip the manifest spelling.
impl std::str::FromStr for Grant {
    type Err = Diagnostic;

    fn from_str(text: &str) -> Result<Grant, Diagnostic> {
        Grant::parse(text)
    }
}

impl std::fmt::Display for Grant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Grant {
    /// Parses a grant string; an unknown family is `manifest.grant-unknown` (ABI-023).
    pub fn parse(text: &str) -> Result<Grant, Diagnostic> {
        let family = text.split(':').next().unwrap_or(text);
        if info(family).is_none() {
            return Err(grant_error(
                "manifest.grant-unknown",
                format!("capability {text:?} is not known to plugin API ilium:plugin@0.1.0"),
            ));
        }
        if !(3..=256).contains(&text.chars().count()) {
            return Err(grant_error(
                "manifest.grant-argument",
                "capability must contain 3 to 256 characters",
            ));
        }
        let grant = Grant(text.to_owned());
        validate_argument(&grant)?;
        Ok(grant)
    }

    /// The grant text as written in the manifest (reference-capabilities.md).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The family part before `:` (reference-capabilities.md).
    pub fn family(&self) -> &str {
        self.0
            .split_once(':')
            .map_or(self.0.as_str(), |(family, _)| family)
    }

    /// The argument after `:`, if any (reference-capabilities.md).
    pub fn argument(&self) -> Option<&str> {
        self.0.split_once(':').map(|(_, argument)| argument)
    }

    /// Whether this grant allows what `requested` asks for, for example `*.example.com`
    /// covering `api.example.com` (reference-capabilities.md).
    pub fn covers(&self, requested: &Grant) -> bool {
        if self.family() != requested.family() || info(self.family()).is_none() {
            return false;
        }
        if validate_argument(self).is_err() || validate_argument(requested).is_err() {
            return false;
        }
        if self == requested {
            return true;
        }
        let (Some(held), Some(wanted)) = (self.argument(), requested.argument()) else {
            return false;
        };
        match self.family() {
            "commands.invoke" => {
                held == "*"
                    || held.strip_suffix("/*").is_some_and(|plugin| {
                        wanted.split_once('/').is_some_and(|(owner, command)| {
                            owner == plugin && !command.contains('/')
                        })
                    })
            }
            "topics.subscribe" => {
                held == "*"
                    || held
                        .strip_suffix('*')
                        .is_some_and(|prefix| wanted.starts_with(prefix))
            }
            "http" => host_covers(held, wanted),
            "fs.read" | "fs.write" | "fs.watch" => path_scope_covers(held, wanted),
            _ => false,
        }
    }
}

// Exact families, sides and baseline risks from reference-capabilities.md, Table.
// Argument-dependent risks are applied by is_high_risk rather than widening a row.
macro_rules! row {
    ($family:literal, $argument:literal, $risk:ident, $tiers:expr, $sides:expr, $unstable:expr, $interfaces:expr) => {
        GrantInfo {
            family: $family,
            takes_argument: $argument,
            risk: Risk::$risk,
            tiers: $tiers,
            sides: $sides,
            unstable: $unstable,
            world_interfaces: $interfaces,
        }
    };
}

static GRANTS: &[GrantInfo] = &[
    row!(
        "tree.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/tree@0.1.0"]
    ),
    row!(
        "tree.write",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/tree@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "tree.metadata",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/tree@0.1.0"]
    ),
    row!(
        "tree.decorate",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/tree@0.1.0"]
    ),
    row!(
        "tree.restructure",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/tree@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "tree.attention",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/tree@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "panes.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/panes@0.1.0"]
    ),
    row!(
        "panes.create",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "panes.signal",
        false,
        High,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "panes.lifecycle",
        false,
        High,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "panes.provide-kind",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/panes@0.1.0"]
    ),
    row!(
        "terminal.read-screen",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:primitives/terminal@0.1.0"]
    ),
    row!(
        "terminal.read-cells",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:primitives/terminal@0.1.0"]
    ),
    row!(
        "terminal.history",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:primitives/terminal@0.1.0"]
    ),
    row!(
        "terminal.output-filter",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:primitives/terminal@0.1.0"]
    ),
    row!(
        "terminal.write",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &[
            "ilium:primitives/terminal@0.1.0",
            "ilium:host/intents-host@0.1.0"
        ]
    ),
    row!(
        "terminal.input-lock",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &[
            "ilium:primitives/terminal@0.1.0",
            "ilium:host/intents-host@0.1.0"
        ]
    ),
    row!(
        "terminal.raw-input",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &[
            "ilium:primitives/terminal@0.1.0",
            "ilium:host/intents-host@0.1.0"
        ]
    ),
    row!(
        "terminal.pointer",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:primitives/terminal@0.1.0"]
    ),
    row!(
        "agent.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:primitives/agent@0.1.0"]
    ),
    row!(
        "agent.read-prompts",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:primitives/agent@0.1.0"]
    ),
    row!(
        "agent.control",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &[
            "ilium:primitives/agent@0.1.0",
            "ilium:host/intents-host@0.1.0"
        ]
    ),
    row!(
        "agent.signatures",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:primitives/agent@0.1.0"]
    ),
    row!(
        "agent.transcript",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:primitives/agent@0.1.0"]
    ),
    row!(
        "editor.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:primitives/editor@0.1.0"]
    ),
    row!(
        "editor.edit",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:primitives/editor@0.1.0"]
    ),
    row!(
        "editor.provide",
        false,
        High,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:primitives/editor@0.1.0"]
    ),
    row!(
        "ui.panels",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/ui-host@0.1.0"]
    ),
    row!(
        "ui.chrome",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "keys.intercept",
        false,
        High,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/keys@0.1.0"]
    ),
    row!(
        "keymap.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/keymap@0.1.0"]
    ),
    row!(
        "commands.invoke",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/commands@0.1.0"]
    ),
    row!(
        "fs.read",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client, Side::Scene],
        None,
        &["ilium:host/fs@0.1.0"]
    ),
    row!(
        "fs.write",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client, Side::Scene],
        None,
        &["ilium:host/fs@0.1.0"]
    ),
    row!(
        "fs.watch",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/fs@0.1.0"]
    ),
    row!(
        "fs.pick",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/fs@0.1.0"]
    ),
    row!(
        "jobs.run",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client, Side::Scene],
        None,
        &["ilium:host/jobs@0.1.0"]
    ),
    row!(
        "http",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client, Side::Scene],
        None,
        &["ilium:host/http@0.1.0"]
    ),
    row!(
        "process.run",
        true,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/process@0.1.0"]
    ),
    row!(
        "panes.create-command",
        true,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "process.long-lived",
        false,
        High,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/process@0.1.0"]
    ),
    row!(
        "inference",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/inference@0.1.0"]
    ),
    row!(
        "inference.provide",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/inference@0.1.0"]
    ),
    row!(
        "inference.embed",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/inference@0.1.0"]
    ),
    row!(
        "services.use",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/services@0.1.0"]
    ),
    row!(
        "services.provide",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/services@0.1.0"]
    ),
    row!(
        "sound.play",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/sounds@0.1.0", "ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "notify.desktop",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "notify.toast",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "open-external",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/intents-host@0.1.0"]
    ),
    row!(
        "clipboard.read",
        false,
        High,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/clipboard@0.1.0"]
    ),
    row!(
        "clipboard.write",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/clipboard@0.1.0"]
    ),
    row!(
        "audio.capture",
        false,
        High,
        &Tier::ALL,
        &[Side::Client, Side::Scene],
        None,
        &["ilium:host/audio@0.1.0"]
    ),
    row!(
        "audio.loopback",
        false,
        High,
        &Tier::ALL,
        &[Side::Client, Side::Scene],
        None,
        &["ilium:host/audio@0.1.0"]
    ),
    row!(
        "audio.playback",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client, Side::Scene],
        None,
        &["ilium:host/audio@0.1.0"]
    ),
    row!(
        "media.video",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client, Side::Scene],
        None,
        &["ilium:host/media@0.1.0"]
    ),
    row!(
        "location.observer",
        false,
        High,
        &Tier::ALL,
        &[Side::Client, Side::Scene],
        None,
        &["ilium:host/location@0.1.0"]
    ),
    row!(
        "location.geocode",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client, Side::Scene],
        None,
        &["ilium:host/geocode@0.1.0"]
    ),
    row!(
        "messages.peer",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/topics@0.1.0"]
    ),
    row!(
        "topics.subscribe",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/topics@0.1.0"]
    ),
    row!(
        "sessions.cross",
        false,
        High,
        &[Tier::T1, Tier::T2],
        &[Side::Server],
        None,
        &["ilium:host/sessions@0.1.0"]
    ),
    row!(
        "cli.contribute",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:host/cli@0.1.0"]
    ),
    row!(
        "http-api.route",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:host/http-api@0.1.0"]
    ),
    row!(
        "workspace.manage",
        false,
        High,
        &Tier::ALL,
        &[Side::Server],
        None,
        &["ilium:host/workspace@0.1.0"]
    ),
    row!(
        "settings.read-all",
        false,
        High,
        &[Tier::T1],
        &[Side::Client],
        None,
        &["ilium:host/settings@0.1.0"]
    ),
    row!(
        "settings.write-all",
        false,
        High,
        &[Tier::T1],
        &[Side::Client],
        None,
        &["ilium:host/settings@0.1.0"]
    ),
    row!(
        "settings.contribute",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/settings@0.1.0"]
    ),
    row!(
        "settings.write-kernel",
        true,
        High,
        &[Tier::T1],
        &[Side::Server],
        None,
        &["ilium:host/kernel-settings@0.1.0"]
    ),
    row!(
        "settings.read-kernel",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/kernel-settings@0.1.0"]
    ),
    row!(
        "plugins.manage",
        false,
        High,
        &[Tier::T1],
        &[Side::Client],
        None,
        &["ilium:host/plugins@0.1.0"]
    ),
    row!(
        "help.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Client],
        None,
        &["ilium:host/help@0.1.0"]
    ),
    row!(
        "journal.read",
        false,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/journal@0.1.0"]
    ),
    row!(
        "gpu.kernels",
        false,
        Low,
        &Tier::ALL,
        &[Side::Scene],
        Some("gpu-kernels"),
        &["ilium:host/gpu@0.1.0"]
    ),
    row!(
        "extensions.provide",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client, Side::Scene],
        None,
        &["ilium:host/extensions@0.1.0"]
    ),
    row!(
        "extensions.contribute",
        true,
        Low,
        &Tier::ALL,
        &[Side::Server, Side::Client],
        None,
        &["ilium:host/extensions@0.1.0"]
    ),
];

/// The grant table of this API version (reference-capabilities.md).
pub fn table() -> &'static [GrantInfo] {
    GRANTS
}

/// The table row of a grant family (reference-capabilities.md).
pub fn info(family: &str) -> Option<&'static GrantInfo> {
    table().iter().find(|entry| entry.family == family)
}

/// Returns the diagnostic code when the grant is not allowed for this tier and side
/// (reference-manifest-schema.md `manifest.grant-tier`, `manifest.grant-side`).
pub fn check_grant(grant: &Grant, tier: Tier, side: Side) -> Result<(), Diagnostic> {
    let Some(entry) = info(grant.family()) else {
        return Err(grant_error("manifest.grant-unknown", "unknown capability"));
    };
    if entry.family == "editor.provide" || entry.unstable.is_some() && !cfg!(debug_assertions) {
        return Err(grant_error(
            "manifest.grant-unknown",
            "reserved or development-only capability",
        ));
    }
    validate_argument(grant)?;
    if !entry.tiers.contains(&tier)
        || tier != Tier::T1
            && grant
                .argument()
                .is_some_and(|argument| argument.starts_with("path:/"))
    {
        return Err(grant_error(
            "manifest.grant-tier",
            format!(
                "capability {:?} is not available at tier {tier}",
                grant.family()
            ),
        ));
    }
    if !entry.sides.contains(&side) {
        return Err(grant_error(
            "manifest.grant-side",
            format!(
                "capability {:?} is not available on the {side} side",
                grant.family()
            ),
        ));
    }
    Ok(())
}

/// The highest risk of a grant list (reference-capabilities.md).
pub fn risk_of(grants: &[Grant]) -> Risk {
    grants
        .iter()
        .map(|grant| {
            if is_high_risk(grant.as_str()) {
                Risk::High
            } else {
                Risk::Low
            }
        })
        .max()
        .unwrap_or(Risk::Low)
}

/// Whether a grant name is high risk (reference-capabilities.md, api-panes.md PAN-032).
pub fn is_high_risk(grant: &str) -> bool {
    let (family, argument) = grant.split_once(':').map_or((grant, ""), |parts| parts);
    if info(family).is_some_and(|entry| entry.risk == Risk::High) {
        return true;
    }
    match family {
        "commands.invoke" | "topics.subscribe" => argument == "*",
        "fs.write" => !matches!(argument, "data" | "cache"),
        "fs.read" | "fs.watch" => {
            argument.starts_with("agent-sessions:")
                || argument.starts_with("agent-config:")
                || argument.starts_with("path:/")
        }
        "settings.read-kernel" => argument == "api",
        "http" => host_parts(argument).is_some_and(|(host, _)| private_host(host)),
        _ => false,
    }
}

fn grant_error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(crate::error::Code::from_static(code), "", message)
}

fn validate_argument(grant: &Grant) -> Result<(), Diagnostic> {
    let Some(entry) = info(grant.family()) else {
        return Err(grant_error("manifest.grant-unknown", "unknown capability"));
    };
    let argument = match (entry.takes_argument, grant.argument()) {
        (false, None) => return Ok(()),
        (false, Some(_)) => {
            return Err(grant_error(
                "manifest.grant-argument",
                "capability takes no argument",
            ))
        }
        (true, None) => {
            return Err(grant_error(
                "manifest.grant-argument-missing",
                "capability needs an argument",
            ))
        }
        (true, Some(argument)) => argument,
    };
    let family = grant.family();
    if matches!(family, "process.run" | "panes.create-command") {
        return if valid_program(argument) {
            Ok(())
        } else {
            Err(grant_error(
                "manifest.grant-program",
                "capability takes a program name or an absolute path",
            ))
        };
    }
    if argument.is_empty()
        || argument
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(grant_error(
            "manifest.grant-argument",
            "invalid capability argument",
        ));
    }
    let wildcard_allowed = match family {
        "commands.invoke" => {
            argument == "*" || argument.strip_suffix("/*").is_some_and(valid_plugin_id)
        }
        "topics.subscribe" => {
            argument == "*"
                || argument
                    .strip_suffix("/*")
                    .is_some_and(|prefix| !prefix.is_empty() && !prefix.contains('*'))
        }
        "http" => argument
            .strip_prefix("*.")
            .is_some_and(|host| host_parts(host).is_some_and(|(host, _)| valid_dns_name(host))),
        "fs.read" | "fs.write" | "fs.watch" => argument
            .strip_prefix("path:")
            .and_then(|path| path.strip_suffix("/**"))
            .is_some_and(|path| !path.contains('*')),
        _ => false,
    };
    if argument.contains('*') && !wildcard_allowed {
        let code = if family == "settings.write-kernel" {
            "manifest.grant-argument"
        } else {
            "manifest.grant-wildcard"
        };
        return Err(grant_error(
            code,
            "wildcard is not allowed in this position",
        ));
    }
    let valid = match family {
        "fs.read" | "fs.write" | "fs.watch" => {
            if !valid_scope(family, argument) {
                return Err(grant_error(
                    "manifest.grant-scope",
                    "unknown file scope or access refused for this scope",
                ));
            }
            true
        }
        "http" => valid_host_argument(argument),
        "commands.invoke" => argument == "*" || valid_global_id(argument, true),
        "topics.subscribe" => true,
        "messages.peer" => valid_plugin_id(argument),
        // CAP-001 defines this as opaque text, with later colons retained. The
        // provider checks existence; a version suffix is not a service name.
        "services.use" | "services.provide" => !argument.contains('@'),
        "extensions.provide" | "extensions.contribute" => valid_global_id(argument, false),
        "settings.contribute" | "settings.read-kernel" | "settings.write-kernel" => {
            KERNEL_TABS.contains(&argument)
        }
        "http-api.route" => valid_route(argument),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(grant_error(
            "manifest.grant-argument",
            "invalid capability argument",
        ))
    }
}

const KERNEL_TABS: &[&str] = &[
    "user-interface",
    "icons",
    "agent-monitoring",
    "agent-cost",
    "optimization",
    "remote-compaction",
    "keyboard",
    "terminal",
    "editor",
    "session",
    "git",
    "kanban-board",
    "animations",
    "sound",
    "voice-control",
    "reset-planning",
    "inference",
    "llm-instructions",
    "titles",
    "triggers",
    "text-triggers",
    "debug",
    "api",
    "about",
    "setup",
];

fn valid_plugin_id(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn valid_global_id(text: &str, allow_wildcard: bool) -> bool {
    let Some((plugin, local)) = text.split_once('/') else {
        return false;
    };
    valid_plugin_id(plugin)
        && !local.is_empty()
        && (allow_wildcard && local == "*"
            || local
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
}

fn valid_program(program: &str) -> bool {
    !program.is_empty()
        && !program
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        && !program.contains(['*', '?', '\\'])
        && !program
            .split('/')
            .any(|component| matches!(component, "." | ".."))
        && (!program.contains('/') || program.starts_with('/'))
        && program != "/"
        && !program.ends_with('/')
}

fn valid_scope(family: &str, scope: &str) -> bool {
    match scope {
        "package" => family != "fs.write",
        "data" | "cache" | "project" | "worktrees" | "picked" | "minecraft-saves" => true,
        _ => {
            if let Some(agent) = scope.strip_prefix("agent-sessions:") {
                return family != "fs.write" && AGENTS.contains(&agent);
            }
            if let Some(agent) = scope.strip_prefix("agent-config:") {
                return family != "fs.watch" && AGENTS.contains(&agent);
            }
            scope.strip_prefix("path:").is_some_and(valid_path_template)
        }
    }
}

const AGENTS: &[&str] = &["claude", "codex", "gemini", "antigravity"];

fn valid_path_template(path: &str) -> bool {
    let root_valid = path.starts_with('/')
        || [
            "{home}",
            "{config}",
            "{data}",
            "{cache}",
            "{project}",
            "{worktrees}",
        ]
        .iter()
        .any(|root| {
            path.strip_prefix(root)
                .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
        });
    if !root_valid || path.contains(['\\', '\0']) {
        return false;
    }
    let Some(root_end) = path
        .find('}')
        .map(|end| end + 1)
        .or_else(|| path.starts_with('/').then_some(0))
    else {
        return false;
    };
    let tail = &path[root_end..];
    if tail.contains(['{', '}']) {
        return false;
    }
    let body = path.strip_suffix("/**").unwrap_or(path);
    !body.is_empty()
        && !body.contains('*')
        && !body.ends_with('/')
        && body
            .split('/')
            .all(|component| !matches!(component, "." | ".."))
}

fn valid_route(route: &str) -> bool {
    route.starts_with('/')
        && !route.contains(['?', '#', '\\', '*'])
        && route.split('/').skip(1).all(|part| {
            if part.starts_with('{') {
                part.strip_prefix('{')
                    .and_then(|part| part.strip_suffix('}'))
                    .is_some_and(|name| {
                        !name.is_empty()
                            && name
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    })
            } else {
                !part.contains(['{', '}']) && !matches!(part, "." | "..")
            }
        })
}

fn host_parts(argument: &str) -> Option<(&str, Option<u16>)> {
    if let Some(rest) = argument.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        host.parse::<std::net::Ipv6Addr>().ok()?;
        let port = if tail.is_empty() {
            None
        } else {
            Some(
                tail.strip_prefix(':')?
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port > 0)?,
            )
        };
        return Some((host, port));
    }
    match argument.split_once(':') {
        Some((host, port)) => Some((
            host,
            Some(port.parse::<u16>().ok().filter(|port| *port > 0)?),
        )),
        None => Some((argument, None)),
    }
}

fn valid_dns_name(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_host_argument(argument: &str) -> bool {
    if let Some(key) = argument.strip_prefix("setting:") {
        return !key.is_empty()
            && key
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    }
    let Some((host, port)) = host_parts(argument.strip_prefix("*.").unwrap_or(argument)) else {
        return false;
    };
    let address = host.parse::<std::net::IpAddr>().ok();
    if address.is_none() && !valid_dns_name(host) {
        return false;
    }
    let loopback = host.eq_ignore_ascii_case("localhost")
        || address.is_some_and(|address| address.is_loopback());
    if argument.starts_with("*.") && (address.is_some() || loopback) {
        return false;
    }
    !loopback || port.is_some()
}

fn private_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(address)) => {
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(address)) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

fn host_covers(held: &str, wanted: &str) -> bool {
    // Settings are symbolic grants, never permissions for a guessed resolved host.
    if held.starts_with("setting:") || wanted.starts_with("setting:") {
        return false;
    }
    let Some(suffix) = held.strip_prefix("*.") else {
        return false;
    };
    let Some((suffix, port)) = host_parts(suffix) else {
        return false;
    };
    let Some((host, wanted_port)) = host_parts(wanted.strip_prefix("*.").unwrap_or(wanted)) else {
        return false;
    };
    if port != wanted_port {
        return false;
    }
    let host = host.to_ascii_lowercase();
    let suffix = suffix.to_ascii_lowercase();
    host.strip_suffix(&format!(".{suffix}"))
        .is_some_and(|prefix| !prefix.is_empty())
}

fn path_scope_covers(held: &str, wanted: &str) -> bool {
    let (Some(held), Some(wanted)) = (held.strip_prefix("path:"), wanted.strip_prefix("path:"))
    else {
        return false;
    };
    if let Some(root) = held.strip_suffix("/**") {
        let wanted = wanted.strip_suffix("/**").unwrap_or(wanted);
        return wanted == root
            || wanted
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('/'));
    }
    // A non-recursive scope covers its own root and direct children only. It
    // cannot cover another directory grant, which would add grandchildren.
    false
}
