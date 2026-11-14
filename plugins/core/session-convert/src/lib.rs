//! Core plugin `ilium.core.session-convert` (manual page core-plugin-session-convert.md).
//!
//! The pure conversion core lives in the modules below (`rollout` parses a Codex rollout, `claude_writer` maps its
//! items to Claude content, `transcript` assembles the Claude Code lines, `verify` checks them, `convert` ties the
//! three together, `paths` names the Claude project directory, `error` holds the one-line failure texts).
//! The server side (`flow`) owns a conversion: it checks the pane, freezes it, stops its agent and then runs the
//! `codex-to-claude` job (read the rollout through the host `fs` import, convert, verify, propose the transcript
//! to the Claude Code folder, read it back and open the new pane) or a private `codex app-server` (the importer
//! of Codex). The client side (`client`) offers the menu entry and draws the modal progress dialog.

pub mod claude_writer;
pub mod client;
pub mod convert;
pub mod error;
pub mod flow;
pub mod lenient_json;
pub mod paths;
pub mod prompts;
pub mod rollout;
pub mod transcript;
pub mod verify;

/// Tool results longer than this are cut while the source is parsed, so one command dump can neither dominate
/// the replayed context nor sit in memory.
pub const MAX_TOOL_RESULT_CHARS: usize = 30_000;

use ilium_plugin_sdk::generated::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::generated::exports::menu_provider::MenuEntry;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::host::{HostClock, HostFs, HostIntentsHost, HostJobs};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::{ErrorCode, FsPath, PluginError};
use ilium_plugin_sdk::{
    host, ilium_plugin, Handler, HostApi, JobRunner, Lifecycle, MenuProvider, ServiceProvider,
    Views,
};
use serde_json::{json, Value};

use crate::convert::{convert_rollout, ConvertInput, PROGRESS_EVERY_LINES};
use crate::error::ConvertError;
use crate::transcript::HashedIdentifiers;

/// The scope the job reads rollouts from (grant `fs.read:agent-sessions:codex`).
const CODEX_SESSIONS_SCOPE: &str = "agent-sessions:codex";
/// One `fs.read` call returns at most this many bytes (host limit 1 MiB).
const READ_CHUNK_BYTES: u32 = 1024 * 1024;
/// A rollout larger than this is refused: the job holds the text, the lines and the output in memory
/// (manifest `memory_mib` is 256).
const MAX_ROLLOUT_BYTES: usize = 48 * 1024 * 1024;

/// How a conversion failure reaches the caller of the job.
fn job_error(error: ConvertError) -> PluginError {
    let code = match &error {
        ConvertError::Cancelled => ErrorCode::Cancelled,
        ConvertError::EmptyConversation { .. } | ConvertError::InvalidSessionId(_) => {
            ErrorCode::InvalidArgument
        }
        ConvertError::SourceNotFound { .. } => ErrorCode::NotFound,
        ConvertError::FileTooLarge { .. } => ErrorCode::QuotaExceeded,
        ConvertError::SourceChanged { .. } => ErrorCode::Conflict,
        ConvertError::Unsupported { .. } => ErrorCode::Unsupported,
        ConvertError::SourceUnreadable { .. }
        | ConvertError::TargetWrite { .. }
        | ConvertError::InsufficientDiskSpace { .. } => ErrorCode::Io,
        _ => ErrorCode::Internal,
    };
    PluginError::new(code, error.to_string())
}

/// Reads a whole rollout of the Codex session scope in host-sized chunks.
fn read_rollout(api: &mut dyn HostApi, rollout: &str) -> Result<String, PluginError> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let chunk = HostFs::read(
            api,
            FsPath {
                scope: CODEX_SESSIONS_SCOPE.to_string(),
                path: rollout.to_string(),
            },
            bytes.len() as u64,
            READ_CHUNK_BYTES,
        )?;
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_ROLLOUT_BYTES {
            return Err(job_error(ConvertError::FileTooLarge {
                path: rollout.into(),
                bytes: bytes.len() as u64,
                maximum: MAX_ROLLOUT_BYTES as u64,
            }));
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The `codex-to-claude` job: rollout path in, converted and verified transcript out.
fn run_codex_to_claude(input: &[u8]) -> Result<Vec<u8>, PluginError> {
    let request: ConvertInput = serde_json::from_slice(input).map_err(|error| {
        PluginError::new(
            ErrorCode::InvalidArgument,
            format!("job input must be {{\"rollout\": <path>}}: {error}"),
        )
    })?;
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let text = read_rollout(api, &request.rollout)?;
    let now_millis = HostClock::now_unix_millis(api);
    let fallback_timestamp = chrono::DateTime::from_timestamp_millis(now_millis as i64)
        .unwrap_or(chrono::DateTime::UNIX_EPOCH);
    let seed = format!("{}\u{0}{now_millis}", request.rollout);
    let mut identifiers = HashedIdentifiers::new(seed);
    let output = convert_rollout(
        &text,
        std::path::Path::new(&request.rollout),
        &mut identifiers,
        fallback_timestamp,
        &mut |index, total| {
            if index % PROGRESS_EVERY_LINES != 0 {
                return Ok(());
            }
            if HostJobs::is_cancelled(&mut *api) {
                return Err(ConvertError::Cancelled);
            }
            HostJobs::report_progress(&mut *api, 100.0 * index as f32 / total.max(1) as f32);
            Ok(())
        },
    )
    .map_err(job_error)?;
    serde_json::to_vec(&output).map_err(|error| {
        PluginError::new(
            ErrorCode::Internal,
            format!("cannot encode the job output: {error}"),
        )
    })
}

/// The server side of `ilium.core.session-convert` (core-plugin-session-convert.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner", "service-provider"])]
#[derive(Default)]
pub struct SessionConvertServer;

impl Lifecycle for SessionConvertServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        // What a previous run left in the durable state is cleaned up by its watchdog timer.
        flow::reset();
        Ok(())
    }
}

impl Handler for SessionConvertServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let mut submissions = Vec::new();
        for event in batch {
            flow::on_event(api, event, &mut submissions);
        }
        submissions
    }
}

impl JobRunner for SessionConvertServer {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        if kind != "codex-to-claude" {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                format!("unknown job kind {kind}"),
            ));
        }
        run_codex_to_claude(&input)
    }
}

/// The answer of a refused service call, in the form the host turns back into an error.
fn service_error(code: ErrorCode, message: &str) -> ServiceReply {
    ServiceReply::Done(json!({"error": code.as_str(), "message": message}).to_string())
}

impl ServiceProvider for SessionConvertServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        if call.method != "start" {
            return service_error(ErrorCode::InvalidArgument, "the only method is start");
        }
        let Ok(request) = serde_json::from_str::<Value>(&call.payload_json) else {
            return service_error(ErrorCode::InvalidArgument, "the request is not JSON");
        };
        let Some(pane) = request.get("pane").and_then(Value::as_u64) else {
            return service_error(ErrorCode::InvalidArgument, "pane must be a node id");
        };
        let target = match request.get("target").and_then(Value::as_str) {
            Some(target @ ("codex" | "claude")) => target,
            _ => {
                return service_error(ErrorCode::InvalidArgument, "target must be codex or claude")
            }
        };
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let mut submissions = Vec::new();
        if let Err(refused) = flow::begin(api, pane, target, false, &mut submissions) {
            return service_error(refused.code, &refused.message);
        }
        for submission in submissions {
            if let Err(error) = HostIntentsHost::submit(&mut *api, submission) {
                return service_error(error.code, &error.message);
            }
        }
        let now = HostClock::now_unix_millis(&mut *api);
        ServiceReply::Done(json!({"conversion": format!("{pane}-{now}")}).to_string())
    }
}

/// The client side of `ilium.core.session-convert` (core-plugin-session-convert.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views", "menu-provider"])]
#[derive(Default)]
pub struct SessionConvertClient;

impl Lifecycle for SessionConvertClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        client::reset();
        Ok(())
    }
}

impl Handler for SessionConvertClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        batch.into_iter().flat_map(client::on_event).collect()
    }
}

impl Views for SessionConvertClient {
    fn render(&mut self, view: ViewTarget, area: Area) -> UiTree {
        client::render(&view, area)
    }
}

impl MenuProvider for SessionConvertClient {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        client::menu_items(&context)
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    const MANIFEST_JSON: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json parses as JSON")
    }

    fn string_list(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("value is a JSON array")
            .iter()
            .map(|item| item.as_str().expect("array item is a string").to_string())
            .collect()
    }

    /// Manual: the complete manifest names the plugin id, the api version and the display name.
    /// REQ: COR-050
    #[test]
    fn manifest_declares_the_plugin_id_api_and_name() {
        let manifest_value = manifest();
        assert_eq!(manifest_value["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest_value["id"], "ilium.core.session-convert");
        assert_eq!(manifest_value["publisher"], "Ilium");
        assert_eq!(manifest_value["name"], "Session conversion");
        assert_eq!(manifest_value["replaceable"], true);
    }

    /// Manual: both sides exist, with their worlds and the exports each side lists.
    /// REQ: COR-050
    #[test]
    fn sides_worlds_and_exports_match_the_manual() {
        let _server = SessionConvertServer::default();
        let _client = SessionConvertClient::default();
        let manifest_value = manifest();
        assert_eq!(manifest_value["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            string_list(&manifest_value["server"]["exports"]),
            vec!["job-runner", "service-provider"]
        );
        assert_eq!(manifest_value["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(
            string_list(&manifest_value["client"]["exports"]),
            vec!["views", "menu-provider"]
        );
    }

    /// Manual: activation triggers and the surface list are part of the complete manifest.
    /// REQ: COR-050
    #[test]
    fn activation_triggers_and_surface_match_the_manual() {
        let manifest_value = manifest();
        assert_eq!(
            string_list(&manifest_value["activation"]),
            vec![
                "command:ilium.core.session-convert/convert",
                "service:convert",
                "view:ilium.core.session-convert/progress"
            ]
        );
        assert_eq!(
            string_list(&manifest_value["surface"]),
            vec![
                "services:convert",
                "points:formats",
                "points:after-convert",
                "commands:convert"
            ]
        );
    }

    /// Manual: the capability grants of each side are exactly the grants listed on the page.
    /// REQ: COR-050
    #[test]
    fn capability_grants_match_the_manual_complete_manifest() {
        let manifest_value = manifest();
        let mut server_grants = string_list(&manifest_value["capabilities"]["server"]);
        server_grants.sort();
        let mut expected_server_grants = vec![
            "agent-config.propose:claude",
            "agent.read",
            "extensions.provide:ilium.core.session-convert/after-convert",
            "extensions.provide:ilium.core.session-convert/formats",
            "fs.read:agent-config:claude",
            "fs.read:agent-sessions:claude",
            "fs.read:agent-sessions:codex",
            "jobs.run",
            "notify.toast",
            "panes.create",
            "panes.create-command:claude",
            "panes.create-command:codex",
            "panes.lifecycle",
            "panes.read",
            "process.long-lived",
            "process.run:codex",
            "services.provide:ilium.core.session-convert/convert",
            "tree.read",
            "tree.write",
        ];
        expected_server_grants.sort();
        assert_eq!(server_grants, expected_server_grants);

        let mut client_grants = string_list(&manifest_value["capabilities"]["client"]);
        client_grants.sort();
        assert_eq!(
            client_grants,
            vec!["agent.read", "keys.intercept", "tree.read", "ui.panels"]
        );
    }

    /// Manual: the plugin holds no write grant on Claude's folders; it proposes the transcript
    /// through agent-config.propose, and the original transcript is read and never changed.
    /// REQ: COR-011
    #[test]
    fn plugin_holds_no_write_grant_on_agent_folders() {
        let manifest_value = manifest();
        let grants = string_list(&manifest_value["capabilities"]["server"]);
        let write_grants: Vec<&String> = grants
            .iter()
            .filter(|grant| grant.starts_with("fs.write:"))
            .collect();
        assert!(
            write_grants.is_empty(),
            "unexpected write grants: {write_grants:?}"
        );
        assert!(grants.contains(&"agent-config.propose:claude".to_string()));
        assert!(grants.contains(&"fs.read:agent-sessions:claude".to_string()));
        assert!(grants.contains(&"fs.read:agent-sessions:codex".to_string()));
    }

    /// Manual: the convert service takes a pane and only the codex or claude target; antigravity
    /// sessions cannot be converted, so an invalid target fails the schema.
    /// REQ: F-077
    #[test]
    fn convert_service_takes_pane_and_only_codex_or_claude_targets() {
        let schema =
            serde_json::from_str::<serde_json::Value>(include_str!("../schemas/convert.json"))
                .expect("the service schema parses");
        let start = &schema["start"];
        let params = &start["params"];
        assert_eq!(string_list(&params["required"]), vec!["pane", "target"]);
        assert_eq!(params["properties"]["pane"]["type"], "integer");
        let target_values = string_list(&params["properties"]["target"]["enum"]);
        assert_eq!(target_values, vec!["codex", "claude"]);
        assert!(!target_values.contains(&"antigravity".to_string()));
        assert_eq!(
            string_list(&start["result"]["required"]),
            vec!["conversion"]
        );
    }

    /// Manual: the codex-to-claude job runs under a 60 second deadline and a 256 MiB memory limit.
    /// REQ: COR-050
    #[test]
    fn codex_to_claude_job_has_its_deadline_and_memory_limit() {
        let manifest_value = manifest();
        let job = &manifest_value["contributes"]["jobs"][0];
        assert_eq!(job["kind"], "codex-to-claude");
        assert_eq!(job["deadline_ms"], 60000);
        assert_eq!(job["memory_mib"], 256);
    }

    /// Manual: formats collects at most 8 entries; after-convert is an observe-only hook with a
    /// 20 ms deadline and at most 8 handlers.
    /// REQ: COR-050
    #[test]
    fn extension_points_match_the_manual_limits() {
        let manifest_value = manifest();
        let points = &manifest_value["contributes"]["extension_points"];
        let formats = &points[0];
        assert_eq!(formats["id"], "formats");
        assert_eq!(formats["kind"], "collect");
        assert_eq!(formats["max_entries"], 8);
        assert_eq!(formats["dynamic"], false);
        assert_eq!(
            string_list(&formats["entry_schema"]["required"]),
            vec!["id", "title", "direction", "service"]
        );
        let hook = &points[1];
        assert_eq!(hook["id"], "after-convert");
        assert_eq!(hook["kind"], "hook");
        assert_eq!(hook["mode"], "observe");
        assert_eq!(hook["risk"], "standard");
        assert_eq!(hook["deadline_ms"], 20);
        assert_eq!(hook["max_handlers"], 8);
        assert_eq!(
            hook["payload_schema"]["properties"]["messages"]["type"],
            "integer"
        );
        assert_eq!(
            hook["payload_schema"]["properties"]["path"]["type"],
            "string"
        );
    }

    /// Manual: the Convert session command appears as the pane context menu entry and opens the
    /// modal Convert session progress panel, which the activation list refers to.
    /// REQ: F-077
    #[test]
    fn command_menu_entry_and_progress_panel_match_the_manual() {
        let manifest_value = manifest();
        let contributes = &manifest_value["contributes"];
        assert_eq!(contributes["commands"][0]["id"], "convert");
        assert_eq!(contributes["commands"][0]["title"], "Convert session");
        assert_eq!(contributes["commands"][0]["category"], "Agents");
        let menu_entry = &contributes["context_menu"][0];
        assert_eq!(menu_entry["id"], "pane-convert");
        assert_eq!(menu_entry["target"], "tree-node");
        assert_eq!(
            menu_entry["when"],
            "node.kind == \"pane\" && pane.primitive == \"agent\""
        );
        assert_eq!(menu_entry["dynamic"], true);
        assert_eq!(menu_entry["command"], "convert");
        let panel = &contributes["panels"][0];
        assert_eq!(panel["id"], "progress");
        assert_eq!(panel["title"], "Convert session");
        assert_eq!(panel["modal"], true);
        assert!(string_list(&manifest_value["activation"])
            .contains(&"view:ilium.core.session-convert/progress".to_string()));
    }
}
