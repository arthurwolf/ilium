//! Twins of `ilium-server/src/http_api.rs` and the workspace/smoke server tests, driven through
//! `POST /create_agent` (src/docs/automation.md). Quota, execution-bank and charge accounting of
//! the lookup are kernel internals and have no twin.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::Outcome;
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

fn reply(host: &FakeHost, call: u64) -> (u16, Vec<u8>) {
    let found: Vec<_> = host
        .http_replies()
        .into_iter()
        .filter(|(id, ..)| *id == call)
        .collect();
    assert_eq!(found.len(), 1, "exactly one reply for call {call}");
    let (_, status, _, body) = found.into_iter().next().unwrap();
    (status, body)
}

fn agent(project: &str) -> Value {
    json!({"agent_type": "claude", "project": project, "prompt": "go"})
}

/// REQ: PORT:ilium-server/src/http_api.rs::project_name_lookup_finds_nested_directories
#[test]
fn http_api_project_name_lookup_finds_nested_directories() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ai/ilium/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    // `cwd` is a granted scope plus a relative path (absolute paths are invalid); the session directory is the project scope root.
    assert_eq!(
        specs[0]
            .cwd
            .as_ref()
            .map(|path| (path.scope.as_str(), path.path.as_str())),
        Some(("project", ""))
    );
}

/// REQ: PORT:ilium-server/src/http_api.rs::project_name_lookup_stops_at_depth_two
#[test]
fn http_api_project_name_lookup_stops_at_depth_two() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ai/vendor/ilium/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    assert_eq!(reply(&host, 1).0, 400);
    assert!(created_panes(&host).is_empty());
}

/// REQ: PORT:ilium-server/src/http_api.rs::lookup_deduplicates_session_directory_and_preserves_ambiguity_and_stale_cwd
#[test]
fn http_api_lookup_deduplicates_session_directory_and_preserves_ambiguity_and_stale_cwd() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/a/ilium/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    assert_eq!(created_panes(&host).len(), 1, "a single match resolves");
    host.fs().write("/home/me/dev/b/ilium/.keep", b"");
    post(&mut host, 2, &agent("ilium"));
    let (status, body) = reply(&host, 2);
    assert_eq!(status, 400);
    assert!(String::from_utf8_lossy(&body).contains("ambiguous"));
    assert_eq!(
        created_panes(&host).len(),
        1,
        "an ambiguous name creates nothing"
    );
}

/// REQ: PORT:ilium-server/src/http_api.rs::incomplete_project_scan_refuses_instead_of_returning_a_unique_prefix
#[test]
fn http_api_incomplete_project_scan_refuses_instead_of_returning_a_unique_prefix() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ai/ilium-extra/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    assert_eq!(
        reply(&host, 1).0,
        400,
        "a prefix of another directory name is not a match"
    );
    assert!(created_panes(&host).is_empty());
}

/// REQ: PORT:ilium-server/src/http_api.rs::create_agent_request_accepts_server_chosen_worktree_path_and_legacy_shape
#[test]
fn http_api_create_agent_request_accepts_server_chosen_worktree_path_and_legacy_shape() {
    let mut host = server_host();
    post(&mut host, 1, &agent("/home/me/dev/app"));
    assert_eq!(
        created_panes(&host).len(),
        1,
        "the request without a workspace is accepted"
    );
    host.stub_service(
        "ilium.core.worktrees/create",
        "create",
        r#"{"path":"/home/me/dev/app.worktrees/agent-x"}"#,
    );
    let mut with_workspace = agent("/home/me/dev/app");
    with_workspace["workspace"] = json!({"branch": "agent/x"});
    post(&mut host, 2, &with_workspace);
    assert_eq!(
        host.service_calls().len(),
        1,
        "the server chooses the path through the worktrees service"
    );
    let mut with_path = agent("/home/me/dev/app");
    with_path["workspace"] = json!({"branch": "agent/x", "path": "/tmp/x"});
    post(&mut host, 3, &with_path);
    assert_eq!(reply(&host, 3).0, 400);
}

/// REQ: PORT:ilium-server/tests/workspace_http.rs::http_workspace_uses_server_path_and_preserves_legacy_request
#[test]
fn http_api_http_workspace_uses_server_path_and_preserves_legacy_request() {
    let mut host = server_host();
    host.stub_service(
        "ilium.core.worktrees/create",
        "create",
        r#"{"path":"/home/me/dev/app.worktrees/agent-x"}"#,
    );
    let mut body = agent("/home/me/dev/app");
    body["workspace"] = json!({"branch": "agent/x", "base": "main"});
    post(&mut host, 1, &body);
    let calls = host.service_calls();
    assert_eq!(calls.len(), 1);
    let request: Value = serde_json::from_str(&calls[0].2).expect("JSON");
    assert_eq!(
        (request["branch"].as_str(), request["base"].as_str()),
        (Some("agent/x"), Some("main"))
    );
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0]
            .cwd
            .as_ref()
            .map(|path| (path.scope.as_str(), path.path.as_str())),
        Some(("project", ""))
    );
}

/// REQ: PORT:ilium-server/tests/workspace_http.rs::disconnect_during_worktree_add_never_commits_a_pane
#[test]
fn http_api_disconnect_during_worktree_add_never_commits_a_pane() {
    let mut host = server_host();
    let mut body = agent("/home/me/dev/app");
    body["workspace"] = json!({"branch": "agent/x"});
    post(&mut host, 1, &body);
    // No worktree path was ever returned (no stub), so no pane may be created.
    assert!(created_panes(&host).is_empty());
    for request in host.pending_requests() {
        host.complete(request, Outcome::OkNode(ilium_plugin_sdk::types::NodeId(9)));
    }
    host.settle();
    assert!(created_panes(&host).is_empty());
}

/// REQ: PORT:ilium-server/src/http_api.rs::project_name_lookup_finds_nested_directories
#[test]
fn http_api_nested_project_lookup_reports_the_resolved_project_path() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ilium/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    for request in host.pending_requests() {
        host.complete(
            request,
            Outcome::OkNode(ilium_plugin_sdk::types::NodeId(42)),
        );
    }
    host.settle();
    let (status, body) = reply(&host, 1);
    let value: Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(status, 201);
    assert_eq!(value["project_path"], json!("/home/me/dev/ilium"));
}

/// REQ: PORT:ilium-server/src/http_api.rs::project_name_lookup_stops_at_depth_two
#[test]
fn http_api_project_one_level_below_the_root_still_resolves() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ilium/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    assert_eq!(created_panes(&host).len(), 1);
}

/// REQ: PORT:ilium-server/src/http_api.rs::lookup_deduplicates_session_directory_and_preserves_ambiguity_and_stale_cwd
#[test]
fn http_api_full_path_bypasses_name_ambiguity() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/a/ilium/.keep", b"");
    host.fs().write("/home/me/dev/b/ilium/.keep", b"");
    post(&mut host, 1, &agent("/home/me/dev/a/ilium"));
    assert_eq!(created_panes(&host).len(), 1);
}

/// REQ: PORT:ilium-server/src/http_api.rs::incomplete_project_scan_refuses_instead_of_returning_a_unique_prefix
#[test]
fn http_api_exact_name_wins_when_a_longer_sibling_also_exists() {
    let mut host = server_host();
    host.fs().write("/home/me/dev/ai/ilium/.keep", b"");
    host.fs().write("/home/me/dev/ai/ilium-extra/.keep", b"");
    post(&mut host, 1, &agent("ilium"));
    assert_eq!(created_panes(&host).len(), 1);
}

/// REQ: PORT:ilium-server/tests/workspace_http.rs::http_workspace_uses_server_path_and_preserves_legacy_request
#[test]
fn http_api_workspace_request_without_base_sends_no_base_to_the_service() {
    let mut host = server_host();
    host.stub_service(
        "ilium.core.worktrees/create",
        "create",
        r#"{"path":"/home/me/dev/app.worktrees/agent-x"}"#,
    );
    let mut body = agent("/home/me/dev/app");
    body["workspace"] = json!({"branch": "agent/y"});
    post(&mut host, 1, &body);
    let calls = host.service_calls();
    assert_eq!(calls.len(), 1);
    let request: Value = serde_json::from_str(&calls[0].2).expect("JSON");
    assert_eq!(request["branch"].as_str(), Some("agent/y"));
    assert!(request["base"].is_null());
}

/// REQ: PORT:ilium-server/tests/workspace_http.rs::disconnect_during_worktree_add_never_commits_a_pane
#[test]
fn http_api_failed_worktree_add_creates_no_pane() {
    use ilium_plugin_sdk::types::{ErrorCode, PluginError};
    let mut host = server_host();
    let mut body = agent("/home/me/dev/app");
    body["workspace"] = json!({"branch": "agent/x"});
    post(&mut host, 1, &body);
    for request in host.pending_requests() {
        host.complete(
            request,
            Outcome::Failed(PluginError {
                code: ErrorCode::Io,
                message: "worktree add failed".to_string(),
                retry_after_ms: None,
            }),
        );
    }
    host.settle();
    assert!(created_panes(&host).is_empty());
}
