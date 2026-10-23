//! A model of a plugin registry: publish, yank, takedown, owners, metadata and the transparency
//! log (reference-registry.md, PKG-800 to PKG-824). Clones of a [`Registry`] share one state.

use std::io::Read;
use std::time::Duration;

use crate::error::PluginError;
use crate::http::HttpResponse;

/// A registry reached at a URL.
#[derive(Debug, Clone)]
pub struct Registry {
    /// The base URL of the registry.
    pub base_url: String,
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

impl PublishRequest {
    /// A request for `id@version` signed by `signer_key`.
    pub fn new(_id: &str, _version: &str, _signer_key: &str) -> PublishRequest {
        unimplemented!("api::registry::PublishRequest::new")
    }

    /// Sets the package bytes.
    pub fn with_package(self, _bytes: Vec<u8>) -> PublishRequest {
        unimplemented!("api::registry::PublishRequest::with_package")
    }

    /// Adds a required plugin with its version range.
    pub fn with_requires_plugin(self, _id: &str, _range: &str, _optional: bool) -> PublishRequest {
        unimplemented!("api::registry::PublishRequest::with_requires_plugin")
    }

    /// Sets the channel.
    pub fn with_channel(self, _channel: &str) -> PublishRequest {
        unimplemented!("api::registry::PublishRequest::with_channel")
    }
}

/// What a successful publish returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReceipt;

impl PublishReceipt {
    /// The 201 body of a successful publish, as JSON text (PKG-809: `id`, `version`, `sha256`,
    /// `length`, `url`, `channel`, `published_at`).
    pub fn response_body(&self) -> String {
        unimplemented!("api::registry::PublishReceipt::response_body")
    }
}

/// A request to yank or unyank one version (PKG-800, PKG-801, PKG-802).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YankRequest {
    /// Plugin id.
    pub id: String,
    /// Plugin version.
    pub version: String,
    /// Fingerprint of the signing key.
    pub signer_key: String,
    /// The reason, 1 to 500 characters for a yank.
    pub reason: Option<String>,
}

impl YankRequest {
    /// A request for `id@version` signed by `signer_key`.
    pub fn new(_id: &str, _version: &str, _signer_key: &str) -> YankRequest {
        unimplemented!("api::registry::YankRequest::new")
    }

    /// Sets the reason.
    pub fn with_reason(self, _reason: &str) -> YankRequest {
        unimplemented!("api::registry::YankRequest::with_reason")
    }
}

/// What a successful yank returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YankReceipt;

/// One entry of the transparency log (PKG-802, PKG-819).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry;

impl LogEntry {
    /// The action recorded, for example `unyank` or `owner-add`.
    pub fn action(&self) -> &str {
        unimplemented!("api::registry::LogEntry::action")
    }
}

impl Registry {
    /// A registry served at `url`.
    pub fn at(_url: &str) -> Registry {
        unimplemented!("api::registry::Registry::at")
    }

    /// The base URL.
    pub fn url(&self) -> &str {
        &self.base_url
    }

    /// Publishes one version (PKG-810, PKG-811, PKG-812, PKG-822).
    pub fn publish(&mut self, _request: PublishRequest) -> Result<PublishReceipt, PluginError> {
        unimplemented!("api::registry::Registry::publish")
    }

    /// Publishes from a body of `announced_length` bytes; over 300 MiB is refused before reading (PKG-810).
    pub fn publish_body(
        &mut self,
        _announced_length: u64,
        _body: &mut dyn Read,
    ) -> Result<PublishReceipt, PluginError> {
        unimplemented!("api::registry::Registry::publish_body")
    }

    /// How long an upload may stall before it is aborted (PKG-810).
    pub fn upload_idle_timeout(&self) -> Duration {
        unimplemented!("api::registry::Registry::upload_idle_timeout")
    }

    /// Sets the upload idle timeout (PKG-810).
    pub fn with_upload_idle_timeout(self, _timeout: Duration) -> Registry {
        unimplemented!("api::registry::Registry::with_upload_idle_timeout")
    }

    /// Yanks a version (PKG-800, PKG-801).
    pub fn yank(&mut self, _request: YankRequest) -> Result<YankReceipt, PluginError> {
        unimplemented!("api::registry::Registry::yank")
    }

    /// Clears the yank of a version (PKG-802).
    pub fn unyank(&mut self, _request: YankRequest) -> Result<(), PluginError> {
        unimplemented!("api::registry::Registry::unyank")
    }

    /// Takes a version down and leaves a tombstone (PKG-821).
    pub fn takedown(
        &mut self,
        _id: &str,
        _version: &str,
        _reason_code: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("api::registry::Registry::takedown")
    }

    /// The metadata document of a plugin as JSON text, `None` for an unknown id (PKG-801, PKG-821).
    pub fn metadata_document(&self, _id: &str) -> Option<String> {
        unimplemented!("api::registry::Registry::metadata_document")
    }

    /// How long after a successful publish the release first appears in the signed index (PKG-809: 60 s).
    pub fn index_delay_of(&self, _id: &str, _version: &str) -> Duration {
        unimplemented!("api::registry::Registry::index_delay_of")
    }

    /// The transparency log, oldest entry first (PKG-802, PKG-819).
    pub fn transparency_log(&self) -> Vec<LogEntry> {
        unimplemented!("api::registry::Registry::transparency_log")
    }

    /// Adds an owner, signed by an existing owner (PKG-819).
    pub fn add_owner(
        &mut self,
        _id: &str,
        _publisher_id: &str,
        _signer_key: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("api::registry::Registry::add_owner")
    }

    /// Removes an owner; the last owner stays (PKG-819).
    pub fn remove_owner(
        &mut self,
        _id: &str,
        _publisher_id: &str,
        _signer_key: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("api::registry::Registry::remove_owner")
    }

    /// How many package blobs are stored (PKG-811).
    pub fn blob_count(&self) -> usize {
        unimplemented!("api::registry::Registry::blob_count")
    }

    /// The revision of the metadata, raised by every write (PKG-811).
    pub fn metadata_revision(&self) -> u64 {
        unimplemented!("api::registry::Registry::metadata_revision")
    }

    /// The lower-case hex SHA-256 of a published package.
    pub fn sha256_of(&self, _id: &str, _version: &str) -> String {
        unimplemented!("api::registry::Registry::sha256_of")
    }

    /// How many requests the registry has received (PKG-824).
    pub fn requests_received(&self) -> usize {
        unimplemented!("api::registry::Registry::requests_received")
    }

    /// How many package downloads have started (PKG-804, PKG-828).
    pub fn downloads_started(&self) -> usize {
        unimplemented!("api::registry::Registry::downloads_started")
    }

    /// Serves requirements that this registry does not hold from `upstream` (PKG-812).
    pub fn with_upstream(self, _upstream: Registry) -> Registry {
        unimplemented!("api::registry::Registry::with_upstream")
    }

    /// Drops every configured upstream (PKG-817).
    pub fn without_upstreams(self) -> Registry {
        unimplemented!("api::registry::Registry::without_upstreams")
    }

    /// Answers a GET for `path`, for example `/plugins/<id>.json` or `/blobs/<sha256>` (PKG-818).
    pub fn serve(&self, _path: &str) -> HttpResponse {
        unimplemented!("api::registry::Registry::serve")
    }
}
