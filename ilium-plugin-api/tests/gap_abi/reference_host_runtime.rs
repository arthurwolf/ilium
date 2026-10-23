//! ABI-070, ABI-071 and ABI-072 from reference-host-runtime.md.

use ilium_plugin_api::conformance::corpus_report;
use ilium_plugin_api::platform::doctor_report;

fn host_runtime_page() -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/reference-host-runtime.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).expect("manual page should be readable")
}

/// REQ: ABI-070
#[test]
fn abi_070_doctor_reports_pinned_wasmtime_and_canonical_abi_revision() {
    let report = doctor_report();
    assert!(
        semver::Version::parse(&report.wasmtime_version).is_ok(),
        "the pinned Wasmtime version must be an exact semver version, got {:?}",
        report.wasmtime_version
    );
    assert!(
        !report.canonical_abi_revision.is_empty(),
        "the canonical ABI revision must be reported"
    );
    let rendered = report.render();
    assert!(
        rendered.contains(&report.wasmtime_version) && rendered.contains(&report.canonical_abi_revision),
        "`ilium plugin doctor` output must show both the Wasmtime version and the canonical ABI revision"
    );
}

/// REQ: ABI-071
#[test]
fn abi_071_runtime_upgrade_gated_on_full_conformance_corpus() {
    let report = corpus_report();
    assert!(
        report.all_passed(),
        "the full ABI conformance corpus must pass before a runtime upgrade ships: {:?}",
        report.failures()
    );
    let page = host_runtime_page();
    assert!(
        page.contains("[ABI-071] A runtime upgrade that changes canonical-ABI behaviour, trap behaviour or fuel accounting MUST NOT ship until the full ABI conformance corpus"),
        "reference-host-runtime.md must state the ABI-071 MUST text"
    );
}

/// REQ: ABI-072
#[test]
fn abi_072_release_notes_list_every_trap_and_refusal_change() {
    let page = host_runtime_page();
    assert!(
        page.contains("[ABI-072] A runtime upgrade MUST list in the release notes every observable change in traps or refusal codes"),
        "reference-host-runtime.md must state the ABI-072 MUST text"
    );
}
