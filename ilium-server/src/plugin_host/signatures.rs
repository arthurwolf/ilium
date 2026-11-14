//! Agent signatures plugins contribute (`contributes.signatures`, reference-manifest.md), fed to
//! the detection loop (gap G6).
//!
//! Each entry names the process names of one agent CLI. The detection loop matches them after
//! the user's own custom signatures, the same way: a lower-case substring of a process name
//! below a pane's shell. The pane then reads as an agent whose class is the matched name.
//! Only enabled plugins contribute, and under `--safe-plugins` only core ones; enabling or
//! disabling a plugin republishes the set and makes the detection loop identify every pane
//! again.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use ilium_core::AgentClass;
use ilium_detect::AgentSignature;
use serde_json::Value;

/// Most process names one plugin may contribute, across its signatures.
const PROCESS_NAMES_PER_PLUGIN: usize = 64;
/// Longest process name, in bytes.
const PROCESS_NAME_BYTES: usize = 64;

/// The detection signatures of one manifest, in manifest order: the `process_names` and then
/// the `aliases` of each entry. A malformed manifest or entry contributes nothing; empty,
/// over-long and repeated names are skipped.
pub(crate) fn manifest_signatures(manifest: &str) -> Vec<AgentSignature> {
    let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
        return Vec::new();
    };
    let entries = manifest
        .get("contributes")
        .and_then(|contributes| contributes.get("signatures"))
        .and_then(Value::as_array);
    let mut seen = BTreeSet::new();
    entries
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("id").and_then(Value::as_str).is_some())
        // `aliases` also identify the agent (capability-agent-signatures.md): with no
        // detector export a pane is identified by `process_names` and `aliases`.
        .flat_map(|entry| {
            ["process_names", "aliases"]
                .into_iter()
                .filter_map(move |field| entry.get(field).and_then(Value::as_array))
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty() && name.len() <= PROCESS_NAME_BYTES)
        // A name given twice (as a process name and an alias) is one signature.
        .filter(|name| seen.insert(name.clone()))
        .take(PROCESS_NAMES_PER_PLUGIN)
        .map(|name| AgentSignature {
            name_substring: Cow::Owned(name),
            class_of: |matched_name: &str| AgentClass::Other(matched_name.to_string()),
        })
        .collect()
}

/// Most actions one signature may declare.
const ACTIONS_PER_SIGNATURE: usize = 32;
/// Longest key string of one action, in bytes.
const ACTION_KEYS_BYTES: usize = 1024;
/// The activities an action runs in when it names none (capability-agent-control.md).
const DEFAULT_ALLOWED_WHEN: [&str; 4] = ["idle", "done", "awaiting-approval", "menu"];

/// One action of a signature: the keys it presses and the activities it may run in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignatureActionSpec {
    pub(crate) keys: String,
    pub(crate) allowed_when: Vec<String>,
}

impl SignatureActionSpec {
    pub(crate) fn new(keys: &str, allowed_when: &[&str]) -> Self {
        Self {
            keys: keys.to_owned(),
            allowed_when: allowed_when.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    /// Whether the action may run while the agent shows `activity` (an `agent-activity` name).
    pub(crate) fn allows(&self, activity: &str) -> bool {
        self.allowed_when.iter().any(|allowed| allowed == activity)
    }
}

/// The actions of one signature entry and the lower-case names that identify its pane: its
/// id, `process_names` and `aliases`. A pane's class is the matched name, so any of them
/// finds the entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SignatureActions {
    names: BTreeSet<String>,
    actions: BTreeMap<String, SignatureActionSpec>,
}

/// The actions every signature entry of one manifest declares. An entry without actions, a
/// malformed manifest and an action without `keys` contribute nothing.
pub(crate) fn manifest_actions(manifest: &str) -> Vec<SignatureActions> {
    let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
        return Vec::new();
    };
    let entries = manifest
        .get("contributes")
        .and_then(|contributes| contributes.get("signatures"))
        .and_then(Value::as_array);
    entries
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?;
            let declared = entry.get("actions").and_then(Value::as_object)?;
            let actions: BTreeMap<String, SignatureActionSpec> = declared
                .iter()
                .filter_map(|(action_id, action)| {
                    let keys = action.get("keys").and_then(Value::as_str)?;
                    if keys.len() > ACTION_KEYS_BYTES {
                        return None;
                    }
                    let allowed_when = match action.get("allowed_when").and_then(Value::as_array) {
                        Some(list) => list
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                        None => DEFAULT_ALLOWED_WHEN
                            .iter()
                            .map(|name| (*name).to_owned())
                            .collect(),
                    };
                    Some((
                        action_id.clone(),
                        SignatureActionSpec {
                            keys: keys.to_owned(),
                            allowed_when,
                        },
                    ))
                })
                .take(ACTIONS_PER_SIGNATURE)
                .collect();
            if actions.is_empty() {
                return None;
            }
            let names = std::iter::once(id)
                .chain(
                    ["process_names", "aliases"]
                        .into_iter()
                        .filter_map(|field| entry.get(field).and_then(Value::as_array))
                        .flatten()
                        .filter_map(Value::as_str),
                )
                .map(|name| name.trim().to_lowercase())
                .filter(|name| !name.is_empty())
                .collect();
            Some(SignatureActions { names, actions })
        })
        .collect()
}

/// The signatures of every catalogue plugin, and which plugins are disabled.
#[derive(Debug, Default)]
pub(crate) struct ContributedSignatures {
    by_plugin: BTreeMap<String, Vec<AgentSignature>>,
    /// The declared actions of each plugin's signatures, for `agent-action` intents.
    actions: BTreeMap<String, Vec<SignatureActions>>,
    disabled: BTreeSet<String>,
}

impl ContributedSignatures {
    /// Reads the signatures of `catalogue` (`(plugin, manifest)` pairs). With `core_only`
    /// (`--safe-plugins`) only core plugins contribute, as only they start.
    pub(crate) fn new<'a>(
        catalogue: impl Iterator<Item = (&'a str, &'a str)>,
        disabled: BTreeSet<String>,
        core_only: bool,
    ) -> Self {
        let catalogue: Vec<(&str, &str)> = catalogue
            .filter(|(plugin, _)| {
                !core_only || ilium_plugin_host::lifecycle::is_core_plugin(plugin)
            })
            .collect();
        let by_plugin = catalogue
            .iter()
            .map(|(plugin, manifest)| ((*plugin).to_owned(), manifest_signatures(manifest)))
            .filter(|(_, signatures)| !signatures.is_empty())
            .collect();
        let actions = catalogue
            .iter()
            .map(|(plugin, manifest)| ((*plugin).to_owned(), manifest_actions(manifest)))
            .filter(|(_, actions)| !actions.is_empty())
            .collect();
        Self {
            by_plugin,
            actions,
            disabled,
        }
    }

    /// The action `action_id` of the enabled signature that identifies a pane whose agent
    /// class is `class_name` (lower-case), or `None` when no enabled plugin declares it.
    pub(crate) fn action(&self, class_name: &str, action_id: &str) -> Option<SignatureActionSpec> {
        let wanted = class_name.to_lowercase();
        self.actions
            .iter()
            .filter(|(plugin, _)| !self.disabled.contains(*plugin))
            .flat_map(|(_, entries)| entries.iter())
            .filter(|entry| entry.names.contains(&wanted))
            .find_map(|entry| entry.actions.get(action_id).cloned())
    }

    /// Records that a plugin was enabled or disabled.
    pub(crate) fn set_enabled(&mut self, plugin: &str, enabled: bool) {
        if enabled {
            self.disabled.remove(plugin);
        } else {
            self.disabled.insert(plugin.to_owned());
        }
    }

    /// The signatures of the enabled plugins, in plugin id order.
    pub(crate) fn active(&self) -> Vec<AgentSignature> {
        self.by_plugin
            .iter()
            .filter(|(plugin, _)| !self.disabled.contains(*plugin))
            .flat_map(|(_, signatures)| signatures.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "contributes": {
            "signatures": [
                { "id": "todo-runner", "process_names": ["Todo-Runner", " ", "todo"] },
                { "process_names": ["no-id"] }
            ]
        }
    }"#;

    #[test]
    fn process_names_become_lower_case_signatures() {
        let signatures = manifest_signatures(MANIFEST);
        let names: Vec<&str> = signatures
            .iter()
            .map(|signature| signature.name_substring.as_ref())
            .collect();
        assert_eq!(names, vec!["todo-runner", "todo"]);
        assert_eq!(
            (signatures[0].class_of)("todo-runner"),
            AgentClass::Other("todo-runner".to_owned())
        );
        assert!(manifest_signatures("not json").is_empty());
    }

    #[test]
    fn declared_actions_default_their_activities_and_follow_the_enabled_plugins() {
        let manifest = r#"{"contributes": {"signatures": [{
            "id": "example-agent",
            "process_names": ["example-cli"],
            "aliases": ["Example"],
            "actions": {
                "compact": {"keys": "/compact\r", "allowed_when": ["idle"]},
                "clear": {"keys": "/clear\r"},
                "broken": {"allowed_when": ["idle"]}
            }
        }]}}"#;
        let mut contributed = ContributedSignatures::new(
            [("example.agent", manifest)].into_iter(),
            BTreeSet::new(),
            false,
        );
        let compact = contributed
            .action("example-cli", "compact")
            .expect("a process name finds the signature");
        assert_eq!(compact.keys, "/compact\r");
        assert!(compact.allows("idle") && !compact.allows("done"));
        let clear = contributed
            .action("EXAMPLE", "clear")
            .expect("an alias too");
        assert!(clear.allows("awaiting-approval") && !clear.allows("working"));
        assert_eq!(contributed.action("example-cli", "broken"), None);
        assert_eq!(contributed.action("other", "compact"), None);
        contributed.set_enabled("example.agent", false);
        assert_eq!(contributed.action("example-cli", "compact"), None);
    }

    #[test]
    fn a_disabled_plugin_contributes_nothing_until_enabled() {
        let catalogue = [("example.todo", MANIFEST), ("example.none", "{}")];
        let mut contributed = ContributedSignatures::new(
            catalogue.iter().copied(),
            BTreeSet::from(["example.todo".to_owned()]),
            false,
        );
        assert!(contributed.active().is_empty());
        contributed.set_enabled("example.todo", true);
        assert_eq!(contributed.active().len(), 2);
    }

    #[test]
    fn aliases_identify_the_agent_too_and_repeats_count_once() {
        let manifest = r#"{"contributes": {"signatures": [{
            "id": "example-agent",
            "process_names": ["example-agent"],
            "aliases": ["Example", "example-agent"],
            "icon": "robot"
        }]}}"#;
        let names: Vec<String> = manifest_signatures(manifest)
            .iter()
            .map(|signature| signature.name_substring.to_string())
            .collect();
        assert_eq!(names, vec!["example-agent", "example"]);
    }

    #[test]
    fn safe_plugins_mode_takes_only_core_signatures() {
        let catalogue = [("example.todo", MANIFEST), ("ilium.core.agents", MANIFEST)];
        let mut safe = ContributedSignatures::new(catalogue.iter().copied(), BTreeSet::new(), true);
        assert_eq!(safe.active().len(), 2, "only the core plugin's two names");
        let mut normal =
            ContributedSignatures::new(catalogue.iter().copied(), BTreeSet::new(), false);
        assert_eq!(normal.active().len(), 4);
        // Enabling a third-party plugin in safe mode adds nothing: it does not start.
        safe.set_enabled("example.todo", true);
        assert_eq!(safe.active().len(), 2);
        normal.set_enabled("example.todo", false);
        assert_eq!(normal.active().len(), 2);
    }

    /// A real process named only by a plugin alias is identified through the same
    /// `identify_agent_with_extra` call the detection loop makes, and not without the
    /// plugin's signatures. Unix-only: the stand-in agent is a `#!/bin/sh` script.
    #[test]
    #[cfg(unix)]
    fn a_live_process_named_by_a_plugin_alias_is_identified() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        use sysinfo::{Pid, System};

        // Under 15 bytes, so the kernel's process name keeps all of it.
        const AGENT: &str = "ilium-sigtest";
        let manifest = format!(
            r#"{{"contributes": {{"signatures": [{{
                "id": "sigtest", "process_names": ["no-such-agent-cli"], "aliases": ["{AGENT}"]
            }}]}}}}"#
        );
        let contributed = ContributedSignatures::new(
            [("example.sigtest", manifest.as_str())].into_iter(),
            BTreeSet::new(),
            false,
        );
        let signatures = contributed.active();

        let directory = tempfile::tempdir().expect("temp dir");
        let script = directory.path().join(AGENT);
        let mut file = std::fs::File::create(&script).expect("create the stand-in agent");
        // Bounded, so a run that never reaches its cleanup cannot leave it running.
        file.write_all(
            b"#!/bin/sh\nelapsed=0\nwhile [ \"$elapsed\" -lt 20 ]; do sleep 1; elapsed=$((elapsed + 1)); done\n",
        )
        .expect("write the stand-in agent");
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .expect("make it executable");
        drop(file);
        // Behind `$0`, so only the script itself can match, never the shell's argv.
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("\"$0\"")
            .arg(&script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn the stand-in agent");

        let shell = Pid::from_u32(child.id());
        let deadline = Instant::now() + Duration::from_secs(10);
        let (with_plugin, without_plugin) = loop {
            // A fresh table per attempt: an argv latched before the exec never updates.
            let mut system = System::new();
            ilium_detect::refresh(&mut system);
            let children = ilium_detect::ProcessChildrenIndex::build(&system);
            let with_plugin =
                ilium_detect::identify_agent_with_extra(&system, shell, &children, &signatures);
            if with_plugin.is_some() || Instant::now() >= deadline {
                let without_plugin =
                    ilium_detect::identify_agent_with_extra(&system, shell, &children, &[]);
                break (with_plugin, without_plugin);
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if let Some(found) = with_plugin.as_ref().filter(|found| found.pid != child.id()) {
            let _ = ilium_platform::process_control::terminate(found.pid);
        }
        let _ = child.kill();
        let _ = child.wait();

        let identity = with_plugin.expect("the plugin alias identifies the stand-in agent");
        assert_eq!(identity.matched_signature, AGENT);
        assert_eq!(identity.class, AgentClass::Other(AGENT.to_owned()));
        assert_eq!(without_plugin, None);
    }
}
