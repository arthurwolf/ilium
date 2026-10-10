//! Always-on, size-capped log of pane and server lifecycle events.
//!
//! The general tracing log is written to a file only when the user enables
//! debug file logging, so after an incident there was no record of which
//! panes failed to start or who closed them. This log is independent of
//! that setting: one JSON object per line in
//! `<project>/.ilium/logs/<session>.lifecycle.jsonl`, rotated once to
//! `.jsonl.1` when it reaches [`MAX_LOG_BYTES`].
//!
//! Recording never blocks or allocates unboundedly: events go through a
//! bounded queue to one owned writer task, which appends each batch through
//! the server's finite I/O lane. A full queue drops the event and counts it.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ilium_execution::{JobCost, Lane};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::execution::ExecutionClient;
use crate::state::ServerState;
use crate::task_guard::AbortOnDropHandle;

/// The log is rotated once it reaches this size; one previous file is kept.
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const QUEUE_CAPACITY: usize = 1024;
const MAX_BATCH_LINES: usize = 256;
/// How long shutdown waits for queued lines to reach the file.
const SHUTDOWN_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// One lifecycle event. Serialized with an `event` tag, snake_case.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum LifecycleEvent {
    ServerStarted {
        executable: Option<String>,
        /// Contents of the install record written next to the executable
        /// (`<executable>.build.json`), identifying the exact build.
        build: Option<serde_json::Value>,
        session: String,
        snapshot_path: String,
    },
    RestoreFinished {
        saved_panes: usize,
        failed_panes: Vec<u64>,
    },
    PaneStartFailed {
        pane_id: u64,
        attempts: u32,
        reason: String,
    },
    PaneStartRetried {
        pane_id: u64,
    },
    /// Who asked for a close; followed by one `PaneClosed` per pane removed.
    PaneCloseRequested {
        pane_id: u64,
        by: &'static str,
    },
    PaneClosed {
        pane_id: u64,
        reason: &'static str,
        resource: String,
    },
    ServerStopping {
        dropped_events: u64,
    },
}

#[derive(Serialize)]
struct Entry<'a> {
    at: String,
    pid: u32,
    #[serde(flatten)]
    event: &'a LifecycleEvent,
}

pub(crate) struct LifecycleLog {
    sender: std::sync::Mutex<Option<mpsc::Sender<String>>>,
    task: std::sync::Mutex<Option<AbortOnDropHandle<()>>>,
    dropped: AtomicU64,
}

impl LifecycleLog {
    pub(crate) fn start(client: ExecutionClient, path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
        let task = AbortOnDropHandle::new(tokio::spawn(write_loop(client, path, receiver)));
        Self {
            sender: std::sync::Mutex::new(Some(sender)),
            task: std::sync::Mutex::new(Some(task)),
            dropped: AtomicU64::new(0),
        }
    }

    fn enqueue(&self, line: String) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(sender) = sender.as_ref() else {
            return;
        };
        if sender.try_send(line).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records the stop event, closes the queue and waits briefly for the
    /// writer to flush it.
    pub(crate) async fn shutdown(&self) {
        if let Some(line) = format_entry(&LifecycleEvent::ServerStopping {
            dropped_events: self.dropped.load(Ordering::Relaxed),
        }) {
            self.enqueue(line);
        }
        drop(
            self.sender
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take(),
        );
        let task = self
            .task
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(task) = task {
            if tokio::time::timeout(SHUTDOWN_FLUSH_TIMEOUT, task.join())
                .await
                .is_err()
            {
                tracing::warn!("lifecycle log did not flush before shutdown");
            }
        }
    }
}

/// The lifecycle log path for a session snapshot path
/// (`<project>/.ilium/sessions/<name>.json`).
pub(crate) fn path_for_snapshot(snapshot_path: &Path, session_name: &str) -> PathBuf {
    let ilium_directory = snapshot_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    ilium_directory
        .join("logs")
        .join(format!("{session_name}.lifecycle.jsonl"))
}

/// Queues one event. A no-op before the log starts.
pub(crate) fn record(state: &ServerState, event: LifecycleEvent) {
    let Some(log) = state.lifecycle_log.get() else {
        return;
    };
    if let Some(line) = format_entry(&event) {
        log.enqueue(line);
    }
}

/// Records who requested closing `pane_id`.
pub(crate) fn record_close_request(
    state: &ServerState,
    pane_id: ilium_core::NodeId,
    by: &'static str,
) {
    record(
        state,
        LifecycleEvent::PaneCloseRequested {
            pane_id: pane_id.0,
            by,
        },
    );
}

/// Short description of a pane resource for close events.
pub(crate) fn describe_resource(resource: &crate::pane::PaneResource) -> String {
    use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
    fn origin_text(origin: &TerminalOrigin) -> String {
        match origin {
            TerminalOrigin::PlainShell => "shell".into(),
            TerminalOrigin::Command(command) => format!("command: {command}"),
            TerminalOrigin::Frozen { resume_command } => format!("frozen: {resume_command}"),
        }
    }
    match resource {
        PaneResource::Terminal(runtime) => {
            let mut text = origin_text(&runtime.origin);
            if let Some(session_id) = &runtime.session_id {
                text.push_str(&format!(" (session {session_id})"));
            }
            text
        }
        PaneResource::Editor { path } => match path {
            Some(path) => format!("editor: {}", path.display()),
            None => "editor".into(),
        },
        PaneResource::Unrestored(unrestored) => match &unrestored.kind {
            PaneSnapshotKind::Terminal(origin) => format!("unrestored {}", origin_text(origin)),
            PaneSnapshotKind::Editor { .. } => "unrestored editor".into(),
        },
    }
}

fn format_entry(event: &LifecycleEvent) -> Option<String> {
    let entry = Entry {
        at: chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S%.3f %:z")
            .to_string(),
        pid: std::process::id(),
        event,
    };
    match serde_json::to_string(&entry) {
        Ok(line) => Some(line),
        Err(error) => {
            tracing::warn!(%error, "lifecycle event could not be serialized");
            None
        }
    }
}

async fn write_loop(client: ExecutionClient, path: PathBuf, mut receiver: mpsc::Receiver<String>) {
    while let Some(first) = receiver.recv().await {
        let mut batch = vec![first];
        while batch.len() < MAX_BATCH_LINES {
            match receiver.try_recv() {
                Ok(line) => batch.push(line),
                Err(_) => break,
            }
        }
        let batch_bytes = batch.iter().map(|line| line.len() + 1).sum::<usize>();
        let cost = JobCost {
            input_bytes: batch_bytes
                .saturating_mul(2)
                .saturating_add(path.as_os_str().len().saturating_mul(4))
                .saturating_add(64 * 1024),
            result_bytes: 4096,
        };
        let job_path = path.clone();
        let result = client
            .run(Lane::Io, cost, move |_context| {
                append_lines(&job_path, &batch)
            })
            .await;
        if let Err(error) = result {
            tracing::warn!(?error, path = %path.display(), "lifecycle log append failed");
        }
    }
}

fn append_lines(path: &Path, lines: &[String]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ilium_platform::secure_fs::create_private_directory(parent)?;
    }
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES) {
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        std::fs::rename(path, PathBuf::from(rotated))?;
    }
    let mut file = ilium_platform::secure_fs::private_open_options()
        .create(true)
        .append(true)
        .open(path)?;
    let mut buffer = Vec::with_capacity(lines.iter().map(|line| line.len() + 1).sum());
    for line in lines {
        buffer.extend_from_slice(line.as_bytes());
        buffer.push(b'\n');
    }
    file.write_all(&buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_sits_beside_the_sessions_directory() {
        let path = path_for_snapshot(Path::new("/p/.ilium/sessions/default.json"), "default");
        assert_eq!(path, Path::new("/p/.ilium/logs/default.lifecycle.jsonl"));
    }

    #[test]
    fn entries_are_one_tagged_json_object_per_line() {
        let line = format_entry(&LifecycleEvent::PaneClosed {
            pane_id: 7,
            reason: "close_pane_request",
            resource: "command: codex".into(),
        })
        .expect("serializes");
        assert!(!line.contains('\n'));
        let value: serde_json::Value = serde_json::from_str(&line).expect("json");
        assert_eq!(value["event"], "pane_closed");
        assert_eq!(value["pane_id"], 7);
        assert_eq!(value["reason"], "close_pane_request");
        assert!(value["at"].as_str().is_some_and(|at| at.len() >= 19));
    }

    #[test]
    fn append_rotates_once_the_cap_is_reached() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("logs").join("s.lifecycle.jsonl");
        append_lines(&path, &["{\"a\":1}".into()]).expect("first append");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open");
        file.set_len(MAX_LOG_BYTES).expect("grow");
        drop(file);
        append_lines(&path, &["{\"b\":2}".into()]).expect("rotating append");
        let current = std::fs::read_to_string(&path).expect("current");
        assert_eq!(current, "{\"b\":2}\n");
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        assert_eq!(
            std::fs::metadata(PathBuf::from(rotated))
                .expect("rotated")
                .len(),
            MAX_LOG_BYTES
        );
    }
}
