//! Baseline scenarios of src/docs/automation.md: F-058 `POST /create_agent` on the local HTTP API.
//! The listener is a kernel service; the fleet plugin registers the route (traceability row).

use crate::common::*;

use serde_json::json;

/// REQ: F-058, COR-001
#[test]
fn f_058_default_api_port_is_8872() {
    on_plugin_build(|kernel| {
        assert_eq!(kernel.setting("api", "port").as_deref(), Some("8872"));
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_a_port_of_zero_is_rejected() {
    on_plugin_build(|kernel| {
        let error = kernel
            .apply_setting("api", "port", "0")
            .expect_err("a port of 0 is rejected");
        assert!(error.contains("invalid-argument"), "{error}");
        assert_eq!(kernel.setting("api", "port").as_deref(), Some("8872"));
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_create_agent_success_answers_pane_id_project_path_and_prompt_delivered() {
    on_plugin_build(|kernel| {
        let (status, body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "claude", "project": kernel.project_root(), "prompt": "Fix the failing login test"}),
        );
        assert_eq!(status, 200);
        assert!(body["pane_id"].is_u64(), "{body}");
        assert_eq!(body["project_path"], json!(kernel.project_root()));
        assert_eq!(body["prompt_delivered"], json!(true));
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_an_empty_prompt_is_a_400_with_an_error_body() {
    on_plugin_build(|kernel| {
        let (status, body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "codex", "project": kernel.project_root(), "prompt": "   "}),
        );
        assert_eq!(status, 400);
        let message = body["error"]
            .as_str()
            .expect("error body is {\"error\": \"...\"}");
        assert!(!message.is_empty(), "{body}");
        assert_eq!(
            body.as_object().map(|object| object.len()),
            Some(1),
            "{body}"
        );
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_an_unresolved_project_is_a_400() {
    on_plugin_build(|kernel| {
        let (status, body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "claude", "project": "no-such-project-anywhere", "prompt": "hi"}),
        );
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_an_agent_type_other_than_claude_or_codex_is_rejected() {
    on_plugin_build(|kernel| {
        let (status, body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "emacs", "project": kernel.project_root(), "prompt": "hi"}),
        );
        // automation.md lists only 400/500/503; a bad request field is a client error, so 400.
        assert_eq!(status, 400, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "{body}"
        );
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_a_workspace_object_with_a_path_key_is_rejected_with_400() {
    on_plugin_build(|kernel| {
        let (status, body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "claude", "project": kernel.project_root(), "prompt": "hi",
                   "workspace": {"branch": "agent/inspect", "path": "/tmp/x"}}),
        );
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_the_listener_binds_loopback_only() {
    on_plugin_build(|kernel| {
        assert_eq!(
            kernel
                .http_listener_address()
                .map(|address| address.ip().to_string()),
            Some("127.0.0.1".to_owned())
        );
    });
}

/// REQ: F-058, COR-001
#[test]
fn f_058_a_get_on_the_route_is_not_served() {
    on_plugin_build(|kernel| {
        let (status, _) = kernel.http_get("/create_agent");
        assert_eq!(status, 405);
    });
}
