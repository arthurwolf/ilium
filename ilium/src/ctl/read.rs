//! `ilium ctl` commands that only read state.

use std::path::Path;
use std::time::Duration;

use ilium_control::documents::{self, Document, PaneFilter};
use ilium_control::{render_document, resolve_pane, Format, Shape};
use ilium_core::NodeId;
use serde_json::{json, Value};

use super::args::{CtlCommand, Target};
use super::link::{self, Link};
use super::CtlFailure;
use crate::pane_scan::{self, PaneEvidence};

pub(super) async fn run(
    command: CtlCommand,
    target: Target,
    format: Format,
    cwd: &Path,
) -> Result<(), CtlFailure> {
    if matches!(command, CtlCommand::Sessions) {
        return sessions(format).await;
    }
    let mut link = link::open(&target, cwd, false).await?;
    let outcome = dispatch(command, &mut link).await;
    link.detach().await;
    let document = outcome?;
    print!(
        "{}",
        render_document(&document.value, format, document.shape)
    );
    Ok(())
}

pub(super) async fn document_at(
    command: CtlCommand,
    target: Target,
    socket: crate::session::LiveSessionSocket,
) -> Result<Document, CtlFailure> {
    let mut link = link::open_at(&target, socket).await?;
    let outcome = dispatch(command, &mut link).await;
    link.detach().await;
    outcome
}

async fn dispatch(command: CtlCommand, link: &mut Link) -> Result<Document, CtlFailure> {
    let tree = &link.tree().clone();
    let session = &link.session_name().to_owned();
    Ok(match command {
        CtlCommand::Tree {
            depth,
            project,
            no_details,
            ..
        } => documents::tree_document(tree, session, project.as_deref(), depth, !no_details)?,
        CtlCommand::Project {
            selector,
            depth,
            no_details,
        } => documents::project_document(tree, &selector, depth, !no_details)?,
        CtlCommand::Node {
            id,
            depth,
            no_details,
        } => documents::node_document(tree, &id, depth, !no_details)?,
        CtlCommand::Pane {
            id, screen, lines, ..
        } => {
            let mut document = documents::pane_document(tree, &id)?;
            if screen {
                let pane_id = resolve_pane(tree, &id)?;
                let text = screen_text(link, pane_id, lines).await?;
                if let Some(object) = document.value.as_object_mut() {
                    object.insert("screen".into(), Value::String(text));
                }
            }
            document
        }
        CtlCommand::Find {
            agent,
            state,
            kind,
            project,
            name_contains,
            ..
        } => documents::find_panes(
            tree,
            &PaneFilter {
                agent,
                state,
                kind,
                project,
                name_contains,
            },
        )?,
        CtlCommand::Screen { id, lines } => {
            let pane_id = resolve_pane(tree, &id)?;
            let text = screen_text(link, pane_id, lines).await?;
            Document {
                value: json!({"pane": pane_id.0, "screen": text}),
                shape: Shape::Generic,
            }
        }
        CtlCommand::Whoami => whoami(link)?,
        CtlCommand::Wait {
            ids,
            all,
            state,
            timeout,
            ..
        } => wait(link, &ids, &state, all, timeout).await?,
        _ => return Err(CtlFailure::usage("not a read command")),
    })
}

async fn screen_text(
    link: &mut Link,
    pane_id: NodeId,
    lines: Option<usize>,
) -> Result<String, CtlFailure> {
    let mut evidence = PaneEvidence::default();
    link.attached
        .collect_shell_screens(&[pane_id], &mut evidence)
        .await;
    if let Some(message) = evidence.screen_errors.remove(&pane_id) {
        return Err(CtlFailure::rejected(format!(
            "cannot read the screen of pane {}: {message}",
            pane_id.0
        )));
    }
    let text = evidence.screen_text.remove(&pane_id).unwrap_or_default();
    let Some(lines) = lines else {
        return Ok(text);
    };
    let all: Vec<&str> = text.lines().collect();
    Ok(all[all.len().saturating_sub(lines)..].join("\n"))
}

fn whoami(link: &Link) -> Result<Document, CtlFailure> {
    let identity = crate::pane_identity_from_env()
        .map_err(|_| CtlFailure::usage("not inside an Ilium pane (ILIUM_PANE_ID is not set)"))?;
    let mut value = json!({
        "pane": identity.pane_id.0,
        "session": identity.session_name,
        "socket": identity.socket_path.to_string_lossy(),
    });
    if let Ok(document) = documents::pane_document(link.tree(), &identity.pane_id.0.to_string()) {
        value["node"] = document.value;
    }
    Ok(Document {
        value,
        shape: Shape::Generic,
    })
}

async fn wait(
    link: &mut Link,
    selectors: &[String],
    states: &str,
    all: bool,
    timeout: Option<u64>,
) -> Result<Document, CtlFailure> {
    use ilium_control::waiting::{evaluate, WaitMode};
    use ilium_ipc::ServerEvent;
    let panes = selectors
        .iter()
        .map(|selector| resolve_pane(link.tree(), selector))
        .collect::<Result<Vec<_>, _>>()?;
    let mode = if all { WaitMode::All } else { WaitMode::Any };
    let deadline =
        timeout.map(|seconds| tokio::time::Instant::now() + Duration::from_secs(seconds));
    loop {
        let result = evaluate(link.tree(), &panes, states, mode).map_err(CtlFailure::usage)?;
        if result.ready {
            return Ok(Document {
                value: serde_json::to_value(result)
                    .map_err(|error| CtlFailure::rejected(error.to_string()))?,
                shape: Shape::Generic,
            });
        }
        // One connection, no timer-driven refresh. timeout_at cancellation only
        // cancels this receive; it never turns a disconnect into pane closure.
        let incoming = match deadline {
            Some(deadline) => {
                tokio::time::timeout_at(deadline, link.attached.connection.events.recv())
                    .await
                    .map_err(|_| {
                        CtlFailure::rejected(format!(
                            "selected panes did not reach {states} before the wait deadline"
                        ))
                    })?
            }
            None => link.attached.connection.events.recv().await,
        }
        .ok_or_else(|| {
            CtlFailure::no_session("session disconnected while waiting; pane outcomes are unknown")
        })?;
        let (event, _retention) = incoming.into_parts();
        match event {
            ServerEvent::TreeSnapshot(tree) | ServerEvent::PaneStateSnapshot { tree, .. } => {
                link.attached.tree = tree
            }
            ServerEvent::PaneStatusChanged { pane_id, status }
            | ServerEvent::PaneDetectedStateChanged {
                pane_id, status, ..
            } => {
                if link.attached.tree.get(pane_id).is_some() {
                    link.attached
                        .tree
                        .set_pane_status(pane_id, status)
                        .map_err(|error| CtlFailure::rejected(error.to_string()))?;
                }
            }
            ServerEvent::Error { message } => return Err(CtlFailure::rejected(message)),
            _ => {}
        }
    }
}

async fn sessions(format: Format) -> Result<(), CtlFailure> {
    let sockets =
        pane_scan::live_sockets().map_err(|error| CtlFailure::no_session(error.to_string()))?;
    let mut listed = Vec::new();
    for socket in &sockets {
        match pane_scan::attach(socket).await {
            Ok(attached) => {
                listed.push(json!({
                    "session": attached.session_name,
                    "socket": socket.socket_path.to_string_lossy(),
                    "projects": attached.tree.project_ids().len(),
                    "panes": attached.tree.panes().count(),
                    "reachable": true,
                }));
                attached.detach().await;
            }
            Err(message) => listed.push(json!({
                "socket": socket.socket_path.to_string_lossy(),
                "reachable": false,
                "error": message,
            })),
        }
    }
    let value = json!({"count": listed.len(), "sessions": listed});
    print!("{}", render_document(&value, format, Shape::Generic));
    Ok(())
}
