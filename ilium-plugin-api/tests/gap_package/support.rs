//! Shared fixtures of the package-gap lane: registry publishing, lock text, source trees and the
//! manifest diagnostics. The registry, client, lock, mirror and package types are the contract the
//! tests describe; the manual pages are the spec (each test names its PKG ID).

use std::path::PathBuf;

use ilium_plugin_api::error::PluginError;
use ilium_plugin_api::manifest::{parse_manifest, Diagnostic};
use ilium_plugin_api::registry::{PublishRequest, Registry};
use serde_json::{json, Value};

/// Fingerprint of the signing key that owns the fixture plugins.
pub const OWNER_KEY: &str = "SHA256:owner-fixture-key";
/// Publisher id that the owner key belongs to.
pub const OWNER_PUBLISHER: &str = "fixture.publisher";
/// Fingerprint of a key that owns nothing; every request it signs must be refused as not-owner.
pub const STRANGER_KEY: &str = "SHA256:stranger-fixture-key";

/// Package bytes used by every fixture publish.
pub fn sample_package() -> Vec<u8> {
    b"ilium-package-fixture".to_vec()
}

/// Publishes `id@version` signed by the owner key with the sample package. The fixture must succeed.
pub fn publish_owned(registry: &mut Registry, id: &str, version: &str) {
    registry
        .publish(PublishRequest::new(id, version, OWNER_KEY).with_package(sample_package()))
        .expect("fixture publish by the owner key succeeds");
}

/// Publishes `id@version` by the owner key with one required plugin range. The fixture must succeed.
pub fn publish_requiring(
    registry: &mut Registry,
    id: &str,
    version: &str,
    requires: &[(&str, &str)],
) {
    let mut request = PublishRequest::new(id, version, OWNER_KEY).with_package(sample_package());
    for (required_id, range) in requires {
        request = request.with_requires_plugin(required_id, range, false);
    }
    registry
        .publish(request)
        .expect("fixture publish with satisfiable requirements succeeds");
}

/// The refusal of a request the test expects to fail. Panics with `what` when it succeeds.
pub fn refusal<T: std::fmt::Debug>(result: Result<T, PluginError>, what: &str) -> PluginError {
    result.expect_err(what)
}

/// The namespaced code of a refusal, for example `registry.not-owner`.
pub fn code_of(error: &PluginError) -> &'static str {
    error.code.as_str()
}

/// A lock file text with the given `(id, version, sha256)` entries, in the order given.
pub fn lock_text(entries: &[(&str, &str, &str)]) -> String {
    let plugins: Vec<Value> = entries
        .iter()
        .map(|(id, version, sha256)| json!({ "id": id, "version": version, "sha256": sha256 }))
        .collect();
    json!({ "format": 1, "plugins": plugins }).to_string()
}

/// Base source manifest for the manifest-limit tests.
pub fn base_manifest() -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.demo",
        "name": "Example demo",
        "version": "0.1.0",
        "publisher": "Example Publisher",
        "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" }
    })
}

/// Every error diagnostic `parse_manifest` reports for a manifest value. Empty when it parses.
pub fn manifest_findings(value: &Value) -> Vec<Diagnostic> {
    parse_manifest(&value.to_string()).err().unwrap_or_default()
}

/// A scratch source tree under the cargo test temp dir with a fixed set of files.
pub fn source_tree(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("create the fixture source tree");
    std::fs::write(root.join("plugin.json"), base_manifest().to_string())
        .expect("write the fixture manifest");
    std::fs::write(root.join("src").join("client.wasm"), b"\0asm fixture")
        .expect("write the fixture component");
    root
}
