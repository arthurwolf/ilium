//! Shared helpers of the lane extensions-services: manifest builders, scripted handlers, the hook
//! world and the manual reader. Every missing host item is referenced from here and from the test
//! files through `use crate::support::*` (see API_GAPS.md).

use std::path::PathBuf;

pub use ilium_plugin_api::generated::exports::hook_handler::HookCall;
pub use ilium_plugin_api::generated::exports::hook_handler::HookReply;
pub use ilium_plugin_api::generated::exports::menu_provider::MenuContext;
pub use ilium_plugin_api::generated::exports::service_provider::{ServiceCall, ServiceReply};
pub use ilium_plugin_api::generated::host::catalogs::CatalogQuery;
pub use ilium_plugin_api::generated::types::ids::BufferId;
pub use ilium_plugin_api::generated::types::ids::RequestId;
pub use ilium_plugin_api::generated::types::requests::Outcome;
pub use ilium_plugin_api::{Diagnostic, ErrorCode, PluginError, Side, Tier};
pub use ilium_plugin_host::instance::{ExportCall, ExportReply};
pub use ilium_plugin_host::testing::{ExtWorld, HandlerAction, HandlerScript};
pub use proptest::prelude::*;
pub use serde_json::{json, Value};

/// The owner plugin of the standard hook world.
pub const OWNER: &str = "com.example.owner";
/// The global name of the standard point of the standard hook world.
pub const POINT: &str = "com.example.owner/p";
/// 64 KiB, the payload, reply and chain-result cap (EXT-004, EXT-023).
pub const KIB64: usize = 64 * 1024;
/// 16 KiB, the entry cap (EXT-023).
pub const KIB16: usize = 16 * 1024;
/// 1 MiB, the service and message cap (SVC-003).
pub const MIB1: usize = 1024 * 1024;

// ---------------------------------------------------------------------------------------------
// Errors and outcomes
// ---------------------------------------------------------------------------------------------

/// A `plugin-error` as a guest returns it.
pub fn perr(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// The base code of a failed call.
pub fn code_of(error: &PluginError) -> ErrorCode {
    error.code
}

/// True when the message starts with the extension code, a colon and a space (EXT-003).
pub fn message_has_prefix(error: &PluginError, code: &str) -> bool {
    error.message.starts_with(&format!("{code}: "))
}

/// Unwraps an `Err` without panicking on the expected path: returns the error or fails the test
/// through an assertion that names the unexpected success.
pub fn err_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> PluginError {
    match result {
        Err(error) => error,
        Ok(value) => panic!("expected an error, got Ok({value:?})"),
    }
}

/// The refusal error of an outcome, or a failed assertion.
pub fn refused_error(outcome: &Option<Outcome>) -> PluginError {
    match outcome {
        Some(Outcome::Refused(error)) => error.clone(),
        other => panic!("expected refused(..), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Scripted handlers
// ---------------------------------------------------------------------------------------------

pub fn script(elapsed_ms: u32, action: HandlerAction) -> HandlerScript {
    HandlerScript { elapsed_ms, action }
}

pub fn pass(ms: u32) -> HandlerScript {
    script(ms, HandlerAction::Reply(HookReply::Pass))
}

pub fn replace(ms: u32, payload_json: &str) -> HandlerScript {
    script(
        ms,
        HandlerAction::Reply(HookReply::Replace(payload_json.to_owned())),
    )
}

pub fn refuse(ms: u32, reason: &str) -> HandlerScript {
    script(
        ms,
        HandlerAction::Reply(HookReply::Refuse(reason.to_owned())),
    )
}

pub fn answer(ms: u32, payload_json: &str) -> HandlerScript {
    script(
        ms,
        HandlerAction::Reply(HookReply::Answer(payload_json.to_owned())),
    )
}

pub fn trap(ms: u32) -> HandlerScript {
    script(ms, HandlerAction::Trap)
}

pub fn error_return(ms: u32) -> HandlerScript {
    script(
        ms,
        HandlerAction::Fail(perr(ErrorCode::Internal, "handler failed")),
    )
}

pub fn runs_hook(ms: u32, point: &str, payload_json: &str) -> HandlerScript {
    script(
        ms,
        HandlerAction::RunHook {
            point: point.to_owned(),
            payload_json: payload_json.to_owned(),
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Manifest builders (shapes from api-extension-points.md and reference-manifest.md)
// ---------------------------------------------------------------------------------------------

/// The payload schema of the standard point: `text` and `n`.
pub fn payload_schema() -> Value {
    json!({"type":"object","properties":{"text":{"type":"string","maxLength":65536},"n":{"type":"integer"}}})
}

/// A reply schema that lists `text` as the only changeable field.
pub fn reply_schema_text() -> Value {
    json!({"type":"object","properties":{"text":{"type":"string","maxLength":65536}}})
}

/// A reply schema of a `first` point.
pub fn reply_schema_answer() -> Value {
    json!({"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}})
}

/// A hook point of the given mode, with the schemas its mode needs.
pub fn hook_point(id: &str, mode: &str) -> Value {
    let mut point = json!({
        "id": id, "kind": "hook", "mode": mode, "title": "Test point",
        "payload_schema": payload_schema(), "risk": "standard",
        "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0"
    });
    if mode == "transform" {
        point["reply_schema"] = reply_schema_text();
    }
    if mode == "first" {
        point["reply_schema"] = reply_schema_answer();
    }
    point
}

/// A collect point.
pub fn collect_point(id: &str, max_entries: u32, dynamic: bool) -> Value {
    json!({
        "id": id, "kind": "collect", "title": "Test collect point",
        "entry_schema": {"type":"object","required":["id"],"properties":{"id":{"type":"string"},"regex":{"type":"string","maxLength":512}}},
        "max_entries": max_entries, "dynamic": dynamic, "version": "0.1.0"
    })
}

/// Sets one field of a point JSON.
pub fn with(mut value: Value, key: &str, field: Value) -> Value {
    value[key] = field;
    value
}

fn side_key(side: &str) -> &'static str {
    if side == "client" {
        "client"
    } else {
        "server"
    }
}

/// An owner manifest: one provide grant per point.
pub fn owner_manifest(owner: &str, points: Vec<Value>) -> Value {
    owner_manifest_on(owner, "server", points)
}

pub fn owner_manifest_on(owner: &str, side: &str, points: Vec<Value>) -> Value {
    let grants: Vec<String> = points
        .iter()
        .map(|point| {
            format!(
                "extensions.provide:{owner}/{}",
                point["id"].as_str().unwrap_or("")
            )
        })
        .collect();
    json!({
        "id": owner,
        "capabilities": { side_key(side): grants },
        "contributes": { "extension_points": points }
    })
}

/// A contributor manifest: one contribute grant per distinct point, `hook-handler` exported.
pub fn contributor_manifest(id: &str, exts: Vec<Value>) -> Value {
    contributor_manifest_on(id, "server", exts)
}

pub fn contributor_manifest_on(id: &str, side: &str, exts: Vec<Value>) -> Value {
    let mut grants: Vec<String> = Vec::new();
    for ext in &exts {
        let point = ext["point"].as_str().unwrap_or("");
        let bare = point.split('@').next().unwrap_or("");
        let grant = format!("extensions.contribute:{bare}");
        if !grants.contains(&grant) {
            grants.push(grant);
        }
    }
    json!({
        "id": id,
        "exports": ["hook-handler"],
        "capabilities": { side_key(side): grants },
        "contributes": { "extensions": exts }
    })
}

/// A hook contribution.
pub fn ext_handler(point: &str, id: &str, priority: u8, handler: &str) -> Value {
    json!({"point": point, "id": id, "priority": priority, "handler": handler})
}

/// A hook contribution that is also a guard.
pub fn ext_guard(point: &str, id: &str, priority: u8, handler: &str) -> Value {
    json!({"point": point, "id": id, "priority": priority, "handler": handler, "guard": true})
}

/// A collect contribution.
pub fn ext_entry(point: &str, id: &str, priority: u8, entry: Value) -> Value {
    json!({"point": point, "id": id, "priority": priority, "entry": entry})
}

// ---------------------------------------------------------------------------------------------
// The standard hook world
// ---------------------------------------------------------------------------------------------

/// One scripted contributor of the standard hook world.
pub struct Contrib {
    pub plugin: String,
    pub extension: String,
    pub priority: u8,
    pub tier: Tier,
    pub script: HandlerScript,
    pub guard: bool,
}

/// The shape of a replacement plugin that deviates from its core plugin (EXT-051); the world
/// builds the manifest from it, so the type is the host's.
pub use ilium_plugin_host::testing::ReplacementShape;

impl Contrib {
    /// A T3 contributor with the given full plugin id; its extension id is `e-<last segment>`.
    pub fn new(plugin: &str, priority: u8, script: HandlerScript) -> Contrib {
        let last = plugin.rsplit('.').next().unwrap_or(plugin);
        Contrib {
            plugin: plugin.to_owned(),
            extension: format!("e-{last}"),
            priority,
            tier: Tier::T3,
            script,
            guard: false,
        }
    }

    pub fn guard(plugin: &str, priority: u8, script: HandlerScript) -> Contrib {
        Contrib {
            guard: true,
            ..Contrib::new(plugin, priority, script)
        }
    }
}

/// A T3 contributor `com.example.<name>` with extension id `e-<name>`.
pub fn contrib(name: &str, priority: u8, script: HandlerScript) -> Contrib {
    Contrib {
        plugin: format!("com.example.{name}"),
        extension: format!("e-{name}"),
        priority,
        tier: Tier::T3,
        script,
        guard: false,
    }
}

pub fn contrib_tier(name: &str, priority: u8, tier: Tier, script: HandlerScript) -> Contrib {
    Contrib {
        tier,
        ..contrib(name, priority, script)
    }
}

/// Installs a contributor of the standard point into a world.
pub fn install_contrib(world: &mut ExtWorld, point: &str, c: &Contrib) -> Result<(), Diagnostic> {
    let entry = if c.guard {
        ext_guard(point, &c.extension, c.priority, "h")
    } else {
        ext_handler(point, &c.extension, c.priority, "h")
    };
    let manifest = contributor_manifest(&c.plugin, vec![entry]);
    world.install(c.tier, manifest, vec![("h".to_owned(), c.script.clone())])
}

/// A world with the standard owner (point `p` of the given mode) and the given contributors,
/// installed in the given order. A failed install fails the test.
pub fn hook_world(mode: &str, contribs: Vec<Contrib>) -> ExtWorld {
    hook_world_with(hook_point("p", mode), contribs)
}

pub fn hook_world_with(point: Value, contribs: Vec<Contrib>) -> ExtWorld {
    let mut world = ExtWorld::new();
    let installed = world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new());
    assert!(installed.is_ok(), "owner install failed: {installed:?}");
    for c in &contribs {
        let installed = install_contrib(&mut world, POINT, c);
        assert!(
            installed.is_ok(),
            "contributor {} install failed: {installed:?}",
            c.plugin
        );
    }
    world.settle();
    world
}

/// Runs the chain of the standard point as the owner and returns the completion and the reply.
pub fn run_chain(world: &mut ExtWorld, payload_json: &str) -> (Option<Outcome>, Option<String>) {
    run_chain_on(world, OWNER, POINT, payload_json)
}

pub fn run_chain_on(
    world: &mut ExtWorld,
    caller: &str,
    point: &str,
    payload_json: &str,
) -> (Option<Outcome>, Option<String>) {
    let request = match world.run_hook(caller, point, payload_json) {
        Ok(request) => request,
        Err(error) => panic!("run-hook failed synchronously: {error:?}"),
    };
    world.settle();
    let outcome = world.outcome(caller, request);
    let reply = world.take_reply(caller, request);
    (outcome, reply)
}

/// The plugin ids of the handler runs of the world, in start order.
pub fn run_order(world: &ExtWorld) -> Vec<String> {
    world
        .handler_runs()
        .into_iter()
        .map(|run| run.plugin)
        .collect()
}

/// True when any captured log line contains the code.
pub fn has_log(world: &ExtWorld, code: &str) -> bool {
    world
        .all_logs()
        .iter()
        .any(|line| line.message.contains(code))
}

/// The number of captured log lines that contain the code.
pub fn count_logs(world: &ExtWorld, code: &str) -> usize {
    world
        .all_logs()
        .iter()
        .filter(|line| line.message.contains(code))
        .count()
}

/// A JSON string of the given byte length inside `{"text": ...}`: the whole payload has `total`
/// bytes.
pub fn payload_of_size(total: usize) -> String {
    let overhead = r#"{"text":""}"#.len();
    format!(
        r#"{{"text":"{}"}}"#,
        "a".repeat(total.saturating_sub(overhead))
    )
}

/// The code of an install refusal.
pub fn diag_code(result: &Result<(), Diagnostic>) -> String {
    match result {
        Err(diagnostic) => diagnostic.code.as_str().to_owned(),
        Ok(()) => "ok".to_owned(),
    }
}

// ---------------------------------------------------------------------------------------------
// The manual
// ---------------------------------------------------------------------------------------------

/// The directory of the manual pages.
pub fn pages_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ILIUM_PLUGIN_MANUAL_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages")
}

/// The text of one manual page; an unreadable page fails the calling test.
pub fn page(name: &str) -> String {
    let path = pages_dir().join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("cannot read manual page {}: {error}", path.display()),
    }
}

/// The rows of the first markdown table that follows the heading line `heading`, as cells.
pub fn table_after(text: &str, heading: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut seen_heading = false;
    let mut in_table = false;
    for line in text.lines() {
        if !seen_heading {
            if line.trim() == heading {
                seen_heading = true;
            }
            continue;
        }
        if line.starts_with('|') {
            in_table = true;
            let cells: Vec<String> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect();
            if cells
                .iter()
                .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
            {
                continue;
            }
            rows.push(cells);
        } else if in_table {
            break;
        }
    }
    if !rows.is_empty() {
        rows.remove(0);
    }
    rows
}

/// The points of the "All points" table of the core points page: (name, kind cell, risk).
pub fn core_points() -> Vec<(String, String, String)> {
    let text = page("reference-core-extension-points.md");
    table_after(&text, "### All points")
        .into_iter()
        .map(|row| {
            (
                row[0].trim_matches('`').to_owned(),
                row[1].clone(),
                row[2].clone(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Services, topics, messages
// ---------------------------------------------------------------------------------------------

/// The inline schema of the test services: `echo` takes `{text: string}` and returns a string,
/// `query` takes any object and returns anything.
pub fn service_schema() -> Value {
    json!({
        "echo": {
            "params": {"type":"object","properties":{"text":{"type":"string"}}},
            "result": {"type":"string"}
        },
        "query": {"params": {"type":"object"}, "result": {}}
    })
}

/// A provider plugin of one service.
pub fn provider_manifest(id: &str, service: &str, version: &str) -> Value {
    json!({
        "id": id,
        "exports": ["service-provider"],
        "capabilities": {"server": [format!("services.provide:{service}")]},
        "contributes": {"services": {"provides": [
            {"name": service, "version": version, "schema": service_schema()}
        ]}}
    })
}

/// A consumer plugin that holds `services.use` and requires the service at a range.
pub fn consumer_manifest(id: &str, service: &str, requires: Option<&str>) -> Value {
    let mut manifest = json!({
        "id": id,
        "capabilities": {"server": [format!("services.use:{service}")]}
    });
    if let Some(range) = requires {
        manifest["requires"] = json!({"services": [format!("{service}@{range}")]});
    }
    manifest
}

/// The handler key of a service method in `ExtWorld::install`.
pub fn method_key(service: &str, method: &str) -> String {
    format!("{service}#{method}")
}

pub fn done(ms: u32, text: &str) -> HandlerScript {
    script(ms, HandlerAction::ServiceDone(text.to_owned()))
}

pub const SERVICE: &str = "com.example.prov/svc";

/// A world with a provider of `SERVICE` at `version` answering `echo` with the given script, and
/// a consumer `com.example.cons` that holds `services.use`.
pub fn service_world(version: &str, echo: HandlerScript) -> ExtWorld {
    let mut world = ExtWorld::new();
    let installed = world.install(
        Tier::T3,
        provider_manifest("com.example.prov", SERVICE, version),
        vec![
            (method_key(SERVICE, "echo"), echo),
            (method_key(SERVICE, "query"), done(1, "[]")),
        ],
    );
    assert!(installed.is_ok(), "provider install failed: {installed:?}");
    let installed = world.install(
        Tier::T3,
        consumer_manifest("com.example.cons", SERVICE, None),
        Vec::new(),
    );
    assert!(installed.is_ok(), "consumer install failed: {installed:?}");
    world.settle();
    world
}

/// Calls `echo` as the consumer and returns the request id of an accepted call.
pub fn call_echo(world: &mut ExtWorld, payload_json: &str) -> Result<RequestId, PluginError> {
    world.call_service("com.example.cons", SERVICE, "echo", payload_json)
}

/// A payload of `echo` whose JSON text has exactly `total` bytes.
pub fn echo_payload_of_size(total: usize) -> String {
    let overhead = r#"{"text":""}"#.len();
    format!(
        r#"{{"text":"{}"}}"#,
        "a".repeat(total.saturating_sub(overhead))
    )
}

/// Menu-provider harness (gap: MenuHarness, see API_GAPS.md).
pub mod menu {
    pub use ilium_plugin_api::generated::exports::menu_provider::{MenuContext, MenuEntry};
    pub use ilium_plugin_host::testing::MenuHarness;
}
