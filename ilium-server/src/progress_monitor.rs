//! Server-run "progress monitor" for a pane's long-running task.
//!
//! An agent CLI running inside a pane opts into this by shelling out to
//! `ilium progress set --command <path> --interval-seconds <n>` (see the
//! `ilium` binary's `progress` subcommand), which reaches
//! [`crate::ipc::handlers::handle_set_pane_progress_monitor`] as
//! `ClientRequest::SetPaneProgressMonitor`. That handler spawns exactly one
//! [`spawn`] loop per pane and stores its handle on
//! `TerminalPaneRuntime::set_progress_monitor_task`, so a replacement call,
//! `ClearPaneProgressMonitor`, or pane close each has one place to cancel it.
//!
//! `command` runs every tick via [`run_one_tick`] and its stdout must be
//! exactly one JSON object, `{"percent": <0-100>, "message": <string>}`; a
//! tick that fails to spawn, times out, exits non-zero, or produces
//! unparseable stdout is logged and skipped rather than clearing or
//! otherwise touching the pane's last known-good progress -- the loop only
//! ever moves forward on a successfully parsed report. This is the first
//! ilium feature that runs an agent-authored command unattended and
//! recurring for as long as the pane exists; see
//! `ServerState::is_progress_monitor_enabled` for the live kill switch every
//! tick (and every new `SetPaneProgressMonitor` request) checks.
//!
//! **Working directory, despite the "runs in the pane" framing agents are
//! given:** [`run_one_tick`] spawns `command` from *this server process*,
//! and deliberately does not set `.current_dir(..)` on it (see that
//! function) -- there is no cheap way for the server to read the pane's
//! live shell cwd, so the subprocess inherits the server's own cwd instead,
//! which is fixed at the session's project root for the server's whole
//! lifetime (see `ilium`'s `session.rs` spawn site). That is NOT the same
//! as the pane's current directory once anything in the pane has `cd`'d
//! elsewhere. The CLI help for `ilium progress set` tells agents to use
//! absolute paths (or an explicit leading `cd /abs/path &&`) for exactly
//! this reason; if that guidance ever gets out of sync with this comment,
//! trust this one -- it describes what the code actually does.
//!
//! **Frequency:** the default tick is 1 second and [`MIN_INTERVAL`] floors
//! any lower request at 500ms, so `command` really is spawned as a fresh
//! shell process that often, for as long as the pane exists. The CLI help
//! pushes agents toward cheap, ideally O(1)/cached commands and toward
//! raising `--interval-seconds` for anything heavier rather than letting an
//! expensive probe run at the default cadence.

use std::sync::Arc;
use std::time::Duration;

use ilium_core::{NodeId, PaneProgress};
use ilium_ipc::ServerEvent;
use serde::Deserialize;
use tokio::task::JoinHandle;

use crate::state::ServerState;

/// Floor on the accepted interval: a misconfigured or malicious
/// agent-authored request for a sub-second cadence must not turn into a busy
/// loop of spawned shells. The CLI-facing contract mentions "1 second,
/// heavier commands could ask for more," so anything at or above that is
/// accepted verbatim; only a request below it is clamped up.
pub const MIN_INTERVAL: Duration = Duration::from_millis(500);

/// Upper bound on one tick's command execution. Bounds the worst case to one
/// hung "heavy" probe command per interval rather than overlapping ticks
/// stacking up indefinitely if the command occasionally runs long.
const TICK_TIMEOUT: Duration = Duration::from_secs(30);

/// The stdout contract every monitor command must satisfy. `message` is
/// optional (defaults to empty) since a bare percent is still useful; the
/// server never validates length here -- see `ilium-core::Tree::
/// set_pane_progress` for the percent clamp and `ilium-client`'s render path
/// for where an overlong message is clipped to fit.
#[derive(Debug, Deserialize)]
struct ProgressReport {
    percent: f64,
    #[serde(default)]
    message: String,
}

/// Spawns `pane_id`'s progress-monitor loop. The caller is responsible for
/// storing the returned handle (see this module's doc comment) -- this
/// function never touches pane runtime state itself, only the shared tree
/// and event broadcast.
pub fn spawn(
    state: Arc<ServerState>,
    pane_id: NodeId,
    command: String,
    interval: Duration,
) -> JoinHandle<()> {
    let interval = interval.max(MIN_INTERVAL);
    tokio::spawn(async move {
        loop {
            if !state.is_progress_monitor_enabled() {
                tracing::info!(
                    pane_id = pane_id.0,
                    "progress monitor stopping: disabled by server settings"
                );
                clear_progress(&state, pane_id).await;
                return;
            }

            if let Some(progress) = run_one_tick(&command).await {
                let mut tree = state.tree.write().await;
                let applied = tree
                    .set_pane_progress(pane_id, Some(progress.clone()))
                    .is_ok();
                drop(tree);
                if !applied {
                    // Pane closed, or no longer a terminal pane -- there is
                    // nothing left for this loop to report into.
                    return;
                }
                state.broadcast(ServerEvent::PaneProgressChanged {
                    pane_id,
                    progress: Some(progress),
                });
            }
            // A tick that failed to spawn, timed out, exited non-zero, or
            // produced unparseable stdout is logged at the failure site and
            // simply skipped -- it never clears or otherwise touches the
            // pane's last known-good progress.

            tokio::time::sleep(interval).await;
        }
    })
}

async fn run_one_tick(command: &str) -> Option<PaneProgress> {
    // `SHELL` here is the SERVER process's own environment, not the pane's --
    // same caveat as the missing `.current_dir(..)` below (see this module's
    // doc comment): nothing about this subprocess actually comes from the
    // pane. No `.current_dir(..)` call is intentional-by-omission, not an
    // oversight -- the server has no cheap way to read the pane's live shell
    // cwd, so this inherits the server's own cwd (the session's project
    // root) instead. Do not "fix" this to add an arbitrary default without
    // updating the CLI help and the module doc comment above, which both
    // depend on describing this behavior accurately.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let child = match tokio::process::Command::new(&shell)
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            tracing::warn!(%error, command, "progress monitor command failed to spawn");
            return None;
        }
    };

    let output = match tokio::time::timeout(TICK_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            tracing::warn!(%error, command, "progress monitor command failed while running");
            return None;
        }
        Err(_) => {
            tracing::warn!(command, "progress monitor command timed out");
            return None;
        }
    };

    if !output.status.success() {
        tracing::debug!(
            command,
            status = %output.status,
            stderr = %String::from_utf8_lossy(&output.stderr),
            "progress monitor command exited non-zero"
        );
        return None;
    }

    match serde_json::from_slice::<ProgressReport>(&output.stdout) {
        Ok(report) => Some(PaneProgress {
            percent: report.percent as f32,
            message: report.message,
        }),
        Err(error) => {
            tracing::debug!(
                %error,
                command,
                stdout = %String::from_utf8_lossy(&output.stdout),
                "progress monitor command produced unparseable stdout"
            );
            None
        }
    }
}

async fn clear_progress(state: &Arc<ServerState>, pane_id: NodeId) {
    let mut tree = state.tree.write().await;
    let applied = tree.set_pane_progress(pane_id, None).is_ok();
    drop(tree);
    if applied {
        state.broadcast(ServerEvent::PaneProgressChanged {
            pane_id,
            progress: None,
        });
    }
}
