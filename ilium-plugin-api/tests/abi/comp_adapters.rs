//! Adapters of the minor window: COMP-006 (compat-load-decision-table.md).

use ilium_plugin_api::compat::{map_outcome_for, matrix_mismatches};
use ilium_plugin_api::generated::types::Outcome;
use ilium_plugin_api::version::{ApiVersion, CURRENT};

/// REQ: COMP-006
#[test]
fn comp_006_every_window_cell_matches_its_stated_result_a() {
    assert!(
        matrix_mismatches().is_empty(),
        "every package inside the window must behave as its minor did"
    );
}

/// REQ: COMP-006
#[test]
fn comp_006_base_outcome_is_passed_unchanged_to_a_package_of_the_window_b() {
    let older = ApiVersion {
        major: CURRENT.major,
        minor: CURRENT.minor.saturating_sub(1),
        patch: 0,
    };
    assert!(matches!(map_outcome_for(&Outcome::Ok, older), Outcome::Ok));
}
