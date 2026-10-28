//! ABI-081, ABI-100, ABI-101, ABI-102, ABI-105 and ABI-114 from reference-runtime-model.md.

use ilium_plugin_api::abi::{
    accept_plugin_float, host_float_fields_in_fixture, lift_invalid, parse_host_payload_host,
    parse_host_payload_sdk, run_host_import, run_import_loop, run_realloc_fixture, Direction,
    InvalidValue, ReallocFixture, NAN_DOCUMENTED_FLOAT_FIELD,
};
use ilium_plugin_api::error::ErrorCode;

/// REQ: ABI-081
#[test]
fn abi_081_unknown_payload_members_are_ignored_by_host_and_sdk_readers() {
    let payload = r#"{"plugin":"fixture","api":"0.1.0","unknown-future-member":{"nested":42}}"#;
    let host_reading =
        parse_host_payload_host(payload).expect("the host must ignore an unknown member");
    let sdk_reading =
        parse_host_payload_sdk(payload).expect("the SDK must ignore an unknown member");
    assert_eq!(
        host_reading.plugin, "fixture",
        "host parsing must still read known members"
    );
    assert_eq!(
        sdk_reading.plugin, "fixture",
        "SDK parsing must still read known members"
    );
}

/// REQ: ABI-100
#[test]
fn abi_100_invalid_by_construction_values_trap_once_and_log_the_type() {
    let kinds = [
        InvalidValue::BoolByte,
        InvalidValue::CharSurrogate,
        InvalidValue::CharAboveMax,
        InvalidValue::DiscriminantWithoutCase,
        InvalidValue::OptionTagAboveOne,
        InvalidValue::ResultTagAboveOne,
        InvalidValue::FlagsUndefinedBit,
    ];
    for kind in kinds {
        for direction in [Direction::PluginToHost, Direction::HostToPlugin] {
            let outcome = lift_invalid(kind, direction);
            assert!(
                outcome.trap_code.is_some(),
                "{kind:?} {direction:?}: the call must trap at the boundary"
            );
            assert_eq!(
                outcome.counted_failures, 1,
                "{kind:?} {direction:?}: one counted failure"
            );
            let logs: Vec<_> = outcome
                .log_records
                .iter()
                .filter(|record| record.code == "abi.invalid-value")
                .collect();
            assert_eq!(
                logs.len(),
                1,
                "{kind:?} {direction:?}: exactly one abi.invalid-value record"
            );
            assert!(
                logs[0].message.contains(kind.type_name()),
                "{kind:?} {direction:?}: the record must name the type"
            );
        }
    }
}

/// REQ: ABI-100
#[test]
fn abi_100_every_invalid_kind_traps_with_only_the_invalid_value_code() {
    let kinds = [
        InvalidValue::BoolByte,
        InvalidValue::CharSurrogate,
        InvalidValue::CharAboveMax,
        InvalidValue::DiscriminantWithoutCase,
        InvalidValue::OptionTagAboveOne,
        InvalidValue::ResultTagAboveOne,
        InvalidValue::FlagsUndefinedBit,
    ];
    for kind in kinds {
        for direction in [Direction::PluginToHost, Direction::HostToPlugin] {
            let outcome = lift_invalid(kind, direction);
            assert_eq!(
                outcome.trap_code.as_deref(),
                Some("abi.invalid-value"),
                "{kind:?} {direction:?}: the trap must carry the abi.invalid-value code"
            );
            assert!(
                outcome
                    .log_records
                    .iter()
                    .all(|record| record.code == "abi.invalid-value"),
                "{kind:?} {direction:?}: no record other than abi.invalid-value may be logged"
            );
        }
    }
}

/// REQ: ABI-101
#[test]
fn abi_101_nan_refused_outside_nan_documented_fields_and_never_sent_to_plugin() {
    let refused = accept_plugin_float("percent", f64::NAN)
        .expect_err("a NaN in a percent field must be refused");
    assert_eq!(
        refused.code,
        ErrorCode::InvalidArgument,
        "the refusal must be invalid-argument"
    );
    assert!(
        accept_plugin_float(NAN_DOCUMENTED_FLOAT_FIELD, f64::NAN).is_ok(),
        "a NaN in a field documented to accept NaN must be accepted"
    );
    for (field, value) in host_float_fields_in_fixture() {
        assert!(
            value.is_finite(),
            "host-produced float field {field} must never be NaN or infinity, got {value}"
        );
    }
}

/// REQ: ABI-102
#[test]
fn abi_102_invalid_realloc_results_trap_and_realloc_runs_only_inside_host_calls() {
    for (fixture, label) in [
        (ReallocFixture::ReturnsZero, "zero"),
        (ReallocFixture::OutOfRange, "out-of-range"),
        (ReallocFixture::Unaligned, "unaligned"),
    ] {
        let run = run_realloc_fixture(fixture);
        assert_eq!(
            run.trap_code.as_deref(),
            Some("abi.realloc-failed"),
            "a {label} pointer from cabi_realloc must trap with abi.realloc-failed"
        );
    }
    let valid = run_realloc_fixture(ReallocFixture::Valid);
    assert_eq!(
        valid.realloc_calls_outside_host_call, 0,
        "cabi_realloc must be called only during a host-to-plugin call"
    );
}

/// REQ: ABI-105
#[test]
fn abi_105_import_call_limit_returns_busy_after_100000_without_trapping() {
    let run = run_import_loop(100_001);
    assert!(
        !run.trapped,
        "exceeding the import-call limit must not trap the export call"
    );
    for (index, result) in run.results.iter().enumerate().take(100_000) {
        assert!(
            result.is_ok(),
            "import call {index} is within the limit and must succeed"
        );
    }
    for (index, result) in run.results.iter().enumerate().skip(100_000) {
        let error = result
            .as_ref()
            .expect_err("call 100,001 and later must be refused");
        assert_eq!(error.code, ErrorCode::Busy, "call {index} must return busy");
        assert!(
            error.message.contains("import-calls-exceeded"),
            "call {index} must carry the reason import-calls-exceeded"
        );
    }
}

/// REQ: ABI-114
#[test]
fn abi_114_slow_import_logged_with_name_and_duration_and_slow_call_returns_request_id() {
    let fast_name = run_host_import("probe-import", 60, false);
    let slow_logs: Vec<_> = fast_name
        .logs
        .iter()
        .filter(|record| record.code == "abi.slow-import")
        .collect();
    assert_eq!(
        slow_logs.len(),
        1,
        "a 60 ms import that is not a slow call must log abi.slow-import"
    );
    assert_eq!(
        slow_logs[0].fields.get("name").map(String::as_str),
        Some("probe-import")
    );
    assert!(
        slow_logs[0]
            .fields
            .get("duration_ms")
            .is_some_and(|value| !value.is_empty()),
        "the abi.slow-import record must carry the duration"
    );

    let named_slow = run_host_import("slow-probe", 60, true);
    assert!(
        named_slow.request_id.is_some(),
        "a slow call named by the page must return a request-id"
    );
}

/// REQ: ABI-081
#[test]
fn abi_081_a_payload_without_optional_members_still_parses_on_both_sides() {
    let payload = r#"{"plugin":"fixture","api":"0.1.0"}"#;
    assert!(
        parse_host_payload_host(payload).is_ok(),
        "the host must accept a payload that omits optional members"
    );
    assert!(
        parse_host_payload_sdk(payload).is_ok(),
        "the SDK must accept a payload that omits optional members"
    );
}

/// REQ: ABI-101
#[test]
fn abi_101_a_finite_float_is_accepted_in_a_field_that_does_not_document_nan() {
    assert!(
        accept_plugin_float("percent", 0.5).is_ok(),
        "a finite value in a non-NaN field must be accepted"
    );
}

/// REQ: ABI-102
#[test]
fn abi_102_a_valid_realloc_pointer_does_not_trap() {
    let valid = run_realloc_fixture(ReallocFixture::Valid);
    assert!(
        valid.trap_code.is_none(),
        "a valid pointer from cabi_realloc must not trap the call"
    );
}

/// REQ: ABI-105
#[test]
fn abi_105_exactly_the_limit_of_import_calls_all_succeed() {
    let run = run_import_loop(100_000);
    assert!(!run.trapped, "a call at exactly the limit must not trap");
    assert!(
        run.results.iter().all(|result| result.is_ok()),
        "every one of the 100,000 import calls is within the limit"
    );
}

/// REQ: ABI-114
#[test]
fn abi_114_a_slow_import_not_named_as_slow_returns_no_request_id() {
    let unnamed = run_host_import("probe-import", 60, false);
    assert!(
        unnamed.request_id.is_none(),
        "only an import the page names as slow may return a request-id"
    );
}
