//! Production-gap doc checks for the CLI output stability page: one test per requirement ID
//! (DEV-862 to DEV-864). Each test reads the manual page that states the rule and checks the
//! rule's key phrases.

use crate::support::manual_page;

/// REQ: DEV-862
#[test]
fn dev_862_enum_removal_or_rename_is_breaking() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST be treated as a breaking change",
        "Adding a value is not breaking",
        "a consumer MUST treat a value it does not know as unknown",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-863
#[test]
fn dev_863_deprecation_listed_in_release_notes() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST appear in the release notes of its release",
        "as one entry that names the command, the item",
        "the kind of change",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-864
#[test]
fn dev_864_deprecated_warning_fields_and_exit_code() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST use only the fields that the",
        "MUST be exactly",
        "It MUST NOT change the exit code",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-862
#[test]
fn dev_862_enumerated_fields_named_in_the_stability_contract() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "The enumerated values are part of the contract too",
        "the `status` values ([DEV-024])",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-863
#[test]
fn dev_863_release_note_entry_states_kind_of_change_and_stop_release() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "the kind of change (`deprecated` or `breaking`) and the stop release",
        "A deprecated item behaves exactly as before.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-864
#[test]
fn dev_864_deprecation_notice_leaves_behaviour_and_exit_code_unchanged() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "The only change is the notice, and the exit code is not affected",
        "An exit code is never reused with a new meaning",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
