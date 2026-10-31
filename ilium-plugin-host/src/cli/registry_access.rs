//! Opens a pinned registry for the CLI commands that read it (`search`, `info`, `mirror` ...).
//!
//! The pinned root key ids come from `[plugins.registry]` in `config.toml` (written by
//! `ilium plugin registry add`). The root document is fetched, matched against the pin and handed
//! to `RegistryClient`, which verifies every other document. The verified documents and the
//! monotonic version counters are cached under the state directory so `--offline` and rollback
//! detection work across runs (reference-registry.md PKG-378, PKG-417).

use super::{offline, CliEnv, CliOutput};
use crate::registry_client::{RegistryClient, RegistryContext};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The verified documents of one registry at one moment.
pub struct Snapshot {
    /// The registry base URL.
    pub url: String,
    /// Verified documents by path (`index.json`, `plugins/<id>.json`, ...).
    pub documents: BTreeMap<String, Vec<u8>>,
    /// Human-readable warnings, such as stale offline data.
    pub warnings: Vec<String>,
}

impl Snapshot {
    /// The `signed` body of a verified document.
    pub fn signed(&self, path: &str) -> Option<Value> {
        let bytes = self.documents.get(path)?;
        let document: Value = serde_json::from_slice(bytes).ok()?;
        document.get("signed").cloned()
    }

    /// The index entries that are not blocked by the revocation document.
    pub fn index_entries(&self) -> Vec<Value> {
        let blocked: Vec<String> = self
            .signed("revocations.json")
            .and_then(|revocations| revocations.get("blocked_ids").cloned())
            .and_then(|ids| ids.as_array().cloned())
            .map(|ids| {
                ids.iter()
                    .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        self.signed("index.json")
            .and_then(|index| index.get("plugins").cloned())
            .and_then(|plugins| plugins.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| {
                entry["id"]
                    .as_str()
                    .is_some_and(|id| !blocked.iter().any(|b| b == id))
            })
            .collect()
    }
}

struct Settings {
    urls: Vec<String>,
    keys: BTreeMap<String, Vec<String>>,
    shared_keys: Vec<String>,
    allow_insecure_localhost: bool,
    offline: bool,
}

fn settings(env: &CliEnv) -> Settings {
    let document = std::fs::read_to_string(env.config_dir.join("config.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default();
    let registry = document
        .get("plugins")
        .and_then(toml::Value::as_table)
        .and_then(|plugins| plugins.get("registry"))
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    let strings = |value: Option<&toml::Value>| -> Vec<String> {
        value
            .and_then(toml::Value::as_array)
            .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    let mut keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(table) = registry.get("root_keys").and_then(toml::Value::as_table) {
        for (url, key) in table {
            if let Some(key) = key.as_str() {
                keys.entry(url.clone()).or_default().push(key.to_owned());
            }
        }
    }
    Settings {
        urls: strings(registry.get("urls")),
        keys,
        shared_keys: strings(registry.get("root")),
        allow_insecure_localhost: registry
            .get("allow_insecure_localhost")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        offline: registry.get("offline").and_then(toml::Value::as_bool).unwrap_or(false),
    }
}

/// The exit code a registry error code maps to (reference-plugin-distribution-cli.md PKG-424).
pub fn exit_code_for(code: &str) -> i32 {
    match code {
        "registry.unreachable"
        | "registry.metadata-expired"
        | "registry.download-failed"
        | "registry.stale" => 8,
        "registry.not-found" => 5,
        _ => 1,
    }
}

fn host_of(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|tail| tail.split('/').next())
        .map(|authority| authority.rsplit('@').next().unwrap_or(authority))
        .map(|authority| authority.split(':').next().unwrap_or(authority))
        .unwrap_or("")
}

fn cache_path(env: &CliEnv, url: &str) -> std::path::PathBuf {
    let digest: String = Sha256::digest(url.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    env.state_dir
        .join("registry-cache")
        .join(format!("{}.json", &digest[..32]))
}

struct Cached {
    root: Vec<u8>,
    documents: BTreeMap<String, Vec<u8>>,
    highest_versions: BTreeMap<String, u64>,
}

fn load_cache(env: &CliEnv, url: &str) -> Option<Cached> {
    let value: Value = serde_json::from_slice(&std::fs::read(cache_path(env, url)).ok()?).ok()?;
    let documents = value["documents"]
        .as_object()?
        .iter()
        .filter_map(|(path, text)| Some((path.clone(), text.as_str()?.as_bytes().to_vec())))
        .collect();
    let highest_versions = value["highest_versions"]
        .as_object()
        .map(|map| map.iter().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect())
        .unwrap_or_default();
    Some(Cached {
        root: value["root"].as_str()?.as_bytes().to_vec(),
        documents,
        highest_versions,
    })
}

fn store_cache(env: &CliEnv, url: &str, root: &[u8], context: &RegistryContext) {
    let documents: serde_json::Map<String, Value> = context
        .documents
        .iter()
        .filter_map(|(path, bytes)| Some((path.clone(), json!(std::str::from_utf8(bytes).ok()?))))
        .collect();
    let value = json!({
        "root": String::from_utf8_lossy(root),
        "documents": documents,
        "highest_versions": context.highest_versions,
    });
    let path = cache_path(env, url);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // A cache that cannot be written only costs the next offline run; never fail the command.
    let _ = std::fs::write(&path, value.to_string());
}

fn failure(code: &str, message: impl Into<String>) -> CliOutput {
    let exit = exit_code_for(code);
    offline::failure(code, message.into(), exit)
}

/// Opens the registry named by `registry_override`, else the first configured one.
pub fn open(
    env: &CliEnv,
    registry_override: Option<&str>,
    offline_flag: bool,
) -> Result<Snapshot, CliOutput> {
    let settings = settings(env);
    let Some(url) = registry_override
        .map(str::to_owned)
        .or_else(|| settings.urls.first().cloned())
    else {
        return Err(failure(
            "registry.none-configured",
            "No registry is set up. Add one with: ilium plugin registry add <url> --root-key <key id>",
        ));
    };
    let localhost = matches!(host_of(&url), "localhost" | "127.0.0.1" | "[::1]");
    let secure = url.starts_with("https://")
        || url.starts_with("file://")
        || (url.starts_with("http://") && localhost && settings.allow_insecure_localhost);
    if !secure {
        return Err(failure(
            "registry.insecure-url",
            format!("{url} is not an https:// URL"),
        ));
    }
    let mut pinned: Vec<String> = settings.keys.get(&url).cloned().unwrap_or_default();
    pinned.extend(settings.shared_keys.iter().cloned());
    if pinned.is_empty() {
        return Err(failure(
            "registry.root-key-missing",
            format!("{url} has no pinned root key; run registry add with --root-key"),
        ));
    }
    let offline_mode = offline_flag
        || settings.offline
        || env.env.get("ILIUM_PLUGIN_OFFLINE").is_some_and(|value| value == "1");
    let cached = load_cache(env, &url);
    let root = if offline_mode {
        match &cached {
            Some(cached) => cached.root.clone(),
            None => {
                return Err(failure(
                    "registry.unreachable",
                    "offline and no cached registry metadata",
                ))
            }
        }
    } else {
        let address = format!("{}/root.json", url.trim_end_matches('/'));
        match env.fetcher.get(&address) {
            Ok(bytes) => bytes,
            Err(_) => match &cached {
                Some(cached) => cached.root.clone(),
                None => {
                    return Err(failure(
                        "registry.unreachable",
                        format!("cannot reach {url}"),
                    ))
                }
            },
        }
    };
    check_pin(&root, &pinned)?;
    let now = crate::trust::utc_timestamp(env.now)
        .map_err(|error| failure("registry.signature-invalid", error.to_string()))?;
    let context = RegistryContext {
        trusted_root: root.clone(),
        now,
        offline: offline_mode,
        documents: cached.as_ref().map(|c| c.documents.clone()).unwrap_or_default(),
        highest_versions: cached.map(|c| c.highest_versions).unwrap_or_default(),
        blobs: BTreeMap::new(),
    };
    let client = RegistryClient::with_context(env.fetcher.as_ref(), url.clone(), context);
    if let Err(error) = client.index() {
        let code = error.code().as_str().to_owned();
        return Err(failure(&code, error.to_string()));
    }
    let checkpoint = client
        .cache()
        .map_err(|error| failure(error.code().as_str(), error.to_string()))?;
    store_cache(env, &url, &root, &checkpoint);
    Ok(Snapshot {
        url,
        documents: checkpoint.documents,
        warnings: client.warnings().unwrap_or_default(),
    })
}

fn check_pin(root: &[u8], pinned: &[String]) -> Result<(), CliOutput> {
    let document: Value = serde_json::from_slice(root)
        .map_err(|_| failure("registry.signature-invalid", "root.json is not valid JSON"))?;
    let listed = document["signed"]["roles"]["root"]["key_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let matches = listed
        .iter()
        .filter_map(Value::as_str)
        .any(|id| pinned.iter().any(|pin| pin == id));
    if matches {
        Ok(())
    } else {
        Err(failure(
            "registry.signature-invalid",
            "root.json is not signed by the pinned root key",
        ))
    }
}
