//! `ilium ctl` commands that only read state.

use std::path::Path;
use std::time::{Duration, Instant};

use ilium_control::documents::{self, Document, PaneFilter};
use ilium_control::{render_document, resolve_pane, Format, Shape};
use ilium_core::NodeId;
use serde_json::{json, Value};

use super::args::{CtlCommand, Target};
use super::link::{self, Link};
use super::CtlFailure;
use crate::pane_scan::{self, PaneEvidence};

/// How often `wait` re-reads the session.
const WAIT_INTERVAL: Duration = Duration::from_secs(1);

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

async fn dispatch(command: CtlCommand, link: &mut Link) -> Result<Document, CtlFailure> {
    let tree = &link.tree().clone();
    let session = &link.session_name().to_owned();
    Ok(match command {
        CtlCommand::Tree {
            depth,
            project,
            no_details,
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
        CtlCommand::Pane { id, screen, lines } => {
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
        CtlCommand::Wait { id, state, timeout } => wait(link, &id, &state, timeout).await?,
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
    link: &Link,
    selector: &str,
    states: &str,
    timeout: Option<u64>,
) -> Result<Document, CtlFailure> {
    let wanted: Vec<String> = states
        .split(',')
        .map(|state| state.trim().to_ascii_lowercase())
        .filter(|state| !state.is_empty())
        .collect();
    if wanted.is_empty() {
        return Err(CtlFailure::usage("--state must name at least one state"));
    }
    let pane_id = resolve_pane(link.tree(), selector)?;
    let deadline = timeout.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut tree = link.tree().clone();
    loop {
        let Some(detail) = ilium_control::pane_detail(&tree, pane_id) else {
            return Ok(Document {
                value: json!({"pane": pane_id.0, "closed": true}),
                shape: Shape::Generic,
            });
        };
        // Agents report a turn state; other panes only have a kind.
        let status = &detail["status"];
        let state = status["state"]
            .as_str()
            .or_else(|| status["kind"].as_str())
            .unwrap_or_default()
            .to_owned();
        if wanted.contains(&state) {
            return Ok(Document {
                value: json!({"pane": pane_id.0, "state": state, "closed": false}),
                shape: Shape::Generic,
            });
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(CtlFailure::rejected(format!(
                "pane {} is {state:?}, not one of {states}, after {} s",
                pane_id.0,
                timeout.unwrap_or_default()
            )));
        }
        tokio::time::sleep(WAIT_INTERVAL).await;
        tree = link.fresh_tree().await?;
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
