//! reference-dependency-resolution.md: locked yanked versions (PKG-803), graph limits before any
//! download (PKG-804) and lock file validation (PKG-828).

use ilium_plugin_api::client::{Client, InstallOptions};
use ilium_plugin_api::lock;
use ilium_plugin_api::registry::{Registry, YankRequest};
use ilium_plugin_api::resolve::Resolver;

use crate::support::*;

const REGISTRY_URL: &str = "https://registry.example";

/// Publishes a plugin with one exact dependency range on each of the given ids.
fn publish_chain_link(registry: &mut Registry, id: &str, requires: &[&str]) {
    let requirements: Vec<(&str, &str)> = requires
        .iter()
        .map(|required| (*required, "^1.0.0"))
        .collect();
    publish_requiring(registry, id, "1.0.0", &requirements);
}

// ---------------------------------------------------------------------------------------------
// PKG-803: yanked versions in locks
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-803
#[test]
fn pkg_803_locked_yanked_version_installs_by_exact_sha256_with_a_warning_record() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let sha256 = registry.sha256_of("demo.plugin", "1.0.0");
    registry
        .yank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_reason("broken"))
        .expect("the owner may yank its version");

    let text = lock_text(&[("demo.plugin", "1.0.0", sha256.as_str())]);
    let mut client = Client::new(vec![registry.clone()]);
    let report = client
        .install_locked_text(&text)
        .expect("a locked yanked version still installs by its exact sha256");

    assert_eq!(
        report.installed_sha256("demo.plugin"),
        Some(sha256.as_str())
    );
    assert!(
        report
            .records()
            .iter()
            .any(|record| record.kind() == "warning" && record.code() == "registry.yanked-locked"),
        "the client prints a warning record with code registry.yanked-locked"
    );
}

/// REQ: PKG-803
#[test]
fn pkg_803_lock_regeneration_never_chooses_a_yanked_version() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    publish_owned(&mut registry, "demo.plugin", "1.1.0");
    registry
        .yank(YankRequest::new("demo.plugin", "1.1.0", OWNER_KEY).with_reason("broken"))
        .expect("the owner may yank its version");
    let regenerated = lock::regenerate(&registry, &["demo.plugin@^1.0.0"])
        .expect("regeneration succeeds with a live 1.0.0");
    assert_eq!(
        regenerated.version_of("demo.plugin").as_deref(),
        Some("1.0.0"),
        "the yanked 1.1.0 is not chosen when the lock is regenerated"
    );
}

// ---------------------------------------------------------------------------------------------
// PKG-804: graph limits before download
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-804
#[test]
fn pkg_804_graph_with_more_than_256_distinct_plugins_is_refused_before_any_download() {
    let mut registry = Registry::at(REGISTRY_URL);
    // One root, 16 middle plugins and 16 leaves under each middle plugin: 1 + 16 + 256 = 273 ids.
    let mut middle_ids = Vec::new();
    for middle in 0..16 {
        let mut leaf_ids = Vec::new();
        for leaf in 0..16 {
            let leaf_id = format!("wide.leaf{middle:02}{leaf:02}");
            publish_owned(&mut registry, &leaf_id, "1.0.0");
            leaf_ids.push(leaf_id);
        }
        let middle_id = format!("wide.mid{middle:02}");
        let leaf_refs: Vec<&str> = leaf_ids.iter().map(String::as_str).collect();
        publish_chain_link(&mut registry, &middle_id, &leaf_refs);
        middle_ids.push(middle_id);
    }
    let middle_refs: Vec<&str> = middle_ids.iter().map(String::as_str).collect();
    publish_chain_link(&mut registry, "wide.root", &middle_refs);

    let error = refusal(
        Resolver::new(vec![registry.clone()]).resolve("wide.root", "^1.0.0"),
        "a graph with 273 distinct plugins must be refused",
    );
    assert_eq!(code_of(&error), "resolve.graph-too-large");
    assert_eq!(
        registry.downloads_started(),
        0,
        "no package is downloaded before the refusal"
    );
}

/// REQ: PKG-804
#[test]
fn pkg_804_dependency_chain_deeper_than_32_is_refused_before_any_download() {
    let mut registry = Registry::at(REGISTRY_URL);
    let chain_length = 40;
    publish_owned(
        &mut registry,
        &format!("chain.p{:02}", chain_length - 1),
        "1.0.0",
    );
    for index in (0..chain_length - 1).rev() {
        let next = format!("chain.p{:02}", index + 1);
        publish_chain_link(
            &mut registry,
            &format!("chain.p{index:02}"),
            &[next.as_str()],
        );
    }

    let error = refusal(
        Resolver::new(vec![registry.clone()]).resolve("chain.p00", "^1.0.0"),
        "a chain 40 plugins deep must be refused",
    );
    assert_eq!(code_of(&error), "resolve.graph-too-large");
    assert_eq!(
        registry.downloads_started(),
        0,
        "no package is downloaded before the refusal"
    );
}

/// REQ: PKG-804
#[test]
fn pkg_804_small_graph_within_the_limits_resolves() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "small.leaf", "1.0.0");
    publish_chain_link(&mut registry, "small.root", &["small.leaf"]);
    let resolution = Resolver::new(vec![registry.clone()])
        .resolve("small.root", "^1.0.0")
        .expect("a two-plugin graph is within the limits");
    assert_eq!(resolution.plugin_ids().len(), 2);
}

// ---------------------------------------------------------------------------------------------
// PKG-828: lock file validation
// ---------------------------------------------------------------------------------------------

/// A registry that serves `demo.plugin@1.0.0`, used to prove that a refused lock downloads nothing.
fn registry_with_demo() -> (Registry, String) {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let sha256 = registry.sha256_of("demo.plugin", "1.0.0");
    (registry, sha256)
}

/// REQ: PKG-828
#[test]
fn pkg_828_valid_lock_text_installs() {
    let (registry, sha256) = registry_with_demo();
    let text = lock_text(&[("demo.plugin", "1.0.0", sha256.as_str())]);
    let mut client = Client::new(vec![registry.clone()]);
    client
        .install_locked_text(&text)
        .expect("a well-formed lock with a served version installs");
    assert_eq!(
        registry.downloads_started(),
        1,
        "the locked package is downloaded once"
    );
}

/// REQ: PKG-828
#[test]
fn pkg_828_invalid_locks_are_refused_with_lock_invalid_and_no_download() {
    let (registry, sha256) = registry_with_demo();
    let upper_hex = sha256.to_uppercase();
    let short_hex = &sha256[..63];
    let cases: Vec<(&str, String)> = vec![
        (
            "unknown format",
            serde_json::json!({ "format": 2, "plugins": [] }).to_string(),
        ),
        (
            "duplicate id",
            lock_text(&[
                ("demo.plugin", "1.0.0", sha256.as_str()),
                ("demo.plugin", "1.0.0", sha256.as_str()),
            ]),
        ),
        (
            "unsorted entries",
            lock_text(&[
                ("demo.zeta", "1.0.0", sha256.as_str()),
                ("demo.plugin", "1.0.0", sha256.as_str()),
            ]),
        ),
        (
            "upper-case sha256",
            lock_text(&[("demo.plugin", "1.0.0", upper_hex.as_str())]),
        ),
        (
            "sha256 of 63 characters",
            lock_text(&[("demo.plugin", "1.0.0", short_hex)]),
        ),
    ];
    for (label, text) in cases {
        let mut client = Client::new(vec![registry.clone()]);
        let error = refusal(
            client.install_locked_text(&text),
            &format!("a lock with {label} must be refused"),
        );
        assert_eq!(
            code_of(&error),
            "lock.invalid",
            "wrong code for a lock with {label}"
        );
        assert_eq!(
            client.downloads_started(),
            0,
            "no package is downloaded for a lock with {label}"
        );
    }
    Client::new(vec![registry.clone()])
        .install("demo.plugin", InstallOptions::new())
        .expect(
            "the same registry still installs by name, so the refusals above are about the lock",
        );
}
