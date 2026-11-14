//! The client side of `ilium.core.recap`: it answers the one question the server cannot know,
//! whether first-run onboarding is finished, so a half-finished setup never sends text to a
//! provider.

use ilium_plugin_sdk::host::{HostClientRequests, HostKernelSettings};
use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::server::onboarding_finished;

/// The kind of client request the server sends.
pub const ONBOARDING_REQUEST: &str = "onboarding-state";

/// The `{"finished": bool}` answer, read from the kernel `setup` tab. An unreadable tab means
/// not finished, so the gate stays closed.
pub fn onboarding_answer(api: &mut dyn HostApi) -> String {
    let finished = HostKernelSettings::read(api, "setup".to_string())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|value| onboarding_finished(&value));
    json!({"finished": finished}).to_string()
}

/// Answers one client request; an unknown kind is refused.
pub fn answer(api: &mut dyn HostApi, call: u64, kind: &str) {
    let outcome = if kind == ONBOARDING_REQUEST {
        Ok(onboarding_answer(api))
    } else {
        Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("ilium.core.recap: unknown client request {kind}"),
        ))
    };
    let _ = HostClientRequests::respond(api, call, outcome);
}
