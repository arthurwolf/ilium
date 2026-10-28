//! A model of the install client: sources, routes, locks, rollback checks and metadata decoding
//! (reference-registry.md, reference-dependency-resolution.md; PKG-803, PKG-813 to PKG-818, PKG-828).

use std::collections::BTreeMap;

use crate::error::{ErrorCode, PluginError};
use crate::http::{gzip_decode, HttpResponse};
use crate::lock;
use crate::registry::{sha256_hex, Registry, VersionInfo};
use crate::resolve::pick_version;

/// Largest decoded metadata document in bytes (PKG-818).
const METADATA_SIZE_LIMIT: usize = 1024 * 1024;

/// The install client.
#[derive(Debug, Clone)]
pub struct Client {
    /// The registries searched when no route matches.
    pub sources: Vec<Registry>,
    routes: Vec<(String, Registry)>,
    installed_sources: BTreeMap<String, String>,
    snapshot_version: Option<u64>,
    timestamp_version: Option<u64>,
    downloads: usize,
}

/// Options of one install (PKG-813).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallOptions {
    /// Install exactly this version.
    pub exact_version: Option<String>,
    /// Allow a plugin to come from a different registry than before.
    pub allow_source_change: bool,
}

impl InstallOptions {
    /// Options with nothing set.
    pub fn new() -> InstallOptions {
        InstallOptions::default()
    }

    /// Install exactly `version`.
    pub fn exact_version(mut self, version: &str) -> InstallOptions {
        self.exact_version = Some(version.to_owned());
        self
    }

    /// Allow or refuse a change of source.
    pub fn allow_source_change(mut self, allowed: bool) -> InstallOptions {
        self.allow_source_change = allowed;
        self
    }
}

/// A plugin the client installed (PKG-813, PKG-814).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    id: String,
    version: String,
    source_url: String,
    sha256: String,
}

impl InstalledPlugin {
    /// The URL of the registry the plugin came from.
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// The installed plugin id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The installed version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The SHA-256 of the installed package.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// One line the client prints while it installs (PKG-803).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    kind: String,
    code: String,
    message: String,
}

impl Record {
    /// The kind of the record, for example `warning`.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The code of the record, for example `registry.yanked-locked`.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The human-readable text of the record.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// What installing from a lock did (PKG-803, PKG-828).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    installed: BTreeMap<String, String>,
    records: Vec<Record>,
}

impl InstallReport {
    /// The SHA-256 that was installed for `id`.
    pub fn installed_sha256(&self, id: &str) -> Option<&str> {
        self.installed.get(id).map(String::as_str)
    }

    /// The records printed while installing.
    pub fn records(&self) -> &[Record] {
        &self.records
    }
}

impl Client {
    /// A client that searches `sources` in order.
    pub fn new(sources: Vec<Registry>) -> Client {
        Client {
            sources,
            routes: Vec::new(),
            installed_sources: BTreeMap::new(),
            snapshot_version: None,
            timestamp_version: None,
            downloads: 0,
        }
    }

    /// Sends every id that starts with `prefix` to `registry` (PKG-814).
    pub fn with_route(mut self, prefix: &str, registry: Registry) -> Client {
        self.routes.push((prefix.to_owned(), registry));
        self
    }

    /// Replaces the configured sources (PKG-813).
    pub fn set_sources(&mut self, sources: Vec<Registry>) {
        self.sources = sources;
    }

    /// The registry a request for `id` goes to: its route only, or the first source that serves it.
    fn source_for(&self, id: &str) -> Result<&Registry, PluginError> {
        let not_found = || {
            PluginError::coded(
                "plugin.not-found",
                ErrorCode::NotFound,
                format!("no registry serves `{id}`"),
            )
        };
        if let Some((_, route)) = self
            .routes
            .iter()
            .filter(|(prefix, _)| id.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
        {
            return if route.serves(id) { Ok(route) } else { Err(not_found()) };
        }
        self.sources
            .iter()
            .find(|source| source.serves(id))
            .ok_or_else(not_found)
    }

    /// Installs the plugin `id` (PKG-801, PKG-813, PKG-814).
    pub fn install(
        &mut self,
        id: &str,
        options: InstallOptions,
    ) -> Result<InstalledPlugin, PluginError> {
        let source = self.source_for(id)?.clone();
        source.note_request();
        if let Some(previous) = self.installed_sources.get(id) {
            if previous != source.url() && !options.allow_source_change {
                return Err(PluginError::coded(
                    "registry.source-changed",
                    ErrorCode::NotPermitted,
                    format!(
                        "`{id}` was installed from {previous}; pass --allow-source-change to use {}",
                        source.url()
                    ),
                ));
            }
        }
        let holder = source
            .serving_registry(id)
            .ok_or_else(|| PluginError::coded("plugin.not-found", ErrorCode::NotFound, id))?;
        let info: VersionInfo = match &options.exact_version {
            Some(version) => {
                let found = holder
                    .own_versions(id)
                    .into_iter()
                    .find(|info| &info.version == version && !info.taken_down)
                    .ok_or_else(|| {
                        PluginError::coded(
                            "plugin.not-found",
                            ErrorCode::NotFound,
                            format!("`{id}@{version}` does not exist"),
                        )
                    })?;
                if let Some(reason) = &found.yank_reason {
                    return Err(PluginError::coded(
                        "registry.yanked",
                        ErrorCode::NotPermitted,
                        format!("`{id}@{version}` was yanked: {reason}"),
                    ));
                }
                found
            }
            None => pick_version(&holder, id, "*").ok_or_else(|| {
                PluginError::coded(
                    "plugin.not-found",
                    ErrorCode::NotFound,
                    format!("`{id}` has no live version"),
                )
            })?,
        };
        let bytes = holder.download(id, &info.version).ok_or_else(|| {
            PluginError::coded("plugin.not-found", ErrorCode::NotFound, "the package is missing")
        })?;
        self.downloads += 1;
        if sha256_hex(&bytes) != info.sha256 {
            return Err(PluginError::coded(
                "registry.hash-mismatch",
                ErrorCode::Conflict,
                format!("the package of `{id}` does not match its sha256"),
            ));
        }
        self.installed_sources
            .insert(id.to_owned(), source.url().to_owned());
        Ok(InstalledPlugin {
            id: id.to_owned(),
            version: info.version,
            source_url: source.url().to_owned(),
            sha256: info.sha256,
        })
    }

    /// Installs what a lock file text names (PKG-803, PKG-828).
    pub fn install_locked_text(&mut self, text: &str) -> Result<InstallReport, PluginError> {
        // Validate the whole lock first so that a refused lock downloads nothing (PKG-828).
        let lock = lock::parse(text)?;
        let mut installed = BTreeMap::new();
        let mut records = Vec::new();
        for (id, version, sha256) in lock.entries() {
            let source = self.source_for(id)?.clone();
            source.note_request();
            let holder = source
                .serving_registry(id)
                .ok_or_else(|| PluginError::coded("plugin.not-found", ErrorCode::NotFound, id))?;
            let info = holder
                .own_versions(id)
                .into_iter()
                .find(|info| &info.version == version && !info.taken_down)
                .ok_or_else(|| {
                    PluginError::coded(
                        "plugin.not-found",
                        ErrorCode::NotFound,
                        format!("`{id}@{version}` is not served"),
                    )
                })?;
            if &info.sha256 != sha256 {
                return Err(PluginError::coded(
                    "registry.hash-mismatch",
                    ErrorCode::Conflict,
                    format!("the registry's sha256 of `{id}@{version}` differs from the lock"),
                ));
            }
            if let Some(reason) = &info.yank_reason {
                records.push(Record {
                    kind: "warning".to_owned(),
                    code: "registry.yanked-locked".to_owned(),
                    message: format!("`{id}@{version}` is yanked ({reason}) but locked by sha256"),
                });
            }
            let bytes = holder.download(id, version).ok_or_else(|| {
                PluginError::coded("plugin.not-found", ErrorCode::NotFound, "package missing")
            })?;
            self.downloads += 1;
            if &sha256_hex(&bytes) != sha256 {
                return Err(PluginError::coded(
                    "registry.hash-mismatch",
                    ErrorCode::Conflict,
                    format!("the downloaded package of `{id}` does not match the lock"),
                ));
            }
            self.installed_sources
                .insert(id.clone(), source.url().to_owned());
            installed.insert(id.clone(), sha256.clone());
        }
        Ok(InstallReport { installed, records })
    }

    fn version_of_document(text: &str) -> Result<u64, PluginError> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|error| {
            PluginError::coded(
                "registry.metadata-invalid",
                ErrorCode::InvalidArgument,
                format!("not JSON: {error}"),
            )
        })?;
        value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                PluginError::coded(
                    "registry.metadata-invalid",
                    ErrorCode::InvalidArgument,
                    "the document has no numeric `version`",
                )
            })
    }

    fn rollback(kind: &str, stored: u64, offered: u64) -> PluginError {
        PluginError::coded(
            "registry.rollback",
            ErrorCode::Conflict,
            format!("the {kind} version {offered} is older than the stored {stored}"),
        )
    }

    /// Accepts a snapshot document; an older version is `registry.rollback` (PKG-816).
    pub fn accept_snapshot(&mut self, text: &str) -> Result<(), PluginError> {
        let version = Self::version_of_document(text)?;
        if let Some(stored) = self.snapshot_version {
            if version < stored {
                return Err(Self::rollback("snapshot", stored, version));
            }
        }
        self.snapshot_version = Some(version);
        Ok(())
    }

    /// Accepts a timestamp document; an older version is `registry.rollback` (PKG-816).
    pub fn accept_timestamp(&mut self, text: &str) -> Result<(), PluginError> {
        let version = Self::version_of_document(text)?;
        if let Some(stored) = self.timestamp_version {
            if version < stored {
                return Err(Self::rollback("timestamp", stored, version));
            }
        }
        self.timestamp_version = Some(version);
        Ok(())
    }

    /// The version of the stored snapshot (PKG-816).
    pub fn stored_snapshot_version(&self) -> Option<u64> {
        self.snapshot_version
    }

    /// The version of the stored timestamp (PKG-816).
    pub fn stored_timestamp_version(&self) -> Option<u64> {
        self.timestamp_version
    }

    /// Decodes a metadata answer; the content type is ignored and gzip is accepted (PKG-818).
    pub fn decode_metadata(
        &self,
        response: &HttpResponse,
    ) -> Result<serde_json::Value, PluginError> {
        let too_large = || {
            PluginError::coded(
                "registry.size",
                ErrorCode::QuotaExceeded,
                format!("metadata over {METADATA_SIZE_LIMIT} decoded bytes is refused"),
            )
        };
        let decoded: Vec<u8> = match response.content_encoding() {
            Some(encoding) if encoding.eq_ignore_ascii_case("gzip") => {
                gzip_decode(response.body(), METADATA_SIZE_LIMIT).ok_or_else(too_large)?
            }
            _ => {
                if response.body().len() > METADATA_SIZE_LIMIT {
                    return Err(too_large());
                }
                response.body().to_vec()
            }
        };
        serde_json::from_slice(&decoded).map_err(|error| {
            PluginError::coded(
                "registry.metadata-invalid",
                ErrorCode::InvalidArgument,
                format!("metadata is not JSON: {error}"),
            )
        })
    }

    /// The largest decoded metadata document in bytes (PKG-818).
    pub fn metadata_size_limit(&self) -> usize {
        METADATA_SIZE_LIMIT
    }

    /// How many package downloads the client has started (PKG-828).
    pub fn downloads_started(&self) -> usize {
        self.downloads
    }
}
