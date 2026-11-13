//! The contribution registry: one table for the 33 `contributes` kinds of the manifest.
//!
//! Static entries come from a manifest when its plugin is loaded; run-time entries come from
//! host imports (`extensions`, `commands`, `ui-host`). Every change is also recorded as a
//! `ContributionOp`, which is what the server sends to clients. `Clear(plugin)` is the single
//! operation behind disable: everything a plugin contributed vanishes in one step.

use std::collections::BTreeMap;

use ilium_plugin_api::Side;
use serde_json::Value;

/// The `contributes` keys of the manifest (reference-manifest.md "Contributions").
pub const KINDS: [&str; 33] = [
    "commands",
    "keybindings",
    "settings",
    "panels",
    "help",
    "context_menu",
    "tree_toolbar",
    "row_actions",
    "decoration_slots",
    "display_modes",
    "tree_orderings",
    "pane_frame_slots",
    "pane_kinds",
    "onboarding_steps",
    "link_detectors",
    "scenes",
    "extension_points",
    "services",
    "signatures",
    "cli",
    "http_routes",
    "jobs",
    "client_requests",
    // The ten kinds below complete the list the manifest check accepts, so a manifest that
    // passes validation always loads (foundation contract section 20.1).
    "status_segments",
    "toolbar_buttons",
    "virtual_rows",
    "output_filters",
    "editor_line_actions",
    "sounds",
    "topics",
    "extensions",
    "catalogs",
    "languages",
];

/// One contribution.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The contributing plugin id.
    pub plugin: String,
    /// The kind, one of `KINDS`.
    pub kind: String,
    /// The local name inside the plugin and kind.
    pub name: String,
    /// The side that registered it at run time; `None` for a manifest entry.
    pub side: Option<Side>,
    /// The manifest object, or the run-time payload.
    pub payload: Value,
    /// Registration order across the registry; lists are shown in this order.
    pub order: u64,
}

impl Entry {
    /// `<plugin>/<kind>/<name>`.
    pub fn id(&self) -> String {
        contribution_id(&self.plugin, &self.kind, &self.name)
    }

    /// `<plugin>/<name>`: the global id used for commands, panels and extension points.
    pub fn global_name(&self) -> String {
        format!("{}/{}", self.plugin, self.name)
    }
}

/// `<plugin>/<kind>/<name>`.
pub fn contribution_id(plugin: &str, kind: &str, name: &str) -> String {
    format!("{plugin}/{kind}/{name}")
}

/// One change of the registry, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum ContributionOp {
    /// An entry was added or replaced.
    Put(Entry),
    /// The entry with this contribution id was removed.
    Remove(String),
    /// Every entry of this plugin was removed.
    Clear(String),
}

/// Why a contribution was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContributionError {
    /// The kind is not one of the 33 manifest kinds.
    #[error("unknown contribution kind {0}")]
    UnknownKind(String),
    /// `contributes` or one of its kinds has the wrong JSON shape.
    #[error("contributes.{0} must be an array of objects or one object")]
    Shape(String),
}

/// The registry.
#[derive(Debug, Default)]
pub struct ContributionRegistry {
    by_kind: BTreeMap<String, BTreeMap<String, Entry>>,
    next_order: u64,
    revision: u64,
    pending: Vec<ContributionOp>,
}

impl ContributionRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the static contributions of a manifest. Returns how many entries were added.
    /// Nothing is added when any kind is malformed.
    pub fn load_manifest(
        &mut self,
        plugin: &str,
        manifest: &Value,
    ) -> Result<usize, ContributionError> {
        let Some(contributes) = manifest.get("contributes") else {
            return Ok(0);
        };
        let Some(kinds) = contributes.as_object() else {
            return Err(ContributionError::Shape(String::new()));
        };
        let mut staged = Vec::new();
        for (kind, value) in kinds {
            if !KINDS.contains(&kind.as_str()) {
                return Err(ContributionError::UnknownKind(kind.clone()));
            }
            match value {
                Value::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        if !item.is_object() {
                            return Err(ContributionError::Shape(kind.clone()));
                        }
                        staged.push((kind.clone(), entry_name(item, index), item.clone()));
                    }
                }
                Value::Object(_) => {
                    staged.push((kind.clone(), entry_name(value, 0), value.clone()))
                }
                _ => return Err(ContributionError::Shape(kind.clone())),
            }
        }
        let count = staged.len();
        for (kind, name, payload) in staged {
            self.insert(plugin, &kind, &name, None, payload);
        }
        Ok(count)
    }

    /// Adds or replaces one entry at run time.
    pub fn put(
        &mut self,
        plugin: &str,
        kind: &str,
        name: &str,
        side: Side,
        payload: Value,
    ) -> Result<(), ContributionError> {
        if !KINDS.contains(&kind) {
            return Err(ContributionError::UnknownKind(kind.to_owned()));
        }
        self.insert(plugin, kind, name, Some(side), payload);
        Ok(())
    }

    fn insert(&mut self, plugin: &str, kind: &str, name: &str, side: Option<Side>, payload: Value) {
        let id = contribution_id(plugin, kind, name);
        let entries = self.by_kind.entry(kind.to_owned()).or_default();
        // A replaced entry keeps its place in the list.
        let order = match entries.get(&id) {
            Some(existing) => existing.order,
            None => {
                self.next_order += 1;
                self.next_order
            }
        };
        let entry = Entry {
            plugin: plugin.to_owned(),
            kind: kind.to_owned(),
            name: name.to_owned(),
            side,
            payload,
            order,
        };
        entries.insert(id, entry.clone());
        self.revision += 1;
        self.pending.push(ContributionOp::Put(entry));
    }

    /// Removes one entry. Returns whether it existed.
    pub fn remove(&mut self, plugin: &str, kind: &str, name: &str) -> bool {
        let id = contribution_id(plugin, kind, name);
        let removed = self
            .by_kind
            .get_mut(kind)
            .is_some_and(|entries| entries.remove(&id).is_some());
        if removed {
            self.revision += 1;
            self.pending.push(ContributionOp::Remove(id));
        }
        removed
    }

    /// Removes everything a plugin contributed. Returns how many entries were removed.
    pub fn clear(&mut self, plugin: &str) -> usize {
        let mut removed = 0;
        for entries in self.by_kind.values_mut() {
            let before = entries.len();
            entries.retain(|_, entry| entry.plugin != plugin);
            removed += before - entries.len();
        }
        if removed > 0 {
            self.revision += 1;
            self.pending.push(ContributionOp::Clear(plugin.to_owned()));
        }
        removed
    }

    /// The entries of a kind in registration order.
    pub fn entries(&self, kind: &str) -> Vec<&Entry> {
        let mut entries: Vec<&Entry> = self
            .by_kind
            .get(kind)
            .map(|entries| entries.values().collect())
            .unwrap_or_default();
        entries.sort_by_key(|entry| entry.order);
        entries
    }

    /// One entry.
    pub fn get(&self, plugin: &str, kind: &str, name: &str) -> Option<&Entry> {
        self.by_kind
            .get(kind)?
            .get(&contribution_id(plugin, kind, name))
    }

    /// The entry of a kind with the global name `<plugin>/<name>`.
    pub fn find_global(&self, kind: &str, global_name: &str) -> Option<&Entry> {
        let (plugin, name) = global_name.split_once('/')?;
        self.get(plugin, kind, name)
    }

    /// Every entry, by kind in `KINDS` order and then in registration order: the snapshot
    /// a newly attached client receives.
    pub fn snapshot(&self) -> Vec<Entry> {
        KINDS
            .iter()
            .flat_map(|kind| self.entries(kind).into_iter().cloned())
            .collect()
    }

    /// The changes since the last call, oldest first.
    pub fn take_ops(&mut self) -> Vec<ContributionOp> {
        std::mem::take(&mut self.pending)
    }

    /// A counter that grows with every change.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.by_kind.values().map(BTreeMap::len).sum()
    }

    /// Whether the registry holds no entry.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The local name of a manifest entry: its `id`, else its `name`, else its position.
fn entry_name(item: &Value, index: usize) -> String {
    ["id", "name", "path", "kind"]
        .iter()
        .find_map(|key| item.get(*key).and_then(Value::as_str))
        .map_or_else(|| index.to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{ContributionError, ContributionOp, ContributionRegistry, KINDS};
    use ilium_plugin_api::Side;
    use serde_json::json;

    fn manifest() -> serde_json::Value {
        json!({
            "contributes": {
                "commands": [{"id": "preset", "title": "Apply layout preset"}],
                "panels": [{"id": "presets", "title": "Layout presets"}]
            }
        })
    }

    #[test]
    fn there_are_33_distinct_kinds() {
        let mut kinds = KINDS.to_vec();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), 33);
    }

    #[test]
    fn a_manifest_fills_the_registry_and_clear_empties_it_in_one_op() {
        let mut registry = ContributionRegistry::new();
        assert_eq!(
            registry.load_manifest("ilium.core.layout", &manifest()),
            Ok(2)
        );
        assert!(registry
            .find_global("commands", "ilium.core.layout/preset")
            .is_some());
        assert_eq!(registry.take_ops().len(), 2);
        assert_eq!(registry.clear("ilium.core.layout"), 2);
        assert!(registry.is_empty());
        assert_eq!(
            registry.take_ops(),
            vec![ContributionOp::Clear("ilium.core.layout".to_owned())]
        );
    }

    #[test]
    fn an_unknown_kind_adds_nothing() {
        let mut registry = ContributionRegistry::new();
        let bad = json!({"contributes": {"commands": [{"id": "a"}], "widgets": []}});
        assert_eq!(
            registry.load_manifest("example.plugin", &bad),
            Err(ContributionError::UnknownKind("widgets".to_owned()))
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn a_replaced_entry_keeps_its_position() {
        let mut registry = ContributionRegistry::new();
        for name in ["first", "second"] {
            registry
                .put("example.plugin", "commands", name, Side::Client, json!({}))
                .expect("commands is a kind");
        }
        registry
            .put(
                "example.plugin",
                "commands",
                "first",
                Side::Client,
                json!({"enabled": false}),
            )
            .expect("commands is a kind");
        let names: Vec<&str> = registry
            .entries("commands")
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["first", "second"]);
    }

    #[test]
    fn removing_an_absent_entry_records_nothing() {
        let mut registry = ContributionRegistry::new();
        assert!(!registry.remove("example.plugin", "commands", "none"));
        assert_eq!(registry.clear("example.plugin"), 0);
        assert!(registry.take_ops().is_empty());
        assert_eq!(registry.revision(), 0);
    }
}
