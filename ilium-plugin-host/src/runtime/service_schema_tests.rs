//! Gap G13 end to end through `route_services`: a caller's request goes through the broker to a
//! scripted provider instance, and the schema checks of the pump decide what the provider and
//! the caller see.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::exports::service_provider::ServiceReply;
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::host::services::HostServices;
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
use serde_json::json;

use crate::calls::{ResolvedLimits, Trap};
use crate::engine::{EngineConfig, PluginEngine};
use crate::imports::backed::LogSink;
use crate::imports::services::BrokeredServices;
use crate::instance::{ExportCall, ExportReply, GuestExports, PluginInstance};
use crate::runtime::{HostRuntime, PluginRecord, PluginState, RuntimeConfig};

const PROVIDER: &str = "com.example.glossary";
const SERVICE: &str = "com.example.glossary/lookup";
const CALLER: &str = "com.example.hover";
const SCHEMA: &str = r#"{"lookup": {
    "params": {"type": "object", "required": ["term"],
               "properties": {"term": {"type": "string", "maxLength": 64}}},
    "result": {"type": "object", "required": ["found"],
               "properties": {"found": {"type": "boolean"}}}
}}"#;

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

/// The log lines the runtime wrote, as (plugin, level, message).
#[derive(Default)]
struct RecordingLog(Mutex<Vec<(String, Level, String)>>);
impl LogSink for RecordingLog {
    fn line(&self, plugin: &str, _side: Side, level: Level, message: &str) {
        if let Ok(mut lines) = self.0.lock() {
            lines.push((plugin.to_owned(), level, message.to_owned()));
        }
    }
}

/// What the provider answers, oldest first, and every payload it received.
#[derive(Default)]
struct Script {
    replies: VecDeque<ServiceReply>,
    seen: Vec<(String, String)>,
    /// The call number of each delivered call, for a deferred reply.
    calls: Vec<u64>,
}

struct ScriptedProvider(Arc<Mutex<Script>>);

impl GuestExports for ScriptedProvider {
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
        if let ExportCall::ServiceHandle(service_call) = &call {
            script.seen.push((
                service_call.method.clone(),
                service_call.payload_json.clone(),
            ));
            script.calls.push(service_call.call);
        }
        let reply = script
            .replies
            .pop_front()
            .unwrap_or(ServiceReply::Done(r#"{"found":false}"#.to_owned()));
        Ok(ExportReply::ServiceReply(reply))
    }
}

/// A server runtime with an active scripted provider of `SERVICE` whose schema is installed,
/// and the recording log it writes to.
fn runtime_with_provider() -> (HostRuntime, Arc<Mutex<Script>>, Arc<RecordingLog>) {
    let engine = Arc::new(
        PluginEngine::new(EngineConfig {
            epoch_tick_ms: 10,
            cache_dir: None,
            process: ProcessKind::Server,
        })
        .expect("engine"),
    );
    let log = Arc::new(RecordingLog::default());
    let mut runtime = HostRuntime::new(
        engine,
        RuntimeConfig {
            side: Side::Server,
            kernel: Arc::new(Kernel),
            log: Arc::clone(&log) as Arc<dyn LogSink>,
            provider: None,
            disabled: Default::default(),
        },
    );
    runtime.install_schema(SERVICE, SCHEMA).expect("installs");
    let script = Arc::new(Mutex::new(Script::default()));
    let mut instance = PluginInstance::from_guest(Box::new(ScriptedProvider(script.clone())));
    instance
        .activate(Activation {
            plugin: PluginId(PROVIDER.to_owned()),
            api: "ilium:plugin@0.1.0".to_owned(),
            instance: 1,
            side: "server".to_owned(),
            settings_json: "{}".to_owned(),
            granted: Vec::new(),
            restored: None,
        })
        .expect("activates");
    runtime.plugins.insert(
        PROVIDER.to_owned(),
        PluginRecord {
            manifest: json!({"id": PROVIDER, "contributes": {"services": {"provides": [
                {"name": SERVICE, "version": "0.1.0", "schema": "schemas/lookup.json"}
            ]}}}),
            tier: Tier::T2,
            granted: vec![format!("services.provide:{SERVICE}")],
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
    (runtime, script, log)
}

fn caller(runtime: &HostRuntime) -> BrokeredServices {
    BrokeredServices::new(
        runtime.services.clone(),
        CALLER,
        &[format!("services.use:{SERVICE}")],
    )
}

fn provider(runtime: &HostRuntime) -> BrokeredServices {
    BrokeredServices::new(
        runtime.services.clone(),
        PROVIDER,
        &[format!("services.provide:{SERVICE}")],
    )
}

/// Runs the service pump once and returns the outcome of `request` for the caller.
fn route(runtime: &mut HostRuntime, request: RequestId) -> Outcome {
    let failures = runtime.route_services();
    assert!(failures.is_empty(), "{failures:?}");
    let ended: Vec<_> = runtime
        .results
        .lock()
        .expect("results lock")
        .drain(..)
        .collect();
    ended
        .into_iter()
        .find(|(plugin, result)| plugin == CALLER && result.request == RequestRef::Call(request))
        .map(|(_, result)| result.outcome)
        .expect("the request completed")
}

fn failed_code(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Failed(error) => Some(error.code),
        _ => None,
    }
}

fn errors_of(log: &RecordingLog) -> Vec<(String, String)> {
    log.0
        .lock()
        .expect("log lock")
        .iter()
        .filter(|(_, level, _)| *level == Level::Error)
        .map(|(plugin, _, message)| (plugin.clone(), message.clone()))
        .collect()
}

/// REQ: SVC-003
#[test]
fn a_request_the_schema_refuses_never_reaches_the_provider() {
    let (mut runtime, script, _) = runtime_with_provider();
    let mut caller = caller(&runtime);
    let request = caller
        .call(
            SERVICE.to_owned(),
            "lookup".to_owned(),
            r#"{"term": 7}"#.to_owned(),
        )
        .expect("queued");
    let outcome = route(&mut runtime, request);
    assert_eq!(failed_code(&outcome), Some(ErrorCode::InvalidArgument));
    if let Outcome::Failed(error) = &outcome {
        assert!(error.message.contains("/term"), "{}", error.message);
    }
    assert!(script.lock().expect("script").seen.is_empty());
    assert_eq!(caller.take_reply(request), None);
}

#[test]
fn a_matching_request_reaches_the_provider_and_its_reply_the_caller() {
    let (mut runtime, script, _) = runtime_with_provider();
    script
        .lock()
        .expect("script")
        .replies
        .push_back(ServiceReply::Done(r#"{"found":true}"#.to_owned()));
    let mut caller = caller(&runtime);
    let request = caller
        .call(
            SERVICE.to_owned(),
            "lookup".to_owned(),
            r#"{"term":"latency"}"#.to_owned(),
        )
        .expect("queued");
    assert_eq!(route(&mut runtime, request), Outcome::Ok);
    assert_eq!(
        script.lock().expect("script").seen,
        vec![("lookup".to_owned(), r#"{"term":"latency"}"#.to_owned())]
    );
    assert_eq!(
        caller.take_reply(request),
        Some(r#"{"found":true}"#.to_owned())
    );
}

/// REQ: SVC-009
#[test]
fn an_immediate_reply_the_schema_refuses_fails_the_caller_and_logs_an_error() {
    let (mut runtime, script, log) = runtime_with_provider();
    script
        .lock()
        .expect("script")
        .replies
        .push_back(ServiceReply::Done(r#"{"found":"yes"}"#.to_owned()));
    let mut caller = caller(&runtime);
    let request = caller
        .call(
            SERVICE.to_owned(),
            "lookup".to_owned(),
            r#"{"term":"x"}"#.to_owned(),
        )
        .expect("queued");
    let outcome = route(&mut runtime, request);
    assert_eq!(failed_code(&outcome), Some(ErrorCode::InvalidArgument));
    assert_eq!(caller.take_reply(request), None);
    let errors = errors_of(&log);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, PROVIDER);
    assert!(errors[0].1.contains("/found"), "{}", errors[0].1);
}

/// REQ: SVC-009
#[test]
fn a_deferred_reply_the_schema_refuses_is_refused_and_logged() {
    let (mut runtime, script, log) = runtime_with_provider();
    script
        .lock()
        .expect("script")
        .replies
        .push_back(ServiceReply::Deferred);
    let mut caller = caller(&runtime);
    let request = caller
        .call(
            SERVICE.to_owned(),
            "lookup".to_owned(),
            r#"{"term":"x"}"#.to_owned(),
        )
        .expect("queued");
    let failures = runtime.route_services();
    assert!(failures.is_empty(), "{failures:?}");
    assert!(runtime.results.lock().expect("results").is_empty());
    // The provider answers later, with a reply its own schema refuses.
    let call = *script
        .lock()
        .expect("script")
        .calls
        .first()
        .expect("the provider saw the call");
    let mut provider = provider(&runtime);
    let refused = provider
        .reply(call, Ok("{}".to_owned()))
        .expect_err("the reply lacks /found");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    let outcome = route(&mut runtime, request);
    assert_eq!(failed_code(&outcome), Some(ErrorCode::InvalidArgument));
    let errors = errors_of(&log);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, PROVIDER);
}

#[test]
fn describe_returns_the_schema_text_without_calling_the_provider() {
    let (mut runtime, script, _) = runtime_with_provider();
    let mut caller = caller(&runtime);
    let request = caller
        .call(SERVICE.to_owned(), "describe".to_owned(), String::new())
        .expect("queued");
    assert_eq!(route(&mut runtime, request), Outcome::Ok);
    assert_eq!(caller.take_reply(request), Some(SCHEMA.to_owned()));
    assert!(script.lock().expect("script").seen.is_empty());
    assert_eq!(runtime.service_schema(SERVICE), Some(SCHEMA.to_owned()));
}
