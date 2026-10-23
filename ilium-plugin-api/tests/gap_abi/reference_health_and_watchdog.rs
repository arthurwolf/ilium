//! ABI-079 and ABI-080 from reference-health-and-watchdog.md.

use ilium_plugin_api::abi::{all_abi_codes, metric_samples_for, trigger_abi_failure};

/// REQ: ABI-079
#[test]
fn abi_079_every_abi_trap_and_refusal_code_logs_plugin_code_and_phase() {
    let codes = all_abi_codes();
    assert!(
        !codes.is_empty(),
        "the ABI trap and refusal code list must not be empty"
    );
    for code in codes {
        let record = trigger_abi_failure(code, "fixture-plugin");
        assert_eq!(
            record.plugin, "fixture-plugin",
            "{code}: the record must carry the plugin id"
        );
        assert_eq!(
            record.code, code,
            "{code}: the record must carry the ABI code"
        );
        assert!(
            !record.phase.is_empty(),
            "{code}: the record must carry a non-empty phase"
        );
    }
}

/// REQ: ABI-080
#[test]
fn abi_080_counter_per_abi_code_labelled_by_plugin_id() {
    for code in all_abi_codes() {
        trigger_abi_failure(code, "metrics-plugin");
    }
    let samples = metric_samples_for("metrics-plugin");
    for code in all_abi_codes() {
        let sample = samples
            .iter()
            .find(|sample| sample.code == code)
            .unwrap_or_else(|| panic!("a counter for {code} must exist"));
        assert_eq!(
            sample.plugin, "metrics-plugin",
            "the counter must be labelled by plugin id"
        );
        assert!(
            sample.value >= 1,
            "the counter for {code} must count the failure"
        );
    }
}
