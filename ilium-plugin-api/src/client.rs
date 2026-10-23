//! A model of the install client: sources, routes, locks, rollback checks and metadata decoding
//! (reference-registry.md, reference-dependency-resolution.md; PKG-803, PKG-813 to PKG-818, PKG-828).

use crate::error::PluginError;
use crate::http::HttpResponse;
use crate::registry::Registry;

/// The install client.
#[derive(Debug, Clone)]
pub struct Client {
    /// The registries searched when no route matches.
    pub sources: Vec<Registry>,
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
        unimplemented!("api::client::InstallOptions::new")
    }

    /// Install exactly `version`.
    pub fn exact_version(self, _version: &str) -> InstallOptions {
        unimplemented!("api::client::InstallOptions::exact_version")
    }

    /// Allow or refuse a change of source.
    pub fn allow_source_change(self, _allowed: bool) -> InstallOptions {
        unimplemented!("api::client::InstallOptions::allow_source_change")
    }
}

/// A plugin the client installed (PKG-813, PKG-814).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin;

impl InstalledPlugin {
    /// The URL of the registry the plugin came from.
    pub fn source_url(&self) -> &str {
        unimplemented!("api::client::InstalledPlugin::source_url")
    }
}

/// One line the client prints while it installs (PKG-803).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record;

impl Record {
    /// The kind of the record, for example `warning`.
    pub fn kind(&self) -> &str {
        unimplemented!("api::client::Record::kind")
    }

    /// The code of the record, for example `registry.yanked-locked`.
    pub fn code(&self) -> &str {
        unimplemented!("api::client::Record::code")
    }
}

/// What installing from a lock did (PKG-803, PKG-828).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport;

impl InstallReport {
    /// The SHA-256 that was installed for `id`.
    pub fn installed_sha256(&self, _id: &str) -> Option<&str> {
        unimplemented!("api::client::InstallReport::installed_sha256")
    }

    /// The records printed while installing.
    pub fn records(&self) -> &[Record] {
        unimplemented!("api::client::InstallReport::records")
    }
}

impl Client {
    /// A client that searches `sources` in order.
    pub fn new(_sources: Vec<Registry>) -> Client {
        unimplemented!("api::client::Client::new")
    }

    /// Sends every id that starts with `prefix` to `registry` (PKG-814).
    pub fn with_route(self, _prefix: &str, _registry: Registry) -> Client {
        unimplemented!("api::client::Client::with_route")
    }

    /// Replaces the configured sources (PKG-813).
    pub fn set_sources(&mut self, _sources: Vec<Registry>) {
        unimplemented!("api::client::Client::set_sources")
    }

    /// Installs the plugin `id` (PKG-801, PKG-813, PKG-814).
    pub fn install(
        &mut self,
        _id: &str,
        _options: InstallOptions,
    ) -> Result<InstalledPlugin, PluginError> {
        unimplemented!("api::client::Client::install")
    }

    /// Installs what a lock file text names (PKG-803, PKG-828).
    pub fn install_locked_text(&mut self, _text: &str) -> Result<InstallReport, PluginError> {
        unimplemented!("api::client::Client::install_locked_text")
    }

    /// Accepts a snapshot document; an older version is `registry.rollback` (PKG-816).
    pub fn accept_snapshot(&mut self, _text: &str) -> Result<(), PluginError> {
        unimplemented!("api::client::Client::accept_snapshot")
    }

    /// Accepts a timestamp document; an older version is `registry.rollback` (PKG-816).
    pub fn accept_timestamp(&mut self, _text: &str) -> Result<(), PluginError> {
        unimplemented!("api::client::Client::accept_timestamp")
    }

    /// The version of the stored snapshot (PKG-816).
    pub fn stored_snapshot_version(&self) -> Option<u64> {
        unimplemented!("api::client::Client::stored_snapshot_version")
    }

    /// The version of the stored timestamp (PKG-816).
    pub fn stored_timestamp_version(&self) -> Option<u64> {
        unimplemented!("api::client::Client::stored_timestamp_version")
    }

    /// Decodes a metadata answer; the content type is ignored and gzip is accepted (PKG-818).
    pub fn decode_metadata(
        &self,
        _response: &HttpResponse,
    ) -> Result<serde_json::Value, PluginError> {
        unimplemented!("api::client::Client::decode_metadata")
    }

    /// The largest decoded metadata document in bytes (PKG-818).
    pub fn metadata_size_limit(&self) -> usize {
        unimplemented!("api::client::Client::metadata_size_limit")
    }

    /// How many package downloads the client has started (PKG-828).
    pub fn downloads_started(&self) -> usize {
        unimplemented!("api::client::Client::downloads_started")
    }
}
