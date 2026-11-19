//! Token-file ownership and durable authorization audit for local automation.
use crate::state::ServerState;
use ilium_control::access::{AccessLevel, Tokens};
use ilium_execution::{JobCost, Lane};
use std::io::{Read, Write};
use std::sync::Arc;

pub(crate) async fn initialize(state: &Arc<ServerState>) -> Result<(), String> {
    let path = state.socket_path.with_extension("ctl-tokens.json");
    let client = &state
        .execution
        .get()
        .ok_or("control execution unavailable")?
        .client;
    let result = client
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 64 * 1024,
                result_bytes: 16 * 1024,
            },
            move |_| {
                use ilium_platform::secure_fs;
                let file = secure_fs::open_private_regular_file(
                    &path,
                    secure_fs::PrivateFileMode::CreateNew,
                );
                match file {
                    Ok(mut file) => {
                        let token = || {
                            format!(
                                "{}{}",
                                uuid::Uuid::new_v4().simple(),
                                uuid::Uuid::new_v4().simple()
                            )
                        };
                        let tokens = Tokens {
                            read: token(),
                            act: token(),
                            admin: token(),
                        };
                        serde_json::to_writer(&mut file, &tokens)?;
                        file.write_all(b"\n")?;
                        file.sync_all()?;
                        secure_fs::sync_parent_directory(&path)?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                let file =
                    secure_fs::open_private_regular_file(&path, secure_fs::PrivateFileMode::Read)?;
                let mut text = String::new();
                file.take(16 * 1024 + 1).read_to_string(&mut text)?;
                if text.len() > 16 * 1024 {
                    return Err(std::io::Error::other("control token file exceeds limit"));
                }
                let tokens: Tokens = serde_json::from_str(&text)?;
                if [
                    tokens.read.as_str(),
                    tokens.act.as_str(),
                    tokens.admin.as_str(),
                ]
                .iter()
                .any(|token| token.len() < 32)
                    || tokens.read == tokens.act
                    || tokens.read == tokens.admin
                    || tokens.act == tokens.admin
                {
                    return Err(std::io::Error::other(
                        "control token file has invalid or duplicate tokens",
                    ));
                }
                Ok::<_, std::io::Error>(text)
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    let tokens = serde_json::from_str(result.view())
        .map_err(|error| format!("invalid control token file: {error}"))?;
    state
        .control_tokens
        .set(tokens)
        .map_err(|_| "control tokens already initialized".to_owned())
}

pub(crate) fn authenticate(
    state: &ServerState,
    token: Option<&str>,
) -> Result<AccessLevel, &'static str> {
    state
        .control_tokens
        .get()
        .ok_or("control tokens unavailable")?
        .authenticate(token)
}

/// Commit the attempt before mutation. Failure refuses dispatch; no bounded queue can drop it.
pub(crate) async fn audit(
    state: &Arc<ServerState>,
    level: AccessLevel,
    caller: Option<ilium_core::NodeId>,
    operation: &str,
    target: serde_json::Value,
) -> Result<(), String> {
    let guard = state.control_audit_lock.clone().lock_owned().await;
    let path = crate::lifecycle_log::path_for_snapshot(&state.snapshot_path, &state.session_name)
        .with_file_name(format!("{}.ctl.jsonl", state.session_name));
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    let line =
        serde_json::json!({"at_unix_millis":at,"caller":{"pane":caller,"token_id":level.as_str()},
        "operation":operation,"target":target})
        .to_string()
            + "\n";
    if line.len() > 16 * 1024 {
        return Err("audit record exceeds limit".into());
    }
    let client = &state
        .execution
        .get()
        .ok_or("control execution unavailable")?
        .client;
    let _retained = client
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 64 * 1024,
                result_bytes: 1024,
            },
            move |_| {
                // The physical writer retains the lock if its caller is cancelled.
                let _guard = guard;
                use ilium_platform::secure_fs;
                let parent = path
                    .parent()
                    .ok_or_else(|| std::io::Error::other("audit path has no parent"))?;
                secure_fs::create_private_directory(parent)?;
                let mut file = secure_fs::open_private_regular_file(
                    &path,
                    secure_fs::PrivateFileMode::Append,
                )?;
                if file.metadata()?.len() >= 8 * 1024 * 1024 {
                    drop(file);
                    secure_fs::replace_file_durably(&path, &path.with_extension("jsonl.1"))?;
                    file = secure_fs::open_private_regular_file(
                        &path,
                        secure_fs::PrivateFileMode::CreateNew,
                    )?;
                }
                file.write_all(line.as_bytes())?;
                file.sync_all()?;
                secure_fs::sync_parent_directory(&path)?;
                Ok::<_, std::io::Error>(())
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn operation_target(operation: &ilium_control::Operation) -> serde_json::Value {
    let value = serde_json::to_value(operation).unwrap_or(serde_json::Value::Null);
    ["pane", "node", "parent"]
        .into_iter()
        .find_map(|key| value.get(key).cloned())
        .unwrap_or(serde_json::Value::Null)
}

/// Every HTTP write needs act; operation adapters enforce admin where required.
pub(crate) async fn guard_http(
    axum::extract::State(state): axum::extract::State<Arc<ServerState>>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let token = match request.headers().get(axum::http::header::AUTHORIZATION) {
        None => None,
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|text| text.strip_prefix("Bearer "))
        {
            Some(token) if !token.is_empty() => Some(token),
            _ => {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({"error":"invalid access token"})),
                )
                    .into_response()
            }
        },
    };
    let level = match authenticate(&state, token) {
        Ok(level) => level,
        Err(error) => {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({"error":error})),
            )
                .into_response()
        }
    };
    if request.method() == axum::http::Method::POST && level == AccessLevel::Read {
        return (
            axum::http::StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"error":"write requires act access"})),
        )
            .into_response();
    }
    request.extensions_mut().insert(level);
    next.run(request).await
}

/// Local UI IPC remains owned by the OS user; ctl opts into this audited write boundary.
pub(crate) async fn authorize_write(
    state: &Arc<ServerState>,
    token: &str,
    caller: Option<ilium_core::NodeId>,
    text: &str,
) -> Result<(), String> {
    let level = authenticate(state, Some(token)).map_err(str::to_owned)?;
    let operation: ilium_control::Operation =
        serde_json::from_str(text).map_err(|error| error.to_string())?;
    level.authorize(&operation).map_err(str::to_owned)?;
    audit(
        state,
        level,
        caller,
        operation.name(),
        operation_target(&operation),
    )
    .await
}
