//! Real signed static-tree fixtures; no sockets or transport simulation as success.
use std::{collections::BTreeMap, sync::Mutex};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};
use ilium_plugin_host::registry_client::{Fetcher, FetchError, RegistryClient, RegistryContext};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct Files { files: BTreeMap<String, Vec<u8>>, calls: Mutex<Vec<String>> }
impl Fetcher for Files {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.calls.lock().unwrap().push(url.to_owned());
        self.files.get(url.strip_prefix("https://registry.example/").unwrap_or(url)).cloned().ok_or(FetchError::Unreachable)
    }
}
fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn document(value: Value) -> Vec<u8> {
    let key = SigningKey::from_bytes(&[31; 32]);
    let mut message = b"ilium-registry-metadata-v1\0".to_vec();
    message.extend(ilium_plugin_host::signing::jcs(&value).unwrap().bytes());
    serde_json::to_vec(&json!({"signed":value,"signatures":[{"key_id":hash(&key.verifying_key().to_bytes()),"signature":URL_SAFE_NO_PAD.encode(key.sign(&message).to_bytes())}]})).unwrap()
}
fn record(bytes: &[u8]) -> Value { json!({"version":1,"length":bytes.len(),"sha256":hash(bytes)}) }
fn fixture() -> (Files, RegistryContext) {
    let key = SigningKey::from_bytes(&[31; 32]);
    let id = hash(&key.verifying_key().to_bytes());
    let role = json!({"key_ids":[id],"threshold":1});
    let root = document(json!({"type":"root","spec":"ilium-registry/1","version":1,"expires":"2030-01-01 00:00:00", "keys":{(id.clone()):{"algorithm":"ed25519","public_key":URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())}},"roles":{"root":role,"timestamp":role,"snapshot":role,"targets":role}}));
    let blob = b"actual package bytes checked before any package parsing".to_vec();
    let index = document(json!({"type":"index","version":1,"expires":"2027-01-01 00:00:00","plugins":[{"id":"example.notes","latest_stable":"1.0.0"}]}));
    let plugin = document(json!({"type":"plugin","id":"example.notes","version":1,"expires":"2027-01-01 00:00:00","versions":[{"version":"1.0.0","url":"blobs/package.iliumpkg","sha256":hash(&blob),"length":blob.len(),"key_id":id,"yanked":null}]}));
    let revoked = document(json!({"type":"revocations","version":1,"expires":"2027-01-01 00:00:00","keys":[],"blocked_ids":[]}));
    let snapshot = document(json!({"type":"snapshot","version":1,"expires":"2027-01-01 00:00:00","files":{"index.json":record(&index),"plugins/example.notes.json":record(&plugin),"revocations.json":record(&revoked)}}));
    let timestamp = document(json!({"type":"timestamp","version":1,"expires":"2026-10-23 00:00:00","snapshot":record(&snapshot)}));
    let files = Files { files: BTreeMap::from([("timestamp.json".into(),timestamp),("snapshot.json".into(),snapshot),("index.json".into(),index),("plugins/example.notes.json".into(),plugin),("revocations.json".into(),revoked),("blobs/package.iliumpkg".into(),blob)]), ..Default::default() };
    (files, RegistryContext { trusted_root:root, now:"2026-10-22 00:00:00".into(), ..Default::default() })
}
#[test]
fn missing_root_never_fetches_or_accepts_network_metadata() {
    let files = Files::default();
    let client = RegistryClient::new(&files, "https://registry.example/".into());
    assert_eq!(client.index().unwrap_err().code().as_str(), "registry.signature-invalid");
    assert!(files.calls.lock().unwrap().is_empty());
}
#[test]
fn signed_chain_authenticates_download_and_offline_cache_reverifies_it() {
    let (files, context) = fixture();
    let client = RegistryClient::with_context(&files,"https://registry.example/".into(),context);
    let entry = client.index().unwrap().remove(0);
    assert_eq!(client.download(&entry).unwrap(),files.files["blobs/package.iliumpkg"]);
    let mut cache = client.cache().unwrap();
    cache.offline = true;
    cache.now = "2028-01-01 00:00:00".into();
    let unavailable = Files::default();
    let offline = RegistryClient::with_context(&unavailable,"https://registry.example/".into(),cache);
    assert_eq!(offline.index().unwrap(),vec![entry.clone()]);
    assert_eq!(offline.download(&entry).unwrap(),files.files["blobs/package.iliumpkg"]);
    assert!(!offline.warnings().unwrap().is_empty());
    assert!(unavailable.calls.lock().unwrap().is_empty());
}
#[test]
fn blob_digest_and_caller_url_cannot_bypass_authenticated_record() {
    let (mut files, context) = fixture();
    files.files.insert("blobs/package.iliumpkg".into(),b"tampered".to_vec());
    let client = RegistryClient::with_context(&files,"https://registry.example/".into(),context);
    let mut entry = client.index().unwrap().remove(0);
    assert_eq!(client.download(&entry).unwrap_err().code().as_str(),"registry.hash-mismatch");
    entry.url = "https://attacker.example/blob".into();
    assert_eq!(client.download(&entry).unwrap_err().code().as_str(),"registry.hash-mismatch");
}
#[test]
fn rollback_or_duplicate_metadata_cannot_publish_a_cache_checkpoint() {
    let (mut files, mut context) = fixture();
    context.highest_versions.insert("timestamp.json".into(),2);
    let client = RegistryClient::with_context(&files,"https://registry.example/".into(),context.clone());
    assert_eq!(client.index().unwrap_err().code().as_str(),"registry.rollback");
    assert!(client.cache().unwrap().documents.is_empty());
    context.highest_versions.clear();
    files.files.insert("timestamp.json".into(),br#"{"signed":{},"signed":{},"signatures":[]}"#.to_vec());
    let client = RegistryClient::with_context(&files,"https://registry.example/".into(),context);
    assert_eq!(client.index().unwrap_err().code().as_str(),"registry.signature-invalid");
    assert!(client.cache().unwrap().documents.is_empty());
}
#[test]
fn online_expired_cache_is_refused_and_insecure_transport_is_not_called() {
    let (files, context) = fixture();
    let client = RegistryClient::with_context(&files,"http://registry.example/".into(),context.clone());
    assert_eq!(client.index().unwrap_err().code().as_str(),"registry.insecure-url");
    assert!(files.calls.lock().unwrap().is_empty());
    let mut expired = context;
    expired.now = "2028-01-01 00:00:00".into();
    let client = RegistryClient::with_context(&files,"https://registry.example/".into(),expired);
    assert_eq!(client.index().unwrap_err().code().as_str(),"registry.metadata-expired");
}
#[test]
fn revocations_are_refetched_when_snapshot_is_unchanged() {
    let (files, context) = fixture();
    let client = RegistryClient::with_context(&files,"https://registry.example/".into(),context);
    client.index().unwrap();
    files.calls.lock().unwrap().clear();
    client.index().unwrap();
    let calls = files.calls.lock().unwrap();
    assert_eq!(calls.iter().filter(|url| url.ends_with("/revocations.json")).count(),1);
    assert!(!calls.iter().any(|url| url.ends_with("/snapshot.json")));
}
