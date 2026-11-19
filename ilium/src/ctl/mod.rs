//! `ilium ctl`: full remote control of a running session for agents and scripts.
//!
//! Reads print one document in the requested format (`--format json|jsonl|yaml|
//! markdown`). Mutations print JSONL (`type` is `result` or `error`). The
//! translation from flags to IPC requests, and the check that the server
//! applied them, live in `ilium-control` so the HTTP API behaves identically.
//!
//! Exit status: 0 success, 2 bad arguments or selector, 3 the server rejected
//! or did not confirm the operation, 4 no reachable session.

mod args;
mod credentials;
mod federation;
mod link;
mod read;
mod write;

use std::path::Path;

use serde_json::{json, Value};

pub(crate) use args::CtlArgs;

use crate::CliError;

/// A failure already shaped for JSONL output.
#[derive(Debug)]
pub(crate) struct CtlFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) exit: u8,
}

impl CtlFailure {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self {
            code: "invalid-argument",
            message: message.into(),
            exit: 2,
        }
    }
    pub(crate) fn rejected(message: impl Into<String>) -> Self {
        Self {
            code: "rejected",
            message: message.into(),
            exit: 3,
        }
    }
    pub(crate) fn no_session(message: impl Into<String>) -> Self {
        Self {
            code: "no-session",
            message: message.into(),
            exit: 4,
        }
    }
}

impl From<ilium_control::SelectError> for CtlFailure {
    fn from(error: ilium_control::SelectError) -> Self {
        let code = match error {
            ilium_control::SelectError::Ambiguous { .. } => "ambiguous-selector",
            _ => "not-found",
        };
        Self {
            code,
            message: error.to_string(),
            exit: 2,
        }
    }
}

impl From<ilium_control::OperationError> for CtlFailure {
    fn from(error: ilium_control::OperationError) -> Self {
        Self::usage(error.to_string())
    }
}

pub(crate) async fn ctl(args: CtlArgs, cwd: &Path) -> Result<(), CliError> {
    match run(args, cwd).await {
        Ok(()) => Ok(()),
        Err(failure) => {
            println!(
                "{}",
                json!({"type": "error", "command": "ctl", "code": failure.code, "message": failure.message})
            );
            Err(CliError::ExitStatus(failure.exit))
        }
    }
}

async fn run(args: CtlArgs, cwd: &Path) -> Result<(), CtlFailure> {
    let across_sessions = args.across_sessions();
    if across_sessions {
        if !matches!(
            &args.command,
            args::CtlCommand::Tree { .. }
                | args::CtlCommand::Find { .. }
                | args::CtlCommand::Events(_)
                | args::CtlCommand::Digest(_)
                | args::CtlCommand::Wait { .. }
        ) {
            return Err(CtlFailure::usage("all-session scope is for read commands; writes require an explicit session and target"));
        }
        if args.session.is_some() || args.socket.is_some() {
            return Err(CtlFailure::usage(
                "all-session scope conflicts with an explicit session or socket",
            ));
        }
    }
    if let args::CtlCommand::Ack { cursor, consumer } = &args.command {
        let consumer = consumer
            .clone()
            .or_else(default_monitoring_consumer)
            .unwrap_or_else(|| "default".into());
        let socket = link::resolve_socket(&args.target(), cwd, false).await?;
        let mut stream = ilium_transport::SessionEndpoint::from_path(socket.socket_path)
            .connect()
            .await
            .map_err(|error| CtlFailure::no_session(error.to_string()))?;
        let request_id = crate::next_progress_request_id();
        ilium_ipc::write_frame(
            &mut stream,
            &ilium_ipc::ClientRequest::MonitoringAck {
                request_id,
                consumer,
                cursor: cursor.clone(),
            },
        )
        .await
        .map_err(|error| CtlFailure::no_session(error.to_string()))?;
        loop {
            let event: ilium_ipc::ServerEvent = ilium_ipc::read_frame(&mut stream)
                .await
                .map_err(|error| CtlFailure::no_session(error.to_string()))?;
            if let ilium_ipc::ServerEvent::MonitoringAck {
                request_id: returned,
                result,
            } = event
            {
                if returned == request_id {
                    println!("ack cursor={}", result.map_err(CtlFailure::rejected)?);
                    return Ok(());
                }
            }
        }
    }
    if let args::CtlCommand::Pane {
        id, brief: true, ..
    } = &args.command
    {
        let pane_id = id
            .parse::<u64>()
            .map(ilium_core::NodeId)
            .map_err(|_| CtlFailure::usage("pane --brief requires a numeric pane ID"))?;
        let query = ilium_ipc::MonitoringQuery {
            view: ilium_ipc::MonitoringView::Brief,
            since: None,
            consumer: None,
            follow: false,
            min_severity: None,
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: Some(pane_id),
            max_items: 1,
            max_chars: 4000,
            format: ilium_ipc::MonitoringFormat::Json,
        };
        print!("{}", monitoring_read(&args.target(), cwd, query).await?);
        return Ok(());
    }
    if let args::CtlCommand::Events(options) | args::CtlCommand::Digest(options) = &args.command {
        let args::MonitoringArgs {
            since,
            follow,
            consumer,
            min_severity,
            kinds,
            project,
            agent,
            pane,
            max_items,
            max_chars,
            ..
        } = options;
        let view = if matches!(&args.command, args::CtlCommand::Digest(_)) {
            ilium_ipc::MonitoringView::Digest
        } else {
            ilium_ipc::MonitoringView::Events
        };
        let format = match args.format.as_deref() {
            None | Some("compact") => ilium_ipc::MonitoringFormat::Compact,
            Some("json") => ilium_ipc::MonitoringFormat::Json,
            _ => {
                return Err(CtlFailure::usage(
                    "monitoring format must be compact or json",
                ))
            }
        };
        let kinds = kinds
            .iter()
            .map(|kind| {
                ilium_ipc::MonitoringEventKind::parse(kind)
                    .ok_or_else(|| CtlFailure::usage(format!("unknown event kind {kind:?}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let query = ilium_ipc::MonitoringQuery {
            view,
            since: since.clone(),
            consumer: consumer.clone().or_else(default_monitoring_consumer),
            follow: *follow,
            min_severity: min_severity.clone(),
            kinds,
            project: project.clone(),
            agent: agent.clone(),
            pane_id: pane.map(ilium_core::NodeId),
            max_items: *max_items,
            max_chars: *max_chars,
            format,
        };
        let output = if across_sessions {
            federation::monitoring(query).await?
        } else {
            monitoring_read(&args.target(), cwd, query).await?
        };
        print!("{output}");
        return Ok(());
    }
    let format = args.format()?;
    let target = args.target();
    if let Some(output) = args::offline(&args.command, format)? {
        print!("{output}");
        return Ok(());
    }
    if across_sessions {
        return federation::read(args.command, target, format).await;
    }
    if args.command.is_read() {
        return read::run(args.command, target, format, cwd).await;
    }
    credentials::require_write_grant(&target)?;
    write::run(args.command, target, cwd).await
}

async fn monitoring_read(
    target: &args::Target,
    cwd: &Path,
    query: ilium_ipc::MonitoringQuery,
) -> Result<String, CtlFailure> {
    let socket = link::resolve_socket(target, cwd, false).await?;
    monitoring_read_at(&socket.socket_path, query).await
}

async fn monitoring_read_at(
    socket: &Path,
    query: ilium_ipc::MonitoringQuery,
) -> Result<String, CtlFailure> {
    let mut stream = ilium_transport::SessionEndpoint::from_path(socket)
        .connect()
        .await
        .map_err(|error| CtlFailure::no_session(error.to_string()))?;
    let request_id = crate::next_progress_request_id();
    ilium_ipc::write_frame(
        &mut stream,
        &ilium_ipc::ClientRequest::MonitoringEvents { request_id, query },
    )
    .await
    .map_err(|error| CtlFailure::no_session(error.to_string()))?;
    loop {
        let event: ilium_ipc::ServerEvent = ilium_ipc::read_frame(&mut stream)
            .await
            .map_err(|error| CtlFailure::no_session(error.to_string()))?;
        if let ilium_ipc::ServerEvent::MonitoringEvents {
            request_id: returned,
            result,
        } = event
        {
            if returned == request_id {
                return result.map_err(CtlFailure::rejected);
            }
        }
    }
}

fn default_monitoring_consumer() -> Option<String> {
    use sha2::{Digest, Sha256};
    let identity = crate::pane_identity_from_env().ok()?;
    let digest = format!(
        "{:x}",
        Sha256::digest(identity.socket_path.to_string_lossy().as_bytes())
    );
    Some(format!("pane:{}:{}", &digest[..16], identity.pane_id.0))
}

/// One JSONL result line for a mutation.
pub(crate) fn print_result(operation: &str, details: Value) {
    let mut record = json!({"type": "result", "ok": true, "op": operation});
    if let (Some(object), Some(extra)) = (record.as_object_mut(), details.as_object()) {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    println!("{record}");
}
