//! The `detector` export route (reference-wit.md section 8.4, capability-agent-signatures.md).
//!
//! The kernel detection loop never calls a detector itself: the plugin runtime thread runs
//! `classify-screen` and `identify-tree` here, one call per pane (HST-451), and the loop reads the
//! cached verdicts. Detectors answer in a fixed order, T1 first and then by plugin id, and the
//! first detector that answers a pane wins it (HST-452). A round spends at most a fixed budget of
//! detector time (HST-453); the screens it did not reach go back to the caller. A verdict that
//! names another pane, generation or signature is dropped, so a detector can never speak for a
//! pane it was not given.

use std::time::{Duration, Instant};

use ilium_plugin_api::generated::exports::detector::{
    DetectorTreeProcess, IdentifiedAgent, ScreenRequest, ScreenVerdict,
};
use ilium_plugin_api::Tier;
use serde_json::Value;

use super::{HostRuntime, PluginState, RuntimeError};
use crate::instance::{ExportCall, ExportReply};

/// The export name a manifest lists for the detector interface.
const DETECTOR_EXPORT: &str = "detector";

/// One accepted verdict and the detector that gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectorVerdict {
    /// The detector plugin.
    pub plugin: String,
    /// Its trust tier: only a T1 detector's interstitial response may send keys to a pane.
    pub tier: Tier,
    /// The verdict.
    pub verdict: ScreenVerdict,
}

/// Detector call time one poll cycle may spend (HST-453).
pub const DETECTOR_CYCLE_BUDGET: Duration = Duration::from_millis(50);

/// The verdicts of one classification round.
#[derive(Debug, Default)]
pub struct DetectorBatch {
    /// Each accepted verdict, in the order the screens were served.
    pub verdicts: Vec<DetectorVerdict>,
    /// The detector calls that failed; their panes keep their activity.
    pub failures: Vec<RuntimeError>,
    /// The screens the round did not reach because the budget was spent, in their order: they
    /// are first in line next cycle.
    pub unserved: Vec<ScreenRequest>,
}

/// Whether a manifest lists the detector export on one side.
fn declares_detector(manifest: &Value, side: &str) -> bool {
    manifest[side]["exports"]
        .as_array()
        .is_some_and(|exports| exports.iter().any(|name| name == DETECTOR_EXPORT))
}

impl HostRuntime {
    /// The active plugins that export `detector` on this side, T1 first and then by plugin id
    /// (HST-452). Empty means the kernel's built-in table classifies.
    pub fn detector_plugins(&self) -> Vec<String> {
        self.ordered_detectors()
            .into_iter()
            .map(|(_, id)| id)
            .collect()
    }

    /// The active detectors with their tier, in [`Self::detector_plugins`] order.
    fn ordered_detectors(&self) -> Vec<(Tier, String)> {
        let side = self.side.as_str();
        let mut detectors: Vec<_> = self
            .plugins
            .iter()
            .filter(|(_, record)| {
                record.state == PluginState::Active && declares_detector(&record.manifest, side)
            })
            .map(|(id, record)| (record.tier, id.clone()))
            .collect();
        detectors.sort();
        detectors
    }

    /// Classifies screens one call per pane (HST-451), asking the detectors in order until one
    /// answers (HST-452). Once `budget` of call time is spent the remaining screens are returned
    /// unserved (HST-453).
    pub fn classify_screens(
        &mut self,
        screens: Vec<ScreenRequest>,
        budget: Duration,
    ) -> DetectorBatch {
        let mut batch = DetectorBatch::default();
        let detectors = self.ordered_detectors();
        let started = Instant::now();
        let mut screens = screens.into_iter();
        for screen in screens.by_ref() {
            if started.elapsed() >= budget {
                batch.unserved.push(screen);
                break;
            }
            for (tier, plugin) in &detectors {
                let call = ExportCall::DetectorClassifyScreen(screen.clone());
                match self.call_export(plugin, call) {
                    Ok(ExportReply::DetectorVerdict(Some(verdict)))
                        if verdict.pane == screen.pane
                            && verdict.generation == screen.generation
                            && verdict.signature == screen.signature =>
                    {
                        batch.verdicts.push(DetectorVerdict {
                            plugin: plugin.clone(),
                            tier: *tier,
                            verdict,
                        });
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => batch.failures.push(error),
                }
            }
        }
        batch.unserved.extend(screens);
        batch
    }

    /// Runs `identify-tree` over the detectors in order and keeps the first answer; the reply
    /// must name a process of the given tree. `Ok(None)` means no detector recognised an agent.
    pub fn identify_process_tree(
        &mut self,
        processes: Vec<DetectorTreeProcess>,
    ) -> Result<Option<(String, IdentifiedAgent)>, Vec<RuntimeError>> {
        let mut failures = Vec::new();
        for plugin in self.detector_plugins() {
            match self.call_export(&plugin, ExportCall::DetectorIdentifyTree(processes.clone())) {
                Ok(ExportReply::DetectorIdentified(Some(agent)))
                    if !agent.signature.is_empty()
                        && processes.iter().any(|process| process.pid == agent.pid) =>
                {
                    return Ok(Some((plugin, agent)));
                }
                Ok(_) => {}
                Err(error) => failures.push(error),
            }
        }
        if failures.is_empty() {
            Ok(None)
        } else {
            Err(failures)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use ilium_plugin_api::generated::exports::detector::{
        ActivityEvidence, AgentTurn, DetectorTreeProcess, GoalEvidence, IdentifiedAgent,
        ScreenRequest, ScreenVerdict,
    };
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::{NodeId, PluginId};
    use ilium_plugin_api::generated::types::intents::{Intent, Submission};
    use ilium_plugin_api::generated::types::layout::Area;
    use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
    use ilium_plugin_api::generated::types::requests::Outcome;
    use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
    };
    use ilium_plugin_api::{ProcessKind, Side, Tier};
    use serde_json::{json, Value};

    use crate::calls::{ResolvedLimits, Trap};
    use crate::engine::{EngineConfig, PluginEngine};
    use crate::imports::backed::NullLogSink;
    use crate::instance::{ExportCall, ExportReply, GuestExports, PluginInstance};
    use crate::runtime::{HostRuntime, PluginRecord, PluginState, RuntimeConfig};

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
    static TREE: EmptyTree = EmptyTree;
    static PANES: EmptyPanes = EmptyPanes;

    struct Kernel;
    impl KernelFacade for Kernel {
        fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult {
            Outcome::Ok
        }
        fn tree(&self) -> &dyn TreeRead {
            &TREE
        }
        fn panes(&self) -> &dyn PaneRead {
            &PANES
        }
        fn process_kind(&self) -> ProcessKind {
            ProcessKind::Server
        }
    }

    /// What a scripted detector answers, oldest first, and the calls it received.
    #[derive(Default)]
    struct Script {
        replies: VecDeque<Result<ExportReply, Trap>>,
        seen: Vec<ExportCall>,
    }

    struct ScriptedDetector(Arc<Mutex<Script>>);

    impl GuestExports for ScriptedDetector {
        fn activate(
            &mut self,
            _activation: Activation,
        ) -> Result<Result<(), ilium_plugin_api::PluginError>, Trap> {
            Ok(Ok(()))
        }
        fn deactivate(&mut self, _reason: DeactivationReason) -> Result<Option<Vec<u8>>, Trap> {
            Ok(None)
        }
        fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
            Ok(Vec::new())
        }
        fn render(&mut self, _view: ViewTarget, _area: Area) -> Result<UiTree, Trap> {
            Err(Trap {
                code: ilium_plugin_api::Code::from_static("trap.abi"),
                interface: "views".into(),
                function: "render".into(),
            })
        }
        fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap> {
            let mut script = self.0.lock().expect("script lock");
            let silent = match &call {
                ExportCall::DetectorIdentifyTree(_) => ExportReply::DetectorIdentified(None),
                _ => ExportReply::DetectorVerdict(None),
            };
            script.seen.push(call);
            script.replies.pop_front().unwrap_or(Ok(silent))
        }
    }

    fn runtime() -> HostRuntime {
        let engine = Arc::new(
            PluginEngine::new(EngineConfig {
                epoch_tick_ms: 10,
                cache_dir: None,
                process: ProcessKind::Server,
            })
            .expect("engine"),
        );
        HostRuntime::new(
            engine,
            RuntimeConfig {
                side: Side::Server,
                kernel: Arc::new(Kernel),
                log: Arc::new(NullLogSink),
                provider: None,
                disabled: Default::default(),
            },
        )
    }

    fn add_plugin(
        runtime: &mut HostRuntime,
        id: &str,
        tier: Tier,
        manifest: Value,
    ) -> Arc<Mutex<Script>> {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut instance = PluginInstance::from_guest(Box::new(ScriptedDetector(script.clone())));
        instance
            .activate(Activation {
                plugin: PluginId(id.to_owned()),
                api: "ilium:plugin@0.1.0".to_owned(),
                instance: 1,
                side: "server".to_owned(),
                settings_json: "{}".to_owned(),
                granted: Vec::new(),
                restored: None,
            })
            .expect("activates");
        runtime.plugins.insert(
            id.to_owned(),
            PluginRecord {
                manifest,
                tier,
                granted: Vec::new(),
                triggers: Vec::new(),
                limits: ResolvedLimits(BTreeMap::new()),
                compiled: None,
                instance: Some(instance),
                state: PluginState::Active,
                saved: None,
                activation_order: 0,
                instance_number: 1,
            },
        );
        script
    }

    fn detector_manifest(id: &str) -> Value {
        json!({"id": id, "server": {"exports": ["detector"]}})
    }

    fn screen(pane: u64) -> ScreenRequest {
        ScreenRequest {
            pane,
            generation: pane + 100,
            signature: "codex".into(),
            text: String::new(),
            cursor: None,
            dimmed: Vec::new(),
        }
    }

    fn verdict(pane: u64, turn: AgentTurn) -> ScreenVerdict {
        ScreenVerdict {
            pane,
            generation: pane + 100,
            signature: "codex".into(),
            turn,
            evidence: ActivityEvidence::NoActiveMarker,
            evidence_line: None,
            goal: GoalEvidence::Unknown,
            goal_line: None,
            goal_rule: None,
            goal_pattern: None,
            fresh_screen: false,
            interstitial_response: None,
            prompt_ready: false,
        }
    }

    /// REQ: HST-452
    #[test]
    fn detectors_are_ordered_tier_first_then_by_id_and_need_the_export() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            "com.b.third",
            Tier::T2,
            detector_manifest("b"),
        );
        add_plugin(
            &mut runtime,
            "com.a.third",
            Tier::T3,
            detector_manifest("a"),
        );
        add_plugin(
            &mut runtime,
            "ilium.core.agent-detect",
            Tier::T1,
            detector_manifest("c"),
        );
        add_plugin(
            &mut runtime,
            "com.a.no-detector",
            Tier::T1,
            json!({"id": "x", "server": {"exports": ["handler"]}}),
        );
        assert_eq!(
            runtime.detector_plugins(),
            vec!["ilium.core.agent-detect", "com.b.third", "com.a.third"]
        );
    }

    /// REQ: HST-451
    /// REQ: HST-452
    #[test]
    fn each_pane_is_one_call_and_the_first_detector_to_answer_wins_it() {
        let mut runtime = runtime();
        let first = add_plugin(&mut runtime, "a.first", Tier::T1, detector_manifest("a"));
        let second = add_plugin(&mut runtime, "b.second", Tier::T2, detector_manifest("b"));
        {
            let mut script = first.lock().expect("lock");
            script
                .replies
                .push_back(Ok(ExportReply::DetectorVerdict(Some(verdict(
                    1,
                    AgentTurn::Working,
                )))));
            // A verdict for another pane does not answer pane 2.
            script
                .replies
                .push_back(Ok(ExportReply::DetectorVerdict(Some(verdict(
                    9,
                    AgentTurn::Idle,
                )))));
        }
        second
            .lock()
            .expect("lock")
            .replies
            .push_back(Ok(ExportReply::DetectorVerdict(Some(verdict(
                2,
                AgentTurn::WaitingApproval,
            )))));
        let batch =
            runtime.classify_screens(vec![screen(1), screen(2)], super::DETECTOR_CYCLE_BUDGET);
        assert!(batch.failures.is_empty());
        assert!(batch.unserved.is_empty());
        let answered: Vec<_> = batch
            .verdicts
            .iter()
            .map(|answer| {
                (
                    answer.plugin.as_str(),
                    answer.tier,
                    answer.verdict.pane,
                    answer.verdict.turn,
                )
            })
            .collect();
        assert_eq!(
            answered,
            vec![
                ("a.first", Tier::T1, 1, AgentTurn::Working),
                ("b.second", Tier::T2, 2, AgentTurn::WaitingApproval)
            ]
        );
        assert_eq!(first.lock().expect("lock").seen.len(), 2);
        let seen = &second.lock().expect("lock").seen;
        assert_eq!(
            seen.len(),
            1,
            "the second detector is asked only for pane 2"
        );
        assert!(matches!(&seen[0], ExportCall::DetectorClassifyScreen(asked) if asked.pane == 2));
    }

    /// REQ: HST-453
    #[test]
    fn a_spent_budget_returns_the_remaining_screens_in_order() {
        let mut runtime = runtime();
        let only = add_plugin(&mut runtime, "a.only", Tier::T1, detector_manifest("a"));
        let batch = runtime.classify_screens(
            vec![screen(1), screen(2), screen(3)],
            std::time::Duration::ZERO,
        );
        assert!(batch.verdicts.is_empty());
        assert_eq!(
            batch.unserved.iter().map(|s| s.pane).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(only.lock().expect("lock").seen.is_empty());
    }

    /// REQ: HST-454
    #[test]
    fn a_failing_detector_leaves_its_screens_unanswered() {
        let mut runtime = runtime();
        let only = add_plugin(&mut runtime, "a.only", Tier::T1, detector_manifest("a"));
        only.lock().expect("lock").replies.push_back(Err(Trap {
            code: ilium_plugin_api::Code::from_static("trap.deadline"),
            interface: "ilium:exports/detector@0.1.0".into(),
            function: "classify-screen".into(),
        }));
        let batch = runtime.classify_screens(vec![screen(1)], super::DETECTOR_CYCLE_BUDGET);
        assert!(batch.verdicts.is_empty());
        assert_eq!(batch.failures.len(), 1);
    }

    #[test]
    fn identify_tree_takes_the_first_answer_that_names_a_process_of_the_tree() {
        let mut runtime = runtime();
        let liar = add_plugin(&mut runtime, "a.liar", Tier::T1, detector_manifest("a"));
        let honest = add_plugin(&mut runtime, "b.honest", Tier::T1, detector_manifest("b"));
        liar.lock()
            .expect("lock")
            .replies
            .push_back(Ok(ExportReply::DetectorIdentified(Some(IdentifiedAgent {
                signature: "claude".into(),
                pid: 999,
            }))));
        honest
            .lock()
            .expect("lock")
            .replies
            .push_back(Ok(ExportReply::DetectorIdentified(Some(IdentifiedAgent {
                signature: "codex".into(),
                pid: 42,
            }))));
        let tree = vec![DetectorTreeProcess {
            pid: 42,
            parent_pid: None,
            depth: 0,
            started_at_unix_seconds: 1,
            name: "codex".into(),
            argv: vec!["codex".into()],
        }];
        let identified = runtime.identify_process_tree(tree).expect("no failures");
        assert_eq!(
            identified,
            Some((
                "b.honest".to_owned(),
                IdentifiedAgent {
                    signature: "codex".into(),
                    pid: 42
                }
            ))
        );
        assert_eq!(runtime.identify_process_tree(Vec::new()), Ok(None));
    }
}
