//! Apply and Revert of the recommended auto-compact threshold (manual page core-plugin-compaction.md, "Optimizer").
//!
//! The optimizer panel proposes one key of the agent's own settings file: `autoCompactWindow` in Claude Code's
//! `settings.json`, `model_auto_compact_token_limit` in Codex's `config.toml`. The server side of the plugin does the
//! work in two steps so the user always sees the change before it is written:
//!
//! 1. `plan_apply` and `plan_revert` read the file through the read-only `agent-config:<agent>` scope, compute the new
//!    text with the byte-preserving editor of [`crate::config_edit`] and keep it as the one pending plan.
//! 2. `confirm` hands the pending text to the host as an `agent-config.propose` change that carries the revision the
//!    plugin read; the host replaces the file only while its bytes still have that revision, so an edit made in between
//!    is never overwritten. The previous value and the revision of the written file are kept in the global storage
//!    bucket, so a later session can revert and a revert is refused when somebody changed the file since.
//!
//! A settings file that does not exist is never created. The environment variable and the project or profile overrides that shadow the user-level value are not shown: a
//! plugin cannot read the environment or the project's own agent files.

use std::cell::RefCell;
use std::ops::RangeInclusive;

use ilium_plugin_sdk::host::{Bucket, ConfigChange, HostFs, HostStorage};
use ilium_plugin_sdk::types::{ErrorCode, FsPath};
use ilium_plugin_sdk::HostApi;
use serde::{Deserialize, Serialize};

use crate::config_edit::{edit_config_text, parse_setting, ConfigTarget, KeyChange};
use crate::report::group_thousands;

/// The two agents whose threshold the optimizer can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    /// The agent of a wire name (`claude` or `codex`).
    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    /// The wire name, also the agent name of the `agent-config` scope.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    const fn target(self) -> ConfigTarget {
        match self {
            Self::Claude => ConfigTarget::ClaudeAutoCompactWindow,
            Self::Codex => ConfigTarget::CodexAutoCompactTokenLimit,
        }
    }

    /// The top-level key the agent reads its threshold from.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Claude => "autoCompactWindow",
            Self::Codex => "model_auto_compact_token_limit",
        }
    }

    const fn file_name(self) -> &'static str {
        match self {
            Self::Claude => "settings.json",
            Self::Codex => "config.toml",
        }
    }

    const fn display_path(self) -> &'static str {
        match self {
            Self::Claude => "~/.claude/settings.json",
            Self::Codex => "~/.codex/config.toml",
        }
    }

    /// Values accepted for the key: Claude documents the window as 100000 to 1000000; Codex takes any token count
    /// a context window can plausibly hold.
    pub const fn valid_range(self) -> RangeInclusive<u64> {
        match self {
            Self::Claude => 100_000..=1_000_000,
            Self::Codex => 1_000..=10_000_000,
        }
    }

    fn scope(self) -> String {
        format!("agent-config:{}", self.wire())
    }

    fn record_key(self) -> String {
        format!("compaction/optimizer-record/{}", self.wire())
    }
}

/// What a later session needs to revert one applied value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    /// The value before the write; `None` when the key was absent.
    previous_value: Option<u64>,
    written_value: u64,
    /// The revision of the file as the write left it.
    written_revision: String,
}

/// What the pending plan does once confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Apply {
        previous: Option<u64>,
        value: u64,
    },
    Revert {
        restored: Option<u64>,
    },
    /// The file already holds the value: nothing to write.
    Nothing,
}

/// The one change waiting for the user's confirmation.
#[derive(Debug, Clone)]
struct Pending {
    agent: Agent,
    action: Action,
    /// The revision the plugin read.
    revision: String,
    new_text: String,
}

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

fn value_text(value: Option<u64>) -> String {
    value.map_or_else(|| "not set".to_string(), group_thousands)
}

/// The text of the agent's settings file and its revision; `None` when the file does not exist.
fn read_config(api: &mut dyn HostApi, agent: Agent) -> Result<Option<(String, String)>, String> {
    let path = FsPath {
        scope: agent.scope(),
        path: agent.file_name().to_string(),
    };
    let request = match HostFs::read_with_revision(api, path) {
        Ok(request) => request,
        Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "{} could not be read: {}",
                agent.display_path(),
                error.message
            ))
        }
    };
    let revisioned = HostFs::take_revisioned(api, request)
        .ok_or_else(|| format!("the host returned no content of {}", agent.display_path()))?;
    let text = String::from_utf8(revisioned.bytes)
        .map_err(|_| format!("{} is not UTF-8 text", agent.display_path()))?;
    Ok(Some((text, revisioned.revision)))
}

/// The value the agent's settings file holds now: `None` when the file or the key is missing or unreadable.
pub fn current_value(api: &mut dyn HostApi, agent: Agent) -> Option<u64> {
    let (text, _) = read_config(api, agent).ok().flatten()?;
    parse_setting(agent.target(), &text).ok().flatten()
}

/// Proposes the new text of the settings file to the host.
fn write_config(
    api: &mut dyn HostApi,
    agent: Agent,
    revision: String,
    text: &str,
    summary: &str,
) -> Result<(), String> {
    let change = ConfigChange {
        path: agent.file_name().to_string(),
        content: Some(text.as_bytes().to_vec()),
        revision: Some(revision),
        summary: summary.to_string(),
    };
    let request = HostFs::propose_agent_config(
        api,
        agent.wire().to_string(),
        summary.to_string(),
        vec![change],
    )
    .map_err(|error| error.message)?;
    let result = HostFs::take_proposal(api, request)
        .ok_or_else(|| "the host returned no answer to the proposal".to_string())?;
    if result.rejected.is_empty() {
        return Ok(());
    }
    if result.history.contains("conflict") {
        Err(format!(
            "{} changed while the change was prepared; nothing was written. Try again.",
            agent.display_path()
        ))
    } else {
        Err(format!(
            "{} was not written: {}",
            agent.display_path(),
            result.history.trim()
        ))
    }
}

fn load_record(api: &mut dyn HostApi, agent: Agent) -> Option<Record> {
    let bytes = HostStorage::get(api, Bucket::Global, agent.record_key())?;
    serde_json::from_slice(&bytes).ok()
}

/// Prepares writing `value` as the agent's threshold; returns the lines of the confirmation.
pub fn plan_apply(api: &mut dyn HostApi, agent: Agent, value: u64) -> Result<Vec<String>, String> {
    let range = agent.valid_range();
    if !range.contains(&value) {
        return Err(format!(
            "{} accepts {} to {}, not {}.",
            agent.key(),
            group_thousands(*range.start()),
            group_thousands(*range.end()),
            group_thousands(value)
        ));
    }
    let (text, revision) = read_config(api, agent)?.ok_or_else(|| {
        format!(
            "{} does not exist; Ilium never creates it. Start the agent once to create it.",
            agent.display_path()
        )
    })?;
    let previous = parse_setting(agent.target(), &text)
        .map_err(|reason| format!("{}: {reason}", agent.display_path()))?;
    let mut lines = vec![
        format!("Apply {} = {}", agent.key(), group_thousands(value)),
        format!("File: {}", agent.display_path()),
        format!("Current {}: {}", agent.key(), value_text(previous)),
    ];
    let (action, new_text) = if previous == Some(value) {
        lines.push("The file already holds this value; nothing will change.".to_string());
        (Action::Nothing, text)
    } else {
        let edited = edit_config_text(agent.target(), &text, KeyChange::Set(value))
            .map_err(|reason| format!("{}: {reason}", agent.display_path()))?;
        lines.push("Only this key changes; every other byte of the file is kept.".to_string());
        (Action::Apply { previous, value }, edited)
    };
    PENDING.with(|cell| {
        *cell.borrow_mut() = Some(Pending {
            agent,
            action,
            revision,
            new_text,
        });
    });
    Ok(lines)
}

/// Prepares restoring the value the last apply replaced; returns the lines of the confirmation.
pub fn plan_revert(api: &mut dyn HostApi, agent: Agent) -> Result<Vec<String>, String> {
    let record =
        load_record(api, agent).ok_or_else(|| format!("No applied {} to revert.", agent.key()))?;
    let (text, revision) = read_config(api, agent)?.ok_or_else(|| {
        format!(
            "{} no longer exists; nothing to revert.",
            agent.display_path()
        )
    })?;
    if revision != record.written_revision {
        return Err(format!(
            "{} changed after the value was applied. The revert is refused so your edit is kept.",
            agent.display_path()
        ));
    }
    let change = record
        .previous_value
        .map_or(KeyChange::Remove, KeyChange::Set);
    let edited = edit_config_text(agent.target(), &text, change)
        .map_err(|reason| format!("{}: {reason}", agent.display_path()))?;
    let headline = match record.previous_value {
        Some(previous) => format!("Revert {} to {}", agent.key(), group_thousands(previous)),
        None => format!("Revert {}: remove the key again", agent.key()),
    };
    let lines = vec![
        headline,
        format!("File: {}", agent.display_path()),
        format!(
            "Current {}: {}",
            agent.key(),
            value_text(Some(record.written_value))
        ),
        "Only this key changes; every other byte of the file is kept.".to_string(),
    ];
    PENDING.with(|cell| {
        *cell.borrow_mut() = Some(Pending {
            agent,
            action: Action::Revert {
                restored: record.previous_value,
            },
            revision,
            new_text: edited,
        });
    });
    Ok(lines)
}

/// Forgets the plan that waits for confirmation.
pub fn cancel() {
    PENDING.with(|cell| *cell.borrow_mut() = None);
}

/// Writes the pending plan; returns the note the panel shows.
pub fn confirm(api: &mut dyn HostApi) -> Result<String, String> {
    let pending = PENDING
        .with(|cell| cell.borrow_mut().take())
        .ok_or_else(|| "Nothing waits for confirmation.".to_string())?;
    let agent = pending.agent;
    match pending.action {
        Action::Nothing => Ok(format!("{} already holds this value.", agent.key())),
        Action::Apply { previous, value } => {
            let summary = format!("Set {} to {}", agent.key(), group_thousands(value));
            write_config(api, agent, pending.revision, &pending.new_text, &summary)?;
            let written_revision = read_config(api, agent)?
                .map(|(_, revision)| revision)
                .unwrap_or_default();
            let record = Record {
                previous_value: previous,
                written_value: value,
                written_revision,
            };
            if let Ok(bytes) = serde_json::to_vec(&record) {
                let _ = HostStorage::put(api, Bucket::Global, agent.record_key(), bytes);
            }
            Ok(format!(
                "Applied {} = {}. Revert restores {}.",
                agent.key(),
                group_thousands(value),
                value_text(previous)
            ))
        }
        Action::Revert { restored } => {
            let summary = match restored {
                Some(value) => format!("Restore {} to {}", agent.key(), group_thousands(value)),
                None => format!("Remove {}", agent.key()),
            };
            write_config(api, agent, pending.revision, &pending.new_text, &summary)?;
            let _ = HostStorage::remove(api, Bucket::Global, agent.record_key());
            Ok(match restored {
                Some(value) => format!("Reverted {} to {}.", agent.key(), group_thousands(value)),
                None => format!("Reverted: {} is not set again.", agent.key()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: F-082
    #[test]
    fn the_wire_names_map_to_the_agents_keys_files_and_ranges() {
        let claude = Agent::from_wire("claude").expect("claude");
        let codex = Agent::from_wire("codex").expect("codex");
        assert_eq!(Agent::from_wire("gemini"), None);
        assert_eq!(claude.key(), "autoCompactWindow");
        assert_eq!(claude.file_name(), "settings.json");
        assert_eq!(claude.scope(), "agent-config:claude");
        assert_eq!(codex.key(), "model_auto_compact_token_limit");
        assert_eq!(codex.file_name(), "config.toml");
        assert!(claude.valid_range().contains(&100_000));
        assert!(!claude.valid_range().contains(&99_999));
        assert!(codex.valid_range().contains(&1_000));
    }

    /// REQ: F-082
    #[test]
    fn a_missing_value_reads_as_not_set_and_a_value_is_grouped() {
        assert_eq!(value_text(None), "not set");
        assert_eq!(value_text(Some(123_456)), "123,456");
    }

    /// REQ: F-082
    #[test]
    fn cancel_without_a_plan_is_harmless_and_confirm_then_has_nothing_to_write() {
        cancel();
        PENDING.with(|cell| assert!(cell.borrow().is_none()));
    }

    /// REQ: F-082
    #[test]
    fn the_record_survives_a_storage_round_trip() {
        let record = Record {
            previous_value: Some(123_456),
            written_value: 150_000,
            written_revision: "sha256-abc".to_string(),
        };
        let bytes = serde_json::to_vec(&record).expect("encode");
        assert_eq!(
            serde_json::from_slice::<Record>(&bytes).expect("decode"),
            record
        );
    }
}
