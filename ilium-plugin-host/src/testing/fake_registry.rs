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
