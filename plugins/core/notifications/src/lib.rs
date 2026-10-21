//! Core plugin `ilium.core.notifications` (manual page core-plugin-notifications.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{Area, UiTree, ViewTarget};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The server side of `ilium.core.notifications` (core-plugin-notifications.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct NotificationsServer;

impl Lifecycle for NotificationsServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.notifications: server lifecycle.activate")
    }
}

impl Handler for NotificationsServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.notifications: server handler.on_events")
    }
}

/// The client side of `ilium.core.notifications` (core-plugin-notifications.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct NotificationsClient;

impl Lifecycle for NotificationsClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.notifications: client lifecycle.activate")
    }
}

impl Handler for NotificationsClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.notifications: client handler.on_events")
    }
}

impl Views for NotificationsClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.notifications: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_sdk::generated::types::events::AgentActivityEvent;
    use ilium_plugin_sdk::generated::types::ids::NodeId;
    use ilium_plugin_sdk::generated::types::intents::Intent;
    use ilium_plugin_sdk::generated::types::tree::AgentActivity;
    use serde_json::{json, Value};

    const MANIFEST_JSON: &str = include_str!("../manifest.json");

    fn load_manifest() -> Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json must parse as JSON")
    }

    /// Returns the grant strings a manifest lists for one side ("server" or "client").
    fn grants_of(manifest: &Value, side: &str) -> Vec<String> {
        manifest["capabilities"][side].as_array().map_or_else(Vec::new, |grants| {
            grants.iter().filter_map(|grant| grant.as_str().map(str::to_owned)).collect()
        })
    }

    /// Returns the manifest's extension point declaration with the given id, if any.
    fn extension_point<'manifest>(manifest: &'manifest Value, point_id: &str) -> Option<&'manifest Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"] == point_id)
    }

    /// Builds one agent activity transition as the server receives it.
    fn agent_activity_event(node: u64, previous: AgentActivity, current: AgentActivity, revision: u64) -> Event {
        Event::AgentActivityChanged(AgentActivityEvent { node: NodeId(node), previous, current, revision })
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_notifications_plugin() {
        let manifest = load_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.notifications");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["limits_tier"], "first-party");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activates_at_startup_with_the_documented_capabilities() {
        let manifest = load_manifest();
        assert_eq!(manifest["activation"], json!(["startup"]));

        let server_grants = grants_of(&manifest, "server");
        for documented_grant in ["sound.play", "notify.desktop", "agent.read", "tree.read", "tree.attention"] {
            assert!(
                server_grants.iter().any(|held| held == documented_grant),
                "server side must hold {documented_grant}"
            );
        }
        let client_grants = grants_of(&manifest, "client");
        assert!(client_grants.iter().any(|held| held == "notify.toast"), "client side must hold notify.toast");
    }

    /// REQ: COR-030, COR-033
    #[test]
    fn server_side_holds_the_single_sound_tab_write_grant() {
        let manifest = load_manifest();
        let sound_tab_grant = "settings.write-kernel:sound";
        let holding_sides: Vec<&str> = ["server", "client"]
            .into_iter()
            .filter(|side| grants_of(&manifest, side).iter().any(|held| held == sound_tab_grant))
            .collect();
        assert_eq!(holding_sides, vec!["server"], "exactly one holder, on the server side");
    }

    /// REQ: EXT-052, SEC-293
    #[test]
    fn before_notify_is_a_standard_risk_veto_hook_with_a_20_ms_deadline() {
        let manifest = load_manifest();
        let before_notify = extension_point(&manifest, "before-notify").expect("before-notify must be declared");
        assert_eq!(before_notify["kind"], "hook");
        assert_eq!(before_notify["mode"], "veto");
        assert_eq!(before_notify["deadline_ms"], 20);
        assert_eq!(before_notify["max_handlers"], 8);
        assert_eq!(before_notify["version"], "0.1.0");
        assert_eq!(before_notify["risk"], "standard");
    }

    /// REQ: EXT-052, SEC-293
    #[test]
    fn channels_is_a_high_risk_collect_point_capped_at_64_entries() {
        let manifest = load_manifest();
        let channels = extension_point(&manifest, "channels").expect("channels must be declared");
        assert_eq!(channels["kind"], "collect");
        assert_eq!(channels["risk"], "high");
        assert_eq!(channels["max_entries"], 64);
        assert_eq!(channels["dynamic"], false);
    }

    #[test]
    fn approval_prompt_submits_one_sound_keyed_to_the_prompt() {
        let mut server = NotificationsServer::default();
        let batch = vec![agent_activity_event(7, AgentActivity::Working, AgentActivity::AwaitingApproval, 3)];

        let submissions = server.on_events(batch);

        assert_eq!(submissions.len(), 1, "one approval prompt, one sound");
        assert!(
            matches!(submissions[0].intent, Intent::PlaySound(ref sound) if !sound.is_empty()),
            "the submission must play the sound chosen in Settings"
        );
        assert_eq!(submissions[0].dedup_key.as_deref(), Some("ask:7:3"));
    }

    #[test]
    fn a_new_revision_of_the_same_node_is_sounded_again() {
        let mut server = NotificationsServer::default();
        let batch = vec![
            agent_activity_event(7, AgentActivity::Working, AgentActivity::AwaitingApproval, 3),
            agent_activity_event(7, AgentActivity::Working, AgentActivity::AwaitingApproval, 4),
        ];

        let submissions = server.on_events(batch);

        assert_eq!(submissions.len(), 2);
        assert_ne!(submissions[0].dedup_key, submissions[1].dedup_key);
    }

    #[test]
    fn empty_batch_submits_nothing() {
        let mut server = NotificationsServer::default();
        assert!(server.on_events(Vec::new()).is_empty());
    }

    /// Manual: failure case: a transition that is not an approval prompt
    #[test]
    fn transition_to_idle_does_not_sound_an_approval_prompt() {
        let mut server = NotificationsServer::default();
        let batch = vec![agent_activity_event(7, AgentActivity::Working, AgentActivity::Idle, 5)];
        assert!(server.on_events(batch).is_empty());
    }
}
