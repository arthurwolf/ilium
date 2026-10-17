//! api-settings.md and capability-settings-all.md: `get-json`, `set-json`, `list-settings`,
//! `get-plugin-json`, `set-plugin-json`, `open`, `set-choices` and `set-choice-items`.
//!
//! WIT ids: `ilium:host/settings` functions up to `set-choices`. `set-info` onward is part 2.

use ilium_plugin_api::generated::host::{ChoiceOption, HostSettings};
use ilium_plugin_api::generated::types::{
    CatalogItem, ErrorCode, Event, PluginId, Preview, SettingsOrigin,
};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use serde_json::{json, Value};

use crate::support::*;

const READ_ALL: &str = "settings.read-all";
const WRITE_ALL: &str = "settings.write-all";

fn get(fixture: &mut ImportsFixture) -> Value {
    serde_json::from_str(&fixture.settings().get_json()).expect("get-json is one JSON object")
}

fn set(fixture: &mut ImportsFixture, patch: &str) -> Result<(), ilium_plugin_api::PluginError> {
    fixture.settings().set_json(patch.to_owned())
}

fn other_plugin_manifest() -> String {
    let mut manifest = manifest_with_rows(vec![
        number("level", 0, 10, 3),
        json!({ "type": "text", "key": "token", "title": "Token", "secret": true,
                "max_length": 64, "default": "" }),
    ]);
    manifest["id"] = json!("test.other");
    manifest.to_string()
}

fn no_settings_manifest() -> String {
    let mut manifest = minimal_manifest();
    manifest["id"] = json!("test.plain");
    manifest.to_string()
}

fn admin_fixture(grants: &[&str]) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T1)
        .grants(grants)
        .manifest_json(&runtime_settings_manifest().to_string())
        .also_install(&other_plugin_manifest())
        .also_install(&no_settings_manifest())
        .build()
}

fn changes_of(fixture: &ImportsFixture, plugin: &str) -> Vec<(Vec<String>, SettingsOrigin)> {
    fixture
        .events_of(plugin)
        .into_iter()
        .filter_map(|event| match event {
            Event::SettingsChanged(change) => Some((change.keys, change.origin)),
            _ => None,
        })
        .collect()
}

// ---- get-json --------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.get-json
#[test]
fn settings_get_json_without_a_settings_entry_is_an_empty_object() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&minimal_manifest().to_string())
        .build();
    assert_eq!(get(&mut fixture), json!({}));
}

/// REQ: WIT:ilium:host/settings.get-json
#[test]
fn settings_get_json_unset_keys_read_as_their_defaults() {
    let mut fixture = settings_fixture(Tier::T2);
    let values = get(&mut fixture);
    assert_eq!(values["enabled"], json!(true));
    assert_eq!(values["count"], json!(5));
    assert_eq!(values["label"], json!("work"));
    assert_eq!(values["mode"], json!("fast"));
}

/// REQ: WIT:ilium:host/settings.get-json
#[test]
fn settings_get_json_reflects_a_set_json_write() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(&mut fixture, r#"{"label": "home"}"#));
    assert_eq!(get(&mut fixture)["label"], json!("home"));
}

/// REQ: WIT:ilium:host/settings.get-json
#[test]
fn settings_get_json_is_one_object_with_only_value_rows() {
    let mut fixture = settings_fixture(Tier::T2);
    let values = get(&mut fixture);
    let object = values.as_object().expect("an object");
    assert!(!object.contains_key("status"), "an info row has no value");
    assert!(!object.contains_key("usage"), "a meter row has no value");
}

/// REQ: WIT:ilium:host/settings.get-json
#[test]
fn settings_get_json_returns_only_the_keys_of_the_calling_side() {
    let mut manifest = manifest_with_tab(vec![
        json!({ "id": "client-part", "title": "Client", "scope": "client",
                "rows": [toggle("client_key", true)] }),
        json!({ "id": "server-part", "title": "Server", "scope": "server",
                "rows": [toggle("server_key", true)] }),
    ]);
    manifest["client"] = json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0" });
    let mut server = ImportsFixture::builder()
        .tier(Tier::T2)
        .side(ilium_plugin_api::side::Side::Server)
        .manifest_json(&manifest.to_string())
        .build();
    let values = get(&mut server);
    assert!(values.get("server_key").is_some());
    assert!(values.get("client_key").is_none());
}

// ---- set-json --------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_replaces_the_given_key_and_leaves_absent_keys_unchanged() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(&mut fixture, r#"{"count": 7}"#));
    let values = get(&mut fixture);
    assert_eq!(values["count"], json!(7));
    assert_eq!(values["label"], json!("work"));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_null_resets_the_key_to_its_default() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(&mut fixture, r#"{"count": 7}"#));
    expect_ok(set(&mut fixture, r#"{"count": null}"#));
    assert_eq!(get(&mut fixture)["count"], json!(5));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_empty_object_succeeds_and_changes_nothing() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(&mut fixture, "{}"));
    fixture.pump();
    assert!(changes_of(&fixture, PLUGIN).is_empty());
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_that_changes_no_value_sends_no_settings_changed() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(&mut fixture, r#"{"count": 5}"#));
    fixture.pump();
    assert!(changes_of(&fixture, PLUGIN).is_empty());
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_event_has_origin_plugin_and_sorted_keys() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(
        &mut fixture,
        r#"{"mode": "slow", "count": 9, "enabled": false}"#,
    ));
    fixture.pump();
    let changes = changes_of(&fixture, PLUGIN);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, vec!["count", "enabled", "mode"]);
    assert_eq!(changes[0].1, SettingsOrigin::Plugin);
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_not_an_object_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    for patch in ["[1]", "3", "\"text\"", "null", "true", "not json", ""] {
        assert_eq!(
            expect_code(set(&mut fixture, patch)),
            ErrorCode::InvalidArgument,
            "patch {patch:?}"
        );
    }
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_unknown_key_invalid_argument_naming_the_key() {
    let mut fixture = settings_fixture(Tier::T2);
    let error = expect_error(set(&mut fixture, r#"{"no_such_key": 1}"#));
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.message.contains("no_such_key"), "{}", error.message);
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_one_invalid_key_stores_nothing() {
    let mut fixture = settings_fixture(Tier::T2);
    let code = expect_code(set(&mut fixture, r#"{"label": "changed", "count": 99}"#));
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert_eq!(get(&mut fixture)["label"], json!("work"));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_integer_row_rejects_4_5_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    assert_eq!(
        expect_code(set(&mut fixture, r#"{"count": 4.5}"#)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_number_row_accepts_an_integer_value() {
    let mut row = number("ratio", 0, 10, 1);
    row["integer"] = json!(false);
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&manifest_with_rows(vec![row]).to_string())
        .build();
    expect_ok(set(&mut fixture, r#"{"ratio": 4}"#));
    expect_ok(set(&mut fixture, r#"{"ratio": 4.5}"#));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_value_outside_the_number_range_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    assert_eq!(
        expect_code(set(&mut fixture, r#"{"count": 11}"#)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        expect_code(set(&mut fixture, r#"{"count": -1}"#)),
        ErrorCode::InvalidArgument
    );
    expect_ok(set(&mut fixture, r#"{"count": 10}"#));
    expect_ok(set(&mut fixture, r#"{"count": 0}"#));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_choice_value_not_offered_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    assert_eq!(
        expect_code(set(&mut fixture, r#"{"mode": "medium"}"#)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_patch_of_exactly_256_kib_is_not_refused_for_its_size() {
    let mut fixture = settings_fixture(Tier::T2);
    let body = r#"{"label": "w"}"#;
    let patch = format!("{body}{}", " ".repeat(256 * KIB - body.len()));
    assert_eq!(patch.len(), 256 * KIB);
    expect_ok(set(&mut fixture, &patch));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_patch_of_256_kib_plus_one_byte_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    let body = r#"{"label": "w"}"#;
    let patch = format!("{body}{}", " ".repeat(256 * KIB + 1 - body.len()));
    assert_eq!(patch.len(), 256 * KIB + 1);
    assert_eq!(
        expect_code(set(&mut fixture, &patch)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_key_of_the_other_side_invalid_argument() {
    let mut manifest = manifest_with_tab(vec![
        json!({ "id": "client-part", "title": "Client", "scope": "client",
                "rows": [toggle("client_key", true)] }),
        json!({ "id": "server-part", "title": "Server", "scope": "server",
                "rows": [toggle("server_key", true)] }),
    ]);
    manifest["client"] = json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0" });
    let mut server = ImportsFixture::builder()
        .tier(Tier::T2)
        .side(ilium_plugin_api::side::Side::Server)
        .manifest_json(&manifest.to_string())
        .build();
    assert_eq!(
        expect_code(set(&mut server, r#"{"client_key": false}"#)),
        ErrorCode::InvalidArgument
    );
    expect_ok(set(&mut server, r#"{"server_key": false}"#));
}

/// REQ: WIT:ilium:host/settings.set-json
#[test]
fn settings_set_json_runs_no_settings_validate_for_the_plugin_origin() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = calls.clone();
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(move |_patch, _merged| {
            *counter.lock().unwrap() += 1;
            Ok(())
        }))
        .build();
    expect_ok(set(&mut fixture, r#"{"label": "x"}"#));
    assert_eq!(*calls.lock().unwrap(), 0);
}

// ---- list-settings ---------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.list-settings
#[test]
fn settings_list_settings_without_the_grant_returns_nothing_and_logs_not_permitted() {
    let mut fixture = admin_fixture(&[]);
    assert!(fixture.settings().list_settings().is_empty());
    assert!(fixture
        .logs()
        .iter()
        .any(|line| line.message.contains("settings.read-all")));
}

/// REQ: WIT:ilium:host/settings.list-settings
#[test]
fn settings_list_settings_lists_tabs_of_every_plugin_with_settings() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let refs = fixture.settings().list_settings();
    let plugins: Vec<String> = refs.iter().map(|entry| entry.plugin.0.clone()).collect();
    assert!(plugins.contains(&"test.imports".to_owned()));
    assert!(plugins.contains(&"test.other".to_owned()));
}

/// REQ: WIT:ilium:host/settings.list-settings
#[test]
fn settings_list_settings_leaves_out_plugins_without_settings() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let refs = fixture.settings().list_settings();
    assert!(refs.iter().all(|entry| entry.plugin.0 != "test.plain"));
}

/// REQ: WIT:ilium:host/settings.list-settings
#[test]
fn settings_list_settings_tab_ids_are_global_plugin_slash_tab() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let refs = fixture.settings().list_settings();
    let own = refs
        .iter()
        .find(|entry| entry.plugin.0 == "test.other")
        .expect("test.other listed");
    assert_eq!(own.tab, "test.other/main");
    assert!(!own.title.is_empty());
}

/// REQ: WIT:ilium:host/settings.list-settings
#[test]
fn settings_list_settings_section_in_a_built_in_tab_names_the_kernel_tab() {
    let mut contributor = minimal_manifest();
    contributor["id"] = json!("test.contributor");
    contributor["capabilities"] = json!({ "server": ["settings.contribute:terminal"] });
    contributor["contributes"] = json!({ "settings": [{
        "kind": "section", "id": "extra", "tab": "terminal", "title": "Extra",
        "scope": "server", "rows": [toggle("flag", true)]
    }] });
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T1)
        .grants(&[READ_ALL])
        .also_install(&contributor.to_string())
        .build();
    let refs = fixture.settings().list_settings();
    let entry = refs
        .iter()
        .find(|entry| entry.plugin.0 == "test.contributor")
        .expect("the section is listed");
    assert_eq!(entry.kernel_tab.as_deref(), Some("terminal"));
    assert!(entry.section.is_some());
}

// ---- get-plugin-json -------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.get-plugin-json
#[test]
fn settings_get_plugin_json_without_the_grant_not_permitted() {
    let mut fixture = admin_fixture(&[]);
    let code = expect_code(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.other".to_owned())),
    );
    assert_eq!(code, ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/settings.get-plugin-json
#[test]
fn settings_get_plugin_json_returns_all_values_of_the_other_plugin() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let text = expect_ok(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.other".to_owned())),
    );
    let values: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(values["level"], json!(3));
}

/// REQ: WIT:ilium:host/settings.get-plugin-json
#[test]
fn settings_get_plugin_json_unknown_plugin_not_found() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let code = expect_code(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.ghost".to_owned())),
    );
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.get-plugin-json
#[test]
fn settings_get_plugin_json_plugin_without_settings_is_an_empty_object() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let text = expect_ok(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.plain".to_owned())),
    );
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), json!({}));
}

/// REQ: WIT:ilium:host/settings.get-plugin-json
#[test]
fn settings_get_plugin_json_secret_values_are_empty_strings_for_other_plugins() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    expect_ok(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"token": "hunter2"}"#.to_owned(),
    ));
    let text = expect_ok(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.other".to_owned())),
    );
    let values: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(values["token"], json!(""));
    assert!(!text.contains("hunter2"));
}

// ---- set-plugin-json -------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_without_the_grant_not_permitted() {
    let mut fixture = admin_fixture(&[READ_ALL]);
    let code = expect_code(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": 4}"#.to_owned(),
    ));
    assert_eq!(code, ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_applies_a_merge_patch_and_notifies_the_target() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    expect_ok(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": 4}"#.to_owned(),
    ));
    fixture.pump();
    let changes = changes_of(&fixture, "test.other");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, vec!["level".to_owned()]);
    assert_eq!(changes[0].1, SettingsOrigin::OtherPlugin);
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_the_writer_gets_no_event_of_its_own() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    expect_ok(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": 4}"#.to_owned(),
    ));
    fixture.pump();
    assert!(changes_of(&fixture, PLUGIN).is_empty());
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_unknown_plugin_not_found() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    let code = expect_code(fixture.settings().set_plugin_json(
        PluginId("test.ghost".to_owned()),
        r#"{"level": 4}"#.to_owned(),
    ));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_key_outside_the_schema_invalid_argument_naming_it() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    let error = expect_error(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"nope": 1}"#.to_owned(),
    ));
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.message.contains("nope"));
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_value_breaking_the_schema_invalid_argument_and_stores_nothing() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    let code = expect_code(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": 99}"#.to_owned(),
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
    let text = expect_ok(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.other".to_owned())),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["level"],
        json!(3)
    );
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_non_empty_patch_to_a_plugin_without_settings_invalid_argument() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    let code = expect_code(
        fixture
            .settings()
            .set_plugin_json(PluginId("test.plain".to_owned()), r#"{"x": 1}"#.to_owned()),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_patch_that_changes_nothing_sends_no_event() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    expect_ok(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": 3}"#.to_owned(),
    ));
    fixture.pump();
    assert!(changes_of(&fixture, "test.other").is_empty());
}

/// REQ: WIT:ilium:host/settings.set-plugin-json
#[test]
fn settings_set_plugin_json_null_resets_the_key_to_its_default() {
    let mut fixture = admin_fixture(&[READ_ALL, WRITE_ALL]);
    expect_ok(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": 8}"#.to_owned(),
    ));
    expect_ok(fixture.settings().set_plugin_json(
        PluginId("test.other".to_owned()),
        r#"{"level": null}"#.to_owned(),
    ));
    let text = expect_ok(
        fixture
            .settings()
            .get_plugin_json(PluginId("test.other".to_owned())),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["level"],
        json!(3)
    );
}

// ---- open ------------------------------------------------------------------------------------

fn client_fixture() -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T2)
        .side(ilium_plugin_api::side::Side::Client)
        .manifest_json(&runtime_settings_manifest().to_string())
        .also_install(&other_plugin_manifest())
        .build()
}

/// REQ: WIT:ilium:host/settings.open
#[test]
fn settings_open_own_tab_opens_settings_at_that_tab() {
    let mut fixture = client_fixture();
    expect_ok(fixture.settings().open("test.imports/main".to_owned()));
    assert_eq!(
        fixture.opened_settings_target().as_deref(),
        Some("test.imports/main")
    );
}

/// REQ: WIT:ilium:host/settings.open
#[test]
fn settings_open_own_section_opens_settings_at_that_section() {
    let mut fixture = client_fixture();
    expect_ok(fixture.settings().open("test.imports/general".to_owned()));
    assert_eq!(
        fixture.opened_settings_target().as_deref(),
        Some("test.imports/general")
    );
}

/// REQ: WIT:ilium:host/settings.open
#[test]
fn settings_open_own_row_opens_settings_at_that_row() {
    let mut fixture = client_fixture();
    expect_ok(
        fixture
            .settings()
            .open("test.imports/general/count".to_owned()),
    );
    assert_eq!(
        fixture.opened_settings_target().as_deref(),
        Some("test.imports/general/count")
    );
}

/// REQ: WIT:ilium:host/settings.open
#[test]
fn settings_open_target_of_another_plugin_invalid_argument() {
    let mut fixture = client_fixture();
    let code = expect_code(fixture.settings().open("test.other/main".to_owned()));
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert!(fixture.opened_settings_target().is_none());
}

/// REQ: WIT:ilium:host/settings.open
#[test]
fn settings_open_unknown_target_of_the_own_plugin_not_found() {
    let mut fixture = client_fixture();
    let code = expect_code(fixture.settings().open("test.imports/ghost".to_owned()));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.open
#[test]
fn settings_open_empty_target_invalid_argument() {
    let mut fixture = client_fixture();
    let code = expect_code(fixture.settings().open(String::new()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

// ---- set-choices -----------------------------------------------------------------------------

fn options(count: usize) -> Vec<ChoiceOption> {
    (0..count)
        .map(|index| ChoiceOption {
            value: format!("v{index}"),
            title: format!("Option {index}"),
        })
        .collect()
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_replaces_the_options_of_a_dynamic_row() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_choices("dyn".to_owned(), options(3)));
    assert_eq!(fixture.choice_values("dyn"), vec!["v0", "v1", "v2"]);
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_one_option_accepted() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(fixture.settings().set_choices("dyn".to_owned(), options(1)));
    assert_eq!(fixture.choice_values("dyn"), vec!["v0"]);
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_64_options_accepted() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(
        fixture
            .settings()
            .set_choices("dyn".to_owned(), options(64)),
    );
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_65_options_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    let code = expect_code(
        fixture
            .settings()
            .set_choices("dyn".to_owned(), options(65)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert_eq!(fixture.choice_values("dyn"), vec!["a", "b"]);
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_zero_options_invalid_argument() {
    let mut fixture = settings_fixture(Tier::T2);
    let code = expect_code(fixture.settings().set_choices("dyn".to_owned(), Vec::new()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_unknown_key_not_found() {
    let mut fixture = settings_fixture(Tier::T2);
    let code = expect_code(
        fixture
            .settings()
            .set_choices("ghost".to_owned(), options(2)),
    );
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_stored_value_no_longer_offered_stays_stored() {
    let mut fixture = settings_fixture(Tier::T2);
    expect_ok(set(&mut fixture, r#"{"dyn": "b"}"#));
    expect_ok(fixture.settings().set_choices("dyn".to_owned(), options(2)));
    assert_eq!(get(&mut fixture)["dyn"], json!("b"));
    assert!(fixture.choice_shows_unavailable("dyn", "b"));
}

/// REQ: WIT:ilium:host/settings.set-choices
#[test]
fn settings_set_choices_counts_in_the_row_update_rate_limit() {
    let mut fixture = settings_fixture(Tier::T2);
    for _ in 0..10 {
        expect_ok(fixture.settings().set_choices("dyn".to_owned(), options(2)));
    }
    let code = expect_code(fixture.settings().set_choices("dyn".to_owned(), options(2)));
    assert_eq!(code, ErrorCode::RateLimited);
}

// ---- set-choice-items ------------------------------------------------------------------------

fn catalogue_fixture() -> ImportsFixture {
    let row = json!({
        "type": "choice", "key": "palette", "title": "Palette",
        "source": { "catalog": "ilium.palettes" }
    });
    ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&manifest_with_rows(vec![row, toggle("plain", true)]).to_string())
        .build()
}

fn items(count: usize) -> Vec<CatalogItem> {
    (0..count)
        .map(|index| CatalogItem {
            id: format!("item{index}"),
            title: format!("Item {index}"),
            description: None,
            preview: None,
            group: None,
            keywords: Vec::new(),
            disabled_reason: None,
        })
        .collect()
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_replaces_the_items_of_a_catalogue_row() {
    let mut fixture = catalogue_fixture();
    expect_ok(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), items(3)),
    );
    assert_eq!(
        fixture.choice_item_ids("palette"),
        vec!["item0", "item1", "item2"]
    );
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_64_items_accepted() {
    let mut fixture = catalogue_fixture();
    expect_ok(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), items(64)),
    );
    let expected: Vec<String> = (0..64).map(|index| format!("item{index}")).collect();
    assert_eq!(fixture.choice_item_ids("palette"), expected);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_65_items_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), items(65)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_unknown_key_not_found() {
    let mut fixture = catalogue_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("ghost".to_owned(), items(1)),
    );
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_on_a_row_without_a_catalogue_source_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("plain".to_owned(), items(1)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_description_121_characters_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].description = Some(ascii(121));
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_description_120_characters_accepted() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].description = Some(ascii(120));
    expect_ok(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_disabled_reason_41_characters_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].disabled_reason = Some(ascii(41));
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_group_33_characters_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].group = Some(ascii(33));
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_nine_keywords_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].keywords = (0..9).map(|index| format!("k{index}")).collect();
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_eight_keywords_accepted() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].keywords = (0..8).map(|index| format!("k{index}")).collect();
    expect_ok(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
    assert_eq!(fixture.choice_item_ids("palette"), vec!["item0"]);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_swatch_of_17_colours_invalid_argument() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].preview = Some(Preview::Swatch(vec![0xff0000; 17]));
    let code = expect_code(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-choice-items
#[test]
fn settings_set_choice_items_swatch_of_16_colours_accepted() {
    let mut fixture = catalogue_fixture();
    let mut list = items(1);
    list[0].preview = Some(Preview::Swatch(vec![0xff0000; 16]));
    expect_ok(
        fixture
            .settings()
            .set_choice_items("palette".to_owned(), list),
    );
}
