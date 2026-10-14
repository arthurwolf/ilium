//! `/v1/*` routes of the loopback HTTP API: the same read documents and
//! operations as `ilium ctl`, both built by the shared `ilium-control` crate.
//!
//! Reads render straight from the live tree. Mutations are translated to the
//! ordinary IPC requests and run through `ipc::handlers::handle_request`, so
//! the HTTP boundary has no mutation logic of its own; the result is then
//! confirmed against the tree with `Operation::outcome`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ilium_control::documents::{self, Document, PaneFilter};
use ilium_control::{
    guide, operation_examples, render_document, resolve_pane, Format, Operation, SelectError, Shape,
};
use ilium_core::NodeId;
use ilium_ipc::{ClientRequest, ServerEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ipc::direct_events::DirectEventSender;
use crate::ipc::handlers::handle_request;
use crate::state::ServerState;

/// How long a mutation may take to show up in the tree.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(30);
const CONFIRM_POLL: Duration = Duration::from_millis(100);
/// Capacity of the reply channel one HTTP request gives the IPC handlers.
const REPLY_CAPACITY: usize = 256;

type ApiResult = Result<Response, (StatusCode, Json<Value>)>;

fn failure(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": message.into(), "code": code})))
}

fn select_failure(error: SelectError) -> (StatusCode, Json<Value>) {
    let (status, code) = match error {
        SelectError::Ambiguous { .. } => (StatusCode::CONFLICT, "ambiguous-selector"),
        SelectError::WrongKind { .. } => (StatusCode::BAD_REQUEST, "wrong-kind"),
        SelectError::NotFound { .. } => (StatusCode::NOT_FOUND, "not-found"),
    };
    failure(status, code, error.to_string())
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/v1/guide", get(guide_index))
        .route("/v1/guide/{topic}", get(guide_topic))
        .route("/v1/ops", get(list_operations).post(apply_operation))
        .route("/v1/tree", get(tree))
        .route("/v1/projects/{selector}", get(project))
        .route("/v1/nodes/{id}", get(node))
        .route("/v1/panes", get(find_panes).post(create_pane))
        .route("/v1/panes/{id}", get(pane))
        .route("/v1/panes/{id}/input", post(pane_input))
}

fn format_of(parameters: &HashMap<String, String>) -> Result<Format, (StatusCode, Json<Value>)> {
    match parameters.get("format") {
        None => Ok(Format::Json),
        Some(name) => Format::parse(name).ok_or_else(|| {
            failure(
                StatusCode::BAD_REQUEST,
                "invalid-argument",
                format!("unknown format {name:?}; use json, jsonl, yaml or markdown"),
            )
        }),
    }
}

fn respond(value: &Value, shape: Shape, format: Format) -> Response {
    (
        [(header::CONTENT_TYPE, format.content_type())],
        render_document(value, format, shape),
    )
        .into_response()
}

fn respond_document(document: &Document, format: Format) -> Response {
    respond(&document.value, document.shape, format)
}

fn depth_of(
    parameters: &HashMap<String, String>,
) -> Result<Option<usize>, (StatusCode, Json<Value>)> {
    parameters
        .get("depth")
        .map(|text| {
            text.parse::<usize>().map_err(|_| {
                failure(
                    StatusCode::BAD_REQUEST,
                    "invalid-argument",
                    "depth must be a number",
                )
            })
        })
        .transpose()
}

fn details_of(parameters: &HashMap<String, String>) -> bool {
    parameters
        .get("details")
        .is_none_or(|value| !matches!(value.as_str(), "0" | "false" | "no"))
}

async fn guide_index(Query(parameters): Query<HashMap<String, String>>) -> ApiResult {
    let text = match parameters.get("topic") {
        None => guide::ESSENTIALS.to_owned(),
        Some(name) if name == "topics" => guide::topic_list(),
        Some(name) => guide::topic(name)
            .map(|topic| topic.body.to_owned())
            .ok_or_else(|| {
                failure(
                    StatusCode::NOT_FOUND,
                    "not-found",
                    format!("no guide topic {name:?}\n{}", guide::topic_list()),
                )
            })?,
    };
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

async fn guide_topic(Path(name): Path<String>) -> ApiResult {
    let topic = guide::topic(&name).ok_or_else(|| {
        failure(
            StatusCode::NOT_FOUND,
            "not-found",
            format!("no guide topic {name:?}\n{}", guide::topic_list()),
        )
    })?;
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        topic.body.to_owned(),
    )
        .into_response())
}

async fn list_operations(Query(parameters): Query<HashMap<String, String>>) -> ApiResult {
    let format = format_of(&parameters)?;
    let operations: Vec<Value> = operation_examples()
        .iter()
        .filter_map(|example| serde_json::to_value(example).ok())
        .collect();
    Ok(respond(
        &json!({"operations": operations}),
        Shape::Generic,
        format,
    ))
}

async fn tree(
    State(state): State<Arc<ServerState>>,
    Query(parameters): Query<HashMap<String, String>>,
) -> ApiResult {
    let format = format_of(&parameters)?;
    let depth = depth_of(&parameters)?;
    let tree = state.tree.read().await;
    let document = documents::tree_document(
        &tree,
        &state.session_name,
        parameters.get("project").map(String::as_str),
        depth,
        details_of(&parameters),
    )
    .map_err(select_failure)?;
    Ok(respond_document(&document, format))
}

async fn project(
    State(state): State<Arc<ServerState>>,
    Path(selector): Path<String>,
    Query(parameters): Query<HashMap<String, String>>,
) -> ApiResult {
    let format = format_of(&parameters)?;
    let depth = depth_of(&parameters)?;
    let tree = state.tree.read().await;
    let document = documents::project_document(&tree, &selector, depth, details_of(&parameters))
        .map_err(select_failure)?;
    Ok(respond_document(&document, format))
}

async fn node(
    State(state): State<Arc<ServerState>>,
    Path(selector): Path<String>,
    Query(parameters): Query<HashMap<String, String>>,
) -> ApiResult {
    let format = format_of(&parameters)?;
    let depth = depth_of(&parameters)?;
    let tree = state.tree.read().await;
    let document = documents::node_document(&tree, &selector, depth, details_of(&parameters))
        .map_err(select_failure)?;
    Ok(respond_document(&document, format))
}

async fn find_panes(
    State(state): State<Arc<ServerState>>,
    Query(parameters): Query<HashMap<String, String>>,
) -> ApiResult {
    let format = format_of(&parameters)?;
    let filter = PaneFilter {
        agent: parameters.get("agent").cloned(),
        state: parameters.get("state").cloned(),
        kind: parameters.get("kind").cloned(),
        project: parameters.get("project").cloned(),
        name_contains: parameters.get("name_contains").cloned(),
    };
    let tree = state.tree.read().await;
    let document = documents::find_panes(&tree, &filter).map_err(select_failure)?;
    Ok(respond_document(&document, format))
}

async fn pane(
    State(state): State<Arc<ServerState>>,
    Path(selector): Path<String>,
    Query(parameters): Query<HashMap<String, String>>,
) -> ApiResult {
    let format = format_of(&parameters)?;
    let (mut document, pane_id) = {
        let tree = state.tree.read().await;
        let pane_id = resolve_pane(&tree, &selector).map_err(select_failure)?;
        (
            documents::pane_document(&tree, &selector).map_err(select_failure)?,
            pane_id,
        )
    };
    if parameters
        .get("screen")
        .is_some_and(|value| value != "0" && value != "false")
    {
        let lines = parameters
            .get("lines")
            .and_then(|text| text.parse::<usize>().ok());
        let text = capture_screen(&state, pane_id, lines).await?;
        if let Some(object) = document.value.as_object_mut() {
            object.insert("screen".into(), Value::String(text));
        }
    }
    Ok(respond_document(&document, format))
}

fn reply_channel(
    state: &Arc<ServerState>,
) -> Result<
    (
        DirectEventSender,
        crate::ipc::direct_events::DirectEventReceiver,
    ),
    (StatusCode, Json<Value>),
> {
    let execution = state
        .execution
        .get()
        .ok_or_else(|| {
            failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "server execution is not ready",
            )
        })?
        .client
        .clone();
    Ok(DirectEventSender::admitted_channel(
        REPLY_CAPACITY,
        execution,
    ))
}

async fn capture_screen(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    lines: Option<usize>,
) -> Result<String, (StatusCode, Json<Value>)> {
    let (sender, mut receiver) = reply_channel(state)?;
    handle_request(
        state,
        ClientRequest::GetPaneRecapScreen { pane_id },
        &sender,
    )
    .await;
    drop(sender);
    while let Some(queued) = receiver.recv_queued().await {
        if let ServerEvent::PaneRecapScreen { result, .. } = queued.event {
            let text = result
                .map_err(|message| failure(StatusCode::CONFLICT, "screen-unavailable", message))?;
            return Ok(match lines {
                Some(count) => {
                    let all: Vec<&str> = text.lines().collect();
                    all[all.len().saturating_sub(count)..].join("\n")
                }
                None => text,
            });
        }
    }
    Err(failure(
        StatusCode::INTERNAL_SERVER_ERROR,
        "no-reply",
        "no screen reply",
    ))
}

async fn apply_operation(
    State(state): State<Arc<ServerState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    let operation: Operation = serde_json::from_value(body).map_err(|error| {
        failure(
            StatusCode::BAD_REQUEST,
            "invalid-argument",
            format!("not a valid operation: {error}; GET /v1/ops lists examples"),
        )
    })?;
    run_operation(&state, operation).await
}

/// `POST /v1/panes`: the body is a `new_pane` operation without its `op` tag.
async fn create_pane(
    State(state): State<Arc<ServerState>>,
    Json(mut body): Json<Value>,
) -> ApiResult {
    if let Some(object) = body.as_object_mut() {
        object.entry("op").or_insert(json!("new_pane"));
    }
    apply_operation(State(state), Json(body)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputBody {
    #[serde(default)]
    text: Option<String>,
    #[serde(default = "default_enter")]
    enter: bool,
    #[serde(default)]
    keys: Option<Vec<String>>,
}

const fn default_enter() -> bool {
    true
}

/// `POST /v1/panes/{id}/input`: `{"text": "...", "enter": true}` or `{"keys": [...]}`.
async fn pane_input(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<u64>,
    Json(body): Json<InputBody>,
) -> ApiResult {
    let operation = match (body.text, body.keys) {
        (Some(text), None) => Operation::SendText {
            pane: id,
            text,
            enter: body.enter,
        },
        (None, Some(keys)) => Operation::SendKeys { pane: id, keys },
        _ => {
            return Err(failure(
                StatusCode::BAD_REQUEST,
                "invalid-argument",
                "give exactly one of \"text\" or \"keys\"",
            ))
        }
    };
    run_operation(&state, operation).await
}

async fn run_operation(state: &Arc<ServerState>, operation: Operation) -> ApiResult {
    let before = state.tree.read().await.clone();
    let requests = operation
        .requests(&before, next_request_id())
        .map_err(|error| {
            failure(
                StatusCode::BAD_REQUEST,
                "invalid-argument",
                error.to_string(),
            )
        })?;
    let (sender, mut receiver) = reply_channel(state)?;
    for request in requests {
        handle_request(state, request, &sender).await;
    }
    drop(sender);
    let mut rejection = None;
    while let Some(queued) = receiver.recv_queued().await {
        if let ServerEvent::Error { message } = queued.event {
            rejection = Some(message);
        }
    }
    if let Some(message) = rejection {
        return Err(failure(StatusCode::CONFLICT, "rejected", message));
    }
    let details = if operation.is_input_only() {
        json!({})
    } else {
        confirm(state, &operation, &before).await.ok_or_else(|| {
            failure(
                StatusCode::CONFLICT,
                "unconfirmed",
                format!(
                    "the server did not confirm {} within {} s; inspect the tree before retrying",
                    operation.name(),
                    CONFIRM_TIMEOUT.as_secs()
                ),
            )
        })?
    };
    let mut result = json!({"ok": true, "op": operation.name()});
    if let (Some(object), Some(extra)) = (result.as_object_mut(), details.as_object()) {
        object.extend(extra.clone());
    }
    Ok(Json(result).into_response())
}

async fn confirm(
    state: &Arc<ServerState>,
    operation: &Operation,
    before: &ilium_core::Tree,
) -> Option<Value> {
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    loop {
        {
            let tree = state.tree.read().await;
            if let Some(details) = operation.outcome(before, &tree) {
                return Some(details);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(CONFIRM_POLL).await;
    }
}

/// Request ids only need to be unique per process to correlate worktree closes.
pub(crate) fn next_request_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_errors_map_to_distinct_statuses() {
        let not_found = SelectError::NotFound {
            wanted: "pane",
            selector: "x".into(),
        };
        assert_eq!(select_failure(not_found).0, StatusCode::NOT_FOUND);
        let ambiguous = SelectError::Ambiguous {
            selector: "x".into(),
            candidates: "1, 2".into(),
        };
        assert_eq!(select_failure(ambiguous).0, StatusCode::CONFLICT);
        let wrong = SelectError::WrongKind {
            id: 1,
            actual: "group",
            wanted: "pane",
        };
        assert_eq!(select_failure(wrong).0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn request_ids_increase() {
        assert!(next_request_id() < next_request_id());
    }

    #[test]
    fn format_and_depth_parameters_validate() {
        let mut parameters = HashMap::new();
        assert!(matches!(format_of(&parameters), Ok(Format::Json)));
        parameters.insert("format".into(), "yaml".into());
        assert!(matches!(format_of(&parameters), Ok(Format::Yaml)));
        parameters.insert("format".into(), "toml".into());
        assert!(format_of(&parameters).is_err());
        parameters.insert("depth".into(), "x".into());
        assert!(depth_of(&parameters).is_err());
    }
}
