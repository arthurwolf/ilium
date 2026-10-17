//! Twins of the delivery tests of `ilium-server/src/notifications.rs`. The bounded execution bank and the I/O
//! admission are host internals; the plugin observes the notice it submits and the outcome the host completes it
//! with. See API_GAPS_svc01.md.

use ilium_plugin_sdk::types::{AgentActivity, ErrorCode, Outcome, PluginError};

use crate::common::*;

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError { code, message: message.to_string(), retry_after_ms: None }
}

/// REQ: PORT:ilium-server/src/notifications.rs::backend_refusal_remains_a_delivery_result
#[test]
fn port_030_a_refused_notice_is_a_delivery_result_not_a_plugin_failure() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(notices(&finish_turn(&mut host, node, 1)).len(), 1);
    let request = host.pending_requests().remove(0);
    host.complete(request, Outcome::Failed(error(ErrorCode::Unsupported, "backend unavailable")));
    host.settle();
    assert_eq!(host.failures().len(), 0);
    let again = add_pane(&mut host, "claude-two", Some(AgentActivity::Working));
    assert_eq!(notices(&finish_turn(&mut host, again, 2)).len(), 1, "later notices are still delivered");
}

/// REQ: PORT:ilium-server/src/notifications.rs::notification_delivery_is_owned_by_the_bounded_execution_bank
#[test]
fn port_031_every_notice_is_submitted_once_and_in_the_order_raised() {
    let mut host = server_host();
    let first = add_pane(&mut host, "first", Some(AgentActivity::Working));
    let second = add_pane(&mut host, "second", Some(AgentActivity::Working));
    let one = notices(&finish_turn(&mut host, first, 1));
    let two = notices(&finish_turn(&mut host, second, 2));
    assert_eq!(one.len(), 1);
    assert_eq!(two.len(), 1);
    assert!(one[0].body.as_deref().unwrap_or_default().contains("first") || one[0].title.contains("first"));
    assert!(two[0].body.as_deref().unwrap_or_default().contains("second") || two[0].title.contains("second"));
}

/// REQ: PORT:ilium-server/src/notifications.rs::notification_is_dropped_when_io_admission_is_saturated
#[test]
fn port_032_a_notice_refused_for_busy_admission_is_dropped_without_retry() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    let before = host.submitted().len();
    let request = host.pending_requests().remove(0);
    host.complete(request, Outcome::Refused(error(ErrorCode::Busy, "admission saturated")));
    host.advance_clock(60_000);
    host.settle();
    assert_eq!(host.submitted().len(), before, "the dropped notice is not resubmitted");
}

/// REQ: PORT:ilium-server/src/notifications.rs::oversized_notification_is_refused_before_execution_admission
#[test]
fn port_033_an_oversized_name_never_produces_an_oversized_attention_reason() {
    let mut host = server_host();
    let long_name = "p".repeat(5000);
    let node = add_pane(&mut host, &long_name, Some(AgentActivity::Working));
    let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1);
    let reasons = marks(&submissions);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].reason.chars().count() <= 80, "{}", reasons[0].reason.len());
}
