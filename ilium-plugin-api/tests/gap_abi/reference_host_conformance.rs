//! ABI-073, ABI-074, ABI-075 and ABI-120 from reference-host-conformance.md.

use ilium_plugin_api::conformance::{
    corpus_manifest, corpus_report, coverage_report, may_claim_conformance,
};
use ilium_plugin_api::version::CURRENT;
use std::collections::BTreeSet;

fn host_conformance_page() -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/reference-host-conformance.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).expect("manual page should be readable")
}

/// REQ: ABI-073
#[test]
fn abi_073_corpus_is_versioned_and_every_case_has_a_stable_unique_id() {
    let manifest = corpus_manifest();
    assert!(
        !manifest.version.is_empty(),
        "the corpus must carry a version"
    );
    assert!(!manifest.cases.is_empty(), "the corpus must contain cases");
    let mut seen_ids = BTreeSet::new();
    for case in &manifest.cases {
        assert!(
            seen_ids.insert(case.id.clone()),
            "corpus case id {} must be unique and stable",
            case.id
        );
    }
    let page = host_conformance_page();
    assert!(
        page.contains("[ABI-073] The project MUST publish a versioned ABI conformance corpus, with a stable identifier per case"),
        "reference-host-conformance.md must state the ABI-073 MUST text"
    );
}

/// REQ: ABI-074
#[test]
fn abi_074_every_corpus_case_states_input_expected_result_and_matrix_row() {
    for case in corpus_manifest().cases {
        assert!(
            !case.input.is_empty(),
            "case {} must state its input",
            case.id
        );
        assert!(
            !case.expected.is_empty(),
            "case {} must state its expected result or refusal code",
            case.id
        );
        assert!(
            !case.matrix_row.is_empty(),
            "case {} must name the ABI-030 matrix row it covers",
            case.id
        );
    }
}

/// REQ: ABI-075
#[test]
fn abi_075_conformance_claim_needs_all_cases_passing_and_matching_corpus_version() {
    let manifest = corpus_manifest();
    assert_eq!(
        manifest.version,
        CURRENT.to_string(),
        "the corpus version must equal the API version it checks"
    );
    let expected_claim = corpus_report().all_passed();
    assert_eq!(
        may_claim_conformance(CURRENT),
        expected_claim,
        "a host may claim conformance only when every case of the corpus passes"
    );
    let page = host_conformance_page();
    assert!(
        page.contains("[ABI-075] A host MAY claim ABI conformance for a version only when every case of that version's corpus passes"),
        "reference-host-conformance.md must state the ABI-075 MUST text"
    );
}

/// REQ: ABI-120
#[test]
fn abi_120_coverage_report_lists_uncovered_pairs_and_fails_release_when_non_empty() {
    let report = coverage_report();
    assert!(
        report.uncovered_pairs().is_empty(),
        "every import/error-code pair, limit edge and revoked grant needs a case: {:?}",
        report.uncovered_pairs()
    );
    assert!(
        !report.fails_release(),
        "the release gate must pass when no import/code pair is uncovered"
    );
    assert!(
        corpus_manifest()
            .cases
            .iter()
            .any(|case| case.id.contains("revoked")),
        "the corpus must include a revoked-grant case"
    );
}
