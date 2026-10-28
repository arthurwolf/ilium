//! Twins of `ilium-client/src/voice_preparation.rs`. The monolith prepared the realtime session context on a
//! native worker with an explicit byte bank; the plugin observes the same behaviour as: the system prompt the
//! realtime session is built with, the transcription/relay jobs it starts and the captures it opens.
//! See API_GAPS_svc01.md (items 12 to 16).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::json;

use crate::common::{configure, enabled_client, f8_grab, key_pressed, key_released};
use crate::host_ext::*;

fn busy() -> PluginError {
    PluginError {
        code: ErrorCode::Busy,
        message: "bank busy".to_string(),
        retry_after_ms: Some(50),
    }
}

fn press_and_release(host: &mut FakeHost) {
    let grab = f8_grab(host);
    host.deliver_one(key_pressed(grab, false));
    host.deliver_one(key_released(grab));
    host.settle();
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::actual_cpu_prepares_original_context_and_source_survives_controller_drop
#[test]
fn port_024_the_prepared_context_carries_the_original_prompt() {
    let mut host = enabled_client("push_to_talk");
    configure(
        &mut host,
        "custom_prompt",
        json!("Always answer in French."),
    );
    assert!(host
        .realtime()
        .system_prompt()
        .contains("Always answer in French."));
    configure(&mut host, "enabled", json!(false));
    configure(&mut host, "enabled", json!(true));
    assert!(
        host.realtime()
            .system_prompt()
            .contains("Always answer in French."),
        "the prompt survives a restart"
    );
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::busy_bank_retries_exact_capture_without_recloning_settings_or_recharging_source
#[test]
fn port_025_a_busy_start_is_retried_with_the_same_capture_request() {
    let mut host = enabled_client("push_to_talk");
    host.inject_failure("jobs.start", busy());
    press_and_release(&mut host);
    host.advance_clock(100);
    host.settle();
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(host.audio().captures().len(), 1);
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::cancel_disposes_stale_preparation_without_publishing_or_actor_start
#[test]
fn port_026_cancelling_before_the_result_publishes_nothing() {
    let mut host = enabled_client("push_to_talk");
    press_and_release(&mut host);
    let request = host.jobs().list().last().expect("job").request;
    host.jobs().request_cancel();
    host.settle();
    assert!(host.jobs().cancelled().contains(&request));
    host.jobs().finish(request, Ok(b"too late".to_vec()));
    host.settle();
    assert_eq!(host.outcomes().len(), 0);
    assert_eq!(host.submitted().len(), 0);
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::deferred_actual_cpu_result_retries_same_original_then_stop_releases_source
#[test]
fn port_027_a_deferred_result_is_retried_and_stopping_releases_the_capture() {
    let mut host = enabled_client("push_to_talk");
    press_and_release(&mut host);
    let request = host.jobs().list().last().expect("job").request;
    host.jobs().finish(request, Err(busy()));
    host.advance_clock(100);
    host.settle();
    assert_eq!(host.jobs().list().len(), 2, "one retry of the same request");
    configure(&mut host, "enabled", json!(false));
    assert!(
        host.audio().captures().iter().all(|capture| !capture.open)
            && !host.audio().captures().is_empty()
    );
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::newer_capture_fences_deferred_actual_cpu_original
#[test]
fn port_028_a_newer_capture_fences_the_result_of_an_older_one() {
    let mut host = enabled_client("push_to_talk");
    press_and_release(&mut host);
    let older = host.jobs().list().last().expect("first job").request;
    press_and_release(&mut host);
    let newer = host.jobs().list().last().expect("second job").request;
    assert_ne!(older, newer);
    host.jobs().finish(older, Ok(b"older words".to_vec()));
    host.settle();
    assert_eq!(host.submitted().len(), 0, "the older transcript is dropped");
    host.jobs().finish(newer, Ok(b"newer words".to_vec()));
    host.settle();
    assert_eq!(host.realtime().typed_text(), ["newer words"]);
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::newer_startup_wins_and_audited_payload_is_ready_without_starting_provider
#[test]
fn port_029_a_newer_start_wins_and_nothing_starts_before_the_audio_arrives() {
    let mut host = enabled_client("push_to_talk");
    configure(&mut host, "model", json!("gpt-realtime-mini"));
    configure(&mut host, "model", json!("gpt-realtime"));
    assert_eq!(
        host.jobs().list().len(),
        0,
        "preparing a context starts no provider work"
    );
    assert_eq!(host.audio().captures().len(), 0);
    assert_eq!(
        host.kernel_settings().get("voice-control", "model"),
        Some(json!("gpt-realtime"))
    );
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::actual_cpu_prepares_original_context_and_source_survives_controller_drop
#[test]
fn port_024_a_changed_prompt_replaces_the_one_the_context_was_built_with() {
    let mut host = enabled_client("push_to_talk");
    configure(
        &mut host,
        "custom_prompt",
        json!("Always answer in French."),
    );
    configure(&mut host, "custom_prompt", json!("Answer briefly."));
    let prompt = host.realtime().system_prompt();
    assert!(prompt.contains("Answer briefly."));
    assert!(!prompt.contains("French"), "{prompt}");
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::busy_bank_retries_exact_capture_without_recloning_settings_or_recharging_source
#[test]
fn port_025_repeated_retries_of_a_busy_start_never_open_a_second_capture() {
    let mut host = enabled_client("push_to_talk");
    host.inject_failure("jobs.start", busy());
    press_and_release(&mut host);
    for _ in 0..3 {
        host.advance_clock(100);
        host.settle();
    }
    assert_eq!(host.audio().captures().len(), 1);
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::cancel_disposes_stale_preparation_without_publishing_or_actor_start
#[test]
fn port_026_a_new_press_after_a_cancel_gets_its_own_request_and_the_cancelled_one_stays_cancelled()
{
    let mut host = enabled_client("push_to_talk");
    press_and_release(&mut host);
    let cancelled = host.jobs().list().last().expect("first job").request;
    host.jobs().request_cancel();
    host.settle();
    press_and_release(&mut host);
    let fresh = host.jobs().list().last().expect("second job").request;
    assert_ne!(cancelled, fresh);
    assert!(host.jobs().cancelled().contains(&cancelled));
    assert!(!host.jobs().cancelled().contains(&fresh));
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::deferred_actual_cpu_result_retries_same_original_then_stop_releases_source
#[test]
fn port_027_the_retry_of_a_deferred_result_reuses_the_original_request_id() {
    let mut host = enabled_client("push_to_talk");
    press_and_release(&mut host);
    let request = host.jobs().list().last().expect("job").request;
    host.jobs().finish(request, Err(busy()));
    host.advance_clock(100);
    host.settle();
    let retry = host.jobs().list().last().expect("retry").request;
    assert_eq!(retry, request);
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::newer_capture_fences_deferred_actual_cpu_original
#[test]
fn port_028_a_newer_result_that_finishes_first_is_kept_when_the_older_one_finishes_later() {
    let mut host = enabled_client("push_to_talk");
    press_and_release(&mut host);
    let older = host.jobs().list().last().expect("first job").request;
    press_and_release(&mut host);
    let newer = host.jobs().list().last().expect("second job").request;
    host.jobs().finish(newer, Ok(b"newer words".to_vec()));
    host.settle();
    host.jobs().finish(older, Ok(b"older words".to_vec()));
    host.settle();
    assert_eq!(host.realtime().typed_text(), ["newer words"]);
}

/// REQ: PORT:ilium-client/src/voice_preparation.rs::newer_startup_wins_and_audited_payload_is_ready_without_starting_provider
#[test]
fn port_029_a_press_after_two_model_changes_starts_exactly_one_job() {
    let mut host = enabled_client("push_to_talk");
    configure(&mut host, "model", json!("gpt-realtime-mini"));
    configure(&mut host, "model", json!("gpt-realtime"));
    press_and_release(&mut host);
    assert_eq!(host.jobs().list().len(), 1);
}
