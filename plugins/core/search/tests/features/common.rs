//! Shared helpers of the Workspace search tests (manual page core-plugin-search.md).
//!
//! Mounted by both the feature tests (features/main.rs) and the port tests (port/main.rs). Helpers
//! that only the feature tests use live in support.rs, which only the feature tests mount.

use std::path::PathBuf;

use ilium_plugin_core_search::SearchClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    Submission, UiAction, ViewTarget,
};
use serde_json::Value;

pub const PANEL: &str = "search";
pub const OPEN_COMMAND: &str = "ilium.core.search/open";
/// Channel the client sends a query on (gap helper, see API_GAPS.md).
pub const QUERY_CHANNEL: &str = "ilium.core.search/query";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build()
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<SearchClient>();
    host
}

pub fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

pub fn open_command_event() -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: OPEN_COMMAND.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Keybinding,
        context: CommandContext::default(),
    })
}

/// A `ui-action` of the panel with a value.
pub fn action(key: &str, kind: ActionKind, value: ActionValue) -> Event {
    Event::UiAction(UiAction {
        view: panel(),
        key: key.to_string(),
        action_id: String::new(),
        kind,
        value: Some(value),
    })
}

pub fn text_changed(text: &str) -> Event {
    action(
        "query",
        ActionKind::Changed,
        ActionValue::Text(text.to_string()),
    )
}

pub fn filter_changed(text: &str) -> Event {
    action(
        "query",
        ActionKind::FilterChanged,
        ActionValue::Text(text.to_string()),
    )
}

// ---------------------------------------------------------------------------------------------
// Gap helper shared by the port tests (../API_GAPS.md).
// ---------------------------------------------------------------------------------------------

/// A client-to-server query message the way the plugin's client side sends it: every `message`
/// intent on `QUERY_CHANNEL` with its JSON payload `{ "revision", "query" }`.
pub fn query_messages(submissions: &[Submission]) -> Vec<Value> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) if spec.channel == QUERY_CHANNEL => {
                Some(serde_json::from_slice(&spec.payload).expect("query payload is JSON"))
            }
            _ => None,
        })
        .collect()
}
