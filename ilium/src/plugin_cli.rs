//! `ilium <name> ...` for the subcommands plugins contribute (api-cli,
//! capability-cli-contribute).
//!
//! A plugin contributes a subcommand in `contributes.cli` of its manifest and holds the
//! `cli.contribute` grant. The `ilium` binary knows only the names it ships natively; any other
//! first word reaches [`run`] through a clap external subcommand. This module then:
//!
//! 1. lists the contributed commands of the enabled bundled plugins ([`contributed_commands`]);
//!    a disabled plugin's command is unknown, exactly as a name nobody contributes,
//! 2. validates the arguments the manifest declares (the `pane-selector` group) and applies the
//!    command's stdin policy ([`prepare`]),
//! 3. for `requires_server: true` attaches to the project's live session, sends `CliInvoke`,
//!    streams `CliOutput` to standard output and standard error and exits with the `CliExit`
//!    code; Ctrl+C sends `CliCancel` and exits 130,
//! 4. for `requires_server: false` runs a transient in-process host with only that plugin over an
//!    empty tree and no panes, and exits the same way.
//!
//! The wire has no caller-pane or terminal fields, so a command that reads `ILIUM_PANE_ID` from
//! its own environment cannot be told over a live session: the server resolves `--pane .` only
//! when it knows the caller, and answers a usage error otherwise.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ilium_ipc::plugin::{CliStream, ControlBody, EnvelopeBody, PluginEnvelope};
use ilium_ipc::{ClientRequest, ServerEvent};
use ilium_plugin_api::generated::types::{ErrorCode, Intent, NodeId, Outcome, PluginError};
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
};
use ilium_plugin_api::{ProcessKind, Side};
use ilium_plugin_host::engine::{EngineConfig, PluginEngine};
use ilium_plugin_host::imports::backed::NullLogSink;
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::runtime::cli::{split_pane_selector, CliCommand, CliStart, CliStartError};
use ilium_plugin_host::runtime::{CataloguePlugin, HostRuntime, RuntimeConfig, RuntimeThread};
use ilium_plugin_host::storage::StorageContext;
use ilium_plugin_host::wiring;
use serde_json::Value;

use crate::plugin_cmd::bundled_catalogue;

/// The grant that lets a plugin contribute subcommands.
const CLI_GRANT: &str = "cli.contribute";
/// The exit code of a command stopped by Ctrl+C (128 + SIGINT).
const EXIT_CANCELLED: i32 = 130;
/// The exit code of a refused invocation (the usual usage-error code).
const EXIT_USAGE: i32 = 2;
/// The exit code of a failure the command did not report itself.
const EXIT_FAILURE: i32 = 1;
/// The invocation number a live CLI process uses; one process runs one command.
const LIVE_INVOCATION: u64 = 1;
/// The width reported when the terminal does not say.
const DEFAULT_COLUMNS: u16 = 80;
/// How often the transient host is pumped while a command runs.
const TRANSIENT_TICK: Duration = Duration::from_millis(5);
/// The time the transient host gets to deactivate its plugin when the command ended.
const TRANSIENT_SHUTDOWN_BUDGET: Duration = Duration::from_secs(2);
/// The epoch tick of the transient engine, the same as the server's.
const EPOCH_TICK_MS: u64 = 10;

/// What [`run`] reports to `main`.
#[derive(Debug, PartialEq, Eq)]
pub enum Ran {
    /// The command ran (or was refused as a usage error) and this is the exit code.
    Exit(i32),
    /// No enabled plugin contributes this name; `main` reports it as clap does.
    Unknown(String),
}

/// Why [`prepare`] did not produce an invocation.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No enabled plugin with the grant contributes this name.
    Unknown(String),
    /// The arguments or the standard input do not fit the command; exit code 2.
    Usage {
        /// The command name.
        command: String,
        /// What is wrong.
        message: String,
    },
}

/// A command ready to run.
#[derive(Debug, PartialEq, Eq)]
pub struct Prepared<'a> {
    /// The contributed command.
    pub command: &'a CliCommand,
    /// The words after the command name, as typed.
    pub args: Vec<String>,
    /// Whether the command reads standard input (the caller reads and sends it).
    pub read_stdin: bool,
}

/// The commands the enabled plugins of `catalogue` contribute, sorted by name.
///
/// A plugin counts when it is not disabled and its manifest lists `cli.contribute` among the
/// grants of any side. The runtime that serves the command enforces the grant the user actually
/// gave; a declined grant is answered as an unknown command there.
pub fn contributed_commands(
    catalogue: &[CataloguePlugin],
    disabled: &BTreeSet<String>,
) -> Vec<CliCommand> {
    let mut commands = Vec::new();
    for plugin in catalogue {
        if disabled.contains(&plugin.id) {
            continue;
        }
        let Ok(manifest) = serde_json::from_str::<Value>(&plugin.manifest) else {
            continue;
        };
        if !declares_grant(&manifest) {
            continue;
        }
        let Some(entries) = manifest
            .pointer("/contributes/cli")
            .and_then(Value::as_array)
        else {
            continue;
        };
        commands.extend(
            entries
                .iter()
                .filter_map(|entry| CliCommand::from_manifest(&plugin.id, entry)),
        );
    }
    commands.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.plugin.cmp(&right.plugin))
    });
    commands
}

/// Whether any side of the manifest lists the `cli.contribute` grant.
fn declares_grant(manifest: &Value) -> bool {
    manifest
        .get("capabilities")
        .and_then(Value::as_object)
        .is_some_and(|sides| {
            sides.values().any(|grants| {
                grants.as_array().is_some_and(|grants| {
                    grants.iter().any(|grant| grant.as_str() == Some(CLI_GRANT))
                })
            })
        })
}

/// The command a word names (its name or one of its aliases).
pub fn resolve<'a>(commands: &'a [CliCommand], word: &str) -> Option<&'a CliCommand> {
    commands.iter().find(|command| command.answers_to(word))
}

/// The `ilium --help` section for the contributed commands, or `None` when there are none.
/// Names in `native` are the subcommands the binary already has; a plugin command with the same
/// name is shadowed by the native one and is not listed twice.
pub fn help_section(commands: &[CliCommand], native: &[String]) -> Option<String> {
    let listed: Vec<&CliCommand> = commands
        .iter()
        .filter(|command| !native.iter().any(|name| name == &command.name))
        .collect();
    if listed.is_empty() {
        return None;
    }
    let width = listed
        .iter()
        .map(|command| command.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::from("Plugin commands:\n");
    for command in listed {
        text.push_str(&format!(
            "  {:<width$}  {}\n",
            command.name,
            command.summary,
            width = width
        ));
    }
    Some(text)
}

/// Checks the arguments and the stdin policy of the command a word names.
///
/// `stdin_is_terminal` says whether standard input is a terminal: a command with
/// `stdin: required` needs piped bytes and refuses a terminal; `stdin: optional` reads only when
/// something is piped; `stdin: none` never reads.
pub fn prepare<'a>(
    commands: &'a [CliCommand],
    words: &[String],
    stdin_is_terminal: bool,
) -> Result<Prepared<'a>, Refusal> {
    let Some((name, args)) = words.split_first() else {
        return Err(Refusal::Unknown(String::new()));
    };
    let Some(command) = resolve(commands, name) else {
        return Err(Refusal::Unknown(name.clone()));
    };
    let usage = |message: String| Refusal::Usage {
        command: command.name.clone(),
        message,
    };
    if command.takes_pane_selector() {
        split_pane_selector(args).map_err(&usage)?;
    }
    let read_stdin = match command.stdin.as_str() {
        "required" if stdin_is_terminal => {
            return Err(usage(
                "standard input is required; pipe or redirect the input".to_owned(),
            ));
        }
        "required" => true,
        "optional" => !stdin_is_terminal,
        _ => false,
    };
    Ok(Prepared {
        command,
        args: args.to_vec(),
        read_stdin,
    })
}

/// The `CliInvoke` request for a command.
pub fn invoke_request(
    invocation: u64,
    command: &CliCommand,
    args: Vec<String>,
    stdin: Vec<u8>,
    cwd: PathBuf,
    columns: u16,
) -> ClientRequest {
    ClientRequest::Plugin(PluginEnvelope {
        plugin: command.plugin.clone(),
        seq: 1,
        body: EnvelopeBody::Control(ControlBody::CliInvoke {
            invocation_id: invocation,
            plugin: command.plugin.clone(),
            command: command.name.clone(),
            args,
            stdin,
            cwd,
            columns,
        }),
    })
}

/// The `CliCancel` request for a running command.
pub fn cancel_request(invocation: u64, plugin: &str) -> ClientRequest {
    ClientRequest::Plugin(PluginEnvelope {
        plugin: plugin.to_owned(),
        seq: 2,
        body: EnvelopeBody::Control(ControlBody::CliCancel {
            invocation_id: invocation,
        }),
    })
}

/// What one server event means for a running command.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep reading.
    Continue,
    /// The command ended with this code.
    Exit(i32),
    /// The caller's output is closed (a broken pipe): stop the command.
    OutputClosed,
}

/// Applies one server event to the output of invocation `invocation`.
pub fn apply_event(
    event: ServerEvent,
    invocation: u64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Step {
    match event {
        ServerEvent::Error { message } => {
            if writeln!(stderr, "ilium: {message}").is_err() {
                return Step::OutputClosed;
            }
            Step::Exit(EXIT_FAILURE)
        }
        ServerEvent::Plugin(PluginEnvelope {
            body:
                EnvelopeBody::Control(ControlBody::CliOutput {
                    invocation_id,
                    stream,
                    data,
                }),
            ..
        }) if invocation_id == invocation => {
            let written = match stream {
                CliStream::Stdout => stdout.write_all(&data).and_then(|()| stdout.flush()),
                CliStream::Stderr => stderr.write_all(&data).and_then(|()| stderr.flush()),
            };
            if written.is_err() {
                Step::OutputClosed
            } else {
                Step::Continue
            }
        }
        ServerEvent::Plugin(PluginEnvelope {
            body:
                EnvelopeBody::Control(ControlBody::CliExit {
                    invocation_id,
                    code,
                }),
            ..
        }) if invocation_id == invocation => Step::Exit(code),
        _ => Step::Continue,
    }
}

/// The directory plugin data lives in, or `None` without a home.
fn data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ilium").map(|dirs| dirs.data_dir().to_path_buf())
}

/// The commands the enabled bundled plugins contribute in this process: none under
/// `--no-plugins`.
pub fn load_contributed() -> Vec<CliCommand> {
    if wiring::start_mode_from_env() == StartMode::NoPlugins {
        return Vec::new();
    }
    let disabled = data_dir()
        .map(|dir| wiring::disabled_at_start(&dir).0)
        .unwrap_or_default();
    contributed_commands(&bundled_catalogue("server"), &disabled)
}

/// Runs the plugin subcommand `words` (the name first) for the project in `cwd`.
pub async fn run(words: Vec<OsString>, cwd: &Path) -> Ran {
    let mut text = Vec::with_capacity(words.len());
    for word in words {
        match word.into_string() {
            Ok(word) => text.push(word),
            Err(word) => {
                eprintln!("ilium: the argument {word:?} is not valid UTF-8");
                return Ran::Exit(EXIT_USAGE);
            }
        }
    }
    let commands = load_contributed();
    let stdin_is_terminal = std::io::stdin().is_terminal();
    let prepared = match prepare(&commands, &text, stdin_is_terminal) {
        Ok(prepared) => prepared,
        Err(Refusal::Unknown(name)) => return Ran::Unknown(name),
        Err(Refusal::Usage { command, message }) => {
            eprintln!("ilium {command}: {message}");
            return Ran::Exit(EXIT_USAGE);
        }
    };
    let command = prepared.command.clone();
    let stdin = if prepared.read_stdin {
        match tokio::task::spawn_blocking(read_standard_input).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                eprintln!(
                    "ilium {}: reading standard input failed: {error}",
                    command.name
                );
                return Ran::Exit(EXIT_FAILURE);
            }
            Err(error) => {
                eprintln!(
                    "ilium {}: reading standard input failed: {error}",
                    command.name
                );
                return Ran::Exit(EXIT_FAILURE);
            }
        }
    } else {
        Vec::new()
    };
    let absolute = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let code = if command.requires_server {
        run_live(&command, prepared.args, stdin, absolute, cwd).await
    } else {
        run_transient(&command, prepared.args, stdin, absolute).await
    };
    Ran::Exit(code)
}

fn read_standard_input() -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::io::stdin().lock().read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn terminal_columns() -> u16 {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|columns| *columns > 0)
        .unwrap_or(DEFAULT_COLUMNS)
}

// ---- over a live session ---------------------------------------------------------------------

/// Runs `command` on the project's live session and returns the exit code.
async fn run_live(
    command: &CliCommand,
    args: Vec<String>,
    stdin: Vec<u8>,
    absolute_cwd: PathBuf,
    project: &Path,
) -> i32 {
    let session = match crate::session::resolve_project_session(
        project,
        crate::session::DEFAULT_SESSION_NAME,
    ) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("ilium {}: {error}", command.name);
            return EXIT_FAILURE;
        }
    };
    if !crate::session::is_session_live(&session.socket_path) {
        eprintln!(
            "ilium {}: this command needs a running session; start one with `ilium` first",
            command.name
        );
        return EXIT_FAILURE;
    }
    let mut connection = match ilium_client::connection::Connection::connect(
        &session.socket_path,
        session.name.clone(),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%error, "ilium plugin command: the live session did not answer");
            eprintln!(
                "ilium {}: the session did not answer: {error}",
                command.name
            );
            return EXIT_FAILURE;
        }
    };
    let request = invoke_request(
        LIVE_INVOCATION,
        command,
        args,
        stdin,
        absolute_cwd,
        terminal_columns(),
    );
    if let Err(error) = connection.requests.send(request).await {
        tracing::warn!(%error, "ilium plugin command: the request was not sent");
        eprintln!("ilium {}: the session closed the connection", command.name);
        return EXIT_FAILURE;
    }
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    loop {
        tokio::select! {
            event = connection.events.recv() => {
                let Some(event) = event else {
                    eprintln!("ilium {}: the session ended before the command finished", command.name);
                    return EXIT_FAILURE;
                };
                let (event, _retention) = event.into_parts();
                match apply_event(event, LIVE_INVOCATION, &mut stdout, &mut stderr) {
                    Step::Continue => {}
                    Step::Exit(code) => return code,
                    Step::OutputClosed => {
                        send_cancel(&mut connection, &command.plugin).await;
                        return EXIT_FAILURE;
                    }
                }
            }
            _ = &mut interrupt => {
                send_cancel(&mut connection, &command.plugin).await;
                return EXIT_CANCELLED;
            }
        }
    }
}

/// Asks the session to stop the command; a closed connection already did.
async fn send_cancel(connection: &mut ilium_client::connection::Connection, plugin: &str) {
    if let Err(error) = connection
        .requests
        .send(cancel_request(LIVE_INVOCATION, plugin))
        .await
    {
        tracing::debug!(%error, "ilium plugin command: the cancel was not sent");
    }
}

// ---- without a server ------------------------------------------------------------------------

/// Runs `command` (which needs no server) in a transient host that holds only its plugin.
async fn run_transient(
    command: &CliCommand,
    args: Vec<String>,
    stdin: Vec<u8>,
    absolute_cwd: PathBuf,
) -> i32 {
    let cancelled = Arc::new(AtomicBool::new(false));
    let watcher = {
        let cancelled = Arc::clone(&cancelled);
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancelled.store(true, Ordering::SeqCst);
            }
        })
    };
    let command = command.clone();
    let work = {
        let cancelled = Arc::clone(&cancelled);
        tokio::task::spawn_blocking(move || {
            transient_host(&command, args, stdin, absolute_cwd, &cancelled)
        })
    };
    let code = match work.await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ilium: the plugin command stopped unexpectedly: {error}");
            EXIT_FAILURE
        }
    };
    watcher.abort();
    code
}

/// The empty kernel of a transient host: no tree, no panes, no intents.
struct EmptyKernel;

static EMPTY_KERNEL: EmptyKernel = EmptyKernel;

impl TreeRead for EmptyKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        Vec::new()
    }
    fn node(&self, _id: NodeId) -> Option<TreeNode> {
        None
    }
    fn epoch(&self) -> u64 {
        0
    }
}

impl PaneRead for EmptyKernel {
    fn screen_text(&self, _pane: NodeId) -> Option<String> {
        None
    }
    fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        None
    }
    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        None
    }
}

impl KernelFacade for EmptyKernel {
    fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult {
        Outcome::Refused(PluginError::new(
            ErrorCode::Unsupported,
            "this command runs without a session; it has no panes to change",
        ))
    }
    fn tree(&self) -> &dyn TreeRead {
        &EMPTY_KERNEL
    }
    fn panes(&self) -> &dyn PaneRead {
        &EMPTY_KERNEL
    }
    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Cli
    }
}

/// Runs the whole transient host on the calling (blocking) thread and returns the exit code.
fn transient_host(
    command: &CliCommand,
    args: Vec<String>,
    stdin: Vec<u8>,
    absolute_cwd: PathBuf,
    cancelled: &AtomicBool,
) -> i32 {
    use ilium_plugin_host::imports::cli::CliChunk;

    let data = data_dir();
    let catalogue: Vec<CataloguePlugin> = bundled_catalogue("server")
        .into_iter()
        .filter(|plugin| plugin.id == command.plugin)
        .collect();
    if catalogue.is_empty() {
        eprintln!(
            "ilium {}: the plugin {} is not installed",
            command.name, command.plugin
        );
        return EXIT_FAILURE;
    }
    let cache_dir = data.as_ref().map(|dir| dir.join("plugins").join("cache"));
    if let Some(cache) = &cache_dir {
        if let Err(error) = std::fs::create_dir_all(cache) {
            tracing::warn!(
                "plugin code cache {} is unavailable: {error}",
                cache.display()
            );
        }
    }
    let engine = match PluginEngine::new(EngineConfig {
        epoch_tick_ms: EPOCH_TICK_MS,
        cache_dir,
        process: ProcessKind::Cli,
    }) {
        Ok(engine) => Arc::new(engine),
        Err(error) => {
            eprintln!(
                "ilium {}: the plugin engine did not start: {error}",
                command.name
            );
            return EXIT_FAILURE;
        }
    };
    let log: Arc<dyn ilium_plugin_host::imports::backed::LogSink> = Arc::new(NullLogSink);
    let provider = data.as_ref().map(|dir| {
        wiring::import_provider(wiring::ProcessImports {
            storage_root: wiring::storage_root(dir),
            scope: StorageContext {
                project: Some(absolute_cwd.clone()),
                ..StorageContext::default()
            },
            basic: None,
            log: Arc::clone(&log),
        })
    });
    let runtime = HostRuntime::new(
        engine,
        RuntimeConfig {
            side: Side::Server,
            kernel: Arc::new(EmptyKernel),
            log,
            provider,
            disabled: BTreeSet::new(),
        },
    );
    let thread = match RuntimeThread::spawn(runtime) {
        Ok(thread) => thread,
        Err(error) => {
            eprintln!(
                "ilium {}: the plugin host did not start: {error}",
                command.name
            );
            return EXIT_FAILURE;
        }
    };
    let start = CliStart {
        plugin: command.plugin.clone(),
        command: command.name.clone(),
        args,
        stdin,
        cwd: absolute_cwd.to_string_lossy().into_owned(),
        stdout_tty: std::io::stdout().is_terminal(),
        stderr_tty: std::io::stderr().is_terminal(),
        caller_pane: None,
    };
    let started = thread.call(move |runtime| {
        for failure in runtime.load(catalogue) {
            tracing::warn!("ilium plugin command: {failure}");
        }
        runtime.start_cli(start)
    });
    let invocation = match started {
        Some(Ok(invocation)) => invocation,
        Some(Err(CliStartError::Usage(message))) => {
            eprintln!("ilium {}: {message}", command.name);
            let _ = thread.shutdown(TRANSIENT_SHUTDOWN_BUDGET);
            return EXIT_USAGE;
        }
        Some(Err(error)) => {
            eprintln!("ilium {}: {error}", command.name);
            let _ = thread.shutdown(TRANSIENT_SHUTDOWN_BUDGET);
            return EXIT_FAILURE;
        }
        None => {
            eprintln!(
                "ilium {}: the plugin host stopped before the command started",
                command.name
            );
            return EXIT_FAILURE;
        }
    };

    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let mut cancel_sent = false;
    let mut code = None;
    while code.is_none() {
        if cancelled.load(Ordering::SeqCst) && !cancel_sent {
            cancel_sent = true;
            if let Some(Err(error)) = thread.call(move |runtime| runtime.cancel_cli(invocation)) {
                tracing::warn!("ilium plugin command: cancelling failed: {error}");
            }
            code = Some(EXIT_CANCELLED);
        }
        let chunks = thread.call(|runtime| {
            for failure in runtime.pump() {
                tracing::warn!("ilium plugin command: {failure}");
            }
            runtime.take_cli_output()
        });
        let Some(chunks) = chunks else {
            eprintln!(
                "ilium {}: the plugin host stopped before the command finished",
                command.name
            );
            return EXIT_FAILURE;
        };
        for chunk in chunks {
            match chunk {
                CliChunk::Stdout { data, .. } => {
                    if stdout
                        .write_all(&data)
                        .and_then(|()| stdout.flush())
                        .is_err()
                    {
                        cancelled.store(true, Ordering::SeqCst);
                    }
                }
                CliChunk::Stderr { data, .. } => {
                    if stderr
                        .write_all(&data)
                        .and_then(|()| stderr.flush())
                        .is_err()
                    {
                        cancelled.store(true, Ordering::SeqCst);
                    }
                }
                CliChunk::Exit { code: exit, .. } => {
                    code.get_or_insert(exit);
                }
            }
        }
        if code.is_none() {
            std::thread::sleep(TRANSIENT_TICK);
        }
    }
    let _ = thread.shutdown(TRANSIENT_SHUTDOWN_BUDGET);
    code.unwrap_or(EXIT_FAILURE)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    const GRANT_SERVER: &str = r#""capabilities": { "server": ["cli.contribute"] }"#;

    fn manifest(id: &str, capabilities: &str, cli: &str) -> CataloguePlugin {
        CataloguePlugin {
            id: id.to_owned(),
            manifest: format!(
                r#"{{ "id": "{id}", {capabilities}, "contributes": {{ "cli": {cli} }} }}"#
            ),
            component: Some(Cow::Borrowed(&[])),
        }
    }

    fn catalogue() -> Vec<CataloguePlugin> {
        vec![
            manifest(
                "acme.tools",
                GRANT_SERVER,
                r#"[
                    { "name": "tally", "summary": "Count things", "stdin": "optional",
                      "aliases": ["count"] },
                    { "name": "offline", "summary": "Works alone", "requires_server": false },
                    { "name": "slurp", "summary": "Needs input", "stdin": "required" },
                    { "name": "focus", "summary": "Acts on panes",
                      "argument_groups": ["pane-selector"] }
                ]"#,
            ),
            // Declares commands but never asks for the grant: nothing is contributed.
            manifest(
                "acme.ungranted",
                r#""capabilities": { "server": ["tree.write"] }"#,
                r#"[ { "name": "sneaky", "summary": "No grant" } ]"#,
            ),
            manifest(
                "acme.other",
                GRANT_SERVER,
                r#"[ { "name": "aardvark", "summary": "First by name" } ]"#,
            ),
        ]
    }

    fn words(text: &[&str]) -> Vec<String> {
        text.iter().map(|word| (*word).to_owned()).collect()
    }

    /// REQ: API-005
    #[test]
    fn contributed_commands_need_the_grant_and_an_enabled_plugin() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_str())
            .collect();
        assert_eq!(names, ["aardvark", "focus", "offline", "slurp", "tally"]);
        assert!(
            resolve(&commands, "sneaky").is_none(),
            "no grant, no command"
        );
    }

    /// REQ: API-005
    #[test]
    fn a_disabled_plugins_command_is_unknown() {
        let disabled: BTreeSet<String> = ["acme.tools".to_owned()].into();
        let commands = contributed_commands(&catalogue(), &disabled);
        assert_eq!(commands.len(), 1);
        assert_eq!(
            prepare(&commands, &words(&["tally"]), false),
            Err(Refusal::Unknown("tally".to_owned()))
        );
    }

    /// REQ: API-005
    #[test]
    fn a_name_or_an_alias_resolves_and_anything_else_is_unknown() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        assert_eq!(
            resolve(&commands, "count").map(|command| command.name.as_str()),
            Some("tally")
        );
        assert_eq!(
            resolve(&commands, "tally").map(|command| command.plugin.as_str()),
            Some("acme.tools")
        );
        assert!(resolve(&commands, "nonexistent").is_none());
        assert_eq!(
            prepare(&commands, &words(&["nonexistent"]), false),
            Err(Refusal::Unknown("nonexistent".to_owned()))
        );
        assert_eq!(
            prepare(&commands, &[], false),
            Err(Refusal::Unknown(String::new()))
        );
    }

    /// REQ: API-005
    #[test]
    fn help_lists_every_contributed_name_that_is_not_native() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        let help = help_section(&commands, &[]).expect("a section");
        assert!(help.starts_with("Plugin commands:\n"));
        for name in ["aardvark", "focus", "offline", "slurp", "tally"] {
            assert!(help.contains(name), "{name} is listed in:\n{help}");
        }
        assert!(help.contains("Count things"), "the summary is shown");
        let shadowed = help_section(&commands, &words(&["tally", "slurp"])).expect("a section");
        assert!(
            !shadowed.contains("tally"),
            "a native name is listed once, by clap"
        );
        assert!(shadowed.contains("aardvark"));
        assert_eq!(
            help_section(
                &commands,
                &words(&["aardvark", "focus", "offline", "slurp", "tally"])
            ),
            None
        );
        assert_eq!(help_section(&[], &[]), None);
    }

    /// REQ: API-005
    #[test]
    fn the_pane_selector_group_is_validated_before_the_command_runs() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        let ok = prepare(
            &commands,
            &words(&["focus", "--pane", "3", "--all-panes", "x"]),
            false,
        )
        .expect("valid");
        assert_eq!(ok.args, words(&["--pane", "3", "--all-panes", "x"]));
        let refused = prepare(&commands, &words(&["focus", "--pane"]), false);
        assert!(
            matches!(&refused, Err(Refusal::Usage { command, .. }) if command == "focus"),
            "a --pane without a value is a usage error: {refused:?}"
        );
        // A command without the group keeps the words to itself.
        let plain = prepare(&commands, &words(&["tally", "--pane"]), false).expect("valid");
        assert_eq!(plain.args, words(&["--pane"]));
    }

    /// REQ: API-005
    #[test]
    fn the_stdin_policy_decides_whether_input_is_read() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        let read = |name: &str, terminal: bool| {
            prepare(&commands, &words(&[name]), terminal).map(|prepared| prepared.read_stdin)
        };
        // none: never read, terminal or not.
        assert_eq!(read("focus", true), Ok(false));
        assert_eq!(read("focus", false), Ok(false));
        // optional: read only what is piped.
        assert_eq!(read("tally", true), Ok(false));
        assert_eq!(read("tally", false), Ok(true));
        // required: piped input is read, a terminal is refused.
        assert_eq!(read("slurp", false), Ok(true));
        assert!(matches!(
            read("slurp", true),
            Err(Refusal::Usage { command, .. }) if command == "slurp"
        ));
    }

    /// REQ: API-005
    #[test]
    fn requires_server_defaults_to_true() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        let requires = |name: &str| resolve(&commands, name).map(|command| command.requires_server);
        assert_eq!(requires("tally"), Some(true));
        assert_eq!(requires("offline"), Some(false));
    }

    /// REQ: API-005
    #[test]
    fn the_invoke_request_carries_the_owner_and_the_arguments() {
        let commands = contributed_commands(&catalogue(), &BTreeSet::new());
        let tally = resolve(&commands, "count").expect("alias");
        let request = invoke_request(
            7,
            tally,
            words(&["--json"]),
            b"input".to_vec(),
            PathBuf::from("/project"),
            100,
        );
        let ClientRequest::Plugin(PluginEnvelope { plugin, body, .. }) = request else {
            panic!("a plugin envelope");
        };
        assert_eq!(plugin, "acme.tools");
        assert_eq!(
            body,
            EnvelopeBody::Control(ControlBody::CliInvoke {
                invocation_id: 7,
                plugin: "acme.tools".to_owned(),
                command: "tally".to_owned(),
                args: words(&["--json"]),
                stdin: b"input".to_vec(),
                cwd: PathBuf::from("/project"),
                columns: 100,
            }),
            "an alias is sent as the command's own name"
        );
        let ClientRequest::Plugin(PluginEnvelope { body, .. }) = cancel_request(7, "acme.tools")
        else {
            panic!("a plugin envelope");
        };
        assert_eq!(
            body,
            EnvelopeBody::Control(ControlBody::CliCancel { invocation_id: 7 })
        );
    }

    fn output(invocation: u64, stream: CliStream, text: &str) -> ServerEvent {
        ServerEvent::Plugin(PluginEnvelope {
            plugin: String::new(),
            seq: 1,
            body: EnvelopeBody::Control(ControlBody::CliOutput {
                invocation_id: invocation,
                stream,
                data: text.as_bytes().to_vec(),
            }),
        })
    }

    fn exit(invocation: u64, code: i32) -> ServerEvent {
        ServerEvent::Plugin(PluginEnvelope {
            plugin: String::new(),
            seq: 2,
            body: EnvelopeBody::Control(ControlBody::CliExit {
                invocation_id: invocation,
                code,
            }),
        })
    }

    /// REQ: API-005
    #[test]
    fn output_events_reach_their_stream_and_exit_ends_the_command() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            apply_event(output(1, CliStream::Stdout, "one\n"), 1, &mut out, &mut err),
            Step::Continue
        );
        assert_eq!(
            apply_event(
                output(1, CliStream::Stderr, "warn\n"),
                1,
                &mut out,
                &mut err
            ),
            Step::Continue
        );
        // Another invocation's bytes and unrelated events are not ours.
        assert_eq!(
            apply_event(
                output(9, CliStream::Stdout, "other\n"),
                1,
                &mut out,
                &mut err
            ),
            Step::Continue
        );
        assert_eq!(
            apply_event(exit(9, 3), 1, &mut out, &mut err),
            Step::Continue
        );
        assert_eq!(
            apply_event(exit(1, 3), 1, &mut out, &mut err),
            Step::Exit(3)
        );
        assert_eq!(out, b"one\n");
        assert_eq!(err, b"warn\n");
    }

    /// REQ: API-005
    #[test]
    fn a_server_error_is_printed_and_fails_the_command() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let step = apply_event(
            ServerEvent::Error {
                message: "session mismatch".to_owned(),
            },
            1,
            &mut out,
            &mut err,
        );
        assert_eq!(step, Step::Exit(EXIT_FAILURE));
        assert_eq!(String::from_utf8_lossy(&err), "ilium: session mismatch\n");
        assert!(out.is_empty());
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// REQ: API-005
    #[test]
    fn a_closed_output_stops_the_command() {
        let mut err = Vec::new();
        let step = apply_event(
            output(1, CliStream::Stdout, "late\n"),
            1,
            &mut Broken,
            &mut err,
        );
        assert_eq!(step, Step::OutputClosed);
    }

    /// REQ: API-005
    #[test]
    fn the_empty_kernel_has_no_panes_and_refuses_intents() {
        let kernel = EmptyKernel;
        assert!(kernel.tree().nodes().is_empty());
        assert!(kernel.tree().node(NodeId(1)).is_none());
        assert!(kernel.panes().screen_text(NodeId(1)).is_none());
        assert_eq!(kernel.process_kind(), ProcessKind::Cli);
    }
}
