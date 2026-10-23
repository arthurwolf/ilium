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
            return Err(grant_error("manifest.grant-argument", "capability must contain 3 to 256 characters"));
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
    row!("tree.read", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/tree@0.1.0"]),
    row!("tree.write", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/tree@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("tree.metadata", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/tree@0.1.0"]),
    row!("tree.decorate", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/tree@0.1.0"]),
    row!("tree.restructure", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/tree@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("tree.attention", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/tree@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("panes.read", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/panes@0.1.0"]),
    row!("panes.create", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("panes.signal", false, High, &Tier::ALL, &[Side::Server], None, &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("panes.lifecycle", false, High, &Tier::ALL, &[Side::Server], None, &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("panes.provide-kind", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:host/panes@0.1.0"]),
    row!("terminal.read-screen", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/terminal@0.1.0"]),
    row!("terminal.read-cells", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/terminal@0.1.0"]),
    row!("terminal.history", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/terminal@0.1.0"]),
    row!("terminal.output-filter", false, Low, &Tier::ALL, &[Side::Server], None, &["ilium:primitives/terminal@0.1.0"]),
    row!("terminal.write", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/terminal@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("terminal.input-lock", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/terminal@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("terminal.raw-input", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/terminal@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("terminal.pointer", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:primitives/terminal@0.1.0"]),
    row!("agent.read", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/agent@0.1.0"]),
    row!("agent.read-prompts", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/agent@0.1.0"]),
    row!("agent.control", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/agent@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("agent.signatures", false, Low, &Tier::ALL, &[Side::Server], None, &["ilium:primitives/agent@0.1.0"]),
    row!("agent.transcript", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:primitives/agent@0.1.0"]),
    row!("editor.read", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:primitives/editor@0.1.0"]),
    row!("editor.edit", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:primitives/editor@0.1.0"]),
    row!("editor.provide", false, High, &Tier::ALL, &[Side::Client], None, &["ilium:primitives/editor@0.1.0"]),
    row!("ui.panels", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:host/ui-host@0.1.0"]),
    row!("ui.chrome", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/intents-host@0.1.0"]),
    row!("keys.intercept", false, High, &Tier::ALL, &[Side::Client], None, &["ilium:host/keys@0.1.0"]),
    row!("keymap.read", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:host/keymap@0.1.0"]),
    row!("commands.invoke", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/commands@0.1.0"]),
    row!("fs.read", true, Low, &Tier::ALL, &[Side::Server, Side::Client, Side::Scene], None, &["ilium:host/fs@0.1.0"]),
    row!("fs.write", true, Low, &Tier::ALL, &[Side::Server, Side::Client, Side::Scene], None, &["ilium:host/fs@0.1.0"]),
    row!("fs.watch", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/fs@0.1.0"]),
    row!("fs.pick", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:host/fs@0.1.0"]),
    row!("jobs.run", false, Low, &Tier::ALL, &[Side::Server, Side::Client, Side::Scene], None, &["ilium:host/jobs@0.1.0"]),
    row!("http", true, Low, &Tier::ALL, &[Side::Server, Side::Client, Side::Scene], None, &["ilium:host/http@0.1.0"]),
    row!("process.run", true, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/process@0.1.0"]),
    row!("panes.create-command", true, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/panes@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("process.long-lived", false, High, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/process@0.1.0"]),
    row!("inference", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/inference@0.1.0"]),
    row!("inference.provide", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/inference@0.1.0"]),
    row!("inference.embed", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/inference@0.1.0"]),
    row!("services.use", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/services@0.1.0"]),
    row!("services.provide", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/services@0.1.0"]),
    row!("sound.play", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/sounds@0.1.0", "ilium:host/intents-host@0.1.0"]),
    row!("notify.desktop", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/intents-host@0.1.0"]),
    row!("notify.toast", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/intents-host@0.1.0"]),
    row!("open-external", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/intents-host@0.1.0"]),
    row!("clipboard.read", false, High, &Tier::ALL, &[Side::Client], None, &["ilium:host/clipboard@0.1.0"]),
    row!("clipboard.write", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:host/clipboard@0.1.0"]),
    row!("audio.capture", false, High, &Tier::ALL, &[Side::Client, Side::Scene], None, &["ilium:host/audio@0.1.0"]),
    row!("audio.loopback", false, High, &Tier::ALL, &[Side::Client, Side::Scene], None, &["ilium:host/audio@0.1.0"]),
    row!("audio.playback", false, Low, &Tier::ALL, &[Side::Client, Side::Scene], None, &["ilium:host/audio@0.1.0"]),
    row!("media.video", false, Low, &Tier::ALL, &[Side::Client, Side::Scene], None, &["ilium:host/media@0.1.0"]),
    row!("location.observer", false, High, &Tier::ALL, &[Side::Client, Side::Scene], None, &["ilium:host/location@0.1.0"]),
    row!("location.geocode", false, Low, &Tier::ALL, &[Side::Client, Side::Scene], None, &["ilium:host/geocode@0.1.0"]),
    row!("messages.peer", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/topics@0.1.0"]),
    row!("topics.subscribe", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/topics@0.1.0"]),
    row!("sessions.cross", false, High, &[Tier::T1, Tier::T2], &[Side::Server], None, &["ilium:host/sessions@0.1.0"]),
    row!("cli.contribute", false, Low, &Tier::ALL, &[Side::Server], None, &["ilium:host/cli@0.1.0"]),
    row!("http-api.route", true, Low, &Tier::ALL, &[Side::Server], None, &["ilium:host/http-api@0.1.0"]),
    row!("workspace.manage", false, High, &Tier::ALL, &[Side::Server], None, &["ilium:host/workspace@0.1.0"]),
    row!("settings.read-all", false, High, &[Tier::T1], &[Side::Client], None, &["ilium:host/settings@0.1.0"]),
    row!("settings.write-all", false, High, &[Tier::T1], &[Side::Client], None, &["ilium:host/settings@0.1.0"]),
    row!("settings.contribute", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/settings@0.1.0"]),
    row!("settings.write-kernel", true, High, &[Tier::T1], &[Side::Server], None, &["ilium:host/kernel-settings@0.1.0"]),
    row!("settings.read-kernel", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/kernel-settings@0.1.0"]),
    row!("plugins.manage", false, High, &[Tier::T1], &[Side::Client], None, &["ilium:host/plugins@0.1.0"]),
    row!("help.read", false, Low, &Tier::ALL, &[Side::Client], None, &["ilium:host/help@0.1.0"]),
    row!("journal.read", false, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/journal@0.1.0"]),
    row!("gpu.kernels", false, Low, &Tier::ALL, &[Side::Scene], Some("gpu-kernels"), &["ilium:host/gpu@0.1.0"]),
    row!("extensions.provide", true, Low, &Tier::ALL, &[Side::Server, Side::Client, Side::Scene], None, &["ilium:host/extensions@0.1.0"]),
    row!("extensions.contribute", true, Low, &Tier::ALL, &[Side::Server, Side::Client], None, &["ilium:host/extensions@0.1.0"]),
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
    if !entry.tiers.contains(&tier) || tier != Tier::T1 && grant.argument().is_some_and(|argument| argument.starts_with("path:/")) {
        return Err(grant_error("manifest.grant-tier", format!("capability {:?} is not available at tier {tier}", grant.family())));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::manifest;
    use crate::side::Side;
    use crate::tier::Tier;
    use std::collections::HashSet;

    /// Builds a grant value directly, so tests of other functions do not depend on `parse`.
    fn grant(text: &str) -> Grant {
        Grant(text.to_owned())
    }

    /// Parses a grant the manual calls valid; a refusal fails the test and shows its diagnostic.
    fn parsed(text: &str) -> Grant {
        match Grant::parse(text) {
            Ok(parsed_grant) => parsed_grant,
            Err(diagnostic) => panic!("`{text}` should parse, but it was refused: {diagnostic}"),
        }
    }

    /// The code `parse` refuses `text` with, or `None` when it accepts the text.
    fn parse_refusal_code(text: &str) -> Option<String> {
        Grant::parse(text)
            .err()
            .map(|diagnostic| diagnostic.code.as_str().to_owned())
    }

    /// The code `check_grant` refuses the grant with, or `None` when it allows it.
    fn check_refusal_code(text: &str, tier: Tier, side: Side) -> Option<String> {
        check_grant(&grant(text), tier, side)
            .err()
            .map(|diagnostic| diagnostic.code.as_str().to_owned())
    }

    /// REQ: CAP-001
    #[test]
    fn parse_splits_at_first_colon_and_keeps_later_colons_in_argument() {
        let parsed_grant = parsed("services.use:ilium:inference/provider");
        assert_eq!(parsed_grant.family(), "services.use");
        assert_eq!(parsed_grant.argument(), Some("ilium:inference/provider"));
        assert_eq!(
            parsed_grant.as_str(),
            "services.use:ilium:inference/provider"
        );
    }

    /// REQ: CAP-001
    #[test]
    fn parse_name_without_colon_has_family_and_no_argument() {
        let parsed_grant = parsed("tree.read");
        assert_eq!(parsed_grant.family(), "tree.read");
        assert_eq!(parsed_grant.argument(), None);
    }

    /// REQ: CAP-001
    #[test]
    fn parse_fs_path_template_keeps_braces_and_subtree_marker_in_argument() {
        let parsed_grant = parsed("fs.read:path:{home}/Pictures/**");
        assert_eq!(parsed_grant.family(), "fs.read");
        assert_eq!(parsed_grant.argument(), Some("path:{home}/Pictures/**"));
    }

    /// REQ: CAP-002
    #[test]
    fn parse_unknown_family_is_refused_with_grant_unknown() {
        assert_eq!(
            parse_refusal_code("files.read"),
            Some(manifest::GRANT_UNKNOWN.to_owned())
        );
    }

    /// REQ: CAP-002
    #[test]
    fn parse_reserved_grant_is_refused_with_grant_unknown() {
        assert_eq!(
            parse_refusal_code("editor.provide"),
            Some(manifest::GRANT_UNKNOWN.to_owned())
        );
    }

    /// REQ: CAP-002
    #[test]
    fn parse_retired_alias_is_refused_with_grant_unknown() {
        assert_eq!(
            parse_refusal_code("extensions.contribute-any"),
            Some(manifest::GRANT_UNKNOWN.to_owned())
        );
    }

    /// REQ: CAP-002
    #[test]
    fn parse_empty_text_is_refused_with_grant_unknown() {
        assert_eq!(
            parse_refusal_code(""),
            Some(manifest::GRANT_UNKNOWN.to_owned())
        );
    }

    /// REQ: CAP-004
    #[test]
    fn parse_argument_given_to_no_argument_grant_is_refused_with_grant_argument() {
        assert_eq!(
            parse_refusal_code("tree.read:all"),
            Some(manifest::GRANT_ARGUMENT.to_owned())
        );
    }

    /// REQ: CAP-004
    #[test]
    fn parse_clipboard_read_with_argument_is_refused_with_grant_argument() {
        assert_eq!(
            parse_refusal_code("clipboard.read:text"),
            Some(manifest::GRANT_ARGUMENT.to_owned())
        );
    }

    /// REQ: CAP-004
    #[test]
    fn parse_argument_grant_without_argument_is_refused_with_grant_argument_missing() {
        assert_eq!(
            parse_refusal_code("process.run"),
            Some(manifest::GRANT_ARGUMENT_MISSING.to_owned())
        );
    }

    /// REQ: CAP-004
    #[test]
    fn parse_kernel_settings_read_without_tab_id_is_refused_with_grant_argument_missing() {
        assert_eq!(
            parse_refusal_code("settings.read-kernel"),
            Some(manifest::GRANT_ARGUMENT_MISSING.to_owned())
        );
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_program_name_and_absolute_path_are_accepted() {
        assert!(Grant::parse("process.run:git").is_ok());
        assert!(Grant::parse("process.run:/usr/bin/git").is_ok());
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_with_arguments_is_refused_with_grant_program() {
        assert_eq!(
            parse_refusal_code("process.run:git status"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_relative_path_with_separator_is_refused_with_grant_program() {
        assert_eq!(
            parse_refusal_code("process.run:./bin/tool"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
        assert_eq!(
            parse_refusal_code("process.run:../git"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_wildcard_or_empty_program_is_refused_with_grant_program() {
        assert_eq!(
            parse_refusal_code("process.run:*"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
        assert_eq!(
            parse_refusal_code("process.run:"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
    }

    /// REQ: CAP-003
    #[test]
    fn parse_command_plugin_wildcard_in_local_position_is_accepted() {
        assert!(Grant::parse("commands.invoke:com.example.notes/*").is_ok());
    }

    /// REQ: CAP-003
    #[test]
    fn parse_bare_star_for_commands_and_topics_is_accepted() {
        assert!(Grant::parse("commands.invoke:*").is_ok());
        assert!(Grant::parse("topics.subscribe:*").is_ok());
    }

    /// REQ: CAP-003
    #[test]
    fn parse_wildcard_in_plugin_id_or_inside_word_is_refused_with_grant_wildcard() {
        assert_eq!(
            parse_refusal_code("commands.invoke:*/notes"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("commands.invoke:ilium.git*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("commands.invoke:ilium.git/prefix*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
    }

    /// REQ: CAP-003
    #[test]
    fn parse_wildcard_in_topic_messages_peer_or_http_host_is_refused_with_grant_wildcard() {
        assert_eq!(
            parse_refusal_code("topics.subscribe:ilium.*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("messages.peer:*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("http:tile.*.org"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
    }

    /// REQ: CAP-008
    #[test]
    fn parse_read_scope_project_is_accepted_and_write_data_is_accepted() {
        assert!(Grant::parse("fs.read:project").is_ok());
        assert!(Grant::parse("fs.write:data").is_ok());
    }

    /// REQ: CAP-008
    #[test]
    fn parse_write_to_package_scope_is_refused_with_grant_scope() {
        assert_eq!(
            parse_refusal_code("fs.write:package"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-008
    #[test]
    fn parse_write_to_agent_sessions_or_agent_config_is_refused_with_grant_scope() {
        assert_eq!(
            parse_refusal_code("fs.write:agent-sessions:claude"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
        assert_eq!(
            parse_refusal_code("fs.write:agent-config:claude"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-008
    #[test]
    fn parse_watch_of_agent_config_is_refused_with_grant_scope() {
        assert_eq!(
            parse_refusal_code("fs.watch:agent-config:claude"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-008
    #[test]
    fn parse_absolute_path_without_path_template_is_refused_with_grant_scope() {
        assert_eq!(
            parse_refusal_code("fs.read:/srv/logs"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-886
    #[test]
    fn parse_agent_config_propose_names_one_known_agent() {
        assert!(Grant::parse("agent-config.propose:claude").is_ok());
        assert_eq!(
            parse_refusal_code("agent-config.propose"),
            Some(manifest::GRANT_ARGUMENT_MISSING.to_owned())
        );
        assert_eq!(
            parse_refusal_code("agent-config.propose:*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("agent-config.propose:cursor"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-003
    #[test]
    fn covers_exact_host_only_matches_that_host() {
        assert!(grant("http:api.example.com").covers(&grant("http:api.example.com")));
        assert!(!grant("http:api.example.com").covers(&grant("http:api.example.org")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_subdomain_wildcard_matches_any_depth_but_not_bare_domain() {
        let pattern = grant("http:*.openstreetmap.org");
        assert!(pattern.covers(&grant("http:a.openstreetmap.org")));
        assert!(pattern.covers(&grant("http:a.b.openstreetmap.org")));
        assert!(!pattern.covers(&grant("http:openstreetmap.org")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_command_plugin_wildcard_matches_every_command_of_that_plugin_only() {
        let pattern = grant("commands.invoke:ilium.git/*");
        assert!(pattern.covers(&grant("commands.invoke:ilium.git/status")));
        assert!(!pattern.covers(&grant("commands.invoke:ilium.gitx/status")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_exact_command_matches_only_that_command() {
        let pattern = grant("commands.invoke:ilium.git/status");
        assert!(pattern.covers(&grant("commands.invoke:ilium.git/status")));
        assert!(!pattern.covers(&grant("commands.invoke:ilium.git/refresh")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_bare_star_matches_every_command() {
        assert!(grant("commands.invoke:*").covers(&grant("commands.invoke:ilium.git/status")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_topic_prefix_requires_text_past_the_slash() {
        let pattern = grant("topics.subscribe:ilium.progress/*");
        assert!(pattern.covers(&grant("topics.subscribe:ilium.progress/finished")));
        assert!(!pattern.covers(&grant("topics.subscribe:ilium.progress")));
    }

    /// REQ: CAP-001
    #[test]
    fn covers_is_false_across_different_families() {
        assert!(!grant("tree.read").covers(&grant("fs.read:project")));
    }

    /// REQ: CAP-005
    #[test]
    fn check_grant_normal_grant_is_allowed_for_every_tier_on_a_listed_side() {
        assert_eq!(
            check_refusal_code("tree.read", Tier::T1, Side::Server),
            None
        );
        assert_eq!(
            check_refusal_code("tree.read", Tier::T2, Side::Server),
            None
        );
        assert_eq!(
            check_refusal_code("tree.read", Tier::T3, Side::Client),
            None
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_on_unlisted_side_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("tree.read", Tier::T1, Side::Scene),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_client_only_grant_on_server_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("ui.panels", Tier::T1, Side::Server),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_server_only_grant_on_client_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("sessions.cross", Tier::T1, Side::Client),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-005
    #[test]
    fn check_grant_bundled_only_grant_is_allowed_for_t1_on_its_side() {
        assert_eq!(
            check_refusal_code("settings.read-all", Tier::T1, Side::Client),
            None
        );
    }

    /// REQ: CAP-005
    #[test]
    fn check_grant_bundled_only_grant_is_refused_with_grant_tier_for_t2_and_t3() {
        assert_eq!(
            check_refusal_code("settings.read-all", Tier::T2, Side::Client),
            Some(manifest::GRANT_TIER.to_owned())
        );
        assert_eq!(
            check_refusal_code("plugins.manage", Tier::T3, Side::Client),
            Some(manifest::GRANT_TIER.to_owned())
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_bundled_only_grant_on_unlisted_side_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("settings.read-all", Tier::T1, Side::Server),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-010
    #[test]
    fn info_high_risk_family_reports_argument_risk_and_sides() {
        let row = info("process.run").expect("process.run is in the grant table");
        assert!(row.takes_argument);
        assert_eq!(row.risk, Risk::High);
        assert!(row.sides.contains(&Side::Server));
        assert!(row.sides.contains(&Side::Client));
        assert!(!row.sides.contains(&Side::Scene));
    }

    /// REQ: CAP-010
    #[test]
    fn info_normal_family_takes_no_argument_and_is_not_high_risk() {
        let row = info("tree.read").expect("tree.read is in the grant table");
        assert!(!row.takes_argument);
        assert_ne!(row.risk, Risk::High);
    }

    /// REQ: CAP-005
    #[test]
    fn info_bundled_only_family_lists_only_t1_as_tier() {
        let row = info("settings.read-all").expect("settings.read-all is in the grant table");
        assert_eq!(row.tiers, [Tier::T1].as_slice());
    }

    /// REQ: CAP-002
    #[test]
    fn info_unknown_family_is_none() {
        assert!(info("files.read").is_none());
        assert!(info("").is_none());
    }

    /// REQ: ABI-022
    #[test]
    fn info_unstable_grant_names_its_feature_and_runs_in_scene_only() {
        let row = info("gpu.kernels").expect("gpu.kernels is in the grant table");
        assert_eq!(row.unstable, Some("gpu-kernels"));
        assert_eq!(row.sides, [Side::Scene].as_slice());
    }

    /// REQ: CAP-017
    #[test]
    fn table_lists_each_grant_family_once() {
        let families: Vec<&str> = table().iter().map(|row| row.family).collect();
        let unique: HashSet<&str> = families.iter().copied().collect();
        assert_eq!(unique.len(), families.len());
        assert!(unique.contains("tree.read"));
    }

    /// REQ: CAP-010
    #[test]
    fn risk_of_list_with_a_high_grant_is_high() {
        assert_eq!(
            risk_of(&[grant("tree.read"), grant("process.run:git")]),
            Risk::High
        );
        assert_eq!(risk_of(&[grant("commands.invoke:*")]), Risk::High);
    }

    /// REQ: CAP-010
    #[test]
    fn risk_of_list_of_normal_grants_is_below_high() {
        assert!(risk_of(&[grant("tree.read"), grant("fs.read:project")]) < Risk::High);
    }

    /// REQ: CAP-010
    #[test]
    fn risk_of_empty_list_is_below_high() {
        assert!(risk_of(&[]) < Risk::High);
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_for_process_run_and_agent_config_propose() {
        assert!(is_high_risk("process.run:git"));
        assert!(is_high_risk("agent-config.propose:claude"));
        assert!(!is_high_risk("tree.read"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_only_for_bare_star_commands_and_topics() {
        assert!(is_high_risk("commands.invoke:*"));
        assert!(is_high_risk("topics.subscribe:*"));
        assert!(!is_high_risk("commands.invoke:ilium.git/*"));
        assert!(!is_high_risk("topics.subscribe:ilium.progress/*"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_only_for_loopback_http_with_port() {
        assert!(is_high_risk("http:localhost:8080"));
        assert!(!is_high_risk("http:api.example.com"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_for_fs_write_outside_data_and_cache_only() {
        assert!(is_high_risk("fs.write:project"));
        assert!(!is_high_risk("fs.write:data"));
        assert!(!is_high_risk("fs.read:project"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_for_clipboard_read_but_not_clipboard_write() {
        assert!(is_high_risk("clipboard.read"));
        assert!(!is_high_risk("clipboard.write"));
    }

    /// REQ: CAP-010
    #[test]
    fn risk_names_round_trip_and_reject_other_text() {
        for risk in Risk::ALL {
            assert_eq!(risk.as_str().parse::<Risk>().ok(), Some(risk));
        }
        assert!("HIGH".parse::<Risk>().is_err());
        assert!("".parse::<Risk>().is_err());
    }
}
