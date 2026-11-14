//! Agent signatures plugins contribute (`contributes.signatures`, reference-manifest.md), fed to
//! the detection loop (gap G6).
//!
//! Each entry names the process names of one agent CLI. The detection loop matches them after
//! the user's own custom signatures, the same way: a lower-case substring of a process name
//! below a pane's shell. The pane then reads as an agent whose class is the matched name.
//! Only enabled plugins contribute; enabling or disabling a plugin republishes the set.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use ilium_core::AgentClass;
use ilium_detect::AgentSignature;
use serde_json::Value;

/// Most process names one plugin may contribute, across its signatures.
const PROCESS_NAMES_PER_PLUGIN: usize = 64;
/// Longest process name, in bytes.
const PROCESS_NAME_BYTES: usize = 64;

/// The detection signatures of one manifest, in manifest order. A malformed manifest or entry
/// contributes nothing; empty and over-long names are skipped.
pub(crate) fn manifest_signatures(manifest: &str) -> Vec<AgentSignature> {
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
        .filter(|entry| entry.get("id").and_then(Value::as_str).is_some())
        .filter_map(|entry| entry.get("process_names").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty() && name.len() <= PROCESS_NAME_BYTES)
        .take(PROCESS_NAMES_PER_PLUGIN)
        .map(|name| AgentSignature {
            name_substring: Cow::Owned(name),
            class_of: |matched_name: &str| AgentClass::Other(matched_name.to_string()),
        })
        .collect()
}

/// The signatures of every catalogue plugin, and which plugins are disabled.
#[derive(Debug, Default)]
pub(crate) struct ContributedSignatures {
    by_plugin: BTreeMap<String, Vec<AgentSignature>>,
    disabled: BTreeSet<String>,
}

impl ContributedSignatures {
    /// Reads the signatures of `catalogue` (`(plugin, manifest)` pairs).
    pub(crate) fn new<'a>(
        catalogue: impl Iterator<Item = (&'a str, &'a str)>,
        disabled: BTreeSet<String>,
    ) -> Self {
        let by_plugin = catalogue
            .map(|(plugin, manifest)| (plugin.to_owned(), manifest_signatures(manifest)))
            .filter(|(_, signatures)| !signatures.is_empty())
            .collect();
        Self {
            by_plugin,
            disabled,
        }
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
    fn a_disabled_plugin_contributes_nothing_until_enabled() {
        let catalogue = [("example.todo", MANIFEST), ("example.none", "{}")];
        let mut contributed = ContributedSignatures::new(
            catalogue.iter().copied(),
            BTreeSet::from(["example.todo".to_owned()]),
        );
        assert!(contributed.active().is_empty());
        contributed.set_enabled("example.todo", true);
        assert_eq!(contributed.active().len(), 2);
    }
}
