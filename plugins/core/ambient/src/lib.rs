//! Core plugin `ilium.core.ambient` (manual page core-plugin-ambient.md).
//!
//! Client side of the Animations tab: the pure row, value, hover, semantic and visibility
//! rules (`rows`, `values`, ...), and the gallery panel. The server side owns the one write to
//! the `animations` kernel tab: the service `ilium.core.ambient/set-setting` validates a value
//! (`set_setting`) and stores it with `kernel-settings.write`. The kernel Settings screen, the
//! scene host and the per-frame look stay in the kernel (see `tests/API_GAPS_ports.md`).

pub mod control;
pub mod credit;
pub mod forward;
pub mod help;
pub mod hover;
pub mod number;
pub mod panel;
pub mod rows;
pub mod semantic;
pub mod set_setting;
pub mod values;
pub mod visibility;

use std::collections::BTreeMap;

use ilium_plugin_sdk::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_sdk::generated::host::log::Level;
use ilium_plugin_sdk::generated::host::services::HostServices;
use ilium_plugin_sdk::generated::host::settings::HostSettings;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{
    Alignment, Area, NodeFlags, Outcome, ParagraphSpec, RequestRef, SettingsOrigin, Span, Style,
    UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, ServiceProvider, Views};

/// The client side of `ilium.core.ambient` (core-plugin-ambient.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct AmbientClient {
    /// The `set-setting` calls still waiting for their reply: the request, and the row it carries.
    pending: BTreeMap<u64, String>,
}

impl Lifecycle for AmbientClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AmbientClient {
    /// A change of one of the plugin's own rows by the user is stored in the Animations kernel
    /// tab through the service `ilium.core.ambient/set-setting`; the client applies it from there.
    /// A change this plugin or a file made is not forwarded again.
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            match event {
                Event::SettingsChanged(change) if change.origin == SettingsOrigin::User => {
                    self.forward_rows(&change.keys);
                }
                Event::RequestCompleted(result) => {
                    if let RequestRef::Call(request) = result.request {
                        self.finish_call(request.0, &result.outcome);
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }
}

impl AmbientClient {
    /// Sends one service call per changed row that reaches the kernel tab.
    fn forward_rows(&mut self, changed: &[String]) {
        let mut api = host();
        let values: serde_json::Value =
            serde_json::from_str(&HostSettings::get_json(&mut *api)).unwrap_or_default();
        for call in forward::calls(changed, &values) {
            match HostServices::call(
                &mut *api,
                forward::SERVICE.to_owned(),
                forward::METHOD.to_owned(),
                call.payload_json,
            ) {
                Ok(request) => {
                    self.pending.insert(request.0, call.key);
                }
                Err(error) => log_failure(&call.key, &error.message),
            }
        }
    }

    /// Reports a refused or failed call; a call that was stored needs no report.
    fn finish_call(&mut self, request: u64, outcome: &Outcome) {
        let Some(key) = self.pending.remove(&request) else {
            return;
        };
        let mut api = host();
        match outcome {
            Outcome::Ok => {
                let reply = HostServices::take_reply(
                    &mut *api,
                    ilium_plugin_sdk::types::RequestId(request),
                );
                if let Some(refusal) = reply.as_deref().and_then(forward::refusal_of) {
                    log_failure(&key, &refusal);
                }
            }
            Outcome::Refused(error) | Outcome::Failed(error) => log_failure(&key, &error.message),
            _ => log_failure(&key, "the call did not complete"),
        }
    }
}

/// Logs that a row could not be stored in the Animations tab.
fn log_failure(key: &str, reason: &str) {
    ilium_plugin_sdk::log::log(
        Level::Warn,
        &format!("ambient: the {key} row was not stored in the Animations tab: {reason}"),
    );
}

impl Views for AmbientClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        gallery_tree()
    }
}

/// The server side of `ilium.core.ambient` (core-plugin-ambient.md "Server part", world `ilium:server/base@0.1.0`).
///
/// It owns the one job of writing the `animations` Settings tab: the client sends a changed value through
/// the service `ilium.core.ambient/set-setting`; this side validates it (`set_setting`) and then writes the kernel tab.
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct AmbientServer;

impl Lifecycle for AmbientServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AmbientServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for AmbientServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        ServiceReply::Done(set_setting::handle(
            &call.method,
            &call.payload_json,
            write_kernel_setting,
        ))
    }
}

/// Stores one validated value in the `animations` kernel tab (grant `settings.write-kernel:animations`).
/// The kernel's own refusal, for example a missing grant, becomes the reply.
fn write_kernel_setting(write: &set_setting::Write) -> Result<(), set_setting::Refusal> {
    let mut api = host();
    HostKernelSettings::write(
        &mut *api,
        set_setting::TAB.to_owned(),
        write.key.clone(),
        write.value_json.clone(),
    )
    .map_err(|error| set_setting::Refusal {
        code: error.code.as_str().to_owned(),
        message: error.message,
    })
}

/// The gallery panel: a single paragraph naming the tab until the scene list can be read.
fn gallery_tree() -> UiTree {
    let spec = ParagraphSpec {
        lines: vec![vec![Span {
            text: "Animations".to_string(),
            style: Style::default(),
        }]],
        rich: None,
        wrap: Some(false),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    };
    UiTree {
        version: 1,
        nodes: vec![UiNode {
            key: None,
            widget: Widget::Paragraph(spec),
            children: Vec::new(),
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }],
        root: 0,
    }
}

#[cfg(test)]
mod spec_tests {

    /// The manifest this plugin ships, parsed from the file next to `src/`.
    fn ambient_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("plugins/core/ambient/manifest.json is valid JSON")
    }

    /// Finds a settings row by its `key` across every tab and section of the manifest.
    fn find_row<'a>(manifest: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        manifest["contributes"]["settings"]
            .as_array()?
            .iter()
            .flat_map(|tab| tab["sections"].as_array().into_iter().flatten())
            .flat_map(|section| section["rows"].as_array().into_iter().flatten())
            .find(|row| row["key"].as_str() == Some(key))
    }

    /// A saved place key must be 1 to 24 characters from `a-z` and `-` (manifest `key_pattern`).
    fn is_valid_saved_place_key(key: &str) -> bool {
        (1..=24).contains(&key.len())
            && key
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '-')
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_ambient_look_plugin() {
        let manifest = ambient_manifest();
        assert_eq!(manifest["id"], "ilium.core.ambient");
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn client_side_exports_only_views() {
        let manifest = ambient_manifest();
        let exports = manifest["client"]["exports"]
            .as_array()
            .expect("client exports is a list");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0], "views");
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_only() {
        let manifest = ambient_manifest();
        assert_eq!(manifest["activation"], serde_json::json!(["startup"]));
    }

    /// REQ: COR-050
    #[test]
    fn client_grants_include_the_manual_table_grants() {
        let manifest = ambient_manifest();
        let grants = manifest["capabilities"]["client"]
            .as_array()
            .expect("client capabilities is a list");
        for required in ["location.geocode", "media.video", "ui.panels"] {
            assert!(
                grants.iter().any(|grant| grant == required),
                "missing client grant {required}"
            );
        }
    }

    /// REQ: COR-030
    #[test]
    fn animations_tab_is_contributed_exactly_once() {
        let manifest = ambient_manifest();
        let tabs = manifest["contributes"]["settings"]
            .as_array()
            .expect("settings is a list");
        let animation_tabs: Vec<_> = tabs
            .iter()
            .filter(|tab| tab["id"] == "animations")
            .collect();
        assert_eq!(animation_tabs.len(), 1);
        assert_eq!(animation_tabs[0]["kind"], "tab");
        assert_eq!(animation_tabs[0]["title"], "Animations");
    }

    /// REQ: COR-050
    #[test]
    fn scene_choice_defaults_to_stars_among_listed_options() {
        let manifest = ambient_manifest();
        let scene = find_row(&manifest, "scene").expect("scene row exists");
        assert_eq!(scene["type"], "choice");
        assert_eq!(scene["default"], "stars");
        let values: Vec<&str> = scene["options"]
            .as_array()
            .expect("scene options is a list")
            .iter()
            .filter_map(|option| option["value"].as_str())
            .collect();
        assert_eq!(values, vec!["stars", "clouds"]);
        assert!(values.contains(&"stars"));
        assert!(find_row(&manifest, "waves").is_none());
    }

    /// REQ: COR-050
    #[test]
    fn observer_location_defaults_to_lisbon() {
        let manifest = ambient_manifest();
        let observer = find_row(&manifest, "observer").expect("observer row exists");
        assert_eq!(observer["type"], "location");
        assert_eq!(observer["precision"].as_u64(), Some(4));
        assert_eq!(observer["default"]["label"], "Lisbon");
        assert_eq!(observer["default"]["lat"].as_f64(), Some(38.7223));
        assert_eq!(observer["default"]["lon"].as_f64(), Some(-9.1393));
    }

    /// REQ: COR-050
    #[test]
    fn saved_place_keys_follow_the_manifest_pattern() {
        let manifest = ambient_manifest();
        let saved_places = find_row(&manifest, "saved_places").expect("saved_places row exists");
        assert_eq!(saved_places["type"], "map");
        assert_eq!(saved_places["key_pattern"], "^[a-z-]{1,24}$");
        assert_eq!(saved_places["max_entries"].as_u64(), Some(8));
        assert!(is_valid_saved_place_key("home"));
        assert!(is_valid_saved_place_key("work-place"));
        assert!(!is_valid_saved_place_key(""));
        assert!(!is_valid_saved_place_key("Harbour"));
        assert!(!is_valid_saved_place_key(&"a".repeat(25)));
    }

    /// REQ: COR-050
    #[test]
    fn scenes_panel_is_contributed_non_modal() {
        let manifest = ambient_manifest();
        let panels = manifest["contributes"]["panels"]
            .as_array()
            .expect("panels is a list");
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0]["id"], "gallery");
        assert_eq!(panels[0]["title"], "Scenes");
        assert_eq!(panels[0]["modal"], false);
    }

    /// Failure case: the manual says the `animations` kernel-settings write grant is server side only
    /// ("the client part never holds it"), so the client capabilities must not list it. The current
    /// manifest.json lists `settings.write-kernel:animations` under `capabilities.client`, so this
    /// test is expected to fail until that grant moves to the server side.
    /// REQ: COR-030
    #[test]
    fn client_side_does_not_hold_the_kernel_settings_write_grant() {
        let manifest = ambient_manifest();
        let client_grants = manifest["capabilities"]["client"]
            .as_array()
            .expect("client capabilities is a list");
        assert!(!client_grants
            .iter()
            .any(|grant| grant == "settings.write-kernel:animations"));
    }
}
