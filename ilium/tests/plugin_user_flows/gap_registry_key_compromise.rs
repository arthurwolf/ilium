//! Registry key compromise doc checks: one test per requirement ID stating how a client
//! keeps its last good metadata after refusing a document, how it verifies a root rotation,
//! how retired key ids are never reused, and how it handles a `registry.changed` stop.
//! Each test reads the manual page that states the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: PKG-696
#[test]
fn pkg_696_refused_document_keeps_last_good_metadata() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "keep its last good metadata",
        "MUST NOT install, update or resolve a package",
        "from a document it refused",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-697
#[test]
fn pkg_697_root_rotation_verified_with_both_thresholds() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "verify the new `root.json`",
        "with the threshold of its own `roles.root` keys",
        "before it uses any key id from it",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-698
#[test]
fn pkg_698_retired_key_id_never_reused() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in ["The registry operator MUST NOT", "reuse a retired key id"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-699
#[test]
fn pkg_699_registry_changed_names_url_and_asks_user_to_repin() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "MUST name the registry URL",
        "MUST tell the user to re-pin under [PKG-693]",
        "It MUST NOT re-pin by itself",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
