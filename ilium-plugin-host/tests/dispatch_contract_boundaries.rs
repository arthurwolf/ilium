//! Dispatcher admission, application and ownership boundaries.
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ids::{NodeId, PluginId};
use ilium_plugin_api::kernel::{Cell, KernelFacade, Origin, PaneRead, TreeNode, TreeRead};
use ilium_plugin_api::platform::ProcessKind;
use ilium_plugin_api::side::Side;
use ilium_plugin_api::{ErrorCode, PluginError};
use ilium_plugin_host::dispatch::{DispatchConfig, Dispatcher, SubmissionAdmission};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Kernel(Mutex<Vec<String>>);
impl TreeRead for Kernel {
    fn nodes(&self) -> Vec<TreeNode> {
        vec![]
    }
    fn node(&self, _: NodeId) -> Option<TreeNode> {
        None
    }
    fn epoch(&self) -> u64 {
        0
    }
}
impl PaneRead for Kernel {
    fn screen_text(&self, _: NodeId) -> Option<String> {
        None
    }
    fn cells(&self, _: NodeId, _: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        None
    }
    fn history(&self, _: NodeId, _: u32) -> Option<Vec<String>> {
        None
    }
}
impl KernelFacade for Kernel {
    fn apply_intent(&self, _: &Origin, intent: Intent) -> Outcome {
        let Intent::PlaySound(name) = intent else {
            panic!("test intent")
        };
        self.0.lock().unwrap().push(name.clone());
        match name.as_str() {
            "refuse" => Outcome::Refused(PluginError::new(ErrorCode::Conflict, "refused")),
            "fail" => Outcome::Failed(PluginError::new(ErrorCode::Io, "failed")),
            _ => Outcome::Ok,
        }
    }
    fn tree(&self) -> &dyn TreeRead {
        self
    }
    fn panes(&self) -> &dyn PaneRead {
        self
    }
    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Server
    }
}
struct Admission;
impl SubmissionAdmission for Admission {
    fn check(&self, _: &Origin, intent: &Intent) -> Result<(), PluginError> {
        if matches!(intent, Intent::PlaySound(name) if name == "denied") {
            Err(PluginError::new(ErrorCode::NotPermitted, "missing grant"))
        } else {
            Ok(())
        }
    }
}
fn origin() -> Origin {
    Origin::Plugin {
        plugin: PluginId("test".into()),
        side: Side::Server,
        instance: 1,
    }
}
fn submission(correlation: u64, name: &str, key: Option<&str>) -> Submission {
    Submission {
        intent: Intent::PlaySound(name.into()),
        correlation,
        dedup_key: key.map(str::to_owned),
    }
}
fn dispatcher(limit: usize) -> (Arc<Kernel>, Dispatcher) {
    let kernel = Arc::new(Kernel::default());
    let dispatcher = Dispatcher::with_admission(
        kernel.clone(),
        DispatchConfig {
            max_pending_per_plugin: limit,
            max_submissions_per_call: 256,
            dedup_window_ms: 600_000,
        },
        Arc::new(Admission),
    );
    (kernel, dispatcher)
}
#[test]
fn malformed_wins_over_grant_and_batch_is_atomic() {
    let (kernel, dispatcher) = dispatcher(64);
    dispatcher.submit_batch(
        origin(),
        vec![
            submission(1, "denied", None),
            submission(2, "ok", Some("bad key")),
        ],
    );
    assert_eq!(dispatcher.pump(1).intents_applied, 0);
    assert!(kernel.0.lock().unwrap().is_empty());
    let outcomes = dispatcher.outcomes(&PluginId("test".into()));
    assert!(
        matches!(&outcomes[0].outcome, Outcome::Refused(e) if e.code == ErrorCode::Cancelled && e.message == "batch refused: submission 1")
    );
    assert!(
        matches!(&outcomes[1].outcome, Outcome::Refused(e) if e.code == ErrorCode::InvalidArgument)
    );
}
#[test]
fn failed_reserves_key_but_refused_does_not_and_order_is_preserved() {
    let (kernel, dispatcher) = dispatcher(64);
    dispatcher.submit_batch(
        origin(),
        vec![
            submission(1, "refuse", Some("retry")),
            submission(2, "ok", Some("retry")),
            submission(3, "fail", Some("once")),
            submission(4, "ok", Some("once")),
        ],
    );
    assert_eq!(dispatcher.pump(1).intents_applied, 3);
    assert_eq!(*kernel.0.lock().unwrap(), vec!["refuse", "ok", "fail"]);
    let results = dispatcher.outcomes(&PluginId("test".into()));
    assert_eq!(
        results[3].outcome,
        Outcome::Duplicate(RequestRef::Submission(3))
    );
    assert!(dispatcher.outcomes(&PluginId("test".into())).is_empty());
}
#[test]
fn pending_quota_releases_at_completion_handoff_and_call_ids_are_unique() {
    let (_, dispatcher) = dispatcher(1);
    let first = dispatcher
        .try_submit_call(origin(), submission(9, "ok", None))
        .unwrap();
    dispatcher.pump(1);
    assert_eq!(
        dispatcher
            .try_submit_call(origin(), submission(9, "ok", None))
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    let results = dispatcher.outcomes(&PluginId("test".into()));
    assert_eq!(results[0].request, RequestRef::Call(first));
    let second = dispatcher
        .try_submit_call(origin(), submission(9, "ok", None))
        .unwrap();
    assert_ne!(first, second);
}
#[test]
fn oversized_batch_traps_without_outcomes_and_zero_rounds_do_no_work() {
    let (_, dispatcher) = dispatcher(64);
    assert!(!dispatcher.submit_batch(
        origin(),
        (0..257).map(|i| submission(i, "ok", None)).collect()
    ));
    assert_eq!(dispatcher.pump(0).traps, 1);
    assert!(dispatcher.outcomes(&PluginId("test".into())).is_empty());
    dispatcher.submit(origin(), submission(1, "ok", None));
    assert_eq!(dispatcher.pump(0).intents_applied, 0);
    assert_eq!(dispatcher.pump(1).intents_applied, 1);
}
#[test]
fn buffered_events_are_not_guest_deliveries_and_backpressure_returns_ownership() {
    use ilium_plugin_api::generated::types::events::Event;
    let (_, dispatcher) = dispatcher(64);
    dispatcher.deliver(vec![Event::ThemeChanged; 4096]).unwrap();
    assert_eq!(
        dispatcher
            .deliver(vec![Event::ThemeChanged])
            .unwrap_err()
            .len(),
        1
    );
    assert_eq!(dispatcher.pump(1).events_delivered, 0);
    assert_eq!(dispatcher.take_events(256).len(), 256);
    assert_eq!(dispatcher.take_events(4096).len(), 3840);
}

#[test]
fn instance_cancellation_and_completion_handoff_do_not_cross_sides() {
    let (kernel, dispatcher) = dispatcher(64);
    let server = origin();
    let client = Origin::Plugin {
        plugin: PluginId("test".into()),
        side: Side::Client,
        instance: 2,
    };
    dispatcher.submit(server.clone(), submission(1, "server", None));
    dispatcher.submit(client.clone(), submission(1, "client", None));
    dispatcher.cancel_origin(&server);
    dispatcher.pump(1);
    let server_results = dispatcher.outcomes_for(&server);
    assert_eq!(server_results.len(), 1);
    assert_eq!(server_results[0].outcome, Outcome::Cancelled);
    let client_results = dispatcher.outcomes_for(&client);
    assert_eq!(client_results.len(), 1);
    assert_eq!(client_results[0].outcome, Outcome::Ok);
    assert_eq!(*kernel.0.lock().unwrap(), vec!["client"]);
}

#[test]
fn default_constructor_fails_closed_for_plugin_grants() {
    let kernel = Arc::new(Kernel::default());
    let dispatcher = Dispatcher::new(
        kernel.clone(),
        DispatchConfig {
            max_pending_per_plugin: 64,
            max_submissions_per_call: 256,
            dedup_window_ms: 1,
        },
    );
    dispatcher.submit(origin(), submission(1, "ok", None));
    assert_eq!(dispatcher.pump(1).intents_applied, 0);
    assert!(kernel.0.lock().unwrap().is_empty());
    assert!(
        matches!(&dispatcher.outcomes_for(&origin())[0].outcome, Outcome::Refused(e) if e.code == ErrorCode::NotPermitted)
    );
    assert_eq!(dispatcher.config().dedup_window_ms, 600_000);
}

#[test]
fn bounded_completion_handoff_keeps_remaining_results_with_their_owner() {
    let (_, dispatcher) = dispatcher(4);
    for correlation in 0..4 {
        dispatcher.submit(origin(), submission(correlation, "ok", None));
    }
    dispatcher.pump(1);
    assert!(dispatcher.take_outcomes_for(&origin(), 0).is_empty());
    let first = dispatcher.take_outcomes_for(&origin(), 1);
    assert_eq!(first.len(), 1);
    let second = dispatcher.take_outcomes_for(&origin(), 2);
    assert_eq!(second.len(), 2);
    let last = dispatcher.take_outcomes_for(&origin(), 2);
    assert_eq!(last.len(), 1);
    let all: Vec<_> = first.into_iter().chain(second).chain(last).collect();
    assert_eq!(all.iter().map(|result| result.request.clone()).collect::<Vec<_>>(),
        (0..4).map(RequestRef::Submission).collect::<Vec<_>>());
    assert!(dispatcher.take_outcomes_for(&origin(), 1).is_empty());
}
