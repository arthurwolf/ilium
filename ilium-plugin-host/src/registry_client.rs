//! The plugin registry client: index documents, metadata roles, revocations, mirrors and the
//! offline cache. HTTP goes through an injected [`Fetcher`] (reference-registry.md).

use crate::error::HostError;

/// A failed fetch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// No route to the host.
    #[error("network unreachable")]
    Unreachable,
    /// The server answered with a non-success status.
    #[error("http status {0}")]
    Status(u16),
}

/// A fetched response with its transport details (reference-install-and-update.md PKG-260, 261).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    /// The HTTP status.
    pub status: u16,
    /// The `Location` header of a redirect.
    pub location: Option<String>,
    /// The `Content-Type` header.
    pub content_type: Option<String>,
    /// The declared `Content-Length`.
    pub content_length: Option<u64>,
    /// The body.
    pub body: Vec<u8>,
}

/// Fetches a URL (injected; tests use a fake).
pub trait Fetcher: Send + Sync {
    /// Gets the body of a URL.
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError>;

    /// Gets a URL with status, redirect target, content type and declared size
    /// (reference-install-and-update.md PKG-260, 261). Fakes that only answer `get` keep the default.
    fn get_response(&self, url: &str) -> Result<FetchResponse, FetchError> {
        self.get(url).map(|body| FetchResponse {
            status: 200,
            location: None,
            content_type: None,
            content_length: Some(body.len() as u64),
            body,
        })
    }
}

/// The revocation document `revocations.json` (reference-registry.md).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Revocations {
    /// Revoked key ids.
    pub keys: Vec<String>,
    /// Revoked `id@version` pairs.
    pub versions: Vec<String>,
}

/// One entry of a registry index.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexEntry {
    /// The plugin id.
    pub id: String,
    /// Latest version on the channel.
    pub version: String,
    /// Where to download it.
    pub url: String,
}

/// Caller-owned trust, time and cache state. The root bytes must be pinned by an
/// explicit registry configuration, never obtained through this client's fetcher.
#[derive(Clone, Default)]
pub struct RegistryContext {
    pub trusted_root: Vec<u8>,
    /// Current UTC time in the manual's `YYYY-MM-DD HH:mm:ss` format.
    pub now: String,
    pub offline: bool,
    /// Exact previously verified documents, persisted by the caller.
    pub documents: std::collections::BTreeMap<String, Vec<u8>>,
    /// Persist these monotonic counters alongside the documents.
    pub highest_versions: std::collections::BTreeMap<String, u64>,
    /// Verified package blobs keyed by lowercase SHA-256.
    pub blobs: std::collections::BTreeMap<String, Vec<u8>>,
}

/// Talks to one registry. Fetching is injected; this type opens no sockets.
/// Cache persistence, transport timeouts/proxies and user warnings belong to the
/// caller. A context-free client refuses metadata rather than trust network keys.
pub struct RegistryClient<'a> {
    fetcher: &'a dyn Fetcher,
    base_url: String,
    context: std::sync::Mutex<RegistryContext>,
    warnings: std::sync::Mutex<Vec<String>>,
}

impl<'a> RegistryClient<'a> {
    pub fn new(fetcher: &'a dyn Fetcher, base_url: String) -> Self {
        Self::with_context(fetcher, base_url, RegistryContext::default())
    }

    pub fn with_context(
        fetcher: &'a dyn Fetcher,
        base_url: String,
        context: RegistryContext,
    ) -> Self {
        Self {
            fetcher,
            base_url,
            context: std::sync::Mutex::new(context),
            warnings: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Returns a coherent cache checkpoint only after verification succeeds.
    pub fn cache(&self) -> Result<RegistryContext, HostError> {
        self.context
            .lock()
            .map(|state| state.clone())
            .map_err(|_| failure("registry.cache-invalid", "registry cache lock poisoned"))
    }

    pub fn warnings(&self) -> Result<Vec<String>, HostError> {
        self.warnings
            .lock()
            .map(|warnings| warnings.clone())
            .map_err(|_| failure("registry.cache-invalid", "registry warnings lock poisoned"))
    }

    pub fn index(&self) -> Result<Vec<IndexEntry>, HostError> {
        let documents = self.refresh()?;
        let index = signed(&documents, "index.json")?;
        let entries = index
            .get("plugins")
            .or_else(|| index.get("entries"))
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                failure(
                    "registry.metadata-invalid",
                    "index must contain a plugins array",
                )
            })?;
        let mut result = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let revocations = signed(&documents, "revocations.json")?;
        for entry in entries {
            let id = text_field(entry, "id")?;
            if !seen.insert(id) {
                return Err(failure("registry.metadata-invalid", "duplicate plugin id"));
            }
            if blocked(&revocations, id) {
                continue;
            }
            let Some(version) = entry
                .get("latest_stable")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let path = format!("plugins/{id}.json");
            let plugin = signed(&documents, &path)?;
            let versions = plugin["versions"]
                .as_array()
                .ok_or_else(|| failure("registry.metadata-invalid", "plugin versions missing"))?;
            let record = versions
                .iter()
                .find(|record| record["version"].as_str() == Some(version))
                .ok_or_else(|| {
                    failure(
                        "registry.not-found",
                        "index version missing from plugin metadata",
                    )
                })?;
            if !record.get("yanked").is_none_or(serde_json::Value::is_null) {
                continue;
            }
            if revoked_key(
                &revocations,
                record.get("key_id").and_then(serde_json::Value::as_str),
                self.current_time()?.as_str(),
            ) {
                continue;
            }
            result.push(IndexEntry {
                id: id.to_owned(),
                version: version.to_owned(),
                url: text_field(record, "url")?.to_owned(),
            });
        }
        Ok(result)
    }

    /// Full authenticated revocation document, including blocked ids, effective
    /// times and reasons that the legacy `Revocations` projection cannot carry.
    pub fn revocation_document(&self) -> Result<serde_json::Value, HostError> {
        signed(&self.refresh()?, "revocations.json")
    }

    pub fn revocations(&self) -> Result<Revocations, HostError> {
        let documents = self.refresh()?;
        let value = signed(&documents, "revocations.json")?;
        Ok(Revocations {
            keys: value["keys"]
                .as_array()
                .ok_or_else(|| failure("registry.metadata-invalid", "revocation keys missing"))?
                .iter()
                .map(|key| text_field(key, "key_id").map(str::to_owned))
                .collect::<Result<_, _>>()?,
            // The compatibility shape cannot expose blocked ids or reasons. Never
            // invent id@version entries; callers needing those details use cache().
            versions: Vec::new(),
        })
    }

    pub fn download(&self, entry: &IndexEntry) -> Result<Vec<u8>, HostError> {
        let documents = self.refresh()?;
        let revocations = signed(&documents, "revocations.json")?;
        if blocked(&revocations, &entry.id) {
            return Err(failure("registry.revoked", "plugin id is blocked"));
        }
        let plugin = signed(&documents, &format!("plugins/{}.json", entry.id))?;
        let record = plugin["versions"]
            .as_array()
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|record| record["version"].as_str() == Some(&entry.version))
            })
            .ok_or_else(|| {
                failure(
                    "registry.not-found",
                    "version has no authenticated metadata",
                )
            })?;
        if text_field(record, "url")? != entry.url {
            return Err(failure(
                "registry.hash-mismatch",
                "download URL differs from authenticated record",
            ));
        }
        if !record.get("yanked").is_none_or(serde_json::Value::is_null) {
            return Err(failure("registry.yanked", "version is yanked"));
        }
        if revoked_key(
            &revocations,
            record.get("key_id").and_then(serde_json::Value::as_str),
            &self.current_time()?,
        ) {
            return Err(failure("registry.revoked", "publisher key is revoked"));
        }
        let length = record["length"]
            .as_u64()
            .ok_or_else(|| failure("registry.metadata-invalid", "package length missing"))?;
        let limit = usize::try_from(length)
            .map_err(|_| failure("registry.size", "package length exceeds platform"))?;
        if limit > 512 * 1024 * 1024 {
            return Err(failure("registry.size", "package exceeds download bound"));
        }
        let cache = self.cache()?;
        let digest = text_field(record, "sha256")?;
        if let Some(bytes) = cache.blobs.get(digest) {
            check_digest(bytes, record)?;
            return Ok(bytes.clone());
        }
        if cache.offline {
            return Err(failure(
                "registry.unreachable",
                "offline package blob missing",
            ));
        }
        let bytes = self.fetch(&entry.url, limit)?;
        check_digest(&bytes, record)?;
        self.context
            .lock()
            .map_err(|_| failure("registry.cache-invalid", "registry cache lock poisoned"))?
            .blobs
            .insert(digest.to_owned(), bytes.clone());
        Ok(bytes)
    }

    fn current_time(&self) -> Result<String, HostError> {
        Ok(self.cache()?.now)
    }

    fn refresh(&self) -> Result<std::collections::BTreeMap<String, Vec<u8>>, HostError> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| failure("registry.cache-invalid", "registry cache lock poisoned"))?;
        let mut next = guard.clone();
        if !valid_time(&next.now) || next.trusted_root.is_empty() {
            return Err(failure(
                "registry.signature-invalid",
                "pinned root and explicit current time are required",
            ));
        }
        if next.trusted_root.len() > 64 * 1024 {
            return Err(failure("registry.size", "pinned root exceeds bound"));
        }
        let root = parse(&next.trusted_root)?;
        if root["signed"]["spec"].as_str() != Some("ilium-registry/1") {
            return Err(failure(
                "registry.signature-invalid",
                "unsupported registry root spec",
            ));
        }
        verify(&root, &root["signed"], "root")?;
        let offline = next.offline;
        check_document(&root["signed"], "root", "root.json", &mut next, offline)?;
        let mut documents = next.documents.clone();
        if !next.offline {
            match self.fetch("timestamp.json", 16 * 1024) {
                Ok(bytes) => {
                    documents.insert("timestamp.json".into(), bytes);
                }
                Err(error)
                    if error.code().as_str() == "registry.unreachable"
                        && documents.contains_key("timestamp.json") => {}
                Err(error) => return Err(error),
            }
        }
        let timestamp = verified_document(
            &documents,
            "timestamp.json",
            &root["signed"],
            "timestamp",
            &mut next,
        )?;
        let snapshot_record = &timestamp["snapshot"];
        self.obtain(
            &mut documents,
            "snapshot.json",
            snapshot_record,
            4 * 1024 * 1024,
            next.offline,
        )?;
        let snapshot = verified_document(
            &documents,
            "snapshot.json",
            &root["signed"],
            "snapshot",
            &mut next,
        )?;
        check_version(&snapshot, snapshot_record)?;
        let files = snapshot["files"]
            .as_object()
            .ok_or_else(|| failure("registry.metadata-invalid", "snapshot files missing"))?;
        for (path, record) in files {
            let limit = metadata_limit(path)?;
            if path == "revocations.json" && !next.offline {
                match self.fetch(path, limit) {
                    Ok(bytes) => {
                        check_digest(&bytes, record)?;
                        documents.insert(path.clone(), bytes);
                    }
                    Err(error)
                        if error.code().as_str() == "registry.unreachable"
                            && documents.contains_key(path) => {}
                    Err(error) => return Err(error),
                }
            }
            self.obtain(&mut documents, path, record, limit, next.offline)?;
            let role = if path == "revocations.json" {
                "timestamp"
            } else {
                "targets"
            };
            let value = verified_document(&documents, path, &root["signed"], role, &mut next)?;
            check_version(&value, record)?;
            if path.starts_with("plugins/")
                && path != &format!("plugins/{}.json", text_field(&value, "id")?)
            {
                return Err(failure(
                    "registry.metadata-invalid",
                    "plugin document id does not match path",
                ));
            }
        }
        if !files.contains_key("index.json") || !files.contains_key("revocations.json") {
            return Err(failure(
                "registry.metadata-invalid",
                "snapshot must authenticate index and revocations",
            ));
        }
        // Drop files no longer in the authenticated snapshot, preventing stale
        // targets from becoming downloadable after removal.
        documents.retain(|path, _| {
            path == "timestamp.json" || path == "snapshot.json" || files.contains_key(path)
        });
        if next.offline {
            let mut warnings = self
                .warnings
                .lock()
                .map_err(|_| failure("registry.cache-invalid", "warnings lock poisoned"))?;
            warnings.clear();
            warnings.push(format!(
                "Offline registry metadata may be stale; current time {}",
                next.now
            ));
        }
        next.documents = documents.clone();
        *guard = next;
        Ok(documents)
    }

    fn obtain(
        &self,
        documents: &mut std::collections::BTreeMap<String, Vec<u8>>,
        path: &str,
        record: &serde_json::Value,
        limit: usize,
        offline: bool,
    ) -> Result<(), HostError> {
        if let Some(bytes) = documents.get(path) {
            if check_digest(bytes, record).is_ok() {
                return Ok(());
            }
        }
        if offline {
            return Err(failure(
                "registry.hash-mismatch",
                "offline cache is missing authenticated content",
            ));
        }
        let bytes = self.fetch(path, limit)?;
        check_digest(&bytes, record)?;
        documents.insert(path.into(), bytes);
        Ok(())
    }

    fn fetch(&self, path: &str, limit: usize) -> Result<Vec<u8>, HostError> {
        let mut url = resolve_url(&self.base_url, path)?;
        for redirect in 0..=3 {
            let response = self
                .fetcher
                .get_response(&url)
                .map_err(|error| match error {
                    FetchError::Status(404) => {
                        failure("registry.not-found", "registry resource missing")
                    }
                    FetchError::Status(_) => {
                        failure("registry.http", "registry HTTP request failed")
                    }
                    FetchError::Unreachable => {
                        failure("registry.unreachable", "registry unreachable")
                    }
                })?;
            if (300..400).contains(&response.status) {
                if redirect == 3 {
                    return Err(failure("registry.insecure-url", "too many redirects"));
                }
                url = resolve_url(
                    &url,
                    response
                        .location
                        .as_deref()
                        .ok_or_else(|| failure("registry.http", "redirect has no location"))?,
                )?;
                continue;
            }
            if response.status != 200 {
                return Err(failure(
                    if response.status == 404 {
                        "registry.not-found"
                    } else {
                        "registry.http"
                    },
                    "non-success registry response",
                ));
            }
            if response.body.len() > limit
                || response
                    .content_length
                    .is_some_and(|size| size > limit as u64)
            {
                return Err(failure(
                    "registry.size",
                    "registry response exceeds its bound",
                ));
            }
            return Ok(response.body);
        }
        Err(failure("registry.http", "redirect limit"))
    }
}

fn failure(code: &'static str, message: &str) -> HostError {
    ilium_plugin_api::Diagnostic::error(ilium_plugin_api::Code::from_static(code), "", message)
        .into()
}

fn text_field<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, HostError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| failure("registry.metadata-invalid", "required string field missing"))
}

fn resolve_url(base: &str, path: &str) -> Result<String, HostError> {
    let url = if path.starts_with("https://") {
        path.to_owned()
    } else {
        if path.contains(':')
            || path.starts_with('/')
            || path.split('/').any(|part| part == "." || part == "..")
            || path.contains(['\\', '?', '#', '%'])
        {
            return Err(failure("registry.insecure-url", "unsafe registry path"));
        }
        format!("{}/{}", base.trim_end_matches('/'), path)
    };
    let authority = url
        .strip_prefix("https://")
        .and_then(|tail| tail.split('/').next())
        .unwrap_or("");
    if authority.is_empty()
        || authority.contains(['@', '?', '#', '\\'])
        || url.chars().any(char::is_control)
    {
        return Err(failure(
            "registry.insecure-url",
            "HTTPS registry URL required",
        ));
    }
    Ok(url)
}

fn check_digest(bytes: &[u8], record: &serde_json::Value) -> Result<(), HostError> {
    use sha2::{Digest, Sha256};
    if record["length"].as_u64() != Some(bytes.len() as u64)
        || record["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(bytes)).as_str())
    {
        return Err(failure(
            "registry.hash-mismatch",
            "authenticated hash or length mismatch",
        ));
    }
    Ok(())
}

fn check_version(value: &serde_json::Value, record: &serde_json::Value) -> Result<(), HostError> {
    if value["version"].as_u64().is_none()
        || value["version"].as_u64() != record["version"].as_u64()
    {
        return Err(failure(
            "registry.hash-mismatch",
            "authenticated version mismatch",
        ));
    }
    Ok(())
}

fn metadata_limit(path: &str) -> Result<usize, HostError> {
    match path {
        "index.json" => Ok(32 * 1024 * 1024),
        "revocations.json" => Ok(4 * 1024 * 1024),
        _ if path.starts_with("plugins/")
            && path.ends_with(".json")
            && path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/.-_".contains(&byte))
            && path.matches('/').count() == 1
            && !path.contains("..") =>
        {
            Ok(1024 * 1024)
        }
        _ => Err(failure(
            "registry.metadata-invalid",
            "unsupported snapshot target",
        )),
    }
}

fn time_seconds(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 19
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b' ',
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    let year: i64 = value[..4].parse().ok()?;
    let month: usize = value[5..7].parse().ok()?;
    let day: i64 = value[8..10].parse().ok()?;
    let hour: i64 = value[11..13].parse().ok()?;
    let minute: i64 = value[14..16].parse().ok()?;
    let second: i64 = value[17..19].parse().ok()?;
    let leap = |year: i64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0
        || !(1..=12).contains(&month)
        || day < 1
        || day > months[month - 1]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let preceding = year - 1;
    let days = preceding * 365 + preceding / 4 - preceding / 100
        + preceding / 400
        + months[..month - 1].iter().sum::<i64>()
        + day
        - 1;
    Some(((days * 24 + hour) * 60 + minute) * 60 + second)
}

fn valid_time(value: &str) -> bool {
    time_seconds(value).is_some()
}

fn check_document(
    value: &serde_json::Value,
    kind: &str,
    path: &str,
    context: &mut RegistryContext,
    allow_stale: bool,
) -> Result<(), HostError> {
    if text_field(value, "type")? != kind {
        return Err(failure(
            "registry.metadata-invalid",
            "wrong metadata role type",
        ));
    }
    let version = value["version"]
        .as_u64()
        .filter(|version| *version > 0)
        .ok_or_else(|| {
            failure(
                "registry.metadata-invalid",
                "positive metadata version required",
            )
        })?;
    if context
        .highest_versions
        .get(path)
        .is_some_and(|highest| version < *highest)
    {
        return Err(failure("registry.rollback", "metadata version decreased"));
    }
    let expiry = text_field(value, "expires")?;
    if !valid_time(expiry) {
        return Err(failure("registry.metadata-invalid", "invalid UTC expiry"));
    }
    if expiry < context.now.as_str() && !allow_stale {
        return Err(failure(
            "registry.metadata-expired",
            "metadata expired; explicit --offline required",
        ));
    }
    if kind == "timestamp"
        && time_seconds(expiry)
            .zip(time_seconds(&context.now))
            .is_some_and(|(expiry, now)| expiry - now > 86_400)
    {
        return Err(failure(
            "registry.metadata-invalid",
            "timestamp freshness exceeds one day",
        ));
    }
    context.highest_versions.insert(path.into(), version);
    Ok(())
}

fn verified_document(
    documents: &std::collections::BTreeMap<String, Vec<u8>>,
    path: &str,
    root: &serde_json::Value,
    role: &str,
    context: &mut RegistryContext,
) -> Result<serde_json::Value, HostError> {
    let bytes = documents
        .get(path)
        .ok_or_else(|| failure("registry.unreachable", "verified cache missing document"))?;
    let limit = match path {
        "timestamp.json" => 16 * 1024,
        "snapshot.json" => 4 * 1024 * 1024,
        _ => metadata_limit(path)?,
    };
    if bytes.len() > limit {
        return Err(failure("registry.size", "cached metadata exceeds bound"));
    }
    let document = parse(bytes)?;
    verify(&document, root, role)?;
    let kind = match path {
        "timestamp.json" => "timestamp",
        "snapshot.json" => "snapshot",
        "index.json" => "index",
        "revocations.json" => "revocations",
        _ => "plugin",
    };
    let offline = context.offline;
    check_document(&document["signed"], kind, path, context, offline)?;
    if kind == "revocations" {
        let keys = document["signed"]["keys"].as_array().ok_or_else(|| {
            failure(
                "registry.metadata-invalid",
                "revocations keys array missing",
            )
        })?;
        for key in keys {
            text_field(key, "publisher_id")?;
            let id = text_field(key, "key_id")?;
            if id.len() != 64
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !valid_time(text_field(key, "effective")?)
                || !matches!(text_field(key, "reason")?, "compromised" | "superseded")
            {
                return Err(failure(
                    "registry.metadata-invalid",
                    "invalid revocation record",
                ));
            }
        }
        let ids = document["signed"]["blocked_ids"]
            .as_array()
            .ok_or_else(|| failure("registry.metadata-invalid", "blocked ids array missing"))?;
        for id in ids {
            text_field(id, "id")?;
            text_field(id, "reason")?;
            if !valid_time(text_field(id, "at")?) {
                return Err(failure(
                    "registry.metadata-invalid",
                    "invalid blocked-id timestamp",
                ));
            }
        }
    }
    Ok(document["signed"].clone())
}

fn verify(
    document: &serde_json::Value,
    root: &serde_json::Value,
    role: &str,
) -> Result<(), HostError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ed25519_dalek::{Signature, VerifyingKey};
    use sha2::{Digest, Sha256};
    let roles = &root["roles"][role];
    let threshold = roles["threshold"]
        .as_u64()
        .filter(|threshold| *threshold > 0)
        .ok_or_else(|| {
            failure(
                "registry.signature-invalid",
                "positive key threshold required",
            )
        })?;
    let ids = roles["key_ids"]
        .as_array()
        .ok_or_else(|| failure("registry.signature-invalid", "role keys missing"))?;
    let canonical = crate::signing::jcs(&document["signed"]).map_err(|_| {
        failure(
            "registry.signature-invalid",
            "metadata cannot be canonicalized",
        )
    })?;
    let mut message = b"ilium-registry-metadata-v1\0".to_vec();
    message.extend_from_slice(canonical.as_bytes());
    let signatures = document["signatures"]
        .as_array()
        .ok_or_else(|| failure("registry.signature-invalid", "metadata signatures missing"))?;
    let mut valid = std::collections::BTreeSet::new();
    for signature in signatures {
        let id = text_field(signature, "key_id")?;
        if !ids.iter().any(|candidate| candidate.as_str() == Some(id)) || valid.contains(id) {
            continue;
        }
        let key = &root["keys"][id];
        if key["algorithm"].as_str() != Some("ed25519") {
            continue;
        }
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(text_field(key, "public_key")?) else {
            continue;
        };
        let Ok(bytes): Result<[u8; 32], _> = bytes.try_into() else {
            continue;
        };
        if format!("{:x}", Sha256::digest(bytes)) != id {
            continue;
        }
        let Ok(key) = VerifyingKey::from_bytes(&bytes) else {
            continue;
        };
        if key.is_weak() || key.to_edwards().compress().to_bytes() != bytes {
            continue;
        }
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(text_field(signature, "signature")?) else {
            continue;
        };
        let Ok(signature) = Signature::from_slice(&bytes) else {
            continue;
        };
        if key.verify_strict(&message, &signature).is_ok() {
            valid.insert(id.to_owned());
        }
    }
    if (valid.len() as u64) < threshold {
        return Err(failure(
            "registry.signature-invalid",
            "metadata signature threshold failed",
        ));
    }
    Ok(())
}

fn signed(
    documents: &std::collections::BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<serde_json::Value, HostError> {
    documents
        .get(path)
        .ok_or_else(|| failure("registry.not-found", "metadata target missing"))
        .and_then(|bytes| parse(bytes))
        .map(|value| value["signed"].clone())
}

fn blocked(revocations: &serde_json::Value, id: &str) -> bool {
    revocations["blocked_ids"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|entry| entry["id"].as_str() == Some(id)))
}

fn revoked_key(revocations: &serde_json::Value, id: Option<&str>, now: &str) -> bool {
    revocations["keys"].as_array().is_some_and(|keys| {
        keys.iter().any(|entry| {
            entry["key_id"].as_str() == id
                && entry["effective"]
                    .as_str()
                    .is_none_or(|effective| effective <= now)
        })
    })
}

// serde_json::Value normally keeps the last duplicate. Registry signatures must
// never authenticate an ambiguous parse (PKG-652).
fn parse(bytes: &[u8]) -> Result<serde_json::Value, HostError> {
    struct Strict(serde_json::Value);
    impl<'de> serde::Deserialize<'de> for Strict {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Strict;
                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("unambiguous JSON")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut access: A,
                ) -> Result<Strict, A::Error> {
                    let mut object = serde_json::Map::new();
                    while let Some((key, value)) = access.next_entry::<String, Strict>()? {
                        if object.insert(key, value.0).is_some() {
                            return Err(serde::de::Error::custom("duplicate JSON field"));
                        }
                    }
                    Ok(Strict(serde_json::Value::Object(object)))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut access: A,
                ) -> Result<Strict, A::Error> {
                    let mut values = Vec::new();
                    while let Some(value) = access.next_element::<Strict>()? {
                        values.push(value.0);
                    }
                    Ok(Strict(serde_json::Value::Array(values)))
                }
                fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Strict, E> {
                    Ok(Strict(value.into()))
                }
                fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Strict, E> {
                    Ok(Strict(value.into()))
                }
                fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Strict, E> {
                    Ok(Strict(value.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Strict, E> {
                    Ok(Strict(value.into()))
                }
                fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Strict, E> {
                    Ok(Strict(value.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Strict, E> {
                    serde_json::Number::from_f64(value)
                        .map(|number| Strict(number.into()))
                        .ok_or_else(|| E::custom("nonfinite number"))
                }
                fn visit_unit<E: serde::de::Error>(self) -> Result<Strict, E> {
                    Ok(Strict(serde_json::Value::Null))
                }
            }
            deserializer.deserialize_any(Visitor)
        }
    }
    serde_json::from_slice::<Strict>(bytes)
        .map(|value| value.0)
        .map_err(|_| {
            failure(
                "registry.signature-invalid",
                "metadata JSON invalid or ambiguous",
            )
        })
}
