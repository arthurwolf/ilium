//! Loopback HTTP automation boundary for a detached ilium server.
//!
//! This module intentionally owns only HTTP parsing, project-name resolution,
//! and response shaping. Actual tree mutation, PTY creation, and prompt
//! readiness remain in `ipc::handlers`, the server's existing lifecycle
//! boundary for those responsibilities.

use std::collections::{BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::execution::{ExecutionClient, ExecutionError};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use ilium_core::{BuiltinAgentProvider, ROOT_ID};
use ilium_execution::{JobCost, Lane, Retained, Retention};
use ilium_platform::paths;
use serde::{Deserialize, Serialize};

use crate::config::HttpApiConfig;
use crate::ipc::handlers::create_agent_with_prompt;
use crate::state::ServerState;
use crate::workspace::{create_agent_in_workspace, CreateAgentOptions};

/// Starts the HTTP API on `127.0.0.1`, never on a public interface. A server
/// that cannot bind its configured port keeps its terminal/IPC duties alive.
pub(crate) fn spawn(state: Arc<ServerState>, config: HttpApiConfig) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let address = SocketAddr::from(([127, 0, 0, 1], config.port));
        let listener = match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(%address, %error, "failed to bind loopback HTTP API");
                return;
            }
        };
        tracing::info!(%address, "loopback HTTP API listening");
        let router = Router::new()
            .route("/create_agent", post(create_agent))
            .with_state(state);
        if let Err(error) = axum::serve(listener, router).await {
            tracing::error!(%address, %error, "loopback HTTP API stopped");
        }
    })
}

#[derive(Debug, Deserialize)]
struct CreateAgentRequest {
    agent_type: HttpAgentType,
    project: String,
    prompt: String,
    #[serde(default)]
    workspace: Option<HttpWorkspaceSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpWorkspaceSpec {
    branch: String,
    #[serde(default)]
    base: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum HttpAgentType {
    Claude,
    Codex,
}

impl HttpAgentType {
    const fn provider(&self) -> BuiltinAgentProvider {
        match self {
            Self::Claude => BuiltinAgentProvider::Claude,
            Self::Codex => BuiltinAgentProvider::Codex,
        }
    }
}

#[derive(Debug, Serialize)]
struct CreateAgentResponse {
    pane_id: u64,
    project_path: String,
    prompt_delivered: bool,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

async fn create_agent(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<CreateAgentRequest>,
) -> Result<Response, ApiError> {
    if request.prompt.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "prompt must not be empty",
        ));
    }
    let client = &state
        .execution
        .get()
        .ok_or_else(|| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "project lookup execution is unavailable",
            )
        })?
        .client;
    let project_result = lookup_with(
        client,
        &state.session_cwd,
        &state.home_dir,
        &request.project,
        resolve_project_path,
    )
    .await?;
    // Keep original result admission while existing semantic callers use
    // derived paths; the final JSON bytes inherit it through actual disposal.
    let (project_path, lookup_retention) = project_result.into_parts();
    let lookup_retention = Arc::new(lookup_retention);
    let pane_id = if let Some(workspace) = request.workspace {
        let spec = crate::workspace::default_new_worktree_spec(
            &project_path,
            workspace.branch,
            workspace.base,
        )
        .await
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
        // The HTTP handler may be dropped when its requester disconnects.
        // Keep Git mutation in a server-owned task, and let the coordinator
        // observe that disconnect at its safe rollback boundaries. Three
        // progress events fit in this channel while the receiver is held.
        let (request_tx, _request_rx) = tokio::sync::mpsc::channel(4);
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let creation_state = Arc::clone(&state);
        let options = CreateAgentOptions {
            request_id: 0,
            parent_group: ROOT_ID,
            project_override: Some(project_path.clone()),
            provider: request.agent_type.provider(),
            spec,
            initial_input: Some(request.prompt),
            wait_for_prompt: true,
        };
        let creation_lookup_retention = Arc::clone(&lookup_retention);
        let handle = tokio::spawn(async move {
            let _lookup_retention = creation_lookup_retention;
            if start_rx.await.is_err() {
                return;
            }
            let reply = crate::ipc::EventReply::Legacy(&request_tx);
            let result = create_agent_in_workspace(&creation_state, options, Some(&reply)).await;
            let _ = result_tx.send(result);
        });
        if !state.track_workspace_creation_task(handle) {
            return Err(api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "session is shutting down",
            ));
        }
        let _ = start_tx.send(());
        result_rx.await.map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("worktree creation task ended without a result: {error}"),
            )
        })?
    } else {
        create_agent_with_prompt(
            &state,
            request.agent_type.provider(),
            project_path.clone(),
            request.prompt,
        )
        .await
    }
    .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    let response = CreateAgentResponse {
        pane_id: pane_id.0,
        project_path: project_path.display().to_string(),
        prompt_delivered: true,
    };
    Ok(json_response(
        StatusCode::OK,
        &response,
        Some(lookup_retention),
    ))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
    retention: Option<Arc<Retention>>,
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        json_response(
            self.status,
            &ErrorResponse {
                error: self.message,
            },
            self.retention,
        )
    }
}
fn api_error(status: StatusCode, message: impl Into<String>) -> ApiError {
    ApiError {
        status,
        message: message.into(),
        retention: None,
    }
}

struct ChargedHttpBytes {
    bytes: Vec<u8>,
    // Bytes::from_owner keeps this lease through every byte-slice clone,
    // including after the HTTP body has handed a chunk to its consumer.
    _retention: Option<Arc<Retention>>,
}
impl AsRef<[u8]> for ChargedHttpBytes {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
fn json_response(
    status: StatusCode,
    value: &impl Serialize,
    retention: Option<Arc<Retention>>,
) -> Response {
    let (status, bytes) = match serde_json::to_vec(value) {
        Ok(bytes) => (status, bytes),
        Err(error) => {
            tracing::error!(%error, "HTTP response encoding failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                b"{\"error\":\"HTTP response encoding failed\"}".to_vec(),
            )
        }
    };
    let bytes = axum::body::Bytes::from_owner(ChargedHttpBytes {
        bytes,
        _retention: retention,
    });
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bytes,
    )
        .into_response()
}

const MAX_LOOKUP_PATH_BYTES: usize = 8192;
const MAX_LOOKUP_ENTRIES: usize = 16_384;
const MAX_PENDING_DIRECTORIES: usize = 512;
const LOOKUP_RESULT_BYTES: usize = 512 * 1024;

struct LookupInput {
    session_cwd: PathBuf,
    home_dir: PathBuf,
    project: String,
}
#[derive(Debug)]
enum LookupIssue {
    Invalid(String),
    Resource(&'static str),
}
impl LookupIssue {
    fn into_error(self, retention: Retention) -> ApiError {
        let (status, message) = match self {
            Self::Invalid(message) => (StatusCode::BAD_REQUEST, message),
            Self::Resource(message) => (StatusCode::SERVICE_UNAVAILABLE, message.to_owned()),
        };
        ApiError {
            status,
            message,
            retention: Some(Arc::new(retention)),
        }
    }
}
fn lookup_cost(cwd: &Path, home: &Path, project: &str) -> Result<JobCost, ApiError> {
    let lengths = [cwd.as_os_str().len(), home.as_os_str().len(), project.len()];
    if lengths.iter().any(|length| *length > MAX_LOOKUP_PATH_BYTES) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "project lookup input exceeds the supported path limit",
        ));
    }
    // A bounded FIFO plus at most two canonical candidates, one current path,
    // copied inputs and 8MiB directory/native scratch. Four bytes per path unit
    // conservatively cover platform representation. Cooperative declaration,
    // not an allocator or native library RSS guarantee.
    let input_bytes = lengths
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .and_then(|bytes| bytes.checked_add((MAX_PENDING_DIRECTORIES + 3) * MAX_LOOKUP_PATH_BYTES))
        .and_then(|bytes| bytes.checked_mul(4))
        .and_then(|bytes| bytes.checked_add(8 * 1024 * 1024))
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "project lookup allocation size overflow",
            )
        })?;
    Ok(JobCost {
        input_bytes,
        result_bytes: LOOKUP_RESULT_BYTES,
    })
}
async fn lookup_with<F>(
    client: &ExecutionClient,
    cwd: &Path,
    home: &Path,
    project: &str,
    resolve: F,
) -> Result<Retained<PathBuf>, ApiError>
where
    F: FnOnce(&LookupInput) -> Result<PathBuf, LookupIssue> + Send + 'static,
{
    let cost = lookup_cost(cwd, home, project)?;
    // Refuse before copying; HTTP callers can retry without another queued
    // native job or a semantic agent/workspace operation having started.
    let reservation = client
        .foundation
        .try_reserve(Lane::Io, cost)
        .map_err(|reason| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("project lookup admission unavailable: {reason:?}"),
            )
        })?;
    let input = LookupInput {
        session_cwd: cwd.to_path_buf(),
        home_dir: home.to_path_buf(),
        project: project.to_owned(),
    };
    match client
        .run_reserved(reservation, move |_| resolve(&input))
        .await
    {
        Ok(path) => Ok(path),
        Err(ExecutionError::Failed(failure)) => {
            let (issue, retention) = failure.into_parts();
            Err(issue.into_error(retention))
        }
        Err(error) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("project lookup worker failed: {error:?}"),
        )),
    }
}

/// Resolves an existing directory. Absolute paths are canonicalized directly;
/// bare names search `~/dev` to depth two and fail closed when ambiguous.
fn resolve_project_path(input: &LookupInput) -> Result<PathBuf, LookupIssue> {
    let project = input.project.trim();
    if project.is_empty() {
        return Err(LookupIssue::Invalid(
            "project must be a non-empty directory path or project name".to_string(),
        ));
    }
    let supplied_path = PathBuf::from(project);
    // `'/'` is a path separator on both platforms; `MAIN_SEPARATOR` adds `\`
    // on Windows only -- it must stay a separate check rather than a
    // hardcoded `'\\'`, since backslash is a legal filename character on
    // Unix. Checking `MAIN_SEPARATOR` alone missed a forward-slash relative
    // path (e.g. "sub/dir") on Windows, misrouting it into the bare-name
    // search below.
    if supplied_path.is_absolute()
        || project.contains('/')
        || project.contains(std::path::MAIN_SEPARATOR)
    {
        return canonical_directory(&supplied_path);
    }

    let mut matches = BTreeSet::new();
    // A stale or now-unavailable `session_cwd` must not abort the whole
    // lookup via `?` -- it should just fail to contribute a candidate, the
    // same way an unresolvable dev-tree candidate below is skipped rather
    // than treated as a hard error.
    if input
        .session_cwd
        .file_name()
        .is_some_and(|name| name == project)
    {
        match canonical_directory(&input.session_cwd) {
            Ok(path) => {
                matches.insert(path);
            }
            Err(issue @ LookupIssue::Resource(_)) => return Err(issue),
            Err(LookupIssue::Invalid(_)) => {}
        }
    }
    let development_root = checked_child(&input.home_dir, std::ffi::OsStr::new("dev"))?;
    for candidate in project_name_candidates(&development_root, project)? {
        matches.insert(candidate);
    }
    match matches.len() {
        0 => Err(LookupIssue::Invalid(format!(
            "could not find project {project:?}; provide its absolute path"
        ))),
        1 => Ok(matches.into_iter().next().expect("one project match")),
        _ => Err(LookupIssue::Invalid(format!(
            "project name {project:?} is ambiguous; provide its absolute path"
        ))),
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, LookupIssue> {
    // `paths::canonicalize`, not `std::fs::canonicalize`: the result becomes
    // the spawned agent's working directory, and a raw Windows
    // extended-length prefix (`\\?\C:\...`) breaks `cmd.exe` there.
    let canonical = paths::canonicalize(path).map_err(|error| {
        LookupIssue::Invalid(format!("project directory is unavailable: {error}"))
    })?;
    if canonical.as_os_str().len() > MAX_LOOKUP_PATH_BYTES {
        return Err(LookupIssue::Resource(
            "canonical project path exceeds lookup capacity",
        ));
    }
    if !canonical.is_dir() {
        return Err(LookupIssue::Invalid(
            "project must identify a directory".to_string(),
        ));
    }
    Ok(canonical)
}

fn checked_child(parent: &Path, name: &std::ffi::OsStr) -> Result<PathBuf, LookupIssue> {
    let bytes = parent
        .as_os_str()
        .len()
        .checked_add(name.len())
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n <= MAX_LOOKUP_PATH_BYTES)
        .ok_or(LookupIssue::Resource(
            "project scan path exceeds lookup capacity",
        ))?;
    let mut path = PathBuf::with_capacity(
        bytes
            .checked_mul(4)
            .ok_or(LookupIssue::Resource("project path size overflow"))?,
    );
    path.push(parent);
    path.push(name);
    Ok(path)
}
fn project_name_candidates(root: &Path, name: &str) -> Result<Vec<PathBuf>, LookupIssue> {
    project_name_candidates_bounded(root, name, MAX_LOOKUP_ENTRIES, MAX_PENDING_DIRECTORIES)
}
fn project_name_candidates_bounded(
    root: &Path,
    name: &str,
    max_entries: usize,
    max_pending: usize,
) -> Result<Vec<PathBuf>, LookupIssue> {
    if root.as_os_str().len() > MAX_LOOKUP_PATH_BYTES || max_pending == 0 {
        return Err(LookupIssue::Resource(
            "project scan root exceeds lookup capacity",
        ));
    }
    let mut matches = Vec::with_capacity(2);
    let mut pending = VecDeque::from([(root.to_path_buf(), 0_u8)]);
    let mut remaining = max_entries;
    while let Some((directory, depth)) = pending.pop_front() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            // A directory-open failure still follows the existing unavailable
            // directory policy. A failure after enumeration starts makes the
            // scan incomplete and cannot support an authoritative unique match.
            let entry = entry.map_err(|_| {
                LookupIssue::Resource("project scan enumeration failed before completion")
            })?;
            remaining = remaining.checked_sub(1).ok_or(LookupIssue::Resource(
                "project scan entry capacity exhausted before completion",
            ))?;
            let entry_name = entry.file_name();
            if entry_name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = checked_child(&directory, &entry_name)?;
            if !path.is_dir() {
                continue;
            }
            if entry_name == name {
                match canonical_directory(&path) {
                    Ok(canonical) if !matches.contains(&canonical) => {
                        matches.push(canonical);
                        // Two distinct canonical matches prove ambiguity even
                        // without completing the remaining scan.
                        if matches.len() == 2 {
                            return Ok(matches);
                        }
                    }
                    Err(issue @ LookupIssue::Resource(_)) => return Err(issue),
                    _ => {}
                }
            }
            if depth < 1 {
                if pending.len() >= max_pending {
                    return Err(LookupIssue::Resource(
                        "project scan directory capacity exhausted before completion",
                    ));
                }
                pending.push_back((path, depth + 1));
            }
        }
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::{project_name_candidates, CreateAgentRequest, HttpWorkspaceSpec};

    #[test]
    fn create_agent_request_accepts_server_chosen_worktree_path_and_legacy_shape() {
        let legacy: CreateAgentRequest = serde_json::from_value(serde_json::json!({
            "agent_type": "codex",
            "project": "/tmp/example",
            "prompt": "inspect"
        }))
        .expect("existing API request");
        assert!(legacy.workspace.is_none());

        let worktree: CreateAgentRequest = serde_json::from_value(serde_json::json!({
            "agent_type": "claude",
            "project": "/tmp/example",
            "prompt": "inspect",
            "workspace": {
                "branch": "agent/inspect",
                "base": "main"
            }
        }))
        .expect("worktree API request");
        assert!(matches!(worktree.workspace, Some(HttpWorkspaceSpec { .. })));
        let path_override = serde_json::json!({
            "agent_type": "claude",
            "project": "/tmp/example",
            "prompt": "inspect",
            "workspace": {
                "branch": "agent/inspect",
                "path": "/tmp/forbidden-override"
            }
        });
        assert!(serde_json::from_value::<CreateAgentRequest>(path_override).is_err());
    }

    #[test]
    fn project_name_lookup_finds_nested_directories() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("ai").join("ilium");
        std::fs::create_dir_all(&target).expect("project directory");

        assert_eq!(
            project_name_candidates(directory.path(), "ilium").expect("complete scan"),
            vec![ilium_platform::paths::canonicalize(&target).unwrap()]
        );
    }

    #[test]
    fn project_name_lookup_stops_at_depth_two() {
        let directory = tempfile::tempdir().expect("tempdir");
        // A depth-three directory must never be a candidate: the documented
        // contract is a depth-two search of the dev tree.
        let too_deep = directory.path().join("ai").join("vendor").join("ilium");
        std::fs::create_dir_all(&too_deep).expect("nested directory");

        assert_eq!(
            project_name_candidates(directory.path(), "ilium").expect("complete scan"),
            Vec::<std::path::PathBuf>::new()
        );
    }

    #[test]
    fn incomplete_project_scan_refuses_instead_of_returning_a_unique_prefix() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("a/ilium")).unwrap();
        std::fs::create_dir_all(root.path().join("b/other")).unwrap();
        assert!(matches!(
            super::project_name_candidates_bounded(root.path(), "ilium", 1, 10),
            Err(super::LookupIssue::Resource(_))
        ));
        assert!(matches!(
            super::project_name_candidates_bounded(root.path(), "ilium", 100, 1),
            Err(super::LookupIssue::Resource(_))
        ));
        assert_eq!(
            super::project_name_candidates(root.path(), "ilium")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn lookup_deduplicates_session_directory_and_preserves_ambiguity_and_stale_cwd() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("dev/a/ilium");
        std::fs::create_dir_all(&target).unwrap();
        let mut input = super::LookupInput {
            home_dir: home.path().to_path_buf(),
            session_cwd: target.clone(),
            project: "ilium".to_owned(),
        };
        assert_eq!(
            super::resolve_project_path(&input).unwrap(),
            ilium_platform::paths::canonicalize(&target).unwrap()
        );
        input.session_cwd = home.path().join("missing/ilium");
        assert_eq!(
            super::resolve_project_path(&input).unwrap(),
            ilium_platform::paths::canonicalize(&target).unwrap()
        );
        std::fs::create_dir_all(home.path().join("dev/b/ilium")).unwrap();
        assert!(
            matches!(super::resolve_project_path(&input), Err(super::LookupIssue::Invalid(message)) if message.contains("ambiguous"))
        );
    }

    #[tokio::test]
    async fn admitted_lookup_runs_on_native_bank_and_json_bytes_keep_original_charge() {
        let owner = crate::execution::ServerExecution::start().unwrap();
        let quota = owner.quota_group();
        let baseline = quota.snapshot().jobs;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("original"), b"unchanged").unwrap();
        let caller = std::thread::current().id();
        let project = root.path().to_string_lossy().into_owned();
        let result = super::lookup_with(
            &owner.client,
            root.path(),
            root.path(),
            &project,
            move |input| {
                assert_ne!(std::thread::current().id(), caller);
                super::resolve_project_path(input)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result.view(),
            &ilium_platform::paths::canonicalize(root.path()).unwrap()
        );
        let (path, retention) = result.into_parts();
        let response = super::json_response(
            axum::http::StatusCode::OK,
            &super::CreateAgentResponse {
                pane_id: 42,
                project_path: path.display().to_string(),
                prompt_delivered: true,
            },
            Some(std::sync::Arc::new(retention)),
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let copy = bytes.clone();
        let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded["pane_id"], 42);
        assert_eq!(decoded["project_path"], path.display().to_string());
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        drop(bytes);
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        drop(copy);
        assert_eq!(quota.snapshot().jobs, baseline);
        assert_eq!(
            std::fs::read(root.path().join("original")).unwrap(),
            b"unchanged"
        );
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn lookup_error_preserves_original_json_and_charge_until_last_byte_clone() {
        use axum::response::IntoResponse;
        let owner = crate::execution::ServerExecution::start().unwrap();
        let quota = owner.quota_group();
        let baseline = quota.snapshot().jobs;
        let root = tempfile::tempdir().unwrap();
        let error = super::lookup_with(&owner.client, root.path(), root.path(), "project", |_| {
            Err(super::LookupIssue::Invalid(
                "original lookup error".to_owned(),
            ))
        })
        .await
        .err()
        .unwrap();
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        let response = error.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let clone = bytes.clone();
        let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded["error"], "original lookup error");
        drop(bytes);
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        drop(clone);
        assert_eq!(quota.snapshot().jobs, baseline);
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn lookup_refusal_precedes_callback_and_allows_original_input_retry() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let owner = crate::execution::ServerExecution::start().unwrap();
        let root = tempfile::tempdir().unwrap();
        let project = "x".repeat(super::MAX_LOOKUP_PATH_BYTES + 1);
        let called = Arc::new(AtomicBool::new(false));
        let callback = called.clone();
        let error = super::lookup_with(
            &owner.client,
            root.path(),
            root.path(),
            &project,
            move |_| {
                callback.store(true, Ordering::SeqCst);
                Ok(std::path::PathBuf::new())
            },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(project.len(), super::MAX_LOOKUP_PATH_BYTES + 1);
        owner.request_shutdown();
        let callback = called.clone();
        let error = super::lookup_with(
            &owner.client,
            root.path(),
            root.path(),
            "project",
            move |_| {
                callback.store(true, Ordering::SeqCst);
                Ok(std::path::PathBuf::new())
            },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert!(!called.load(Ordering::SeqCst));
        assert!(root.path().is_dir());
        let retry_owner = crate::execution::ServerExecution::start().unwrap();
        let project = root.path().to_string_lossy().into_owned();
        let result = super::lookup_with(
            &retry_owner.client,
            root.path(),
            root.path(),
            &project,
            super::resolve_project_path,
        )
        .await
        .unwrap();
        assert_eq!(
            result.view(),
            &ilium_platform::paths::canonicalize(root.path()).unwrap()
        );
        drop(result);
        retry_owner.request_shutdown();
    }

    #[tokio::test]
    async fn cancelled_lookup_waiter_keeps_native_callback_admitted_until_return() {
        let owner = crate::execution::ServerExecution::start().unwrap();
        let quota = owner.quota_group();
        let baseline = quota.snapshot().jobs;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_path_buf();
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(async move {
            super::lookup_with(&client, &path, &path, "project", move |input| {
                let _ = started_tx.send(());
                release_rx.recv().unwrap();
                let _ = finished_tx.send(());
                Ok(input.session_cwd.clone())
            })
            .await
        });
        started_rx.await.unwrap();
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        release_tx.send(()).unwrap();
        finished_rx.await.unwrap();
        let completed = owner.client.completion_notification();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let notification = completed.notified();
                tokio::pin!(notification);
                notification.as_mut().enable();
                if quota.snapshot().jobs == baseline {
                    break;
                }
                notification.await;
            }
        })
        .await
        .unwrap();
        owner.request_shutdown();
    }
}
