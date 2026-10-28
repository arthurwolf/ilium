//! Backwards-compatibility test matrix: every shipped plugin version still loads on the current
//! host (DEV-987). Page: guide-backwards-compatibility-test-matrix.

use crate::{load_fixture_on_current_host, manual_page, released_plugin_fixtures};

/// REQ: DEV-987
#[test]
fn every_released_plugin_fixture_loads_on_the_current_host_and_the_matrix_lists_each_one() {
    let page = manual_page("guide-backwards-compatibility-test-matrix");
    assert!(
        page.to_lowercase().contains("matrix"),
        "the guide defines the matrix"
    );

    let fixtures = released_plugin_fixtures();
    assert!(
        !fixtures.is_empty(),
        "the matrix holds at least one released plugin fixture"
    );
    for fixture in &fixtures {
        assert!(
            page.contains(fixture.version.as_str()),
            "the matrix lists version {}",
            fixture.version
        );
        let outcome = load_fixture_on_current_host(fixture);
        assert!(
            outcome.loaded,
            "released {} loads on the current host: {:?}",
            fixture.version, outcome.error_code
        );
        assert_eq!(
            outcome.error_code, None,
            "no load error for {}",
            fixture.version
        );
    }
}

/// REQ: DEV-987
#[test]
fn the_matrix_command_is_documented_with_the_window_option_and_jsonl_output() {
    let page = manual_page("guide-backwards-compatibility-test-matrix");
    assert!(
        page.contains("--against window"),
        "the guide documents the --against window option"
    );
    assert!(page.contains("JSONL"), "the guide says the report is JSONL");
}
