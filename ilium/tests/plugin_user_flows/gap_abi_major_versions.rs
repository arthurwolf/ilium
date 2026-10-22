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
