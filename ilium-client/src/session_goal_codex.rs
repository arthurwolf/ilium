//! Owned, bounded local Codex database adapter for goal reads.
//!
//! No objective or provider response is logged. The caller must verify thread
//! ownership and fence completions against its pane generation.

use ilium_agent_session::goal_evidence::{codex_database_snapshot, GoalLookup};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const QUEUE_CAPACITY: usize = 32;

struct Request {
    thread_id: String,
    process_id: u32,
    process_identity_inode: u64,
    project: PathBuf,
    transcript_path: PathBuf,
    reply: oneshot::Sender<ProviderReply>,
}

pub(crate) struct ProviderReply {
    pub homes: Option<ilium_platform::process_info::AgentProviderHomes>,
    pub result: Option<ProviderGoalResult>,
    pub binding_valid: bool,
}

/// The retained objective remains charged until its consumer drops it.
pub(crate) struct ProviderGoalResult {
    pub lookup: GoalLookup,
    pub storage: ilium_execution::StorageAdmission,
}

/// One bounded queue for all Codex panes in this client.
pub(crate) struct CodexGoalService {
    requests: mpsc::Sender<Request>,
    worker: JoinHandle<()>,
    _storage: Arc<ilium_execution::StorageAdmission>,
}

impl CodexGoalService {
    pub(crate) fn start() -> Option<Self> {
        // Unsupported platforms cannot provide the process identity and sandbox
        // guarantees required for a provider read.
        if !Self::acquisition_is_qualified() {
            return None;
        }
        Self::start_worker()
    }

    fn start_worker() -> Option<Self> {
        tokio::runtime::Handle::try_current().ok()?;
        // Cover framing, JSON decoding, and temporary objective copies before
        // allowing the worker to accept provider output.
        let storage = Arc::new(
            crate::execution::process_quota()
                .reserve_external_storage(MAX_RESPONSE * 8)
                .ok()?,
        );
        let (requests, receiver) = mpsc::channel(QUEUE_CAPACITY);
        let worker_storage = Arc::clone(&storage);
        let worker = tokio::spawn(async move {
            run(receiver, worker_storage).await;
        });
        Some(Self {
            requests,
            worker,
            _storage: storage,
        })
    }

    fn acquisition_is_qualified() -> bool {
        cfg!(all(target_os = "linux", target_pointer_width = "64"))
    }

    /// Queue admission failure is unavailable, not proof that no goal exists.
    pub(crate) fn try_get(
        &self,
        thread_id: String,
        process_id: u32,
        process_identity_inode: u64,
        project: PathBuf,
        transcript_path: PathBuf,
    ) -> Option<oneshot::Receiver<ProviderReply>> {
        if thread_id.is_empty() || thread_id.len() > 128 {
            return None;
        }
        let (reply, receiver) = oneshot::channel();
        self.requests
            .try_send(Request {
                thread_id,
                process_id,
                process_identity_inode,
                project,
                transcript_path,
                reply,
            })
            .ok()?;
        Some(receiver)
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.requests.is_closed()
    }
}

/// A thread ID is authoritative only when the same agent holds its verified
/// transcript under the provider home and project being queried.
fn verified_thread_binding(
    process_id: u32,
    codex_home: &Path,
    project: &Path,
    path: &Path,
    thread_id: &str,
) -> Option<ilium_platform::file_identity::FileIdentity> {
    use ilium_agent_session::{TranscriptLocator, TranscriptReadLimits};
    use ilium_core::AgentClass;
    if !crate::session_goal_custody::only_transcript_open(process_id, path) {
        return None;
    }
    let locator = TranscriptLocator::new_bounded(
        codex_home,
        project,
        TranscriptReadLimits {
            line_bytes: 1024 * 1024,
            total_read_bytes: 16 * 1024 * 1024,
            scanned_entries: 4096,
            retained_path_bytes: 1024 * 1024,
        },
    )
    .with_codex_home(codex_home)
    .without_access_time_updates();
    if locator
        .transcript_from_path(&AgentClass::Codex, path)
        .is_none_or(|transcript| transcript.session_id != thread_id)
    {
        return None;
    }
    let file = ilium_platform::secure_fs::open_regular_file_without_atime(path).ok()?;
    if !ilium_platform::process_info::process_has_open_file(process_id, &file, 4096)
        .unwrap_or(false)
    {
        return None;
    }
    ilium_platform::file_identity::identity(&file).ok()
}

impl Drop for CodexGoalService {
    fn drop(&mut self) {
        // The worker's blocking query owns its sandbox and storage reservation
        // until it completes, even if the async worker is cancelled.
        self.worker.abort();
    }
}

#[cfg(target_os = "linux")]
fn read_sandboxed_goal(codex_home: &std::path::Path, thread_id: &str) -> Option<GoalLookup> {
    let request = serde_json::to_vec(&json!({"thread_id": thread_id})).ok()?;
    let sandbox = ilium_platform::codex_goal_sandbox::CodexGoalSandbox::spawn(codex_home).ok()?;
    let bytes = sandbox.query(&request, REQUEST_TIMEOUT).ok()?;
    let snapshot = serde_json::from_slice(&bytes).ok()?;
    codex_database_snapshot(thread_id, &snapshot)
}

#[cfg(not(target_os = "linux"))]
fn read_sandboxed_goal(_codex_home: &std::path::Path, _thread_id: &str) -> Option<GoalLookup> {
    None
}

/// Match the server's process identity inode and keep that process alive across
/// provider-home inspection and the read. Transcript/home binding is separate.
fn read_for_current_process(
    process_id: u32,
    process_identity_inode: u64,
    thread_id: &str,
    query: impl FnOnce(&std::path::Path, &str) -> Option<GoalLookup>,
) -> Option<(
    ilium_platform::process_info::AgentProviderHomes,
    Option<GoalLookup>,
)> {
    let process = ilium_platform::process_control::capture_pty_process(process_id).ok()?;
    if process.pidfs_inode() != Some(process_identity_inode) {
        return None;
    }
    let homes = ilium_platform::process_info::agent_provider_homes(process_id)?;
    let observation = query(&homes.codex_home, thread_id);
    if ilium_platform::process_info::agent_provider_homes(process_id).as_ref() != Some(&homes)
        || !ilium_platform::process_control::captured_process_is_current(&process).unwrap_or(false)
    {
        return None;
    }
    Some((homes, observation))
}

async fn run(
    mut requests: mpsc::Receiver<Request>,
    storage: Arc<ilium_execution::StorageAdmission>,
) {
    while let Some(request) = requests.recv().await {
        if request.reply.is_closed() {
            continue;
        }
        let thread_id = request.thread_id;
        let process_id = request.process_id;
        let process_identity_inode = request.process_identity_inode;
        let project = request.project;
        let transcript_path = request.transcript_path;
        let worker_storage = Arc::clone(&storage);
        let reply = tokio::task::spawn_blocking(move || {
            // The reservation follows the owned blocking task on cancellation.
            let _storage = worker_storage;
            let mut binding_valid = false;
            let Some((homes, observation)) = read_for_current_process(
                process_id,
                process_identity_inode,
                &thread_id,
                |home, thread| {
                    let identity = verified_thread_binding(
                        process_id,
                        home,
                        &project,
                        &transcript_path,
                        thread,
                    )?;
                    let result = read_sandboxed_goal(home, thread);
                    binding_valid = verified_thread_binding(
                        process_id,
                        home,
                        &project,
                        &transcript_path,
                        thread,
                    ) == Some(identity);
                    binding_valid.then_some(result).flatten()
                },
            ) else {
                return ProviderReply {
                    homes: None,
                    result: None,
                    binding_valid: false,
                };
            };
            let retained = observation.and_then(|lookup| {
                let bytes = match &lookup {
                    GoalLookup::Absent => 256,
                    GoalLookup::Present { objective, .. } => {
                        objective.capacity().saturating_mul(3).saturating_add(256)
                    }
                };
                let storage = crate::execution::process_quota()
                    .reserve_external_storage(bytes)
                    .ok()?;
                Some(ProviderGoalResult { lookup, storage })
            });
            ProviderReply {
                homes: Some(homes),
                result: retained,
                binding_valid,
            }
        })
        .await;
        if let Ok(reply) = reply {
            let _ = request.reply.send(reply);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn unavailable_process_identity_never_queries_provider() {
        let mut queried = false;
        let result = read_for_current_process(std::process::id(), 0, "synthetic-thread", |_, _| {
            queried = true;
            Some(GoalLookup::Absent)
        });
        assert!(result.is_none());
        assert!(!queried);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn verified_thread_requires_matching_open_transcript_and_project() {
        use std::io::{BufRead, BufReader};
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let project = root.path().join("project");
        let sessions = home.join("sessions/2026/10/29");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir(&project).unwrap();
        let thread = "00000000-0000-4000-8000-000000000001";
        let transcript = sessions.join(format!("rollout-2026-10-29T00-00-00-{thread}.jsonl"));
        let metadata = serde_json::json!({"type":"session_meta", "payload":{"id":thread, "cwd":project.to_string_lossy()}});
        std::fs::write(&transcript, format!("{metadata}\n")).unwrap();
        let mut child = std::process::Command::new("python3")
            .args([
                "-u",
                "-c",
                "import sys; f=open(sys.argv[1]); print('ready', flush=True); sys.stdin.read()",
            ])
            .arg(&transcript)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "ready");
        assert!(
            verified_thread_binding(child.id(), &home, &project, &transcript, thread).is_some()
        );
        assert!(
            verified_thread_binding(child.id(), &home, root.path(), &transcript, thread).is_none()
        );
        assert!(verified_thread_binding(
            child.id(),
            &home,
            &project,
            &transcript,
            "different-thread"
        )
        .is_none());
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn codex_process_exit_during_query_discards_result() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .env("HOME", "/")
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .spawn()
            .expect("spawn owned process");
        let mut queried = false;
        let Some(birth) = ilium_platform::process_control::capture_pty_process(child.id())
            .unwrap()
            .pidfs_inode()
        else {
            child.kill().expect("stop owned process");
            child.wait().expect("reap owned process");
            return;
        };
        let rejected = read_for_current_process(
            child.id(),
            birth.wrapping_add(1),
            "synthetic-thread",
            |_, _| {
                queried = true;
                Some(GoalLookup::Absent)
            },
        );
        assert!(rejected.is_none());
        assert!(
            !queried,
            "a different process birth must not reach the provider"
        );
        let observation =
            read_for_current_process(child.id(), birth, "synthetic-thread", |_, _| {
                queried = true;
                child.kill().expect("stop owned process");
                child.wait().expect("reap owned process");
                Some(GoalLookup::Absent)
            });
        assert!(queried, "the provider query must have been reached");
        assert!(observation.is_none());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn synthetic_codex_goal_is_read_without_changing_provider_database() {
        use std::io::{BufRead, BufReader};
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let codex_home = home.join("codex");
        std::fs::create_dir(&codex_home).unwrap();
        let script = r#"
import json, os, sqlite3, sys
home = os.environ['CODEX_HOME']
thread_id = '00000000-0000-4000-8000-000000000001'
session_dir = os.path.join(home, 'sessions', '2026', '10', '29')
os.makedirs(session_dir)
rollout = os.path.join(session_dir, 'rollout-2026-10-29T00-00-00-' + thread_id + '.jsonl')
transcript = open(rollout, 'w+')
transcript.write(json.dumps({'type':'session_meta', 'payload':{'id':thread_id, 'cwd':os.environ['HOME']}}) + '\n')
transcript.flush()
db = sqlite3.connect(os.path.join(home, 'goals_1.sqlite'))
db.execute('pragma journal_mode=wal')
db.execute('create table _sqlx_migrations (version bigint primary key, description text not null, installed_on timestamp not null default current_timestamp, success boolean not null, checksum blob not null, execution_time bigint not null)')
for version, description, checksum in [(1, 'thread goals', '8CE6280244A0B4B39C7C37C7F67AFF1670EC963D2401CDF2ABCFEA9904102DB6CD99B7BEDF366267791A45EB12F17713'), (2, 'thread goal continuation deferrals', 'F3FF7DE2FD89914820BEF371081E885FD17DF18A6FBDC432399A83255A8A01AA32FB7EEA5E4DE21A17BDA3DA16C49584')]:
    db.execute('insert into _sqlx_migrations (version, description, success, checksum, execution_time) values (?, ?, 1, ?, 0)', (version, description, bytes.fromhex(checksum)))
db.execute("create table thread_goals (thread_id text primary key not null, goal_id text not null, objective text not null, status text not null check(status in ('active', 'paused', 'blocked', 'usage_limited', 'budget_limited', 'complete')), token_budget integer, tokens_used integer not null default 0, time_used_seconds integer not null default 0, created_at_ms integer not null, updated_at_ms integer not null)")
db.execute('insert into thread_goals (thread_id, goal_id, objective, status, created_at_ms, updated_at_ms) values (?, ?, ?, ?, ?, ?)', (thread_id, 'synthetic-goal', 'synthetic objective', 'paused', 1, 1))
db.commit()
print('ready', flush=True)
sys.stdin.read()
"#;
        let mut provider = std::process::Command::new("python3")
            .args(["-u", "-c", script])
            .current_dir(home)
            .env("HOME", home)
            .env("CODEX_HOME", &codex_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = provider.stdout.take().unwrap();
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        let paths = ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"];
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(codex_home.join(path)).unwrap())
            .collect();
        let before_times: Vec<_> = paths
            .iter()
            .map(|path| {
                let metadata = std::fs::metadata(codex_home.join(path)).unwrap();
                (metadata.accessed().ok(), metadata.modified().ok())
            })
            .collect();
        let service = CodexGoalService::start_worker().expect("isolated Codex goal test worker");
        let Some(birth) = ilium_platform::process_control::capture_pty_process(provider.id())
            .unwrap()
            .pidfs_inode()
        else {
            drop(provider.stdin.take());
            provider.wait().expect("reap owned provider");
            return;
        };
        let transcript_path = codex_home.join("sessions/2026/10/29/rollout-2026-10-29T00-00-00-00000000-0000-4000-8000-000000000001.jsonl");
        let receiver = service
            .try_get(
                "00000000-0000-4000-8000-000000000001".into(),
                provider.id(),
                birth,
                home.to_path_buf(),
                transcript_path,
            )
            .unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(10), receiver)
            .await
            .unwrap()
            .unwrap();
        let lookup = reply.result.unwrap().lookup;
        assert!(
            matches!(lookup, GoalLookup::Present { phase: ilium_agent_session::goal_evidence::GoalPhase::Paused, objective } if objective == "synthetic objective")
        );
        let after_times: Vec<_> = paths
            .iter()
            .map(|path| {
                let metadata = std::fs::metadata(codex_home.join(path)).unwrap();
                (metadata.accessed().ok(), metadata.modified().ok())
            })
            .collect();
        let after: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(codex_home.join(path)).unwrap())
            .collect();
        assert_eq!(before, after);
        assert_eq!(before_times, after_times);
        drop(provider.stdin.take());
        assert!(provider.wait().unwrap().success());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn sandbox_adapter_reads_synthetic_goal_without_changing_database() {
        use std::io::{BufRead, BufReader};
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let codex_home = home.join("codex");
        std::fs::create_dir(&codex_home).unwrap();
        let script = r#"
import json, os, sqlite3, sys
home = os.environ['CODEX_HOME']
thread_id = '00000000-0000-4000-8000-000000000001'
session_dir = os.path.join(home, 'sessions', '2026', '10', '29')
os.makedirs(session_dir)
rollout = os.path.join(session_dir, 'rollout-2026-10-29T00-00-00-' + thread_id + '.jsonl')
transcript = open(rollout, 'w+')
transcript.write(json.dumps({'type':'session_meta', 'payload':{'id':thread_id, 'cwd':os.environ['HOME']}}) + '\n')
transcript.flush()
db = sqlite3.connect(os.path.join(home, 'goals_1.sqlite'))
db.execute('pragma journal_mode=wal')
db.execute('create table _sqlx_migrations (version bigint primary key, description text not null, installed_on timestamp not null default current_timestamp, success boolean not null, checksum blob not null, execution_time bigint not null)')
for version, description, checksum in [(1, 'thread goals', '8CE6280244A0B4B39C7C37C7F67AFF1670EC963D2401CDF2ABCFEA9904102DB6CD99B7BEDF366267791A45EB12F17713'), (2, 'thread goal continuation deferrals', 'F3FF7DE2FD89914820BEF371081E885FD17DF18A6FBDC432399A83255A8A01AA32FB7EEA5E4DE21A17BDA3DA16C49584')]:
    db.execute('insert into _sqlx_migrations (version, description, success, checksum, execution_time) values (?, ?, 1, ?, 0)', (version, description, bytes.fromhex(checksum)))
db.execute("create table thread_goals (thread_id text primary key not null, goal_id text not null, objective text not null, status text not null check(status in ('active', 'paused', 'blocked', 'usage_limited', 'budget_limited', 'complete')), token_budget integer, tokens_used integer not null default 0, time_used_seconds integer not null default 0, created_at_ms integer not null, updated_at_ms integer not null)")
db.execute('insert into thread_goals (thread_id, goal_id, objective, status, created_at_ms, updated_at_ms) values (?, ?, ?, ?, ?, ?)', (thread_id, 'synthetic-goal', 'synthetic objective', 'paused', 1, 1))
db.commit()
print('ready', flush=True)
sys.stdin.read()
"#;
        let mut provider = std::process::Command::new("python3")
            .args(["-u", "-c", script])
            .current_dir(home)
            .env("HOME", home)
            .env("CODEX_HOME", &codex_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = provider.stdout.take().unwrap();
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        let paths = ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"];
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(codex_home.join(path)).unwrap())
            .collect();
        let before_times: Vec<_> = paths
            .iter()
            .map(|path| {
                let metadata = std::fs::metadata(codex_home.join(path)).unwrap();
                (metadata.accessed().ok(), metadata.modified().ok())
            })
            .collect();
        let lookup =
            read_sandboxed_goal(&codex_home, "00000000-0000-4000-8000-000000000001").unwrap();
        assert!(
            matches!(lookup, GoalLookup::Present { phase: ilium_agent_session::goal_evidence::GoalPhase::Paused, objective } if objective == "synthetic objective")
        );
        let after_times: Vec<_> = paths
            .iter()
            .map(|path| {
                let metadata = std::fs::metadata(codex_home.join(path)).unwrap();
                (metadata.accessed().ok(), metadata.modified().ok())
            })
            .collect();
        let after: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(codex_home.join(path)).unwrap())
            .collect();
        assert_eq!(before, after);
        assert_eq!(before_times, after_times);
        drop(provider.stdin.take());
        assert!(provider.wait().unwrap().success());
    }

    #[test]
    fn reader_requires_the_supported_process_identity_platform() {
        assert_eq!(
            CodexGoalService::acquisition_is_qualified(),
            cfg!(all(target_os = "linux", target_pointer_width = "64"))
        );
    }
}
