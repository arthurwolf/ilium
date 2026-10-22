//! Closed signature and digest algorithm allow-list (SEC-459). Runtime tests against the
//! compiled-in allow-list; the retired-algorithm refusal and the deprecated-use audit event are
//! not covered here because the manual does not name a retired algorithm or the audit code.

use ilium_plugin_host::signature_algorithms::{accepted_algorithms, check_algorithm};

/// REQ: SEC-459
#[test]
fn sec_459_the_allow_list_is_closed_and_holds_only_ed25519() {
    assert_eq!(accepted_algorithms(), &["ed25519"]);
}

/// REQ: SEC-459
#[test]
fn sec_459_an_unknown_algorithm_in_a_key_or_signature_entry_is_refused_not_skipped() {
    let outcome = check_algorithm("rsa-unlisted-experimental");
    assert!(
        outcome.is_err(),
        "an unknown algorithm must be a verification failure, not a skip"
    );
}

/// REQ: SEC-459
#[test]
fn sec_459_the_listed_ed25519_algorithm_is_still_accepted() {
    assert!(check_algorithm("ed25519").is_ok());
}
