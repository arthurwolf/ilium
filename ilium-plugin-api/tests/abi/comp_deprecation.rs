//! Removal of a host feature and its notice: COMP-010, COMP-011, COMP-012, COMP-013, COMP-014
//! (compat-deprecation-timeline.md).

use ilium_plugin_api::codes;
use ilium_plugin_api::deprecation::{
    deprecation_records, feature_available, notice_is_long_enough, refusal_for_feature,
    MarkedFeature,
};
use ilium_plugin_api::version::{ApiVersion, CURRENT};

/// Marked in minor 1, removed in minor 3 (M = 1, M + 2 = 3).
fn clipboard() -> MarkedFeature {
    MarkedFeature {
        name: "host.legacy-clipboard".to_owned(),
        marked_in_minor: 1,
        removal_minor: 3,
    }
}

fn other_feature() -> MarkedFeature {
    MarkedFeature {
        name: "host.other-thing".to_owned(),
        marked_in_minor: 1,
        removal_minor: 4,
    }
}

/// REQ: COMP-010
#[test]
fn comp_010_removal_minor_is_the_first_minor_without_the_item_a() {
    let feature = clipboard();
    assert!(feature_available(&feature, 2));
    assert!(!feature_available(&feature, 3));
}

/// REQ: COMP-010
#[test]
fn comp_010_deprecation_record_states_the_removal_minor_b() {
    let records = deprecation_records(
        "plugin-a",
        &["host.legacy-clipboard"],
        &[clipboard()],
        2,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].removal_minor, 3);
}

/// REQ: COMP-011
#[test]
fn comp_011_package_using_removed_item_is_refused_with_unsupported_code_a() {
    assert_eq!(
        refusal_for_feature(&clipboard(), 3),
        Some(codes::package::API_UNSUPPORTED)
    );
}

/// REQ: COMP-011
#[test]
fn comp_011_removal_does_not_refuse_packages_that_do_not_use_it_b() {
    assert_eq!(refusal_for_feature(&other_feature(), 3), None);
}

/// REQ: COMP-012
#[test]
fn comp_012_plugin_listing_feature_gets_record_one_minor_before_removal_a() {
    let records = deprecation_records(
        "plugin-a",
        &["host.legacy-clipboard"],
        &[clipboard()],
        2,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].plugin_id, "plugin-a");
    assert_eq!(records[0].feature, "host.legacy-clipboard");
}

/// REQ: COMP-012
#[test]
fn comp_012_plugin_not_listing_feature_gets_no_record_b() {
    let records = deprecation_records("plugin-b", &["host.other-thing"], &[clipboard()], 2);
    assert!(records.is_empty());
}

/// REQ: COMP-013
#[test]
fn comp_013_removing_one_item_does_not_change_another_item_a() {
    let records = deprecation_records(
        "plugin-a",
        &["host.legacy-clipboard", "host.other-thing"],
        &[clipboard(), other_feature()],
        3,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].feature, "host.other-thing");
}

/// REQ: COMP-013
#[test]
fn comp_013_patch_release_keeps_the_whole_current_minor_loaded_b() {
    let patch_nine = ApiVersion {
        major: CURRENT.major,
        minor: CURRENT.minor,
        patch: 9,
    };
    assert!(ApiVersion::host_window().contains(&patch_nine));
}

/// REQ: COMP-014
#[test]
fn comp_014_marking_record_carries_item_name_and_removal_minor_a() {
    let records = deprecation_records(
        "plugin-a",
        &["host.legacy-clipboard"],
        &[clipboard()],
        2,
    );
    assert_eq!(records[0].feature, "host.legacy-clipboard");
    assert_eq!(records[0].removal_minor, 3);
}

/// REQ: COMP-014
#[test]
fn comp_014_records_keep_the_order_of_the_used_items_b() {
    let records = deprecation_records(
        "plugin-a",
        &["host.other-thing", "host.legacy-clipboard"],
        &[clipboard(), other_feature()],
        2,
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].feature, "host.other-thing");
    assert_eq!(records[1].feature, "host.legacy-clipboard");
    assert!(notice_is_long_enough(&clipboard()));
}
