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

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use ilium_core::BuiltinAgentProvider;
use ilium_platform::paths;
use serde::{Deserialize, Serialize};

use crate::config::HttpApiConfig;
use crate::ipc::handlers::create_agent_with_prompt;
use crate::state::ServerState;

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
) -> Result<Json<CreateAgentResponse>, (StatusCode, Json<ErrorResponse>)> {
    if request.prompt.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "prompt must not be empty",
        ));
    }
    // Project resolution walks the filesystem (canonicalize plus a bounded
    // `~/dev` scan), which is blocking I/O -- run it off the async runtime so
    // a slow disk or a large dev tree cannot stall the server's other duties.
    let project_path = {
        let state = Arc::clone(&state);
        let project = request.project.clone();
        tokio::task::spawn_blocking(move || resolve_project_path(&state, &project))
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("project lookup task failed: {error}"),
                )
            })?
            .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?
    };
    let pane_id = create_agent_with_prompt(
        &state,
        request.agent_type.provider(),
        project_path.clone(),
        request.prompt,
    )
    .await
    .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    Ok(Json(CreateAgentResponse {
        pane_id: pane_id.0,
        project_path: project_path.display().to_string(),
        prompt_delivered: true,
    }))
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}

/// Resolves an existing directory. Absolute paths are canonicalized directly;
/// bare names search `~/dev` to depth two and fail closed when ambiguous.
fn resolve_project_path(state: &ServerState, project: &str) -> Result<PathBuf, String> {
    let project = project.trim();
    if project.is_empty() {
        return Err("project must be a non-empty directory path or project name".to_string());
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
    if state
        .session_cwd
        .file_name()
        .is_some_and(|name| name == project)
    {
        matches.extend(canonical_directory(&state.session_cwd).ok());
    }
    let development_root = state.home_dir.join("dev");
    for candidate in project_name_candidates(&development_root, project) {
        matches.insert(candidate);
    }
    match matches.len() {
        0 => Err(format!(
            "could not find project {project:?}; provide its absolute path"
        )),
        1 => Ok(matches.into_iter().next().expect("one project match")),
        _ => Err(format!(
            "project name {project:?} is ambiguous; provide its absolute path"
        )),
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    // `paths::canonicalize`, not `std::fs::canonicalize`: the result becomes
    // the spawned agent's working directory, and a raw Windows
    // extended-length prefix (`\\?\C:\...`) breaks `cmd.exe` there.
    let canonical = paths::canonicalize(path)
        .map_err(|error| format!("project directory is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("project must identify a directory".to_string());
    }
    Ok(canonical)
}

fn project_name_candidates(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    let mut pending = VecDeque::from([(root.to_path_buf(), 0_u8)]);
    while let Some((directory, depth)) = pending.pop_front() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if entry.file_name() == name {
                if let Ok(canonical) = canonical_directory(&path) {
                    matches.push(canonical);
                }
            }
            // `depth` is the directory's own depth below `root`, so its
            // entries sit at `depth + 1`. Descending only from depth-0/1
            // directories caps candidates at depth two (`~/dev/<area>/<proj>`),
            // matching the documented contract and keeping the scan from
            // reading the contents of every depth-two directory (e.g.
            // `~/dev/<proj>/node_modules`).
            if depth < 1 {
                pending.push_back((path, depth + 1));
            }
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::project_name_candidates;

    #[test]
    fn project_name_lookup_finds_nested_directories() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("ai").join("ilium");
        std::fs::create_dir_all(&target).expect("project directory");

        assert_eq!(
            project_name_candidates(directory.path(), "ilium"),
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
            project_name_candidates(directory.path(), "ilium"),
            Vec::<std::path::PathBuf>::new()
        );
    }
}
