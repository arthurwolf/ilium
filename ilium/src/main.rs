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

mod progress_wait;
mod voice;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};

use ilium::error::CliError;
use ilium_core::BuiltinAgentProvider;
use ilium_ipc::{ClientRequest, RepoFacts, ServerEvent, WorkspaceCreateSpec, WorkspaceCreateStage};
use ilium_platform::paths;

/// How long the `new-pane`/`kill-session` one-shot subcommands wait for
/// the server to confirm a request before giving up and reporting failure.
const REQUEST_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(5);
const WORKSPACE_FACTS_TIMEOUT: Duration = Duration::from_secs(30);
const WORKSPACE_CREATION_TIMEOUT: Duration = Duration::from_secs(180);

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

    /// Open the guided setup even when this installation is already configured.
    #[arg(long, global = true)]
    onboarding: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Internal native clipboard owner; never opens a session or runtime.
    #[command(hide = true)]
    ClipboardHelper,
    /// Internal Antigravity status-line capture; never opens a session or runtime.
    #[command(name = "__antigravity-model-statusline", hide = true)]
    AntigravityModelStatusline,
    /// Internal Claude status-line model capture; never opens a session or runtime.
    #[command(name = "__claude-model-statusline", hide = true)]
    ClaudeModelStatusline,
    /// Internal offline release qualification; opens no terminal or session.
    #[command(hide = true)]
    ReleaseEmbeddingProbe {
        #[arg(long)]
        model_directory: PathBuf,
        #[arg(long)]
        text: String,
        #[arg(long)]
        hold_for_native_audit: bool,
    },
    /// Internal installed animation qualification through this shipped binary.
    #[command(hide = true)]
    ReleaseAnimationProbe,
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
        /// Start a built-in agent in a new Git worktree on its own branch.
        #[arg(long, requires = "branch")]
        worktree: bool,
        /// New branch for --worktree. Must not already exist.
        #[arg(long, requires = "worktree")]
        branch: Option<String>,
        /// Starting ref for --worktree; defaults to the repository's default base.
        #[arg(long, requires = "worktree")]
        base: Option<String>,
        /// Keep the pane open after the command exits. By default a
        /// `new-pane` command pane closes itself when its command ends.
        #[arg(long, conflicts_with = "worktree")]
        keep_open: bool,
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
    /// Same as `ilium progress wait`: blocks until this pane's progress
    /// monitor (or MONITOR_ID) settles, then prints one JSONL line.
    Wait {
        monitor_id: Option<u64>,
        #[arg(long)]
        timeout_seconds: Option<u64>,
    },
    /// Voice control from the command line: `voice say` types sentences into
    /// the running voice session as if they had been spoken. Output is JSONL.
    Voice {
        #[command(subcommand)]
        command: voice::VoiceCommand,
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
        /// Print only records this reader has not seen yet (nothing when none
        /// are new). The reader is `--reader`, else `ILIUM_PANE_ID`, else the
        /// hook's `session_id` read from standard input.
        #[arg(long)]
        since_last_read: bool,
        /// Cap the printed context at this many bytes; newer records win.
        #[arg(long)]
        max_bytes: Option<usize>,
        /// Explicit reader identity for `--since-last-read`.
        #[arg(long)]
        reader: Option<String>,
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
        /// After registering, block until the task settles, exactly like
        /// `ilium progress wait` (prints a second JSONL line and exits with
        /// the wait's status). The usual way to run a long task.
        #[arg(long)]
        wait: bool,
        /// With --wait: give up after this many seconds (exit 6, task still
        /// running). Omit to wait as long as the task takes.
        #[arg(long, requires = "wait")]
        timeout_seconds: Option<u64>,
        /// Replace a monitor that is still running. Without this, `set`
        /// refuses so a second `set` cannot silently discard a running job's
        /// monitor and its notification.
        #[arg(long)]
        replace: bool,
    },
    /// Blocks until this pane's monitor (or MONITOR_ID) reports done or
    /// error, or the monitor fails, is replaced, or is cleared. Prints one
    /// JSONL line and exits 0 done, 3 task error, 4 monitor failed (outcome
    /// unknown), 5 replaced/cleared/no monitor, 6 timeout. While this command
    /// is waiting, the result is returned here instead of being typed into
    /// the agent's prompt.
    Wait {
        /// Monitor to wait for; defaults to the pane's current monitor.
        monitor_id: Option<u64>,
        /// Give up after this many seconds (exit 6). Omit to wait as long as
        /// the task takes.
        #[arg(long)]
        timeout_seconds: Option<u64>,
    },
    /// Returns the current registration, latest report, and monitor health.
    Status,
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
            Self::Wait { .. } => "wait",
            Self::Status => "status",
            Self::Clear { .. } => "clear",
        }
    }
}

fn main() -> ExitCode {
    // A Flatpak package is a distribution wrapper for the trusted host CLI.
    // This runs before parsing, quota registration, threads, or session state.
    match ilium_platform::flatpak_host::maybe_handoff() {
        Ok(Some(status)) => {
            return status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .map(ExitCode::from)
                .unwrap_or(ExitCode::FAILURE);
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("ilium: Flatpak host handoff: {error}");
            return ExitCode::FAILURE;
        }
    }
    let cli = Cli::parse();
    if matches!(&cli.command, Some(Command::ClipboardHelper)) {
        return match ilium_client::terminal_clipboard::run_helper() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("ilium clipboard helper: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if matches!(&cli.command, Some(Command::AntigravityModelStatusline)) {
        return match ilium_client::antigravity_model_statusline::run_statusline_helper() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("ilium Antigravity model capture: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if matches!(&cli.command, Some(Command::ClaudeModelStatusline)) {
        return match ilium_client::claude_model_statusline::run_statusline_helper() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("ilium Claude model capture: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if let Err(error) = ilium_client::bootstrap_process_quota() {
        eprintln!("ilium: process resource startup: {error}");
        return ExitCode::FAILURE;
    }
    const RUNTIME_WORKER_THREADS: usize = 2;
    const RUNTIME_MAX_BLOCKING_THREADS: usize = 4;
    const RUNTIME_STACK_BYTES: usize = 2 * 1024 * 1024;
    if let Err(error) = ilium_client::bootstrap_runtime_admission(
        RUNTIME_WORKER_THREADS + RUNTIME_MAX_BLOCKING_THREADS,
        RUNTIME_STACK_BYTES,
    ) {
        eprintln!("ilium: runtime admission: {error}");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(RUNTIME_WORKER_THREADS)
        .max_blocking_threads(RUNTIME_MAX_BLOCKING_THREADS)
        .thread_stack_size(RUNTIME_STACK_BYTES)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ilium: runtime startup: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run_main(cli))
}

async fn run_main(cli: Cli) -> ExitCode {
    let outcome = dispatch(cli).await;
    if let Some(error) = outcome
        .as_ref()
        .err()
        .filter(|error| !matches!(error, CliError::ExitStatus(_)))
    {
        tracing::error!(%error, error_debug = ?error, "ilium CLI action failed");
        eprintln!("ilium: {error}");
    }
    // The final CLI event must precede the ordered drain. Errors after this
    // point go to stderr, because the process logging owner is being closed.
    let logging = process_logging_barrier(true).await;
    if let Err(error) = &logging {
        eprintln!("ilium: final logging drain failed: {error}");
    }
    if let Err(CliError::ExitStatus(code)) = &outcome {
        return ExitCode::from(*code);
    }
    if outcome.is_ok() && logging.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Process-boundary adapter, never an interactive-loop wait. Admission and
/// completion share one deadline; timeout reports unknown completion honestly.
async fn process_logging_barrier(shutdown: bool) -> Result<(), ilium_logging::LoggingError> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let receipt = if shutdown {
                ilium_logging::request_shutdown()
            } else {
                ilium_logging::request_flush()
            };
            match receipt {
                Ok(receipt) => return receipt.await,
                Err(ilium_logging::LoggingError::NotInitialized) => return Ok(()),
                Err(ilium_logging::LoggingError::AdmissionBusy) => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| ilium_logging::LoggingError::Deadline)?
}

async fn dispatch(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Some(Command::ClipboardHelper) => Err(CliError::ServerReportedError(
            "Clipboard helper must run before runtime startup".into(),
        )),
        Some(Command::AntigravityModelStatusline) => Err(CliError::ServerReportedError(
            "Antigravity model capture must run before runtime startup".into(),
        )),
        Some(Command::ClaudeModelStatusline) => Err(CliError::ServerReportedError(
            "Claude model capture must run before runtime startup".into(),
        )),
        Some(Command::ReleaseEmbeddingProbe {
            model_directory,
            text,
            hold_for_native_audit,
        }) => {
            ilium_client::release_embedding::probe(&model_directory, &text, hold_for_native_audit)
                .map_err(|error| {
                    CliError::ServerReportedError(format!("release embedding probe: {error:#}"))
                })
        }
        Some(Command::ReleaseAnimationProbe) => ilium_client::release_animation::probe()
            .await
            .map_err(|error| {
                CliError::ServerReportedError(format!("release animation probe: {error:#}"))
            }),
        None => {
            attach_or_create(
                session::DEFAULT_SESSION_NAME,
                &cli.cwd,
                cli.restart_server,
                cli.reset_session,
                cli.onboarding,
            )
            .await
        }
        Some(Command::NewSession { name }) => {
            attach_or_create(
                &name,
                &cli.cwd,
                cli.restart_server,
                cli.reset_session,
                cli.onboarding,
            )
            .await
        }
        Some(Command::Ls) => list_sessions(&cli.cwd),
        Some(Command::KillSession { name }) => kill_session(&name, &cli.cwd).await,
        Some(Command::NewPane {
            session_name,
            worktree,
            branch,
            base,
            keep_open,
            cmd,
        }) => match (worktree, branch) {
            (true, Some(branch)) => {
                new_workspace_pane(&session_name, &cmd, &cli.cwd, &branch, base.as_deref()).await
            }
            _ => new_pane(&session_name, &cmd, &cli.cwd, keep_open).await,
        },
        Some(Command::Chat { command }) => chat(command, &cli.cwd),
        Some(Command::Progress { command }) => progress(command).await,
        Some(Command::Wait {
            monitor_id,
            timeout_seconds,
        }) => {
            progress(ProgressCommand::Wait {
                monitor_id,
                timeout_seconds,
            })
            .await
        }
        Some(Command::Voice { command }) => voice::voice(command, &cli.cwd).await,
    }
}

/// The parts of an agent lifecycle hook's JSON standard input that chat
/// context uses. Both Claude Code and Codex send `session_id` and
/// `hook_event_name`.
#[derive(Default)]
struct HookInput {
    session_id: Option<String>,
    is_session_start: bool,
}

/// Bound on hook standard input; real hook payloads are a few kilobytes.
const HOOK_INPUT_LIMIT_BYTES: u64 = 1024 * 1024;
/// A hook closes standard input right after writing it. Anything else (an
/// interactive shell, an inherited open pipe) must not stall the command.
const HOOK_INPUT_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

fn read_hook_input() -> HookInput {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return HookInput::default();
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    // Detached on purpose: if standard input never reaches end of file, the
    // reader stays blocked until this short-lived CLI process exits.
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = std::io::stdin()
            .take(HOOK_INPUT_LIMIT_BYTES)
            .read_to_end(&mut bytes);
        let _ = sender.send(result.map(|_| bytes));
    });
    let Ok(Ok(bytes)) = receiver.recv_timeout(HOOK_INPUT_WAIT) else {
        return HookInput::default();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return HookInput::default();
    };
    HookInput {
        session_id: value
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        is_session_start: value
            .get("hook_event_name")
            .and_then(serde_json::Value::as_str)
            == Some("SessionStart"),
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
        ChatCommand::Context {
            limit,
            since_last_read,
            max_bytes,
            reader,
        } => {
            let project_root = chatroom_project_root(&cwd);
            let hook_input = if since_last_read {
                read_hook_input()
            } else {
                HookInput::default()
            };
            let reader_key = reader
                .or_else(|| {
                    std::env::var("ILIUM_PANE_ID")
                        .ok()
                        .filter(|key| !key.is_empty())
                })
                .or(hook_input.session_id);
            let max_bytes = max_bytes.unwrap_or(usize::MAX);
            let output = match reader_key.as_deref() {
                Some(key) if since_last_read => ilium_client::chatroom::unread_context(
                    &project_root,
                    limit,
                    max_bytes,
                    &ilium_client::chatroom::UnreadReader {
                        key,
                        is_session_start: hook_input.is_session_start,
                    },
                )?,
                _ => ilium_client::chatroom::capped_context(&project_root, limit, max_bytes)?,
            };
            if !output.is_empty() {
                println!("{output}");
            }
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

    // Pending `--wait` after a successful `set`, as its timeout.
    let mut wait_after_set: Option<Option<u64>> = None;
    let (request, expected_response) = match command {
        ProgressCommand::Wait {
            monitor_id,
            timeout_seconds,
        } => {
            let result = progress_wait::wait_for_monitor(
                &mut connection,
                identity.pane_id,
                monitor_id,
                timeout_seconds.map(Duration::from_secs),
            )
            .await;
            return finish_progress_wait(connection, identity.pane_id, request_id, result).await;
        }
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
            wait,
            timeout_seconds,
            replace,
        } => {
            if !replace {
                refuse_replacing_running_monitor(&mut connection, identity.pane_id, request_id)
                    .await?;
            }
            if wait {
                wait_after_set = Some(timeout_seconds);
            }
            (
                ilium_ipc::ClientRequest::SetPaneProgressMonitor {
                    request_id,
                    pane_id: identity.pane_id,
                    command,
                    interval_seconds,
                },
                ExpectedProgressResponse::Set,
            )
        }
        ProgressCommand::Status => (
            ilium_ipc::ClientRequest::GetPaneProgressMonitorStatus {
                request_id,
                pane_id: identity.pane_id,
            },
            ExpectedProgressResponse::Status,
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

    let accepted_monitor_id = match response.as_ref().map(|response| response.view()) {
        Ok(ProgressResponse::Set {
            result: Ok(accepted),
            ..
        }) => Some(accepted.monitor_id),
        _ => None,
    };
    let response = match (wait_after_set, accepted_monitor_id, response) {
        (Some(timeout_seconds), Some(monitor_id), Ok(response)) => {
            print_progress_response(request_id, response)?;
            let result = progress_wait::wait_for_monitor(
                &mut connection,
                identity.pane_id,
                Some(monitor_id),
                timeout_seconds.map(Duration::from_secs),
            )
            .await;
            return finish_progress_wait(connection, identity.pane_id, request_id, result).await;
        }
        (_, _, response) => response,
    };

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

/// Prints a finished wait and turns its outcome into the process exit
/// status; a transport failure prints the usual failure record instead.
async fn finish_progress_wait(
    connection: ilium_client::connection::Connection,
    pane_id: ilium_core::NodeId,
    request_id: u64,
    result: Result<progress_wait::WaitReport, CliError>,
) -> Result<(), CliError> {
    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;
    match result {
        Ok(report) => {
            report.print();
            match report.exit_code() {
                0 => Ok(()),
                code => Err(CliError::ExitStatus(code)),
            }
        }
        Err(error) => {
            print_progress_request_failure(
                "wait",
                request_id,
                Some(pane_id),
                "wait-failed",
                &error,
            );
            Err(error)
        }
    }
}

/// `set` without `--replace` must not discard a monitor whose task is still
/// running: that silently loses the earlier job's notification.
async fn refuse_replacing_running_monitor(
    connection: &mut ilium_client::connection::Connection,
    pane_id: ilium_core::NodeId,
    request_id: u64,
) -> Result<(), CliError> {
    let current = match progress_wait::current_monitor(connection, pane_id).await {
        Ok(current) => current,
        Err(error) => {
            print_progress_request_failure(
                "set",
                request_id,
                Some(pane_id),
                "status-failed",
                &error,
            );
            return Err(error);
        }
    };
    let Some(current) = current else {
        return Ok(());
    };
    if current.is_terminal() || current.monitor_health.is_failed() {
        return Ok(());
    }
    let message = format!(
        "pane already has running monitor {} (job {}, {:.0}%). Wait for it with `ilium progress \
         wait {}`, use one probe that covers the whole pipeline, or pass --replace to discard it",
        current.monitor_id, current.report.job_id, current.report.percent, current.monitor_id
    );
    println!(
        "{{\"type\":\"progress_rejected\",\"operation\":\"set\",\"request_id\":{request_id},\"pane_id\":{},\"code\":\"monitor-active\",\"active_monitor_id\":{},\"message\":{}}}",
        pane_id.0,
        current.monitor_id,
        json_string(&message)
    );
    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;
    Err(CliError::ServerReportedError(message))
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
) -> Result<ilium_client::connection::Received<ProgressResponse>, CliError> {
    tokio::time::timeout(PROGRESS_REQUEST_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
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
                return Ok(ilium_client::connection::Received::with_retention(
                    response,
                    _event_retention,
                ));
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

fn print_progress_response(
    request_id: u64,
    response: ilium_client::connection::Received<ProgressResponse>,
) -> Result<(), CliError> {
    let (response, _retention) = response.into_parts();
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
            Err(rejection) => print_progress_rejection(
                request_id,
                pane_id,
                "check",
                ilium_client::connection::Received::with_retention(rejection, _retention),
            ),
        },
        ProgressResponse::Set { pane_id, result } => match result {
            Ok(accepted) => {
                println!(
                    "{{\"type\":\"progress_set\",\"request_id\":{request_id},\"pane_id\":{},\"monitor_id\":{},\"progress\":{},\"recovery_command\":\"ilium progress status\"}}",
                    pane_id.0,
                    accepted.monitor_id,
                    pane_progress_json(&accepted.progress)
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(
                request_id,
                pane_id,
                "set",
                ilium_client::connection::Received::with_retention(rejection, _retention),
            ),
        },
        ProgressResponse::Status { pane_id, result } => match result {
            Ok(status) => {
                let progress = status
                    .progress
                    .as_ref()
                    .map_or_else(|| "null".to_string(), pane_progress_json);
                println!(
                    "{{\"type\":\"progress_status\",\"request_id\":{request_id},\"pane_id\":{},\"active\":{},\"progress\":{progress}}}",
                    pane_id.0,
                    status.progress.is_some()
                );
                Ok(())
            }
            Err(rejection) => print_progress_rejection(
                request_id,
                pane_id,
                "status",
                ilium_client::connection::Received::with_retention(rejection, _retention),
            ),
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
            Err(rejection) => print_progress_rejection(
                request_id,
                pane_id,
                "clear",
                ilium_client::connection::Received::with_retention(rejection, _retention),
            ),
        },
    }
}

fn print_progress_rejection(
    request_id: u64,
    pane_id: ilium_core::NodeId,
    operation: &str,
    rejection: ilium_client::connection::Received<ilium_ipc::ProgressMonitorRejection>,
) -> Result<(), CliError> {
    let (rejection, retention) = rejection.into_parts();
    println!(
        "{{\"type\":\"progress_rejected\",\"operation\":{},\"request_id\":{request_id},\"pane_id\":{},\"code\":{},\"message\":{}}}",
        json_string(operation),
        pane_id.0,
        json_string(progress_rejection_code_name(rejection.code)),
        json_string(&rejection.message)
    );
    Err(CliError::received_server_error(
        rejection.message,
        retention,
    ))
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
        "{{\"job_id\":{},\"status\":{},\"percent\":{},\"message\":{},\"details\":{},\"error\":{error}}}",
        json_string(&report.job_id),
        json_string(progress_task_status_name(report.status)),
        report.percent,
        json_string(&report.message),
        json_string(&report.details)
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
    onboarding: bool,
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
        onboarding,
    })
    .await?;
    match exit_reason {
        ilium_client::ClientExitReason::Quit => Ok(()),
        ilium_client::ClientExitReason::RestartRequested => {
            restart_client_process(&client_executable, &project_session).await
        }
    }
}

/// Replaces this client process with the executable captured before the TUI
/// started. The reconstructed invocation carries only project/session identity,
/// so `--restart-server` and `--reset-session` can never leak into this path.
async fn restart_client_process(
    executable_path: &Path,
    project_session: &session::ProjectSession,
) -> Result<(), CliError> {
    tracing::info!("client process replacement requested");
    process_logging_barrier(false).await?;
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
            let (event, _event_retention) = event.into_parts();
            match event {
                ilium_ipc::ServerEvent::TreeSnapshot(_) => return Ok(()),
                ilium_ipc::ServerEvent::Error { message } => {
                    return Err(CliError::received_server_error(message, _event_retention));
                }
                _ => {}
            }
        }
        Ok(())
    })
    .await;
    if let Ok(Err(message)) = initial_attach_reply {
        return Err(message);
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
            let (event, _event_retention) = event.into_parts();
            if let ilium_ipc::ServerEvent::Error { message } = event {
                return Err(CliError::received_server_error(message, _event_retention));
            }
        }
        Ok(())
    })
    .await;
    if let Ok(Err(message)) = drain_result {
        return Err(message);
    }

    println!("session {session_name:?} killed");
    Ok(())
}

/// Worktree creation is an agent-facing one-shot operation. The final event
/// must carry this request ID: a tree broadcast can describe another client's
/// pane, and a send acknowledgement cannot prove that Git or startup worked.
async fn new_workspace_pane(
    session_name: &str,
    cmd: &[String],
    cwd: &Path,
    branch: &str,
    base: Option<&str>,
) -> Result<(), CliError> {
    let request_id = next_progress_request_id();
    let result = run_new_workspace_pane(session_name, cmd, cwd, branch, base, request_id).await;
    if let Err(error) = &result {
        println!(
            "{{\"type\":\"error\",\"request_id\":{request_id},\"message\":{}}}",
            json_string(&error.to_string())
        );
    }
    result
}

async fn run_new_workspace_pane(
    session_name: &str,
    cmd: &[String],
    cwd: &Path,
    branch: &str,
    base: Option<&str>,
    request_id: u64,
) -> Result<(), CliError> {
    let provider = workspace_provider(cmd)?;
    ilium_core::validate_branch_name(branch).map_err(|error| {
        CliError::ServerReportedError(format!("invalid worktree branch {branch:?}: {error}"))
    })?;
    // Snapshot the same persisted Git setting as the TUI before starting the
    // detached server. A malformed config must not silently skip setup.
    let setup_command = ilium_platform::paths::config_dir()
        .map(|directory| {
            ilium_client::config::load(&directory).map(|config| config.git.setup_command)
        })
        .transpose()?
        .unwrap_or_default();
    let project_session = session::resolve_project_session(cwd, session_name)?;
    let log_path = session::ensure_server_running(&project_session).await?;
    initialize_cli_logging(&log_path)?;

    let mut connection = ilium_client::connection::Connection::connect(
        &project_session.socket_path,
        session_name.to_string(),
    )
    .await?;
    let attach = tokio::time::timeout(REQUEST_CONFIRMATION_TIMEOUT, async {
        let mut initial_tree = None;
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::PaneStateSnapshot { tree, .. } => {
                    initial_tree = Some(ilium_client::connection::Received::with_retention(
                        tree,
                        _event_retention,
                    ))
                }
                ServerEvent::TreeSnapshot(tree) if initial_tree.is_none() => {
                    initial_tree = Some(ilium_client::connection::Received::with_retention(
                        tree,
                        _event_retention,
                    ))
                }
                ServerEvent::InitialStateSyncComplete => {
                    return initial_tree.ok_or_else(|| {
                        CliError::ServerReportedError(
                            "session attach completed without a tree".into(),
                        )
                    });
                }
                ServerEvent::Error { message } => {
                    return Err(CliError::received_server_error(message, _event_retention));
                }
                _ => {}
            }
        }
        Err(CliError::ServerReportedError(
            "connection closed before the session attach completed".into(),
        ))
    })
    .await
    .map_err(|_| CliError::ServerReportedError("session attach timed out".into()))?;
    let _initial_tree = attach?;

    println!(
        "{{\"type\":\"progress\",\"request_id\":{request_id},\"stage\":\"querying-repository\"}}"
    );
    connection
        .requests
        .send(ClientRequest::QueryRepoFacts {
            request_id,
            project: ilium_core::ROOT_ID,
        })
        .await
        .map_err(|_| {
            CliError::ServerReportedError(
                "connection closed before repository query was sent".into(),
            )
        })?;
    let facts = tokio::time::timeout(WORKSPACE_FACTS_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::RepoFactsReported {
                    request_id: response_id,
                    result,
                    ..
                } if response_id == request_id => {
                    return result
                        .map_err(|message| {
                            CliError::received_server_error(message, _event_retention.clone())
                        })
                        .map(|facts| {
                            ilium_client::connection::Received::with_retention(
                                facts,
                                _event_retention,
                            )
                        });
                }
                ServerEvent::Error { message } => {
                    return Err(CliError::received_server_error(message, _event_retention));
                }
                _ => {}
            }
        }
        Err(CliError::ServerReportedError(
            "connection closed before repository facts arrived".into(),
        ))
    })
    .await
    .map_err(|_| CliError::ServerReportedError("repository query timed out".into()))??;

    let path = default_workspace_path(&facts, branch)?;
    let base_ref = base.unwrap_or(&facts.default_base_ref);
    let spec = if setup_command.trim().is_empty() {
        WorkspaceCreateSpec::New {
            branch: branch.to_string(),
            base_ref: base_ref.to_string(),
            path: path.clone(),
        }
    } else {
        WorkspaceCreateSpec::NewWithSetup {
            branch: branch.to_string(),
            base_ref: base_ref.to_string(),
            path: path.clone(),
            setup_command,
        }
    };
    connection
        .requests
        .send(ClientRequest::CreateAgentInWorkspace {
            request_id,
            parent_group: ilium_core::ROOT_ID,
            provider,
            spec,
            initial_input: None,
        })
        .await
        .map_err(|_| {
            CliError::ServerReportedError(
                "connection closed before workspace request was sent".into(),
            )
        })?;
    let result = tokio::time::timeout(WORKSPACE_CREATION_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::WorkspaceCreateProgress {
                    request_id: response_id,
                    stage,
                } if response_id == request_id => {
                    println!(
                        "{{\"type\":\"progress\",\"request_id\":{request_id},\"stage\":{}}}",
                        json_string(workspace_stage_name(stage))
                    );
                }
                ServerEvent::WorkspaceCreated {
                    request_id: response_id,
                    pane_id,
                } if response_id == request_id => return Ok(pane_id),
                ServerEvent::WorkspaceCreateFailed {
                    request_id: response_id,
                    error,
                } if response_id == request_id => return Err(CliError::received_server_error(error,_event_retention)),
                ServerEvent::Error { message } => return Err(CliError::received_server_error(message,_event_retention)),
                _ => {}
            }
        }
        Err(CliError::ServerReportedError("connection closed before workspace creation was confirmed".into()))
    })
    .await
    .map_err(|_| {
        CliError::ServerReportedError(
            "workspace confirmation timed out; creation may still have completed, so inspect the session and Git worktrees before retrying".into(),
        )
    })?;
    let _ = connection.requests.send(ClientRequest::Detach).await;
    let pane_id = result?;
    println!(
        "{{\"type\":\"result\",\"request_id\":{request_id},\"pane_id\":{},\"branch\":{},\"base\":{},\"worktree_path\":{}}}",
        pane_id.0,
        json_string(branch),
        json_string(base_ref),
        json_string(&path.to_string_lossy())
    );
    Ok(())
}

fn workspace_provider(cmd: &[String]) -> Result<BuiltinAgentProvider, CliError> {
    match cmd {
        [command] => BuiltinAgentProvider::from_command_line(command).ok_or_else(|| {
            CliError::ServerReportedError(
                "--worktree requires exactly one built-in agent command: claude, codex, or agy"
                    .into(),
            )
        }),
        _ => Err(CliError::ServerReportedError(
            "--worktree requires exactly one built-in agent command: claude, codex, or agy".into(),
        )),
    }
}

fn default_workspace_path(facts: &RepoFacts, branch: &str) -> Result<PathBuf, CliError> {
    // Git lists the main worktree first. The session may have started from a
    // linked checkout, so its `checkout_root` is not necessarily the repo's
    // stable location for sibling worktrees.
    let main_worktree = facts.worktrees.first().ok_or_else(|| {
        CliError::ServerReportedError("repository has no registered main worktree".into())
    })?;
    let main_root = &main_worktree.path;
    if !main_root.is_absolute() {
        return Err(CliError::ServerReportedError(
            "repository main worktree path is not absolute".into(),
        ));
    }
    let parent = main_root.parent().ok_or_else(|| {
        CliError::ServerReportedError("repository main worktree has no parent directory".into())
    })?;
    let mut directory_name = main_root
        .file_name()
        .ok_or_else(|| {
            CliError::ServerReportedError("repository main worktree has no name".into())
        })?
        .to_os_string();
    directory_name.push(".worktrees");
    Ok(parent
        .join(directory_name)
        .join(ilium_core::slugify_branch(branch)))
}

const fn workspace_stage_name(stage: WorkspaceCreateStage) -> &'static str {
    match stage {
        WorkspaceCreateStage::CreatingWorktree => "creating-worktree",
        WorkspaceCreateStage::Preparing => "preparing",
        WorkspaceCreateStage::Starting => "starting",
        WorkspaceCreateStage::RunningSetup => "running-setup",
    }
}

async fn new_pane(
    session_name: &str,
    cmd: &[String],
    cwd: &Path,
    keep_open: bool,
) -> Result<(), CliError> {
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
            let (event, _event_retention) = event.into_parts();
            match event {
                ilium_ipc::ServerEvent::TreeSnapshot(tree) => {
                    return Ok(Some(tree.panes().count()));
                }
                ilium_ipc::ServerEvent::Error { message } => {
                    return Err(CliError::received_server_error(message, _event_retention));
                }
                _ => {}
            }
        }
        Ok(None)
    })
    .await;
    let baseline_pane_count = match baseline_wait {
        Ok(Ok(count)) => count,
        Ok(Err(message)) => return Err(message),
        Err(_elapsed) => None,
    };

    let command_line = shell_join(cmd);
    let kind = if keep_open {
        ilium_ipc::NewPaneKind::Command(command_line)
    } else {
        ilium_ipc::NewPaneKind::CommandClosingOnExit(command_line)
    };
    connection
        .requests
        .send(ilium_ipc::ClientRequest::NewPane {
            parent_group: ilium_core::ROOT_ID,
            kind,
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
            let (event, _event_retention) = event.into_parts();
            match event {
                ilium_ipc::ServerEvent::Error { message } => {
                    return Some(Err(CliError::received_server_error(
                        message,
                        _event_retention,
                    )));
                }
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
        Ok(Some(Err(message))) => Err(message),
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
    let quota = ilium_client::bootstrap_process_quota().map_err(ilium_logging::LoggingError::Io)?;
    ilium_logging::initialize_forwarded(log_path, enabled, "cli", &quota)?;
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
    join_for_shell(args, pane_shell_is_cmd())
}

fn join_for_shell(args: &[String], cmd_shell: bool) -> String {
    args.iter()
        .map(|argument| quote_for_shell(argument, cmd_shell))
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the pane command line will be parsed by `cmd.exe` rather than a
/// POSIX shell. Mirrors the server's shell choice (`SHELL`, else `COMSPEC`): a
/// Git Bash/MSYS `SHELL` still takes POSIX quoting.
fn pane_shell_is_cmd() -> bool {
    if !cfg!(windows) {
        return false;
    }
    std::env::var("SHELL")
        .ok()
        .and_then(|shell| {
            std::path::Path::new(&shell)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.eq_ignore_ascii_case("cmd"))
        })
        .unwrap_or(true)
}

/// True if `argument` is safe to place bare (unquoted) in the pane shell's
/// command line: non-empty, and made up only of characters that never
/// trigger word-splitting, globbing, expansion, or redirection. Under
/// `cmd.exe` a backslash and `~` (8.3 short names such as `RUNNER~1`) are
/// ordinary path characters.
fn is_shell_safe_bare_word(argument: &str, cmd_shell: bool) -> bool {
    !argument.is_empty()
        && argument.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'-' | b'.' | b'/' | b'=' | b',' | b':' | b'@' | b'+'
                )
                || (cmd_shell && matches!(byte, b'\\' | b'~'))
        })
}

/// Quotes `argument` for the chosen shell only if `is_shell_safe_bare_word`
/// says it needs it; otherwise returns it unchanged.
fn quote_for_shell(argument: &str, cmd_shell: bool) -> String {
    if is_shell_safe_bare_word(argument, cmd_shell) {
        return argument.to_string();
    }
    if cmd_shell {
        // `cmd.exe` has no single quotes; double them inside a double-quoted word.
        return format!("\"{}\"", argument.replace('"', "\"\""));
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
        chatroom_project_root, client_restart_args, default_workspace_path, join_for_shell,
        json_string, pane_identity_from_values, pane_progress_json, progress_report_json, session,
        shell_join, workspace_provider, Cli, Command, ProgressCommand,
    };
    use clap::Parser;

    #[test]
    fn onboarding_flag_is_available_for_bare_and_named_interactive_attach() {
        assert!(
            Cli::try_parse_from(["ilium", "--onboarding"])
                .unwrap()
                .onboarding
        );
        let named = Cli::try_parse_from(["ilium", "new-session", "demo", "--onboarding"]).unwrap();
        assert!(named.onboarding);
        assert!(matches!(named.command, Some(Command::NewSession { .. })));
        assert!(!Cli::try_parse_from(["ilium"]).unwrap().onboarding);
    }

    #[test]
    fn cmd_shell_keeps_backslash_and_short_name_paths_bare() {
        let path = r"C:\Users\RUNNER~1\x\codex.exe";
        assert!(super::is_shell_safe_bare_word(path, true));
        assert!(!super::is_shell_safe_bare_word(path, false));
        assert!(!super::is_shell_safe_bare_word("a b", true));
    }

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
    fn worktree_flags_require_a_branch_and_keep_plain_new_pane_available() {
        let plain = Cli::try_parse_from(["ilium", "new-pane", "--", "cat"]).unwrap();
        assert!(matches!(
            plain.command,
            Some(Command::NewPane {
                worktree: false,
                branch: None,
                base: None,
                ..
            })
        ));
        assert!(Cli::try_parse_from(["ilium", "new-pane", "--worktree", "--", "codex"]).is_err());
        assert!(
            Cli::try_parse_from(["ilium", "new-pane", "--branch", "agent/fix", "--", "codex"])
                .is_err()
        );
        let workspace = Cli::try_parse_from([
            "ilium",
            "new-pane",
            "--worktree",
            "--branch",
            "agent/fix",
            "--base",
            "main",
            "--",
            "codex",
        ])
        .unwrap();
        assert!(matches!(
            workspace.command,
            Some(Command::NewPane {
                worktree: true,
                branch: Some(branch),
                base: Some(base),
                cmd,
                ..
            }) if branch == "agent/fix" && base == "main" && cmd == ["codex"]
        ));
    }

    #[test]
    fn worktree_mode_accepts_only_exact_builtin_agent_commands() {
        for command in ["claude", "codex", "agy"] {
            assert!(workspace_provider(&[command.to_string()]).is_ok());
        }
        for command in ["cat", "/usr/bin/codex", "codex --ask-for-approval never"] {
            assert!(workspace_provider(&[command.to_string()]).is_err());
        }
        assert!(workspace_provider(&["codex".into(), "resume".into()]).is_err());
    }

    #[test]
    fn default_worktree_path_uses_main_checkout_for_a_linked_project_subdirectory() {
        // A rooted path without a drive letter is not absolute on Windows, and
        // the code under test rejects relative main-worktree paths, so the
        // fixture needs a base that is absolute on the platform running it.
        let work = if cfg!(windows) {
            PathBuf::from(r"C:\work")
        } else {
            PathBuf::from("/work")
        };
        let facts = ilium_ipc::RepoFacts {
            repo_common_dir: work.join("api").join(".git"),
            checkout_root: work.join("api.worktrees").join("existing-agent"),
            project_subpath: PathBuf::from("crates/service"),
            current_branch: Some("main".into()),
            default_base_ref: "main".into(),
            default_base_commit: "abc123".into(),
            local_branches: vec!["main".into()],
            worktrees: vec![
                ilium_ipc::WorkspaceWorktreeFact {
                    path: work.join("api"),
                    branch: Some("main".into()),
                    created_by_ilium: false,
                    is_dirty: false,
                    occupied_pane_id: None,
                },
                ilium_ipc::WorkspaceWorktreeFact {
                    path: work.join("api.worktrees").join("existing-agent"),
                    branch: Some("agent/existing".into()),
                    created_by_ilium: true,
                    is_dirty: false,
                    occupied_pane_id: None,
                },
            ],
            source_dirty_count: 0,
            main_dirty_count: 0,
            has_gitmodules: false,
            git_version: ilium_ipc::WorkspaceGitVersion {
                major: 2,
                minor: 17,
                patch: 0,
            },
        };
        assert_eq!(
            default_workspace_path(&facts, "agent/fix-login").unwrap(),
            work.join("api.worktrees").join("agent-fix-login")
        );
    }

    #[test]
    fn arguments_with_spaces_are_quoted_so_they_stay_one_word() {
        assert_eq!(
            join_for_shell(&["ls".to_string(), "my folder".to_string()], false),
            "ls 'my folder'"
        );
    }

    #[test]
    fn embedded_single_quotes_are_escaped() {
        assert_eq!(
            join_for_shell(&["echo".to_string(), "it's here".to_string()], false),
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
                    wait: false,
                    timeout_seconds: None,
                    replace: false,
                }
            }) if command == "/work/status --json"
        ));
    }

    #[test]
    fn progress_set_can_wait_with_a_timeout_and_replace_on_purpose() {
        let cli = Cli::try_parse_from([
            "ilium",
            "progress",
            "set",
            "--command",
            "/work/status --json",
            "--wait",
            "--timeout-seconds",
            "540",
            "--replace",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Progress {
                command: ProgressCommand::Set {
                    wait: true,
                    timeout_seconds: Some(540),
                    replace: true,
                    ..
                }
            })
        ));
        // A timeout only makes sense for a waiting set.
        assert!(Cli::try_parse_from([
            "ilium",
            "progress",
            "set",
            "--command",
            "/work/status --json",
            "--timeout-seconds",
            "540",
        ])
        .is_err());
    }

    #[test]
    fn progress_wait_and_top_level_wait_take_an_optional_monitor_id() {
        let current = Cli::try_parse_from(["ilium", "progress", "wait"]).unwrap();
        assert!(matches!(
            current.command,
            Some(Command::Progress {
                command: ProgressCommand::Wait {
                    monitor_id: None,
                    timeout_seconds: None
                }
            })
        ));
        let exact =
            Cli::try_parse_from(["ilium", "progress", "wait", "7", "--timeout-seconds", "60"])
                .unwrap();
        assert!(matches!(
            exact.command,
            Some(Command::Progress {
                command: ProgressCommand::Wait {
                    monitor_id: Some(7),
                    timeout_seconds: Some(60)
                }
            })
        ));
        let top_level = Cli::try_parse_from(["ilium", "wait", "7"]).unwrap();
        assert!(matches!(
            top_level.command,
            Some(Command::Wait {
                monitor_id: Some(7),
                timeout_seconds: None
            })
        ));
        assert!(Cli::try_parse_from(["ilium", "wait", "not-a-number"]).is_err());
    }

    #[test]
    fn progress_no_longer_exposes_goal_pause_or_resume_controls() {
        assert!(Cli::try_parse_from([
            "ilium",
            "progress",
            "set",
            "--command",
            "/work/status --json",
            "--goal-policy",
            "pause-and-resume",
        ])
        .is_err());
        for operation in ["arm-goal-resume", "disarm-goal-resume"] {
            assert!(
                Cli::try_parse_from(["ilium", "progress", operation, "--monitor-id", "42"])
                    .is_err()
            );
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
            String::new(),
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
            String::new(),
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
