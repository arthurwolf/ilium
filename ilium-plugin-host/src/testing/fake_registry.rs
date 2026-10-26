//! In-memory registry documents for tests. The deterministic metadata key is public test
//! material, never a publisher trust root. Packages are not silently countersigned.

use std::collections::BTreeMap;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};
use ilium_plugin_api::{Code, Diagnostic};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct FakeRegistry {
    base_url: String,
    versions: BTreeMap<(String, String), Value>,
    packages: BTreeMap<String, Vec<u8>>,
    summaries: BTreeMap<String, Value>,
}
impl FakeRegistry {
    pub fn new(base_url: &str) -> Self {
        Self { base_url: base_url.trim_end_matches('/').into(), versions: BTreeMap::new(), packages: BTreeMap::new(), summaries: BTreeMap::new() }
    }
    pub fn publish(&mut self, package: Vec<u8>, channel: &str) {
        if let Err(diagnostic) = self.try_publish(package, channel) { panic!("fake registry rejected package: {diagnostic:?}"); }
    }
    pub fn try_publish(&mut self, package: Vec<u8>, channel: &str) -> Result<(), Diagnostic> {
        let archive = crate::package::read_package(&package).map_err(|errors| errors.into_iter().next().unwrap_or_else(|| refusal("package.corrupt", "invalid package")))?;
        let manifest = archive.manifest;
        let version = semver::Version::parse(&manifest.version).map_err(|_| refusal("registry.version-invalid", "invalid release version"))?;
        if !version.build.is_empty() || !matches!(channel, "stable" | "beta") || (channel == "stable" && !version.pre.is_empty()) {
            return Err(refusal("registry.version-invalid", "registry releases require stable/beta channels, no build metadata, and prereleases only on beta"));
        }
        let identity = (manifest.id.clone(), manifest.version.clone());
        if self.versions.contains_key(&identity) { return Err(refusal("registry.version-exists", "published versions are immutable and belong to one channel")); }
        let digest = hash(&package);
        let publisher_id = archive.signature.as_ref().map(|s| s.publisher_id.as_str()).unwrap_or("");
        let row = json!({"version":manifest.version,"channel":channel,"api":manifest.api,
            "url":format!("blobs/{digest}.iliumpkg"),"sha256":digest,"length":package.len(),
            "key_id":archive.signature.as_ref().map(|s| s.key_id.as_str()).unwrap_or(""),
            "countersigned":false,"host":manifest.requires.as_ref().and_then(|r| r.host.as_deref()).unwrap_or("*"),
            "platforms":["linux","macos","windows"],"categories":manifest.categories,
            "license":manifest.license,"published":"2030-01-01 12:00:00","yanked":null,"deprecated":null});
        self.summaries.insert(manifest.id.clone(), json!({"id":manifest.id,"name":manifest.name,
            "description":manifest.description,"publisher_id":publisher_id,"publisher":manifest.publisher,
            "categories":manifest.categories,"keywords":manifest.keywords,"latest_stable":null,"latest_beta":null,
            "license":manifest.license,"icon":manifest.icon,"updated":"2030-01-01 12:00:00","deprecated":null}));
        self.versions.insert(identity, row);
        self.packages.insert(format!("blobs/{digest}.iliumpkg"), package);
        Ok(())
    }
    /// Signed metadata and byte-exact blobs. Expiry is fixed for deterministic fixtures.
    /// Tests must explicitly trust `root.json`; this builder does not alter host trust.
    pub fn documents(&self) -> Vec<(String, Vec<u8>)> {
        let mut documents = self.packages.clone();
        let revision = self.versions.len() as u64 + 1;
        let mut summaries = Vec::new();
        for (id, summary) in &self.summaries {
            let versions: Vec<_> = self.versions.iter().filter(|((owner,_),_)| owner == id).map(|(_,row)| row.clone()).collect();
            let mut summary = summary.clone();
            for channel in ["stable", "beta"] {
                let latest = versions.iter().filter(|row| row["channel"] == channel).filter_map(|row| row["version"].as_str()).filter_map(|v| semver::Version::parse(v).ok()).max();
                summary[format!("latest_{channel}")] = json!(latest.map(|v| v.to_string()));
            }
            summaries.push(summary.clone());
            documents.insert(format!("plugins/{id}.json"), envelope(json!({"type":"plugin","version":revision,"id":id,"publisher_id":summary.get("publisher_id").unwrap_or(&Value::Null),"versions":versions})));
        }
        documents.insert("index.json".into(), envelope(json!({"type":"index","version":revision,"plugins":summaries})));
        documents.insert("revocations.json".into(), envelope(json!({"type":"revocations","version":revision,"expires":"2099-01-01 00:00:00","keys":[],"blocked_ids":[]})));
        let files: BTreeMap<_,_> = documents.iter().filter(|(path,_)| path.ends_with(".json")).map(|(path,bytes)| (path.clone(), json!({"version":revision,"sha256":hash(bytes),"length":bytes.len()}))).collect();
        let snapshot = envelope(json!({"type":"snapshot","version":revision,"expires":"2099-01-01 00:00:00","files":files}));
        let timestamp = envelope(json!({"type":"timestamp","version":revision,"expires":"2099-01-01 00:00:00","snapshot":{"version":revision,"sha256":hash(&snapshot),"length":snapshot.len()}}));
        documents.insert("snapshot.json".into(), snapshot);
        documents.insert("timestamp.json".into(), timestamp);
        let key = SigningKey::from_bytes(&[0x52;32]);
        let key_id = hash(key.verifying_key().as_bytes());
        let role = json!({"key_ids":[key_id],"threshold":1});
        let keys = BTreeMap::from([(key_id, json!({"algorithm":"ed25519","public_key":URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes())}))]);
        documents.insert("root.json".into(), envelope(json!({"type":"root","spec":"ilium-registry/1","version":1,"expires":"2099-01-01 00:00:00","keys":keys,"roles":{"root":role,"timestamp":role,"snapshot":role,"targets":role}})));
        documents.into_iter().map(|(path,bytes)| (format!("{}/{path}",self.base_url), bytes)).collect()
    }
}
fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn refusal(code: &'static str, message: &str) -> Diagnostic { Diagnostic::error(Code::from_static(code), "", message) }
fn envelope(signed: Value) -> Vec<u8> {
    let key = SigningKey::from_bytes(&[0x52;32]);
    // JSON values produced above contain no unsupported numbers or map keys.
    let canonical = serde_json_canonicalizer::to_vec(&signed).expect("test metadata is canonicalizable");
    let mut message = b"ilium-registry-metadata-v1\0".to_vec(); message.extend(canonical);
    let signature = URL_SAFE_NO_PAD.encode(key.sign(&message).to_bytes());
    serde_json::to_vec(&json!({"signed":signed,"signatures":[{"key_id":hash(key.verifying_key().as_bytes()),"signature":signature}]})).expect("test metadata is serializable")
}
