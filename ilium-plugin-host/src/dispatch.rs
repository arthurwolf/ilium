//! The dispatcher: submissions in, outcomes out, deduplication and pending-request accounting
//! (reference-events-and-intents.md ABI-008, ABI-010, ABI-013).

use std::sync::Arc;

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::requests::{RequestRef, RequestResult};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::kernel::{KernelFacade, Origin};

/// ABI-013: pending requests per plugin.
pub const MAX_PENDING_PER_PLUGIN: usize = 64;

/// ABI-008: a call that returns more submissions than this traps (the 257th).
pub const MAX_SUBMISSIONS_PER_CALL: usize = 256;

/// Dispatcher settings (reference-events-and-intents.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchConfig {
    /// Pending requests allowed per plugin.
    pub max_pending_per_plugin: usize,
    /// Submissions allowed per guest call.
    pub max_submissions_per_call: usize,
    /// Deduplication window in milliseconds (ABI-010).
    pub dedup_window_ms: u64,
}

/// What one [`Dispatcher::pump`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpReport {
    /// Rounds run.
    pub rounds: u32,
    /// Events handed to guests.
    pub events_delivered: usize,
    /// Intents applied to the kernel.
    pub intents_applied: usize,
    /// Calls that trapped.
    pub traps: usize,
}

/// Routes submissions to the kernel and outcomes back to plugins (reference-host-architecture.md).
pub struct Dispatcher {
    config: DispatchConfig,
}

impl Dispatcher {
    /// A dispatcher over a kernel facade.
    pub fn new(_kernel: Arc<dyn KernelFacade>, _config: DispatchConfig) -> Dispatcher {
        unimplemented!("host::dispatch::Dispatcher::new")
    }

    /// Queues one submission for an origin and returns its request reference.
    pub fn submit(&self, _origin: Origin, _submission: Submission) -> RequestRef {
        unimplemented!("host::dispatch::Dispatcher::submit")
    }

    /// Queues events for delivery to the plugins that subscribed.
    pub fn deliver(&self, _events: Vec<Event>) {
        unimplemented!("host::dispatch::Dispatcher::deliver")
    }

    /// The outcomes waiting for a plugin.
    pub fn outcomes(&self, _plugin: &PluginId) -> Vec<RequestResult> {
        unimplemented!("host::dispatch::Dispatcher::outcomes")
    }

    /// Runs delivery and application rounds until quiet or `max_rounds` is reached.
    pub fn pump(&self, _max_rounds: u32) -> PumpReport {
        unimplemented!("host::dispatch::Dispatcher::pump")
    }

    /// The settings in force.
    pub fn config(&self) -> &DispatchConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::{DispatchConfig, Dispatcher, MAX_PENDING_PER_PLUGIN, MAX_SUBMISSIONS_PER_CALL};
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::intents::{
        AgentActionSpec, Delivery, Intent, RenameNodeSpec, SendInputSpec, Submission,
    };
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
    use ilium_plugin_api::generated::types::tree::TitleSource;
    use ilium_plugin_api::ids::PluginId;
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
    };
    use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Kernel stand-in that records every applied intent and answers with scripted outcomes,
    /// then `Ok` once the script is empty.
    #[derive(Default)]
    struct RecordingKernel {
        applied: Mutex<Vec<(Origin, Intent)>>,
        scripted_outcomes: Mutex<VecDeque<Outcome>>,
    }

    impl RecordingKernel {
        fn new() -> Arc<RecordingKernel> {
            Arc::new(RecordingKernel::default())
        }

        fn script(&self, outcome: Outcome) {
            self.scripted_outcomes
                .lock()
                .expect("scripted outcomes lock")
                .push_back(outcome);
        }

        fn applied_intents(&self) -> Vec<(Origin, Intent)> {
            self.applied.lock().expect("applied intents lock").clone()
        }
    }

    impl KernelFacade for RecordingKernel {
        fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
            self.applied
                .lock()
                .expect("applied intents lock")
                .push((origin.clone(), intent));
            self.scripted_outcomes
                .lock()
                .expect("scripted outcomes lock")
                .pop_front()
                .unwrap_or(Outcome::Ok)
        }

        fn tree(&self) -> &dyn TreeRead {
            &EMPTY_TREE
        }

        fn panes(&self) -> &dyn PaneRead {
            &EMPTY_PANES
        }

        fn process_kind(&self) -> ProcessKind {
            ProcessKind::Server
        }
    }

    struct EmptyTree;

    impl TreeRead for EmptyTree {
        fn nodes(&self) -> Vec<TreeNode> {
            Vec::new()
        }

        fn node(&self, _id: NodeId) -> Option<TreeNode> {
            None
        }

        fn epoch(&self) -> u64 {
            0
        }
    }

    static EMPTY_TREE: EmptyTree = EmptyTree;

    struct EmptyPanes;

    impl PaneRead for EmptyPanes {
        fn screen_text(&self, _pane: NodeId) -> Option<String> {
            None
        }

        fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
            None
        }

        fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
            None
        }
    }

    static EMPTY_PANES: EmptyPanes = EmptyPanes;

    fn default_config() -> DispatchConfig {
        DispatchConfig {
            max_pending_per_plugin: MAX_PENDING_PER_PLUGIN,
            max_submissions_per_call: MAX_SUBMISSIONS_PER_CALL,
            dedup_window_ms: 600_000,
        }
    }

    fn dispatcher_over(kernel: &Arc<RecordingKernel>) -> Dispatcher {
        Dispatcher::new(kernel.clone(), default_config())
    }

    fn plugin_origin(plugin_name: &str) -> Origin {
        Origin::Plugin {
            plugin: PluginId(plugin_name.to_string()),
            side: Side::Server,
            instance: 1,
        }
    }

    fn plugin_id(plugin_name: &str) -> PluginId {
        PluginId(plugin_name.to_string())
    }

    /// A rename submission whose target node equals its correlation number, so the applied
    /// order can be read back from the kernel.
    fn rename_submission(correlation: u64, dedup_key: Option<&str>) -> Submission {
        Submission {
            intent: Intent::RenameNode(RenameNodeSpec {
                node: NodeId(correlation),
                title: "build".to_string(),
                source: TitleSource::Plugin,
                expected_revision: None,
            }),
            correlation,
            dedup_key: dedup_key.map(str::to_string),
        }
    }

    fn send_input_submission(correlation: u64, dedup_key: Option<&str>) -> Submission {
        Submission {
            intent: Intent::SendInput(SendInputSpec {
                pane: NodeId(7),
                text: "ls".to_string(),
                delivery: Delivery::Typed,
                prompt_epoch: None,
                stages: Vec::new(),
                raw: None,
                mouse: None,
                if_current: None,
                lock_user_input: false,
                journal: true,
            }),
            correlation,
            dedup_key: dedup_key.map(str::to_string),
        }
    }

    fn agent_action_submission(correlation: u64, dedup_key: Option<&str>) -> Submission {
        Submission {
            intent: Intent::AgentAction(AgentActionSpec {
                pane: NodeId(7),
                action_id: "approve".to_string(),
            }),
            correlation,
            dedup_key: dedup_key.map(str::to_string),
        }
    }

    fn outcome_for(results: &[RequestResult], correlation: u64) -> Outcome {
        results
            .iter()
            .find(|result| result.request == RequestRef::Submission(correlation))
            .map(|result| result.outcome.clone())
            .unwrap_or_else(|| panic!("no outcome for submission {correlation}: {results:?}"))
    }

    fn refused_code(outcome: &Outcome) -> ErrorCode {
        match outcome {
            Outcome::Refused(error) => error.code,
            other => panic!("expected refused outcome, got {other:?}"),
        }
    }

    fn applied_nodes(kernel: &RecordingKernel) -> Vec<u64> {
        kernel
            .applied_intents()
            .iter()
            .map(|(_, intent)| match intent {
                Intent::RenameNode(spec) => spec.node.0,
                other => panic!("unexpected applied intent {other:?}"),
            })
            .collect()
    }

    /// REQ: ABI-011
    #[test]
    fn submit_returns_submission_ref_carrying_the_plugin_correlation() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);

        let reference = dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(42, None));

        assert_eq!(reference, RequestRef::Submission(42));
    }

    /// REQ: RQO-001
    #[test]
    fn submit_queues_without_applying_until_pump_runs() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);

        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, None));

        assert!(kernel.applied_intents().is_empty());
        assert!(dispatcher.outcomes(&plugin_id("com.example.alpha")).is_empty());
    }

    /// REQ: RQO-003
    #[test]
    fn pump_applies_submissions_in_submission_order() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=3 {
            dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(correlation, None));
        }

        dispatcher.pump(8);

        assert_eq!(applied_nodes(&kernel), vec![1, 2, 3]);
    }

    /// REQ: RQO-001
    #[test]
    fn every_submission_completes_exactly_once() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=10 {
            dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(correlation, None));
        }

        dispatcher.pump(4);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(results.len(), 10);
        for correlation in 1..=10 {
            let completions = results
                .iter()
                .filter(|result| result.request == RequestRef::Submission(correlation))
                .count();
            assert_eq!(completions, 1, "submission {correlation} completed {completions} times");
        }
    }

    /// REQ: ABI-008
    #[test]
    fn pump_report_counts_applied_intents_and_no_traps_for_valid_submissions() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=3 {
            dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(correlation, None));
        }

        let report = dispatcher.pump(8);

        assert_eq!(report.intents_applied, 3);
        assert_eq!(report.traps, 0);
        assert!(report.rounds >= 1 && report.rounds <= 8, "rounds was {}", report.rounds);
    }

    /// REQ: RQO-001
    #[test]
    fn pump_on_an_empty_queue_delivers_and_applies_nothing() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);

        let report = dispatcher.pump(5);

        assert_eq!(report.events_delivered, 0);
        assert_eq!(report.intents_applied, 0);
        assert_eq!(report.traps, 0);
        assert!(report.rounds <= 5, "rounds was {}", report.rounds);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-001
    #[test]
    fn pump_with_zero_rounds_runs_nothing_and_a_later_pump_applies_the_queue() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, None));

        let idle_report = dispatcher.pump(0);

        assert_eq!(idle_report.rounds, 0);
        assert_eq!(idle_report.intents_applied, 0);
        assert!(kernel.applied_intents().is_empty());

        let report = dispatcher.pump(1);

        assert_eq!(report.intents_applied, 1);
        assert_eq!(dispatcher.outcomes(&plugin_id("com.example.alpha")).len(), 1);
    }

    /// REQ: ABI-008
    #[test]
    fn per_call_submission_cap_is_256_so_the_257th_traps() {
        assert_eq!(MAX_SUBMISSIONS_PER_CALL, 256);
        assert_eq!(MAX_SUBMISSIONS_PER_CALL + 1, 257);
    }

    /// REQ: ABI-013
    #[test]
    fn pending_limit_is_64_per_plugin_by_default() {
        assert_eq!(MAX_PENDING_PER_PLUGIN, 64);
    }

    /// REQ: ABI-013
    #[test]
    fn completed_submissions_free_pending_slots_for_new_submissions() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=64 {
            dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(correlation, None));
        }
        dispatcher.pump(8);

        for correlation in 65..=128 {
            dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(correlation, None));
        }
        let report = dispatcher.pump(8);

        assert_eq!(report.intents_applied, 64);
        assert_eq!(kernel.applied_intents().len(), 128);
        assert_eq!(dispatcher.outcomes(&plugin_id("com.example.alpha")).len(), 128);
    }

    /// REQ: ABI-011
    #[test]
    fn outcomes_are_handed_only_to_the_plugin_that_submitted_them() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, None));
        dispatcher.submit(plugin_origin("com.example.beta"), rename_submission(2, None));

        dispatcher.pump(8);
        let alpha = dispatcher.outcomes(&plugin_id("com.example.alpha"));
        let beta = dispatcher.outcomes(&plugin_id("com.example.beta"));

        assert_eq!(
            alpha,
            vec![RequestResult {
                request: RequestRef::Submission(1),
                outcome: Outcome::Ok,
            }]
        );
        assert_eq!(
            beta,
            vec![RequestResult {
                request: RequestRef::Submission(2),
                outcome: Outcome::Ok,
            }]
        );
    }

    /// REQ: RQO-001
    #[test]
    fn outcomes_for_a_plugin_without_submissions_are_empty() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, None));
        dispatcher.pump(8);

        assert!(dispatcher.outcomes(&plugin_id("com.example.unknown")).is_empty());
    }

    /// REQ: RQO-004
    #[test]
    fn repeated_dedup_key_completes_as_duplicate_naming_the_first_submission() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("finish:node-12")));
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(2, Some("finish:node-12")));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(applied_nodes(&kernel), vec![1]);
        assert_eq!(outcome_for(&results, 1), Outcome::Ok);
        assert_eq!(
            outcome_for(&results, 2),
            Outcome::Duplicate(RequestRef::Submission(1))
        );
    }

    /// REQ: RQO-004
    #[test]
    fn dedup_key_is_scoped_to_the_plugin_so_two_plugins_do_not_collide() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("shared.key")));
        dispatcher.submit(plugin_origin("com.example.beta"), rename_submission(2, Some("shared.key")));

        dispatcher.pump(8);

        assert_eq!(kernel.applied_intents().len(), 2);
        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.beta")), 2),
            Outcome::Ok
        );
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_of_exactly_128_characters_is_accepted() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        let key = "k".repeat(128);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some(&key)));

        dispatcher.pump(8);

        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_of_129_characters_is_refused_as_invalid_argument() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        let key = "k".repeat(129);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some(&key)));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::InvalidArgument);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-020
    #[test]
    fn empty_dedup_key_is_refused_as_invalid_argument() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("")));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::InvalidArgument);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_with_a_character_outside_the_allowed_set_is_refused() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("finish node")));
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(2, Some("a@b")));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::InvalidArgument);
        assert_eq!(refused_code(&outcome_for(&results, 2)), ErrorCode::InvalidArgument);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_using_every_allowed_character_class_is_accepted() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("Aa0.9_:/-")));

        dispatcher.pump(8);

        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
    }

    /// REQ: RQO-011
    #[test]
    fn refused_submission_does_not_reserve_its_dedup_key() {
        let kernel = RecordingKernel::new();
        kernel.script(Outcome::Refused(PluginError {
            code: ErrorCode::Conflict,
            message: "node is busy".to_string(),
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("k1")));
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(2, Some("k1")));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::Conflict);
        assert_eq!(outcome_for(&results, 2), Outcome::Ok);
        assert_eq!(kernel.applied_intents().len(), 2);
    }

    /// REQ: RQO-022
    #[test]
    fn accepted_submission_that_failed_keeps_its_dedup_key_reserved() {
        let kernel = RecordingKernel::new();
        kernel.script(Outcome::Failed(PluginError {
            code: ErrorCode::TimedOut,
            message: "timed out".to_string(),
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, Some("k1")));
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(2, Some("k1")));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert!(matches!(outcome_for(&results, 1), Outcome::Failed(_)));
        assert_eq!(
            outcome_for(&results, 2),
            Outcome::Duplicate(RequestRef::Submission(1))
        );
        assert_eq!(kernel.applied_intents().len(), 1);
    }

    /// REQ: EVT-021
    #[test]
    fn send_input_without_dedup_key_is_refused_and_never_applied() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), send_input_submission(1, None));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::InvalidArgument);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: EVT-021
    #[test]
    fn agent_action_without_dedup_key_is_refused_and_never_applied() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), agent_action_submission(1, None));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::InvalidArgument);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: EVT-021
    #[test]
    fn send_input_with_dedup_key_is_applied() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), send_input_submission(1, Some("input-1")));

        dispatcher.pump(8);

        assert_eq!(kernel.applied_intents().len(), 1);
        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
    }

    /// REQ: ABI-032
    #[test]
    fn kernel_refusal_applies_to_its_own_submission_only() {
        let kernel = RecordingKernel::new();
        kernel.script(Outcome::Refused(PluginError {
            code: ErrorCode::NotFound,
            message: "no such node".to_string(),
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(1, None));
        dispatcher.submit(plugin_origin("com.example.alpha"), rename_submission(2, None));

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::NotFound);
        assert_eq!(outcome_for(&results, 2), Outcome::Ok);
    }

    /// REQ: HST-011
    #[test]
    fn deliver_with_no_subscribers_hands_nothing_to_any_plugin() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.deliver(vec![Event::TreeChanged(vec![NodeId(1)])]);

        let report = dispatcher.pump(4);

        assert_eq!(report.events_delivered, 0);
    }

    /// REQ: HST-011
    #[test]
    fn deliver_of_an_empty_batch_is_a_no_op() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.deliver(Vec::new());

        let report = dispatcher.pump(2);

        assert_eq!(report.events_delivered, 0);
        assert_eq!(report.intents_applied, 0);
        assert_eq!(report.traps, 0);
    }

    /// REQ: RQO-004
    #[test]
    fn config_reports_the_settings_it_was_built_with_including_the_600_second_window() {
        let kernel = RecordingKernel::new();
        let config = DispatchConfig {
            max_pending_per_plugin: MAX_PENDING_PER_PLUGIN,
            max_submissions_per_call: MAX_SUBMISSIONS_PER_CALL,
            dedup_window_ms: 600_000,
        };
        let dispatcher = Dispatcher::new(kernel.clone(), config);

        assert_eq!(*dispatcher.config(), config);
        assert_eq!(dispatcher.config().dedup_window_ms, 600_000);
    }
}
