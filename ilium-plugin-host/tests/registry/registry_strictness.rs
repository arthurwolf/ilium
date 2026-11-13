//! reference-registry.md and guide-publishing.md: what review level B requires (PKG-680..685).
//! Lane `registry-publish`. Red phase: the level computation and the publish gate do not exist yet.

/// REQ: PKG-680
#[test]
fn level_b_requires_review_record_sbom_spdx_license_and_notice() {
    unimplemented!(
        "PKG-680: level B needs a review record, sbom.json, valid SPDX licence, LICENSE or NOTICE"
    );
}

/// REQ: PKG-681
#[test]
fn publish_to_a_level_b_registry_without_sbom_or_license_is_refused_with_registry_policy() {
    unimplemented!(
        "PKG-681: registry.policy with sbom-missing, license-invalid, license-notice-missing"
    );
}

/// REQ: PKG-682
#[test]
fn level_a_publish_without_sbom_or_notice_is_accepted_and_never_badged_b() {
    unimplemented!("PKG-682: accepted at level A, page states No SBOM / No licence notice");
}

/// REQ: PKG-683
#[test]
fn level_b_badge_is_derived_per_version_and_package_hash() {
    unimplemented!("PKG-683: new version, other hash or withdrawn review returns to level A");
}

/// REQ: PKG-684
#[test]
fn client_shows_badge_b_only_when_index_level_sbom_and_notice_agree() {
    unimplemented!("PKG-684: review_level B plus sbom and license_notice true, else level A");
}

/// REQ: PKG-685
#[test]
fn publishing_guide_lists_the_level_b_inputs() {
    unimplemented!("PKG-685: guide row names sbom.json, SPDX licence, LICENSE or NOTICE");
}
