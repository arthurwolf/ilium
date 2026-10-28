//! reference-registry.md and guide-publishing.md: what review level B requires (PKG-680..685).
//! Lane `registry-publish`. Red phase: the level computation and the publish gate do not exist yet.

use ilium_plugin_host::testing::FakeRegistry;

use crate::common::*;
use crate::pages_p2::has_error;

const PLUGIN: &str = "com.acme.strict";
const REGISTRY_BASE: &str = "https://strict.example.org/ilium/v1";
/// Seed of the metadata key of `FakeRegistry` (its documents are signed with 32 bytes of 0x52).
const FAKE_REGISTRY_SEED: u8 = 0x52;

/// A signed package of `PLUGIN` whose manifest carries `license` and whose archive holds `extra`.
fn package_with(version: &str, license: Option<&str>, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut manifest = manifest_text(PLUGIN, version);
    if let Some(license) = license {
        manifest = manifest.replace(
            "\"publisher\": \"Acme Labs\",",
            &format!("\"publisher\": \"Acme Labs\",\n  \"license\": \"{license}\","),
        );
    }
    let tail = "\n  ]\n}\n";
    let cut = manifest.rfind(tail).expect("files array tail of the fixture manifest");
    let mut inventory = String::new();
    for (path, bytes) in extra {
        inventory.push_str(&format!(
            ",\n    {{\n      \"path\": \"{path}\",\n      \"length\": {},\n      \"sha256\": \"{}\"\n    }}",
            bytes.len(),
            sha256_hex(bytes)
        ));
    }
    manifest.insert_str(cut, &inventory);
    let manifest = manifest.into_bytes();
    let signature = signature_entry(&manifest, PUBLISHER_SEED, PUBLISHER_ID).into_bytes();
    let mut files = vec![
        ("manifest.json".to_owned(), manifest),
        ("server.wasm".to_owned(), unhex(crate::common::COMPONENT_HEX)),
        ("signature".to_owned(), signature),
    ];
    for (path, bytes) in extra {
        files.push(((*path).to_owned(), bytes.to_vec()));
    }
    ilium_plugin_host::package::write_package(&files).expect("write package")
}

/// A package that meets every level B input.
fn complete_package(version: &str) -> Vec<u8> {
    package_with(version, Some("MIT OR Apache-2.0"), &[("sbom.json", b"{}"), ("LICENSE", b"MIT text")])
}

/// The `review_level` of `version` in the plugin document the registry would serve.
fn served_level(registry: &FakeRegistry, version: &str) -> String {
    let (_, bytes) = registry
        .documents()
        .into_iter()
        .find(|(url, _)| url.ends_with(&format!("plugins/{PLUGIN}.json")))
        .expect("plugin document");
    signed_of(&bytes)["versions"]
        .as_array()
        .expect("versions")
        .iter()
        .find(|row| row["version"] == version)
        .and_then(|row| row["review_level"].as_str())
        .expect("review_level of the version")
        .to_owned()
}

fn refusal_detail(registry: &mut FakeRegistry, package: Vec<u8>) -> Option<String> {
    match registry.try_publish(package, "stable") {
        Ok(()) => None,
        Err(diagnostic) => {
            assert_eq!(diagnostic.code.as_str(), "registry.policy");
            Some(diagnostic.message)
        }
    }
}

/// REQ: PKG-680
#[test]
fn level_b_requires_review_record_sbom_spdx_license_and_notice() {
    let mut registry = FakeRegistry::new(REGISTRY_BASE);
    registry.publish(complete_package("1.0.0"), "stable");
    assert_eq!(served_level(&registry, "1.0.0"), "A", "no review record yet");
    registry.record_review(PLUGIN, "1.0.0").expect("review");
    assert_eq!(served_level(&registry, "1.0.0"), "B");
    // Each of (b), (c), (d) missing keeps a reviewed version at level A.
    let variants: [(&str, Vec<u8>); 3] = [
        ("2.0.0", package_with("2.0.0", Some("MIT"), &[("LICENSE", b"text")])),
        ("3.0.0", package_with("3.0.0", None, &[("sbom.json", b"{}"), ("LICENSE", b"text")])),
        ("4.0.0", package_with("4.0.0", Some("MIT"), &[("sbom.json", b"{}")])),
    ];
    for (version, package) in variants {
        registry.publish(package, "stable");
        registry.record_review(PLUGIN, version).expect("review");
        assert_eq!(served_level(&registry, version), "A", "{version} lacks an input");
    }
}

/// REQ: PKG-681
#[test]
fn publish_to_a_level_b_registry_without_sbom_or_license_is_refused_with_registry_policy() {
    let mut registry = FakeRegistry::new(REGISTRY_BASE);
    registry.require_level_b();
    let no_sbom = refusal_detail(&mut registry, package_with("1.0.0", Some("MIT"), &[("LICENSE", b"t")]));
    assert!(no_sbom.expect("refused").contains("sbom-missing"));
    let bad_license = refusal_detail(
        &mut registry,
        package_with("1.0.0", Some("not a licence"), &[("sbom.json", b"{}"), ("LICENSE", b"t")]),
    );
    assert!(bad_license.expect("refused").contains("license-invalid"));
    let no_notice = refusal_detail(&mut registry, package_with("1.0.0", Some("MIT"), &[("sbom.json", b"{}")]));
    assert!(no_notice.expect("refused").contains("license-notice-missing"));
    // Refused uploads leave no version behind; a complete one is queued and shown at level A.
    assert!(refusal_detail(&mut registry, complete_package("1.0.0")).is_none());
    assert_eq!(served_level(&registry, "1.0.0"), "A", "queued for review, level A until reviewed");
}

/// REQ: PKG-682
#[test]
fn level_a_publish_without_sbom_or_notice_is_accepted_and_never_badged_b() {
    let mut registry = FakeRegistry::new(REGISTRY_BASE);
    assert!(refusal_detail(&mut registry, package_with("1.0.0", None, &[])).is_none());
    registry.record_review(PLUGIN, "1.0.0").expect("review");
    assert_eq!(served_level(&registry, "1.0.0"), "A", "a reviewed version without inputs is never B");
    let page = page_text("reference-registry.md");
    assert!(page.contains("`No SBOM`") && page.contains("`No licence notice`"));
}

/// REQ: PKG-683
#[test]
fn level_b_badge_is_derived_per_version_and_package_hash() {
    let mut registry = FakeRegistry::new(REGISTRY_BASE);
    registry.publish(complete_package("1.0.0"), "stable");
    registry.record_review(PLUGIN, "1.0.0").expect("review");
    registry.publish(complete_package("1.1.0"), "stable");
    assert_eq!(served_level(&registry, "1.0.0"), "B");
    assert_eq!(served_level(&registry, "1.1.0"), "A", "a new version never inherits the level");
    // A re-upload with another hash returns the reviewed version to level A.
    registry.reupload(
        PLUGIN,
        "1.0.0",
        package_with("1.0.0", Some("MIT"), &[("sbom.json", b"{\"changed\":1}"), ("LICENSE", b"t")]),
    );
    assert_eq!(served_level(&registry, "1.0.0"), "A");
    // A withdrawn record returns the version to level A.
    registry.publish(complete_package("1.2.0"), "stable");
    registry.record_review(PLUGIN, "1.2.0").expect("review");
    assert_eq!(served_level(&registry, "1.2.0"), "B");
    registry.withdraw_review(PLUGIN, "1.2.0");
    assert_eq!(served_level(&registry, "1.2.0"), "A");
}

/// REQ: PKG-684
#[test]
fn client_shows_badge_b_only_when_index_level_sbom_and_notice_agree() {
    let mut registry = FakeRegistry::new(BASE.trim_end_matches('/'));
    registry.publish(complete_package("1.0.0"), "stable");
    registry.record_review(PLUGIN, "1.0.0").expect("review");
    let sandbox = Sandbox::new();
    for (url, bytes) in registry.documents() {
        sandbox.net.put_file(&url, bytes);
    }
    assert_eq!(sandbox.pin(BASE, &key_id_of(FAKE_REGISTRY_SEED)).exit_code, 0);
    let output = sandbox.run(&["info", PLUGIN, "--json"]);
    assert!(!has_error(&output, "registry.unreachable"), "{output:?}");
    let shown = serde_json::to_string(&output.stdout_jsonl).expect("serialize output");
    assert!(shown.contains("\"B\"") || shown.contains("level B"), "level B must be shown: {shown}");
    // The same registry without a review record must show level A.
    registry.withdraw_review(PLUGIN, "1.0.0");
    let sandbox = Sandbox::new();
    for (url, bytes) in registry.documents() {
        sandbox.net.put_file(&url, bytes);
    }
    assert_eq!(sandbox.pin(BASE, &key_id_of(FAKE_REGISTRY_SEED)).exit_code, 0);
    let output = sandbox.run(&["info", PLUGIN, "--json"]);
    let shown = serde_json::to_string(&output.stdout_jsonl).expect("serialize output");
    assert!(!shown.contains("level B"), "no badge without a review record: {shown}");
}

/// REQ: PKG-685
#[test]
fn publishing_guide_lists_the_level_b_inputs() {
    let guide = page_text("guide-publishing.md");
    let row = guide.lines().find(|line| line.contains("[PKG-685]")).expect("PKG-685 row");
    for input in ["`sbom.json`", "SPDX", "`LICENSE`", "`NOTICE`", "`registry.policy`"] {
        assert!(row.contains(input), "row must name {input}");
    }
    // The inputs the row names are exactly what the publish gate checks.
    let mut registry = FakeRegistry::new(REGISTRY_BASE);
    registry.require_level_b();
    assert!(refusal_detail(&mut registry, package_with("1.0.0", None, &[])).is_some());
}


/// REQ: PKG-680
#[test]
fn level_b_manual_states_the_4_mib_sbom_limit_and_spdx_rule() {
    let registry_page = page_text("reference-registry.md");
    assert!(registry_page.contains("`sbom.json` of at most 4 MiB"));
    assert!(registry_page.contains("valid SPDX expression"));
}

/// REQ: PKG-681
#[test]
fn level_b_refusal_detail_names_the_three_missing_inputs() {
    let registry_page = page_text("reference-registry.md");
    assert!(registry_page.contains("`registry.policy`"));
    for detail in ["sbom-missing", "license-invalid", "license-notice-missing"] {
        assert!(registry_page.contains(detail), "{detail}");
    }
}

/// REQ: PKG-682
#[test]
fn level_a_fallback_labels_are_no_sbom_and_no_licence_notice() {
    let guide_page = page_text("guide-publishing.md");
    assert!(guide_page.contains("`No SBOM`"));
    assert!(guide_page.contains("`No licence notice`"));
}

/// REQ: PKG-683
#[test]
fn level_b_manual_drops_a_withdrawn_review_back_to_level_a() {
    let threat_page = page_text("reference-threat-model.md");
    assert!(threat_page
        .contains("a new version, a different hash or a withdrawn record returns the version to level A"));
}

/// REQ: PKG-684
#[test]
fn index_badge_rule_needs_review_level_b_and_both_flags_true() {
    let registry_page = page_text("reference-registry.md");
    assert!(registry_page.contains(
        "shows the level B badge only when `review_level` is `\"B\"` and `sbom` and `license_notice` are both `true`"
    ));
}

/// REQ: PKG-685
#[test]
fn publishing_guide_row_names_the_three_level_b_inputs() {
    let guide_page = page_text("guide-publishing.md");
    assert!(guide_page.contains(
        "include `sbom.json`, a valid SPDX `license` in the manifest and a `LICENSE` or `NOTICE` file"
    ));
}
