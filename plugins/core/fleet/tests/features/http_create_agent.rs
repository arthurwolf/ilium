//! `POST /create_agent` (F-058, COR-010): validation, the `create-pane` submission and the reply
//! once the new node is known (src/docs/automation.md "POST /create_agent" and the page sample).
//! The kernel owns the listener; the plugin owns the route only.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Intent, NodeId, Outcome};
use serde_json::{json, Value};

use crate::common::*;

fn post(host: &mut FakeHost, call: u64, body: &Value) {
    host.deliver_one(http_call(
        call,
        "POST",
        "/create_agent",
        body.to_string().as_bytes(),
    ));
    host.settle();
}

fn good_body() -> Value {
    json!({"agent_type": "claude", "project": "/home/me/dev/app", "prompt": "Fix the failing login test"})
}

fn reply_for(host: &FakeHost, call: u64) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let found: Vec<_> = host
        .http_replies()
        .into_iter()
        .filter(|(id, ..)| *id == call)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one reply for call {call}: {found:?}"
    );
    let (_, status, headers, body) = found.into_iter().next().unwrap();
    (status, headers, body)
}

fn error_body(body: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(body).expect("error body is JSON");
    assert!(value["error"].is_string(), "{value}");
    value
}

fn finish_creation(host: &mut FakeHost, outcome: Outcome) {
    for request in host.pending_requests() {
        host.complete(request, outcome.clone());
    }
    host.settle();
}

fn creates(host: &FakeHost) -> Vec<ilium_plugin_sdk::types::CreatePaneSpec> {
    created_panes(host)
}

/// REQ: F-058
#[test]
fn http_058_a_valid_request_submits_one_create_pane_for_a_terminal() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    let specs = creates(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].content_kind, "ilium.terminal");
}

/// REQ: F-058
#[test]
fn http_058_the_agent_type_selects_the_agent_command() {
    for (agent, command) in [("claude", "claude"), ("codex", "codex")] {
        let mut host = server_host();
        post(
            &mut host,
            1,
            &json!({"agent_type": agent, "project": "/home/me/dev/app", "prompt": "go"}),
        );
        assert_eq!(
            creates(&host)[0].command,
            Some(vec![command.to_string()]),
            "{agent}"
        );
    }
}

/// REQ: F-058
#[test]
fn http_058_the_prompt_becomes_the_initial_input() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    assert_eq!(
        creates(&host)[0].initial_input.as_deref(),
        Some("Fix the failing login test")
    );
}

/// REQ: F-058
#[test]
fn http_058_an_absolute_project_path_is_the_working_directory() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    // `cwd` is a granted scope plus a relative path; the absolute project path is the project scope root.
    assert_eq!(
        creates(&host)[0]
            .cwd
            .as_ref()
            .map(|path| (path.scope.as_str(), path.path.as_str())),
        Some(("project", ""))
    );
}

/// REQ: F-058
#[test]
fn http_058_the_submission_is_keyed_by_the_call_number() {
    let mut host = server_host();
    post(&mut host, 7, &good_body());
    let keys: Vec<Option<String>> = host.submitted().into_iter().map(|s| s.dedup_key).collect();
    assert_eq!(keys, [Some("http-7".to_string())]);
}

/// REQ: F-058
#[test]
fn http_058_a_second_call_with_the_same_number_creates_no_second_pane() {
    let mut host = server_host();
    post(&mut host, 7, &good_body());
    post(&mut host, 7, &good_body());
    assert_eq!(creates(&host).len(), 1);
}

/// REQ: F-058
#[test]
fn http_058_no_reply_is_sent_before_the_pane_exists() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    assert!(host.http_replies().is_empty());
}

/// REQ: F-058
#[test]
fn http_058_the_reply_carries_the_new_pane_id_the_project_path_and_prompt_delivered() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    finish_creation(&mut host, Outcome::OkNode(NodeId(42)));
    let (status, _, body) = reply_for(&host, 1);
    assert!(
        status == 200 || status == 201,
        "success status, got {status}"
    );
    let value: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(value["pane_id"], 42);
    assert_eq!(value["project_path"], "/home/me/dev/app");
    assert_eq!(value["prompt_delivered"], true);
}

/// REQ: F-058
#[test]
fn http_058_the_success_reply_is_json() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    finish_creation(&mut host, Outcome::OkNode(NodeId(42)));
    let (_, headers, _) = reply_for(&host, 1);
    assert!(
        headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("content-type")
                && value == "application/json"),
        "{headers:?}"
    );
}

/// REQ: F-058
#[test]
fn http_058_two_requests_get_their_own_replies() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    post(
        &mut host,
        2,
        &json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "other"}),
    );
    let requests = host.pending_requests();
    assert_eq!(requests.len(), 2);
    host.complete(requests[0].clone(), Outcome::OkNode(NodeId(10)));
    host.complete(requests[1].clone(), Outcome::OkNode(NodeId(11)));
    host.settle();
    let first: Value = serde_json::from_slice(&reply_for(&host, 1).2).unwrap();
    let second: Value = serde_json::from_slice(&reply_for(&host, 2).2).unwrap();
    assert_eq!(first["pane_id"], 10);
    assert_eq!(second["pane_id"], 11);
}

/// REQ: F-058
#[test]
fn http_058_a_failed_pane_creation_replies_500_with_an_error_body() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    finish_creation(
        &mut host,
        Outcome::Failed(ilium_plugin_sdk::types::PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::Io,
            message: "spawn failed".to_string(),
            retry_after_ms: None,
        }),
    );
    let (status, _, body) = reply_for(&host, 1);
    assert_eq!(status, 500);
    error_body(&body);
}

/// REQ: F-058
#[test]
fn http_058_a_refused_pane_creation_does_not_reply_success() {
    let mut host = server_host();
    post(&mut host, 1, &good_body());
    finish_creation(
        &mut host,
        Outcome::Refused(ilium_plugin_sdk::types::PluginError {
            code: ilium_plugin_sdk::types::ErrorCode::NotPermitted,
            message: "refused".to_string(),
            retry_after_ms: None,
        }),
    );
    let (status, _, _) = reply_for(&host, 1);
    assert!(status >= 400, "{status}");
}

/// REQ: F-058
#[test]
fn http_058_another_path_is_404_and_creates_nothing() {
    let mut host = server_host();
    host.deliver_one(http_call(3, "POST", "/other", b"{}"));
    host.settle();
    assert_eq!(reply_for(&host, 3).0, 404);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_another_method_on_the_route_is_404() {
    let mut host = server_host();
    host.deliver_one(http_call(3, "GET", "/create_agent", b""));
    host.settle();
    assert_eq!(reply_for(&host, 3).0, 404);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_a_query_string_does_not_change_the_route() {
    let mut host = server_host();
    host.deliver_one(http_call(
        4,
        "POST",
        "/create_agent?x=1",
        good_body().to_string().as_bytes(),
    ));
    host.settle();
    assert_eq!(creates(&host).len(), 1);
}

/// REQ: F-058
#[test]
fn http_058_a_path_with_a_trailing_segment_is_404() {
    let mut host = server_host();
    host.deliver_one(http_call(
        5,
        "POST",
        "/create_agent/extra",
        good_body().to_string().as_bytes(),
    ));
    host.settle();
    assert_eq!(reply_for(&host, 5).0, 404);
}

/// REQ: F-058
#[test]
fn http_058_a_body_that_is_not_json_is_400_invalid_json() {
    let mut host = server_host();
    host.deliver_one(http_call(6, "POST", "/create_agent", b"not json"));
    host.settle();
    let (status, _, body) = reply_for(&host, 6);
    assert_eq!(status, 400);
    assert_eq!(
        String::from_utf8(body).unwrap(),
        r#"{"error":"invalid json"}"#
    );
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_an_empty_body_is_400() {
    let mut host = server_host();
    host.deliver_one(http_call(6, "POST", "/create_agent", b""));
    host.settle();
    assert_eq!(reply_for(&host, 6).0, 400);
}

/// REQ: F-058
#[test]
fn http_058_an_empty_prompt_is_400_and_creates_nothing() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "claude", "project": "/home/me/dev/app", "prompt": ""}),
    );
    let (status, _, body) = reply_for(&host, 1);
    assert_eq!(status, 400);
    error_body(&body);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_a_whitespace_only_prompt_is_400() {
    for prompt in [" ", "\n", "\t \n"] {
        let mut host = server_host();
        post(
            &mut host,
            1,
            &json!({"agent_type": "claude", "project": "/home/me/dev/app", "prompt": prompt}),
        );
        assert_eq!(reply_for(&host, 1).0, 400, "{prompt:?}");
        assert!(creates(&host).is_empty());
    }
}

/// REQ: F-058
#[test]
fn http_058_a_one_character_prompt_is_accepted() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "claude", "project": "/home/me/dev/app", "prompt": "x"}),
    );
    assert_eq!(creates(&host).len(), 1);
}

/// REQ: F-058
#[test]
fn http_058_a_bare_project_name_that_matches_nothing_is_400() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "claude", "project": "no-such-project-anywhere", "prompt": "go"}),
    );
    let (status, _, body) = reply_for(&host, 1);
    assert_eq!(status, 400);
    error_body(&body);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_a_missing_field_is_400() {
    for body in [
        json!({"project": "/home/me/dev/app", "prompt": "go"}),
        json!({"agent_type": "claude", "prompt": "go"}),
        json!({"agent_type": "claude", "project": "/home/me/dev/app"}),
    ] {
        let mut host = server_host();
        post(&mut host, 1, &body);
        assert_eq!(reply_for(&host, 1).0, 400, "{body}");
        assert!(creates(&host).is_empty());
    }
}

/// REQ: F-058
#[test]
fn http_058_an_unknown_agent_type_is_400() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "emacs", "project": "/home/me/dev/app", "prompt": "go"}),
    );
    assert_eq!(reply_for(&host, 1).0, 400);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_a_workspace_with_a_branch_asks_the_worktrees_service() {
    let mut host = server_host();
    host.stub_service(
        "ilium.core.worktrees/create",
        "create",
        r#"{"path":"/home/me/dev/app.worktrees/agent-inspect"}"#,
    );
    post(
        &mut host,
        1,
        &json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "go", "workspace": {"branch": "agent/inspect", "base": "main"}}),
    );
    let calls = host.service_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ilium.core.worktrees/create");
    let request: Value = serde_json::from_str(&calls[0].2).expect("JSON");
    assert_eq!(request["branch"], "agent/inspect");
    assert_eq!(request["base"], "main");
}

/// REQ: F-058
#[test]
fn http_058_a_workspace_without_base_is_accepted() {
    let mut host = server_host();
    host.stub_service(
        "ilium.core.worktrees/create",
        "create",
        r#"{"path":"/home/me/dev/app.worktrees/agent-x"}"#,
    );
    post(
        &mut host,
        1,
        &json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "go", "workspace": {"branch": "agent/x"}}),
    );
    assert!(host
        .http_replies()
        .iter()
        .all(|(_, status, ..)| *status < 400));
}

/// REQ: F-058
#[test]
fn http_058_a_workspace_with_a_path_key_is_rejected_with_400() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "go", "workspace": {"branch": "agent/x", "path": "/tmp/x"}}),
    );
    let (status, _, body) = reply_for(&host, 1);
    assert_eq!(status, 400);
    error_body(&body);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058
#[test]
fn http_058_a_workspace_without_a_branch_is_400() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "go", "workspace": {"base": "main"}}),
    );
    assert_eq!(reply_for(&host, 1).0, 400);
}

/// REQ: F-058
#[test]
fn http_058_an_invalid_branch_name_in_the_workspace_is_400() {
    let mut host = server_host();
    post(
        &mut host,
        1,
        &json!({"agent_type": "codex", "project": "/home/me/dev/app", "prompt": "go", "workspace": {"branch": "has space"}}),
    );
    assert_eq!(reply_for(&host, 1).0, 400);
    assert!(creates(&host).is_empty());
}

/// REQ: F-058, COR-010
#[test]
fn http_010_the_plugin_never_binds_a_listener_it_only_answers_calls() {
    // The plugin has no http listener grant: the only http-related server grant is the route.
    let manifest = std::fs::read_to_string(manifest_path()).expect("manifest");
    assert!(manifest.contains("\"http-api.route:/create_agent\""));
    assert!(!manifest.contains("http-api.listen"));
}

/// REQ: F-058
#[test]
fn http_058_an_error_reply_never_creates_a_pane_intent() {
    let mut host = server_host();
    host.deliver_one(http_call(9, "POST", "/create_agent", b"[1,2,3]"));
    host.settle();
    assert!(host
        .submitted()
        .iter()
        .all(|s| !matches!(s.intent, Intent::CreatePane(_))));
    assert_eq!(reply_for(&host, 9).0, 400);
}
