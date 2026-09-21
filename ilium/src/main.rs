//! ilium: the `clap`-based CLI entrypoint -- a tmux-shaped surface over
//! `ilium-client` (the TUI) and a separately-built `ilium-server`
//! process. This binary owns none of the domain/PTY/detection logic
//! itself: it only parses the subcommand, ensures the target session's
//! server is running (spawning a detached `ilium-server` process if not
//! -- see `session::ensure_server_running`), and then either attaches the
//! TUI (`ilium_client::run`) or sends one short-lived IPC request and
//! exits. See ARCHITECTURE.md "Process architecture" and the workspace `CLAUDE.md` for why
//! the actual logic lives in `ilium-client`/`ilium-server` instead of
//! here.
//!
//! `ilium-server` is spawned as a *separate process*, not linked in as a
//! library: its own `main.rs` already exists as a standalone daemon
//! entrypoint (own `tokio` runtime, own tracing setup, own tiny
//! `<session-name>` argument surface) explicitly meant to be launched this
//! way -- see its module doc comment. Linking it in-process here would
//! mean this CLI's `ilium_client::run` (a raw-mode terminal UI) and the
//! server's PTY/detection machinery sharing one process and one runtime
//! for no benefit, plus pulling every one of `ilium-server`'s dependencies
//! (`sysinfo`, `crossterm`, `tracing-subscriber`, ...) into what's meant to
//! be this workspace's thinnest crate.

use ilium::session;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::time::Duration;

use clap::{Parser, Subcommand};

use ilium::error::CliError;
use ilium_platform::paths;

/// How long the `new-pane`/`kill-session` one-shot subcommands wait for
/// the server to confirm a request before giving up and reporting failure.
const REQUEST_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(5);

/// ilium: a tmux-like terminal multiplexer TUI.
#[derive(Parser, Debug)]
#[command(
    name = "ilium",
    about = "A tmux-like terminal multiplexer TUI",
    version
)]
struct Cli {
    /// Project directory for the session being attached to or created.
    /// Every pane spawns here, and the editor's file picker opens rooted
    /// here. Every command that addresses a session is scoped to this
    /// canonical project directory.
    #[arg(long, global = true, default_value = ".")]
    cwd: PathBuf,

    /// Replace this project's running server before attaching while retaining
    /// its snapshot. Useful after installing a new server binary.
    #[arg(long, global = true)]
    restart_server: bool,

    /// Delete this project's named session snapshot and start it empty. This
    /// is intentionally destructive and never affects another project.
    #[arg(long, global = true, conflicts_with = "restart_server")]
    reset_session: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create (if not already running) and attach to a named session.
    NewSession { name: String },
    /// List this project's known sessions and whether each is currently running.
    Ls,
    /// Gracefully end a running session: kills every pane and tears down
    /// its tree.
    KillSession { name: String },
    /// Add a pane running `cmd` to this project's default session, spawning that
    /// session's server if it isn't running yet. Does not attach a TUI --
    /// run bare `ilium` to view the result.
    NewPane {
        /// Project-local session to receive the pane.
        #[arg(long, default_value = session::DEFAULT_SESSION_NAME)]
        session_name: String,
        #[arg(last = true, required = true, value_name = "CMD")]
        cmd: Vec<String>,
    },
    /// Read, initialize, or post to this project's file-backed agent room.
    Chat {
        #[command(subcommand)]
        command: ChatCommand,
    },
    /// Reports (or clears) a long-running task's progress from inside the
    /// pane running it. Unlike every other subcommand here, this one is
    /// meant to be run by an agent CLI (or a script it wrote) from *inside*
    /// an already-running pane, not from an arbitrary shell -- see
    /// `pane_identity_from_env`. See `ProgressCommand::Set` for the working-
    /// directory caveat: the polled `command` does NOT run with the pane's
    /// live shell cwd.
    Progress {
        #[command(subcommand)]
        command: ProgressCommand,
    },
}

/// Local chatroom operations intentionally avoid the detached server: agents
/// can coordinate through the project file even when no TUI is attached.
#[derive(Subcommand, Debug)]
enum ChatCommand {
    /// Create CHATROOM.md and install the project guidance/hook bridge.
    Init,
    /// Post one message as the calling agent. The author defaults to
    /// ILIUM_CHATROOM_AUTHOR, then AGENT_NAME, then `agent`.
    Send {
        #[arg(long)]
        message: String,
        #[arg(long)]
        author: Option<String>,
    },
    /// Print the recent room tail in a form that lifecycle hooks inject into an agent turn.
    Context {
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// Print the most recent room records for direct terminal inspection.
    Tail {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

/// Local mirror of the `[percent, message]` contract every progress-monitor
/// command's stdout must satisfy -- see `ilium-server`'s `progress_monitor`
/// module doc for the full contract this subcommand's `Set` variant installs.
#[derive(Subcommand, Debug)]
enum ProgressCommand {
    /// Starts (or replaces) this pane's server-run progress monitor:
    /// `command` runs every `interval-seconds` and its stdout must be
    /// exactly one JSON object, `{"percent": <0-100>, "message": <string>}`.
    /// Prefer more detail in `message` over less -- it is clipped to fit,
    /// not rejected for being long.
    ///
    /// WORKING DIRECTORY: `command` is spawned by the ilium SERVER process,
    /// not by the pane's own shell -- despite running "in the pane", it does
    /// NOT inherit the pane's live cwd (wherever an agent or user has since
    /// `cd`'d to), your own shell's cwd, or any of the pane's exported
    /// variables/aliases. It only inherits the server's own working
    /// directory, which is fixed at the session's project root for the
    /// lifetime of the server process. A relative path in `command` is
    /// therefore silently wrong the moment the pane's actual cwd diverges
    /// from that project root (the common failure mode: a plausible-looking
    /// command that spawns fine, produces valid JSON, and just always
    /// reports 0%/empty because every relative path it touches resolves
    /// against the wrong directory). Always use absolute paths inside
    /// `command`, or an explicit unconditional `cd /abs/path && ...` at its
    /// start -- never rely on inherited relative-path resolution.
    ///
    /// PERFORMANCE: this command is spawned as a brand-new shell process on
    /// every tick (default every 1 second, `MIN_INTERVAL` floors it at
    /// 500ms), for as long as the pane exists. It must be cheap: prefer O(1)
    /// or cached state reads over recursive filesystem walks, avoid
    /// spawning further heavy subprocesses from within it, and avoid network
    /// calls unless truly necessary. If the underlying check is inherently
    /// expensive, raise `--interval-seconds` rather than letting an
    /// expensive command run every second -- a stale-by-a-few-seconds
    /// number beats a pane that is constantly forking work to answer it.
    Set {
        #[arg(long)]
        command: String,
        /// How often `command` re-runs. 1 second is the common case; ask
        /// for more only if `command` itself is heavy enough that running
        /// it every second would be wasteful. See the working-directory and
        /// performance notes above.
        #[arg(long, default_value_t = 1)]
        interval_seconds: u32,
    },
    /// Stops this pane's active progress monitor, if any, and clears its
    /// last reported progress.
    Clear,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, error_debug = ?error, "ilium CLI action failed");
            eprintln!("ilium: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        None => {
            attach_or_create(
                session::DEFAULT_SESSION_NAME,
                &cli.cwd,
                cli.restart_server,
                cli.reset_session,
            )
            .await
        }
        Some(Command::NewSession { name }) => {
            attach_or_create(&name, &cli.cwd, cli.restart_server, cli.reset_session).await
        }
        Some(Command::Ls) => list_sessions(&cli.cwd),
        Some(Command::KillSession { name }) => kill_session(&name, &cli.cwd).await,
        Some(Command::NewPane { session_name, cmd }) => {
            new_pane(&session_name, &cmd, &cli.cwd).await
        }
        Some(Command::Chat { command }) => chat(command, &cli.cwd),
        Some(Command::Progress { command }) => progress(command).await,
    }
}

fn chat(command: ChatCommand, cwd: &Path) -> Result<(), CliError> {
    // `paths::canonicalize` strips Windows' extended-length `\\?\` prefix:
    // this value is printed straight to the user below ("chatroom ready at
    // ...") and written into `CHATROOM.md`, neither of which should show it.
    let cwd = paths::canonicalize(cwd).map_err(|_| CliError::InvalidCwd(cwd.to_path_buf()))?;
    if !cwd.is_dir() {
        return Err(CliError::InvalidCwd(cwd.to_path_buf()));
    }
    match command {
        ChatCommand::Init => {
            ilium_client::chatroom::initialize(&cwd)?;
            println!("chatroom ready at {}", cwd.join("CHATROOM.md").display());
            Ok(())
        }
        ChatCommand::Send { message, author } => {
            let project_root = chatroom_project_root(&cwd);
            let author = author.unwrap_or_else(default_chatroom_author);
            ilium_client::chatroom::append_message(&project_root, &author, &message)?;
            println!("chatroom message sent");
            Ok(())
        }
        ChatCommand::Context { limit } => {
            let project_root = chatroom_project_root(&cwd);
            println!("{}", ilium_client::chatroom::context(&project_root, limit)?);
            Ok(())
        }
        ChatCommand::Tail { limit } => {
            let project_root = chatroom_project_root(&cwd);
            for message in ilium_client::chatroom::read_messages(&project_root, limit)? {
                println!(
                    "{} | {} | {}",
                    message.timestamp, message.author, message.content
                );
            }
            Ok(())
        }
    }
}

/// Finds the nearest owning room so a provider hook also works when an agent
/// starts in a project subdirectory rather than exactly at the project root.
fn chatroom_project_root(cwd: &Path) -> PathBuf {
    cwd.ancestors()
        .find(|ancestor| ilium_client::chatroom::exists(ancestor))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.to_path_buf())
}

fn default_chatroom_author() -> String {
    std::env::var("ILIUM_CHATROOM_AUTHOR")
        .or_else(|_| std::env::var("AGENT_NAME"))
        .unwrap_or_else(|_| "agent".to_string())
}

/// This pane's identity, as injected by `ilium-server` at spawn time (see
/// `ilium_ipc::pane_env`) -- what `progress` needs to address the exact pane
/// and server this process happens to be running inside.
struct PaneIdentity {
    pane_id: ilium_core::NodeId,
    session_name: String,
    socket_path: PathBuf,
}

fn pane_identity_from_env() -> Result<PaneIdentity, CliError> {
    let pane_id = std::env::var(ilium_ipc::pane_env::PANE_ID)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(ilium_core::NodeId)
        .ok_or(CliError::NotInsideIliumPane(ilium_ipc::pane_env::PANE_ID))?;
    let session_name = std::env::var(ilium_ipc::pane_env::SESSION_NAME)
        .map_err(|_| CliError::NotInsideIliumPane(ilium_ipc::pane_env::SESSION_NAME))?;
    let socket_path = std::env::var(ilium_ipc::pane_env::SESSION_SOCKET)
        .map(PathBuf::from)
        .map_err(|_| CliError::NotInsideIliumPane(ilium_ipc::pane_env::SESSION_SOCKET))?;
    Ok(PaneIdentity {
        pane_id,
        session_name,
        socket_path,
    })
}

/// How long `progress` waits for a rejection before assuming its request was
/// accepted. Neither `SetPaneProgressMonitor` nor `ClearPaneProgressMonitor`
/// broadcasts a success confirmation -- the monitor's first tick may be
/// seconds away, or never arrive at all if the command turns out to be bad
/// -- so only a `ServerEvent::Error` is worth waiting for; a quiet window
/// this short keeps the common case (an agent calling this once at the start
/// of a long task) from feeling like it hung.
const PROGRESS_REQUEST_QUIET_WINDOW: Duration = Duration::from_millis(750);

async fn progress(command: ProgressCommand) -> Result<(), CliError> {
    let identity = pane_identity_from_env()?;
    let mut connection =
        ilium_client::connection::Connection::connect(&identity.socket_path, identity.session_name)
            .await?;

    let (request, accepted_message) = match command {
        ProgressCommand::Set {
            command,
            interval_seconds,
        } => (
            ilium_ipc::ClientRequest::SetPaneProgressMonitor {
                pane_id: identity.pane_id,
                command,
                interval_seconds,
            },
            "progress monitor started",
        ),
        ProgressCommand::Clear => (
            ilium_ipc::ClientRequest::ClearPaneProgressMonitor {
                pane_id: identity.pane_id,
            },
            "progress monitor cleared",
        ),
    };
    connection
        .requests
        .send(request)
        .await
        .map_err(|_send_error| {
            CliError::ServerReportedError(
                "connection closed before the request was sent".to_string(),
            )
        })?;

    let rejection = tokio::time::timeout(PROGRESS_REQUEST_QUIET_WINDOW, async {
        while let Some(event) = connection.events.recv().await {
            if let ilium_ipc::ServerEvent::Error { message } = event {
                return Some(message);
            }
        }
        None
    })
    .await;

    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;

    match rejection {
        Ok(Some(message)) => Err(CliError::ServerReportedError(message)),
        Ok(None) | Err(_) => {
            println!("{accepted_message}");
            Ok(())
        }
    }
}

/// The bare-invocation and `new-session` paths: ensure the session's
/// server is running, then hand off to the TUI until the user quits or
/// the connection drops.
async fn attach_or_create(
    session_name: &str,
    cwd: &Path,
    should_restart_server: bool,
    should_reset_session: bool,
) -> Result<(), CliError> {
    let project_session = session::resolve_project_session(cwd, session_name)?;
    // Capture this before the TUI starts. A later `make install` can replace
    // the directory entry backing the running executable, but the original
    // path remains the correct location from which Restart must load it.
    let client_executable = std::env::current_exe().map_err(CliError::ResolveClientExecutable)?;
    let log_path = if should_reset_session {
        session::reset_session(&project_session).await?
    } else if should_restart_server {
        session::replace_server(&project_session).await?
    } else {
        session::ensure_server_running(&project_session).await?
    };
    initialize_cli_logging(&log_path)?;
    tracing::info!(
        session_name,
        should_restart_server,
        should_reset_session,
        "interactive session attach started"
    );
    let exit_reason = ilium_client::run(ilium_client::RunOptions {
        session_name: project_session.name.clone(),
        session_cwd: project_session.project_root.clone(),
        socket_path: project_session.socket_path.clone(),
        log_path,
    })
    .await?;
    match exit_reason {
        ilium_client::ClientExitReason::Quit => Ok(()),
        ilium_client::ClientExitReason::RestartRequested => {
            restart_client_process(&client_executable, &project_session)
        }
    }
}

/// Replaces this client process with the executable captured before the TUI
/// started. The reconstructed invocation carries only project/session identity,
/// so `--restart-server` and `--reset-session` can never leak into this path.
fn restart_client_process(
    executable_path: &Path,
    project_session: &session::ProjectSession,
) -> Result<(), CliError> {
    let source = ilium_platform::process_control::replace_current_process(
        ProcessCommand::new(executable_path)
            .args(client_restart_args(project_session))
            .current_dir(&project_session.project_root),
    );
    Err(CliError::RestartClient {
        path: executable_path.to_path_buf(),
        source,
    })
}

/// Reconstructs the smallest CLI invocation that reattaches the same project
/// session. Keeping this pure makes the no-server-lifecycle guarantee testable.
fn client_restart_args(project_session: &session::ProjectSession) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--cwd"),
        project_session.project_root.as_os_str().to_os_string(),
    ];
    if project_session.name != session::DEFAULT_SESSION_NAME {
        arguments.extend([
            OsString::from("new-session"),
            OsString::from(&project_session.name),
        ]);
    }
    arguments
}

fn list_sessions(cwd: &Path) -> Result<(), CliError> {
    let sessions = session::list_sessions(cwd)?;
    if sessions.is_empty() {
        println!("no sessions");
        return Ok(());
    }
    for listing in &sessions {
        let status = if listing.live {
            "running"
        } else {
            "not running"
        };
        println!("{:<24} {status}", listing.name);
    }
    Ok(())
}

async fn kill_session(session_name: &str, cwd: &Path) -> Result<(), CliError> {
    let project_session = session::resolve_project_session(cwd, session_name)?;
    if !session::is_session_live(&project_session.socket_path) {
        return Err(CliError::SessionNotRunning(session_name.to_string()));
    }
    initialize_cli_logging(&session::read_active_log_path(&project_session)?)?;
    tracing::info!(session_name, "kill-session action started");

    let mut connection = ilium_client::connection::Connection::connect(
        &project_session.socket_path,
        session_name.to_string(),
    )
    .await?;

    // `Connection::connect` already queued the initial `AttachInteractive`.
    // `handle_attach` replies with `ServerEvent::Error` instead of a
    // `TreeSnapshot` on a session-name mismatch, without closing the
    // connection -- sending the destructive `KillSession` request anyway
    // would tear down whatever session this socket actually serves, which is
    // not necessarily the one this command targets. Wait for that first
    // reply and bail on an `Error` before sending anything destructive; a
    // `TreeSnapshot` or a timed-out wait both mean it's safe to proceed (a
    // timeout only means the reply didn't arrive in time, not that attach
    // failed).
    let initial_attach_reply = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            match event {
                ilium_ipc::ServerEvent::TreeSnapshot(_) => return Ok(()),
                ilium_ipc::ServerEvent::Error { message } => return Err(message),
                _ => {}
            }
        }
        Ok(())
    })
    .await;
    if let Ok(Err(message)) = initial_attach_reply {
        return Err(CliError::ServerReportedError(message));
    }

    connection
        .requests
        .send(ilium_ipc::ClientRequest::KillSession)
        .await
        .map_err(|_send_error| CliError::SessionNotRunning(session_name.to_string()))?;

    // `ilium_server::run`'s `KillSession` path closes every connection on
    // this session (including this one) after broadcasting the final
    // `TreeSnapshot` and a short grace period -- draining events until the
    // channel closes is this CLI's confirmation the shutdown actually
    // completed, not merely that the request was sent. A closed channel
    // (`recv` returning `None`) and a timed-out wait are both treated as
    // "done here" rather than errors: either way there is nothing further
    // this connection can do, and the server process tears down its own
    // socket file regardless. `handle_kill_session` itself is infallible, so
    // an `Error` reaching this drain would only mean some other request on
    // this connection failed -- surface it rather than reporting success.
    let drain_result = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            if let ilium_ipc::ServerEvent::Error { message } = event {
                return Err(message);
            }
        }
        Ok(())
    })
    .await;
    if let Ok(Err(message)) = drain_result {
        return Err(CliError::ServerReportedError(message));
    }

    println!("session {session_name:?} killed");
    Ok(())
}

async fn new_pane(session_name: &str, cmd: &[String], cwd: &Path) -> Result<(), CliError> {
    let project_session = session::resolve_project_session(cwd, session_name)?;
    let log_path = session::ensure_server_running(&project_session).await?;
    initialize_cli_logging(&log_path)?;
    tracing::info!(
        session_name,
        command_argument_count = cmd.len(),
        "new-pane CLI action started"
    );

    let mut connection = ilium_client::connection::Connection::connect(
        &project_session.socket_path,
        session_name.to_string(),
    )
    .await?;

    // `Connection::connect` already queued the initial `Attach`. Its
    // direct-reply `TreeSnapshot` carries the pane count *before* this
    // command's own pane exists; recording it here (skipping over any
    // unrelated `ScreenUpdate`/`PaneStatusChanged` broadcasts that might
    // interleave from other panes/clients already attached to this
    // session) lets the wait below tell "our `NewPane` landed" apart from
    // "some unrelated `TreeSnapshot` broadcast arrived" instead of just
    // trusting the first `TreeSnapshot` it happens to see. `handle_attach`
    // replies with `ServerEvent::Error` instead of a `TreeSnapshot` on a
    // session-name mismatch, so that must short-circuit here too -- otherwise
    // this loop would wait out the full timeout for a snapshot that will
    // never come, then still send `NewPane` against a connection whose
    // attach already failed.
    let baseline_wait = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            match event {
                ilium_ipc::ServerEvent::TreeSnapshot(tree) => {
                    return Ok(Some(tree.panes().count()))
                }
                ilium_ipc::ServerEvent::Error { message } => return Err(message),
                _ => {}
            }
        }
        Ok(None)
    })
    .await;
    let baseline_pane_count = match baseline_wait {
        Ok(Ok(count)) => count,
        Ok(Err(message)) => return Err(CliError::ServerReportedError(message)),
        Err(_elapsed) => None,
    };

    let command_line = shell_join(cmd);
    connection
        .requests
        .send(ilium_ipc::ClientRequest::NewPane {
            parent_group: ilium_core::ROOT_ID,
            kind: ilium_ipc::NewPaneKind::Command(command_line),
            // The non-interactive CLI has no focused client-side terminal
            // or per-client settings, so preserve its established behavior
            // of starting new panes at the project session root.
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        })
        .await
        .map_err(|_send_error| {
            CliError::ServerReportedError("connection closed before NewPane was sent".to_string())
        })?;

    let outcome = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            match event {
                ilium_ipc::ServerEvent::Error { message } => return Some(Err(message)),
                ilium_ipc::ServerEvent::TreeSnapshot(tree) => {
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

    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;

    match outcome {
        Ok(Some(Ok(()))) => {
            println!("pane created in project session {session_name:?}");
            Ok(())
        }
        Ok(Some(Err(message))) => Err(CliError::ServerReportedError(message)),
        Ok(None) | Err(_) => Err(CliError::ServerReportedError(
            "no confirmation received from the server".to_string(),
        )),
    }
}

/// Starts the session-scoped writer before short-lived IPC actions and reuses
/// it when this process hands off to the full client. Reading only the Debug
/// boolean first preserves diagnostics even if an unrelated config table is
/// malformed and the client later falls back to defaults.
fn initialize_cli_logging(log_path: &Path) -> Result<(), CliError> {
    let enabled = ilium_platform::paths::config_dir()
        .and_then(|config_dir| {
            ilium_logging::file_logging_enabled_hint(&config_dir.join("config.toml"))
                .ok()
                .flatten()
        })
        .unwrap_or(false);
    ilium_logging::initialize(log_path, enabled, "cli")?;
    Ok(())
}

/// Joins `new-pane`'s trailing `CMD` arguments into the single shell
/// command string `NewPaneKind::Command` carries over IPC -- the server
/// always runs it as `$SHELL -c <command_line>` (see
/// `ilium-server/src/pane.rs`'s `spawn_terminal_session`) and also echoes it
/// verbatim as the pane's display name (`TerminalOrigin::default_pane_name`).
/// A naive `cmd.join(" ")` loses each argument's boundary: `ilium new-pane
/// -- ls "my folder"` would reconstruct as `ls my folder`, which `$SHELL -c`
/// re-splits into two arguments instead of one. Quoting only the arguments
/// that need it keeps the overwhelmingly common single bare-token case
/// (`claude`, `codex`, `cat`, ...) rendering unquoted.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|argument| shell_quote_if_needed(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

/// True if `argument` is safe to place bare (unquoted) in a POSIX shell
/// command line: non-empty, and made up only of characters that never
/// trigger word-splitting, globbing, expansion, or redirection.
fn is_shell_safe_bare_word(argument: &str) -> bool {
    !argument.is_empty()
        && argument.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'-' | b'.' | b'/' | b'=' | b',' | b':' | b'@' | b'+'
                )
        })
}

/// Quotes `argument` for `$SHELL -c` only if `is_shell_safe_bare_word` says
/// it needs it; otherwise returns it unchanged.
fn shell_quote_if_needed(argument: &str) -> String {
    if is_shell_safe_bare_word(argument) {
        return argument.to_string();
    }
    // Close the current single-quoted segment, emit a backslash-escaped
    // single quote, then reopen single-quoting -- the standard POSIX
    // technique for embedding a literal `'` inside a `'...'` string.
    // Mirrors `ilium-server/src/persistence.rs`'s private `shell_quote`;
    // duplicated here rather than imported per this workspace's crate
    // layering rules (`ilium/CLAUDE.md`).
    format!("'{}'", argument.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{
        chatroom_project_root, client_restart_args, pane_identity_from_env, session, shell_join,
    };

    fn project_session(name: &str) -> session::ProjectSession {
        session::ProjectSession {
            name: name.to_string(),
            project_root: PathBuf::from("/work/project"),
            socket_path: PathBuf::from("/tmp/session.sock"),
            snapshot_path: PathBuf::from("/work/project/.ilium/sessions/session.json"),
            log_directory: PathBuf::from("/tmp/.ilium/work-project-default"),
            active_log_path_file: PathBuf::from(
                "/tmp/.ilium/work-project-default/.active-log-path",
            ),
            server_start_lock_file: PathBuf::from(
                "/tmp/.ilium/work-project-default/.server-start.lock",
            ),
        }
    }

    #[test]
    fn restart_args_reattach_default_without_replaying_lifecycle_flags() {
        let arguments = client_restart_args(&project_session(session::DEFAULT_SESSION_NAME));

        assert_eq!(
            arguments,
            vec![OsString::from("--cwd"), OsString::from("/work/project")]
        );
    }

    #[test]
    fn restart_args_preserve_a_named_session_without_server_flags() {
        let arguments = client_restart_args(&project_session("review"));

        assert_eq!(
            arguments,
            vec![
                OsString::from("--cwd"),
                OsString::from("/work/project"),
                OsString::from("new-session"),
                OsString::from("review"),
            ]
        );
        assert!(!arguments
            .iter()
            .any(|argument| { argument == "--restart-server" || argument == "--reset-session" }));
    }

    #[test]
    fn bare_single_token_commands_round_trip_unquoted() {
        assert_eq!(shell_join(&["cat".to_string()]), "cat");
        assert_eq!(shell_join(&["ls".to_string(), "-la".to_string()]), "ls -la");
    }

    #[test]
    fn arguments_with_spaces_are_quoted_so_they_stay_one_word() {
        assert_eq!(
            shell_join(&["ls".to_string(), "my folder".to_string()]),
            "ls 'my folder'"
        );
    }

    #[test]
    fn embedded_single_quotes_are_escaped() {
        assert_eq!(
            shell_join(&["echo".to_string(), "it's here".to_string()]),
            "echo 'it'\\''s here'"
        );
    }

    #[test]
    fn chatroom_lookup_uses_the_nearest_parent_room() {
        let project = tempfile::tempdir().unwrap();
        let nested = project.path().join("src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        ilium_client::chatroom::initialize(project.path()).unwrap();

        assert_eq!(chatroom_project_root(&nested), project.path());
    }

    /// `ilium progress` only works run from inside a pane ilium itself
    /// spawned (see `pane_env`'s doc comment) -- an ordinary shell, or a
    /// test process, has none of `ILIUM_PANE_ID`/`ILIUM_SESSION_NAME`/
    /// `ILIUM_SESSION_SOCKET` set. Deliberately does not call
    /// `std::env::set_var`/`remove_var` (process-global and racy under
    /// parallel tests) -- this only asserts the common "not set at all"
    /// case, which is already the ambient state of any normal test run.
    #[test]
    fn progress_outside_a_pane_reports_the_missing_env_var() {
        assert!(matches!(
            pane_identity_from_env(),
            Err(super::CliError::NotInsideIliumPane(_))
        ));
    }
}
