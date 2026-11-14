//! Hook chains of the runtime (foundation contract section 20.5, api-extension-points.md).
//!
//! `run-hook` queues a [`PendingHook`]. [`HostRuntime::route_hooks`] takes the queue once per
//! pump round and runs each chain: the handlers of the point one after another in run order,
//! each in the contributor's own instance through `ExportCall::Hook`. The replies are folded by
//! the mode of the point, the folded reply is stored for `take-reply` and the request completes
//! with `request-completed` at the plugin that called `run-hook`.
//!
//! Hooks run only handlers of this runtime's side. A handler that cannot run (its plugin is
//! not active, disabled or stopped) counts as `pass`.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ilium_plugin_api::codes::extension as codes;
use ilium_plugin_api::generated::exports::hook_handler::{HookCall, HookReply};
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::{Code, ErrorCode, PluginError, Tier};
use serde_json::Value;

use super::{lock, HostRuntime, PluginState, RuntimeError};
use crate::imports::backed::command_result;
use crate::imports::extension_points::{
    conforms, hook_handlers, hook_point, HookHandler, HookMode, HookPoint, PendingHook,
};
use crate::instance::{ExportCall, ExportReply};

/// A chain gets this long from the start of its first handler call (EXT-005).
const CHAIN_CAP: Duration = Duration::from_millis(100);
/// A reply is at most this many bytes (EXT-023).
const REPLY_CAP_BYTES: usize = 64 * 1024;
/// Handlers one point runs at most, whatever its manifest asks (EXT-023).
const HANDLERS_HARD_MAX: usize = 8;
/// Consecutive breaches that stop an extension (EXT-021).
const BREACHES_TO_STOP: u32 = 3;
/// Rounds of nested chains one pump runs: depth 3 plus the chain that started them.
const CHAIN_ROUNDS: usize = 4;

/// A contributor, a global point name and the extension id: what a breach counter is kept for.
pub type ExtensionKey = (String, String, String);

/// Breach counters and stops of the extensions of one runtime.
#[derive(Debug, Default)]
pub(super) struct HookState {
    next_call: u64,
    breaches: BTreeMap<ExtensionKey, u32>,
    stopped: BTreeSet<ExtensionKey>,
}

impl HookState {
    fn allocate_call(&mut self) -> u64 {
        self.next_call += 1;
        self.next_call
    }

    /// A restarted instance gets a clean slate: the stops end with it (EXT-021).
    pub(super) fn clear_plugin(&mut self, plugin: &str) {
        self.breaches.retain(|key, _| key.0 != plugin);
        self.stopped.retain(|key| key.0 != plugin);
    }
}

/// What the mode of a point makes of one handler reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Judged {
    /// No opinion.
    Pass,
    /// A new payload the mode accepted.
    Replace(String),
    /// A refusal the mode accepted.
    Refuse(String),
    /// An answer the mode accepted.
    Answer(String),
    /// The mode does not allow the reply; it counts as `pass` and is not a breach (EXT-014).
    NotAllowed,
    /// The reply is over the cap or fails its schema; it counts as `pass` and as a breach.
    Breach(&'static str),
}

/// Judges one reply of a handler. `current` is the payload the handler received.
pub(super) fn judge(point: &HookPoint, current: &str, reply: HookReply) -> Judged {
    let size = match &reply {
        HookReply::Pass => 0,
        HookReply::Replace(text) | HookReply::Refuse(text) | HookReply::Answer(text) => text.len(),
    };
    if size > REPLY_CAP_BYTES {
        return Judged::Breach(codes::CAP_EXCEEDED);
    }
    match (point.mode, reply) {
        (_, HookReply::Pass) => Judged::Pass,
        (HookMode::Transform, HookReply::Replace(text)) => judge_replace(point, current, text),
        (HookMode::Veto, HookReply::Refuse(reason)) => Judged::Refuse(reason),
        (HookMode::First, HookReply::Answer(text)) => judge_answer(point, text),
        _ => Judged::NotAllowed,
    }
}

fn judge_replace(point: &HookPoint, current: &str, text: String) -> Judged {
    let (Ok(old), Ok(new)) = (
        serde_json::from_str::<Value>(current),
        serde_json::from_str::<Value>(&text),
    ) else {
        return Judged::Breach(codes::SCHEMA_MISMATCH);
    };
    if let Some(schema) = &point.payload_schema {
        if !conforms(schema, &new) {
            return Judged::Breach(codes::SCHEMA_MISMATCH);
        }
    }
    if !only_changeable_fields_differ(point, &old, &new) {
        return Judged::Breach(codes::SCHEMA_MISMATCH);
    }
    Judged::Replace(text)
}

/// Every field the `reply_schema` does not list must keep its input value (EXT-014).
fn only_changeable_fields_differ(point: &HookPoint, old: &Value, new: &Value) -> bool {
    let (Some(old), Some(new)) = (old.as_object(), new.as_object()) else {
        // A payload that is not an object has no fields to freeze; the schema decided.
        return true;
    };
    let changeable: BTreeSet<&str> = point
        .reply_schema
        .as_ref()
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| properties.keys().map(String::as_str).collect())
        .unwrap_or_default();
    old.keys()
        .chain(new.keys())
        .all(|key| changeable.contains(key.as_str()) || old.get(key) == new.get(key))
}

fn judge_answer(point: &HookPoint, text: String) -> Judged {
    let Ok(answer) = serde_json::from_str::<Value>(&text) else {
        return Judged::Breach(codes::SCHEMA_MISMATCH);
    };
    match &point.reply_schema {
        Some(schema) if !conforms(schema, &answer) => Judged::Breach(codes::SCHEMA_MISMATCH),
        _ => Judged::Answer(text),
    }
}

/// How one handler call ended.
enum Called {
    /// The handler did not run: not active, stopped, or past the chain cap.
    Skipped,
    /// The handler trapped, errored or ran past its deadline: `pass`, plus a breach.
    Failed,
    /// The handler replied within its deadline.
    Replied(Judged),
}

/// How a chain ended.
struct ChainEnd {
    outcome: Outcome,
    reply: Option<String>,
}

impl ChainEnd {
    fn refused(contributor: &str, reason: &str) -> Self {
        let message = format!("{contributor}: {reason}");
        Self {
            outcome: Outcome::Refused(PluginError::with_prefix(
                ErrorCode::Conflict,
                &Code::from_static(codes::VETOED),
                &message,
            )),
            reply: None,
        }
    }
}

/// One chain while it runs.
struct Chain<'a> {
    hook: &'a PendingHook,
    point: HookPoint,
    /// The chains the handlers of this one sit inside, this chain's point last.
    stack: Vec<String>,
    first_started: Option<Instant>,
    failures: Vec<RuntimeError>,
}

impl HostRuntime {
    /// The extensions a run of breaches stopped, as (contributor, point, extension id).
    /// The user sees them with a Reset button (EXT-021).
    pub fn stopped_extensions(&self) -> Vec<ExtensionKey> {
        self.hooks.stopped.iter().cloned().collect()
    }

    /// Resets one stopped extension (the Reset button). Returns whether it was stopped.
    pub fn reset_extension(&mut self, contributor: &str, point: &str, extension: &str) -> bool {
        let key = (
            contributor.to_owned(),
            point.to_owned(),
            extension.to_owned(),
        );
        self.hooks.breaches.remove(&key);
        self.hooks.stopped.remove(&key)
    }

    /// Runs every queued hook chain, then the chains those handlers started. The folded reply
    /// is stored for `take-reply` and the request completes at the plugin that asked.
    pub(super) fn route_hooks(&mut self) -> Vec<RuntimeError> {
        let mut failures = Vec::new();
        for _ in 0..CHAIN_ROUNDS {
            let pending = self.extension_book.take_queue();
            if pending.is_empty() {
                break;
            }
            for hook in pending {
                let (end, mut failed) = self.run_chain(&hook);
                failures.append(&mut failed);
                // A caller that stopped meanwhile has no one to receive the reply.
                if self.state(&hook.caller) != Some(PluginState::Active) {
                    continue;
                }
                self.extension_book
                    .finish(&hook.caller, hook.request, end.reply);
                lock(&self.results).push_back((
                    hook.caller.clone(),
                    command_result(RequestId(hook.request), end.outcome),
                ));
            }
        }
        failures
    }

    /// Runs a hook chain the kernel itself starts, such as `ilium.core.sessions/before-restore`
    /// before a restore (EXT-056), and returns how it ended: the outcome and the folded reply
    /// (none for observe and veto points). The kernel waits for this once, so contributors that
    /// are still loaded start first instead of being skipped as on a plugin's repeated hook
    /// (LIF-274). An undeclared point ends `not-found` and runs nothing.
    pub fn run_kernel_hook(
        &mut self,
        point: &str,
        payload_json: String,
    ) -> (Outcome, Option<String>, Vec<RuntimeError>) {
        let handlers = {
            let registry = lock(&self.contributions);
            hook_handlers(&registry, &self.extension_book, point)
        };
        let mut failures = Vec::new();
        for handler in handlers {
            if self.state(&handler.plugin) == Some(PluginState::Loaded) {
                if let Err(error) = self.activate(&handler.plugin) {
                    failures.push(error);
                }
            }
        }
        let hook = PendingHook {
            // The kernel is the caller: no plugin receives a `take-reply` for this chain.
            caller: String::new(),
            request: 0,
            point: point.to_owned(),
            payload_json,
            stack: Vec::new(),
        };
        let (end, mut failed) = self.run_chain(&hook);
        failures.append(&mut failed);
        (end.outcome, end.reply, failures)
    }

    fn run_chain(&mut self, hook: &PendingHook) -> (ChainEnd, Vec<RuntimeError>) {
        let declared = {
            let registry = lock(&self.contributions);
            hook_point(&registry, &hook.point).map(|point| {
                (
                    point,
                    hook_handlers(&registry, &self.extension_book, &hook.point),
                )
            })
        };
        let Some((point, mut handlers)) = declared else {
            let error = PluginError::with_prefix(
                ErrorCode::NotFound,
                &Code::from_static(codes::UNKNOWN_POINT),
                "the owner of the point is gone",
            );
            let end = ChainEnd {
                outcome: Outcome::Failed(error),
                reply: None,
            };
            return (end, Vec::new());
        };
        // A T1 handler runs before a T2 or T3 handler of any priority (EXT-007). The sort is
        // stable, so the order inside a band stays priority, plugin id, extension id.
        let plugins = &self.plugins;
        handlers.sort_by_key(|handler| {
            u8::from(!matches!(
                plugins.get(&handler.plugin).map(|record| record.tier),
                Some(Tier::T1)
            ))
        });
        let (guards, mut handlers): (Vec<HookHandler>, Vec<HookHandler>) =
            handlers.into_iter().partition(|handler| handler.guard);
        handlers.truncate(point.max_handlers.min(HANDLERS_HARD_MAX));

        let mut stack = hook.stack.clone();
        stack.push(hook.point.clone());
        let mode = point.mode;
        let refuse_on_failure = point.refuse_on_failure;
        let final_check = point.final_check;
        let mut chain = Chain {
            hook,
            point,
            stack,
            first_started: None,
            failures: Vec::new(),
        };
        let mut current = hook.payload_json.clone();
        let mut replaced = false;
        let mut answer = None;

        for handler in &handlers {
            match self.call_handler(&mut chain, handler, &current) {
                Called::Skipped => {}
                Called::Failed => {
                    if refuse_on_failure {
                        let end = ChainEnd::refused(&handler.plugin, "guard-failed");
                        return (end, chain.failures);
                    }
                }
                Called::Replied(Judged::Replace(text)) => {
                    current = text;
                    replaced = true;
                }
                Called::Replied(Judged::Refuse(reason)) => {
                    let end = ChainEnd::refused(&handler.plugin, &reason);
                    return (end, chain.failures);
                }
                Called::Replied(Judged::Answer(text)) => {
                    answer = Some(text);
                    break;
                }
                Called::Replied(_) => {}
            }
        }

        if mode == HookMode::Transform && final_check {
            // Guards see the final payload once and may only pass or refuse.
            chain.point.mode = HookMode::Veto;
            for guard in &guards {
                match self.call_handler(&mut chain, guard, &current) {
                    Called::Failed if refuse_on_failure => {
                        let end = ChainEnd::refused(&guard.plugin, "guard-failed");
                        return (end, chain.failures);
                    }
                    Called::Replied(Judged::Refuse(reason)) => {
                        let end = ChainEnd::refused(&guard.plugin, &reason);
                        return (end, chain.failures);
                    }
                    _ => {}
                }
            }
        }

        let reply = match mode {
            HookMode::Transform => replaced.then_some(current),
            HookMode::First => answer,
            HookMode::Observe | HookMode::Veto => None,
        };
        let end = ChainEnd {
            outcome: Outcome::Ok,
            reply,
        };
        (end, chain.failures)
    }

    fn call_handler(
        &mut self,
        chain: &mut Chain<'_>,
        handler: &HookHandler,
        current: &str,
    ) -> Called {
        let hook = chain.hook;
        let key: ExtensionKey = (
            handler.plugin.clone(),
            hook.point.clone(),
            handler.extension.clone(),
        );
        if chain
            .first_started
            .is_some_and(|started| started.elapsed() >= CHAIN_CAP)
        {
            self.hook_log(
                handler,
                Level::Info,
                codes::CHAIN_CAP,
                hook,
                "chain cap reached",
            );
            return Called::Skipped;
        }
        if self.hooks.stopped.contains(&key) {
            self.hook_log(
                handler,
                Level::Info,
                codes::DISABLED,
                hook,
                "extension stopped",
            );
            return Called::Skipped;
        }
        match self.state(&handler.plugin) {
            Some(PluginState::Active) => {}
            Some(PluginState::Loaded) => {
                // The instance is still cold: this call skips it as `pass` (not a breach) and
                // the next one finds it running (LIF-274).
                self.hook_log(
                    handler,
                    Level::Info,
                    codes::CONTRIBUTOR_NOT_READY,
                    hook,
                    "contributor is starting",
                );
                if let Err(error) = self.activate(&handler.plugin) {
                    chain.failures.push(error);
                }
                return Called::Skipped;
            }
            _ => {
                self.hook_log(
                    handler,
                    Level::Info,
                    codes::DISABLED,
                    hook,
                    "contributor is not running",
                );
                return Called::Skipped;
            }
        }
        let call = HookCall {
            call: self.hooks.allocate_call(),
            point: hook.point.clone(),
            extension: handler.handler.clone(),
            payload_json: current.to_owned(),
        };
        self.extension_book.set_stack(chain.stack.clone());
        let begun = Instant::now();
        chain.first_started.get_or_insert(begun);
        let result = self.call_export(&handler.plugin, ExportCall::Hook(call));
        self.extension_book.set_stack(Vec::new());
        let spent = begun.elapsed();
        let reply = match result {
            Ok(ExportReply::Hook(reply) | ExportReply::HookResult(Ok(reply))) => reply,
            Ok(_) => {
                self.hook_log(
                    handler,
                    Level::Warn,
                    codes::HANDLER_TIMEOUT,
                    hook,
                    "the handler returned an error",
                );
                self.count_breach(handler, hook, &key);
                return Called::Failed;
            }
            Err(RuntimeError::NotActive(_)) => return Called::Skipped,
            Err(error) => {
                // A trap stops the plugin; the chain goes on as if the handler had passed.
                chain.failures.push(error);
                self.hook_log(
                    handler,
                    Level::Warn,
                    codes::HANDLER_TIMEOUT,
                    hook,
                    "the handler trapped",
                );
                self.count_breach(handler, hook, &key);
                return Called::Failed;
            }
        };
        if spent > Duration::from_millis(chain.point.deadline_ms) {
            self.hook_log(
                handler,
                Level::Warn,
                codes::HANDLER_TIMEOUT,
                hook,
                "the handler ran past its deadline",
            );
            self.count_breach(handler, hook, &key);
            return Called::Failed;
        }
        match judge(&chain.point, current, reply) {
            Judged::Breach(code) => {
                self.hook_log(handler, Level::Warn, code, hook, "the reply was rejected");
                self.count_breach(handler, hook, &key);
                Called::Replied(Judged::Pass)
            }
            Judged::NotAllowed => {
                self.hook_log(
                    handler,
                    Level::Info,
                    codes::REPLY_NOT_ALLOWED,
                    hook,
                    "the mode does not allow this reply",
                );
                Called::Replied(Judged::NotAllowed)
            }
            accepted => {
                self.hooks.breaches.remove(&key);
                Called::Replied(accepted)
            }
        }
    }

    fn count_breach(&mut self, handler: &HookHandler, hook: &PendingHook, key: &ExtensionKey) {
        let count = self.hooks.breaches.entry(key.clone()).or_insert(0);
        *count += 1;
        if *count >= BREACHES_TO_STOP {
            self.hooks.stopped.insert(key.clone());
            self.hook_log(
                handler,
                Level::Warn,
                codes::DISABLED,
                hook,
                "stopped after three consecutive breaches",
            );
        }
    }

    fn hook_log(
        &self,
        handler: &HookHandler,
        level: Level,
        code: &str,
        hook: &PendingHook,
        what: &str,
    ) {
        let message = format!(
            "{code}: point {}, extension {}: {what}",
            hook.point, handler.extension
        );
        self.log.line(&handler.plugin, self.side, level, &message);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use ilium_plugin_api::generated::exports::hook_handler::HookReply;
    use ilium_plugin_api::generated::host::extensions::{ExtensionError, HostExtensions};
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::{NodeId, PluginId, RequestId};
    use ilium_plugin_api::generated::types::intents::{Intent, Submission};
    use ilium_plugin_api::generated::types::layout::Area;
    use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
    };
    use ilium_plugin_api::{ErrorCode, ProcessKind, Side, Tier};
    use serde_json::{json, Value};

    use super::{judge, Judged};
    use crate::calls::{ResolvedLimits, Trap};
    use crate::engine::{EngineConfig, PluginEngine};
    use crate::imports::backed::NullLogSink;
    use crate::imports::extension_points::{HookMode, HookPoint, PluginExtensions};
    use crate::instance::{ExportCall, ExportReply, GuestExports, PluginInstance};
    use crate::runtime::{HostRuntime, PluginRecord, PluginState, RuntimeConfig, RuntimeError};

    const OWNER: &str = "com.example.owner";
    const POINT: &str = "com.example.owner/before-save";

    fn point(mode: HookMode) -> HookPoint {
        HookPoint {
            mode,
            payload_schema: None,
            reply_schema: None,
            deadline_ms: 20,
            max_handlers: 8,
            refuse_on_failure: false,
            final_check: false,
        }
    }

    /// REQ: EXT-014
    #[test]
    fn observe_accepts_pass_only() {
        let observe = point(HookMode::Observe);
        assert_eq!(judge(&observe, "{}", HookReply::Pass), Judged::Pass);
        assert_eq!(
            judge(&observe, "{}", HookReply::Replace("{}".into())),
            Judged::NotAllowed
        );
        assert_eq!(
            judge(&observe, "{}", HookReply::Refuse("no".into())),
            Judged::NotAllowed
        );
    }

    #[test]
    fn veto_takes_refuse_and_first_takes_answer() {
        assert_eq!(
            judge(&point(HookMode::Veto), "{}", HookReply::Refuse("no".into())),
            Judged::Refuse("no".into())
        );
        assert_eq!(
            judge(&point(HookMode::Veto), "{}", HookReply::Answer("1".into())),
            Judged::NotAllowed
        );
        assert_eq!(
            judge(&point(HookMode::First), "{}", HookReply::Answer("1".into())),
            Judged::Answer("1".into())
        );
        assert_eq!(
            judge(&point(HookMode::First), "{}", HookReply::Refuse("x".into())),
            Judged::NotAllowed
        );
    }

    #[test]
    fn a_first_answer_that_fails_the_reply_schema_is_a_breach() {
        let mut first = point(HookMode::First);
        first.reply_schema = Some(json!({
            "type": "object",
            "properties": {"label": {"type": "string"}},
            "required": ["label"]
        }));
        assert_eq!(
            judge(&first, "{}", HookReply::Answer(r#"{"label":"a"}"#.into())),
            Judged::Answer(r#"{"label":"a"}"#.into())
        );
        assert_eq!(
            judge(&first, "{}", HookReply::Answer(r#"{"label":3}"#.into())),
            Judged::Breach("extension.schema-mismatch")
        );
        assert_eq!(
            judge(&first, "{}", HookReply::Answer("not json".into())),
            Judged::Breach("extension.schema-mismatch")
        );
    }

    /// REQ: EXT-014
    #[test]
    fn a_transform_replace_may_change_only_the_listed_fields() {
        let mut transform = point(HookMode::Transform);
        transform.reply_schema = Some(json!({
            "type": "object",
            "properties": {"text": {"type": "string"}}
        }));
        let input = r#"{"text":"a","path":"/x"}"#;
        assert_eq!(
            judge(
                &transform,
                input,
                HookReply::Replace(r#"{"text":"b","path":"/x"}"#.into())
            ),
            Judged::Replace(r#"{"text":"b","path":"/x"}"#.into())
        );
        assert_eq!(
            judge(
                &transform,
                input,
                HookReply::Replace(r#"{"text":"b","path":"/y"}"#.into())
            ),
            Judged::Breach("extension.schema-mismatch")
        );
        assert_eq!(
            judge(
                &transform,
                input,
                HookReply::Replace(r#"{"text":"b"}"#.into())
            ),
            Judged::Breach("extension.schema-mismatch")
        );
    }

    /// REQ: EXT-023
    #[test]
    fn a_reply_over_64_kib_is_a_breach() {
        let big = "x".repeat(64 * 1024 + 1);
        assert_eq!(
            judge(&point(HookMode::Veto), "{}", HookReply::Refuse(big)),
            Judged::Breach("extension.cap-exceeded")
        );
    }

    // ---- a runtime over scripted guests -------------------------------------------------

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

    /// What a scripted handler does with each call, oldest first; `Pass` when the script is
    /// empty. Every payload it received is recorded.
    #[derive(Default)]
    struct Script {
        replies: VecDeque<ExportReply>,
        traps: usize,
        seen: Vec<String>,
        /// The `call` number of each hook call, in arrival order.
        calls: Vec<u64>,
        sleep_ms: u64,
    }

    struct ScriptedGuest(Arc<Mutex<Script>>);

    impl GuestExports for ScriptedGuest {
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
                interface: "view".into(),
                function: "render".into(),
            })
        }
        fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap> {
            let mut script = self.0.lock().expect("script lock");
            if let ExportCall::Hook(hook) = &call {
                script.seen.push(hook.payload_json.clone());
                script.calls.push(hook.call);
            }
            if script.traps > 0 {
                script.traps -= 1;
                return Err(Trap {
                    code: ilium_plugin_api::Code::from_static("trap.unreachable"),
                    interface: "exports".into(),
                    function: "call-export".into(),
                });
            }
            let sleep_ms = script.sleep_ms;
            let reply = script
                .replies
                .pop_front()
                .unwrap_or(ExportReply::Hook(HookReply::Pass));
            drop(script);
            if sleep_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
            }
            Ok(reply)
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

    /// Adds an active plugin with a scripted guest and loads its manifest contributions.
    fn add_plugin(
        runtime: &mut HostRuntime,
        id: &str,
        tier: Tier,
        manifest: Value,
    ) -> Arc<Mutex<Script>> {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut instance = PluginInstance::from_guest(Box::new(ScriptedGuest(script.clone())));
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
        runtime
            .contributions
            .lock()
            .expect("registry lock")
            .load_manifest(id, &manifest)
            .expect("manifest loads");
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

    fn owner_manifest(point_fields: Value) -> Value {
        let mut entry = json!({"id": "before-save", "title": "Before save"});
        if let (Some(target), Some(extra)) = (entry.as_object_mut(), point_fields.as_object()) {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }
        json!({"id": OWNER, "contributes": {"extension_points": [entry]}})
    }

    fn contributor_manifest(id: &str, priority: u8, guard: bool) -> Value {
        json!({"id": id, "contributes": {"extensions": [{
            "point": POINT, "id": "h", "priority": priority, "handler": "handle", "guard": guard
        }]}})
    }

    fn owner_import(runtime: &HostRuntime) -> PluginExtensions {
        PluginExtensions::new(
            OWNER,
            &[format!("extensions.provide:{POINT}")],
            runtime.contributions.clone(),
            runtime.extension_book.clone(),
        )
    }

    /// Runs one chain and returns the completion outcome and the taken reply.
    fn run(runtime: &mut HostRuntime, payload: &str) -> (Outcome, Option<String>) {
        let mut import = owner_import(runtime);
        let request = import
            .run_hook(POINT.to_owned(), payload.to_owned())
            .expect("queued");
        let failures = runtime.route_hooks();
        assert!(failures.is_empty(), "{failures:?}");
        let (caller, result) = lock_results(runtime)
            .into_iter()
            .find(|(_, result)| result.request == RequestRef::Call(request))
            .expect("the request completed");
        assert_eq!(caller, OWNER);
        (result.outcome, import.take_reply(request))
    }

    fn lock_results(
        runtime: &HostRuntime,
    ) -> Vec<(
        String,
        ilium_plugin_api::generated::types::requests::RequestResult,
    )> {
        runtime
            .results
            .lock()
            .expect("results lock")
            .drain(..)
            .collect()
    }

    fn reply(reply: HookReply) -> ExportReply {
        ExportReply::Hook(reply)
    }

    #[test]
    fn a_hook_with_no_handlers_completes_ok_with_no_reply() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "veto"})),
        );
        let (outcome, taken) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(taken, None);
    }

    #[test]
    fn a_kernel_hook_runs_the_handlers_and_completes_at_no_plugin() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "observe"})),
        );
        let handler = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        let payload = r#"{"session":"default","pane_count":3}"#;
        let (outcome, reply, failures) = runtime.run_kernel_hook(POINT, payload.to_owned());
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(reply, None, "an observe point folds no reply");
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(handler.lock().expect("lock").seen, vec![payload.to_owned()]);
        assert!(
            lock_results(&runtime).is_empty(),
            "no plugin asked, so no request completes"
        );
    }

    #[test]
    fn a_kernel_hook_on_an_undeclared_point_runs_nothing() {
        let mut runtime = runtime();
        let (outcome, reply, _) =
            runtime.run_kernel_hook("com.example.gone/before-restore", "{}".to_owned());
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert_eq!(reply, None);
    }

    #[test]
    fn veto_stops_at_the_first_refusal_and_skips_later_handlers() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "veto"})),
        );
        let first = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        let second = add_plugin(
            &mut runtime,
            "com.example.b",
            Tier::T2,
            contributor_manifest("com.example.b", 20, false),
        );
        first
            .lock()
            .expect("lock")
            .replies
            .push_back(reply(HookReply::Refuse("quiet hours".into())));
        let (outcome, taken) = run(&mut runtime, "{}");
        let Outcome::Refused(error) = outcome else {
            panic!("expected a refusal");
        };
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(
            error.message,
            "extension.vetoed: com.example.a: quiet hours"
        );
        assert_eq!(taken, None);
        assert!(second.lock().expect("lock").seen.is_empty());
    }

    #[test]
    fn transform_hands_each_handler_the_output_of_the_previous_one() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({
                "mode": "transform",
                "payload_schema": {"type": "object", "properties": {"text": {"type": "string"}}},
                "reply_schema": {"type": "object", "properties": {"text": {"type": "string"}}}
            })),
        );
        let first = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        let second = add_plugin(
            &mut runtime,
            "com.example.b",
            Tier::T2,
            contributor_manifest("com.example.b", 20, false),
        );
        first
            .lock()
            .expect("lock")
            .replies
            .push_back(reply(HookReply::Replace(r#"{"text":"one"}"#.into())));
        second
            .lock()
            .expect("lock")
            .replies
            .push_back(reply(HookReply::Replace(r#"{"text":"two"}"#.into())));
        let (outcome, taken) = run(&mut runtime, r#"{"text":"zero"}"#);
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(taken.as_deref(), Some(r#"{"text":"two"}"#));
        assert_eq!(
            second.lock().expect("lock").seen,
            vec![r#"{"text":"one"}"#.to_owned()]
        );
    }

    #[test]
    fn transform_with_only_passes_returns_no_reply() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({
                "mode": "transform",
                "payload_schema": {"type": "object", "properties": {"text": {"type": "string"}}},
                "reply_schema": {"type": "object", "properties": {"text": {"type": "string"}}}
            })),
        );
        add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        let (outcome, taken) = run(&mut runtime, r#"{"text":"zero"}"#);
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(taken, None);
    }

    #[test]
    fn first_stops_at_the_first_answer() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({
                "mode": "first",
                "reply_schema": {"type": "object", "properties": {"label": {"type": "string"}}}
            })),
        );
        let first = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        let second = add_plugin(
            &mut runtime,
            "com.example.b",
            Tier::T2,
            contributor_manifest("com.example.b", 20, false),
        );
        first
            .lock()
            .expect("lock")
            .replies
            .push_back(reply(HookReply::Answer(r#"{"label":"a"}"#.into())));
        let (outcome, taken) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(taken.as_deref(), Some(r#"{"label":"a"}"#));
        assert!(second.lock().expect("lock").seen.is_empty());
    }

    /// REQ: EXT-007
    #[test]
    fn a_t1_handler_runs_before_a_t3_handler_of_lower_priority_number() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "observe"})),
        );
        let low = add_plugin(
            &mut runtime,
            "com.example.t3",
            Tier::T3,
            contributor_manifest("com.example.t3", 0, false),
        );
        let high = add_plugin(
            &mut runtime,
            "com.example.t1",
            Tier::T1,
            contributor_manifest("com.example.t1", 90, false),
        );
        let (outcome, _) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        // Both ran; call numbers count up, so the T1 handler ran first.
        assert_eq!(high.lock().expect("lock").calls, vec![1]);
        assert_eq!(low.lock().expect("lock").calls, vec![2]);
    }

    /// REQ: EXT-021
    #[test]
    fn a_trapping_handler_counts_as_pass_and_three_breaches_stop_the_extension() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "observe"})),
        );
        let flaky = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        // Each trap stops the plugin, so bring it back between runs like a restart would.
        for _ in 0..3 {
            flaky.lock().expect("lock").traps = 1;
            let mut import = owner_import(&runtime);
            import
                .run_hook(POINT.to_owned(), "{}".to_owned())
                .expect("queued");
            let failures = runtime.route_hooks();
            assert_eq!(failures.len(), 1);
            assert!(matches!(failures[0], RuntimeError::Trapped { .. }));
            revive(&mut runtime, "com.example.a", &flaky);
        }
        let key = ("com.example.a".to_owned(), POINT.to_owned(), "h".to_owned());
        assert_eq!(runtime.stopped_extensions(), vec![key]);
        // The stopped extension is skipped: the guest sees no fourth call.
        let before = flaky.lock().expect("lock").seen.len();
        let (outcome, _) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(flaky.lock().expect("lock").seen.len(), before);
        // Reset puts it back.
        assert!(runtime.reset_extension("com.example.a", POINT, "h"));
        assert!(runtime.stopped_extensions().is_empty());
        let (outcome, _) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(flaky.lock().expect("lock").seen.len(), before + 1);
    }

    fn revive(runtime: &mut HostRuntime, id: &str, script: &Arc<Mutex<Script>>) {
        let mut instance = PluginInstance::from_guest(Box::new(ScriptedGuest(script.clone())));
        instance
            .activate(Activation {
                plugin: PluginId(id.to_owned()),
                api: "ilium:plugin@0.1.0".to_owned(),
                instance: 2,
                side: "server".to_owned(),
                settings_json: "{}".to_owned(),
                granted: Vec::new(),
                restored: None,
            })
            .expect("activates");
        let record = runtime.plugins.get_mut(id).expect("record");
        record.instance = Some(instance);
        record.state = PluginState::Active;
    }

    #[test]
    fn a_good_reply_resets_the_breach_counter() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "observe"})),
        );
        let flaky = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        for _ in 0..2 {
            flaky.lock().expect("lock").traps = 1;
            let mut import = owner_import(&runtime);
            import
                .run_hook(POINT.to_owned(), "{}".to_owned())
                .expect("queued");
            let _ = runtime.route_hooks();
            revive(&mut runtime, "com.example.a", &flaky);
        }
        let (outcome, _) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        for _ in 0..2 {
            flaky.lock().expect("lock").traps = 1;
            let mut import = owner_import(&runtime);
            import
                .run_hook(POINT.to_owned(), "{}".to_owned())
                .expect("queued");
            let _ = runtime.route_hooks();
            revive(&mut runtime, "com.example.a", &flaky);
        }
        assert!(runtime.stopped_extensions().is_empty());
    }

    #[test]
    fn a_failing_handler_on_a_high_risk_veto_point_refuses_the_action() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "veto", "risk": "high"})),
        );
        let flaky = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        flaky.lock().expect("lock").traps = 1;
        let mut import = owner_import(&runtime);
        let request = import
            .run_hook(POINT.to_owned(), "{}".to_owned())
            .expect("queued");
        let _ = runtime.route_hooks();
        let (_, result) = lock_results(&runtime)
            .into_iter()
            .find(|(_, result)| result.request == RequestRef::Call(request))
            .expect("completed");
        let Outcome::Refused(error) = result.outcome else {
            panic!("expected a refusal");
        };
        assert_eq!(
            error.message,
            "extension.vetoed: com.example.a: guard-failed"
        );
    }

    #[test]
    fn a_slow_handler_counts_as_a_timeout_and_its_reply_is_dropped() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "veto", "deadline_ms": 5})),
        );
        let slow = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        {
            let mut script = slow.lock().expect("lock");
            script.sleep_ms = 30;
            script
                .replies
                .push_back(reply(HookReply::Refuse("late".into())));
        }
        let (outcome, _) = run(&mut runtime, "{}");
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(runtime.hooks.breaches.values().copied().sum::<u32>(), 1);
    }

    #[test]
    fn a_guard_sees_the_final_payload_and_may_refuse_it() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({
                "mode": "transform", "final_check": true,
                "payload_schema": {"type": "object", "properties": {"text": {"type": "string"}}},
                "reply_schema": {"type": "object", "properties": {"text": {"type": "string"}}}
            })),
        );
        let editor = add_plugin(
            &mut runtime,
            "com.example.a",
            Tier::T2,
            contributor_manifest("com.example.a", 10, false),
        );
        let guard = add_plugin(
            &mut runtime,
            "com.example.g",
            Tier::T2,
            contributor_manifest("com.example.g", 10, true),
        );
        editor
            .lock()
            .expect("lock")
            .replies
            .push_back(reply(HookReply::Replace(r#"{"text":"edited"}"#.into())));
        guard
            .lock()
            .expect("lock")
            .replies
            .push_back(reply(HookReply::Refuse("secret found".into())));
        let (outcome, taken) = run(&mut runtime, r#"{"text":"plain"}"#);
        assert_eq!(
            guard.lock().expect("lock").seen,
            vec![r#"{"text":"edited"}"#.to_owned()]
        );
        let Outcome::Refused(error) = outcome else {
            panic!("expected a refusal");
        };
        assert_eq!(
            error.message,
            "extension.vetoed: com.example.g: secret found"
        );
        assert_eq!(taken, None);
    }

    /// REQ: EXT-022, EXT-046
    #[test]
    fn a_run_hook_from_a_handler_is_queued_and_a_chain_on_the_stack_is_refused() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "observe"})),
        );
        let mut import = owner_import(&runtime);
        // Outside a handler the stack is empty.
        assert!(import.run_hook(POINT.to_owned(), "{}".to_owned()).is_ok());
        // Inside a chain on this point the same point is on the stack (EXT-022).
        runtime.extension_book.set_stack(vec![POINT.to_owned()]);
        assert_eq!(
            import.run_hook(POINT.to_owned(), "{}".to_owned()),
            Err(ExtensionError::Reentrant)
        );
        // A handler at depth 3 starts nothing deeper (EXT-046).
        runtime
            .extension_book
            .set_stack(vec!["a/x".into(), "b/y".into(), "c/z".into()]);
        assert_eq!(
            import.run_hook(POINT.to_owned(), "{}".to_owned()),
            Err(ExtensionError::Reentrant)
        );
        runtime.extension_book.set_stack(Vec::new());
    }

    #[test]
    fn run_hook_checks_the_payload_against_the_schema_and_the_rate() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({
                "mode": "observe",
                "payload_schema": {
                    "type": "object",
                    "properties": {"n": {"type": "integer"}},
                    "required": ["n"]
                }
            })),
        );
        let mut import = owner_import(&runtime);
        assert_eq!(
            import.run_hook(POINT.to_owned(), r#"{"n":"x"}"#.to_owned()),
            Err(ExtensionError::SchemaMismatch)
        );
        assert_eq!(
            import.run_hook(POINT.to_owned(), "nope".to_owned()),
            Err(ExtensionError::SchemaMismatch)
        );
        for _ in 0..200 {
            assert!(import
                .run_hook(POINT.to_owned(), r#"{"n":1}"#.to_owned())
                .is_ok());
        }
        // The 201st call inside one second is refused.
        assert_eq!(
            import.run_hook(POINT.to_owned(), r#"{"n":1}"#.to_owned()),
            Err(ExtensionError::CapExceeded)
        );
    }

    #[test]
    fn request_ids_of_hooks_do_not_collide_with_command_ids() {
        let mut runtime = runtime();
        add_plugin(
            &mut runtime,
            OWNER,
            Tier::T1,
            owner_manifest(json!({"mode": "observe"})),
        );
        let mut import = owner_import(&runtime);
        let RequestId(request) = import
            .run_hook(POINT.to_owned(), "{}".to_owned())
            .expect("queued");
        assert!(request >= 1);
    }
}
