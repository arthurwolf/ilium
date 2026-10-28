//! Core plugin `ilium.core.agent-setup` (manual page core-plugin-agent-setup.md).
//!
//! The server side serves `ilium.core.agent-setup/install` (install, remove, status): it validates
//! the parameters here and edits agent configuration text through `config_writer`. Reading and
//! writing the agent files needs SDK host file calls that are not implemented yet, so a valid
//! request is answered with an `unsupported` error reply for now. The client side holds no state
//! and draws an empty Setup tab.

pub mod config_writer;

use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::types::{
    Activation, Alignment, Area, Event, NodeFlags, ParagraphSpec, PluginError, Span, Style,
    Submission, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, ServiceProvider, Views};

/// The server side of `ilium.core.agent-setup` (core-plugin-agent-setup.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct AgentSetupServer;

impl Lifecycle for AgentSetupServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AgentSetupServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for AgentSetupServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        ServiceReply::Done(handle_call(&call.method, &call.payload_json))
    }
}

/// The client side of `ilium.core.agent-setup` (core-plugin-agent-setup.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct AgentSetupClient;

impl Lifecycle for AgentSetupClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AgentSetupClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl Views for AgentSetupClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        UiTree {
            version: 1,
            nodes: vec![UiNode {
                key: None,
                widget: Widget::Paragraph(ParagraphSpec {
                    lines: vec![vec![Span {
                        text: "Agent setup".to_string(),
                        style: Style::default(),
                    }]],
                    rich: None,
                    wrap: Some(false),
                    alignment: Alignment::Left,
                    style: Style::default(),
                    scrollable: false,
                }),
                children: Vec::new(),
                flags: NodeFlags::default(),
                tooltip: None,
                tab_index: None,
                disabled_reason: None,
            }],
            root: 0,
        }
    }
}

/// Target ids of the service (core-plugin-agent-setup.md).
const TARGETS: [&str; 4] = [
    "claude-global",
    "codex-global",
    "claude-project",
    "codex-project",
];

/// Extracts the string value of `key` from a flat JSON object; `None` when absent or not a string.
fn json_string_member(payload: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after_key = payload.split_once(&needle)?.1;
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let mut characters = after_colon.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(character) = characters.next() {
        match character {
            '"' => return Some(out),
            '\\' => out.push(characters.next()?),
            other => out.push(other),
        }
    }
    None
}

fn error_reply(code: &str, message: &str) -> String {
    format!("{{\"error\":{{\"code\":\"{code}\",\"message\":\"{message}\"}}}}")
}

/// Validates the parameters of one service method; the file work follows once host file calls exist.
fn handle_call(method: &str, payload: &str) -> String {
    let payload = payload.trim();
    if !payload.starts_with('{') || !payload.ends_with('}') {
        return error_reply("invalid-argument", "the payload is not a JSON object");
    }
    match method {
        "install" | "remove" => {
            let block = json_string_member(payload, "block");
            let target = json_string_member(payload, "target");
            let (Some(_block), Some(target)) = (block, target) else {
                return error_reply("invalid-argument", "block and target are required strings");
            };
            if !TARGETS.contains(&target.as_str()) {
                return error_reply("invalid-argument", "unknown target");
            }
            error_reply("unsupported", "agent files cannot be reached from the plugin yet")
        }
        "status" => error_reply("unsupported", "agent files cannot be reached from the plugin yet"),
        _ => error_reply("not-found", "unknown method"),
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The shipped manifest, read at compile time so the contract checks need no host.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json parses as JSON")
    }

    fn string_items(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("expected a JSON array")
            .iter()
            .map(|item| item.as_str().expect("expected a JSON string").to_string())
            .collect()
    }

    fn side_capabilities(manifest: &serde_json::Value, side: &str) -> Vec<String> {
        string_items(&manifest["capabilities"][side])
    }

    /// REQ: COR-050
    #[test]
    fn manifest_carries_every_required_key() {
        let manifest = manifest();
        for key in [
            "api",
            "id",
            "name",
            "version",
            "publisher",
            "server",
            "client",
            "capabilities",
            "activation",
            "limits_tier",
        ] {
            assert!(manifest.get(key).is_some(), "manifest is missing `{key}`");
        }
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.agent-setup");
        assert_eq!(manifest["limits_tier"], "first-party");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_server_and_client_sides() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            string_items(&manifest["server"]["exports"]),
            vec!["service-provider"]
        );
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(string_items(&manifest["client"]["exports"]), vec!["views"]);
        let _server = AgentSetupServer;
        let _client = AgentSetupClient;
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activation_lists_the_three_triggers() {
        assert_eq!(
            string_items(&manifest()["activation"]),
            vec!["settings:setup", "command:refresh", "service:install"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_surface_names_exactly_the_replaceable_contributions() {
        let manifest = manifest();
        assert_eq!(manifest["replaceable"], true);
        assert_eq!(
            string_items(&manifest["surface"]),
            vec![
                "services:install",
                "points:instruction-blocks",
                "points:targets",
                "points:before-install",
                "commands:refresh",
            ]
        );
    }

    /// REQ: COR-011
    #[test]
    fn client_side_holds_no_file_grants() {
        let manifest = manifest();
        let client = side_capabilities(&manifest, "client");
        assert!(
            client.iter().all(|grant| !grant.starts_with("fs.")),
            "client grants: {client:?}"
        );
    }

    /// REQ: COR-050
    #[test]
    fn no_direct_agent_config_write_grant_is_declared() {
        let manifest = manifest();
        let server = side_capabilities(&manifest, "server");
        let direct_writes: Vec<&String> = server
            .iter()
            .filter(|grant| grant.starts_with("fs.write:agent-config:"))
            .collect();
        assert!(
            direct_writes.is_empty(),
            "direct agent-config writes: {direct_writes:?}"
        );
        assert!(server
            .iter()
            .any(|grant| grant == "agent-config.propose:claude"));
        assert!(server
            .iter()
            .any(|grant| grant == "agent-config.propose:codex"));
    }

    /// REQ: COR-033
    #[test]
    fn exactly_one_grant_writes_the_setup_tab() {
        let manifest = manifest();
        let all_grants: Vec<String> = side_capabilities(&manifest, "server")
            .into_iter()
            .chain(side_capabilities(&manifest, "client"))
            .collect();
        let kernel_writes: Vec<String> = all_grants
            .into_iter()
            .filter(|grant| grant.starts_with("settings.write-kernel:"))
            .collect();
        assert_eq!(kernel_writes, vec!["settings.write-kernel:setup"]);
    }

    /// REQ: SEC-293
    #[test]
    fn before_install_hook_is_a_high_risk_veto() {
        let manifest = manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array");
        let before_install = points
            .iter()
            .find(|point| point["id"] == "before-install")
            .expect("before-install point is declared");
        assert_eq!(before_install["kind"], "hook");
        assert_eq!(before_install["mode"], "veto");
        assert_eq!(before_install["risk"], "high");
        assert_eq!(before_install["deadline_ms"], 20);
        assert_eq!(before_install["max_handlers"], 4);
    }

    /// REQ: COR-050
    #[test]
    fn collect_points_keep_their_documented_caps() {
        let manifest = manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array");
        let cap_of = |point_id: &str| -> u64 {
            points
                .iter()
                .find(|point| point["id"] == point_id)
                .and_then(|point| point["max_entries"].as_u64())
                .expect("collect point declares a positive entry cap")
        };
        assert_eq!(cap_of("instruction-blocks"), 8);
        assert_eq!(cap_of("targets"), 16);
        assert!(cap_of("targets") > 0);
    }

    /// REQ: COR-050
    #[test]
    fn install_and_remove_params_require_block_and_target() {
        let manifest = manifest();
        let service = &manifest["contributes"]["services"]["ilium.core.agent-setup/install"];
        for operation in ["install", "remove"] {
            let required = string_items(&service[operation]["params"]["required"]);
            assert_eq!(required, vec!["block", "target"], "operation `{operation}`");
        }
    }
}
