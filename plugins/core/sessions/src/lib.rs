//! Core plugin `ilium.core.sessions` (manual page core-plugin-sessions.md).
//!
//! The server side freezes restored panes until every one of them reports `running`, then thaws
//! them together (sample "Freeze panes during a restore"). Resuming agent sessions, the graceful
//! stop and the attach-aware poll need the SDK `host()` calls (agent session, pane list, timers),
//! which the SDK lane has not implemented yet.

use std::collections::BTreeSet;

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::{
    FreezePaneSpec, Intent, Submission, UnfreezePaneSpec,
};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{NodeId, PaneLifecycleState};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// Restored panes that were frozen and the ones that still have to report `running`.
#[derive(Debug, Default)]
struct RestoreWait {
    /// Frozen panes in the order they were restored.
    frozen: Vec<NodeId>,
    /// Frozen panes that have not reported `running` yet.
    waiting: BTreeSet<u64>,
}

impl RestoreWait {
    fn freeze(&mut self, pane: NodeId) -> Option<Submission> {
        if self.frozen.contains(&pane) {
            return None;
        }
        self.frozen.push(pane);
        self.waiting.insert(pane.0);
        Some(Submission::keyed(
            Intent::FreezePane(FreezePaneSpec { pane }),
            format!("freeze-{}", pane.0),
        ))
    }

    /// Records a `running` report; returns the thaw submissions once every frozen pane runs.
    fn running(&mut self, pane: NodeId) -> Vec<Submission> {
        if !self.waiting.remove(&pane.0) || !self.waiting.is_empty() {
            return Vec::new();
        }
        std::mem::take(&mut self.frozen)
            .into_iter()
            .map(|pane| {
                Submission::keyed(
                    Intent::UnfreezePane(UnfreezePaneSpec { pane }),
                    format!("thaw-{}", pane.0),
                )
            })
            .collect()
    }
}

/// The server side of `ilium.core.sessions` (core-plugin-sessions.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SessionsServer {
    activated: bool,
    restore: RestoreWait,
}

impl Lifecycle for SessionsServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for SessionsServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::PaneCreated(info) if info.is_restored => {
                    submissions.extend(self.restore.freeze(info.node));
                }
                Event::PaneLifecycleChanged(change)
                    if change.state == PaneLifecycleState::Running =>
                {
                    submissions.extend(self.restore.running(change.pane));
                }
                _ => {}
            }
        }
        submissions
    }
}

/// The client side of `ilium.core.sessions` (core-plugin-sessions.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct SessionsClient {
    activated: bool,
}

impl Lifecycle for SessionsClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for SessionsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::{json, Value};

    const SESSION_GRANT: &str = "settings.write-kernel:session";

    fn shipped_manifest() -> Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn array_contains(values: &Value, expected: &str) -> bool {
        values
            .as_array()
            .map(|entries| entries.iter().any(|entry| entry.as_str() == Some(expected)))
            .unwrap_or(false)
    }

    /// Returns the sides ("server", "client") whose `capabilities` list declares `grant`.
    fn grant_holders(manifest: &Value, grant: &str) -> Vec<String> {
        ["server", "client"]
            .into_iter()
            .filter(|side| array_contains(&manifest["capabilities"][*side], grant))
            .map(|side| side.to_string())
            .collect()
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> Option<&'a Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"].as_str() == Some(point_id))
    }

    #[test]
    fn manifest_identifies_the_sessions_plugin() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.sessions"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
    }

    #[test]
    fn each_side_names_its_entry_and_world() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(
            manifest["server"]["world"].as_str(),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(
            manifest["client"]["world"].as_str(),
            Some("ilium:client/base@0.1.0")
        );
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_and_the_sessions_view() {
        let manifest = shipped_manifest();
        assert!(array_contains(&manifest["activation"], "startup"));
        assert!(array_contains(
            &manifest["activation"],
            "view:sessions.list"
        ));
    }

    /// the client side needs no grants of its own). Fails until the shipped manifest moves the grant to the server side.
    #[test]
    fn server_side_holds_the_session_settings_grant() {
        let manifest = shipped_manifest();
        assert_eq!(
            grant_holders(&manifest, SESSION_GRANT),
            vec!["server".to_string()]
        );
    }

    /// `settings.write-kernel:session`). Empty input: a manifest without the grant has no owner, so the tab stays read-only.
    /// REQ: COR-030
    #[test]
    fn session_tab_has_exactly_one_owner_and_none_without_a_grant() {
        let manifest = shipped_manifest();
        assert_eq!(grant_holders(&manifest, SESSION_GRANT).len(), 1);
        assert!(grant_holders(&json!({}), SESSION_GRANT).is_empty());
    }

    /// `manifest.settings-owner`, so a manifest that declares it on both sides is not a valid single owner).
    /// REQ: COR-030
    #[test]
    fn duplicate_session_grant_on_both_sides_is_not_a_single_owner() {
        let duplicate = json!({
            "capabilities": {
                "server": [SESSION_GRANT],
                "client": [SESSION_GRANT]
            }
        });
        assert_ne!(grant_holders(&duplicate, SESSION_GRANT).len(), 1);
    }

    #[test]
    fn server_capabilities_cover_pane_tree_and_agent_work() {
        let manifest = shipped_manifest();
        for capability in [
            "panes.read",
            "panes.lifecycle",
            "tree.restructure",
            "agent.read",
        ] {
            assert!(
                array_contains(&manifest["capabilities"]["server"], capability),
                "server capabilities are missing {capability}"
            );
        }
    }

    /// on the server side).
    /// REQ: EXT-052
    #[test]
    fn server_grants_both_sessions_extension_points() {
        let manifest = shipped_manifest();
        assert!(array_contains(
            &manifest["capabilities"]["server"],
            "extensions.provide:ilium.core.sessions/snapshot-sections"
        ));
        assert!(array_contains(
            &manifest["capabilities"]["server"],
            "extensions.provide:ilium.core.sessions/before-restore"
        ));
    }

    /// version 0.1.0, deadline 20 ms, 8 handlers).
    /// REQ: EXT-052
    #[test]
    fn extension_points_declare_the_documented_limits() {
        let manifest = shipped_manifest();
        let snapshot =
            extension_point(&manifest, "snapshot-sections").expect("snapshot-sections is declared");
        assert_eq!(snapshot["version"].as_str(), Some("0.1.0"));
        assert_eq!(snapshot["max_entries"].as_u64(), Some(64));
        assert_eq!(snapshot["dynamic"].as_bool(), Some(false));

        let before_restore =
            extension_point(&manifest, "before-restore").expect("before-restore is declared");
        assert_eq!(before_restore["version"].as_str(), Some("0.1.0"));
        assert_eq!(before_restore["deadline_ms"].as_u64(), Some(20));
        assert_eq!(before_restore["max_handlers"].as_u64(), Some(8));
    }
}
