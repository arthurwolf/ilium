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
