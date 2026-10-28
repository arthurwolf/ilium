//! Release-key compromise doc checks: one test per requirement ID stating that a release key
//! compromise leaves the trust store, publisher pins and installed plugins untouched, and that
//! no user-facing input can accept a release key. Each test reads the manual page that states
//! the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: SEC-431
#[test]
fn sec_431_release_key_compromise_leaves_trust_state_unchanged() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "A release key compromise MUST NOT change the trust store",
        "any publisher pin, or any installed plugin's trust state",
        "Plugins keep loading exactly as before.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-432
#[test]
fn sec_432_no_input_accepts_release_key_or_retirement_statement() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "No command, setting, environment variable or policy key MAY accept a release key",
        "a release key fingerprint or a retirement statement",
        "The only path to a new release key is a new binary",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-431
#[test]
fn sec_431_plugins_keep_loading_exactly_as_before_a_key_compromise() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "A user acts on a compromise only through a new release.",
        "The user does not edit the blocklist",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-432
#[test]
fn sec_432_a_new_release_key_is_only_accepted_from_a_new_binary() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "A new release key found in any file other than a new release binary",
        "| Not accepted ([SEC-426]) |",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
