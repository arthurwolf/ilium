//! api-exports.md: the `job-runner.run` and `settings-validate.settings-validate` exports.

use std::time::Duration;

use ilium_plugin_api::generated::host::HostJobs;
use ilium_plugin_api::generated::host::FieldError;
use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::{ImportsFixture, UserInput, UserSave};
use serde_json::json;

use crate::support::*;

const DEADLINE_MESSAGE: &str = "Could not check this value: Imports did not answer.";

fn validating(
    validate: Box<dyn FnMut(&str, &str) -> Result<(), Vec<FieldError>>>,
) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(validate)
        .build()
}

fn job_manifest() -> String {
    let mut manifest = minimal_manifest();
    manifest["contributes"] = json!({ "jobs": [{ "kind": "work", "title": "Work" }] });
    manifest.to_string()
}

// ---- job-runner.run --------------------------------------------------------------------------

/// REQ: WIT:ilium:exports/job-runner.run
#[test]
fn job_runner_run_receives_the_kind_and_the_input_bytes() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .grants(&["jobs.run"])
        .manifest_json(&job_manifest())
        .job_runner(Box::new(move |kind, input| {
            sink.lock().unwrap().push((kind.to_owned(), input.to_vec()));
            Ok(Vec::new())
        }))
        .build();
    let job = expect_ok(fixture.jobs().start("work".to_owned(), b"payload".to_vec()));
    complete(&mut fixture, job);
    assert_eq!(*seen.lock().unwrap(), vec![("work".to_owned(), b"payload".to_vec())]);
}

/// REQ: WIT:ilium:exports/job-runner.run
#[test]
fn job_runner_run_ok_bytes_become_the_job_output() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .grants(&["jobs.run"])
        .manifest_json(&job_manifest())
        .job_runner(Box::new(|_, _| Ok(b"result".to_vec())))
        .build();
    let job = expect_ok(fixture.jobs().start("work".to_owned(), Vec::new()));
    assert!(is_ok(&complete(&mut fixture, job)));
    assert_eq!(fixture.jobs().take_output(job), Some(b"result".to_vec()));
}

/// REQ: WIT:ilium:exports/job-runner.run
#[test]
fn job_runner_run_every_error_code_becomes_the_request_failure_code() {
    for code in [
        ErrorCode::NotFound,
        ErrorCode::InvalidArgument,
        ErrorCode::Conflict,
        ErrorCode::QuotaExceeded,
        ErrorCode::Unsupported,
        ErrorCode::Io,
        ErrorCode::Internal,
    ] {
        let mut fixture = ImportsFixture::builder()
            .tier(Tier::T2)
            .grants(&["jobs.run"])
            .manifest_json(&job_manifest())
            .job_runner(Box::new(move |_, _| {
                Err(PluginError { code, message: "x".to_owned(), retry_after_ms: None })
            }))
            .build();
        let job = expect_ok(fixture.jobs().start("work".to_owned(), Vec::new()));
        assert_eq!(outcome_code(&complete(&mut fixture, job)), Some(code), "{code:?}");
    }
}

/// REQ: WIT:ilium:exports/job-runner.run
#[test]
fn job_runner_run_error_message_is_kept() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .grants(&["jobs.run"])
        .manifest_json(&job_manifest())
        .job_runner(Box::new(|_, _| {
            Err(PluginError { code: ErrorCode::Io, message: "disk gone".to_owned(), retry_after_ms: None })
        }))
        .build();
    let job = expect_ok(fixture.jobs().start("work".to_owned(), Vec::new()));
    match complete(&mut fixture, job) {
        ilium_plugin_api::generated::types::Outcome::Failed(error) => assert_eq!(error.message, "disk gone"),
        other => panic!("expected failed, got {other:?}"),
    }
}

// ---- settings-validate -----------------------------------------------------------------------

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_ok_lets_the_save_through() {
    let mut fixture = validating(Box::new(|_patch, _merged| Ok(())));
    assert_eq!(fixture.user_save(r#"{"label": "fine"}"#), UserSave::Saved);
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_field_error_refuses_the_save_with_its_message() {
    let mut fixture = validating(Box::new(|_patch, _merged| {
        Err(vec![FieldError { key: "label".to_owned(), message: "Label is taken.".to_owned() }])
    }));
    assert_eq!(fixture.user_save(r#"{"label": "x"}"#), UserSave::Refused("Label is taken.".to_owned()));
    assert_eq!(fixture.settings_value("label"), json!("work"));
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_receives_the_patch_and_the_merged_object() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut fixture = validating(Box::new(move |patch, merged| {
        sink.lock().unwrap().push((patch.to_owned(), merged.to_owned()));
        Ok(())
    }));
    fixture.user_save(r#"{"count": 7}"#);
    let calls = seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let patch: serde_json::Value = serde_json::from_str(&calls[0].0).unwrap();
    let merged: serde_json::Value = serde_json::from_str(&calls[0].1).unwrap();
    assert_eq!(patch, json!({ "count": 7 }));
    assert_eq!(merged["count"], json!(7));
    assert_eq!(merged["label"], json!("work"));
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_answering_within_100_ms_is_honoured() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(|_, _| Ok(())))
        .settings_validate_latency(Duration::from_millis(99))
        .build();
    assert_eq!(fixture.user_save(r#"{"label": "fine"}"#), UserSave::Saved);
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_answering_after_100_ms_refuses_with_the_did_not_answer_message() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&runtime_settings_manifest().to_string())
        .settings_validate(Box::new(|_, _| Ok(())))
        .settings_validate_latency(Duration::from_millis(101))
        .build();
    assert_eq!(fixture.user_save(r#"{"label": "slow"}"#), UserSave::Refused(DEADLINE_MESSAGE.to_owned()));
    assert_eq!(fixture.settings_value("label"), json!("work"));
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_a_trap_refuses_with_the_did_not_answer_message() {
    let mut fixture = validating(Box::new(|_, _| Ok(())));
    fixture.make_next_validate_trap();
    assert_eq!(fixture.user_save(r#"{"label": "x"}"#), UserSave::Refused(DEADLINE_MESSAGE.to_owned()));
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_a_stopped_plugin_refuses_with_the_did_not_answer_message() {
    let mut fixture = validating(Box::new(|_, _| Ok(())));
    fixture.stop_plugin();
    assert_eq!(fixture.user_save(r#"{"label": "x"}"#), UserSave::Refused(DEADLINE_MESSAGE.to_owned()));
}

/// REQ: WIT:ilium:exports/settings-validate.settings-validate
#[test]
fn settings_validate_runs_after_the_schema_check_and_only_for_schema_valid_values() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = calls.clone();
    let mut fixture = validating(Box::new(move |_, _| {
        *counter.lock().unwrap() += 1;
        Ok(())
    }));
    let outcome = fixture.user_edit("count", "99", UserInput::NumberStep);
    assert!(matches!(outcome, UserSave::Refused(_)));
    assert_eq!(*calls.lock().unwrap(), 0);
}
