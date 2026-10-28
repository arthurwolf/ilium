//! Progress-lane testing hooks on `FakeHost` (startup phase publishing, durable-write faults).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. The startup phase model belongs to the
//! plugin: every phase hook calls the service `ilium.core.progress/startup` (methods `begin`, `publish`,
//! `finish`, `cancel`, `marker`, `admitted`, `block-write`, `release-write`) so the fencing rules under test run
//! in the plugin, not in this file. A plugin that does not answer the service fails the calling test.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::{json, Value};

/// One startup phase handle returned by `begin_startup_phase`.
pub type StartupPhase = u64;

const STARTUP_SERVICE: &str = "ilium.core.progress/startup";

/// Calls one method of the startup service; a failure stops the test with the method named.
fn startup_call(host: &mut FakeHost, method: &str, payload: Value) -> Value {
    let text = host
        .call_service(STARTUP_SERVICE, method, &payload.to_string())
        .unwrap_or_else(|error| panic!("{STARTUP_SERVICE}.{method} failed: {error:?}"));
    if text.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{STARTUP_SERVICE}.{method} replied invalid JSON: {error}"))
}

/// Progress-lane hooks. Implemented for `FakeHost`.
pub trait ProgressHostExt {
    /// Opens a new startup phase; older phases are retired (progress "Startup marker").
    fn begin_startup_phase(&mut self) -> StartupPhase;
    /// Publishes a startup marker for `phase`: `(title, step, done, total)`.
    fn startup_publish(&mut self, phase: StartupPhase, title: &str, step: &str, done: u32, total: u32);
    /// Finishes `phase`, clearing its marker.
    fn startup_finish(&mut self, phase: StartupPhase);
    /// Cancels the startup sequence.
    fn cancel_startup(&mut self);
    /// The current startup marker: `(title, step, done, total)`.
    fn startup_marker(&mut self) -> Option<(String, String, u32, u32)>;
    /// Whether the startup callback is still admitted.
    fn startup_callback_admitted(&mut self) -> bool;
    /// Blocks the next native startup write until `release_startup_write`.
    fn block_next_startup_write(&mut self);
    /// Releases a blocked native startup write.
    fn release_startup_write(&mut self);
    /// Fails the next durable write.
    fn fail_next_durable_write(&mut self);
    /// Sets the footer split orientation (`"vertical"` or `"horizontal"`).
    fn set_split_orientation(&mut self, orientation: &str);
}

impl ProgressHostExt for FakeHost {
    fn begin_startup_phase(&mut self) -> StartupPhase {
        startup_call(self, "begin", json!({}))["phase"]
            .as_u64()
            .expect("the startup service answers begin with a phase number")
    }

    fn startup_publish(&mut self, phase: StartupPhase, title: &str, step: &str, done: u32, total: u32) {
        startup_call(
            self,
            "publish",
            json!({ "phase": phase, "title": title, "step": step, "done": done, "total": total }),
        );
    }

    fn startup_finish(&mut self, phase: StartupPhase) {
        startup_call(self, "finish", json!({ "phase": phase }));
    }

    fn cancel_startup(&mut self) {
        startup_call(self, "cancel", json!({}));
    }

    fn startup_marker(&mut self) -> Option<(String, String, u32, u32)> {
        let reply = startup_call(self, "marker", json!({}));
        let marker = reply.get("marker")?;
        if marker.is_null() {
            return None;
        }
        Some((
            marker["title"].as_str()?.to_string(),
            marker["step"].as_str()?.to_string(),
            u32::try_from(marker["done"].as_u64()?).ok()?,
            u32::try_from(marker["total"].as_u64()?).ok()?,
        ))
    }

    fn startup_callback_admitted(&mut self) -> bool {
        startup_call(self, "admitted", json!({}))["admitted"]
            .as_bool()
            .expect("the startup service answers admitted with a flag")
    }

    fn block_next_startup_write(&mut self) {
        startup_call(self, "block-write", json!({}));
    }

    fn release_startup_write(&mut self) {
        startup_call(self, "release-write", json!({}));
    }

    fn fail_next_durable_write(&mut self) {
        self.inject_failure(
            "storage.put",
            PluginError::new(ErrorCode::Io, "the durable write failed"),
        );
    }

    /// The footer reads the orientation from the workspace kernel settings tab.
    fn set_split_orientation(&mut self, orientation: &str) {
        self.kernel_settings()
            .set("workspace", "split_orientation", json!(orientation))
            .expect("the workspace tab accepts the split orientation");
    }
}
