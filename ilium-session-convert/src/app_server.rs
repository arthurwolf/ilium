//! A private `codex app-server` child spoken to over stdio JSON-RPC.
//!
//! `codex app-server --listen stdio://` is its own server process bound to
//! this pipe pair; it never attaches to the user's shared app-server daemon
//! (that is `codex app-server daemon`, a different subcommand). The child and
//! all of its descendants form one process tree owned by a
//! [`ProcessTreeGuard`], so every exit path -- success, error, cancellation,
//! panic -- kills and reaps it.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ilium_platform::process_control::{
    lower_background_child_priority, prepare_process_tree, ProcessTreeGuard,
};
use serde_json::{json, Value};

use crate::error::{single_line, ConvertError};
use crate::report::Reporter;

/// How often a wait wakes up to check cancellation and the deadline.
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// How long a clean exit is awaited after stdin closes before the tree is killed.
const GRACEFUL_EXIT_WAIT: Duration = Duration::from_secs(2);
/// Upper bound on waiting for the reader threads to see EOF after a failure.
const READER_EOF_WAIT: Duration = Duration::from_secs(2);
/// Stderr lines remembered for error messages.
const STDERR_TAIL_LINES: usize = 12;

/// JSON-RPC error code for "method not found", used to refuse server-initiated
/// requests (approvals, auth refresh) this client does not implement.
const METHOD_NOT_FOUND: i64 = -32601;

enum Incoming {
    Message(Value),
    Garbage(String),
    Stderr(String),
    Closed,
}

/// What a message handler wants the wait loop to do next.
pub(crate) enum Flow {
    Continue,
    Done,
}

pub(crate) struct AppServer {
    child: Child,
    guard: ProcessTreeGuard,
    stdin: Option<ChildStdin>,
    incoming: Receiver<Incoming>,
    readers: Vec<JoinHandle<()>>,
    stderr_tail: Vec<String>,
    next_request_id: u64,
}

/// Everything needed to launch the child.
pub(crate) struct AppServerLaunch {
    pub(crate) executable: OsString,
    pub(crate) environment: Vec<(OsString, OsString)>,
}

impl AppServer {
    pub(crate) fn spawn(launch: &AppServerLaunch) -> Result<Self, ConvertError> {
        let mut command = Command::new(&launch.executable);
        command
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &launch.environment {
            command.env(key, value);
        }
        lower_background_child_priority(&mut command);
        prepare_process_tree(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| ConvertError::CodexUnavailable {
                executable: launch.executable.to_string_lossy().into_owned(),
                error,
            })?;
        let guard = match ProcessTreeGuard::attach(child.id()) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ConvertError::CodexProtocol(format!(
                    "cannot own the app-server process tree: {error}"
                )));
            }
        };
        // Both handles were requested piped above, so `take` cannot be None.
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (sender, incoming) = mpsc::channel();
        let mut readers = Vec::new();
        if let Some(stdout) = stdout {
            let sender = sender.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let item = match serde_json::from_str::<Value>(&line) {
                        Ok(value) => Incoming::Message(value),
                        Err(_) => Incoming::Garbage(line),
                    };
                    if sender.send(item).is_err() {
                        return;
                    }
                }
                let _ = sender.send(Incoming::Closed);
            }));
        }
        if let Some(stderr) = stderr {
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if sender.send(Incoming::Stderr(line)).is_err() {
                        return;
                    }
                }
            }));
        }
        Ok(Self {
            child,
            guard,
            stdin,
            incoming,
            readers,
            stderr_tail: Vec::new(),
            next_request_id: 1,
        })
    }

    fn write_message(&mut self, message: &Value) -> Result<(), ConvertError> {
        let mut encoded = message.to_string();
        encoded.push('\n');
        let written = match self.stdin.as_mut() {
            Some(stdin) => stdin
                .write_all(encoded.as_bytes())
                .and_then(|()| stdin.flush())
                .is_ok(),
            None => false,
        };
        if written {
            Ok(())
        } else {
            Err(self.exited_error())
        }
    }

    pub(crate) fn notify(&mut self, method: &str) -> Result<(), ConvertError> {
        self.write_message(&json!({"method": method}))
    }

    /// Sends a request and returns its id.
    pub(crate) fn send_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<u64, ConvertError> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.write_message(&json!({"id": id, "method": method, "params": params}))?;
        Ok(id)
    }

    fn tail_text(&self) -> String {
        if self.stderr_tail.is_empty() {
            return "no output on stderr".to_string();
        }
        single_line(&self.stderr_tail.join(" | "), 300)
    }

    fn remember_stderr(&mut self, line: String) {
        self.stderr_tail.push(line);
        if self.stderr_tail.len() > STDERR_TAIL_LINES {
            self.stderr_tail.remove(0);
        }
    }

    /// Pumps messages until `handler` returns [`Flow::Done`], the child closes
    /// its stdout, `timeout` elapses, or cancellation is requested. Every
    /// other concern -- stderr lines, non-JSON output, server-initiated
    /// requests -- is handled here so callers see only protocol messages.
    pub(crate) fn wait_for(
        &mut self,
        stage: &'static str,
        timeout: Duration,
        reporter: &mut Reporter<'_>,
        handler: &mut dyn FnMut(&Value, &mut Reporter<'_>) -> Result<Flow, ConvertError>,
    ) -> Result<(), ConvertError> {
        let deadline = Instant::now() + timeout;
        loop {
            reporter.check_cancel()?;
            let now = Instant::now();
            if now >= deadline {
                return Err(ConvertError::CodexTimeout {
                    stage,
                    seconds: timeout.as_secs(),
                });
            }
            let wait = POLL_INTERVAL.min(deadline - now);
            match self.incoming.recv_timeout(wait) {
                Ok(Incoming::Message(message)) => {
                    if is_server_request(&message) {
                        self.refuse_server_request(&message, reporter)?;
                        continue;
                    }
                    if matches!(handler(&message, reporter)?, Flow::Done) {
                        return Ok(());
                    }
                }
                Ok(Incoming::Garbage(line)) => {
                    reporter.log(format!(
                        "codex stdout (not JSON): {}",
                        single_line(&line, 200)
                    ));
                }
                Ok(Incoming::Stderr(line)) => {
                    reporter.log(format!("codex stderr: {}", single_line(&line, 300)));
                    self.remember_stderr(line);
                }
                Ok(Incoming::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.exited_error());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// The error for a child that is gone or going. stdout and stderr are
    /// read by separate threads, so the child's last words may still be in
    /// flight; killing the tree closes every pipe end and both readers are
    /// awaited (bounded) so the message carries the real reason.
    fn exited_error(&mut self) -> ConvertError {
        let _ = self.guard.terminate();
        let deadline = Instant::now() + READER_EOF_WAIT;
        while Instant::now() < deadline && self.readers.iter().any(|reader| !reader.is_finished()) {
            std::thread::sleep(Duration::from_millis(5));
        }
        while let Ok(item) = self.incoming.try_recv() {
            if let Incoming::Stderr(line) = item {
                self.remember_stderr(line);
            }
        }
        ConvertError::CodexExited(self.tail_text())
    }

    fn refuse_server_request(
        &mut self,
        request: &Value,
        reporter: &mut Reporter<'_>,
    ) -> Result<(), ConvertError> {
        let method = request.get("method").and_then(Value::as_str).unwrap_or("?");
        reporter.log(format!(
            "codex asked for `{method}`, which this client does not handle"
        ));
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        self.write_message(&json!({
            "id": id,
            "error": {"code": METHOD_NOT_FOUND, "message": "not supported by ilium-session-convert"}
        }))
    }

    /// Closes stdin so the server can exit on its own, then kills and reaps
    /// the whole process tree. Safe to call more than once.
    pub(crate) fn shutdown(&mut self) {
        self.stdin = None;
        let deadline = Instant::now() + GRACEFUL_EXIT_WAIT;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            }
        }
        // Kills descendants too (the `codex` launcher may wrap a native binary).
        let _ = self.guard.terminate();
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A message carrying both `id` and `method` is a request from the server.
fn is_server_request(message: &Value) -> bool {
    message.get("id").is_some() && message.get("method").is_some()
}

/// Interprets a JSON-RPC response to request `id`: `Some(Ok(result))`,
/// `Some(Err(text))` for an error object, `None` for any other message.
pub(crate) fn response_for(message: &Value, id: u64) -> Option<Result<Value, String>> {
    if message.get("method").is_some() || message.get("id").and_then(Value::as_u64) != Some(id) {
        return None;
    }
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Some(Err(text));
    }
    Some(Ok(message.get("result").cloned().unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_matching_ignores_notifications_and_other_ids() {
        let notification = json!({"method": "x", "params": {}});
        let other = json!({"id": 9, "result": {}});
        let ours = json!({"id": 2, "result": {"importId": "a"}});
        let failed = json!({"id": 2, "error": {"code": -1, "message": "boom"}});
        assert!(response_for(&notification, 2).is_none());
        assert!(response_for(&other, 2).is_none());
        assert_eq!(response_for(&ours, 2), Some(Ok(json!({"importId": "a"}))));
        assert_eq!(response_for(&failed, 2), Some(Err("boom".to_string())));
    }

    #[test]
    fn server_requests_need_both_id_and_method() {
        assert!(is_server_request(&json!({"id": 1, "method": "m"})));
        assert!(!is_server_request(&json!({"id": 1, "result": {}})));
        assert!(!is_server_request(&json!({"method": "m"})));
    }
}
