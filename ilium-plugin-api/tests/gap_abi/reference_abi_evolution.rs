//! ABI-064, ABI-065, ABI-066 and ABI-119 from reference-abi-evolution.md.

use ilium_plugin_api::abi::Activation;
use ilium_plugin_api::compat::{
    ci_matrix_jobs, matrix_cells, matrix_mismatches, observed_result, ExpectedResult,
};
use ilium_plugin_api::version::{host_window, CURRENT};

fn abi_evolution_page() -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/reference-abi-evolution.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).expect("manual page should be readable")
}

/// REQ: ABI-064
#[test]
fn abi_064_matrix_covers_both_directions_for_every_window_minor() {
    let window = host_window();
    let cells = matrix_cells();
    let host_minors_in_window: Vec<u16> = (window.start().minor..=window.end().minor).collect();
    for host_minor in &host_minors_in_window {
        for sdk_minor in &host_minors_in_window {
            assert!(
                cells
                    .iter()
                    .any(|cell| cell.host_minor == *host_minor && cell.sdk_minor == *sdk_minor),
                "matrix must hold a cell for host minor {host_minor} against SDK minor {sdk_minor}"
            );
        }
    }
    let ci_jobs = ci_matrix_jobs();
    for cell in &cells {
        assert!(
            ci_jobs.contains(&cell.job_name),
            "CI job list must run the matrix cell {}",
            cell.job_name
        );
    }
    let page = abi_evolution_page();
    assert!(
        page.contains(
            "[ABI-064] The project MUST publish and run a matrix of host minor against SDK minor"
        ),
        "reference-abi-evolution.md must state the ABI-064 MUST text"
    );
}

/// REQ: ABI-065
#[test]
fn abi_065_every_matrix_cell_states_expected_result_and_observed_matches() {
    for cell in matrix_cells() {
        let observed = observed_result(cell.host_minor, cell.sdk_minor);
        assert_eq!(
            observed, cell.expected,
            "cell host {} / sdk {} must match its stated result",
            cell.host_minor, cell.sdk_minor
        );
        assert!(
            matches!(
                cell.expected,
                ExpectedResult::Loads
                    | ExpectedResult::LoadsDeprecated
                    | ExpectedResult::Refused(_)
            ),
            "every cell must state loads, loads-deprecated or a refusal code"
        );
    }
}

/// REQ: ABI-066
#[test]
fn abi_066_release_gate_blocks_any_mismatched_window_cell() {
    let mismatches = matrix_mismatches();
    assert!(
        mismatches.is_empty(),
        "no matrix cell inside the window may differ from its stated result: {mismatches:?}"
    );
    let page = abi_evolution_page();
    assert!(
        page.contains("[ABI-066] A release MUST NOT ship while any matrix cell inside the window differs from its stated result."),
        "reference-abi-evolution.md must state the ABI-066 MUST text"
    );
}

/// REQ: ABI-119
#[test]
fn abi_119_host_info_reports_exact_version_and_is_stable_within_activation() {
    let activation = Activation::begin();
    let first = activation.host_info();
    let second = activation.host_info();
    assert_eq!(
        first.api_version, CURRENT,
        "host-info must report the exact api-version"
    );
    assert_eq!(
        first, second,
        "host-info must not change during one activation"
    );
    assert_eq!(
        first.limits.len(),
        ilium_plugin_api::limits::LimitKey::ALL.len(),
        "host-info must list every limit in effect"
    );
}

/// REQ: ABI-064
#[test]
fn abi_064_matrix_has_old_plugin_on_new_host_and_new_sdk_on_old_host_cells() {
    let window = host_window();
    let old_minor = window.start().minor;
    let new_minor = window.end().minor;
    let cells = matrix_cells();
    assert!(
        cells
            .iter()
            .any(|cell| cell.host_minor == new_minor && cell.sdk_minor == old_minor),
        "matrix must hold the old plugin (sdk {old_minor}) on the new host ({new_minor})"
    );
    assert!(
        cells
            .iter()
            .any(|cell| cell.host_minor == old_minor && cell.sdk_minor == new_minor),
        "matrix must hold the new SDK (sdk {new_minor}) on the old host ({old_minor})"
    );
}

/// REQ: ABI-065
#[test]
fn abi_065_each_host_and_sdk_minor_pair_has_exactly_one_stated_cell() {
    let cells = matrix_cells();
    let pairs: std::collections::HashSet<(u16, u16)> = cells
        .iter()
        .map(|cell| (cell.host_minor, cell.sdk_minor))
        .collect();
    assert_eq!(
        pairs.len(),
        cells.len(),
        "a host/SDK minor pair with two stated results cannot say which one is expected"
    );
}

/// REQ: ABI-066
#[test]
fn abi_066_every_gated_cell_lies_inside_the_host_window() {
    let window = host_window();
    for cell in matrix_cells() {
        assert!(
            (window.start().minor..=window.end().minor).contains(&cell.host_minor),
            "cell with host minor {} is outside the window the release gate checks",
            cell.host_minor
        );
        assert!(
            (window.start().minor..=window.end().minor).contains(&cell.sdk_minor),
            "cell with SDK minor {} is outside the window the release gate checks",
            cell.sdk_minor
        );
    }
}

/// REQ: ABI-119
#[test]
fn abi_119_host_info_names_a_build_and_only_offers_known_unstable_features() {
    let info = Activation::begin().host_info();
    assert!(!info.build_id.is_empty(), "host-info must carry the host build id");
    assert!(
        info.unstable_features.iter().all(|feature| {
            ilium_plugin_api::abi::UNSTABLE_FEATURES.contains(&feature.as_str())
        }),
        "host-info reports an unstable feature the host does not define: {:?}",
        info.unstable_features
    );
}
