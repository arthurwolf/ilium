//! Rule statements of the performance page as the manual states them (doc-lint tests).
use crate::support::{assert_page_line, assert_rule};

/// REQ: AUT-101
#[test]
fn doc_aut_101_design_for_tier_default_not_ceiling() {
    assert_rule("AUT-101", "MUST", &["within the default of its tier"]);
}

/// REQ: AUT-102 WARN
#[test]
fn doc_aut_102_stay_under_half_of_each_default_budget() {
    assert_rule("AUT-102", "SHOULD", &["under half of each default budget"]);
}

/// REQ: AUT-103
#[test]
fn doc_aut_103_rebuildable_data_not_in_durable_bucket_perf() {
    assert_rule(
        "AUT-103",
        "MUST NOT",
        &[
            "Rebuildable data MUST NOT go into a durable bucket",
            "16 MiB",
        ],
    );
}

/// REQ: AUT-104 WARN
#[test]
fn doc_aut_104_own_timing_read_from_monotonic_millis() {
    assert_rule(
        "AUT-104",
        "SHOULD NOT",
        &["`monotonic-millis`", "SHOULD NOT depend on a host tool"],
    );
}

/// REQ: AUT-105
#[test]
fn doc_aut_105_comparison_timings_from_release_build() {
    assert_rule("AUT-105", "MUST", &["MUST come from a release build"]);
}

/// REQ: AUT-103
#[test]
fn doc_aut_103_bucket_rule_points_to_the_lease_recipe() {
    assert_page_line(
        "AUT-103",
        &["The recipe with the lease and the time limit is in the"],
    );
}

/// REQ: AUT-105
#[test]
fn doc_aut_105_debug_build_is_named_as_slower_than_release() {
    assert_page_line(
        "AUT-105",
        &["A debug build (`ilium plugin build --debug`) is slower than a release build."],
    );
}
