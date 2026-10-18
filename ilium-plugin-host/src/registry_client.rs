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
    fn get_response(&self, _url: &str) -> Result<FetchResponse, FetchError> {
        unimplemented!("host::registry_client::Fetcher::get_response")
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

/// Talks to a registry and its mirrors.
pub struct RegistryClient;

impl RegistryClient {
    /// A client for a registry base URL.
    pub fn new(_fetcher: &dyn Fetcher, _base_url: String) -> RegistryClient {
        RegistryClient
    }

    /// Fetches and verifies the index.
    pub fn index(&self) -> Result<Vec<IndexEntry>, HostError> {
        unimplemented!("host::registry_client::RegistryClient::index")
    }

    /// Fetches and verifies the revocation document.
    pub fn revocations(&self) -> Result<Revocations, HostError> {
        unimplemented!("host::registry_client::RegistryClient::revocations")
    }

    /// Downloads a package, checking its recorded digest.
    pub fn download(&self, _entry: &IndexEntry) -> Result<Vec<u8>, HostError> {
        unimplemented!("host::registry_client::RegistryClient::download")
    }
}
