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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand, ValueEnum};

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

/// Agent-facing lifecycle operations for one pane's server-owned long-task
/// monitor. Every successful operation prints exactly one JSONL record so an
/// agent can consume the result without scraping prose.
#[derive(Subcommand, Debug)]
enum ProgressCommand {
    /// Runs and validates one probe through the server without installing it.
    /// Stdout must contain exactly one JSON object with `job_id`, `status`,
    /// `percent`, optional `message`, and `error` when status is `error`.
    Check {
        #[arg(long)]
        command: String,
    },
    /// Validates, then atomically starts or replaces this pane's monitor. The
    /// command waits for a correlated server acknowledgement containing the
    /// monitor ID and accepted first report; silence is never acceptance.
    /// Probe stdout follows the same contract as `progress check`.
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
    /// every tick (default every 1 second), for as long as the pane exists.
    /// It must be cheap: prefer O(1) or cached state reads over recursive
    /// filesystem walks, avoid
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
        /// Whether Ilium should leave an active goal alone or safely pause it
        /// after this turn and resume only the causally owned pause.
        #[arg(long, value_enum, default_value_t = ProgressGoalPolicyArgument::KeepRunning)]
        goal_policy: ProgressGoalPolicyArgument,
    },
    /// Returns the current registration, latest report, and monitor health.
    Status,
    /// Arms safe `/goal pause` and causally owned `/goal resume` delivery for
    /// the active monitor after this agent turn finishes.
    ArmGoalResume {
        #[arg(long)]
        monitor_id: u64,
    },
    /// Removes goal pause/resume intent from the specified active monitor.
    DisarmGoalResume {
        #[arg(long)]
        monitor_id: u64,
    },
    /// Stops the active monitor and clears its retained progress. Supplying a
    /// monitor ID fences the operation so a stale agent cannot clear a newer
    /// replacement.
    Clear {
        #[arg(long)]
        monitor_id: Option<u64>,
    },
}

impl ProgressCommand {
    const fn operation_name(&self) -> &'static str {
        match self {
            Self::Check { .. } => "check",
            Self::Set { .. } => "set",
            Self::Status => "status",
            Self::ArmGoalResume { .. } => "arm-goal-resume",
            Self::DisarmGoalResume { .. } => "disarm-goal-resume",
            Self::Clear { .. } => "clear",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProgressGoalPolicyArgument {
    KeepRunning,
    PauseAndResume,
}

impl From<ProgressGoalPolicyArgument> for ilium_ipc::ProgressGoalPolicy {
    fn from(value: ProgressGoalPolicyArgument) -> Self {
        match value {
            ProgressGoalPolicyArgument::KeepRunning => Self::KeepRunning,
            ProgressGoalPolicyArgument::PauseAndResume => Self::PauseAndResume,
        }
    }
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
    pane_identity_from_values(
        std::env::var(ilium_ipc::pane_env::PANE_ID).ok(),
        std::env::var(ilium_ipc::pane_env::SESSION_NAME).ok(),
        std::env::var(ilium_ipc::pane_env::SESSION_SOCKET).ok(),
    )
}

fn pane_identity_from_values(
    pane_id: Option<String>,
    session_name: Option<String>,
    socket_path: Option<String>,
) -> Result<PaneIdentity, CliError> {
    let pane_id = pane_id
        .and_then(|value| value.parse::<u64>().ok())
        .map(ilium_core::NodeId)
        .ok_or(CliError::NotInsideIliumPane(ilium_ipc::pane_env::PANE_ID))?;
    let session_name = session_name.ok_or(CliError::NotInsideIliumPane(
        ilium_ipc::pane_env::SESSION_NAME,
    ))?;
    let socket_path = socket_path
        .map(PathBuf::from)
        .ok_or(CliError::NotInsideIliumPane(
            ilium_ipc::pane_env::SESSION_SOCKET,
        ))?;
    Ok(PaneIdentity {
        pane_id,
        session_name,
        socket_path,
    })
}

/// Includes the server's bounded first-probe execution plus enough local IPC
/// slack to return its correlated result. A quiet socket is never acceptance.
const PROGRESS_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

static NEXT_PROGRESS_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
enum ExpectedProgressResponse {
    Check,
    Set,
    Status,
    ArmGoalResume,
    DisarmGoalResume,
    Clear,
}

enum ProgressResponse {
    Check {
        pane_id: ilium_core::NodeId,
        result: Result<ilium_ipc::ProgressMonitorPreflight, ilium_ipc::ProgressMonitorRejection>,
    },
    Set {
        pane_id: ilium_core::NodeId,
        result: Result<ilium_ipc::ProgressMonitorAccepted, ilium_ipc::ProgressMonitorRejection>,
    },
    Status {
        pane_id: ilium_core::NodeId,
        result: Result<ilium_ipc::ProgressMonitorStatus, ilium_ipc::ProgressMonitorRejection>,
    },
    GoalPolicyChanged {
        operation: &'static str,
        pane_id: ilium_core::NodeId,
        monitor_id: u64,
        result: Result<ilium_ipc::ProgressGoalPolicy, ilium_ipc::ProgressMonitorRejection>,
    },
    Clear {
        pane_id: ilium_core::NodeId,
        result: Result<Option<u64>, ilium_ipc::ProgressMonitorRejection>,
    },
}

async fn progress(command: ProgressCommand) -> Result<(), CliError> {
    let request_id = next_progress_request_id();
    let operation = command.operation_name();
    let identity = match pane_identity_from_env() {
        Ok(identity) => identity,
        Err(error) => {
            print_progress_request_failure(
                operation,
                request_id,
                None,
                "pane-identity-unavailable",
                &error,
            );
            return Err(error);
        }
    };
    let mut connection = match ilium_client::connection::Connection::connect(
        &identity.socket_path,
        identity.session_name.clone(),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => {
            let error = CliError::from(error);
            print_progress_request_failure(
                operation,
                request_id,
                Some(identity.pane_id),
                "connection-failed",
                &error,
            );
            return Err(error);
        }
    };

    let (request, expected_response) = match command {
        ProgressCommand::Check { command } => (
            ilium_ipc::ClientRequest::CheckPaneProgressMonitor {
                request_id,
                pane_id: identity.pane_id,
                command,
            },
            ExpectedProgressResponse::Check,
        ),
        ProgressCommand::Set {
            command,
            interval_seconds,
            goal_policy,
        } => (
            ilium_ipc::ClientRequest::SetPaneProgressMonitor {
                request_id,
                pane_id: identity.pane_id,
                command,
                interval_seconds,
                goal_policy: goal_policy.into(),
            },
            ExpectedProgressResponse::Set,
        ),
        ProgressCommand::Status => (
            ilium_ipc::ClientRequest::GetPaneProgressMonitorStatus {
                request_id,
                pane_id: identity.pane_id,
            },
            ExpectedProgressResponse::Status,
        ),
        ProgressCommand::ArmGoalResume { monitor_id } => (
            ilium_ipc::ClientRequest::ArmProgressGoalResume {
                request_id,
                pane_id: identity.pane_id,
                monitor_id,
            },
            ExpectedProgressResponse::ArmGoalResume,
        ),
        ProgressCommand::DisarmGoalResume { monitor_id } => (
            ilium_ipc::ClientRequest::DisarmProgressGoalResume {
                request_id,
                pane_id: identity.pane_id,
                monitor_id,
            },
            ExpectedProgressResponse::DisarmGoalResume,
        ),
        ProgressCommand::Clear { monitor_id } => (
            ilium_ipc::ClientRequest::ClearPaneProgressMonitor {
                request_id,
                pane_id: identity.pane_id,
                expected_monitor_id: monitor_id,
            },
            ExpectedProgressResponse::Clear,
        ),
    };
    if connection.requests.send(request).await.is_err() {
        let error = CliError::ServerReportedError(
            "connection closed before the request was sent".to_string(),
        );
        print_progress_request_failure(
            operation,
            request_id,
            Some(identity.pane_id),
            "request-send-failed",
            &error,
        );
        return Err(error);
    }

    let response = wait_for_progress_response(&mut connection, request_id, expected_response).await;

    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;

    match response {
        Ok(response) => print_progress_response(request_id, response),
        Err(error) => {
            print_progress_request_failure(
                operation,
                request_id,
                Some(identity.pane_id),
                "transport-error",
                &error,
            );
            Err(error)
        }
    }
}

fn print_progress_request_failure(
    operation: &str,
    request_id: u64,
    pane_id: Option<ilium_core::NodeId>,
    code: &str,
    error: &CliError,
) {
    let pane_id = pane_id.map_or_else(|| "null".to_string(), |pane_id| pane_id.0.to_string());
    println!(
        "{{\"type\":\"progress_request_failed\",\"operation\":{},\"request_id\":{request_id},\"pane_id\":{pane_id},\"code\":{},\"message\":{}}}",
        json_string(operation),
        json_string(code),
        json_string(&error.to_string())
    );
}

fn next_progress_request_id() -> u64 {
    let sequence = NEXT_PROGRESS_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let unix_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    let process_component = u64::from(std::process::id()).rotate_left(32);
    let request_id = unix_nanos ^ process_component ^ sequence.rotate_left(17);
    if request_id == 0 {
        1
    } else {
        request_id
    }
}

async fn wait_for_progress_response(
    connection: &mut ilium_client::connection::Connection,
    request_id: u64,
    expected: ExpectedProgressResponse,
) -> Result<ProgressResponse, CliError> {
    tokio::time::timeout(PROGRESS_REQUEST_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let matched = match (expected, event) {
                (
                    ExpectedProgressResponse::Check,
                    ilium_ipc::ServerEvent::ProgressMonitorCheckCompleted {
                        request_id: response_id,
                        pane_id,
                        result,
                    },
                ) if response_id == request_id => Some(ProgressResponse::Check { pane_id, result }),
                (
                    ExpectedProgressResponse::Set,
                    ilium_ipc::ServerEvent::ProgressMonitorSetCompleted {
                        request_id: response_id,
                        pane_id,
                        result,
                    },
                ) if response_id == request_id => Some(ProgressResponse::Set { pane_id, result }),
                (
                    ExpectedProgressResponse::Status,
                    ilium_ipc::ServerEvent::ProgressMonitorStatusReported {
                        request_id: response_id,
                        pane_id,
                        result,
                    },
                ) if response_id == request_id => {
                    Some(ProgressResponse::Status { pane_id, result })
                }
                (
                    operation @ (ExpectedProgressResponse::ArmGoalResume
                    | ExpectedProgressResponse::DisarmGoalResume),
                    ilium_ipc::ServerEvent::ProgressMonitorGoalPolicyChanged {
                        request_id: response_id,
                        pane_id,
                        monitor_id,
                        result,
                    },
                ) if response_id == request_id => Some(ProgressResponse::GoalPolicyChanged {
                    operation: match operation {
                        ExpectedProgressResponse::ArmGoalResume => "arm-goal-resume",
                        ExpectedProgressResponse::DisarmGoalResume => "disarm-goal-resume",
                        _ => "goal-policy",
                    },
                    pane_id,
                    monitor_id,
                    result,
                }),
                (
                    ExpectedProgressResponse::Clear,
                    ilium_ipc::ServerEvent::ProgressMonitorCleared {
                        request_id: response_id,
                        pane_id,
                        result,
                    },
                ) if response_id == request_id => Some(ProgressResponse::Clear { pane_id, result }),
                _ => None,
            };
            if let Some(response) = matched {
                return Ok(response);
            }
        }
        Err(CliError::ServerReportedError(
            "connection closed before the correlated progress response arrived".to_string(),
        ))
    })
    .await
    .map_err(|_| {
        CliError::ServerReportedError(format!(
            "timed out after {PROGRESS_REQUEST_TIMEOUT:?} waiting for correlated progress response"
        ))
    })?
}

fn print_progress_response(request_id: u64, response: ProgressResponse) -> Result<(), CliError> {
    match response {
        ProgressResponse::Check { pane_id, result } => match result {
            Ok(preflight) => {
                println!(
                    "{{\"type\":\"progress_check\",\"request_id\":{request_id},\"pane_id\":{},\"checked_at_unix_millis\":{},\"report\":{}}}",
                    pane_id.0,
                    preflight.checked_at_unix_millis,
                    progress_report_json(&preflight.report)
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(request_id, pane_id, "check", rejection),
        },
        ProgressResponse::Set { pane_id, result } => match result {
            Ok(accepted) => {
                println!(
                    "{{\"type\":\"progress_set\",\"request_id\":{request_id},\"pane_id\":{},\"monitor_id\":{},\"goal_policy\":{},\"progress\":{}}}",
                    pane_id.0,
                    accepted.monitor_id,
                    json_string(progress_goal_policy_name(accepted.goal_policy)),
                    pane_progress_json(&accepted.progress)
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(request_id, pane_id, "set", rejection),
        },
        ProgressResponse::Status { pane_id, result } => match result {
            Ok(status) => {
                let progress = status
                    .progress
                    .as_ref()
                    .map_or_else(|| "null".to_string(), pane_progress_json);
                let goal_policy = status.goal_policy.map_or_else(
                    || "null".to_string(),
                    |policy| json_string(progress_goal_policy_name(policy)),
                );
                println!(
                    "{{\"type\":\"progress_status\",\"request_id\":{request_id},\"pane_id\":{},\"active\":{},\"progress\":{progress},\"goal_policy\":{goal_policy},\"goal_resume_armed\":{}}}",
                    pane_id.0,
                    status.progress.is_some(),
                    status.goal_resume_armed
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(request_id, pane_id, "status", rejection),
        },
        ProgressResponse::GoalPolicyChanged {
            operation,
            pane_id,
            monitor_id,
            result,
        } => match result {
            Ok(goal_policy) => {
                println!(
                    "{{\"type\":\"progress_goal_policy_changed\",\"operation\":{operation_json},\"request_id\":{request_id},\"pane_id\":{},\"monitor_id\":{monitor_id},\"goal_policy\":{}}}",
                    pane_id.0,
                    json_string(progress_goal_policy_name(goal_policy)),
                    operation_json = json_string(operation)
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(request_id, pane_id, operation, rejection),
        },
        ProgressResponse::Clear { pane_id, result } => match result {
            Ok(cleared_monitor_id) => {
                let cleared_id = cleared_monitor_id
                    .map_or_else(|| "null".to_string(), |monitor_id| monitor_id.to_string());
                println!(
                    "{{\"type\":\"progress_clear\",\"request_id\":{request_id},\"pane_id\":{},\"cleared\":{},\"cleared_monitor_id\":{cleared_id}}}",
                    pane_id.0,
                    cleared_monitor_id.is_some()
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(request_id, pane_id, "clear", rejection),
        },
    }
}

fn print_progress_rejection(
    request_id: u64,
    pane_id: ilium_core::NodeId,
    operation: &str,
    rejection: ilium_ipc::ProgressMonitorRejection,
) -> Result<(), CliError> {
    println!(
        "{{\"type\":\"progress_rejected\",\"operation\":{},\"request_id\":{request_id},\"pane_id\":{},\"code\":{},\"message\":{}}}",
        json_string(operation),
        pane_id.0,
        json_string(progress_rejection_code_name(rejection.code)),
        json_string(&rejection.message)
    );
    Err(CliError::ServerReportedError(rejection.message))
}

fn pane_progress_json(progress: &ilium_core::PaneProgress) -> String {
    format!(
        "{{\"monitor_id\":{},\"report\":{},\"monitor_health\":{},\"last_observed_unix_millis\":{}}}",
        progress.monitor_id,
        progress_report_json(&progress.report),
        progress_monitor_health_json(&progress.monitor_health),
        progress.last_observed_unix_millis
    )
}

fn progress_report_json(report: &ilium_core::ProgressTaskReport) -> String {
    let error = report
        .error
        .as_deref()
        .map_or_else(|| "null".to_string(), json_string);
    format!(
        "{{\"job_id\":{},\"status\":{},\"percent\":{},\"message\":{},\"error\":{error}}}",
        json_string(&report.job_id),
        json_string(progress_task_status_name(report.status)),
        report.percent,
        json_string(&report.message)
    )
}

fn progress_monitor_health_json(health: &ilium_core::ProgressMonitorHealth) -> String {
    match health {
        ilium_core::ProgressMonitorHealth::Healthy => "{\"state\":\"healthy\"}".to_string(),
        ilium_core::ProgressMonitorHealth::Degraded {
            consecutive_failures,
            last_error,
        } => format!(
            "{{\"state\":\"degraded\",\"consecutive_failures\":{consecutive_failures},\"last_error\":{}}}",
            json_string(last_error)
        ),
        ilium_core::ProgressMonitorHealth::Failed {
            consecutive_failures,
            last_error,
        } => format!(
            "{{\"state\":\"failed\",\"consecutive_failures\":{consecutive_failures},\"last_error\":{}}}",
            json_string(last_error)
        ),
    }
}

const fn progress_task_status_name(status: ilium_core::ProgressTaskStatus) -> &'static str {
    match status {
        ilium_core::ProgressTaskStatus::NotStartedYet => "not-started-yet",
        ilium_core::ProgressTaskStatus::Running => "running",
        ilium_core::ProgressTaskStatus::Error => "error",
        ilium_core::ProgressTaskStatus::Done => "done",
    }
}

const fn progress_goal_policy_name(policy: ilium_ipc::ProgressGoalPolicy) -> &'static str {
    match policy {
        ilium_ipc::ProgressGoalPolicy::KeepRunning => "keep-running",
        ilium_ipc::ProgressGoalPolicy::PauseAndResume => "pause-and-resume",
    }
}

const fn progress_rejection_code_name(
    code: ilium_ipc::ProgressMonitorRejectionCode,
) -> &'static str {
    match code {
        ilium_ipc::ProgressMonitorRejectionCode::Disabled => "disabled",
        ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest => "invalid-request",
        ilium_ipc::ProgressMonitorRejectionCode::InvalidProbeReport => "invalid-probe-report",
        ilium_ipc::ProgressMonitorRejectionCode::ProbeSpawnFailed => "probe-spawn-failed",
        ilium_ipc::ProgressMonitorRejectionCode::ProbeTimedOut => "probe-timed-out",
        ilium_ipc::ProgressMonitorRejectionCode::ProbeExitedNonZero => "probe-exited-non-zero",
        ilium_ipc::ProgressMonitorRejectionCode::ProbeOutputTooLarge => "probe-output-too-large",
        ilium_ipc::ProgressMonitorRejectionCode::ProbeIoFailed => "probe-io-failed",
        ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound => "pane-not-found",
        ilium_ipc::ProgressMonitorRejectionCode::StaleMonitor => "stale-monitor",
        ilium_ipc::ProgressMonitorRejectionCode::GoalOwnershipUnavailable => {
            "goal-ownership-unavailable"
        }
    }
}

fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{08}' => encoded.push_str("\\b"),
            '\u{0c}' => encoded.push_str("\\f"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(encoded, "\\u{:04x}", u32::from(character));
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
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
                    return Ok(Some(tree.panes().count()));
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
        chatroom_project_root, client_restart_args, json_string, pane_identity_from_values,
        pane_progress_json, progress_report_json, session, shell_join, Cli, Command,
        ProgressCommand,
    };
    use clap::Parser;

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
    /// spawned. Test the pure environment-value boundary instead of assuming
    /// the test runner itself is outside Ilium or mutating process-global env.
    #[test]
    fn progress_outside_a_pane_reports_the_missing_env_var() {
        assert!(matches!(
            pane_identity_from_values(None, None, None),
            Err(super::CliError::NotInsideIliumPane(_))
        ));
    }

    #[test]
    fn progress_check_parses_probe_command_without_installing_interval() {
        let cli = Cli::try_parse_from([
            "ilium",
            "progress",
            "check",
            "--command",
            "/work/status --json",
        ])
        .unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Progress {
                command: ProgressCommand::Check { command }
            }) if command == "/work/status --json"
        ));
    }

    #[test]
    fn progress_set_defaults_to_one_second_interval() {
        let cli = Cli::try_parse_from([
            "ilium",
            "progress",
            "set",
            "--command",
            "/work/status --json",
        ])
        .unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Progress {
                command: ProgressCommand::Set {
                    command,
                    interval_seconds: 1,
                    goal_policy: super::ProgressGoalPolicyArgument::KeepRunning,
                }
            }) if command == "/work/status --json"
        ));
    }

    #[test]
    fn progress_set_accepts_pause_and_resume_goal_policy() {
        let cli = Cli::try_parse_from([
            "ilium",
            "progress",
            "set",
            "--command",
            "/work/status --json",
            "--goal-policy",
            "pause-and-resume",
        ])
        .unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Progress {
                command: ProgressCommand::Set {
                    goal_policy: super::ProgressGoalPolicyArgument::PauseAndResume,
                    ..
                }
            })
        ));
    }

    #[test]
    fn progress_lifecycle_commands_require_explicit_monitor_ids() {
        for operation in ["arm-goal-resume", "disarm-goal-resume"] {
            assert!(Cli::try_parse_from(["ilium", "progress", operation]).is_err());

            let cli = Cli::try_parse_from(["ilium", "progress", operation, "--monitor-id", "42"])
                .unwrap();

            assert!(matches!(
                cli.command,
                Some(Command::Progress {
                    command: ProgressCommand::ArmGoalResume { monitor_id: 42 }
                        | ProgressCommand::DisarmGoalResume { monitor_id: 42 }
                })
            ));
        }
    }

    #[test]
    fn progress_clear_accepts_an_optional_monitor_fence() {
        let unfenced = Cli::try_parse_from(["ilium", "progress", "clear"]).unwrap();
        assert!(matches!(
            unfenced.command,
            Some(Command::Progress {
                command: ProgressCommand::Clear { monitor_id: None }
            })
        ));

        let fenced =
            Cli::try_parse_from(["ilium", "progress", "clear", "--monitor-id", "99"]).unwrap();
        assert!(matches!(
            fenced.command,
            Some(Command::Progress {
                command: ProgressCommand::Clear {
                    monitor_id: Some(99)
                }
            })
        ));
    }

    #[test]
    fn progress_status_rejects_unexpected_arguments() {
        assert!(Cli::try_parse_from(["ilium", "progress", "status"]).is_ok());
        assert!(Cli::try_parse_from(["ilium", "progress", "status", "extra"]).is_err());
    }

    #[test]
    fn progress_json_strings_escape_control_characters_without_breaking_jsonl() {
        let source = "quote \" slash \\ newline\n tab\t nul\0 snowman ☃";
        let encoded = json_string(source);

        assert!(!encoded.contains('\n'));
        assert_eq!(serde_json::from_str::<String>(&encoded).unwrap(), source);
    }

    #[test]
    fn progress_report_output_preserves_the_complete_probe_contract() {
        let report = ilium_core::ProgressTaskReport::new(
            "render-42".to_string(),
            ilium_core::ProgressTaskStatus::Error,
            73.5,
            "encoder stopped after frame 735".to_string(),
            Some("exit status 9: bad \"frame\"".to_string()),
        )
        .unwrap();

        let output: serde_json::Value =
            serde_json::from_str(&progress_report_json(&report)).unwrap();
        assert_eq!(output["job_id"], "render-42");
        assert_eq!(output["status"], "error");
        assert_eq!(output["percent"], 73.5);
        assert_eq!(output["message"], "encoder stopped after frame 735");
        assert_eq!(output["error"], "exit status 9: bad \"frame\"");
    }

    #[test]
    fn pane_progress_output_keeps_monitor_health_separate_from_task_status() {
        let report = ilium_core::ProgressTaskReport::new(
            "render-42".to_string(),
            ilium_core::ProgressTaskStatus::Running,
            73.5,
            "frame 735/1000".to_string(),
            None,
        )
        .unwrap();
        let mut progress = ilium_core::PaneProgress::new(91, report, 1_700_000_000_000).unwrap();
        progress.monitor_health = ilium_core::ProgressMonitorHealth::Degraded {
            consecutive_failures: 2,
            last_error: "probe timed out".to_string(),
        };

        let output: serde_json::Value =
            serde_json::from_str(&pane_progress_json(&progress)).unwrap();
        assert_eq!(output["monitor_id"], 91);
        assert_eq!(output["report"]["status"], "running");
        assert_eq!(output["monitor_health"]["state"], "degraded");
        assert_eq!(output["monitor_health"]["consecutive_failures"], 2);
        assert_eq!(output["monitor_health"]["last_error"], "probe timed out");
    }
}
