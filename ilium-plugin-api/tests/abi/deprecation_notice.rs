//! Notice and removal of host features: ABI-052..054 of reference-requirements-wit-abi.md
//! (reference-abi-evolution.md, "Notice and removal"). Each test calls an unimplemented! stub
//! until the deprecation policy lands.

use ilium_plugin_api::deprecation::{
    deprecation_records, feature_available, notice_is_long_enough, refusal_for_feature,
    MarkedFeature,
};

const FEATURE: &str = "host.legacy-clipboard";
const UNMARKED: &str = "host.stable-clock";
const PLUGIN: &str = "example.notes";

fn marked(marked_in_minor: u32, removal_minor: u32) -> MarkedFeature {
    MarkedFeature {
        name: FEATURE.to_owned(),
        marked_in_minor,
        removal_minor,
    }
}

/// REQ: ABI-052
#[test]
fn abi_052_removal_is_at_least_two_minors_after_the_marking_release() {
    assert!(!notice_is_long_enough(&marked(2, 3)));
    assert!(notice_is_long_enough(&marked(2, 4)));
}

/// REQ: ABI-052
#[test]
fn abi_052_a_removal_in_the_marking_minor_is_refused_as_too_short() {
    assert!(!notice_is_long_enough(&marked(2, 2)));
}

/// REQ: ABI-053
#[test]
fn abi_053_marked_feature_keeps_working_until_its_removal_minor() {
    let feature = marked(2, 4);
    assert!(feature_available(&feature, 3));
    assert!(!feature_available(&feature, 4));
}

/// REQ: ABI-053
#[test]
fn abi_053_one_deprecated_record_per_plugin_and_feature_naming_the_removal_minor() {
    let feature = marked(2, 4);
    let records = deprecation_records(
        PLUGIN,
        &[FEATURE, FEATURE, UNMARKED],
        std::slice::from_ref(&feature),
        3,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].plugin_id, PLUGIN);
    assert_eq!(records[0].feature, FEATURE);
    assert_eq!(records[0].removal_minor, 4);
}

/// REQ: ABI-054
#[test]
fn abi_054_a_removed_feature_refuses_with_a_section_4_code() {
    let feature = marked(2, 4);
    let code = refusal_for_feature(&feature, 4).expect("removed feature refuses");
    let section_4_codes = [
        "manifest.schema",
        "package.api-unsupported",
        "manifest.api-version",
        "manifest.import-not-requested",
        "manifest.grant-unknown",
    ];
    assert!(section_4_codes.contains(&code), "got {code}");
}

/// REQ: ABI-054
#[test]
fn abi_054_a_feature_that_still_works_has_no_refusal() {
    let feature = marked(2, 4);
    assert_eq!(refusal_for_feature(&feature, 3), None);
}
