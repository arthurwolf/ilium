//! Unit gaps of the `ilium-plugin-sdk` facade: the `ilium_plugin` attribute re-export, the
//! `testing` re-export behind the `testing` feature, and the glob re-export of
//! `ilium-plugin-sdk-core` (rust-sdk.md "The ilium_plugin macro", "Modules, import paths").
//!
//! The facade has no logic of its own, so these tests reach every item through the
//! `ilium_plugin_sdk` path only. Functions that are `unimplemented!()` stubs in core are
//! referenced by path and never called.

use ilium_plugin_sdk::ilium_plugin;
use ilium_plugin_sdk::types::{Activation, PluginError};
use ilium_plugin_sdk::{Lifecycle, NativeEntry};
use std::any::type_name;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct MinimalServerPlugin;

#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
struct EmptyExportsServerPlugin;

#[ilium_plugin(side = "client", exports = ["views", "catalog-provider", "token-provider"])]
#[derive(Default)]
struct ClientPlugin {
    _state: u32,
}

#[ilium_plugin(side = "scene", exports = ["renderer"])]
#[derive(Default)]
struct ScenePlugin;

macro_rules! quiet_lifecycle {
    ($($plugin:ty),*) => {$(
        impl Lifecycle for $plugin {
            fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
                Ok(())
            }
        }
    )*};
}
quiet_lifecycle!(
    MinimalServerPlugin,
    EmptyExportsServerPlugin,
    ClientPlugin,
    ScenePlugin
);

fn assert_native_entry<Plugin: NativeEntry>() {}

#[test]
fn ilium_plugin_attribute_sets_server_side_on_minimal_struct() {
    assert_eq!(<MinimalServerPlugin as NativeEntry>::SIDE, "server");
}

#[test]
fn ilium_plugin_attribute_without_exports_yields_no_export_names() {
    assert!(
        <MinimalServerPlugin as NativeEntry>::EXPORTS.is_empty(),
        "omitted exports must expand to an empty list, got {:?}",
        <MinimalServerPlugin as NativeEntry>::EXPORTS
    );
}

#[test]
fn ilium_plugin_attribute_with_empty_exports_list_yields_no_export_names() {
    assert_eq!(<EmptyExportsServerPlugin as NativeEntry>::SIDE, "server");
    assert!(
        <EmptyExportsServerPlugin as NativeEntry>::EXPORTS.is_empty(),
        "an empty exports list must expand to an empty list"
    );
}

#[test]
fn ilium_plugin_attribute_records_client_exports_in_declared_order() {
    assert_eq!(<ClientPlugin as NativeEntry>::SIDE, "client");
    assert_eq!(
        <ClientPlugin as NativeEntry>::EXPORTS,
        &["views", "catalog-provider", "token-provider"]
    );
}

#[test]
fn ilium_plugin_attribute_accepts_scene_side_with_renderer_export() {
    assert_eq!(<ScenePlugin as NativeEntry>::SIDE, "scene");
    assert_eq!(<ScenePlugin as NativeEntry>::EXPORTS, &["renderer"]);
}

#[test]
fn ilium_plugin_expansion_keeps_user_struct_and_its_derives() {
    let plugin = ClientPlugin::default();

    assert_eq!(
        plugin._state, 0,
        "the user's field must survive the expansion"
    );
}

#[test]
fn ilium_plugin_expansion_implements_native_entry_for_declared_struct() {
    assert_native_entry::<MinimalServerPlugin>();
    assert_native_entry::<ClientPlugin>();
    assert_native_entry::<ScenePlugin>();
}

#[test]
fn core_dedup_key_for_resolves_through_facade_path() {
    let key_for: fn(&[&str]) -> String = ilium_plugin_sdk::dedup::key_for;

    assert!(
        type_name::<fn(&[&str]) -> String>().contains("fn"),
        "dedup::key_for must be a function item reachable from the facade"
    );
    let _ = key_for;
}

#[test]
fn core_types_module_event_and_submission_resolve_through_facade() {
    let event_name = type_name::<ilium_plugin_sdk::types::Event>();
    let submission_name = type_name::<ilium_plugin_sdk::types::Submission>();

    assert!(event_name.ends_with("Event"), "got: {event_name}");
    assert!(
        submission_name.ends_with("Submission"),
        "got: {submission_name}"
    );
}

mod prelude_names_through_facade {
    use ilium_plugin_sdk::prelude::*;
    use std::any::type_name;

    #[test]
    fn prelude_glob_exposes_event_submission_and_intent_at_facade_path() {
        let event_name = type_name::<Event>();
        let submission_name = type_name::<Submission>();
        let intent_name = type_name::<Intent>();

        assert!(event_name.ends_with("Event"), "got: {event_name}");
        assert!(
            submission_name.ends_with("Submission"),
            "got: {submission_name}"
        );
        assert!(intent_name.ends_with("Intent"), "got: {intent_name}");
    }
}

#[cfg(feature = "testing")]
#[test]
fn testing_reexport_resolves_to_testkit_types_when_feature_enabled() {
    let screen_name = type_name::<ilium_plugin_sdk::testing::Screen>();
    let handle_name = type_name::<ilium_plugin_sdk::testing::PluginHandle>();

    assert!(screen_name.ends_with("Screen"), "got: {screen_name}");
    assert!(handle_name.ends_with("PluginHandle"), "got: {handle_name}");
}
