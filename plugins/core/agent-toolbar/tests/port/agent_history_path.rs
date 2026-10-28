//! Twins of ilium-client/src/agent_history_path.rs: resolving the verified JSONL history path of an agent session.
//! Files are given relative to a virtual home directory; the reply carries the resolved path relative to it.

use serde_json::{json, Value};

use crate::common::{ask, Place};

const PROJECT: &str = "/work/history-path-test";

fn codex_file(project: &str, session_id: &str) -> (String, Value) {
    let path = format!(".codex/sessions/2026/09/05/rollout-2026-09-05T12-00-00-{session_id}.jsonl");
    let content =
        json!({ "type": "session_meta", "payload": { "id": session_id, "cwd": project } })
            .to_string();
    (path.clone(), json!({ "path": path, "content": content }))
}

fn claude_file(project: &str, session_id: &str) -> (String, Value) {
    let slug: String = project
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = format!(".claude/projects/{slug}/{session_id}.jsonl");
    let content = json!({
        "type": "user", "sessionId": session_id, "cwd": project, "message": { "content": "test prompt" }
    })
    .to_string();
    (path.clone(), json!({ "path": path, "content": content }))
}

fn resolve(
    files: Vec<Value>,
    project: &str,
    agent: &str,
    session_id: &str,
    hint: Option<&str>,
    scan_budget: Option<u64>,
) -> Value {
    ask(
        Place::Client,
        "history-path.resolve",
        json!({
            "files": files, "project": project, "agent": agent, "session_id": session_id,
            "hint": hint, "scan_budget": scan_budget
        }),
    )["path"]
        .clone()
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::hint_naming_another_session_is_ignored
#[test]
fn port_agent_history_path_hint_naming_another_session_is_ignored() {
    let requested_id = "77777777-7777-4777-8777-777777777777";
    let other_id = "88888888-8888-4888-8888-888888888888";
    let (requested_path, requested) = codex_file(PROJECT, requested_id);
    let (other_path, other) = codex_file(PROJECT, other_id);
    let path = resolve(
        vec![requested, other],
        PROJECT,
        "codex",
        requested_id,
        Some(&other_path),
        None,
    );
    assert_eq!(path, json!(requested_path));
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::hint_outside_the_expected_store_is_rejected
#[test]
fn port_agent_history_path_hint_outside_the_expected_store_is_rejected() {
    let session_id = "99999999-9999-4999-8999-999999999999";
    let foreign = format!("/foreign/rollout-2026-09-05T12-00-00-{session_id}.jsonl");
    let content =
        json!({ "type": "session_meta", "payload": { "id": session_id, "cwd": PROJECT } })
            .to_string();
    let files = vec![json!({ "path": foreign, "content": content, "outside_home": true })];
    assert_eq!(
        resolve(files, PROJECT, "codex", session_id, Some(&foreign), Some(1)),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::refuses_providers_without_a_jsonl_history_contract
#[test]
fn port_agent_history_path_refuses_providers_without_a_jsonl_history_contract() {
    let session_id = "33333333-3333-4333-8333-333333333333";
    assert_eq!(
        resolve(vec![], PROJECT, "antigravity", session_id, None, None),
        Value::Null
    );
    assert_eq!(
        resolve(vec![], PROJECT, "other:aider", session_id, None, None),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::resolves_codex_history_in_a_store_larger_than_the_old_menu_budget
#[test]
fn port_agent_history_path_resolves_codex_history_in_a_store_larger_than_the_old_menu_budget() {
    let session_id = "55555555-5555-4555-8555-555555555555";
    let (target, target_file) = codex_file(PROJECT, session_id);
    let mut files = vec![target_file];
    for month in 1..=12 {
        for day in 1..=28 {
            for index in 0..14 {
                files.push(json!({
                    "path": format!(".codex/sessions/2025/{month:02}/{day:02}/rollout-{index}.jsonl"),
                    "content": "{}"
                }));
            }
        }
    }
    assert_eq!(
        resolve(files, PROJECT, "codex", session_id, None, None),
        json!(target)
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::resolves_verified_absolute_paths_for_claude_and_codex_sessions
#[test]
fn port_agent_history_path_resolves_verified_absolute_paths_for_claude_and_codex_sessions() {
    let claude_id = "11111111-1111-4111-8111-111111111111";
    let codex_id = "22222222-2222-4222-8222-222222222222";
    let (claude_path, claude) = claude_file(PROJECT, claude_id);
    let (codex_path, codex) = codex_file(PROJECT, codex_id);
    let files = vec![claude, codex];
    assert_eq!(
        resolve(files.clone(), PROJECT, "claude", claude_id, None, None),
        json!(claude_path)
    );
    assert_eq!(
        resolve(files, PROJECT, "codex", codex_id, None, None),
        json!(codex_path)
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::server_verified_hint_resolves_when_the_directory_scan_budget_is_exhausted
#[test]
fn port_agent_history_path_server_verified_hint_resolves_when_the_directory_scan_budget_is_exhausted(
) {
    let session_id = "66666666-6666-4666-8666-666666666666";
    let (target, file) = codex_file(PROJECT, session_id);
    let scan_only = resolve(
        vec![file.clone()],
        PROJECT,
        "codex",
        session_id,
        None,
        Some(1),
    );
    assert_eq!(scan_only, Value::Null);
    let hinted = resolve(
        vec![file],
        PROJECT,
        "codex",
        session_id,
        Some(&target),
        Some(1),
    );
    assert_eq!(hinted, json!(target));
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::worktree_history_requires_the_panes_launch_cwd
#[test]
fn port_agent_history_path_worktree_history_requires_the_panes_launch_cwd() {
    let worktree = "/work/history-path-test.worktrees/agent-task";
    let session_id = "44444444-4444-4444-8444-444444444444";
    let (path, file) = codex_file(worktree, session_id);
    assert_eq!(
        resolve(
            vec![file.clone()],
            worktree,
            "codex",
            session_id,
            None,
            None
        ),
        json!(path)
    );
    assert_eq!(
        resolve(vec![file], PROJECT, "codex", session_id, None, None),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::hint_naming_another_session_is_ignored
#[test]
fn port_agent_history_path_hint_for_another_session_alone_resolves_to_nothing() {
    let requested_id = "77777777-7777-4777-8777-777777777777";
    let other_id = "88888888-8888-4888-8888-888888888888";
    let (other_path, other) = codex_file(PROJECT, other_id);
    assert_eq!(
        resolve(
            vec![other],
            PROJECT,
            "codex",
            requested_id,
            Some(&other_path),
            None
        ),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::hint_outside_the_expected_store_is_rejected
#[test]
fn port_agent_history_path_hint_naming_a_missing_file_resolves_to_nothing() {
    let session_id = "12121212-1212-4212-8212-121212121212";
    let missing = ".codex/sessions/2026/09/05/rollout-2026-09-05T12-00-00-missing.jsonl";
    assert_eq!(
        resolve(vec![], PROJECT, "codex", session_id, Some(missing), None),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::refuses_providers_without_a_jsonl_history_contract
#[test]
fn port_agent_history_path_refuses_a_claude_shaped_file_for_a_provider_without_contract() {
    let session_id = "13131313-1313-4313-8313-131313131313";
    let (_, file) = claude_file(PROJECT, session_id);
    assert_eq!(
        resolve(vec![file], PROJECT, "antigravity", session_id, None, None),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::resolves_codex_history_in_a_store_larger_than_the_old_menu_budget
#[test]
fn port_agent_history_path_large_store_without_the_target_resolves_to_nothing() {
    let session_id = "14141414-1414-4414-8414-141414141414";
    let mut files = Vec::new();
    for index in 0..50 {
        files.push(json!({
            "path": format!(".codex/sessions/2025/01/01/rollout-{index}.jsonl"),
            "content": "{}"
        }));
    }
    assert_eq!(
        resolve(files, PROJECT, "codex", session_id, None, None),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::resolves_verified_absolute_paths_for_claude_and_codex_sessions
#[test]
fn port_agent_history_path_claude_file_does_not_resolve_for_an_unknown_session_id() {
    let claude_id = "15151515-1515-4515-8515-151515151515";
    let other_id = "16161616-1616-4616-8616-161616161616";
    let (_, claude) = claude_file(PROJECT, claude_id);
    assert_eq!(
        resolve(vec![claude], PROJECT, "claude", other_id, None, None),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::server_verified_hint_resolves_when_the_directory_scan_budget_is_exhausted
#[test]
fn port_agent_history_path_hint_without_a_matching_file_stays_unresolved_under_budget() {
    let session_id = "17171717-1717-4717-8717-171717171717";
    let (target, _) = codex_file(PROJECT, session_id);
    assert_eq!(
        resolve(vec![], PROJECT, "codex", session_id, Some(&target), Some(1)),
        Value::Null
    );
}

/// REQ: PORT:ilium-client/src/agent_history_path.rs::worktree_history_requires_the_panes_launch_cwd
#[test]
fn port_agent_history_path_claude_worktree_history_resolves_from_the_worktree_cwd() {
    let worktree = "/work/history-path-test.worktrees/agent-task";
    let session_id = "18181818-1818-4818-8818-181818181818";
    let (path, file) = claude_file(worktree, session_id);
    assert_eq!(
        resolve(vec![file], worktree, "claude", session_id, None, None),
        json!(path)
    );
}
