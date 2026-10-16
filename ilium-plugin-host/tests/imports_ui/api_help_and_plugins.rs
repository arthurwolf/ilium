//! `ilium:host/help` and `ilium:host/plugins` (api-help-and-plugins.md).

use ilium_plugin_api::generated::host::help::HostHelp;
use ilium_plugin_api::generated::host::plugins::HostPlugins;
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["help.read", "plugins.manage"];

fn plugin(id: &str) -> PluginId {
    PluginId(id.to_owned())
}

/// REQ: WIT:ilium:host/help.topic
#[test]
fn wit_help_topic_returns_the_markdown() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_help_topics(&[("getting-started", "# Start\n")]);
    assert_eq!(rig.host.help().topic("getting-started".to_owned()), Some("# Start\n".to_owned()));
}

/// REQ: WIT:ilium:host/help.topic
#[test]
fn wit_help_topic_unknown_id_is_none_without_failing() {
    let mut rig = rig(GRANTS);
    assert_eq!(rig.host.help().topic("no-such-topic".to_owned()), None);
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/help.topic
#[test]
fn wit_help_topic_declined_grant_is_none() {
    let mut rig = declined_rig(&["help.read"]);
    rig.kernel.set_help_topics(&[("a", "text")]);
    assert_eq!(rig.host.help().topic("a".to_owned()), None);
}

/// REQ: WIT:ilium:host/plugins.list
#[test]
fn wit_plugins_list_reports_installed_plugins() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_installed_plugins(&["com.example.a", "com.example.b"]);
    let listed = rig.host.plugins().list();
    assert!(listed.iter().any(|info| info.id == plugin("com.example.a")));
    assert!(listed.iter().any(|info| info.id == plugin("com.example.b")));
}

/// REQ: WIT:ilium:host/plugins.list
#[test]
fn wit_plugins_list_info_carries_the_enabled_flag() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_installed_plugins(&["com.example.a"]);
    rig.host.plugins().set_enabled(plugin("com.example.a"), false, "user".to_owned()).expect("disables");
    let info = rig.host.plugins().list().into_iter().find(|i| i.id == plugin("com.example.a")).expect("listed");
    assert!(!info.enabled);
}

/// REQ: WIT:ilium:host/plugins.list
#[test]
fn wit_plugins_list_declined_grant_is_empty() {
    let mut rig = declined_rig(&["plugins.manage"]);
    rig.kernel.set_installed_plugins(&["com.example.a"]);
    assert!(rig.host.plugins().list().is_empty());
}

/// REQ: WIT:ilium:host/plugins.set-enabled
#[test]
fn wit_plugins_set_enabled_disables_and_enables() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_installed_plugins(&["com.example.a"]);
    rig.host.plugins().set_enabled(plugin("com.example.a"), false, "user".to_owned()).expect("off");
    assert!(!rig.kernel.plugin_enabled("com.example.a"));
    rig.host.plugins().set_enabled(plugin("com.example.a"), true, "user".to_owned()).expect("on");
    assert!(rig.kernel.plugin_enabled("com.example.a"));
}

/// REQ: WIT:ilium:host/plugins.set-enabled
#[test]
fn wit_plugins_set_enabled_same_state_is_ok() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_installed_plugins(&["com.example.a"]);
    assert!(rig.host.plugins().set_enabled(plugin("com.example.a"), true, "user".to_owned()).is_ok());
}

/// REQ: WIT:ilium:host/plugins.set-enabled
#[test]
fn wit_plugins_set_enabled_unknown_plugin_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.plugins().set_enabled(plugin("no.such"), true, "user".to_owned())),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/plugins.set-enabled
#[test]
fn wit_plugins_set_enabled_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["plugins.manage"]);
    assert_eq!(
        code_of(rig.host.plugins().set_enabled(plugin("com.example.a"), false, "user".to_owned())),
        ErrorCode::NotPermitted
    );
}
