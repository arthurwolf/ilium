//! Runtime ABI rules checked against the host harness (ABI-014, 025, 035, 037, 038, 040, 041,
//! 043, 046, 047, 049, 050). Each test drives the harness through the public surface named in
//! API_GAPS.md and asserts the exact observable result.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilium_plugin_api::generated::types::{
    DeactivationReason, ErrorCode, Event, Intent, InvokeCommandSpec, PluginError, RequestId,
    RequestRef, Submission,
};
use ilium_plugin_api::Code;
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::{HarnessHandle, HostHarness, NativeGuest};

use crate::common::*;

/// A guest that accepts every batch and returns no submission.
struct IdleGuest;

impl NativeGuest for IdleGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(Vec::new())
    }
}

/// A T3 server plugin that may call one http host and play sounds.
fn http_host() -> HarnessHandle {
    HostHarness::new()
        .grants(&["http:example.com", "sound.play"])
        .with_native_guest(Box::new(IdleGuest))
        .build()
        .expect("the harness builds a host for a native guest")
}

/// A guest that records the number of exports on the stack at every entry. It returns one
/// own-command invocation from its first batch.
struct DepthGuest {
    depth: Arc<AtomicUsize>,
    deepest: Arc<AtomicUsize>,
    batches: Arc<Mutex<Vec<Vec<Event>>>>,
    command: Option<Intent>,
}

impl NativeGuest for DepthGuest {
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        let now = self.depth.fetch_add(1, Ordering::SeqCst) + 1;
        self.deepest.fetch_max(now, Ordering::SeqCst);
        lock(&self.batches).push(batch);
        let reply = match self.command.take() {
            Some(intent) => vec![sub(1, intent)],
            None => Vec::new(),
        };
        self.depth.fetch_sub(1, Ordering::SeqCst);
        Ok(reply)
    }
}

/// Every batch a guest received, in delivery order.
type RecordedBatches = Arc<Mutex<Vec<Vec<Event>>>>;

fn depth_host(command: Option<Intent>) -> (HarnessHandle, Arc<AtomicUsize>, RecordedBatches) {
    let deepest = Arc::new(AtomicUsize::new(0));
    let batches = Arc::new(Mutex::new(Vec::new()));
    let guest = DepthGuest {
        depth: Arc::new(AtomicUsize::new(0)),
        deepest: deepest.clone(),
        batches: batches.clone(),
        command,
    };
    let host = HostHarness::new()
        .grants(&["commands.invoke"])
        .with_native_guest(Box::new(guest))
        .build()
        .expect("the harness builds a host for a native guest");
    (host, deepest, batches)
}

fn invoke_own_command() -> Intent {
    Intent::InvokeCommand(InvokeCommandSpec {
        command: "own.command".to_owned(),
        arguments_json: "{}".to_owned(),
    })
}

fn is_command_invoked(event: &Event) -> bool {
    matches!(event, Event::CommandInvoked(_))
}

// ---------------------------------------------------------------- ABI-014

/// REQ: ABI-014
#[test]
fn abi_014_take_before_completion_is_not_found() {
    let host = http_host();
    let request = host
        .start_slow_request()
        .request_id()
        .expect("a request id");
    assert_eq!(
        host.take_result(RequestId(request)),
        Err(ErrorCode::NotFound)
    );
}

/// REQ: ABI-014
#[test]
fn abi_014_second_take_is_not_found() {
    let env = writer_env();
    let request = env
        .host
        .submit_directly(sub(1, rename(2, "a")))
        .expect("id");
    env.settle();
    assert!(env.host.take_result(request).is_ok());
    assert_eq!(env.host.take_result(request), Err(ErrorCode::NotFound));
}

/// REQ: ABI-014
#[test]
fn abi_014_take_of_an_unknown_id_is_not_found() {
    let env = writer_env();
    env.settle();
    assert_eq!(
        env.host.take_result(RequestId(987_654)),
        Err(ErrorCode::NotFound)
    );
}

fn writer_env() -> Env {
    Setup::new(&["tree.write"]).tree(flat_tree(1)).build()
}

// ---------------------------------------------------------------- ABI-025 and ABI-049

/// REQ: ABI-025
#[test]
fn abi_025_an_own_command_from_on_events_never_runs_inside_the_export() {
    let (host, deepest, batches) = depth_host(Some(invoke_own_command()));
    host.deliver(vec![trigger()]);
    host.pump(64);
    assert_eq!(deepest.load(Ordering::SeqCst), 1);
    let seen = lock(&batches).clone();
    let invoked: usize = seen
        .iter()
        .flatten()
        .filter(|e| is_command_invoked(e))
        .count();
    assert_eq!(
        invoked, 1,
        "the command runs, but in a later batch: {seen:?}"
    );
}

/// REQ: ABI-049
#[test]
fn abi_049_no_export_is_entered_while_another_is_on_the_stack() {
    let (host, deepest, batches) = depth_host(Some(invoke_own_command()));
    host.deliver(vec![trigger()]);
    host.pump(64);
    host.deliver(vec![trigger()]);
    host.pump(64);
    assert_eq!(deepest.load(Ordering::SeqCst), 1);
    let seen = lock(&batches).clone();
    // manual does not pin the exact batch count; each trigger yields one own-command event
    assert!(seen.len() >= 2, "{seen:?}");
    let invoked: usize = seen
        .iter()
        .flatten()
        .filter(|e| is_command_invoked(e))
        .count();
    assert_eq!(invoked, 2, "{seen:?}");
}

// ---------------------------------------------------------------- ABI-050

/// REQ: ABI-050
#[test]
fn abi_050_own_command_invoked_in_on_events_arrives_in_a_later_batch() {
    let (host, _deepest, batches) = depth_host(Some(invoke_own_command()));
    host.deliver(vec![trigger()]);
    host.pump(64);
    let seen = lock(&batches).clone();
    assert!(!seen[0].iter().any(is_command_invoked), "{:?}", seen[0]);
    let later = seen
        .iter()
        .skip(1)
        .flatten()
        .filter(|event| is_command_invoked(event))
        .count();
    assert_eq!(later, 1);
}

// ---------------------------------------------------------------- ABI-035

/// REQ: ABI-035
#[test]
fn abi_035_a_repeated_dedup_key_completes_as_duplicate_of_the_first() {
    let env = writer_env();
    env.run(vec![
        keyed(1, "same", rename(2, "a")),
        keyed(2, "same", rename(2, "b")),
    ]);
    assert!(is_ok(&env.outcome(1)));
    assert!(is_duplicate_of(&env.outcome(2), 1));
}

/// REQ: ABI-035
#[test]
fn abi_035_the_host_does_not_check_correlation_uniqueness() {
    let env = writer_env();
    env.run(vec![
        keyed(7, "first", rename(2, "a")),
        keyed(7, "second", rename(2, "b")),
    ]);
    assert_eq!(count_for(&env.outcomes(), 7), 2);
}

// ---------------------------------------------------------------- ABI-037

/// REQ: ABI-037
#[test]
fn abi_037_a_batch_delayed_by_40_s_still_allows_the_take() {
    let env = writer_env();
    let request = env
        .host
        .submit_directly(sub(1, rename(2, "a")))
        .expect("id");
    env.host.advance_clock(Duration::from_secs(40));
    env.settle();
    assert!(env.host.take_result(request).is_ok());
}

/// REQ: ABI-037
#[test]
fn abi_037_the_hold_runs_30_s_from_the_batch_return() {
    let env = writer_env();
    let request = env
        .host
        .submit_directly(sub(1, rename(2, "a")))
        .expect("id");
    env.host.advance_clock(Duration::from_secs(40));
    env.settle();
    env.host.advance_clock(Duration::from_secs(30));
    assert_eq!(env.host.take_result(request), Err(ErrorCode::NotFound));
}

// ---------------------------------------------------------------- ABI-038

/// A guest that traps on its first batch and records every batch afterwards.
struct TrapOnceGuest {
    trapped: bool,
    batches: Arc<Mutex<Vec<Vec<Event>>>>,
}

impl NativeGuest for TrapOnceGuest {
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        if !self.trapped {
            self.trapped = true;
            return Err(Trap {
                code: Code(std::borrow::Cow::Borrowed("trap.panic")),
                interface: "ilium:exports/handler".to_owned(),
                function: "on-events".to_owned(),
            });
        }
        lock(&self.batches).push(batch);
        Ok(Vec::new())
    }
}

fn trapped_host() -> (HarnessHandle, Arc<Mutex<Vec<Vec<Event>>>>, u64) {
    let batches = Arc::new(Mutex::new(Vec::new()));
    let host = HostHarness::new()
        .grants(&["http:example.com", "sound.play"])
        .with_native_guest(Box::new(TrapOnceGuest {
            trapped: false,
            batches: batches.clone(),
        }))
        .build()
        .expect("the harness builds a host for a native guest");
    let call = host
        .start_slow_request()
        .request_id()
        .expect("a request id");
    host.deliver(vec![trigger()]);
    host.pump(64);
    (host, batches, call)
}

/// REQ: ABI-038
#[test]
fn abi_038_surviving_events_are_redelivered_once_at_the_head_of_the_new_queue() {
    let (host, batches, _call) = trapped_host();
    host.pump(64);
    let seen = lock(&batches).clone();
    let redelivered = seen
        .iter()
        .flatten()
        .filter(|event| **event == trigger())
        .count();
    assert_eq!(redelivered, 1);
    assert_eq!(seen[0].first(), Some(&trigger()));
}

/// REQ: ABI-038
#[test]
fn abi_038_completions_of_call_requests_are_discarded_after_a_trap() {
    let (host, _batches, call) = trapped_host();
    host.pump(64);
    assert_eq!(host.failures().len(), 1);
    assert!(host.completion_of(call).is_none());
}

/// REQ: ABI-038
#[test]
fn abi_038_completions_of_submissions_reach_the_new_instance() {
    let kernel_env = writer_env();
    let request = kernel_env
        .host
        .submit_directly(sub(1, rename(2, "a")))
        .expect("id");
    kernel_env
        .host
        .restart_instance(DeactivationReason::CrashRestart);
    kernel_env.settle();
    let completions = kernel_env
        .probe
        .events_of("request-completed")
        .into_iter()
        .filter(|event| matches!(event, Event::RequestCompleted(result) if result.request == RequestRef::Submission(1)))
        .count();
    assert_eq!(completions, 1);
    assert!(kernel_env.host.take_result(request).is_ok());
}

// ---------------------------------------------------------------- ABI-040

/// REQ: ABI-040
#[test]
fn abi_040_the_first_request_id_of_a_server_run_is_1() {
    let host = http_host();
    assert_eq!(host.start_slow_request().request_id(), Some(1));
}

/// REQ: ABI-040
#[test]
fn abi_040_request_ids_are_never_reused_across_a_restart() {
    let host = http_host();
    let before = host.start_slow_request().request_id().expect("first id");
    host.restart_instance(DeactivationReason::CrashRestart);
    host.pump(64);
    let after = host.start_slow_request().request_id().expect("second id");
    assert_eq!(before, 1);
    assert_eq!(after, 2);
}

// ---------------------------------------------------------------- ABI-041

/// REQ: ABI-041
#[test]
fn abi_041_a_refused_submission_after_64_slow_calls_leaves_the_count_at_64() {
    let host = http_host();
    for _ in 0..64 {
        host.start_slow_request();
    }
    let refused = host.submit_directly(sub(1, send_input(99, "x")));
    // ABI-013: the call that exceeds the limit returns busy at once
    assert_eq!(refused, Err(ErrorCode::Busy));
    assert_eq!(host.pending_requests(), 64);
}

/// REQ: ABI-041
#[test]
fn abi_041_a_request_stops_counting_when_its_completion_is_enqueued() {
    let host = http_host();
    host.start_slow_request();
    host.start_slow_request();
    assert_eq!(host.pending_requests(), 2);
    host.complete_oldest_request();
    assert_eq!(host.pending_requests(), 1);
}

// ---------------------------------------------------------------- ABI-043

/// REQ: ABI-043
#[test]
fn abi_043_a_pending_request_completes_failed_revoked_before_deactivate() {
    let host = http_host();
    let request = host
        .start_slow_request()
        .request_id()
        .expect("a request id");
    host.revoke("http:example.com");
    host.pump(64);
    let completion = host.completion_of(request).expect("a completion");
    assert_eq!(completion.failed_code(), Some("revoked"));
}

/// REQ: ABI-043
#[test]
fn abi_043_a_synchronous_call_needing_the_grant_returns_revoked() {
    let host = http_host();
    host.revoke("sound.play");
    host.pump(64);
    let error: PluginError = host
        .call_import_value(
            "ilium:host/sounds.play",
            serde_json::json!({"name": "bell"}),
        )
        .expect_err("the grant is revoked");
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert!(error.message.starts_with("revoked: "), "{}", error.message);
}

// ---------------------------------------------------------------- ABI-046

/// REQ: ABI-046
#[test]
fn abi_046_events_produced_during_activate_arrive_in_the_first_batch() {
    let setup = Setup::new(&["panes.read"]);
    let kernel = setup.kernel.clone();
    *lock(&setup.probe.activate_hook) = Some(Box::new(move || kernel.emit_event(pane_exited(4))));
    let env = setup.build();
    env.settle();
    let batches = lock(&env.probe.batches).clone();
    assert_eq!(
        batches.first().map(|batch| batch.contains(&pane_exited(4))),
        Some(true)
    );
}

/// REQ: ABI-046
#[test]
fn abi_046_events_of_a_failed_activate_are_discarded_without_events_dropped() {
    let setup = Setup::new(&["panes.read"]);
    let kernel = setup.kernel.clone();
    *lock(&setup.probe.activate_hook) = Some(Box::new(move || kernel.emit_event(pane_exited(4))));
    *lock(&setup.probe.activate_error) = Some(PluginError {
        code: ErrorCode::Internal,
        message: "boom".to_owned(),
        retry_after_ms: None,
    });
    let (built, probe, _kernel) = setup.try_build();
    drop(built);
    assert_eq!(probe.count_of("pane-exited"), 0);
    assert_eq!(probe.count_of("events-dropped"), 0);
}

// ---------------------------------------------------------------- ABI-047

/// REQ: ABI-047
#[test]
fn abi_047_the_first_batch_starts_with_one_tree_changed_listing_every_node_in_pre_order() {
    let env = Setup::new(&["tree.read"])
        .tree(vec![folder(1, None), pane(2, Some(1)), pane(3, Some(1))])
        .build();
    env.settle();
    let first = lock(&env.probe.batches)
        .first()
        .cloned()
        .unwrap_or_default();
    assert_eq!(first.first(), Some(&tree_changed(&[1, 2, 3])));
    assert_eq!(tree_changed_lists(&first).len(), 1);
}

/// REQ: ABI-047
#[test]
fn abi_047_every_activation_repeats_the_listing() {
    let env = Setup::new(&["tree.read"])
        .tree(vec![folder(1, None), pane(2, Some(1)), pane(3, Some(1))])
        .build();
    env.settle();
    env.host.restart_instance(DeactivationReason::CrashRestart);
    env.deliver(vec![trigger()]);
    assert_eq!(
        tree_changed_lists(&env.probe.events()),
        vec![vec![1, 2, 3], vec![1, 2, 3]]
    );
}
