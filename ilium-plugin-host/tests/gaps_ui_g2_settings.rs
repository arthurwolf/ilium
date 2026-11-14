//! G2: a plugin's settings live in `[plugins."<id>"]` of `config.toml`; the settings import
//! returns defaults plus stored values, stores a merge patch atomically and reports one change
//! per accepted write (api-settings.md, reference-settings-schema.md).

use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::host::settings::HostSettings;
use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_host::config_settings::PluginSettingsStore;
use ilium_plugin_host::imports::settings::{
    PeerSchemas, SettingsChangeSink, SettingsNotice, StoredSettingsImport,
};
use ilium_plugin_host::settings::{manifest_row_names, manifest_rows, schema_for_side};
use serde_json::{json, Value};

fn manifest() -> Value {
    json!({"contributes": {"settings": [{
        "kind": "tab", "id": "main", "title": "Demo",
        "sections": [{"title": "Sound", "rows": [
            {"type": "number", "key": "volume", "title": "Volume",
             "minimum": 0, "maximum": 10, "default": 5},
            {"type": "text", "key": "label", "title": "Label", "default": "ilium"},
            {"type": "info", "id": "status", "title": "Status"}
        ]}]
    }]}})
}

struct Rig {
    import: StoredSettingsImport,
    notices: Arc<Mutex<Vec<SettingsNotice>>>,
    store: Arc<PluginSettingsStore>,
    directory: tempfile::TempDir,
}

fn rig(peers: PeerSchemas, read_all: bool, write_all: bool) -> Rig {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = Arc::new(PluginSettingsStore::at(
        directory.path().join("config.toml"),
    ));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink: SettingsChangeSink = {
        let notices = Arc::clone(&notices);
        Arc::new(move |notice| notices.lock().expect("notices").push(notice))
    };
    let manifest = manifest();
    let import = StoredSettingsImport::new(
        "acme.demo",
        schema_for_side(&manifest, "client").to_string(),
        manifest_row_names(&manifest),
        Arc::clone(&store),
        sink,
    )
    .with_peers(peers, read_all, write_all);
    Rig {
        import,
        notices,
        store,
        directory,
    }
}

#[test]
fn the_manifest_compiles_to_rows_and_a_client_schema() {
    let manifest = manifest();
    let keys: Vec<String> = manifest_rows(&manifest)
        .into_iter()
        .map(|row| row.key)
        .collect();
    assert_eq!(keys, ["volume", "label"]);
    assert!(manifest_row_names(&manifest).contains("status"));
}

#[test]
fn values_reach_the_plugin_as_defaults_then_stored_and_survive_a_new_import() {
    let mut rig = rig(PeerSchemas::new(), false, false);
    let first: Value = serde_json::from_str(&rig.import.get_json()).expect("JSON");
    assert_eq!(first["volume"], 5);
    assert_eq!(first["label"], "ilium");
    rig.import
        .set_json(r#"{"volume":9}"#.to_owned())
        .expect("stored");
    let text = std::fs::read_to_string(rig.directory.path().join("config.toml")).expect("file");
    assert!(text.contains("[plugins.\"acme.demo\"]"), "{text}");
    assert_eq!(rig.store.read("acme.demo").expect("read")["volume"], 9);
}

#[test]
fn a_write_reports_exactly_one_change_and_a_repeat_reports_none() {
    let mut rig = rig(PeerSchemas::new(), false, false);
    rig.import
        .set_json(r#"{"volume":9,"label":"x"}"#.to_owned())
        .expect("stored");
    rig.import
        .set_json(r#"{"volume":9,"label":"x"}"#.to_owned())
        .expect("same values");
    let notices = rig.notices.lock().expect("notices").clone();
    assert_eq!(notices.len(), 1);
    let mut keys = notices[0].keys.clone();
    keys.sort();
    assert_eq!(keys, ["label", "volume"]);
}

#[test]
fn refused_patches_store_nothing() {
    let mut rig = rig(PeerSchemas::new(), false, false);
    for bad in [r#"{"volume":99}"#, r#"{"ghost":1}"#, "[1]", "not json"] {
        let failure = rig.import.set_json(bad.to_owned()).expect_err(bad);
        assert_eq!(failure.code, ErrorCode::InvalidArgument, "{bad}");
    }
    assert!(rig.store.read("acme.demo").expect("read").is_empty());
    assert!(rig.notices.lock().expect("notices").is_empty());
}

#[test]
fn other_plugins_need_the_all_grants() {
    let mut denied = rig(PeerSchemas::new(), false, false);
    let other = PluginId("acme.other".to_owned());
    assert_eq!(
        denied
            .import
            .get_plugin_json(other.clone())
            .expect_err("no read-all")
            .code,
        ErrorCode::NotPermitted
    );
    assert_eq!(
        denied
            .import
            .set_plugin_json(other.clone(), "{}".to_owned())
            .expect_err("no write-all")
            .code,
        ErrorCode::NotPermitted
    );

    let mut peers = PeerSchemas::new();
    peers.insert(
        "acme.other".to_owned(),
        schema_for_side(&manifest(), "client").to_string(),
    );
    let mut allowed = rig(peers, true, true);
    allowed
        .import
        .set_plugin_json(other.clone(), r#"{"volume":2}"#.to_owned())
        .expect("granted write");
    let values: Value =
        serde_json::from_str(&allowed.import.get_plugin_json(other).expect("granted read"))
            .expect("JSON");
    assert_eq!(values["volume"], 2);
    assert_eq!(
        allowed.notices.lock().expect("notices")[0].plugin,
        "acme.other"
    );
}
