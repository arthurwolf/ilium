//! Exercise macro-generated native callbacks through the public SDK boundary.

use ilium_plugin_sdk as renamed_sdk;
use ilium_plugin_sdk::types::{
    Activation, DeactivationReason, ErrorCode, Event, Intent, PluginError, Submission,
};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, NativeEntry};

#[ilium_plugin(side = "server", exports = ["lifecycle", "handler"])]
#[derive(Default)]
struct ServerPlugin {
    activation: Option<Activation>,
    reason: Option<DeactivationReason>,
    events: Vec<Event>,
}

impl Lifecycle for ServerPlugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        self.activation = Some(context);
        Ok(())
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        self.reason = Some(reason);
        Some(vec![4, 2])
    }
}

impl Handler for ServerPlugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        self.events = batch;
        vec![Submission {
            intent: Intent::PlaySound("ding".to_owned()),
            correlation: 42,
            dedup_key: Some("native-dispatch".to_owned()),
        }]
    }
}

#[ilium_plugin(side = "client", exports = ["lifecycle", "handler"], crate = "renamed_sdk")]
struct ClientPlugin;

impl Lifecycle for ClientPlugin {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Err(PluginError {
            code: ErrorCode::NotPermitted,
            message: "activation rejected".to_owned(),
            retry_after_ms: Some(42),
        })
    }
}

impl Handler for ClientPlugin {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

// Scene callbacks have a distinct contract and must not require Lifecycle or Handler.
#[ilium_plugin(side = "scene", exports = ["renderer"])]
struct ScenePlugin;

#[test]
fn native_server_activation_receives_the_complete_context() {
    let mut plugin = ServerPlugin::default();
    let context = Activation {
        instance: 7,
        side: "server".to_owned(),
        settings_json: "{\"enabled\":true}".to_owned(),
        granted: vec!["tree.read".to_owned()],
        restored: Some(vec![3, 1]),
        ..Activation::default()
    };
    plugin.native_activate(context.clone()).expect("activation");
    assert_eq!(plugin.activation, Some(context));
}

#[test]
fn native_server_deactivation_preserves_reason_and_restore_bytes() {
    let mut plugin = ServerPlugin::default();
    assert_eq!(
        plugin.native_deactivate(DeactivationReason::Upgrade),
        Some(vec![4, 2])
    );
    assert_eq!(plugin.reason, Some(DeactivationReason::Upgrade));
}

#[test]
fn native_server_events_preserve_batch_and_submissions() {
    let mut plugin = ServerPlugin::default();
    let submissions = plugin.native_on_events(vec![Event::ThemeChanged, Event::ThemeChanged]);
    assert_eq!(
        plugin.events,
        vec![Event::ThemeChanged, Event::ThemeChanged]
    );
    assert_eq!(
        submissions,
        vec![Submission {
            intent: Intent::PlaySound("ding".to_owned()),
            correlation: 42,
            dedup_key: Some("native-dispatch".to_owned()),
        }]
    );
}

#[test]
fn native_client_with_renamed_sdk_preserves_activation_error() {
    let error = ClientPlugin
        .native_activate(Activation::default())
        .expect_err("rejected");
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(error.message, "activation rejected");
    assert_eq!(error.retry_after_ms, Some(42));
}

#[test]
fn native_scene_does_not_require_server_lifecycle_traits() {
    let _plugin = ScenePlugin;
    assert_eq!(ScenePlugin::SIDE, "scene");
}
