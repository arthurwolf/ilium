//! Shared helpers of the registry-publish lane: a signed-metadata registry builder, a recording
//! fake network, a sandboxed CLI environment, package fixtures and record selectors.
//!
//! Everything here follows the manual, not the implementation: metadata documents are signed
//! with Ed25519 over `ilium-registry-metadata-v1`, `0x00`, JCS(signed) (reference-registry.md,
//! "Metadata documents"); packages follow the vector of reference-package-format.md.

#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey as DalekKey};
use ilium_plugin_host::cli::{self, CliEnv, CliOutput};
use ilium_plugin_host::package::write_package;
use ilium_plugin_host::registry::InstalledRegistry;
use ilium_plugin_host::registry::InstalledRecord;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------------------------

/// The fixed "now" of the tests: 2030-01-01 12:00:00 UTC.
pub const NOW_SECS: u64 = 1_893_499_200;

/// The fixed "now" as a `SystemTime`.
pub fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS)
}

/// `now` shifted by a signed number of seconds.
pub fn now_plus(seconds: i64) -> SystemTime {
    let moved = NOW_SECS as i64 + seconds;
    SystemTime::UNIX_EPOCH + Duration::from_secs(moved as u64)
}

/// Formats seconds since the epoch as `YYYY-MM-DD HH:mm:ss` (UTC).
pub fn format_utc(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let rest = epoch_seconds % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 { month_part + 3 } else { month_part - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, (rest % 3600) / 60, rest % 60)
}

/// The text of `now` shifted by seconds, in the metadata time form.
pub fn at(offset_seconds: i64) -> String {
    format_utc((NOW_SECS as i64 + offset_seconds) as u64)
}

// ---------------------------------------------------------------------------------------------
// Crypto and canonical JSON
// ---------------------------------------------------------------------------------------------

/// Lowercase hexadecimal of bytes.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Bytes of a hexadecimal string.
pub fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2).map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("hex digit")).collect()
}

/// Lowercase hexadecimal SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// RFC 8785 form of a JSON value. The documents of these tests hold only strings, integers,
/// booleans, null, arrays and objects, so the scalar forms of `serde_json` are canonical.
pub fn jcs(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut pairs: Vec<(&String, &Value)> = map.iter().collect();
            pairs.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
            let parts: Vec<String> = pairs
                .iter()
                .map(|(key, inner)| format!("{}:{}", serde_json::to_string(key).expect("key"), jcs(inner)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(jcs).collect::<Vec<_>>().join(",")),
        scalar => serde_json::to_string(scalar).expect("scalar"),
    }
}

/// An Ed25519 key whose 32 seed bytes all equal `seed`.
pub fn dalek_key(seed: u8) -> DalekKey {
    DalekKey::from_bytes(&[seed; 32])
}

/// The key id (lowercase hex SHA-256 of the 32 public bytes) of the key `seed`.
pub fn key_id_of(seed: u8) -> String {
    sha256_hex(&dalek_key(seed).verifying_key().to_bytes())
}

/// The base64url public key of the key `seed`.
pub fn public_key_b64(seed: u8) -> String {
    URL_SAFE_NO_PAD.encode(dalek_key(seed).verifying_key().to_bytes())
}

/// Signs a metadata `signed` value with the given keys and returns the document bytes.
pub fn sign_metadata(signed: &Value, seeds: &[u8]) -> Vec<u8> {
    let mut message = b"ilium-registry-metadata-v1".to_vec();
    message.push(0);
    message.extend_from_slice(jcs(signed).as_bytes());
    let signatures: Vec<Value> = seeds
        .iter()
        .map(|seed| {
            let signature = dalek_key(*seed).sign(&message).to_bytes();
            json!({ "key_id": key_id_of(*seed), "signature": URL_SAFE_NO_PAD.encode(signature) })
        })
        .collect();
    serde_json::to_vec(&json!({ "signed": signed, "signatures": signatures })).expect("document")
}

/// Parses a document and returns its `signed` value.
pub fn signed_of(bytes: &[u8]) -> Value {
    let document: Value = serde_json::from_slice(bytes).expect("document is JSON");
    document["signed"].clone()
}

/// Re-signs a document after `edit` changed its `signed` value.
pub fn edit_signed(bytes: &[u8], seeds: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut signed = signed_of(bytes);
    edit(&mut signed);
    sign_metadata(&signed, seeds)
}

// ---------------------------------------------------------------------------------------------
// Packages
// ---------------------------------------------------------------------------------------------

/// The publisher key seed of the fixture packages (publisher id `com.acme`).
pub const PUBLISHER_SEED: u8 = 50;
/// The publisher id of the fixture packages.
pub const PUBLISHER_ID: &str = "com.acme";
/// The 34-byte minimal server component of reference-package-format.md.
pub const COMPONENT_HEX: &str = "0061736d0d000100001811696c69756d3a6170692d76657273696f6e000000010000";

/// The key id of the fixture publisher.
pub fn publisher_key_id() -> String {
    key_id_of(PUBLISHER_SEED)
}

/// The packed manifest text (reference-package-format.md vector layout) for a plugin.
pub fn manifest_text(plugin_id: &str, version: &str) -> String {
    let component = unhex(COMPONENT_HEX);
    format!(
        "{{\n  \"api\": \"ilium:plugin@0.1.0\",\n  \"id\": \"{plugin_id}\",\n  \"name\": \"Fixture\",\n  \"version\": \"{version}\",\n  \"publisher\": \"Acme Labs\",\n  \"server\": {{\n    \"entry\": \"server.wasm\",\n    \"world\": \"ilium:server/base@0.1.0\",\n    \"exports\": []\n  }},\n  \"files\": [\n    {{\n      \"path\": \"server.wasm\",\n      \"length\": {},\n      \"sha256\": \"{}\"\n    }}\n  ]\n}}\n",
        component.len(),
        sha256_hex(&component)
    )
}

/// The signature entry text (reference-signing-and-trust.md PKG-650) over manifest bytes.
pub fn signature_entry(manifest: &[u8], seed: u8, publisher_id: &str) -> String {
    let mut message = b"ilium-plugin-signature-v1".to_vec();
    message.push(0);
    message.extend_from_slice(publisher_id.as_bytes());
    message.push(0);
    message.extend_from_slice(&Sha256::digest(manifest));
    let signature = URL_SAFE_NO_PAD.encode(dalek_key(seed).sign(&message).to_bytes());
    format!(
        "{{\n  \"format\": \"ilium-sig/1\",\n  \"signatures\": [\n    {{\n      \"algorithm\": \"ed25519\",\n      \"key_id\": \"{}\",\n      \"publisher_id\": \"{publisher_id}\",\n      \"signature\": \"{signature}\",\n      \"signed_at\": \"2026-10-12 10:00:00\"\n    }}\n  ]\n}}\n",
        key_id_of(seed)
    )
}

/// A package signed by `seed` as `publisher_id`.
pub fn package_signed_by(plugin_id: &str, version: &str, seed: u8, publisher_id: &str) -> Vec<u8> {
    let manifest = manifest_text(plugin_id, version).into_bytes();
    let signature = signature_entry(&manifest, seed, publisher_id).into_bytes();
    write_package(&[
        ("manifest.json".to_owned(), manifest),
        ("server.wasm".to_owned(), unhex(COMPONENT_HEX)),
        ("signature".to_owned(), signature),
    ])
    .expect("write signed package")
}

/// A package signed by the fixture publisher (`com.acme`).
pub fn signed_package(plugin_id: &str, version: &str) -> Vec<u8> {
    package_signed_by(plugin_id, version, PUBLISHER_SEED, PUBLISHER_ID)
}

/// A package without a `signature` entry.
pub fn unsigned_package(plugin_id: &str, version: &str) -> Vec<u8> {
    write_package(&[
        ("manifest.json".to_owned(), manifest_text(plugin_id, version).into_bytes()),
        ("server.wasm".to_owned(), unhex(COMPONENT_HEX)),
    ])
    .expect("write unsigned package")
}

/// Bytes that stand in for a package whose content no test reads.
pub fn dummy_package(plugin_id: &str, version: &str) -> Vec<u8> {
    format!("dummy package {plugin_id} {version}").into_bytes()
}

// ---------------------------------------------------------------------------------------------
// Registry builder
// ---------------------------------------------------------------------------------------------

/// The base URL of the standard fixture registry.
pub const BASE: &str = "https://plugins.example.org/ilium/v1/";
/// A second registry base (a mirror or another registry).
pub const BASE_TWO: &str = "https://mirror.team.example/ilium/";
/// Seed of the first root key and, by default, of every role key.
pub const KEY_A: u8 = 1;
/// Seed of the second role key (for threshold tests).
pub const KEY_B: u8 = 2;
/// Seed of a key that no root lists.
pub const KEY_FOREIGN: u8 = 99;

/// A file tree: path relative to the registry base to bytes.
pub type Tree = BTreeMap<String, Vec<u8>>;

/// One published version.
#[derive(Clone)]
pub struct VersionSpec {
    pub version: String,
    pub channel: String,
    pub yanked: Option<String>,
    pub package: Vec<u8>,
    /// Extra members merged into the version object (for example `dependencies`).
    pub extra: Value,
}

impl VersionSpec {
    pub fn stable(plugin_id: &str, version: &str) -> VersionSpec {
        VersionSpec { version: version.to_owned(), channel: "stable".to_owned(), yanked: None, package: dummy_package(plugin_id, version), extra: json!({}) }
    }

    pub fn beta(plugin_id: &str, version: &str) -> VersionSpec {
        VersionSpec { version: version.to_owned(), channel: "beta".to_owned(), yanked: None, package: dummy_package(plugin_id, version), extra: json!({}) }
    }

    /// A stable version whose package is a real signed package of the fixture publisher.
    pub fn signed(plugin_id: &str, version: &str) -> VersionSpec {
        VersionSpec { version: version.to_owned(), channel: "stable".to_owned(), yanked: None, package: signed_package(plugin_id, version), extra: json!({}) }
    }

    /// Declares a dependency (`dependencies` map of id to range in the version object).
    pub fn depends_on(mut self, id: &str, range: &str) -> VersionSpec {
        self.extra["dependencies"][id] = Value::String(range.to_owned());
        self
    }

    pub fn yanked(mut self, reason: &str) -> VersionSpec {
        self.yanked = Some(reason.to_owned());
        self
    }
}

/// One plugin of the registry.
#[derive(Clone)]
pub struct PluginSpec {
    pub id: String,
    pub publisher_id: String,
    pub publisher: String,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub updated: String,
    pub versions: Vec<VersionSpec>,
    pub unlisted: bool,
    pub deprecated: Option<(String, String)>,
    pub transferred_from: Option<String>,
    /// Extra members merged into the index entry (for example `downloads`, `rating`).
    pub extra_index: Value,
}

impl PluginSpec {
    pub fn new(id: &str) -> PluginSpec {
        let publisher_id = id.rsplit_once('.').map(|(head, _)| head.to_owned()).unwrap_or_else(|| id.to_owned());
        PluginSpec {
            id: id.to_owned(),
            publisher_id,
            publisher: "Anon".to_owned(),
            name: id.to_owned(),
            description: String::new(),
            keywords: Vec::new(),
            categories: vec!["tools".to_owned()],
            updated: "2029-01-01 00:00:00".to_owned(),
            versions: vec![VersionSpec::stable(id, "1.0.0")],
            unlisted: false,
            deprecated: None,
            transferred_from: None,
            extra_index: json!({}),
        }
    }

    pub fn named(mut self, name: &str) -> PluginSpec {
        self.name = name.to_owned();
        self
    }

    pub fn described(mut self, description: &str) -> PluginSpec {
        self.description = description.to_owned();
        self
    }

    pub fn keyworded(mut self, keywords: &[&str]) -> PluginSpec {
        self.keywords = keywords.iter().map(|word| (*word).to_owned()).collect();
        self
    }

    pub fn by(mut self, publisher: &str) -> PluginSpec {
        self.publisher = publisher.to_owned();
        self
    }

    pub fn updated_at(mut self, updated: &str) -> PluginSpec {
        self.updated = updated.to_owned();
        self
    }

    pub fn in_categories(mut self, categories: &[&str]) -> PluginSpec {
        self.categories = categories.iter().map(|word| (*word).to_owned()).collect();
        self
    }

    pub fn with_versions(mut self, versions: Vec<VersionSpec>) -> PluginSpec {
        self.versions = versions;
        self
    }
}

/// Builds a complete, correctly signed registry file tree, with knobs to break one thing.
#[derive(Clone)]
pub struct RegistryBuilder {
    /// Seeds of the keys listed in every role of `root.json`.
    pub role_keys: Vec<u8>,
    pub threshold: usize,
    /// How many of `role_keys` sign each document (a prefix).
    pub signer_count: usize,
    /// Per document signer seeds that replace the default ("root", "timestamp", "snapshot",
    /// "index", "revocations", "plugin:<id>").
    pub overrides: BTreeMap<String, Vec<u8>>,
    pub root_version: u64,
    pub timestamp_version: u64,
    pub snapshot_version: u64,
    pub index_version: u64,
    pub revocations_version: u64,
    pub plugin_doc_version: u64,
    pub root_expires: String,
    pub timestamp_expires: String,
    pub snapshot_expires: String,
    pub index_expires: String,
    pub revocations_expires: String,
    pub plugin_expires: String,
    pub plugins: Vec<PluginSpec>,
    pub revoked_keys: Vec<Value>,
    pub blocked_ids: Vec<Value>,
    /// Exact total byte length wanted for a document ("index", "root", "plugin:<id>", ...).
    pub target_len: BTreeMap<String, usize>,
}

impl RegistryBuilder {
    /// An empty registry with valid windows (timestamp 1 day, snapshot 7 days, root 1 year).
    pub fn empty() -> RegistryBuilder {
        RegistryBuilder {
            role_keys: vec![KEY_A],
            threshold: 1,
            signer_count: 1,
            overrides: BTreeMap::new(),
            root_version: 3,
            timestamp_version: 4821,
            snapshot_version: 4821,
            index_version: 4821,
            revocations_version: 17,
            plugin_doc_version: 9,
            root_expires: at(364 * 86_400),
            timestamp_expires: at(86_400),
            snapshot_expires: at(7 * 86_400),
            index_expires: at(7 * 86_400),
            revocations_expires: at(7 * 86_400),
            plugin_expires: at(7 * 86_400),
            plugins: Vec::new(),
            revoked_keys: Vec::new(),
            blocked_ids: Vec::new(),
            target_len: BTreeMap::new(),
        }
    }

    /// The standard registry: three plugins, `com.acme.notes` with real signed packages.
    pub fn standard() -> RegistryBuilder {
        let mut builder = RegistryBuilder::empty();
        builder.plugins = vec![
            PluginSpec::new("com.acme.notes")
                .named("Notes")
                .described("Quick notes in a side panel")
                .keyworded(&["markdown", "memo"])
                .by("Acme Labs")
                .in_categories(&["editors"])
                .updated_at("2030-01-01 10:00:00")
                .with_versions(vec![
                    VersionSpec::signed("com.acme.notes", "1.0.0"),
                    VersionSpec::signed("com.acme.notes", "1.1.0"),
                ]),
            PluginSpec::new("com.acme.git-tools")
                .named("Git Tools")
                .described("Branch helper")
                .keyworded(&["git", "branch"])
                .by("Acme Labs")
                .in_categories(&["git"])
                .updated_at("2029-12-01 10:00:00")
                .with_versions(vec![VersionSpec::signed("com.acme.git-tools", "2.0.0")]),
            PluginSpec::new("org.zeta.cafe-timer")
                .named("Caf\u{e9} Timer")
                .described("Pomodoro timer for focus")
                .keyworded(&["pomodoro"])
                .by("Zeta")
                .in_categories(&["tools"])
                .updated_at("2029-11-01 10:00:00")
                .with_versions(vec![VersionSpec::stable("org.zeta.cafe-timer", "0.3.0")]),
        ];
        builder
    }

    /// A registry holding exactly these plugins.
    pub fn with_plugins(plugins: Vec<PluginSpec>) -> RegistryBuilder {
        let mut builder = RegistryBuilder::empty();
        builder.plugins = plugins;
        builder
    }

    /// The key id pinned by `registry add` for this registry.
    pub fn pinned_key_id(&self) -> String {
        key_id_of(self.role_keys[0])
    }

    fn signers(&self, name: &str) -> Vec<u8> {
        self.overrides.get(name).cloned().unwrap_or_else(|| self.role_keys[..self.signer_count.min(self.role_keys.len())].to_vec())
    }

    /// Signs a document, padding it with an ignored `pad` member to the wanted total length.
    fn sign_doc(&self, name: &str, signed: &Value) -> Vec<u8> {
        let seeds = self.signers(name);
        let plain = sign_metadata(signed, &seeds);
        let Some(target) = self.target_len.get(name) else { return plain };
        let overhead = 9; // the bytes of `,"pad":""`
        assert!(*target >= plain.len() + overhead, "target length {target} is below the document size {}", plain.len() + overhead);
        let mut padded = signed.clone();
        padded["pad"] = Value::String("x".repeat(target - plain.len() - overhead));
        let bytes = sign_metadata(&padded, &seeds);
        assert_eq!(bytes.len(), *target, "padded document length");
        bytes
    }

    fn version_key(version: &str) -> (u64, u64, u64) {
        let core = version.split(['-', '+']).next().unwrap_or(version);
        let mut parts = core.split('.').map(|part| part.parse::<u64>().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    }

    fn latest(plugin: &PluginSpec, channel: &str) -> Value {
        plugin
            .versions
            .iter()
            .filter(|version| version.channel == channel && version.yanked.is_none())
            .max_by_key(|version| Self::version_key(&version.version))
            .map(|version| Value::String(version.version.clone()))
            .unwrap_or(Value::Null)
    }

    /// Builds the whole tree.
    pub fn build(&self) -> Tree {
        let mut tree = Tree::new();
        let mut file_entries = serde_json::Map::new();

        for plugin in &self.plugins {
            let mut versions = Vec::new();
            for version in &plugin.versions {
                let digest = sha256_hex(&version.package);
                tree.insert(format!("blobs/{digest}.iliumpkg"), version.package.clone());
                let mut entry = json!({
                    "version": version.version,
                    "channel": version.channel,
                    "api": "ilium:plugin@0.1.0",
                    "url": format!("blobs/{digest}.iliumpkg"),
                    "sha256": digest,
                    "length": version.package.len(),
                    "key_id": publisher_key_id(),
                    "countersigned": true,
                    "host": ">=0.1.0",
                    "platforms": ["linux", "windows", "macos"],
                    "categories": plugin.categories,
                    "license": "MIT",
                    "published": "2030-01-01 10:00:00",
                    "yanked": version.yanked.as_ref().map(|reason| json!({ "reason": reason, "at": "2030-01-01 11:00:00" })).unwrap_or(Value::Null),
                    "deprecated": Value::Null,
                });
                if let (Some(target), Some(extra)) = (entry.as_object_mut(), version.extra.as_object()) {
                    for (key, value) in extra {
                        target.insert(key.clone(), value.clone());
                    }
                }
                versions.push(entry);
            }
            let mut signed = json!({
                "type": "plugin",
                "version": self.plugin_doc_version,
                "expires": self.plugin_expires,
                "id": plugin.id,
                "publisher_id": plugin.publisher_id,
                "versions": versions,
            });
            if let Some(previous) = &plugin.transferred_from {
                signed["transferred_from"] = Value::String(previous.clone());
            }
            let path = format!("plugins/{}.json", plugin.id);
            let bytes = self.sign_doc(&format!("plugin:{}", plugin.id), &signed);
            file_entries.insert(
                path.clone(),
                json!({ "version": self.plugin_doc_version, "sha256": sha256_hex(&bytes), "length": bytes.len() }),
            );
            tree.insert(path, bytes);
        }

        let entries: Vec<Value> = self
            .plugins
            .iter()
            .filter(|plugin| !plugin.unlisted)
            .map(|plugin| {
                let mut entry = json!({
                    "id": plugin.id,
                    "name": plugin.name,
                    "description": plugin.description,
                    "publisher_id": plugin.publisher_id,
                    "publisher": plugin.publisher,
                    "categories": plugin.categories,
                    "keywords": plugin.keywords,
                    "latest_stable": Self::latest(plugin, "stable"),
                    "latest_beta": Self::latest(plugin, "beta"),
                    "license": "MIT",
                    "icon": "puzzle",
                    "updated": plugin.updated,
                    "deprecated": plugin.deprecated.as_ref().map(|(message, successor)| json!({ "message": message, "successor": successor })).unwrap_or(Value::Null),
                });
                if let (Some(target), Some(extra)) = (entry.as_object_mut(), plugin.extra_index.as_object()) {
                    for (key, value) in extra {
                        target.insert(key.clone(), value.clone());
                    }
                }
                entry
            })
            .collect();
        let index_signed = json!({ "type": "index", "version": self.index_version, "expires": self.index_expires, "plugins": entries });
        let index_bytes = self.sign_doc("index", &index_signed);
        file_entries.insert(
            "index.json".to_owned(),
            json!({ "version": self.index_version, "sha256": sha256_hex(&index_bytes), "length": index_bytes.len() }),
        );
        tree.insert("index.json".to_owned(), index_bytes);

        let revocations_signed = json!({
            "type": "revocations",
            "version": self.revocations_version,
            "expires": self.revocations_expires,
            "keys": self.revoked_keys,
            "blocked_ids": self.blocked_ids,
        });
        let revocations_bytes = self.sign_doc("revocations", &revocations_signed);
        file_entries.insert(
            "revocations.json".to_owned(),
            json!({ "version": self.revocations_version, "sha256": sha256_hex(&revocations_bytes), "length": revocations_bytes.len() }),
        );
        tree.insert("revocations.json".to_owned(), revocations_bytes);

        let snapshot_signed = json!({
            "type": "snapshot",
            "version": self.snapshot_version,
            "expires": self.snapshot_expires,
            "files": Value::Object(file_entries),
        });
        let snapshot_bytes = self.sign_doc("snapshot", &snapshot_signed);
        let timestamp_signed = json!({
            "type": "timestamp",
            "version": self.timestamp_version,
            "expires": self.timestamp_expires,
            "snapshot": { "version": self.snapshot_version, "sha256": sha256_hex(&snapshot_bytes), "length": snapshot_bytes.len() },
        });
        tree.insert("timestamp.json".to_owned(), self.sign_doc("timestamp", &timestamp_signed));
        tree.insert("snapshot.json".to_owned(), snapshot_bytes);

        let keys: serde_json::Map<String, Value> = self
            .role_keys
            .iter()
            .map(|seed| (key_id_of(*seed), json!({ "algorithm": "ed25519", "public_key": public_key_b64(*seed) })))
            .collect();
        let key_ids: Vec<String> = self.role_keys.iter().map(|seed| key_id_of(*seed)).collect();
        let role = json!({ "key_ids": key_ids, "threshold": self.threshold });
        let root_signed = json!({
            "type": "root",
            "spec": "ilium-registry/1",
            "version": self.root_version,
            "expires": self.root_expires,
            "keys": Value::Object(keys),
            "roles": { "root": role.clone(), "timestamp": role.clone(), "snapshot": role.clone(), "targets": role },
        });
        tree.insert("root.json".to_owned(), self.sign_doc("root", &root_signed));
        tree
    }

    /// The path of the blob of a published version.
    pub fn blob_path(&self, plugin_id: &str, version: &str) -> String {
        let plugin = self.plugins.iter().find(|plugin| plugin.id == plugin_id).expect("plugin in builder");
        let spec = plugin.versions.iter().find(|spec| spec.version == version).expect("version in builder");
        format!("blobs/{}.iliumpkg", sha256_hex(&spec.package))
    }
}

/// Writes a tree below a directory, creating folders.
pub fn write_tree(root: &Path, tree: &Tree) {
    for (path, bytes) in tree {
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("create tree folder");
        std::fs::write(target, bytes).expect("write tree file");
    }
}

/// Reads every file below a directory into a tree (paths with `/`).
pub fn read_tree(root: &Path) -> Tree {
    fn walk(base: &Path, folder: &Path, tree: &mut Tree) {
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, tree);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let relative = path.strip_prefix(base).expect("below base").to_string_lossy().replace('\\', "/");
                tree.insert(relative, bytes);
            }
        }
    }
    let mut tree = Tree::new();
    walk(root, root, &mut tree);
    tree
}

// ---------------------------------------------------------------------------------------------
// Fake network
// ---------------------------------------------------------------------------------------------

/// A scripted failure that is consumed before the normal file answer.
pub type Scripted = FetchError;

/// The mutable state of the fake network.
#[derive(Default)]
pub struct NetState {
    pub files: BTreeMap<String, Vec<u8>>,
    pub scripted: BTreeMap<String, VecDeque<Scripted>>,
    pub down: bool,
    pub log: Vec<String>,
}

/// A recording static file server behind the injected `Fetcher`.
#[derive(Default)]
pub struct FakeNet {
    pub state: Mutex<NetState>,
}

impl FakeNet {
    pub fn put_file(&self, url: &str, bytes: Vec<u8>) {
        self.state.lock().expect("net lock").files.insert(url.to_owned(), bytes);
    }

    pub fn remove_file(&self, url: &str) {
        self.state.lock().expect("net lock").files.remove(url);
    }

    pub fn serve(&self, base: &str, tree: &Tree) {
        let mut state = self.state.lock().expect("net lock");
        for (path, bytes) in tree {
            state.files.insert(format!("{base}{path}"), bytes.clone());
        }
    }

    /// The next requests of `url` fail with these errors, in order.
    pub fn script(&self, url: &str, failures: Vec<Scripted>) {
        self.state.lock().expect("net lock").scripted.insert(url.to_owned(), failures.into());
    }

    /// Every requested URL, in order.
    pub fn log(&self) -> Vec<String> {
        self.state.lock().expect("net lock").log.clone()
    }

    pub fn clear_log(&self) {
        self.state.lock().expect("net lock").log.clear();
    }

    pub fn set_down(&self, down: bool) {
        self.state.lock().expect("net lock").down = down;
    }

    /// Requested URLs that end with the suffix.
    pub fn requests_to(&self, suffix: &str) -> Vec<String> {
        self.log().into_iter().filter(|url| url.ends_with(suffix)).collect()
    }

    /// The request paths relative to `base`, in order.
    pub fn paths(&self, base: &str) -> Vec<String> {
        self.log().into_iter().filter_map(|url| url.strip_prefix(base).map(str::to_owned)).collect()
    }
}

impl Fetcher for FakeNet {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let mut state = self.state.lock().expect("net lock");
        state.log.push(url.to_owned());
        if state.down {
            return Err(FetchError::Unreachable);
        }
        if let Some(queue) = state.scripted.get_mut(url) {
            if let Some(failure) = queue.pop_front() {
                return Err(failure);
            }
        }
        state.files.get(url).cloned().ok_or(FetchError::Status(404))
    }
}

// ---------------------------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------------------------

/// Serialises tests that set process environment variables.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// A temporary installation (data, state, config, project) wired to a fake network.
pub struct Sandbox {
    pub dir: tempfile::TempDir,
    pub net: Arc<FakeNet>,
    pub env: CliEnv,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        let dir = tempfile::tempdir().expect("create sandbox");
        for sub in ["data", "state", "config", "project"] {
            std::fs::create_dir_all(dir.path().join(sub)).expect("create sandbox folder");
        }
        let net = Arc::new(FakeNet::default());
        let fetcher: Arc<dyn Fetcher> = net.clone();
        let env = CliEnv::new(
            dir.path().join("data"),
            dir.path().join("state"),
            dir.path().join("config"),
            dir.path().join("project"),
            now(),
            true,
            fetcher,
        );
        Sandbox { dir, net, env }
    }

    /// A sandbox whose network serves the registry at `BASE`, pinned with the first role key.
    pub fn with_registry(builder: &RegistryBuilder) -> Sandbox {
        let sandbox = Sandbox::new();
        sandbox.net.serve(BASE, &builder.build());
        let added = sandbox.pin(BASE, &builder.pinned_key_id());
        assert_eq!(added.exit_code, 0, "registry add must succeed in the fixture: {added:?}");
        sandbox
    }

    /// The standard registry, pinned.
    pub fn standard() -> Sandbox {
        Sandbox::with_registry(&RegistryBuilder::standard())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    pub fn config_dir(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    pub fn project_dir(&self) -> PathBuf {
        self.dir.path().join("project")
    }

    /// Runs `ilium plugin <args...>`.
    pub fn run(&self, args: &[&str]) -> CliOutput {
        run_in(&self.env, args)
    }

    /// Runs with another clock.
    pub fn run_at(&self, time: SystemTime, args: &[&str]) -> CliOutput {
        let mut env = self.env.clone();
        env.now = time;
        run_in(&env, args)
    }

    /// Runs with extra process environment variables (set, run, restored under a lock).
    pub fn run_with_vars(&self, vars: &[(&str, &str)], args: &[&str]) -> CliOutput {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous: Vec<(String, Option<String>)> = vars.iter().map(|(name, _)| ((*name).to_owned(), std::env::var(name).ok())).collect();
        for (name, value) in vars {
            std::env::set_var(name, value);
        }
        let output = run_in(&self.env, args);
        for (name, old) in previous {
            match old {
                Some(value) => std::env::set_var(&name, value),
                None => std::env::remove_var(&name),
            }
        }
        output
    }

    /// `registry add <url> --root-key <key_id>`.
    pub fn pin(&self, url: &str, key_id: &str) -> CliOutput {
        self.run(&["registry", "add", url, "--root-key", key_id])
    }

    /// Sets a key in `[plugins.registry]` of `<config>/config.toml`.
    pub fn set_registry_setting(&self, key: &str, value: toml::Value) {
        let path = self.config_dir().join("config.toml");
        let mut document: toml::Table = std::fs::read_to_string(&path).ok().and_then(|text| text.parse::<toml::Table>().ok()).unwrap_or_default();
        let plugins = document
            .entry("plugins".to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .expect("plugins is a table");
        let registry = plugins
            .entry("registry".to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .expect("registry is a table");
        registry.insert(key.to_owned(), value);
        std::fs::write(&path, toml::to_string(&document).expect("serialize config")).expect("write config");
    }

    /// Writes a file in the project folder and returns its path.
    pub fn project_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.project_dir().join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create project subfolder");
        std::fs::write(&path, bytes).expect("write project file");
        path
    }

    /// The installed records (`<data>/plugins/installed.json`).
    pub fn installed(&self) -> Vec<InstalledRecord> {
        let path = self.data_dir().join("plugins").join("installed.json");
        match InstalledRegistry::open(&path) {
            Ok(registry) => registry.list().unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// Seeds one installed record.
    pub fn seed_installed(&self, id: &str, version: &str, source: &str, channel: &str, pinned: bool) {
        let path = self.data_dir().join("plugins").join("installed.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create plugin root");
        let mut registry = InstalledRegistry::open(&path).expect("open installed registry");
        registry
            .put(InstalledRecord { id: id.to_owned(), version: version.to_owned(), source: source.to_owned(), channel: channel.to_owned(), pinned })
            .expect("seed installed record");
    }

    /// Every file below the sandbox with its bytes.
    pub fn snapshot(&self) -> Tree {
        read_tree(self.dir.path())
    }

    /// Every file below a sub folder of the sandbox.
    pub fn snapshot_of(&self, sub: &str) -> Tree {
        read_tree(&self.dir.path().join(sub))
    }

    /// The version installed for a plugin id, if any.
    pub fn installed_version(&self, id: &str) -> Option<String> {
        self.installed().into_iter().find(|record| record.id == id).map(|record| record.version)
    }
}

/// Runs `ilium plugin <args...>` in an environment.
pub fn run_in(env: &CliEnv, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|argument| (*argument).to_owned()).collect();
    cli::run(&owned, env)
}

// ---------------------------------------------------------------------------------------------
// Record selectors and assertions
// ---------------------------------------------------------------------------------------------

/// Records of one `type`.
pub fn records<'a>(output: &'a CliOutput, kind: &str) -> Vec<&'a Value> {
    output.stdout_jsonl.iter().filter(|record| record.get("type").and_then(Value::as_str) == Some(kind)).collect()
}

/// The `code` of every `error` record, in order.
pub fn error_codes(output: &CliOutput) -> Vec<String> {
    records(output, "error").iter().filter_map(|record| record.get("code").and_then(Value::as_str)).map(str::to_owned).collect()
}

/// The `code` of every `warning` record, in order.
pub fn warning_codes(output: &CliOutput) -> Vec<String> {
    records(output, "warning").iter().filter_map(|record| record.get("code").and_then(Value::as_str)).map(str::to_owned).collect()
}

/// The first error code.
pub fn first_error(output: &CliOutput) -> Option<String> {
    error_codes(output).into_iter().next()
}

/// The message of the first error record with a code.
pub fn error_message(output: &CliOutput, code: &str) -> String {
    records(output, "error")
        .iter()
        .find(|record| record.get("code").and_then(Value::as_str) == Some(code))
        .and_then(|record| record.get("message").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

/// The `id` of every `plugin` record, in order.
pub fn plugin_ids(output: &CliOutput) -> Vec<String> {
    records(output, "plugin").iter().filter_map(|record| record.get("id").and_then(Value::as_str)).map(str::to_owned).collect()
}

/// The last record, which must be `result`.
pub fn result_record(output: &CliOutput) -> Option<&Value> {
    output.stdout_jsonl.last().filter(|record| record.get("type").and_then(Value::as_str) == Some("result"))
}

/// Asserts the command failed with `code` and exit code `exit`, and that the `result` record agrees.
pub fn assert_failed(output: &CliOutput, code: &str, exit: i32) {
    assert!(error_codes(output).iter().any(|found| found == code), "expected error {code}, got {:?} in {output:?}", error_codes(output));
    assert_eq!(output.exit_code, exit, "exit code of {code}: {output:?}");
    let result = result_record(output).unwrap_or_else(|| panic!("last record must be result: {output:?}"));
    assert_eq!(result["exit_code"].as_i64(), Some(i64::from(exit)), "result exit_code: {output:?}");
    assert_eq!(result["ok"].as_bool(), Some(false), "result ok: {output:?}");
}

/// Asserts the command succeeded and the `result` record agrees.
pub fn assert_ok(output: &CliOutput) {
    assert_eq!(output.exit_code, 0, "expected success: {output:?}");
    let result = result_record(output).unwrap_or_else(|| panic!("last record must be result: {output:?}"));
    assert_eq!(result["ok"].as_bool(), Some(true), "result ok: {output:?}");
    assert_eq!(result["exit_code"].as_i64(), Some(0), "result exit_code: {output:?}");
}

/// Asserts that no plugin record and no value of the registry data reached the output.
pub fn assert_no_plugin_records(output: &CliOutput) {
    assert!(plugin_ids(output).is_empty(), "no registry value may be used: {output:?}");
}

/// True when any string anywhere in the records contains the needle.
pub fn output_contains(output: &CliOutput, needle: &str) -> bool {
    output.stdout_jsonl.iter().any(|record| record.to_string().contains(needle)) || output.stderr.contains(needle)
}

/// The text of a manual page.
pub fn page_text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read manual page {}: {error}", path.display()))
}

/// Relative request paths below `BASE` in order.
pub fn base_paths(net: &FakeNet) -> Vec<String> {
    net.paths(BASE)
}

/// Asserts an error code without checking the exit code.
pub fn assert_code(output: &CliOutput, code: &str) {
    assert!(error_codes(output).iter().any(|found| found == code), "expected error {code}, got {:?} in {output:?}", error_codes(output));
    assert_ne!(output.exit_code, 0, "a failed command exits non-zero: {output:?}");
}

/// Asserts a failure whose first code is in the `registry.` family.
pub fn assert_registry_failure(output: &CliOutput) {
    let first = first_error(output).unwrap_or_default();
    assert!(first.starts_with("registry."), "expected a registry.* error, got {first:?} in {output:?}");
    assert_ne!(output.exit_code, 0, "{output:?}");
}
