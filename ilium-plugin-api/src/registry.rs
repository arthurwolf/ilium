//! A model of a plugin registry: publish, yank, takedown, owners, metadata and the transparency
//! log (reference-registry.md, PKG-800 to PKG-824). Clones of a [`Registry`] share one state.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use semver::{Version, VersionReq};
use serde_json::{json, Value};

use crate::error::PluginError;
use crate::generated::types::ErrorCode;
use crate::http::HttpResponse;

/// The largest publish body the registry accepts (PKG-810).
pub const MAX_PUBLISH_BYTES: u64 = 300 * 1024 * 1024;
/// How long an upload may make no progress by default (PKG-810).
pub const DEFAULT_UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long after a publish the release is in the signed index (PKG-809).
pub const INDEX_DELAY: Duration = Duration::from_secs(30);
/// Longest yank reason in characters (PKG-801).
pub const MAX_YANK_REASON_CHARS: usize = 500;

const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Lower-case hex SHA-256 of `bytes` (FIPS 180-4). The API crate has no digest dependency.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend(((bytes.len() as u64) * 8).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut schedule = [0u32; 64];
        for (index, word) in block.chunks_exact(4).enumerate() {
            schedule[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let low = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let high = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(low)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(high);
        }
        let mut working = state;
        for index in 0..64 {
            let sum1 = working[4].rotate_right(6)
                ^ working[4].rotate_right(11)
                ^ working[4].rotate_right(25);
            let choice = (working[4] & working[5]) ^ (!working[4] & working[6]);
            let temp1 = working[7]
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(SHA256_ROUND_CONSTANTS[index])
                .wrapping_add(schedule[index]);
            let sum0 = working[0].rotate_right(2)
                ^ working[0].rotate_right(13)
                ^ working[0].rotate_right(22);
            let majority = (working[0] & working[1]) ^ (working[0] & working[2]) ^ (working[1] & working[2]);
            let temp2 = sum0.wrapping_add(majority);
            working[7] = working[6];
            working[6] = working[5];
            working[5] = working[4];
            working[4] = working[3].wrapping_add(temp1);
            working[3] = working[2];
            working[2] = working[1];
            working[1] = working[0];
            working[0] = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip(working) {
            *slot = slot.wrapping_add(value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// The current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time_of_day / 3600,
        time_of_day % 3600 / 60,
        time_of_day % 60
    )
}

#[derive(Debug, Clone)]
struct Owner {
    publisher: String,
    keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct YankRecord {
    at: String,
    by: String,
    reason: String,
}

#[derive(Debug, Clone)]
struct VersionRecord {
    sha256: String,
    length: u64,
    channel: String,
    published_at: String,
    requires: Vec<(String, String, bool)>,
    yank: Option<YankRecord>,
    taken_down: bool,
}

#[derive(Debug, Clone)]
struct Tombstone {
    version: String,
    sha256: String,
    reason_code: String,
    time: String,
}

#[derive(Debug, Default)]
struct PluginRecord {
    owners: Vec<Owner>,
    versions: BTreeMap<String, VersionRecord>,
    tombstones: Vec<Tombstone>,
}

#[derive(Debug, Default)]
struct State {
    plugins: BTreeMap<String, PluginRecord>,
    blobs: BTreeMap<String, Vec<u8>>,
    log: Vec<LogEntry>,
    revision: u64,
    requests: usize,
    downloads: usize,
    upstreams: Vec<Registry>,
}

fn refuse(code: &str, base: ErrorCode, message: impl std::fmt::Display) -> PluginError {
    PluginError::coded(code, base, message)
}

/// A registry reached at a URL.
#[derive(Debug, Clone)]
pub struct Registry {
    /// The base URL of the registry.
    pub base_url: String,
    state: Arc<Mutex<State>>,
    upload_idle_timeout: Duration,
}

/// A request to publish one version (PKG-810, PKG-811, PKG-812, PKG-822).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRequest {
    /// Plugin id.
    pub id: String,
    /// Plugin version.
    pub version: String,
    /// Fingerprint of the signing key.
    pub signer_key: String,
    /// The package bytes.
    pub package: Vec<u8>,
    /// Required plugins as (id, range, optional).
    pub requires_plugins: Vec<(String, String, bool)>,
    /// The channel, for example `stable` or `beta`.
    pub channel: Option<String>,
}

/// The result of a successful publish (PKG-809).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReceipt {
    /// Plugin id.
    pub id: String,
    /// Plugin version.
    pub version: String,
    /// Lower-case hex SHA-256 of the package.
    pub sha256: String,
    /// Package length in bytes.
    pub length: u64,
    /// Where the package can be downloaded.
    pub url: String,
    /// The channel the version was published to.
    pub channel: String,
    /// When the version was published.
    pub published_at: String,
}

/// A request to yank or unyank one version (PKG-801, PKG-802).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YankRequest {
    /// Plugin id.
    pub id: String,
    /// Plugin version.
    pub version: String,
    /// Fingerprint of the signing key.
    pub signer_key: String,
    /// The yank reason (1 to 500 characters); unused by an unyank.
    pub reason: Option<String>,
}

/// The result of a successful yank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YankReceipt {
    /// Plugin id.
    pub id: String,
    /// Plugin version.
    pub version: String,
    /// When the version was yanked.
    pub yanked_at: String,
}

/// One entry of the registry's transparency log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    action: String,
    id: String,
    detail: String,
    time: String,
}

/// A published version as clients and the resolver read it.
#[derive(Debug, Clone)]
pub(crate) struct VersionInfo {
    pub version: String,
    pub sha256: String,
    pub length: u64,
    pub channel: String,
    pub requires: Vec<(String, String, bool)>,
    pub yank_reason: Option<String>,
    pub taken_down: bool,
}

impl VersionInfo {
    fn parsed(&self) -> Option<Version> {
        Version::parse(&self.version).ok()
    }

    /// True when `range` accepts this version (pre-releases only for an exact `=` range).
    pub(crate) fn satisfies(&self, range: &str) -> bool {
        match (VersionReq::parse(range), self.parsed()) {
            (Ok(requirement), Some(version)) => requirement.matches(&version),
            _ => false,
        }
    }
}

impl PublishRequest {
    /// A request for `id@version` signed by `signer_key`.
    pub fn new(id: &str, version: &str, signer_key: &str) -> PublishRequest {
        PublishRequest {
            id: id.to_owned(),
            version: version.to_owned(),
            signer_key: signer_key.to_owned(),
            package: Vec::new(),
            requires_plugins: Vec::new(),
            channel: None,
        }
    }

    /// Sets the package bytes.
    pub fn with_package(mut self, bytes: Vec<u8>) -> PublishRequest {
        self.package = bytes;
        self
    }

    /// Adds a required plugin with its version range.
    pub fn with_requires_plugin(mut self, id: &str, range: &str, optional: bool) -> PublishRequest {
        self.requires_plugins
            .push((id.to_owned(), range.to_owned(), optional));
        self
    }

    /// Sets the channel.
    pub fn with_channel(mut self, channel: &str) -> PublishRequest {
        self.channel = Some(channel.to_owned());
        self
    }
}

impl PublishReceipt {
    /// The 201 body of a successful publish, as JSON text (PKG-809: `id`, `version`, `sha256`,
    /// `length`, `url`, `channel`, `published_at`).
    pub fn response_body(&self) -> String {
        json!({
            "id": self.id,
            "version": self.version,
            "sha256": self.sha256,
            "length": self.length,
            "url": self.url,
            "channel": self.channel,
            "published_at": self.published_at,
        })
        .to_string()
    }

    /// The HTTP status of a successful publish.
    pub fn status(&self) -> u16 {
        201
    }
}

impl YankRequest {
    /// A request for `id@version` signed by `signer_key`.
    pub fn new(id: &str, version: &str, signer_key: &str) -> YankRequest {
        YankRequest {
            id: id.to_owned(),
            version: version.to_owned(),
            signer_key: signer_key.to_owned(),
            reason: None,
        }
    }

    /// Sets the reason.
    pub fn with_reason(mut self, reason: &str) -> YankRequest {
        self.reason = Some(reason.to_owned());
        self
    }
}

impl LogEntry {
    /// The action recorded, for example `unyank` or `owner-add`.
    pub fn action(&self) -> &str {
        &self.action
    }

    /// The plugin id the entry is about.
    pub fn plugin_id(&self) -> &str {
        &self.id
    }

    /// Free-text detail of the entry.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// When the entry was appended.
    pub fn time(&self) -> &str {
        &self.time
    }
}

impl State {
    fn record_write(&mut self, action: &str, id: &str, detail: &str) {
        self.revision += 1;
        self.log.push(LogEntry {
            action: action.to_owned(),
            id: id.to_owned(),
            detail: detail.to_owned(),
            time: utc_timestamp(),
        });
    }
}

fn version_info(version: &str, record: &VersionRecord) -> VersionInfo {
    VersionInfo {
        version: version.to_owned(),
        sha256: record.sha256.clone(),
        length: record.length,
        channel: record.channel.clone(),
        requires: record.requires.clone(),
        yank_reason: record.yank.as_ref().map(|yank| yank.reason.clone()),
        taken_down: record.taken_down,
    }
}

impl Registry {
    /// A registry served at `url`.
    pub fn at(url: &str) -> Registry {
        Registry {
            base_url: url.to_owned(),
            state: Arc::new(Mutex::new(State::default())),
            upload_idle_timeout: DEFAULT_UPLOAD_IDLE_TIMEOUT,
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The base URL.
    pub fn url(&self) -> &str {
        &self.base_url
    }

    fn upstreams(&self) -> Vec<Registry> {
        self.state().upstreams.clone()
    }

    /// True when this registry or one of its upstreams holds a plugin with this id.
    pub(crate) fn serves(&self, id: &str) -> bool {
        self.serving_registry(id).is_some()
    }

    /// The registry (this one or an upstream) that holds `id`.
    pub(crate) fn serving_registry(&self, id: &str) -> Option<Registry> {
        if self.state().plugins.contains_key(id) {
            return Some(self.clone());
        }
        self.upstreams()
            .iter()
            .find_map(|upstream| upstream.serving_registry(id))
    }

    /// The versions of `id` held by this registry itself, oldest first.
    pub(crate) fn own_versions(&self, id: &str) -> Vec<VersionInfo> {
        let state = self.state();
        let Some(plugin) = state.plugins.get(id) else {
            return Vec::new();
        };
        let mut versions: Vec<VersionInfo> = plugin
            .versions
            .iter()
            .map(|(version, record)| version_info(version, record))
            .collect();
        versions.sort_by(|left, right| left.parsed().cmp(&right.parsed()));
        versions
    }

    /// Counts one request received by this registry.
    pub(crate) fn note_request(&self) {
        self.state().requests += 1;
    }

    /// Every plugin id of this registry with its highest live version (for search).
    pub(crate) fn search_ids(&self, query: &str) -> Vec<(String, String)> {
        let state = self.state();
        state
            .plugins
            .iter()
            .filter(|(id, _)| id.contains(query))
            .map(|(id, plugin)| {
                let latest = plugin
                    .versions
                    .iter()
                    .filter(|(_, record)| !record.taken_down)
                    .filter_map(|(version, _)| Version::parse(version).ok())
                    .max()
                    .map(|version| version.to_string())
                    .unwrap_or_default();
                (id.clone(), latest)
            })
            .collect()
    }

    /// The package bytes of `id@version`; counts one download.
    pub(crate) fn download(&self, id: &str, version: &str) -> Option<Vec<u8>> {
        let mut state = self.state();
        let sha256 = state.plugins.get(id)?.versions.get(version)?.sha256.clone();
        let bytes = state.blobs.get(&sha256)?.clone();
        state.downloads += 1;
        Some(bytes)
    }

    fn validate_range_list(requires: &[(String, String, bool)]) -> Result<(), PluginError> {
        for (id, range, _) in requires {
            if VersionReq::parse(range).is_err() {
                return Err(refuse(
                    "registry.dependency-unknown",
                    ErrorCode::InvalidArgument,
                    format!("the range `{range}` required for `{id}` is not valid"),
                ));
            }
        }
        Ok(())
    }

    /// Publishes one version (PKG-810, PKG-811, PKG-812, PKG-822).
    pub fn publish(&mut self, request: PublishRequest) -> Result<PublishReceipt, PluginError> {
        // PKG-811 order: size, namespace ownership, version-exists, version-order, dependencies.
        if request.package.len() as u64 > MAX_PUBLISH_BYTES {
            return Err(refuse(
                "registry.size",
                ErrorCode::QuotaExceeded,
                "the package is larger than 300 MiB",
            ));
        }
        let version = Version::parse(&request.version).map_err(|error| {
            refuse(
                "registry.version-invalid",
                ErrorCode::InvalidArgument,
                format!("`{}` is not a version: {error}", request.version),
            )
        })?;
        Self::validate_range_list(&request.requires_plugins)?;
        let channel = request.channel.clone().unwrap_or_else(|| "stable".to_owned());
        {
            let state = self.state();
            if let Some(plugin) = state.plugins.get(&request.id) {
                if !plugin
                    .owners
                    .iter()
                    .any(|owner| owner.keys.contains(&request.signer_key))
                {
                    return Err(refuse(
                        "registry.not-owner",
                        ErrorCode::NotPermitted,
                        format!("the key {} does not own `{}`", request.signer_key, request.id),
                    ));
                }
                let tombstoned = plugin
                    .tombstones
                    .iter()
                    .any(|tombstone| tombstone.version == request.version);
                if plugin.versions.contains_key(&request.version) || tombstoned {
                    return Err(refuse(
                        "registry.version-exists",
                        ErrorCode::Conflict,
                        format!("`{}@{}` already exists", request.id, request.version),
                    ));
                }
                if channel == "stable" {
                    let blocked = plugin
                        .versions
                        .iter()
                        .filter(|(_, record)| record.channel == "stable")
                        .filter_map(|(existing, _)| Version::parse(existing).ok())
                        .any(|existing| existing.major == version.major && existing > version);
                    if blocked {
                        return Err(refuse(
                            "registry.version-order",
                            ErrorCode::Conflict,
                            format!(
                                "`{}` is lower than a stable version of the same major line",
                                request.version
                            ),
                        ));
                    }
                }
            }
        }
        for (required_id, range, optional) in &request.requires_plugins {
            if *optional {
                continue;
            }
            let known = self.serving_registry(required_id).is_some_and(|registry| {
                registry
                    .own_versions(required_id)
                    .iter()
                    .any(|info| !info.taken_down && info.satisfies(range))
            });
            if !known {
                return Err(refuse(
                    "registry.dependency-unknown",
                    ErrorCode::NotFound,
                    format!("required plugin `{required_id}` {range} is not served"),
                ));
            }
        }
        let sha256 = sha256_hex(&request.package);
        let length = request.package.len() as u64;
        let published_at = utc_timestamp();
        let mut state = self.state();
        let plugin = state.plugins.entry(request.id.clone()).or_default();
        if plugin.owners.is_empty() {
            plugin.owners.push(Owner {
                publisher: request.signer_key.clone(),
                keys: vec![request.signer_key.clone()],
            });
        }
        plugin.versions.insert(
            request.version.clone(),
            VersionRecord {
                sha256: sha256.clone(),
                length,
                channel: channel.clone(),
                published_at: published_at.clone(),
                requires: request.requires_plugins.clone(),
                yank: None,
                taken_down: false,
            },
        );
        state.blobs.insert(sha256.clone(), request.package);
        state.record_write("publish", &request.id, &request.version);
        Ok(PublishReceipt {
            url: format!("{}/blobs/{sha256}", self.base_url),
            id: request.id,
            version: request.version,
            sha256,
            length,
            channel,
            published_at,
        })
    }

    /// Publishes from a body of `announced_length` bytes; over 300 MiB is refused before reading (PKG-810).
    ///
    /// The body is a JSON publish envelope: `{"id", "version", "signer_key", "channel"?,
    /// "package"?, "requires_plugins"?: [[id, range, optional]]}`.
    pub fn publish_body(
        &mut self,
        announced_length: u64,
        body: &mut dyn Read,
    ) -> Result<PublishReceipt, PluginError> {
        if announced_length > MAX_PUBLISH_BYTES {
            return Err(refuse(
                "registry.size",
                ErrorCode::QuotaExceeded,
                format!("the announced body of {announced_length} bytes is over 300 MiB"),
            ));
        }
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut last_progress = Instant::now();
        while (bytes.len() as u64) < announced_length {
            match body.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    last_progress = Instant::now();
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    if last_progress.elapsed() >= self.upload_idle_timeout {
                        return Err(refuse(
                            "registry.upload-timeout",
                            ErrorCode::TimedOut,
                            "the upload made no progress and was aborted",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    return Err(refuse(
                        "registry.upload-timeout",
                        ErrorCode::Io,
                        format!("the upload failed: {error}"),
                    ))
                }
            }
        }
        if bytes.len() as u64 > MAX_PUBLISH_BYTES {
            return Err(refuse(
                "registry.size",
                ErrorCode::QuotaExceeded,
                "the body is over 300 MiB",
            ));
        }
        let envelope: Value = serde_json::from_slice(&bytes).map_err(|error| {
            refuse(
                "registry.version-invalid",
                ErrorCode::InvalidArgument,
                format!("the publish body is not a JSON envelope: {error}"),
            )
        })?;
        let text = |key: &str| envelope.get(key).and_then(Value::as_str).unwrap_or_default();
        let mut request = PublishRequest::new(text("id"), text("version"), text("signer_key"))
            .with_package(text("package").as_bytes().to_vec());
        if let Some(channel) = envelope.get("channel").and_then(Value::as_str) {
            request = request.with_channel(channel);
        }
        if let Some(requires) = envelope.get("requires_plugins").and_then(Value::as_array) {
            for entry in requires {
                if let Some(parts) = entry.as_array() {
                    request = request.with_requires_plugin(
                        parts.first().and_then(Value::as_str).unwrap_or_default(),
                        parts.get(1).and_then(Value::as_str).unwrap_or("*"),
                        parts.get(2).and_then(Value::as_bool).unwrap_or(false),
                    );
                }
            }
        }
        self.publish(request)
    }

    /// How long an upload may stall before it is aborted (PKG-810).
    pub fn upload_idle_timeout(&self) -> Duration {
        self.upload_idle_timeout
    }

    /// Sets the upload idle timeout (PKG-810).
    pub fn with_upload_idle_timeout(mut self, timeout: Duration) -> Registry {
        self.upload_idle_timeout = timeout;
        self
    }

    fn require_owner(
        plugin: &PluginRecord,
        id: &str,
        signer_key: &str,
    ) -> Result<(), PluginError> {
        if plugin
            .owners
            .iter()
            .any(|owner| owner.keys.iter().any(|key| key == signer_key))
        {
            Ok(())
        } else {
            Err(refuse(
                "registry.not-owner",
                ErrorCode::NotPermitted,
                format!("the key {signer_key} does not own `{id}`"),
            ))
        }
    }

    /// Yanks a version (PKG-800, PKG-801).
    pub fn yank(&mut self, request: YankRequest) -> Result<YankReceipt, PluginError> {
        let mut state = self.state();
        let plugin = state.plugins.get_mut(&request.id).ok_or_else(|| {
            refuse(
                "registry.not-found",
                ErrorCode::NotFound,
                format!("no plugin `{}`", request.id),
            )
        })?;
        Self::require_owner(plugin, &request.id, &request.signer_key)?;
        let reason = request.reason.clone().unwrap_or_default();
        let reason_chars = reason.chars().count();
        if reason_chars == 0 || reason_chars > MAX_YANK_REASON_CHARS {
            return Err(refuse(
                "usage.invalid-value",
                ErrorCode::InvalidArgument,
                "a yank reason holds 1 to 500 characters",
            ));
        }
        let record = plugin.versions.get_mut(&request.version).ok_or_else(|| {
            refuse(
                "registry.not-found",
                ErrorCode::NotFound,
                format!("no version `{}@{}`", request.id, request.version),
            )
        })?;
        if record.taken_down {
            return Err(refuse(
                "registry.taken-down",
                ErrorCode::NotFound,
                "the version was taken down",
            ));
        }
        let yanked_at = utc_timestamp();
        record.yank = Some(YankRecord {
            at: yanked_at.clone(),
            by: request.signer_key.clone(),
            reason: reason.clone(),
        });
        state.record_write("yank", &request.id, &format!("{}: {reason}", request.version));
        Ok(YankReceipt {
            id: request.id,
            version: request.version,
            yanked_at,
        })
    }

    /// Clears the yank of a version (PKG-802).
    pub fn unyank(&mut self, request: YankRequest) -> Result<(), PluginError> {
        let mut state = self.state();
        let plugin = state.plugins.get_mut(&request.id).ok_or_else(|| {
            refuse(
                "registry.not-found",
                ErrorCode::NotFound,
                format!("no plugin `{}`", request.id),
            )
        })?;
        Self::require_owner(plugin, &request.id, &request.signer_key)?;
        let record = plugin.versions.get_mut(&request.version).ok_or_else(|| {
            refuse(
                "registry.not-found",
                ErrorCode::NotFound,
                format!("no version `{}@{}`", request.id, request.version),
            )
        })?;
        if record.taken_down {
            return Err(refuse(
                "registry.taken-down",
                ErrorCode::NotFound,
                "a taken-down version cannot be unyanked",
            ));
        }
        if record.yank.take().is_none() {
            return Err(refuse(
                "registry.not-yanked",
                ErrorCode::Conflict,
                format!("`{}@{}` is not yanked", request.id, request.version),
            ));
        }
        state.record_write("unyank", &request.id, &request.version);
        Ok(())
    }

    /// Takes a version down and leaves a tombstone (PKG-821).
    pub fn takedown(
        &mut self,
        id: &str,
        version: &str,
        reason_code: &str,
    ) -> Result<(), PluginError> {
        let mut state = self.state();
        let plugin = state.plugins.get_mut(id).ok_or_else(|| {
            refuse("registry.not-found", ErrorCode::NotFound, format!("no plugin `{id}`"))
        })?;
        let record = plugin.versions.get_mut(version).ok_or_else(|| {
            refuse(
                "registry.not-found",
                ErrorCode::NotFound,
                format!("no version `{id}@{version}`"),
            )
        })?;
        record.taken_down = true;
        let sha256 = record.sha256.clone();
        plugin.tombstones.push(Tombstone {
            version: version.to_owned(),
            sha256,
            reason_code: reason_code.to_owned(),
            time: utc_timestamp(),
        });
        state.record_write("takedown", id, &format!("{version}: {reason_code}"));
        Ok(())
    }

    /// The metadata document of a plugin as JSON text, `None` for an unknown id (PKG-801, PKG-821).
    pub fn metadata_document(&self, id: &str) -> Option<String> {
        let state = self.state();
        let plugin = state.plugins.get(id)?;
        let mut versions = serde_json::Map::new();
        for (version, record) in plugin.versions.iter().filter(|(_, record)| !record.taken_down) {
            let mut entry = json!({
                "sha256": record.sha256,
                "length": record.length,
                "channel": record.channel,
                "published_at": record.published_at,
                "requires": record
                    .requires
                    .iter()
                    .map(|(required, range, optional)| {
                        json!({ "id": required, "range": range, "optional": optional })
                    })
                    .collect::<Vec<_>>(),
            });
            if let (Some(yank), Some(object)) = (&record.yank, entry.as_object_mut()) {
                object.insert("yanked_at".into(), json!(yank.at));
                object.insert("yanked_by".into(), json!(yank.by));
                object.insert("reason".into(), json!(yank.reason));
            }
            versions.insert(version.clone(), entry);
        }
        let tombstones: Vec<Value> = plugin
            .tombstones
            .iter()
            .map(|tombstone| {
                json!({
                    "id": id,
                    "version": tombstone.version,
                    "sha256": tombstone.sha256,
                    "reason_code": tombstone.reason_code,
                    "time": tombstone.time,
                })
            })
            .collect();
        let owners: Vec<&str> = plugin.owners.iter().map(|owner| owner.publisher.as_str()).collect();
        Some(
            json!({
                "id": id,
                "owners": owners,
                "versions": versions,
                "tombstones": tombstones,
            })
            .to_string(),
        )
    }

    /// How long after a successful publish the release first appears in the signed index (PKG-809: 60 s).
    pub fn index_delay_of(&self, id: &str, version: &str) -> Duration {
        let state = self.state();
        if state
            .plugins
            .get(id)
            .is_some_and(|plugin| plugin.versions.contains_key(version))
        {
            INDEX_DELAY
        } else {
            Duration::ZERO
        }
    }

    /// The transparency log, oldest entry first (PKG-802, PKG-819).
    pub fn transparency_log(&self) -> Vec<LogEntry> {
        self.state().log.clone()
    }

    /// Adds an owner, signed by an existing owner (PKG-819).
    pub fn add_owner(
        &mut self,
        id: &str,
        publisher_id: &str,
        signer_key: &str,
    ) -> Result<(), PluginError> {
        let mut state = self.state();
        let plugin = state.plugins.get_mut(id).ok_or_else(|| {
            refuse("registry.not-found", ErrorCode::NotFound, format!("no plugin `{id}`"))
        })?;
        Self::require_owner(plugin, id, signer_key)?;
        if !plugin.owners.iter().any(|owner| owner.publisher == publisher_id) {
            plugin.owners.push(Owner {
                publisher: publisher_id.to_owned(),
                keys: Vec::new(),
            });
        }
        state.record_write("owner-add", id, publisher_id);
        Ok(())
    }

    /// Removes an owner; the last owner stays (PKG-819).
    pub fn remove_owner(
        &mut self,
        id: &str,
        publisher_id: &str,
        signer_key: &str,
    ) -> Result<(), PluginError> {
        let mut state = self.state();
        let plugin = state.plugins.get_mut(id).ok_or_else(|| {
            refuse("registry.not-found", ErrorCode::NotFound, format!("no plugin `{id}`"))
        })?;
        Self::require_owner(plugin, id, signer_key)?;
        if plugin.owners.len() <= 1 {
            return Err(refuse(
                "registry.last-owner",
                ErrorCode::Conflict,
                format!("`{publisher_id}` is the last owner of `{id}`"),
            ));
        }
        plugin.owners.retain(|owner| owner.publisher != publisher_id);
        state.record_write("owner-remove", id, publisher_id);
        Ok(())
    }

    /// How many package blobs are stored (PKG-811).
    pub fn blob_count(&self) -> usize {
        self.state().blobs.len()
    }

    /// The revision of the metadata, raised by every write (PKG-811).
    pub fn metadata_revision(&self) -> u64 {
        self.state().revision
    }

    /// The lower-case hex SHA-256 of a published package.
    pub fn sha256_of(&self, id: &str, version: &str) -> String {
        self.state()
            .plugins
            .get(id)
            .and_then(|plugin| plugin.versions.get(version))
            .map(|record| record.sha256.clone())
            .unwrap_or_default()
    }

    /// How many requests the registry has received (PKG-824).
    pub fn requests_received(&self) -> usize {
        self.state().requests
    }

    /// How many package downloads have started (PKG-804, PKG-828).
    pub fn downloads_started(&self) -> usize {
        self.state().downloads
    }

    /// Serves requirements that this registry does not hold from `upstream` (PKG-812).
    pub fn with_upstream(self, upstream: Registry) -> Registry {
        self.state().upstreams.push(upstream);
        self
    }

    /// Drops every configured upstream (PKG-817).
    pub fn without_upstreams(self) -> Registry {
        self.state().upstreams.clear();
        self
    }

    /// Answers a GET for `path`, for example `/plugins/<id>.json` or `/blobs/<sha256>` (PKG-818).
    pub fn serve(&self, path: &str) -> HttpResponse {
        self.note_request();
        if let Some(name) = path.strip_prefix("/plugins/") {
            let id = name.strip_suffix(".json").unwrap_or(name);
            return match self.metadata_document(id) {
                Some(document) => {
                    HttpResponse::new(200, "application/json", None, document.into_bytes())
                }
                None => HttpResponse::new(404, "application/json", None, b"{}".to_vec()),
            };
        }
        if let Some(name) = path.strip_prefix("/blobs/") {
            let sha256 = name.strip_suffix(".iliumpkg").unwrap_or(name);
            let state = self.state();
            return match state.blobs.get(sha256) {
                Some(bytes) => {
                    HttpResponse::new(200, "application/octet-stream", None, bytes.clone())
                }
                None => HttpResponse::new(404, "application/octet-stream", None, Vec::new()),
            };
        }
        HttpResponse::new(404, "application/json", None, b"{}".to_vec())
    }

    /// The yank reason of `id@version`, if it is yanked.
    pub(crate) fn yank_reason(&self, id: &str, version: &str) -> Option<String> {
        self.own_versions(id)
            .into_iter()
            .find(|info| info.version == version)
            .and_then(|info| info.yank_reason)
    }

    /// How many other plugins' latest release depends on a range that only `id@version` satisfies (PKG-823).
    pub(crate) fn dependents_of(&self, id: &str, version: &str) -> usize {
        let versions = self.own_versions(id);
        let state = self.state();
        state
            .plugins
            .iter()
            .filter(|(other, _)| other.as_str() != id)
            .filter_map(|(_, plugin)| {
                plugin
                    .versions
                    .iter()
                    .filter(|(_, record)| !record.taken_down)
                    .filter_map(|(name, record)| Some((Version::parse(name).ok()?, record)))
                    .max_by(|left, right| left.0.cmp(&right.0))
                    .map(|(_, record)| record.requires.clone())
            })
            .filter(|requires| {
                requires.iter().any(|(required, range, optional)| {
                    required == id && !optional && {
                        let satisfying: Vec<&VersionInfo> = versions
                            .iter()
                            .filter(|info| info.version != version)
                            .filter(|info| !info.taken_down && info.yank_reason.is_none())
                            .filter(|info| info.satisfies(range))
                            .collect();
                        let target_matches = versions
                            .iter()
                            .any(|info| info.version == version && info.satisfies(range));
                        target_matches && satisfying.is_empty()
                    }
                })
            })
            .count()
    }
}
