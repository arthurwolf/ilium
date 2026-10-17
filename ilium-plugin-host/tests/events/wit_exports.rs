//! WIT exports and the submit import (wit/exports/handler.wit, lifecycle.wit, host/intents-host.wit).

use std::time::Duration;

use ilium_plugin_api::generated::types::{DeactivationReason, ErrorCode};

use crate::common::*;

/// REQ: WIT:ilium:exports/handler.on-events
#[test]
fn wit_handler_on_events_receives_the_batch_and_returns_submissions() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    assert_eq!(env.probe.count_of("timer-fired"), 1);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: WIT:ilium:exports/handler.on-events
#[test]
fn wit_handler_on_events_signature_matches_the_wit_file() {
    let text = wit_file("exports/handler.wit");
    assert!(
        text.contains("on-events: func(events: list<event>) -> list<submission>"),
        "{text}"
    );
}

/// REQ: WIT:ilium:exports/handler.on-events
#[test]
fn wit_handler_on_events_more_than_256_returned_submissions_trap() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run((1..=257).map(|n| sub(n, rename(2, "t"))).collect());
    assert_eq!(env.host.failures().len(), 1);
}

/// REQ: WIT:ilium:exports/lifecycle.activate
#[test]
fn wit_lifecycle_activate_is_called_once_before_any_event() {
    let env = Setup::new(&[]).build();
    env.deliver(vec![trigger()]);
    let sequence = lock(&env.probe.sequence).clone();
    assert_eq!(sequence.first().map(String::as_str), Some("activate"));
    assert_eq!(env.probe.activation_count(), 1);
}

/// REQ: WIT:ilium:exports/lifecycle.activate
#[test]
fn wit_lifecycle_activate_error_fails_the_activation() {
    let setup = Setup::new(&[]);
    *lock(&setup.probe.activate_error) = Some(ilium_plugin_api::PluginError {
        code: ErrorCode::Internal,
        message: "no".to_owned(),
        retry_after_ms: None,
    });
    let (built, _probe, _kernel) = setup.try_build();
    assert!(built.is_err());
}

/// REQ: WIT:ilium:exports/lifecycle.activate
#[test]
fn wit_lifecycle_activate_signature_matches_the_wit_file() {
    let text = wit_file("exports/lifecycle.wit");
    assert!(text.contains("activate: func("), "{text}");
}

/// REQ: WIT:ilium:exports/lifecycle.deactivate
#[test]
fn wit_lifecycle_deactivate_is_called_once_on_a_clean_stop() {
    let mut env = Setup::new(&[]).build();
    env.host.deactivate_plugin();
    env.settle();
    assert_eq!(lock(&env.probe.deactivations).len(), 1);
}

/// REQ: WIT:ilium:exports/lifecycle.deactivate
#[test]
fn wit_lifecycle_deactivate_carries_the_reason() {
    let env = Setup::new(&[]).build();
    env.host.restart_instance(DeactivationReason::CrashRestart);
    env.settle();
    assert_eq!(
        lock(&env.probe.deactivations).first().cloned(),
        Some(DeactivationReason::CrashRestart)
    );
}

/// REQ: WIT:ilium:exports/lifecycle.deactivate
#[test]
fn wit_lifecycle_deactivate_signature_matches_the_wit_file() {
    let text = wit_file("exports/lifecycle.wit");
    assert!(text.contains("deactivate: func("), "{text}");
}

/// REQ: WIT:ilium:host/intents-host.submit
#[test]
fn wit_intents_host_submit_returns_a_request_id_and_completes_once() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    let request = env.host.submit_directly(sub(1, rename(2, "a")));
    assert!(request.is_ok());
    env.settle();
    assert_eq!(env.probe.count_of("request-completed"), 1);
}

/// REQ: WIT:ilium:host/intents-host.submit
#[test]
fn wit_intents_host_submit_without_the_grant_is_err_not_permitted_at_once() {
    let env = Setup::new(&[]).tree(flat_tree(1)).build();
    let request = env.host.submit_directly(sub(1, rename(2, "a")));
    assert_eq!(request.err(), Some(ErrorCode::NotPermitted));
}

/// REQ: WIT:ilium:host/intents-host.submit
#[test]
fn wit_intents_host_submit_signature_matches_the_wit_file() {
    let text = wit_file("host/intents-host.wit");
    assert!(
        text.contains("submit: func(submission: submission) -> result<request-id, error>"),
        "{text}"
    );
}

#[allow(dead_code)]
fn unused(_: Duration) {}
