//! Shared helpers for the sdk-fakehost lane: test plugins, panic capture, environment lock and temp files.
//!
//! Part 2 of the lane extends this file. Keep every helper here free of assertions about a specific
//! requirement; the section modules own the REQ tags.


use std::cell::RefCell;
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ilium_plugin_sdk_core::types::{
    Activation, ClientId, DeactivationReason, ErrorCode, Event, Intent, NodeId, PluginError,
    Submission,
};
use ilium_plugin_sdk_core::NativeEntry;

/// The default fake clock: 2030-01-01 00:00:00 UTC in milliseconds since the Unix epoch.
pub const DEFAULT_CLOCK_MS: u64 = 1_893_456_000_000;

/// The 15 values of the WIT `error-code` enum, in kebab-case (reference-errors-and-limits.md).
pub const ERROR_CODE_NAMES: [&str; 15] = [
    "not-permitted",
    "not-found",
    "invalid-argument",
    "conflict",
    "quota-exceeded",
    "rate-limited",
    "busy",
    "timed-out",
    "cancelled",
    "unsupported",
    "pane-not-ready",
    "revoked",
    "io",
    "network",
    "internal",
];

// ---------------------------------------------------------------------------------------------
// Panic capture. Builder and harness failures have no Result channel in the skeleton, so a test
// asserts the dotted code inside the panic text (DEV-410, DEV-421 and the other harness rows).
// ---------------------------------------------------------------------------------------------

/// Runs `f` and returns the panic text, or `None` when `f` returned normally.
pub fn panic_text<R>(f: impl FnOnce() -> R) -> Option<String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(_) => None,
        Err(payload) => Some(payload_text(payload.as_ref())),
    }
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    String::from("<non-string panic payload>")
}

/// Asserts that `f` panics and that the panic text contains the dotted `code`.
pub fn assert_fails_with<R>(code: &str, f: impl FnOnce() -> R) {
    match panic_text(f) {
        None => panic!("expected a failure with code {code}, but the call returned normally"),
        Some(text) => assert!(
            text.contains(code),
            "expected the failure text to contain {code:?}, got {text:?}"
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Environment lock. std::env::set_var is process-global and tests run in parallel, so every test
// that reads or writes one of the harness variables runs inside `with_env`.
// ---------------------------------------------------------------------------------------------

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// The variables the harness reads.
pub const HARNESS_ENV: [&str; 4] = [
    "ILIUM_PLUGIN_TEST_SEED",
    "ILIUM_PLUGIN_TEST_MODE",
    "ILIUM_PLUGIN_UPDATE_GOLDEN",
    "CI",
];

/// Runs `f` with the harness variables set to `settings` and every other harness variable removed.
/// The previous values come back afterwards, also when `f` panics.
pub fn with_env<R>(settings: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
    let guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let saved: Vec<(&str, Option<String>)> = HARNESS_ENV
        .iter()
        .map(|name| (*name, std::env::var(name).ok()))
        .collect();
    for name in HARNESS_ENV {
        // SAFETY-free in edition 2021: set_var and remove_var are safe functions here.
        std::env::remove_var(name);
    }
    for (name, value) in settings {
        std::env::set_var(name, value);
    }
    let result = catch_unwind(AssertUnwindSafe(f));
    for (name, value) in saved {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    drop(guard);
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// `with_env` with no harness variable set.
pub fn with_clean_env<R>(f: impl FnOnce() -> R) -> R {
    with_env(&[], f)
}

// ---------------------------------------------------------------------------------------------
// Temp files under the cargo test temp dir, never under the real HOME.
// ---------------------------------------------------------------------------------------------

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty directory unique to one call.
pub fn temp_dir(label: &str) -> PathBuf {
    let serial = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sdk_fakehost")
        .join(format!("{label}-{}-{serial}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the test temp dir");
    dir
}

/// Writes `text` to `name` inside `dir` and returns the full path.
pub fn write_file(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create the parent dir");
    }
    std::fs::write(&path, text).expect("write the test file");
    path
}

/// A minimal valid manifest with a server and a client side and one grant list per side.
pub fn manifest_text(id: &str, server_grants: &[&str], client_grants: &[&str]) -> String {
    let quote = |names: &[&str]| {
        names
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        r#"{{
  "api": "ilium:plugin@0.1.0",
  "id": "{id}",
  "name": "Test plugin",
  "version": "0.1.0",
  "publisher": "Example Publisher",
  "server": {{ "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["handler"] }},
  "client": {{ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] }},
  "capabilities": {{ "server": [{}], "client": [{}] }}
}}
"#,
        quote(server_grants),
        quote(client_grants)
    )
}

// ---------------------------------------------------------------------------------------------
// Test plugins. Each one is `Default`, because `FakeHost::activate::<P>()` builds it with
// `P::default()`. Observations go to a thread-local log because the host owns the instance.
// ---------------------------------------------------------------------------------------------

/// What the `Spy` plugin saw and what it will answer.
#[derive(Default, Debug, Clone)]
pub struct SpyState {
    pub constructed: u32,
    pub activations: Vec<Activation>,
    pub deactivations: Vec<DeactivationReason>,
    pub batches: Vec<Vec<Event>>,
    /// One entry is consumed per `native_on_events` call; an empty queue answers with no submission.
    pub replies: VecDeque<Vec<Submission>>,
    /// When set, every call answers with this submission once more (settle-limit tests).
    pub always: Option<Submission>,
    /// The bytes `native_deactivate` returns.
    pub restore: Option<Vec<u8>>,
    /// When set, `native_activate` returns this error after it records the activation.
    pub fail_activation: Option<PluginError>,
}

thread_local! {
    static SPY: RefCell<SpyState> = RefCell::new(SpyState::default());
}

/// Clears the spy log. Call it first in every test that uses `Spy`.
pub fn spy_reset() {
    SPY.with(|state| *state.borrow_mut() = SpyState::default());
}

/// A copy of the spy log.
pub fn spy() -> SpyState {
    SPY.with(|state| state.borrow().clone())
}

/// Edits the spy script.
pub fn spy_edit(edit: impl FnOnce(&mut SpyState)) {
    SPY.with(|state| edit(&mut state.borrow_mut()));
}

/// Queues the answers of the next `native_on_events` calls.
pub fn spy_script(replies: Vec<Vec<Submission>>) {
    spy_edit(|state| state.replies = replies.into());
}

/// A plugin that records every call and answers from the script.
pub struct Spy {
    /// In-memory state a restart must reset (DEV-491, DEV-425).
    pub counter: u32,
}

impl Default for Spy {
    fn default() -> Self {
        spy_edit(|state| state.constructed += 1);
        Spy { counter: 0 }
    }
}

impl NativeEntry for Spy {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, context: Activation) -> Result<(), PluginError> {
        self.counter += 1;
        let failure = SPY.with(|state| {
            let mut state = state.borrow_mut();
            state.activations.push(context);
            state.fail_activation.clone()
        });
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn native_deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        SPY.with(|state| {
            let mut state = state.borrow_mut();
            state.deactivations.push(reason);
            state.restore.clone()
        })
    }

    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        SPY.with(|state| {
            let mut state = state.borrow_mut();
            state.batches.push(batch);
            if let Some(always) = state.always.clone() {
                return vec![always];
            }
            state.replies.pop_front().unwrap_or_default()
        })
    }
}

/// A plugin whose activation always fails with `not-permitted` and message `denied`.
#[derive(Default)]
pub struct Failing;

impl NativeEntry for Failing {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Err(PluginError::new(ErrorCode::NotPermitted, "denied"))
    }

    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }

    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------------------------
// Builders for events and submissions.
// ---------------------------------------------------------------------------------------------

/// `count` distinct `client-attached` events. They need no grant, so they reach a server-side plugin.
pub fn client_batch(count: usize) -> Vec<Event> {
    (0..count)
        .map(|index| Event::ClientAttached(ClientId(index as u64 + 1)))
        .collect()
}

/// A `play-sound` submission (needs the `sound.play` grant).
pub fn sound(correlation: u64) -> Submission {
    Submission {
        intent: Intent::PlaySound("ding".to_string()),
        correlation,
        dedup_key: None,
    }
}

/// A `play-sound` submission with a dedup key.
pub fn sound_keyed(correlation: u64, key: &str) -> Submission {
    Submission {
        intent: Intent::PlaySound("ding".to_string()),
        correlation,
        dedup_key: Some(key.to_string()),
    }
}

/// A `close-node` submission (needs the `tree.write` grant).
pub fn close_node(correlation: u64, node: u64) -> Submission {
    Submission {
        intent: Intent::CloseNode(NodeId(node)),
        correlation,
        dedup_key: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Small conversions used by several sections.
// ---------------------------------------------------------------------------------------------

/// `["a", "b"]` as owned strings.
pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

/// The error code of a refused or failed outcome.
pub fn outcome_code(outcome: &ilium_plugin_sdk_core::types::Outcome) -> Option<ErrorCode> {
    use ilium_plugin_sdk_core::types::Outcome;
    match outcome {
        Outcome::Refused(error) | Outcome::Failed(error) => Some(error.code),
        _ => None,
    }
}

/// The kebab-case name of an error code, as it appears on the wire.
pub fn code_name(code: ErrorCode) -> String {
    match serde_json::to_value(code) {
        Ok(serde_json::Value::String(name)) => name,
        other => panic!("an error code must serialize to a string, got {other:?}"),
    }
}
