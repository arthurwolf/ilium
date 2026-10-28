//! ABI major-version gap doc checks: one test per requirement ID for the ABI major
//! version rules. Each test reads the manual page that states the rule and checks the
//! rule's key phrases.

use crate::support::manual_page;

/// REQ: ABI-059
#[test]
fn abi_059_component_api_version_section_must_match_manifest() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "MUST carry the custom section `ilium:api-version`",
        "with bytes `00 01 00 00 00 00`",
        "a section that differs from the manifest `api` MUST be refused with `manifest.api-version`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-060
#[test]
fn abi_060_inside_1x_host_applies_minor_window_and_refuses_major_mismatch() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "Inside the 1.x line a host MUST apply the window rule of ABI evolution",
        "current minor and previous one",
        "MUST NOT be loaded through that window",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-059
#[test]
fn abi_059_inputs_of_the_api_version_check_give_one_result() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "Inputs are the `api` key of the manifest and the `ilium:api-version` section of the component.",
        "The output of the check is either a loaded package or one refusal code.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-060
#[test]
fn abi_060_major_one_package_is_outside_every_zero_host_window() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "is outside the window of every `0.x` host",
        "A `0.x` package that is not the final minor is refused on a `1.0.0` host",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
