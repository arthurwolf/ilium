#![cfg(feature = "testing")]
//! The native harness executes production owners, without pretending to instantiate Wasm.
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::{NodeId, TimerId};
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::{Code, ErrorCode};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::harness::HostHarness;
use ilium_plugin_host::testing::native_guest::NativeGuest;

#[derive(Default)]
struct Seen { threads: Vec<String>, batch_sizes: Vec<usize>, grants: Vec<String>, stopped: usize }
struct Guest { seen: Arc<Mutex<Seen>>, submissions: Vec<Submission>, trap: bool }
impl NativeGuest for Guest {
    fn activate(&mut self, activation: Activation) -> Result<(), ilium_plugin_api::PluginError> {
        let mut seen = self.seen.lock().unwrap();
        seen.grants = activation.granted;
        seen.threads.push(std::thread::current().name().unwrap().into());
        Ok(())
    }
    fn on_events(&mut self, events: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        let mut seen = self.seen.lock().unwrap();
        seen.batch_sizes.push(events.len());
        seen.threads.push(std::thread::current().name().unwrap().into());
        if self.trap { return Err(Trap { code: Code::from_static("trap.panic"), interface: "fixture".into(), function: "on-events".into() }); }
        Ok(std::mem::take(&mut self.submissions))
    }
    fn deactivate(&mut self, _: DeactivationReason) -> Option<Vec<u8>> {
        self.seen.lock().unwrap().stopped += 1;
        Some(vec![1, 2])
    }
}
fn submission(intent: Intent, correlation: u64) -> Submission {
    Submission { intent, correlation, dedup_key: None }
}
#[test]
fn native_calls_run_on_owned_workers_and_stop_once() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let host = HostHarness::new().with_native_guest(Box::new(Guest { seen: seen.clone(), submissions: vec![], trap: false })).build().unwrap();
    host.deliver(vec![Event::TimerFired(TimerId(1))]);
    assert_eq!(host.pump(2).events_delivered, 1);
    assert_eq!(host.deactivate(DeactivationReason::Shutdown), Some(vec![1, 2]));
    assert_eq!(host.deactivate(DeactivationReason::Shutdown), None);
    assert_eq!(host.state(), InstanceState::Stopped);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.stopped, 1);
    assert!(seen.threads.iter().all(|name| name.starts_with("ilium-exec-")));
}
#[test]
fn missing_grant_refuses_entire_returned_batch_before_kernel_mutation() {
    let host = HostHarness::new().grants(&["tree.write"]).with_native_guest(Box::new(Guest {
        seen: Arc::default(), trap: false,
        submissions: vec![submission(Intent::CloseNode(NodeId(99)), 1), submission(Intent::PlaySound("bell".into()), 2)],
    })).build().unwrap();
    host.deliver(vec![Event::TimerFired(TimerId(1))]);
    host.pump(2);
    assert!(host.kernel().applied_intents().is_empty());
    let outcomes = host.received_outcomes();
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().any(|result| matches!(&result.outcome, Outcome::Refused(error) if error.code == ErrorCode::NotPermitted)));
}
#[test]
fn trapped_guest_cannot_submit_or_keep_running() {
    let host = HostHarness::new().with_native_guest(Box::new(Guest { seen: Arc::default(), submissions: vec![], trap: true })).build().unwrap();
    host.deliver(vec![Event::TimerFired(TimerId(1))]);
    assert_eq!(host.pump(1).traps, 1);
    assert_eq!(host.failure_count(), 1);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    assert_eq!(host.submit(submission(Intent::CloseNode(NodeId(1)), 1)), Err(ErrorCode::Cancelled));
}
#[test]
fn manual_clock_is_monotonic_and_empty_component_reports_missing_lifecycle() {
    let host = HostHarness::new().build().unwrap();
    host.advance_clock(Duration::from_millis(42));
    host.inject_clock_jump(Duration::from_secs(500));
    assert_eq!(host.now_monotonic(), Duration::from_millis(42));
    let component = HostHarness::new().with_wat("(component)").build().unwrap();
    assert_eq!(component.state(), InstanceState::RestartScheduled);
    assert_eq!(component.failures()[0].code.as_str(), "trap.abi");
}
#[test]
fn invalid_and_declined_grants_never_enter_activation() {
    assert!(HostHarness::new().grants(&["unknown.capability"]).build().is_err());
    let seen = Arc::new(Mutex::new(Seen::default()));
    let host = HostHarness::new().grants(&["tree.read"]).declined(&["tree.read"]).with_native_guest(Box::new(Guest { seen: seen.clone(), submissions: vec![], trap: false })).build().unwrap();
    assert!(seen.lock().unwrap().grants.is_empty());
    host.deliver(vec![Event::TreeChanged(vec![])]);
    assert!(host.queued_events().is_empty());
}
#[test]
fn event_batches_are_bounded_and_oversized_submission_results_trap() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let host = HostHarness::new().with_native_guest(Box::new(Guest { seen: seen.clone(), submissions: vec![], trap: false })).build().unwrap();
    host.deliver((0..300).map(|number| Event::TimerFired(TimerId(number))).collect());
    assert_eq!(host.pump(3).events_delivered, 300);
    assert_eq!(seen.lock().unwrap().batch_sizes, vec![256, 44]);
    let host = HostHarness::new().grants(&["tree.write"]).with_native_guest(Box::new(Guest {
        seen: Arc::default(), trap: false,
        submissions: (0..257).map(|number| submission(Intent::CloseNode(NodeId(99)), number)).collect(),
    })).build().unwrap();
    host.deliver(vec![Event::TimerFired(TimerId(1))]);
    assert_eq!(host.pump(1).traps, 1);
    assert!(host.kernel().applied_intents().is_empty());
    assert!(host.received_outcomes().is_empty());
}
#[test]
fn scheduler_coalescing_and_never_drop_pressure_preserve_ownership() {
    let host = HostHarness::new().grants(&["tree.read"]).build().unwrap();
    host.deliver(vec![Event::TreeChanged(vec![NodeId(1)])]);
    host.deliver(vec![Event::TreeChanged(vec![NodeId(2), NodeId(1)])]);
    assert_eq!(host.queued_events(), vec![Event::TreeChanged(vec![NodeId(1), NodeId(2)])]);
    assert_eq!(host.pump(1).events_delivered, 1);

    let mut limits = ilium_plugin_api::limits::Limits::default();
    limits.0.insert("event_queue".into(), 1);
    let host = HostHarness::new().limits(limits).grants(&["tree.read"]).build().unwrap();
    host.deliver(vec![Event::NodeRemoved(NodeId(11))]);
    let retry = host.try_deliver(vec![Event::NodeRemoved(NodeId(12)), Event::NodeRemoved(NodeId(13))]).unwrap_err();
    assert_eq!(retry, vec![Event::NodeRemoved(NodeId(12)), Event::NodeRemoved(NodeId(13))]);
    assert_eq!(host.queued_events(), vec![Event::NodeRemoved(NodeId(11))]);
    assert_eq!(host.pump(1).events_delivered, 1);
    let retry = host.try_deliver(retry).unwrap_err();
    assert_eq!(retry, vec![Event::NodeRemoved(NodeId(13))]);
    assert_eq!(host.queued_events(), vec![Event::NodeRemoved(NodeId(12))]);
    host.pump(1);
    assert!(host.try_deliver(retry).is_ok());
}

#[test]
fn repeated_completion_backpressure_retains_every_outcome_until_guest_accepts_it() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Weak;
    use ilium_plugin_api::generated::types::requests::RequestRef;
    use ilium_plugin_host::testing::harness::HarnessHandle;

    struct RefillingGuest {
        host: Arc<Mutex<Option<Weak<HarnessHandle>>>>,
        keep_full: Arc<AtomicBool>,
        submitted: Arc<AtomicU64>,
        next_timer: u64,
    }
    impl NativeGuest for RefillingGuest {
        fn on_events(&mut self, _: Vec<Event>) -> Result<Vec<Submission>, Trap> {
            if !self.keep_full.load(Ordering::SeqCst) { return Ok(Vec::new()); }
            let host = self.host.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
            // Fill the space freed by this callback before its returned submissions
            // complete. Each timer identity is distinct, so none coalesces away.
            let events = (self.next_timer..self.next_timer + 256)
                .map(|number| Event::TimerFired(TimerId(number))).collect();
            self.next_timer += 256;
            assert!(host.try_deliver(events).is_ok());
            let first = self.submitted.fetch_add(256, Ordering::SeqCst) + 1;
            Ok((first..first + 256)
                .map(|correlation| submission(Intent::CloseNode(NodeId(99)), correlation)).collect())
        }
    }

    let slot = Arc::new(Mutex::new(None));
    let keep_full = Arc::new(AtomicBool::new(true));
    let submitted = Arc::new(AtomicU64::new(0));
    let mut limits = ilium_plugin_api::limits::Limits::default();
    limits.0.insert("event_queue".into(), 256);
    limits.0.insert("pending_requests".into(), 256);
    let host = Arc::new(HostHarness::new().limits(limits).grants(&["tree.write"])
        .with_native_guest(Box::new(RefillingGuest {
            host: slot.clone(), keep_full: keep_full.clone(), submitted: submitted.clone(), next_timer: 256,
        })).build().unwrap());
    *slot.lock().unwrap() = Some(Arc::downgrade(&host));
    host.deliver((0..256).map(|number| Event::TimerFired(TimerId(number))).collect());

    // More than four thousand returned requests are deliberately held behind the
    // full guest queue. This must neither panic on staging pressure nor lose replies.
    assert_eq!(host.pump(24).events_delivered, 24 * 256);
    assert_eq!(submitted.load(Ordering::SeqCst), 24 * 256);
    assert!(host.received_outcomes().is_empty());
    keep_full.store(false, Ordering::SeqCst);
    host.pump(64);
    assert!(host.failures().is_empty());
    assert!(host.queued_events().is_empty());
    let outcomes = host.received_outcomes();
    assert_eq!(outcomes.len() as u64, submitted.load(Ordering::SeqCst));
    let mut correlations: Vec<_> = outcomes.iter().map(|result| match &result.request {
        RequestRef::Submission(correlation) => *correlation,
        RequestRef::Call(_) => panic!("returned submissions must retain their correlations"),
    }).collect();
    correlations.sort_unstable();
    assert_eq!(correlations, (1..=24 * 256).collect::<Vec<u64>>());
    // The T3 pending-request ceiling is 64, even though this fixture requested
    // 256 (reference-errors-and-limits.md API-073). Each callback submits 256
    // requests together: its first 64 reach the missing-node kernel check, and
    // the remaining 192 must complete as capacity refusals, never disappear.
    assert!(host.logs().iter().any(|line| line.message.contains("pending_requests clamped from 256 to 64")));
    for result in &outcomes {
        let RequestRef::Submission(correlation) = &result.request else { unreachable!() };
        let expected = if (correlation - 1) % 256 < 64 { ErrorCode::NotFound } else { ErrorCode::Busy };
        assert!(matches!(&result.outcome, Outcome::Refused(error) if error.code == expected),
            "correlation {correlation} should be refused({expected:?}), got {:?}", result.outcome);
    }
}
