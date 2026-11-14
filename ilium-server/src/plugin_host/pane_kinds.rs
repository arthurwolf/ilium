//! The plugin pane kinds the catalogue contributes (gap G14a).
//!
//! A plugin declares a pane kind in the `pane_kinds` entry of its manifest. Its global id is
//! `<plugin-id>.<local-id>` and its panes are `PaneContentKind::Plugin` nodes with no server
//! process: the plugin draws their body on the client. This table answers the two questions
//! the server asks before it creates such a pane: does the kind exist, and which plugin owns it.

use std::collections::BTreeMap;

use ilium_core::PluginPaneKind;
use serde_json::Value;

/// One pane kind of the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneKindEntry {
    /// The plugin that provides the kind and draws its panes.
    pub(crate) plugin: String,
    /// The display name of the kind (the manifest's `title`), the default pane title.
    pub(crate) title: String,
}

/// Every basic pane kind of the catalogue, by global kind id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PaneKinds {
    by_id: BTreeMap<String, PaneKindEntry>,
}

impl PaneKinds {
    /// Reads the `pane_kinds` of every manifest, given as `(plugin id, manifest JSON)`. A
    /// manifest that does not parse, an entry with another primitive than `basic`, or an id
    /// that does not make a valid global kind id contributes nothing: the manifest check
    /// already refuses those, and a pane nobody can draw must never be created.
    pub(crate) fn of_manifests<'a>(
        manifests: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let mut by_id = BTreeMap::new();
        for (plugin, manifest) in manifests {
            let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
                continue;
            };
            let Some(kinds) = manifest
                .pointer("/contributes/pane_kinds")
                .and_then(Value::as_array)
            else {
                continue;
            };
            for kind in kinds {
                let Some(local) = kind.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let is_basic = kind
                    .get("primitive")
                    .and_then(Value::as_str)
                    .is_none_or(|primitive| primitive == "basic");
                if !is_basic {
                    continue;
                }
                let Ok(global) = PluginPaneKind::new(format!("{plugin}.{local}")) else {
                    continue;
                };
                // The owner is what the id says: a plugin can never declare a kind that reads
                // as another plugin's.
                if global.plugin() != plugin {
                    continue;
                }
                let title = kind
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(local)
                    .to_owned();
                by_id.insert(
                    global.kind_id().to_owned(),
                    PaneKindEntry {
                        plugin: plugin.to_owned(),
                        title,
                    },
                );
            }
        }
        Self { by_id }
    }

    /// The kind with this global id, if a catalogue plugin contributes it.
    pub(crate) fn get(&self, kind_id: &str) -> Option<&PaneKindEntry> {
        self.by_id.get(kind_id)
    }

    /// Checks a request for a pane of `kind_id`. `requester` is the plugin that asks, or `None`
    /// for the user (a client menu): a plugin may create panes of its own kinds only.
    pub(crate) fn admit(
        &self,
        kind_id: &str,
        requester: Option<&str>,
    ) -> Result<(PluginPaneKind, &PaneKindEntry), String> {
        let entry = self
            .get(kind_id)
            .ok_or_else(|| format!("no plugin contributes the pane kind {kind_id}"))?;
        if requester.is_some_and(|plugin| plugin != entry.plugin) {
            return Err(format!(
                "the pane kind {kind_id} belongs to {}, not to the plugin that asked",
                entry.plugin
            ));
        }
        let kind = PluginPaneKind::new(kind_id).map_err(|error| error.to_string())?;
        Ok((kind, entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KANBAN: &str = r#"{
        "id": "ilium.core.kanban",
        "contributes": {
            "pane_kinds": [
                {"id": "board", "title": "Board", "primitive": "basic", "wants_keys": true}
            ]
        }
    }"#;

    #[test]
    fn a_basic_pane_kind_is_found_by_its_global_id() {
        let kinds = PaneKinds::of_manifests([("ilium.core.kanban", KANBAN)]);
        let entry = kinds.get("ilium.core.kanban.board").unwrap();
        assert_eq!(entry.plugin, "ilium.core.kanban");
        assert_eq!(entry.title, "Board");
        assert!(kinds.get("ilium.core.kanban.list").is_none());
        assert!(kinds.get("board").is_none());
    }

    #[test]
    fn only_the_owner_or_the_user_may_create_a_kind() {
        let kinds = PaneKinds::of_manifests([("ilium.core.kanban", KANBAN)]);
        let (kind, entry) = kinds.admit("ilium.core.kanban.board", None).unwrap();
        assert_eq!(kind.plugin(), "ilium.core.kanban");
        assert_eq!(entry.title, "Board");
        assert!(kinds
            .admit("ilium.core.kanban.board", Some("ilium.core.kanban"))
            .is_ok());
        assert!(kinds
            .admit("ilium.core.kanban.board", Some("example.other"))
            .unwrap_err()
            .contains("belongs to ilium.core.kanban"));
        assert!(kinds
            .admit("example.other.board", None)
            .unwrap_err()
            .contains("no plugin contributes"));
    }

    #[test]
    fn broken_manifests_and_other_primitives_contribute_nothing() {
        let terminal_kind = r#"{"contributes": {"pane_kinds": [
            {"id": "tty", "title": "TTY", "primitive": "terminal"},
            {"id": "Bad Id", "title": "Bad"},
            {"title": "no id"}
        ]}}"#;
        let kinds = PaneKinds::of_manifests([
            ("example.broken", "{not json"),
            ("example.tty", terminal_kind),
            ("example.empty", "{}"),
        ]);
        assert_eq!(kinds, PaneKinds::default());
    }

    #[test]
    fn a_missing_primitive_means_basic_and_a_missing_title_falls_back_to_the_id() {
        let notes = r#"{"contributes": {"pane_kinds": [{"id": "note"}]}}"#;
        let kinds = PaneKinds::of_manifests([("example.notes", notes)]);
        assert_eq!(kinds.get("example.notes.note").unwrap().title, "note");
    }
}
