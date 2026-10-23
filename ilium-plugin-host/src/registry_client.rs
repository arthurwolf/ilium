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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ilium_plugin_api::codes::registry as registry_codes;

    use super::{FetchError, FetchResponse, Fetcher, IndexEntry, RegistryClient};

    /// The registry base URL used by every test (reference-registry.md, File layout).
    const HTTPS_BASE_URL: &str = "https://plugins.example.org/ilium/v1/";
    /// An insecure base URL (PKG-371 refuses it).
    const HTTP_BASE_URL: &str = "http://plugins.example.org/ilium/v1/";
    /// SHA-256 of the bytes `abc`, the name of its blob (PKG-363, File layout `blobs/<sha256>.iliumpkg`).
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    /// SHA-256 of zero bytes.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    /// The revocation document cap (PKG-369): 4 MiB.
    const REVOCATIONS_CAP_BYTES: usize = 4 * 1024 * 1024;
    /// The timestamp document cap (PKG-369): 16 KiB.
    const TIMESTAMP_CAP_BYTES: usize = 16 * 1024;

    type Reply = Box<dyn Fn(&str) -> Result<Vec<u8>, FetchError> + Send + Sync>;

    /// A fake fetcher that answers each URL with a scripted reply and records every request.
    struct ScriptedFetcher {
        reply: Reply,
        requested_urls: Mutex<Vec<String>>,
    }

    impl ScriptedFetcher {
        fn new(
            reply: impl Fn(&str) -> Result<Vec<u8>, FetchError> + Send + Sync + 'static,
        ) -> ScriptedFetcher {
            ScriptedFetcher {
                reply: Box::new(reply),
                requested_urls: Mutex::new(Vec::new()),
            }
        }

        fn record_and_reply(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.requested_urls
                .lock()
                .expect("request log lock")
                .push(url.to_string());
            (self.reply)(url)
        }

        fn requests(&self) -> Vec<String> {
            self.requested_urls
                .lock()
                .expect("request log lock")
                .clone()
        }
    }

    impl Fetcher for ScriptedFetcher {
        fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.record_and_reply(url)
        }

        fn get_response(&self, url: &str) -> Result<FetchResponse, FetchError> {
            let body = self.record_and_reply(url)?;
            Ok(FetchResponse {
                status: 200,
                location: None,
                content_type: None,
                content_length: Some(body.len() as u64),
                body,
            })
        }
    }

    /// An index entry whose blob URL is `url`, relative to the registry base.
    fn entry_with_url(url: &str) -> IndexEntry {
        IndexEntry {
            id: "com.example.notes".to_string(),
            version: "1.2.0".to_string(),
            url: url.to_string(),
        }
    }

    /// A metadata document with no signatures; the signature check must refuse it.
    fn unsigned_timestamp_document() -> Vec<u8> {
        br#"{"signed":{"type":"timestamp","version":4821,"expires":"2027-10-21 12:00:00","snapshot":{"version":4821,"sha256":"0000000000000000000000000000000000000000000000000000000000000000","length":18233}},"signatures":[]}"#.to_vec()
    }

    /// An unsigned revocations document (the wrapper of PKG-653 with an empty signature list).
    fn unsigned_revocations_document() -> Vec<u8> {
        br#"{"signed":{"type":"revocations","version":17,"expires":"2027-10-21 12:00:00","keys":[],"blocked_ids":[]},"signatures":[]}"#.to_vec()
    }

    /// Pads a JSON document with trailing spaces to exactly `length` bytes (JSON allows trailing whitespace).
    fn pad_to_length(mut document: Vec<u8>, length: usize) -> Vec<u8> {
        assert!(
            document.len() <= length,
            "document longer than the requested length"
        );
        document.resize(length, b' ');
        document
    }

    /// The code of a failed registry call, as its namespaced text.
    fn error_code_text(error: &crate::error::HostError) -> String {
        error.code().as_str().to_string()
    }

    /// REQ: PKG-363
    #[test]
    fn download_returns_bytes_whose_sha256_matches_blob_name() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(b"abc".to_vec()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let bytes = client.download(&entry).expect("matching digest downloads");

        assert_eq!(bytes, b"abc".to_vec());
    }

    /// REQ: PKG-361
    #[test]
    fn download_requests_blob_relative_to_base_url() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(b"abc".to_vec()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        client.download(&entry).expect("matching digest downloads");

        assert_eq!(
            fetcher.requests(),
            vec![format!("{HTTPS_BASE_URL}blobs/{ABC_SHA256}.iliumpkg")]
        );
    }

    /// REQ: PKG-363
    #[test]
    fn download_refuses_bytes_whose_sha256_differs_from_blob_name() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(b"abd".to_vec()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let error = client
            .download(&entry)
            .expect_err("one changed byte must be refused");

        assert_eq!(error_code_text(&error), registry_codes::HASH_MISMATCH);
    }

    /// REQ: PKG-418
    #[test]
    fn download_hash_mismatch_is_not_retried_by_the_client() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(b"abd".to_vec()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let error = client.download(&entry).expect_err("mismatch must fail");

        assert_eq!(error_code_text(&error), registry_codes::HASH_MISMATCH);
        assert_eq!(
            fetcher.requests().len(),
            1,
            "a hash mismatch is not a transport retry"
        );
    }

    /// REQ: PKG-363
    #[test]
    fn download_accepts_empty_package_with_empty_digest() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(Vec::new()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{EMPTY_SHA256}.iliumpkg"));

        let bytes = client
            .download(&entry)
            .expect("empty body matches the empty digest");

        assert!(bytes.is_empty());
    }

    /// REQ: PKG-363
    #[test]
    fn download_refuses_empty_body_when_digest_names_other_bytes() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(Vec::new()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let error = client
            .download(&entry)
            .expect_err("empty body must not match a non-empty digest");

        assert_eq!(error_code_text(&error), registry_codes::HASH_MISMATCH);
    }

    /// REQ: PKG-415
    #[test]
    fn download_reports_unreachable_when_network_fails() {
        let fetcher = ScriptedFetcher::new(|_url| Err(FetchError::Unreachable));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let error = client.download(&entry).expect_err("no network must fail");

        assert_eq!(error_code_text(&error), registry_codes::UNREACHABLE);
    }

    /// REQ: PKG-373
    #[test]
    fn download_missing_blob_is_not_found_and_not_retried() {
        let fetcher = ScriptedFetcher::new(|_url| Err(FetchError::Status(404)));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let error = client
            .download(&entry)
            .expect_err("a missing blob must fail");

        assert_eq!(error_code_text(&error), registry_codes::NOT_FOUND);
        assert_eq!(fetcher.requests().len(), 1, "a 4xx response is not retried");
    }

    /// REQ: PKG-371
    #[test]
    fn download_refuses_insecure_base_url_without_any_request() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(b"abc".to_vec()));
        let client = RegistryClient::new(&fetcher, HTTP_BASE_URL.to_string());
        let entry = entry_with_url(&format!("blobs/{ABC_SHA256}.iliumpkg"));

        let error = client
            .download(&entry)
            .expect_err("plain http must be refused");

        assert_eq!(error_code_text(&error), registry_codes::INSECURE_URL);
        assert!(
            fetcher.requests().is_empty(),
            "a refused URL is never requested"
        );
    }

    /// REQ: PKG-371
    #[test]
    fn index_refuses_insecure_base_url_without_any_request() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(unsigned_timestamp_document()));
        let client = RegistryClient::new(&fetcher, HTTP_BASE_URL.to_string());

        let error = client.index().expect_err("plain http must be refused");

        assert_eq!(error_code_text(&error), registry_codes::INSECURE_URL);
        assert!(
            fetcher.requests().is_empty(),
            "a refused URL is never requested"
        );
    }

    /// REQ: PKG-415
    #[test]
    fn index_reports_unreachable_when_network_fails() {
        let fetcher = ScriptedFetcher::new(|_url| Err(FetchError::Unreachable));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client.index().expect_err("no network must fail");

        assert_eq!(error_code_text(&error), registry_codes::UNREACHABLE);
    }

    /// REQ: PKG-362
    #[test]
    fn index_refuses_unsigned_timestamp_document() {
        let fetcher = ScriptedFetcher::new(|url| {
            if url.ends_with("timestamp.json") {
                Ok(unsigned_timestamp_document())
            } else {
                Err(FetchError::Status(404))
            }
        });
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client
            .index()
            .expect_err("an unsigned timestamp must not be used");

        assert_eq!(error_code_text(&error), registry_codes::SIGNATURE_INVALID);
    }

    /// REQ: PKG-369
    #[test]
    fn index_refuses_timestamp_one_byte_over_cap_with_size_code() {
        let oversized = pad_to_length(unsigned_timestamp_document(), TIMESTAMP_CAP_BYTES + 1);
        let fetcher = ScriptedFetcher::new(move |url| {
            if url.ends_with("timestamp.json") {
                Ok(oversized.clone())
            } else {
                Err(FetchError::Status(404))
            }
        });
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client
            .index()
            .expect_err("a 16 KiB + 1 byte timestamp is refused");

        assert_eq!(error_code_text(&error), registry_codes::SIZE);
    }

    /// REQ: PKG-369
    #[test]
    fn index_timestamp_exactly_at_cap_passes_size_check() {
        let at_cap = pad_to_length(unsigned_timestamp_document(), TIMESTAMP_CAP_BYTES);
        let fetcher = ScriptedFetcher::new(move |url| {
            if url.ends_with("timestamp.json") {
                Ok(at_cap.clone())
            } else {
                Err(FetchError::Status(404))
            }
        });
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client
            .index()
            .expect_err("the unsigned document still fails");

        assert_eq!(error_code_text(&error), registry_codes::SIGNATURE_INVALID);
    }

    /// REQ: PKG-362
    #[test]
    fn index_refuses_empty_timestamp_body() {
        let fetcher = ScriptedFetcher::new(|url| {
            if url.ends_with("timestamp.json") {
                Ok(Vec::new())
            } else {
                Err(FetchError::Status(404))
            }
        });
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let result = client.index();

        assert!(
            result.is_err(),
            "an empty document cannot be verified and must not yield an index"
        );
    }

    /// REQ: PKG-653
    #[test]
    fn revocations_refuse_document_without_signature() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(unsigned_revocations_document()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client
            .revocations()
            .expect_err("an unsigned revocation document is refused");

        assert_eq!(error_code_text(&error), registry_codes::SIGNATURE_INVALID);
    }

    /// REQ: PKG-369
    #[test]
    fn revocations_refuse_document_one_byte_over_cap_with_size_code() {
        let oversized = pad_to_length(unsigned_revocations_document(), REVOCATIONS_CAP_BYTES + 1);
        let fetcher = ScriptedFetcher::new(move |_url| Ok(oversized.clone()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client
            .revocations()
            .expect_err("a 4 MiB + 1 byte document is refused");

        assert_eq!(error_code_text(&error), registry_codes::SIZE);
    }

    /// REQ: PKG-369
    #[test]
    fn revocations_exactly_at_cap_pass_size_check() {
        let at_cap = pad_to_length(unsigned_revocations_document(), REVOCATIONS_CAP_BYTES);
        let fetcher = ScriptedFetcher::new(move |_url| Ok(at_cap.clone()));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client
            .revocations()
            .expect_err("the unsigned document still fails");

        assert_eq!(error_code_text(&error), registry_codes::SIGNATURE_INVALID);
    }

    /// REQ: PKG-415
    #[test]
    fn revocations_report_unreachable_when_network_fails() {
        let fetcher = ScriptedFetcher::new(|_url| Err(FetchError::Unreachable));
        let client = RegistryClient::new(&fetcher, HTTPS_BASE_URL.to_string());

        let error = client.revocations().expect_err("no network must fail");

        assert_eq!(error_code_text(&error), registry_codes::UNREACHABLE);
    }

    /// REQ: PKG-371
    #[test]
    fn revocations_refuse_insecure_base_url_without_any_request() {
        let fetcher = ScriptedFetcher::new(|_url| Ok(unsigned_revocations_document()));
        let client = RegistryClient::new(&fetcher, HTTP_BASE_URL.to_string());

        let error = client
            .revocations()
            .expect_err("plain http must be refused");

        assert_eq!(error_code_text(&error), registry_codes::INSECURE_URL);
        assert!(
            fetcher.requests().is_empty(),
            "a refused URL is never requested"
        );
    }
}
