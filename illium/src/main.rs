//! illium: the `clap`-based CLI entrypoint -- a tmux-shaped surface over
//! `illium-client` (the TUI) and a separately-built `illium-server`
//! process. This binary owns none of the domain/PTY/detection logic
//! itself: it only parses the subcommand, ensures the target session's
//! server is running (spawning a detached `illium-server` process if not
//! -- see `session::ensure_server_running`), and then either attaches the
//! TUI (`illium_client::run`) or sends one short-lived IPC request and
//! exits. See README "Architecture" and the workspace `CLAUDE.md` for why
//! the actual logic lives in `illium-client`/`illium-server` instead of
//! here.
//!
//! `illium-server` is spawned as a *separate process*, not linked in as a
//! library: its own `main.rs` already exists as a standalone daemon
//! entrypoint (own `tokio` runtime, own tracing setup, own tiny
//! `<session-name>` argument surface) explicitly meant to be launched this
//! way -- see its module doc comment. Linking it in-process here would
//! mean this CLI's `illium_client::run` (a raw-mode terminal UI) and the
//! server's PTY/detection machinery sharing one process and one runtime
//! for no benefit, plus pulling every one of `illium-server`'s dependencies
//! (`sysinfo`, `crossterm`, `tracing-subscriber`, ...) into what's meant to
//! be this workspace's thinnest crate.

mod error;
mod session;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::error::CliError;

/// How long the `new-pane`/`kill-session` one-shot subcommands wait for
/// the server to confirm a request before giving up and reporting failure.
const REQUEST_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(5);

/// illium: a tmux-like terminal multiplexer TUI.
#[derive(Parser, Debug)]
#[command(
    name = "illium",
    about = "A tmux-like terminal multiplexer TUI",
    version
)]
struct Cli {
    /// Project directory for the session being attached to or created.
    /// Every pane spawns here, and the editor's file picker opens rooted
    /// here. Only meaningful for the bare (attach-or-create-default) form
    /// and `new-session`; ignored by `ls`/`kill-session`/`new-pane`.
    #[arg(long, global = true, default_value = ".")]
    cwd: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create (if not already running) and attach to a named session.
    NewSession { name: String },
    /// List every known session and whether it's currently running.
    Ls,
    /// Gracefully end a running session: kills every pane and tears down
    /// its tree.
    KillSession { name: String },
    /// Add a pane running `cmd` to the default session, spawning that
    /// session's server if it isn't running yet. Does not attach a TUI --
    /// run bare `illium` to view the result.
    NewPane {
        #[arg(last = true, required = true, value_name = "CMD")]
        cmd: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("illium: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        None => attach_or_create(session::DEFAULT_SESSION_NAME, &cli.cwd).await,
        Some(Command::NewSession { name }) => attach_or_create(&name, &cli.cwd).await,
        Some(Command::Ls) => list_sessions(),
        Some(Command::KillSession { name }) => kill_session(&name).await,
        Some(Command::NewPane { cmd }) => new_pane(&cmd).await,
    }
}

/// Resolves and validates `--cwd`, checked before any server spawn or
/// terminal takeover so a bad `--cwd` prints a plain error to a normal
/// terminal instead of surfacing deep inside a raw-mode TUI or an
/// already-spawned server process.
fn resolve_session_cwd(cwd: &Path) -> Result<PathBuf, CliError> {
    let canonical = cwd
        .canonicalize()
        .map_err(|_source| CliError::InvalidCwd(cwd.to_path_buf()))?;
    if !canonical.is_dir() {
        return Err(CliError::InvalidCwd(cwd.to_path_buf()));
    }
    Ok(canonical)
}

/// The bare-invocation and `new-session` paths: ensure the session's
/// server is running, then hand off to the TUI until the user quits or
/// the connection drops.
async fn attach_or_create(session_name: &str, cwd: &Path) -> Result<(), CliError> {
    let session_cwd = resolve_session_cwd(cwd)?;
    session::ensure_server_running(session_name, &session_cwd).await?;
    illium_client::run(illium_client::RunOptions {
        session_name: session_name.to_string(),
        session_cwd,
    })
    .await?;
    Ok(())
}

fn list_sessions() -> Result<(), CliError> {
    let sessions = session::list_sessions()?;
    if sessions.is_empty() {
        println!("no sessions");
        return Ok(());
    }
    for listing in &sessions {
        let status = if listing.live {
            "running"
        } else {
            "not running (stale socket removed)"
        };
        println!("{:<24} {status}", listing.name);
    }
    Ok(())
}

async fn kill_session(session_name: &str) -> Result<(), CliError> {
    let socket_path = session::socket_path(session_name)?;
    if !session::is_session_live(&socket_path) {
        return Err(CliError::SessionNotRunning(session_name.to_string()));
    }

    let mut connection =
        illium_client::connection::Connection::connect(&socket_path, session_name.to_string())
            .await?;
    connection
        .requests
        .send(illium_ipc::ClientRequest::KillSession)
        .map_err(|_send_error| CliError::SessionNotRunning(session_name.to_string()))?;

    // `illium_server::run`'s `KillSession` path closes every connection on
    // this session (including this one) after broadcasting the final
    // `TreeSnapshot` and a short grace period -- draining events until the
    // channel closes is this CLI's confirmation the shutdown actually
    // completed, not merely that the request was sent. A closed channel
    // (`recv` returning `None`) and a timed-out wait are both treated as
    // "done here" rather than errors: either way there is nothing further
    // this connection can do, and the server process tears down its own
    // socket file regardless.
    let _ = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while connection.events.recv().await.is_some() {}
    })
    .await;

    println!("session {session_name:?} killed");
    Ok(())
}

async fn new_pane(cmd: &[String]) -> Result<(), CliError> {
    let session_name = session::DEFAULT_SESSION_NAME;
    // No `--cwd` flag on this subcommand (see README's minimal CLI
    // surface) -- a freshly-spawned default session simply roots its
    // panes at wherever this command was actually run from.
    let session_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    session::ensure_server_running(session_name, &session_cwd).await?;
    let socket_path = session::socket_path(session_name)?;

    let mut connection =
        illium_client::connection::Connection::connect(&socket_path, session_name.to_string())
            .await?;

    // `Connection::connect` already queued the initial `Attach`. Its
    // direct-reply `TreeSnapshot` carries the pane count *before* this
    // command's own pane exists; recording it here (skipping over any
    // unrelated `ScreenUpdate`/`PaneStatusChanged` broadcasts that might
    // interleave from other panes/clients already attached to this
    // session) lets the wait below tell "our `NewPane` landed" apart from
    // "some unrelated `TreeSnapshot` broadcast arrived" instead of just
    // trusting the first `TreeSnapshot` it happens to see.
    let baseline_pane_count = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            if let illium_ipc::ServerEvent::TreeSnapshot(tree) = event {
                return Some(tree.panes().count());
            }
        }
        None
    })
    .await
    .ok()
    .flatten();

    let command_line = cmd.join(" ");
    connection
        .requests
        .send(illium_ipc::ClientRequest::NewPane {
            parent_group: illium_core::ROOT_ID,
            kind: illium_ipc::NewPaneKind::Command(command_line),
        })
        .map_err(|_send_error| {
            CliError::ServerReportedError("connection closed before NewPane was sent".to_string())
        })?;

    let outcome = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            match event {
                illium_ipc::ServerEvent::Error { message } => return Some(Err(message)),
                illium_ipc::ServerEvent::TreeSnapshot(tree) => {
                    let grew =
                        baseline_pane_count.is_none_or(|baseline| tree.panes().count() > baseline);
                    if grew {
                        return Some(Ok(()));
                    }
                    // A `TreeSnapshot` that didn't grow the pane count is
                    // some other client's unrelated mutation racing on the
                    // same session -- keep waiting for our own.
                }
                _ => {}
            }
        }
        None
    })
    .await;

    let _ = connection.requests.send(illium_ipc::ClientRequest::Detach);

    match outcome {
        Ok(Some(Ok(()))) => {
            println!("pane created in session {session_name:?}");
            Ok(())
        }
        Ok(Some(Err(message))) => Err(CliError::ServerReportedError(message)),
        Ok(None) | Err(_) => Err(CliError::ServerReportedError(
            "no confirmation received from the server".to_string(),
        )),
    }
}
