//! `ilium panes`: lists the panes of every running session on this machine
//! that pass the shared pane selection (see `pane_filter`). It shows exactly
//! which panes another selection-based command, such as `ilium broadcast`,
//! would act on.
//!
//! Output is JSONL on stdout: one `pane` record per selected pane, a
//! `warning` per session that could not be read, then one `summary`.

use std::path::Path;

use clap::Args;
use serde_json::json;

use crate::pane_filter::PaneFilterArgs;
use crate::pane_scan::{self, CallerPane};
use crate::CliError;

#[derive(Args, Debug)]
pub(crate) struct PanesArgs {
    #[command(flatten)]
    pub(crate) filter: PaneFilterArgs,
}

pub(crate) async fn panes(args: PanesArgs, cwd: &Path) -> Result<(), CliError> {
    let filter = match args.filter.compile(cwd) {
        Ok(filter) => filter,
        Err(message) => {
            println!(
                "{}",
                json!({"type": "error", "command": "panes", "code": "invalid-selection", "message": message})
            );
            return Err(CliError::ExitStatus(2));
        }
    };
    let caller = CallerPane::from_env();
    let sockets = pane_scan::live_sockets()?;
    let mut selected = 0_usize;
    let mut unreachable = 0_usize;
    for socket in &sockets {
        let attached = match pane_scan::attach(socket).await {
            Ok(attached) => attached,
            Err(message) => {
                unreachable += 1;
                println!(
                    "{}",
                    unreachable_warning("panes", &socket.socket_path, &message)
                );
                continue;
            }
        };
        for pane in attached.panes(caller.as_ref()) {
            if filter.accepts(&pane) {
                selected += 1;
                let mut record = pane.to_json();
                record["type"] = json!("pane");
                println!("{record}");
            }
        }
        attached.detach().await;
    }
    println!(
        "{}",
        json!({
            "type": "summary",
            "command": "panes",
            "ok": unreachable == 0,
            "sessions": sockets.len(),
            "unreachable_sessions": unreachable,
            "panes": selected,
        })
    );
    Ok(())
}

/// The `warning` record for a live session whose state could not be read.
pub(crate) fn unreachable_warning(
    command: &str,
    socket_path: &Path,
    message: &str,
) -> serde_json::Value {
    json!({
        "type": "warning",
        "command": command,
        "code": "session-unreachable",
        "socket": socket_path.to_string_lossy(),
        "message": message,
        "hint": "a server started before this command existed may not answer; its panes were not included",
    })
}
