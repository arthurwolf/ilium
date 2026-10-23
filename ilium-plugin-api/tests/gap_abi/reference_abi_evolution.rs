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
