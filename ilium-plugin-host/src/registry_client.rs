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

/// Fetches a URL (injected; tests use a fake).
pub trait Fetcher: Send + Sync {
    /// Gets the body of a URL.
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError>;
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
pub struct RegistryClient<'fetcher> {
    fetcher: &'fetcher dyn Fetcher,
    base_url: String,
}

impl<'fetcher> RegistryClient<'fetcher> {
    /// A client for a registry base URL.
    pub fn new(fetcher: &'fetcher dyn Fetcher, base_url: String) -> RegistryClient<'fetcher> {
        RegistryClient { fetcher, base_url }
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
    pub fn download(&self, entry: &IndexEntry) -> Result<Vec<u8>, HostError> {
        unimplemented!("host::registry_client::RegistryClient::download")
    }
}
