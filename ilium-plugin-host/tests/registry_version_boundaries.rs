use ilium_plugin_api::ids::PluginId;
use ilium_plugin_host::registry::{InstalledRegistry, PackageStore};
use serde_json::{json, Value};
use std::path::Path;

fn row() -> Value {
    json!({"id":"example.notes", "active_version":"1.0.0", "previous_version":"0.9.0",
        "state":"installed-enabled", "source":"file", "source_url":null, "channel":"file",
        "publisher_id":null, "key_id":null, "package_sha256":"a".repeat(64), "manifest_sha256":"b".repeat(64),
        "pinned":false, "skip":["2.0.0"], "declined":["3.0.0-beta.1"], "failure_stopped":[],
        "installed_at":"2030-01-01T12:00:00Z", "update": {"candidate":"1.1.0", "state":"staged", "attempts":0, "started_at":"2030-01-01T12:00:00Z"}})
}

#[test]
fn checked_version_path_rejects_traversal_and_invalid_semver() {
    let store = PackageStore {
        root: "/isolated/plugins".into(),
    };
    let plugin = PluginId("example.notes".into());
    for version in [
        "../outside",
        "/outside",
        "1.0.0/../../outside",
        "1.0.0\\outside",
        "..",
        "",
        "v1.0.0",
        "1.0",
        "01.0.0",
        "1.0.0\0",
        "1.0.0 ",
        "2147483648.0.0",
    ] {
        assert!(
            store.checked_version_dir(&plugin, version).is_err(),
            "{version:?}"
        );
        let fallback = store.version_dir(&plugin, version);
        assert!(fallback.starts_with(store.root.join(".invalid-package-path")));
        assert!(!fallback
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)));
    }
    assert!(store
        .checked_version_dir(&plugin, &format!("1.0.0+{}", "a".repeat(59)))
        .is_err());
}

#[test]
fn raw_plugin_id_cannot_escape_or_alias_a_valid_package_directory() {
    let store = PackageStore {
        root: "/isolated/plugins".into(),
    };
    for id in ["../outside", "/outside", "a/b", "a\\b", ".", ""] {
        let plugin = PluginId(id.into());
        assert!(store.checked_version_dir(&plugin, "1.0.0").is_err());
        assert!(store
            .version_dir(&plugin, "1.0.0")
            .starts_with(store.root.join(".invalid-package-path")));
    }
    let plugin = PluginId("example.notes".into());
    for version in ["0.0.0", "1.2.3-beta.1+build.7", "2147483647.0.0"] {
        let expected = Path::new("/isolated/plugins/example.notes").join(version);
        assert_eq!(
            store.checked_version_dir(&plugin, version).unwrap(),
            expected
        );
        assert_eq!(store.version_dir(&plugin, version), expected);
    }
}

#[test]
fn every_persisted_version_and_plugin_identity_is_validated_without_rewriting() {
    for pointer in [
        "/active_version",
        "/previous_version",
        "/skip/0",
        "/declined/0",
        "/update/candidate",
        "/id",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("installed.json");
        let mut record = row();
        *record.pointer_mut(pointer).unwrap() = json!("../outside");
        let bytes = serde_json::to_vec(
            &json!({"format":"ilium-installed/1", "plugins":[record], "purge_tombstones":[]}),
        )
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(InstalledRegistry::open(&path).is_err(), "{pointer}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn invalid_complete_record_is_rejected_before_creating_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("installed.json");
    let mut registry = InstalledRegistry::open(&path).unwrap();
    let mut record = row();
    record["update"]["candidate"] = json!("/outside");
    assert!(registry.put_complete_record(record).is_err());
    assert!(!path.exists());
    registry.put_complete_record(row()).unwrap();
    assert_eq!(
        InstalledRegistry::open(&path).unwrap().list().unwrap()[0].version,
        "1.0.0"
    );
}
