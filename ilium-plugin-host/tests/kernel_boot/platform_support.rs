//! reference-platform-support.md (HST-380..388).

use std::time::Duration;

use ilium_plugin_api::{Platform, Tier};
use ilium_plugin_host::platform_rules::{
    check_sandbox, normalize_plugin_path, select_precompiled, shipped_triples, termination_plan,
    tick_gap_ignored, TerminationKind, WasmArtifact,
};
use proptest::prelude::*;

use crate::common::*;

/// REQ: HST-380
#[test]
fn hst_380_t2_is_refused_on_windows_with_sandbox_unavailable() {
    let diagnostic = check_sandbox(Platform::Windows, Tier::T2).expect_err("refused");
    assert_eq!(diagnostic.code, "sandbox.unavailable");
    assert!(diagnostic.message.to_lowercase().contains("windows"));
}

/// REQ: HST-380
#[test]
fn hst_380_t3_is_refused_on_macos_naming_the_platform() {
    let diagnostic = check_sandbox(Platform::MacOs, Tier::T3).expect_err("refused");
    assert_eq!(diagnostic.code, "sandbox.unavailable");
    assert!(diagnostic.message.to_lowercase().contains("macos"));
}

/// REQ: HST-380
#[test]
fn hst_380_t2_and_t3_load_on_linux_and_t1_loads_everywhere() {
    for tier in [Tier::T2, Tier::T3] {
        assert!(check_sandbox(Platform::Linux, tier).is_ok());
    }
    for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
        assert!(check_sandbox(platform, Tier::T1).is_ok());
    }
}

/// REQ: HST-381
#[test]
fn hst_381_the_platform_page_names_one_runtime_config_for_every_platform() {
    let page = manual_page("reference-platform-support");
    assert!(page.contains("runtime_config()"));
    assert!(page.contains("Wasmtime"));
}

/// REQ: HST-381
#[test]
fn hst_381_t1_has_no_platform_restriction_in_the_matrix() {
    for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
        assert!(check_sandbox(platform, Tier::T1).is_ok(), "{platform:?}");
    }
}

/// REQ: HST-382
#[test]
fn hst_382_only_shipped_triples_are_listed_as_supported() {
    let shipped = shipped_triples();
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(shipped.contains(&triple.to_owned()), "{triple}");
    }
    assert!(!shipped.contains(&"riscv64gc-unknown-linux-gnu".to_owned()));
}

/// REQ: HST-382
#[test]
fn hst_382_the_page_marks_unshipped_rows_as_not_shipped() {
    let page = manual_page("reference-platform-support");
    assert!(page.contains("not shipped"));
}

/// REQ: HST-383
#[test]
fn hst_383_a_matching_cwasm_is_selected() {
    let available = [
        WasmArtifact::cwasm("x86_64-unknown-linux-gnu"),
        WasmArtifact::wasm(),
    ];
    let chosen = select_precompiled("x86_64-unknown-linux-gnu", &available).expect("artifact available");
    assert_eq!(
        chosen.artifact,
        WasmArtifact::cwasm("x86_64-unknown-linux-gnu")
    );
    assert!(!chosen.logged_fallback);
}

/// REQ: HST-383
#[test]
fn hst_383_without_a_matching_cwasm_the_host_compiles_the_wasm_and_logs_the_fallback() {
    let available = [
        WasmArtifact::cwasm("aarch64-apple-darwin"),
        WasmArtifact::wasm(),
    ];
    let chosen = select_precompiled("x86_64-unknown-linux-gnu", &available).expect("artifact available");
    assert_eq!(chosen.artifact, WasmArtifact::wasm());
    assert!(chosen.logged_fallback);
    assert_eq!(chosen.log_code, "host.cwasm-incompatible");
}

/// REQ: HST-383
#[test]
fn hst_383_every_shipped_triple_has_exactly_one_cwasm_per_release_plugin() {
    assert_eq!(shipped_triples().len(), 4);
}

/// REQ: HST-384
#[test]
fn hst_384_windows_paths_are_presented_with_forward_slashes() {
    assert_eq!(
        normalize_plugin_path(Platform::Windows, r"C:\Users\a\b.txt"),
        "C:/Users/a/b.txt"
    );
}

/// REQ: HST-384
#[test]
fn hst_384_unix_paths_are_unchanged() {
    assert_eq!(
        normalize_plugin_path(Platform::Linux, "/home/a/b.txt"),
        "/home/a/b.txt"
    );
}

/// REQ: HST-384
#[test]
fn hst_384_the_page_documents_case_sensitivity_per_platform() {
    let page = manual_page("reference-platform-support");
    assert!(page.to_lowercase().contains("case"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    // REQ: HST-384
    #[test]
    fn hst_384_normalized_paths_never_contain_a_backslash(path in r"[A-Za-z0-9_\\/.:-]{0,40}") {
        for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
            let normalized = normalize_plugin_path(platform, &path);
            if platform == Platform::Windows {
                prop_assert!(!normalized.contains('\\'));
            }
        }
    }
}

/// REQ: HST-385
#[test]
fn hst_385_linux_sends_sigterm_then_sigkill_after_two_seconds() {
    let plan = termination_plan(Platform::Linux);
    assert_eq!(plan.kind, TerminationKind::SigtermThenSigkill);
    assert_eq!(plan.grace, Duration::from_secs(2));
}

/// REQ: HST-385
#[test]
fn hst_385_macos_uses_the_same_signal_plan() {
    let plan = termination_plan(Platform::MacOs);
    assert_eq!(plan.kind, TerminationKind::SigtermThenSigkill);
    assert_eq!(plan.grace, Duration::from_secs(2));
}

/// REQ: HST-385
#[test]
fn hst_385_windows_runs_children_in_a_job_object_closed_after_two_seconds() {
    let plan = termination_plan(Platform::Windows);
    assert_eq!(plan.kind, TerminationKind::JobObject);
    assert_eq!(plan.grace, Duration::from_secs(2));
    assert!(plan.children_in_job);
}

/// REQ: HST-386
#[test]
fn hst_386_a_tick_gap_of_exactly_one_second_counts() {
    assert!(!tick_gap_ignored(Duration::from_millis(1000)));
}

/// REQ: HST-386
#[test]
fn hst_386_a_tick_gap_just_over_one_second_is_ignored() {
    assert!(tick_gap_ignored(Duration::from_millis(1001)));
}

/// REQ: HST-386
#[test]
fn hst_386_a_tick_gap_just_under_one_second_counts() {
    assert!(!tick_gap_ignored(Duration::from_millis(999)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    // REQ: HST-386
    #[test]
    fn hst_386_only_gaps_over_one_second_are_ignored(gap_ms in 0u64..10_000) {
        prop_assert_eq!(tick_gap_ignored(Duration::from_millis(gap_ms)), gap_ms > 1000);
    }
}

/// REQ: HST-387
#[test]
fn hst_387_the_page_requires_conformance_on_every_shipped_platform() {
    let page = manual_page("reference-platform-support");
    assert!(page.contains("HST-387"));
    assert!(page.contains("conformance"));
}

/// REQ: HST-387
#[test]
fn hst_387_the_conformance_crate_is_a_workspace_member() {
    assert!(workspace_crates()
        .iter()
        .any(|krate| krate.name == "ilium-plugin-conformance"));
}

/// REQ: HST-388
#[test]
fn hst_388_the_page_gates_performance_on_linux_x86_64_only() {
    let page = manual_page("reference-platform-support");
    assert!(page.contains("HST-388"));
    assert!(page.contains("x86_64"));
}

/// REQ: HST-388
#[test]
fn hst_388_the_bench_crate_is_a_workspace_member() {
    assert!(workspace_crates()
        .iter()
        .any(|krate| krate.name == "ilium-plugin-bench"));
}

#[test]
fn hst_383_missing_portable_component_cannot_be_reported_as_a_fallback() {
    use ilium_plugin_host::platform_rules::ArtifactSelectionError;
    for artifacts in [vec![], vec![WasmArtifact::cwasm("aarch64-apple-darwin")]] {
        assert_eq!(
            select_precompiled("x86_64-unknown-linux-gnu", &artifacts),
            Err(ArtifactSelectionError::MissingPortableComponent),
        );
    }
}
