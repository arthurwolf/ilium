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
    let format = args.format()?;
    let target = args.target();
    if let Some(output) = args::offline(&args.command, format)? {
        print!("{output}");
        return Ok(());
    }
    if args.command.is_read() {
        return read::run(args.command, target, format, cwd).await;
    }
    write::run(args.command, target, cwd).await
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
