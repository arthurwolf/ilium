//! Persistence boundaries independent of installer/CLI fixtures.
use ilium_plugin_api::ids::parse_plugin_id;
use ilium_plugin_host::registry::{
    downloads_dir, registry_cache_dir, EnableFlags, InstalledRecord, InstalledRegistry,
};
use serde_json::json;

fn row() -> serde_json::Value {
    json!({"id":"example.notes","active_version":"1.0.0","previous_version":null,
        "state":"installed-enabled","source":"file","source_url":null,"channel":"file",
        "publisher_id":null,"key_id":null,"package_sha256":"a".repeat(64),"manifest_sha256":"b".repeat(64),
        "pinned":false,"skip":["2.0.0"],"declined":[],"failure_stopped":[],
        "installed_at":"2030-01-01T12:00:00Z","update":null})
}
#[test]
fn complete_records_survive_projected_pin_updates_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("installed.json");
    let mut registry = InstalledRegistry::open(&path).unwrap();
    registry.put_complete_record(row()).unwrap();
    registry
        .put(InstalledRecord {
            id: "example.notes".into(),
            version: "1.0.0".into(),
            source: "file".into(),
            channel: "file".into(),
            pinned: true,
        })
        .unwrap();
    assert!(InstalledRegistry::open(&path).unwrap().list().unwrap()[0].pinned);
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["plugins"][0]["skip"], json!(["2.0.0"]));
    assert_eq!(document["plugins"][0]["manifest_sha256"], "b".repeat(64));
    assert!(std::fs::read(&path).unwrap().ends_with(b"\n"));
}
#[test]
fn incomplete_insert_cannot_fabricate_admission_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("installed.json");
    let mut registry = InstalledRegistry::open(&path).unwrap();
    assert!(registry
        .put(InstalledRecord {
            id: "example.notes".into(),
            version: "1.0.0".into(),
            source: "file".into(),
            channel: "file".into(),
            pinned: false
        })
        .is_err());
    assert!(!path.exists());
}
#[test]
fn malformed_installed_state_is_never_rewritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("installed.json");
    let bytes = br#"{"format":"ilium-installed/1","plugins":[{}],"purge_tombstones":[]}"#;
    std::fs::write(&path, bytes).unwrap();
    assert!(InstalledRegistry::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[test]
fn user_flags_preserve_unrelated_config_and_use_literal_plugin_ids() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "theme = 'dark'\n").unwrap();
    let plugin = parse_plugin_id("example.notes").unwrap();
    let mut flags = EnableFlags::open(&path).unwrap();
    assert!(flags.is_enabled(&plugin));
    flags.set_enabled(&plugin, false).unwrap();
    assert!(!EnableFlags::open(&path).unwrap().is_enabled(&plugin));
    let document: toml::Value = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(document["theme"].as_str(), Some("dark"));
    assert_eq!(
        document["plugins"]["example.notes"]["enabled"].as_bool(),
        Some(false)
    );
}
#[test]
fn cache_url_identity_includes_the_entire_url() {
    let cache = std::path::Path::new("/cache");
    assert_eq!(downloads_dir(cache), cache.join("ilium/plugins/downloads"));
    assert_ne!(
        registry_cache_dir(cache, "https://example.org/a"),
        registry_cache_dir(cache, "https://example.org/b")
    );
}

#[cfg(feature = "testing")]
#[test]
fn fake_registry_metadata_signatures_cover_canonical_signed_bytes() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let registry =
        ilium_plugin_host::testing::fake_registry::FakeRegistry::new("https://registry.example/");
    let documents = registry.documents();
    let root: serde_json::Value = serde_json::from_slice(
        &documents
            .iter()
            .find(|(url, _)| url.ends_with("/root.json"))
            .unwrap()
            .1,
    )
    .unwrap();
    let key = root["signed"]["keys"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let public_bytes: [u8; 32] = URL_SAFE_NO_PAD
        .decode(key["public_key"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let public = VerifyingKey::from_bytes(&public_bytes).unwrap();
    for (_, bytes) in documents {
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut message = b"ilium-registry-metadata-v1\0".to_vec();
        message.extend(serde_json_canonicalizer::to_vec(&envelope["signed"]).unwrap());
        let signature = Signature::from_slice(
            &URL_SAFE_NO_PAD
                .decode(envelope["signatures"][0]["signature"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        public.verify(&message, &signature).unwrap();
        message.push(0);
        assert!(public.verify(&message, &signature).is_err());
    }
}
