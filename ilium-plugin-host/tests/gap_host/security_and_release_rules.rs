//! Requirements HST-512, HST-520 and HST-521: the release stages for a Wasmtime bump, the intake
//! and rating rules for host and sandbox reports, and the embargo and fix windows.
//!
//! The manual rows of these three requirements are process checks (release checklist, security
//! review, advisory dates). The tests here check what can be checked without a release: that the
//! manual states each rule with its numbers, and that `SECURITY.md` exists at the repository root
//! with the contact the security-response page names. The timing of a real report or release stays
//! a manual release check and is not claimed as covered by these tests.

fn repository_file(relative_path: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative_path);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} must be readable: {error}", path.display());
    })
}

fn manual_page(page_file: &str) -> String {
    repository_file(&format!("docs/plugins-manual/pages/{page_file}"))
}

/// REQ: HST-512
#[test]
fn hst_512_the_runtime_page_states_the_three_release_stages() {
    let page = manual_page("reference-host-runtime.md");
    assert!(
        page.contains("MUST pass through three stages before general release"),
        "the page must state the three release stages"
    );
    assert!(
        page.contains("the conformance suite and the soak test on the reference machine"),
        "the first stage is the conformance suite and the soak test"
    );
    assert!(
        page.contains("a pre-release channel for at least 3 days"),
        "the pre-release stage lasts at least 3 days"
    );
}

/// REQ: HST-512
#[test]
fn hst_512_an_exploited_fix_may_shorten_the_prerelease_but_never_skip_the_suite() {
    let page = manual_page("reference-host-runtime.md");
    assert!(
        page.contains("MAY shorten the pre-release stage to 24 hours"),
        "the shortened pre-release stage is 24 hours and only for a fix exploited in the wild"
    );
    assert!(
        page.contains("MUST NOT skip the conformance suite"),
        "no release, shortened or not, skips the conformance suite"
    );
}

/// REQ: HST-520
#[test]
fn hst_520_security_md_exists_at_the_root_and_names_the_contact_the_manual_names() {
    let security = repository_file("SECURITY.md");
    let page = manual_page("reference-security-response.md");
    let contact = "security@ilium.example";
    assert!(
        page.contains(contact),
        "the security-response page names the placeholder contact {contact} until a real address is published"
    );
    assert!(
        security.contains(contact),
        "SECURITY.md must name the same contact as the manual"
    );
}

/// REQ: HST-520
#[test]
fn hst_520_the_response_page_states_the_rating_floor_and_the_acknowledgement_window() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("MUST be rated at least High"),
        "a host, Wasmtime-configuration or sandbox-helper report is rated at least High"
    );
    assert!(
        page.contains("a sandbox escape MUST be rated Critical"),
        "a sandbox escape is rated Critical"
    );
    assert!(
        page.contains("acknowledge such a report within 2 working days"),
        "a host or sandbox report is acknowledged within 2 working days"
    );
}

/// REQ: HST-521
#[test]
fn hst_521_the_response_page_states_the_embargo_and_the_fix_windows() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("the embargo MUST be 90 days from the report or until a fix is released, whichever is first"),
        "the embargo is 90 days from the report or until a fix, whichever is first"
    );
    assert!(
        page.contains("A fix MUST be released within 30 days for a Critical flaw"),
        "a Critical host or sandbox flaw is fixed within 30 days"
    );
    assert!(
        page.contains("within 60 days for a High flaw"),
        "a High host or sandbox flaw is fixed within 60 days"
    );
}

/// REQ: HST-521
#[test]
fn hst_521_the_advisory_must_name_affected_tiers_and_grant_reachability() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("the advisory MUST name the affected tiers and say whether the flaw can be reached without a grant"),
        "every host advisory names the affected tiers and says whether a grant is needed"
    );
}
