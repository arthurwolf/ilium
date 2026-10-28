//! In-memory registry documents for tests. The deterministic metadata key is public test
//! material, never a publisher trust root. Packages are not silently countersigned.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use ilium_plugin_api::{Code, Diagnostic};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct FakeRegistry {
    base_url: String,
    versions: BTreeMap<(String, String), Value>,
    packages: BTreeMap<String, Vec<u8>>,
    summaries: BTreeMap<String, Value>,
    /// Review records by (plugin id, version): the SHA-256 of the package that was reviewed (PKG-680 a).
    reviews: BTreeMap<(String, String), String>,
    requires_level_b: bool,
}
impl FakeRegistry {
    pub fn new(base_url: &str) -> Self {
        Self { base_url: base_url.trim_end_matches('/').into(), versions: BTreeMap::new(), packages: BTreeMap::new(), summaries: BTreeMap::new(), reviews: BTreeMap::new(), requires_level_b: false }
    }
    /// Makes this registry require level B inputs at publish time (PKG-681).
    pub fn require_level_b(&mut self) { self.requires_level_b = true; }
    /// Records a review for the exact package stored under (`plugin_id`, `version`) (PKG-680 a).
    pub fn record_review(&mut self, plugin_id: &str, version: &str) -> Result<(), Diagnostic> {
        let identity = (plugin_id.to_owned(), version.to_owned());
        let sha = self.versions.get(&identity).and_then(|row| row["sha256"].as_str()).map(str::to_owned)
            .ok_or_else(|| refusal("registry.not-found", "no such version to review"))?;
        self.reviews.insert(identity, sha);
        Ok(())
    }
    /// Withdraws the review record of a version (PKG-683).
    pub fn withdraw_review(&mut self, plugin_id: &str, version: &str) {
        self.reviews.remove(&(plugin_id.to_owned(), version.to_owned()));
    }
    /// Replaces the stored bytes of a version under the same number, as a re-upload with another hash (PKG-683).
    pub fn reupload(&mut self, plugin_id: &str, version: &str, package: Vec<u8>) {
        let identity = (plugin_id.to_owned(), version.to_owned());
        if let Some(row) = self.versions.get_mut(&identity) {
            let digest = hash(&package);
            row["url"] = json!(format!("blobs/{digest}.iliumpkg"));
            row["sha256"] = json!(digest);
            row["length"] = json!(package.len());
            let inputs = level_inputs(&package);
            row["sbom"] = json!(inputs.0);
            row["license_notice"] = json!(inputs.1);
            self.packages.insert(format!("blobs/{digest}.iliumpkg"), package);
        }
    }
    pub fn publish(&mut self, package: Vec<u8>, channel: &str) {
        if let Err(diagnostic) = self.try_publish(package, channel) {
            panic!("fake registry rejected package: {diagnostic:?}");
        }
    }
    pub fn try_publish(&mut self, package: Vec<u8>, channel: &str) -> Result<(), Diagnostic> {
        let archive = crate::package::read_package(&package).map_err(|errors| {
            errors
                .into_iter()
                .next()
                .unwrap_or_else(|| refusal("package.corrupt", "invalid package"))
        })?;
        let manifest = archive.manifest;
        let version = semver::Version::parse(&manifest.version)
            .map_err(|_| refusal("registry.version-invalid", "invalid release version"))?;
        if !version.build.is_empty()
            || !matches!(channel, "stable" | "beta")
            || (channel == "stable" && !version.pre.is_empty())
        {
            return Err(refusal("registry.version-invalid", "registry releases require stable/beta channels, no build metadata, and prereleases only on beta"));
        }
        let identity = (manifest.id.clone(), manifest.version.clone());
        let has_sbom = archive.entries.iter().any(|entry| entry.path == "sbom.json" && entry.bytes.len() <= 4 * 1024 * 1024);
        let has_notice = archive.entries.iter().any(|entry| entry.path == "LICENSE" || entry.path == "NOTICE");
        let license_valid = manifest.license.as_deref().is_some_and(spdx_expression_is_valid);
        if self.requires_level_b {
            let detail = if !has_sbom { Some("sbom-missing") } else if !license_valid { Some("license-invalid") } else if !has_notice { Some("license-notice-missing") } else { None };
            if let Some(detail) = detail {
                return Err(refusal("registry.policy", &format!("level B requires sbom.json, a valid SPDX license and a LICENSE or NOTICE file: {detail}")));
            }
        }
        if self.versions.contains_key(&identity) { return Err(refusal("registry.version-exists", "published versions are immutable and belong to one channel")); }
        let digest = hash(&package);
        let publisher_id = archive
            .signature
            .as_ref()
            .map(|s| s.publisher_id.as_str())
            .unwrap_or("");
        let row = json!({"version":manifest.version,"channel":channel,"api":manifest.api,
            "url":format!("blobs/{digest}.iliumpkg"),"sha256":digest,"length":package.len(),
            "key_id":archive.signature.as_ref().map(|s| s.key_id.as_str()).unwrap_or(""),
            "countersigned":false,"host":manifest.requires.as_ref().and_then(|r| r.host.as_deref()).unwrap_or("*"),
            "platforms":["linux","macos","windows"],"categories":manifest.categories,
            "license":manifest.license,"published":"2030-01-01 12:00:00","yanked":null,"deprecated":null,
            "review_level":"A","sbom":has_sbom,"license_notice":has_notice,"license_valid":license_valid});
        self.summaries.insert(manifest.id.clone(), json!({"id":manifest.id,"name":manifest.name,
            "description":manifest.description,"publisher_id":publisher_id,"publisher":manifest.publisher,
            "categories":manifest.categories,"keywords":manifest.keywords,"latest_stable":null,"latest_beta":null,
            "license":manifest.license,"icon":manifest.icon,"updated":"2030-01-01 12:00:00","deprecated":null}));
        self.versions.insert(identity, row);
        self.packages
            .insert(format!("blobs/{digest}.iliumpkg"), package);
        Ok(())
    }
    /// The version row with `review_level` derived from the review of that exact version and package hash (PKG-683).
    fn with_derived_level(&self, identity: &(String, String), row: &Value) -> Value {
        let reviewed = self.reviews.get(identity).is_some_and(|sha| Some(sha.as_str()) == row["sha256"].as_str());
        let complete = row["sbom"] == json!(true) && row["license_notice"] == json!(true) && row["license_valid"] == json!(true);
        let mut row = row.clone();
        row["review_level"] = json!(if reviewed && complete { "B" } else { "A" });
        row
    }
    /// Signed metadata and byte-exact blobs. Expiry is fixed for deterministic fixtures.
    /// Tests must explicitly trust `root.json`; this builder does not alter host trust.
    pub fn documents(&self) -> Vec<(String, Vec<u8>)> {
        let mut documents = self.packages.clone();
        let revision = self.versions.len() as u64 + 1;
        let mut summaries = Vec::new();
        for (id, summary) in &self.summaries {
            let versions: Vec<_> = self.versions.iter().filter(|((owner,_),_)| owner == id).map(|(identity,row)| self.with_derived_level(identity, row)).collect();
            let mut summary = summary.clone();
            for channel in ["stable", "beta"] {
                let latest = versions
                    .iter()
                    .filter(|row| row["channel"] == channel)
                    .filter_map(|row| row["version"].as_str())
                    .filter_map(|v| semver::Version::parse(v).ok())
                    .max();
                summary[format!("latest_{channel}")] = json!(latest.map(|v| v.to_string()));
            }
            summaries.push(summary.clone());
            documents.insert(format!("plugins/{id}.json"), envelope(json!({"type":"plugin","version":revision,"id":id,"publisher_id":summary.get("publisher_id").unwrap_or(&Value::Null),"versions":versions})));
        }
        documents.insert(
            "index.json".into(),
            envelope(json!({"type":"index","version":revision,"plugins":summaries})),
        );
        documents.insert("revocations.json".into(), envelope(json!({"type":"revocations","version":revision,"expires":"2099-01-01 00:00:00","keys":[],"blocked_ids":[]})));
        let files: BTreeMap<_, _> = documents
            .iter()
            .filter(|(path, _)| path.ends_with(".json"))
            .map(|(path, bytes)| {
                (
                    path.clone(),
                    json!({"version":revision,"sha256":hash(bytes),"length":bytes.len()}),
                )
            })
            .collect();
        let snapshot = envelope(
            json!({"type":"snapshot","version":revision,"expires":"2099-01-01 00:00:00","files":files}),
        );
        let timestamp = envelope(
            json!({"type":"timestamp","version":revision,"expires":"2099-01-01 00:00:00","snapshot":{"version":revision,"sha256":hash(&snapshot),"length":snapshot.len()}}),
        );
        documents.insert("snapshot.json".into(), snapshot);
        documents.insert("timestamp.json".into(), timestamp);
        let key = SigningKey::from_bytes(&[0x52; 32]);
        let key_id = hash(key.verifying_key().as_bytes());
        let role = json!({"key_ids":[key_id],"threshold":1});
        let keys = BTreeMap::from([(
            key_id,
            json!({"algorithm":"ed25519","public_key":URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes())}),
        )]);
        documents.insert("root.json".into(), envelope(json!({"type":"root","spec":"ilium-registry/1","version":1,"expires":"2099-01-01 00:00:00","keys":keys,"roles":{"root":role,"timestamp":role,"snapshot":role,"targets":role}})));
        documents
            .into_iter()
            .map(|(path, bytes)| (format!("{}/{path}", self.base_url), bytes))
            .collect()
    }
}
/// (has sbom.json within 4 MiB, has LICENSE or NOTICE) of a package; unreadable packages have neither.
fn level_inputs(package: &[u8]) -> (bool, bool) {
    match crate::package::read_package(package) {
        Ok(archive) => (
            archive.entries.iter().any(|entry| entry.path == "sbom.json" && entry.bytes.len() <= 4 * 1024 * 1024),
            archive.entries.iter().any(|entry| entry.path == "LICENSE" || entry.path == "NOTICE"),
        ),
        Err(_) => (false, false),
    }
}
/// A structural SPDX expression check: ids joined by AND, OR or WITH, with optional parentheses.
fn spdx_expression_is_valid(text: &str) -> bool {
    let spaced = text.replace('(', " ( ").replace(')', " ) ");
    let mut tokens = spaced.split_whitespace().peekable();
    if tokens.peek().is_none() { return false; }
    let mut expect_id = true;
    let mut depth = 0i32;
    for token in tokens {
        match token {
            "(" if expect_id => depth += 1,
            ")" if !expect_id => { depth -= 1; if depth < 0 { return false; } }
            "AND" | "OR" | "WITH" if !expect_id => expect_id = true,
            id if expect_id && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) && id != "(" && id != ")" => expect_id = false,
            _ => return false,
        }
    }
    !expect_id && depth == 0
}
fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn refusal(code: &'static str, message: &str) -> Diagnostic { Diagnostic::error(Code::from_static(code), "", message) }
fn envelope(signed: Value) -> Vec<u8> {
    let key = SigningKey::from_bytes(&[0x52; 32]);
    // JSON values produced above contain no unsupported numbers or map keys.
    let canonical =
        serde_json_canonicalizer::to_vec(&signed).expect("test metadata is canonicalizable");
    let mut message = b"ilium-registry-metadata-v1\0".to_vec();
    message.extend(canonical);
    let signature = URL_SAFE_NO_PAD.encode(key.sign(&message).to_bytes());
    serde_json::to_vec(&json!({"signed":signed,"signatures":[{"key_id":hash(key.verifying_key().as_bytes()),"signature":signature}]})).expect("test metadata is serializable")
}

#[cfg(test)]
mod tests {
    use super::FakeRegistry;
    use ilium_plugin_api::Diagnostic;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    const BASE_URL: &str = "https://registry.example.org/";
    const EXAMPLE_ID: &str = "example.registry";
    /// The 34-byte component of the minimal package vector (reference-package-format.md).
    const COMPONENT_HEX: &str =
        "0061736d0d000100001811696c69756d3a6170692d76657273696f6e000000010000";

    fn component_bytes() -> Vec<u8> {
        (0..COMPONENT_HEX.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&COMPONENT_HEX[index..index + 2], 16)
                    .expect("COMPONENT_HEX holds only hex digits")
            })
            .collect()
    }

    /// A minimal package of `EXAMPLE_ID` at `version`, written with the crate's package writer.
    fn package_bytes(version: &str) -> Vec<u8> {
        let manifest = format!(
            "{{\n  \"api\": \"ilium:plugin@0.1.0\",\n  \"id\": \"{EXAMPLE_ID}\",\n  \"name\": \"Example\",\n  \"version\": \"{version}\",\n  \"publisher\": \"Example Labs\",\n  \"server\": {{\"entry\": \"server.wasm\", \"world\": \"ilium:server/base@0.1.0\", \"exports\": []}}\n}}\n"
        );
        let files = vec![
            ("manifest.json".to_owned(), manifest.into_bytes()),
            ("server.wasm".to_owned(), component_bytes()),
        ];
        crate::package::write_package(&files)
            .unwrap_or_else(|diagnostic| panic!("test package failed: {diagnostic:?}"))
    }

    fn any_body_mentions(registry: &FakeRegistry, needle: &str) -> bool {
        registry
            .documents()
            .iter()
            .any(|(_, body)| String::from_utf8_lossy(body).contains(needle))
    }

    #[test]
    fn new_registry_serves_no_documents() {
        let registry = FakeRegistry::new(BASE_URL);
        assert!(registry.documents().is_empty());
    }

    #[test]
    fn every_served_document_url_starts_with_the_base_url() {
        let mut registry = FakeRegistry::new(BASE_URL);
        registry.publish(package_bytes("1.0.0"), "stable");
        let documents = registry.documents();
        assert!(!documents.is_empty());
        for (url, _) in &documents {
            assert!(url.starts_with(BASE_URL), "{url} is outside {BASE_URL}");
        }
    }

    #[test]
    fn publish_of_a_valid_release_serves_its_version() {
        let mut registry = FakeRegistry::new(BASE_URL);
        registry.publish(package_bytes("1.0.0"), "stable");
        assert!(any_body_mentions(&registry, "1.0.0"));
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_prerelease_is_accepted_on_beta() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let result: Result<(), Diagnostic> =
            registry.try_publish(package_bytes("1.0.0-beta.1"), "beta");
        assert!(result.is_ok(), "a prerelease belongs to beta: {result:?}");
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_release_candidate_is_accepted_on_beta() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let result = registry.try_publish(package_bytes("1.0.0-rc.1"), "beta");
        assert!(result.is_ok(), "a prerelease belongs to beta: {result:?}");
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_prerelease_is_refused_on_stable_and_leaves_no_document() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let refusal = registry
            .try_publish(package_bytes("1.0.0-beta.1"), "stable")
            .expect_err("a prerelease cannot go to stable");
        assert!(!refusal.code.as_str().is_empty());
        assert!(!any_body_mentions(&registry, "1.0.0-beta.1"));
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_release_may_be_published_to_stable_or_beta() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry
            .try_publish(package_bytes("1.0.0"), "stable")
            .is_ok());
        assert!(registry.try_publish(package_bytes("1.1.0"), "beta").is_ok());
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_dev_is_refused_as_a_registry_channel() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry.try_publish(package_bytes("1.0.0"), "dev").is_err());
    }

    /// Boundary: an empty channel name names no channel and is refused.
    #[test]
    fn empty_channel_name_is_refused() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry.try_publish(package_bytes("1.0.0"), "").is_err());
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_a_version_is_indexed_under_exactly_one_channel() {
        let mut registry = FakeRegistry::new(BASE_URL);
        registry.publish(package_bytes("1.0.0"), "stable");
        registry.publish(package_bytes("1.1.0-beta.1"), "beta");
        let text: String = registry
            .documents()
            .iter()
            .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
            .collect();
        let stable_mentions = text.matches("\"channel\": \"stable\"").count()
            + text.matches("\"channel\":\"stable\"").count();
        assert_eq!(stable_mentions, 1, "{text}");
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_publish_panics_when_the_package_is_refused() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            registry.publish(package_bytes("1.0.0-beta.1"), "stable");
        }));
        assert!(outcome.is_err(), "publish must panic on a refused package");
    }

    /// REQ: PKG-313
    #[test]
    fn pkg_313_build_metadata_is_refused_with_registry_version_invalid() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let refusal = registry
            .try_publish(package_bytes("1.0.0+build.5"), "stable")
            .expect_err("a registry refuses build metadata");
        assert_eq!(refusal.code.as_str(), "registry.version-invalid");
    }

    /// REQ: PKG-313
    #[test]
    fn pkg_313_refused_build_metadata_leaves_no_document() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let _ = registry.try_publish(package_bytes("1.0.0-beta.1+build.5"), "beta");
        assert!(!any_body_mentions(&registry, "build.5"));
    }

    /// Boundary: an empty package file is refused rather than served.
    #[test]
    fn empty_package_bytes_are_refused() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry.try_publish(Vec::new(), "stable").is_err());
    }
}
