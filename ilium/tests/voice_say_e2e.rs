//! End-to-end test of `ilium voice say`, with no network, no API key and no
//! audio device.
//!
//! Real processes: a detached `ilium-server`, the interactive `ilium` client
//! under a PTY (which owns the voice session), and one-shot `ilium voice say`
//! commands. The only stand-in is the OpenAI Realtime endpoint: a scripted
//! WebSocket server on loopback, reached through `ILIUM_VOICE_REALTIME_URL`,
//! with `ILIUM_VOICE_AUDIO=none` so no microphone or speaker is opened.
//!
//! The mock plays the model's part: when it sees a typed sentence it answers
//! with a `function_call` for the client's real `ilium_send_to_terminal` tool.
//! Text therefore travels the complete production route -- CLI, server relay,
//! voice actor, tool dispatcher, IPC, PTY -- and the assertion is on bytes
//! arriving in a real terminal pane, not on a shortcut.
//!
//! Isolation: every process gets the same throwaway XDG/runtime/config
//! directories, the pane is `sh -c 'cat > file'` so its input can be read
//! back, and media pausing is off with the session bus pointed nowhere, so the
//! developer's music player is never touched.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ilium_client::connection::Connection;
use ilium_core::NodeId;
use ilium_ipc::ServerEvent;
use ilium_pty::{PtyCommand, PtySession};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const SESSION_NAME: &str = "default";
const PROJECT_NAME: &str = "Voicetest";

fn ilium_binary() -> String {
    std::env::var_os("ILIUM_PTY_SMOKE_BINARY")
        .map(|binary_path| binary_path.to_string_lossy().into_owned())
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_ilium").to_string())
}

async fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// The throwaway directories every spawned process shares.
struct IsolatedDirs {
    data_home: PathBuf,
    config_home: PathBuf,
    ilium_config_dir: PathBuf,
    debug_log_dir: PathBuf,
    agent_setup_home: PathBuf,
    runtime_dir: PathBuf,
    socket_dir: PathBuf,
    /// Short directory under `/tmp` so the session socket fits `sockaddr_un`.
    _runtime_root: tempfile::TempDir,
}

impl IsolatedDirs {
    fn under(root: &Path) -> Self {
        let data_home = root.join("data");
        let config_home = root.join("config");
        let ilium_config_dir = config_home.join("ilium");
        let debug_log_dir = root.join("debug-logs");
        let agent_setup_home = root.join("agent-home");
        for dir in [
            &data_home,
            &ilium_config_dir,
            &debug_log_dir,
            &agent_setup_home,
        ] {
            std::fs::create_dir_all(dir).expect("create isolated directory");
        }
        let runtime_root = tempfile::Builder::new()
            .prefix("iv")
            .tempdir_in("/tmp")
            .expect("short runtime directory");
        let runtime_dir = runtime_root.path().to_path_buf();
        let socket_dir = runtime_dir.join("ilium");
        Self {
            data_home,
            config_home,
            ilium_config_dir,
            debug_log_dir,
            agent_setup_home,
            runtime_dir,
            socket_dir,
            _runtime_root: runtime_root,
        }
    }

    fn pairs(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            ("XDG_DATA_HOME", self.data_home.clone()),
            ("XDG_CONFIG_HOME", self.config_home.clone()),
            ("XDG_RUNTIME_DIR", self.runtime_dir.clone()),
            (
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                self.socket_dir.clone(),
            ),
            (
                ilium_platform::paths::CONFIG_DIR_ENV,
                self.ilium_config_dir.clone(),
            ),
            (
                ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                self.debug_log_dir.clone(),
            ),
            (
                ilium_client::AGENT_SETUP_HOME_ENV,
                self.agent_setup_home.clone(),
            ),
        ]
    }

    /// Seeds the client config: voice off but fully configured (a dummy key
    /// that only ever reaches the loopback mock), no media pausing, and every
    /// AI trigger cleared so nothing calls a provider.
    fn seed_config(&self, project_dir: &Path) {
        let mut config = ilium_client::config::load(&self.ilium_config_dir).expect("load config");
        config.voice.enabled = false;
        config.voice.api_key = "loopback-only-test-key".to_owned();
        config.voice.pause_media_while_active = false;
        config.voice.output_volume_percent = 0;
        let triggers = &mut config.triggers;
        for actions in [
            &mut triggers.startup_complete,
            &mut triggers.agent_session_ready,
            &mut triggers.agent_prompt_submitted,
            &mut triggers.agent_started_working,
            &mut triggers.agent_waiting_background,
            &mut triggers.agent_approval_required,
            &mut triggers.agent_finished_work,
            &mut triggers.terminal_activity_checkpoint,
        ] {
            actions.clear();
        }
        ilium_client::config::save_voice_settings(&self.ilium_config_dir, &config.voice)
            .expect("save voice settings");
        ilium_client::config::save_trigger_settings(&self.ilium_config_dir, &config.triggers)
            .expect("save trigger settings");
        let mut agent_setup = config.agent_setup.clone();
        agent_setup.never_ask_global = true;
        agent_setup
            .never_ask_projects
            .push(project_dir.to_path_buf());
        ilium_client::config::save_agent_setup_settings(&self.ilium_config_dir, &agent_setup)
            .expect("save agent setup policy");

        // A stored project name keeps the client from inferring one online.
        let ilium_dir = project_dir.join(".ilium");
        std::fs::create_dir_all(&ilium_dir).expect("create .ilium");
        std::fs::write(
            ilium_dir.join("config.yaml"),
            format!("project name: {PROJECT_NAME}\n"),
        )
        .expect("write project config");
    }
}

/// Kills the detached server this test started, even when an assertion fails.
struct KillSessionOnDrop<'a> {
    dirs: &'a IsolatedDirs,
    cwd: PathBuf,
}

impl Drop for KillSessionOnDrop<'_> {
    fn drop(&mut self) {
        let mut command = std::process::Command::new(ilium_binary());
        command
            .args(["kill-session", SESSION_NAME])
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        for (key, value) in self.dirs.pairs() {
            command.env(key, value);
        }
        if let Ok(mut child) = command.spawn() {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

async fn run_ilium(
    dirs: &IsolatedDirs,
    cwd: &Path,
    args: &[&str],
    stdin: Option<&str>,
) -> CommandOutput {
    use tokio::io::AsyncWriteExt;

    let mut command = tokio::process::Command::new(ilium_binary());
    command
        .args(args)
        .current_dir(cwd)
        .stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in dirs.pairs() {
        command.env(key, value);
    }
    // A pane environment must never leak in from the developer's own Ilium
    // session: it would redirect the command to that session.
    for variable in [
        ilium_ipc::pane_env::PANE_ID,
        ilium_ipc::pane_env::SESSION_NAME,
        ilium_ipc::pane_env::SESSION_SOCKET,
    ] {
        command.env_remove(variable);
    }
    let mut child = command.spawn().expect("spawn ilium");
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().expect("piped stdin");
        pipe.write_all(input.as_bytes()).await.expect("write stdin");
    }
    let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .unwrap_or_else(|_| panic!("`ilium {args:?}` did not finish in time"))
        .expect("wait for ilium");
    CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The JSONL records of one command, each parsed and required to carry `type`.
fn records(output: &CommandOutput) -> Vec<Value> {
    output
        .stdout
        .lines()
        .map(|line| {
            let record: Value =
                serde_json::from_str(line).unwrap_or_else(|error| panic!("{line:?}: {error}"));
            assert!(record["type"].is_string(), "record without a type: {line}");
            record
        })
        .collect()
}

/// A scripted stand-in for the Realtime API that plays the model.
#[derive(Clone)]
struct MockRealtime {
    url: String,
    /// Every event the client sent, in order.
    received: Arc<Mutex<Vec<Value>>>,
    /// Tool call to answer a typed sentence with, keyed by the sentence.
    script: Arc<Mutex<Vec<(String, Value)>>>,
    /// How many provider connections the client opened (a restart or a
    /// reconnect would add one).
    connections: Arc<std::sync::atomic::AtomicUsize>,
}

impl MockRealtime {
    async fn start() -> (Self, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let address = listener.local_addr().expect("mock address");
        let mock = Self {
            url: format!("ws://{address}/v1/realtime"),
            received: Arc::new(Mutex::new(Vec::new())),
            script: Arc::new(Mutex::new(Vec::new())),
            connections: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        let serving = mock.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                serving.serve(stream).await;
            }
        });
        (mock, task)
    }

    async fn serve(&self, stream: tokio::net::TcpStream) {
        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
            return;
        };
        self.connections
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut last_user_text: Option<String> = None;
        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            let event: Value = serde_json::from_str(&text).expect("client JSON");
            self.received.lock().unwrap().push(event.clone());
            let reply = match event["type"].as_str() {
                Some("session.update") => Some(json!({"type": "session.updated"})),
                Some("conversation.item.create") => {
                    if event["item"]["role"] == "user" {
                        last_user_text = event["item"]["content"][0]["text"]
                            .as_str()
                            .map(str::to_owned);
                    }
                    None
                }
                Some("response.create") => {
                    let call = last_user_text.take().and_then(|text| {
                        self.script
                            .lock()
                            .unwrap()
                            .iter()
                            .find(|(sentence, _)| *sentence == text)
                            .map(|(_, call)| call.clone())
                    });
                    Some(json!({
                        "type": "response.done",
                        "response": {"output": call.into_iter().collect::<Vec<_>>()},
                    }))
                }
                _ => None,
            };
            if let Some(reply) = reply {
                if socket
                    .send(Message::Text(reply.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    /// Text of every user message the client created, in order.
    fn user_texts(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event["type"] == "conversation.item.create" && event["item"]["role"] == "user"
            })
            .filter_map(|event| {
                event["item"]["content"][0]["text"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect()
    }

    fn function_call_outputs(&self) -> Vec<Value> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event["item"]["type"] == "function_call_output")
            .cloned()
            .collect()
    }

    fn connection_count(&self) -> usize {
        self.connections.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn session_update_count(&self) -> usize {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event["type"] == "session.update")
            .count()
    }

    fn send_to_terminal_call(&self, sentence: &str, call_id: &str, pane_id: NodeId, text: &str) {
        self.script.lock().unwrap().push((
            sentence.to_owned(),
            json!({
                "type": "function_call",
                "call_id": call_id,
                "name": "ilium_send_to_terminal",
                "arguments": json!({"target": {"id": pane_id.0}, "text": text}).to_string(),
            }),
        ));
    }
}

async fn first_pane_id(dirs: &IsolatedDirs, project_dir: &Path) -> NodeId {
    let project_root = project_dir.canonicalize().expect("project root");
    let socket_path = ilium::session::socket_path_in(&dirs.socket_dir, &project_root, SESSION_NAME);
    let mut connection = Connection::connect(&socket_path, SESSION_NAME.to_owned())
        .await
        .expect("connect to the isolated server");
    let pane_id = tokio::time::timeout(WAIT_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            if let ServerEvent::TreeSnapshot(tree) = event {
                if let Some(pane) = tree.panes().next() {
                    return pane.id;
                }
            }
        }
        panic!("server closed the connection before a tree snapshot with a pane");
    })
    .await
    .expect("a tree snapshot with the receiver pane");
    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;
    pane_id
}

#[tokio::test]
async fn voice_say_reaches_a_terminal_through_the_real_voice_pipeline() {
    let temp_root = tempfile::tempdir().expect("tempdir");
    let dirs = IsolatedDirs::under(temp_root.path());
    let project_dir = temp_root.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    dirs.seed_config(&project_dir);
    let _cleanup = KillSessionOnDrop {
        dirs: &dirs,
        cwd: project_dir.clone(),
    };
    let received_file = temp_root.path().join("received.txt");

    // Nothing runs yet: the command must refuse, not spawn a server.
    let not_running = run_ilium(&dirs, &project_dir, &["voice", "say", "hello"], None).await;
    assert!(!not_running.success);
    let not_running_records = records(&not_running);
    let error = not_running_records.last().expect("an error record");
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "session-not-running");
    assert_eq!(error["ok"], false);

    // A server with one pane whose stdin is captured to a file.
    let script = format!("cat > {}", received_file.display());
    let new_pane = run_ilium(
        &dirs,
        &project_dir,
        &["new-pane", "--", "sh", "-c", &script],
        None,
    )
    .await;
    assert!(new_pane.success, "new-pane failed: {}", new_pane.stdout);
    let pane_id = first_pane_id(&dirs, &project_dir).await;

    let (mock, mock_task) = MockRealtime::start().await;
    mock.send_to_terminal_call(
        "first typed sentence",
        "call-1",
        pane_id,
        "MARKER-ONE from a typed sentence",
    );
    mock.send_to_terminal_call(
        "second typed sentence",
        "call-2",
        pane_id,
        "MARKER-TWO from stdin",
    );

    // The interactive client, voice off, under a real PTY. It hosts the voice
    // session and, being interactive, registers with the server as its host.
    let mut command = PtyCommand::new(ilium_binary(), &project_dir, 40, 120)
        .arg("--cwd")
        .arg(project_dir.to_string_lossy().to_string())
        .env("ILIUM_VOICE_REALTIME_URL", mock.url.clone())
        .env("ILIUM_VOICE_AUDIO", "none")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent");
    for (key, value) in dirs.pairs() {
        command = command.env(key, value.to_string_lossy().to_string());
    }
    let mut tui = PtySession::spawn(command).expect("spawn the interactive client");
    assert!(
        wait_until(|| tui.screen_text().contains("VOICE OFF"), WAIT_TIMEOUT).await,
        "expected the footer to show voice off, got: {:?}",
        tui.screen_text()
    );

    // Voice off and no --start: a structured refusal naming the fix.
    // (The client registers as the voice host right after attaching; retry
    // briefly so the very first command cannot outrun that registration.)
    let mut refused = run_ilium(&dirs, &project_dir, &["voice", "say", "hello"], None).await;
    for _ in 0..40 {
        let code = records(&refused)
            .last()
            .map(|record| record["code"].clone())
            .unwrap_or(Value::Null);
        if code != "no-voice-client" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        refused = run_ilium(&dirs, &project_dir, &["voice", "say", "hello"], None).await;
    }
    assert!(!refused.success, "stdout: {}", refused.stdout);
    let refusal = records(&refused).last().cloned().expect("error record");
    assert_eq!(refusal["type"], "error");
    assert_eq!(refusal["code"], "voice-off", "{refusal}");
    assert!(refusal["hint"].as_str().unwrap().contains("--start"));
    assert!(mock.user_texts().is_empty(), "nothing may reach the model");

    // --start switches voice on, and the sentence reaches the model as a
    // user turn.
    let started = run_ilium(
        &dirs,
        &project_dir,
        &["voice", "say", "--start", "first typed sentence"],
        None,
    )
    .await;
    assert!(started.success, "{} / {}", started.stdout, started.stderr);
    let started_records = records(&started);
    assert_eq!(started_records.first().unwrap()["type"], "progress");
    let result = started_records.last().unwrap();
    assert_eq!(result["type"], "result", "{result}");
    assert_eq!(result["ok"], true);
    assert_eq!(result["started_voice"], true);
    assert_eq!(result["accepted_sentences"], 1);
    assert!(wait_until(|| mock.session_update_count() >= 1, WAIT_TIMEOUT).await);
    assert!(
        wait_until(
            || mock.user_texts() == ["first typed sentence"],
            WAIT_TIMEOUT
        )
        .await,
        "the model must receive the typed sentence, got {:?}",
        mock.user_texts()
    );
    let persisted = std::fs::read_to_string(dirs.ilium_config_dir.join("config.toml"))
        .expect("config after --start");
    assert!(
        persisted.contains("enabled = true"),
        "--start persists the voice setting like F8 does: {persisted}"
    );

    // The scripted model called the real tool: the text arrived in the pane.
    assert!(
        wait_until(
            || std::fs::read_to_string(&received_file)
                .is_ok_and(|text| text.contains("MARKER-ONE from a typed sentence")),
            WAIT_TIMEOUT
        )
        .await,
        "the pane never received the text; screen: {:?}",
        tui.screen_text()
    );
    assert!(
        wait_until(|| !mock.function_call_outputs().is_empty(), WAIT_TIMEOUT).await,
        "the client must return the tool result to the model"
    );
    assert!(
        wait_until(
            || tui.screen_text().contains("VOICE LISTENING"),
            WAIT_TIMEOUT
        )
        .await,
        "the footer shows the live session, got: {:?}",
        tui.screen_text()
    );

    // Voice already on: no --start needed, and stdin sentences follow the
    // positional ones, one turn each, in order.
    let mixed = run_ilium(
        &dirs,
        &project_dir,
        &["voice", "say", "-"],
        Some("second typed sentence\n\nthird typed sentence\n"),
    )
    .await;
    assert!(mixed.success, "{} / {}", mixed.stdout, mixed.stderr);
    let mixed_result = records(&mixed).last().cloned().unwrap();
    assert_eq!(mixed_result["type"], "result", "{mixed_result}");
    assert_eq!(mixed_result["started_voice"], false);
    assert_eq!(mixed_result["accepted_sentences"], 2);
    assert!(
        wait_until(
            || mock.user_texts()
                == [
                    "first typed sentence",
                    "second typed sentence",
                    "third typed sentence"
                ],
            WAIT_TIMEOUT
        )
        .await,
        "typed turns must arrive in order, got {:?}",
        mock.user_texts()
    );
    assert!(
        wait_until(
            || std::fs::read_to_string(&received_file)
                .is_ok_and(|text| text.contains("MARKER-TWO from stdin")),
            WAIT_TIMEOUT
        )
        .await,
        "the second tool call must reach the pane too"
    );

    // Restating --start on a running session changes nothing.
    let again = run_ilium(
        &dirs,
        &project_dir,
        &["voice", "say", "--start", "thanks"],
        None,
    )
    .await;
    assert!(again.success, "{} / {}", again.stdout, again.stderr);
    let again_result = records(&again).last().cloned().unwrap();
    assert_eq!(again_result["started_voice"], false);
    assert_eq!(mock.connection_count(), 1, "one session, never restarted");

    // A malformed request never reaches the model.
    let invalid = run_ilium(&dirs, &project_dir, &["voice", "say", "   "], None).await;
    assert!(!invalid.success);
    assert_eq!(records(&invalid).last().unwrap()["code"], "invalid-request");

    let _ = tui.kill();
    mock_task.abort();
}
