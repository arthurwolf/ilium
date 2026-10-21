//! Fake registry: serves index and package documents of published packages to a fake fetcher
//! (PKG-263, PKG-313). Test support only.

use ilium_plugin_api::Diagnostic;

/// A registry held in memory (PKG-263, PKG-313).
#[derive(Debug)]
pub struct FakeRegistry {
    _private: (),
}

impl FakeRegistry {
    /// An empty registry at a base URL.
    pub fn new(_base_url: &str) -> FakeRegistry {
        unimplemented!("host::testing::fake_registry::FakeRegistry::new")
    }

    /// Publishes a package on a channel; panics when the package is refused.
    pub fn publish(&mut self, _package: Vec<u8>, _channel: &str) {
        unimplemented!("host::testing::fake_registry::FakeRegistry::publish")
    }

    /// Publishes a package on a channel.
    pub fn try_publish(&mut self, _package: Vec<u8>, _channel: &str) -> Result<(), Diagnostic> {
        unimplemented!("host::testing::fake_registry::FakeRegistry::try_publish")
    }

    /// Every served document as (url, body).
    pub fn documents(&self) -> Vec<(String, Vec<u8>)> {
        unimplemented!("host::testing::fake_registry::FakeRegistry::documents")
    }
}

#[cfg(test)]
mod tests {
    use super::FakeRegistry;
    use ilium_plugin_api::Diagnostic;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    const BASE_URL: &str = "https://registry.example.org/";
    const EXAMPLE_ID: &str = "example.registry";
    /// The 34-byte component of the minimal package vector (reference-package-format.md).
    const COMPONENT_HEX: &str =
        "0061736d0d000100001811696c69756d3a6170692d76657273696f6e000000010000";

    fn component_bytes() -> Vec<u8> {
        (0..COMPONENT_HEX.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&COMPONENT_HEX[index..index + 2], 16)
                    .expect("COMPONENT_HEX holds only hex digits")
            })
            .collect()
    }

    /// A minimal package of `EXAMPLE_ID` at `version`, written with the crate's package writer.
    fn package_bytes(version: &str) -> Vec<u8> {
        let manifest = format!(
            "{{\n  \"api\": \"ilium:plugin@0.1.0\",\n  \"id\": \"{EXAMPLE_ID}\",\n  \"name\": \"Example\",\n  \"version\": \"{version}\",\n  \"publisher\": \"Example Labs\",\n  \"server\": {{\"entry\": \"server.wasm\", \"world\": \"ilium:server/base@0.1.0\", \"exports\": []}}\n}}\n"
        );
        let files = vec![
            ("manifest.json".to_owned(), manifest.into_bytes()),
            ("server.wasm".to_owned(), component_bytes()),
        ];
        crate::package::write_package(&files)
            .unwrap_or_else(|diagnostic| panic!("test package failed: {diagnostic:?}"))
    }

    fn any_body_mentions(registry: &FakeRegistry, needle: &str) -> bool {
        registry
            .documents()
            .iter()
            .any(|(_, body)| String::from_utf8_lossy(body).contains(needle))
    }

    #[test]
    fn new_registry_serves_no_documents() {
        let registry = FakeRegistry::new(BASE_URL);
        assert!(registry.documents().is_empty());
    }

    #[test]
    fn every_served_document_url_starts_with_the_base_url() {
        let mut registry = FakeRegistry::new(BASE_URL);
        registry.publish(package_bytes("1.0.0"), "stable");
        let documents = registry.documents();
        assert!(!documents.is_empty());
        for (url, _) in &documents {
            assert!(url.starts_with(BASE_URL), "{url} is outside {BASE_URL}");
        }
    }

    #[test]
    fn publish_of_a_valid_release_serves_its_version() {
        let mut registry = FakeRegistry::new(BASE_URL);
        registry.publish(package_bytes("1.0.0"), "stable");
        assert!(any_body_mentions(&registry, "1.0.0"));
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_prerelease_is_accepted_on_beta() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let result: Result<(), Diagnostic> =
            registry.try_publish(package_bytes("1.0.0-beta.1"), "beta");
        assert!(result.is_ok(), "a prerelease belongs to beta: {result:?}");
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_release_candidate_is_accepted_on_beta() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let result = registry.try_publish(package_bytes("1.0.0-rc.1"), "beta");
        assert!(result.is_ok(), "a prerelease belongs to beta: {result:?}");
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_prerelease_is_refused_on_stable_and_leaves_no_document() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let refusal = registry
            .try_publish(package_bytes("1.0.0-beta.1"), "stable")
            .expect_err("a prerelease cannot go to stable");
        assert!(!refusal.code.as_str().is_empty());
        assert!(!any_body_mentions(&registry, "1.0.0-beta.1"));
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_release_may_be_published_to_stable_or_beta() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry
            .try_publish(package_bytes("1.0.0"), "stable")
            .is_ok());
        assert!(registry.try_publish(package_bytes("1.1.0"), "beta").is_ok());
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_dev_is_refused_as_a_registry_channel() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry.try_publish(package_bytes("1.0.0"), "dev").is_err());
    }

    /// Boundary: an empty channel name names no channel and is refused.
    #[test]
    fn empty_channel_name_is_refused() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry.try_publish(package_bytes("1.0.0"), "").is_err());
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_a_version_is_indexed_under_exactly_one_channel() {
        let mut registry = FakeRegistry::new(BASE_URL);
        registry.publish(package_bytes("1.0.0"), "stable");
        registry.publish(package_bytes("1.1.0-beta.1"), "beta");
        let text: String = registry
            .documents()
            .iter()
            .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
            .collect();
        let stable_mentions = text.matches("\"channel\": \"stable\"").count()
            + text.matches("\"channel\":\"stable\"").count();
        assert_eq!(stable_mentions, 1, "{text}");
    }

    /// REQ: PKG-353
    #[test]
    fn pkg_353_publish_panics_when_the_package_is_refused() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            registry.publish(package_bytes("1.0.0-beta.1"), "stable");
        }));
        assert!(outcome.is_err(), "publish must panic on a refused package");
    }

    /// REQ: PKG-313
    #[test]
    fn pkg_313_build_metadata_is_refused_with_registry_version_invalid() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let refusal = registry
            .try_publish(package_bytes("1.0.0+build.5"), "stable")
            .expect_err("a registry refuses build metadata");
        assert_eq!(refusal.code.as_str(), "registry.version-invalid");
    }

    /// REQ: PKG-313
    #[test]
    fn pkg_313_refused_build_metadata_leaves_no_document() {
        let mut registry = FakeRegistry::new(BASE_URL);
        let _ = registry.try_publish(package_bytes("1.0.0-beta.1+build.5"), "beta");
        assert!(!any_body_mentions(&registry, "build.5"));
    }

    /// Boundary: an empty package file is refused rather than served.
    #[test]
    fn empty_package_bytes_are_refused() {
        let mut registry = FakeRegistry::new(BASE_URL);
        assert!(registry.try_publish(Vec::new(), "stable").is_err());
    }
}
