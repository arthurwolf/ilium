//! Session-owned, two-tier Git status refresh for worktree panes.
//!
//! The coordinator owns every short-lived probe through its `JoinSet`. Dropping
//! the coordinator aborts those probes as well, including on a cancelled
//! server run. Only two probes may run concurrently, independent of pane count.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use ilium_core::{NodeId, PaneWorkspace};
use ilium_git::{HeadPaths, HeadProbe};
use ilium_ipc::WorkspaceGitStatus;
use tokio::sync::mpsc;
use tokio::task::{Id, JoinHandle, JoinSet};

use crate::state::ServerState;

const CHEAP_INTERVAL: Duration = Duration::from_secs(10);
const MAXIMUM_CONCURRENT_PROBES: usize = 2;
const CHEAP_TIMEOUT: Duration = Duration::from_secs(20);
const FULL_TIMEOUT: Duration = Duration::from_secs(40);
const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
struct PaneProbe {
    workspace: PaneWorkspace,
    cwd: PathBuf,
    head_paths: Option<HeadPaths>,
    fingerprint: Option<HeadFingerprint>,
    latest_status: Option<WorkspaceGitStatus>,
    next_cheap_at: Instant,
    retry_after: Instant,
    failures: u32,
}

impl PaneProbe {
    fn new(workspace: PaneWorkspace, cwd: PathBuf, now: Instant) -> Self {
        Self {
            workspace,
            cwd,
            head_paths: None,
            fingerprint: None,
            latest_status: None,
            next_cheap_at: now,
            retry_after: now,
            failures: 0,
        }
    }

    fn failed(&mut self, now: Instant) -> Duration {
        self.failures = self.failures.saturating_add(1);
        let delay = retry_delay(self.failures);
        self.retry_after = now + delay;
        delay
    }

    fn succeeded(&mut self, now: Instant) {
        self.failures = 0;
        self.retry_after = now;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    length: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeadFingerprint {
    head: Option<FileStamp>,
    head_log: Option<FileStamp>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbeKind {
    Cheap,
    Full,
}

struct ProbeResult {
    pane_id: NodeId,
    workspace: PaneWorkspace,
    kind: ProbeKind,
    outcome: Result<ProbeOutput, String>,
}

struct ProbeOutput {
    head_paths: Option<HeadPaths>,
    fingerprint: Option<HeadFingerprint>,
    status: Option<WorkspaceGitStatus>,
}

/// Start the single session-owned coordinator. The caller wraps this handle
/// in `AbortOnDropHandle` beside the other server-owned background loops.
pub(crate) fn spawn(
    state: Arc<ServerState>,
    full_requests: mpsc::Receiver<NodeId>,
) -> JoinHandle<()> {
    tokio::spawn(run(state, full_requests))
}

async fn run(state: Arc<ServerState>, mut full_requests: mpsc::Receiver<NodeId>) {
    let mut interval = tokio::time::interval(CHEAP_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut probes = HashMap::<NodeId, PaneProbe>::new();
    let mut full_pending = HashSet::<NodeId>::new();
    let mut in_flight = HashSet::<NodeId>::new();
    let mut task_panes = HashMap::<Id, NodeId>::new();
    let mut jobs = JoinSet::<ProbeResult>::new();
    let mut requests_open = true;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                reconcile_panes(&state, &mut probes, &mut full_pending).await;
            }
            request = full_requests.recv(), if requests_open => {
                match request {
                    Some(pane_id) => {
                        // A newly created pane can request full status before
                        // the next ten-second discovery tick.
                        if !probes.contains_key(&pane_id) {
                            reconcile_panes(&state, &mut probes, &mut full_pending).await;
                        }
                        if probes.contains_key(&pane_id) {
                            full_pending.insert(pane_id);
                        }
                    }
                    None => requests_open = false,
                }
            }
            completed = jobs.join_next_with_id(), if !jobs.is_empty() => {
                if let Some(result) = completed {
                    match result {
                        Ok((task_id, result)) => {
                            task_panes.remove(&task_id);
                            in_flight.remove(&result.pane_id);
                            accept_result(&state, &mut probes, &mut full_pending, result).await;
                        }
                        Err(error) => {
                            if let Some(pane_id) = task_panes.remove(&error.id()) {
                                in_flight.remove(&pane_id);
                                if let Some(probe) = probes.get_mut(&pane_id) {
                                    let delay = probe.failed(Instant::now());
                                    tracing::warn!(pane_id = pane_id.0, ?delay, %error,
                                        "Git status probe task failed; backing off");
                                }
                            }
                        }
                    }
                }
            }
        }

        schedule_jobs(
            &state,
            &mut probes,
            &full_pending,
            &mut in_flight,
            &mut task_panes,
            &mut jobs,
        );
    }
}

async fn reconcile_panes(
    state: &ServerState,
    probes: &mut HashMap<NodeId, PaneProbe>,
    full_pending: &mut HashSet<NodeId>,
) {
    let workspaces = {
        let tree = state.tree.read().await;
        tree.panes()
            .filter_map(|pane| {
                Some((
                    pane.id,
                    tree.pane_workspace(pane.id)?.clone(),
                    tree.pane_cwd(pane.id)?.to_path_buf(),
                ))
            })
            .collect::<Vec<_>>()
    };
    let active = workspaces
        .iter()
        .map(|(id, _, _)| *id)
        .collect::<HashSet<_>>();
    probes.retain(|id, _| active.contains(id));
    full_pending.retain(|id| active.contains(id));
    let now = Instant::now();
    for (pane_id, workspace, cwd) in workspaces {
        match probes.get_mut(&pane_id) {
            Some(probe) if probe.workspace == workspace && probe.cwd == cwd => {}
            _ => {
                probes.insert(pane_id, PaneProbe::new(workspace, cwd, now));
            }
        }
    }
}

fn schedule_jobs(
    state: &Arc<ServerState>,
    probes: &mut HashMap<NodeId, PaneProbe>,
    full_pending: &HashSet<NodeId>,
    in_flight: &mut HashSet<NodeId>,
    task_panes: &mut HashMap<Id, NodeId>,
    jobs: &mut JoinSet<ProbeResult>,
) {
    let now = Instant::now();
    while jobs.len() < MAXIMUM_CONCURRENT_PROBES {
        let next = probes
            .iter()
            .filter(|(id, probe)| !in_flight.contains(id) && probe.retry_after <= now)
            .find(|(id, _)| full_pending.contains(id))
            .map(|(id, _)| (*id, ProbeKind::Full))
            .or_else(|| {
                probes
                    .iter()
                    .find(|(id, probe)| {
                        !in_flight.contains(id)
                            && probe.retry_after <= now
                            && probe.next_cheap_at <= now
                    })
                    .map(|(id, _)| (*id, ProbeKind::Cheap))
            });
        let Some((pane_id, kind)) = next else { break };
        let Some(probe) = probes.get_mut(&pane_id) else {
            break;
        };
        if kind == ProbeKind::Cheap {
            probe.next_cheap_at = now + CHEAP_INTERVAL;
        }
        let task = run_probe(Arc::clone(state), pane_id, probe.clone(), kind);
        let handle = jobs.spawn(task);
        task_panes.insert(handle.id(), pane_id);
        in_flight.insert(pane_id);
    }
}

async fn run_probe(
    state: Arc<ServerState>,
    pane_id: NodeId,
    probe: PaneProbe,
    kind: ProbeKind,
) -> ProbeResult {
    let workspace = probe.workspace.clone();
    let (timeout, action) = match kind {
        ProbeKind::Cheap => (CHEAP_TIMEOUT, cheap_probe(state, probe)),
        ProbeKind::Full => {
            return ProbeResult {
                pane_id,
                workspace,
                kind,
                outcome: tokio::time::timeout(FULL_TIMEOUT, async {
                    let status = crate::workspace::refresh_pane_git_status(&state, pane_id).await?;
                    Ok(ProbeOutput {
                        head_paths: None,
                        fingerprint: None,
                        status: Some(status),
                    })
                })
                .await
                .unwrap_or_else(|_| {
                    Err(format!("full Git status timed out after {FULL_TIMEOUT:?}"))
                }),
            };
        }
    };
    ProbeResult {
        pane_id,
        workspace,
        kind,
        outcome: tokio::time::timeout(timeout, action)
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "cheap Git status timed out after {CHEAP_TIMEOUT:?}"
                ))
            }),
    }
}

async fn cheap_probe(state: Arc<ServerState>, probe: PaneProbe) -> Result<ProbeOutput, String> {
    let head_paths = match &probe.head_paths {
        Some(paths) => paths.clone(),
        None => match ilium_git::head_paths(&probe.cwd).await {
            Ok(paths) => paths,
            Err(error) => {
                if matches!(
                    crate::workspace::restore_target(
                        &state
                            .execution
                            .get()
                            .ok_or_else(|| "execution service is unavailable".to_string())?
                            .client,
                        &probe.cwd,
                        &probe.workspace,
                    )
                    .await,
                    crate::workspace::RestoreTarget::Missing(_)
                ) {
                    return Ok(ProbeOutput {
                        head_paths: None,
                        fingerprint: None,
                        status: Some(missing_status(&probe)),
                    });
                }
                return Err(format!("cannot resolve worktree HEAD files: {error}"));
            }
        },
    };
    let fingerprint = head_fingerprint(&head_paths).await?;
    if Some(fingerprint) == probe.fingerprint {
        return Ok(ProbeOutput {
            head_paths: Some(head_paths),
            fingerprint: Some(fingerprint),
            status: None,
        });
    }
    if fingerprint.head.is_none() {
        return Ok(ProbeOutput {
            head_paths: Some(head_paths),
            fingerprint: Some(fingerprint),
            status: Some(missing_status(&probe)),
        });
    }
    let head = ilium_git::head_probe(&probe.cwd)
        .await
        .map_err(|error| format!("cannot probe worktree HEAD: {error}"))?;
    Ok(ProbeOutput {
        head_paths: Some(head_paths),
        fingerprint: Some(fingerprint),
        status: Some(status_from_head(&probe, head)),
    })
}

async fn head_fingerprint(paths: &HeadPaths) -> Result<HeadFingerprint, String> {
    Ok(HeadFingerprint {
        head: file_stamp(&paths.head).await?,
        head_log: file_stamp(&paths.head_log).await?,
    })
}

async fn file_stamp(path: &Path) -> Result<Option<FileStamp>, String> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(Some(FileStamp {
            modified: metadata.modified().map_err(|error| {
                format!(
                    "cannot read modification time of {}: {error}",
                    path.display()
                )
            })?,
            length: metadata.len(),
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot stat {}: {error}", path.display())),
    }
}

fn status_from_head(probe: &PaneProbe, head: HeadProbe) -> WorkspaceGitStatus {
    let mut status = probe
        .latest_status
        .clone()
        .filter(|status| !status.missing)
        .unwrap_or_else(|| initial_status(&probe.workspace));
    let branch_changed = status.branch != head.branch || status.detached != head.detached;
    status.branch = head.branch;
    status.detached = head.detached;
    status.upstream = head.upstream;
    status.ahead = head.ahead;
    status.behind = head.behind;
    // Only a change against an observed HEAD fingerprint invalidates the
    // subject. The first cheap pass merely establishes that baseline; it may
    // follow a full refresh whose subject is still current.
    if probe.fingerprint.is_some() || branch_changed {
        status.last_commit_subject = None;
    }
    status.checked_at_unix_millis = now_unix_millis();
    status.missing = false;
    status
}

fn missing_status(probe: &PaneProbe) -> WorkspaceGitStatus {
    // No old dirty count or full-tier timestamp is authoritative after the
    // path disappears. If it returns, the cheap probe starts from unknown.
    let mut status = initial_status(&probe.workspace);
    status.missing = true;
    status.checked_at_unix_millis = now_unix_millis();
    status
}

fn initial_status(workspace: &PaneWorkspace) -> WorkspaceGitStatus {
    WorkspaceGitStatus {
        branch: Some(workspace.branch.clone()),
        detached: false,
        ahead: 0,
        behind: 0,
        staged: 0,
        modified: 0,
        untracked: 0,
        conflicted: 0,
        upstream: None,
        last_commit_subject: None,
        checked_at_unix_millis: now_unix_millis(),
        full_checked_at_unix_millis: None,
        missing: false,
    }
}

fn now_unix_millis() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn retry_delay(failures: u32) -> Duration {
    let multiplier = 1_u32
        .checked_shl(failures.saturating_sub(1).min(5))
        .unwrap_or(32);
    CHEAP_INTERVAL
        .saturating_mul(multiplier)
        .min(MAXIMUM_RETRY_DELAY)
}

async fn accept_result(
    state: &ServerState,
    probes: &mut HashMap<NodeId, PaneProbe>,
    full_pending: &mut HashSet<NodeId>,
    result: ProbeResult,
) {
    let Some(probe) = probes.get_mut(&result.pane_id) else {
        return;
    };
    if probe.workspace != result.workspace {
        return;
    }
    match result.outcome {
        Ok(output) => {
            probe.succeeded(Instant::now());
            if result.kind == ProbeKind::Full {
                full_pending.remove(&result.pane_id);
            }
            if let Some(paths) = output.head_paths {
                probe.head_paths = Some(paths);
            }
            if let Some(fingerprint) = output.fingerprint {
                probe.fingerprint = Some(fingerprint);
            }
            if let Some(status) = output.status {
                if status.missing {
                    // A recreated linked worktree may receive a different
                    // Git metadata directory. Resolve paths again next tick.
                    probe.head_paths = None;
                    probe.fingerprint = None;
                }
                state
                    .publish_pane_git_status(result.pane_id, &result.workspace, status.clone())
                    .await;
                // The publication method returns whether semantic fields
                // changed, not whether the status was accepted. Retain the
                // newest timestamp/full-tier marker even for equal values.
                probe.latest_status = Some(status);
            }
        }
        Err(error) => {
            let delay = probe.failed(Instant::now());
            tracing::warn!(pane_id = result.pane_id.0, ?delay, %error,
                "Git status probe failed; backing off");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> PaneWorkspace {
        PaneWorkspace {
            workspace_id: Some("workspace-1".into()),
            repo_common_dir: PathBuf::from("/tmp/repo/.git"),
            worktree_root: PathBuf::from("/tmp/worktree"),
            branch: "agent/original".into(),
            base_ref: "main".into(),
            base_commit: "abc".into(),
            created_by_ilium: true,
            created_at_unix: 0,
        }
    }

    #[test]
    fn retry_delay_doubles_and_caps() {
        assert_eq!(retry_delay(1), Duration::from_secs(10));
        assert_eq!(retry_delay(2), Duration::from_secs(20));
        assert_eq!(retry_delay(6), Duration::from_secs(300));
        assert_eq!(retry_delay(u32::MAX), Duration::from_secs(300));
    }

    #[test]
    fn cheap_head_refresh_preserves_last_full_dirty_counts() {
        let mut probe = PaneProbe::new(workspace(), PathBuf::from("/tmp/worktree"), Instant::now());
        let mut previous = initial_status(&probe.workspace);
        previous.modified = 3;
        previous.full_checked_at_unix_millis = Some(42);
        probe.latest_status = Some(previous);
        let refreshed = status_from_head(
            &probe,
            HeadProbe {
                branch: Some("agent/new".into()),
                detached: false,
                upstream: Some("origin/agent/new".into()),
                ahead: 2,
                behind: 1,
            },
        );
        assert_eq!(refreshed.branch.as_deref(), Some("agent/new"));
        assert_eq!(refreshed.modified, 3);
        assert_eq!(refreshed.full_checked_at_unix_millis, Some(42));
        assert_eq!(refreshed.ahead, 2);
        assert_eq!(refreshed.behind, 1);
    }

    #[test]
    fn initial_cheap_status_is_not_claimed_as_full() {
        let status = initial_status(&workspace());
        assert!(status.full_checked_at_unix_millis.is_none());
    }

    #[test]
    fn first_cheap_baseline_keeps_full_commit_subject_until_head_changes() {
        let mut probe = PaneProbe::new(workspace(), PathBuf::from("/tmp/worktree"), Instant::now());
        let mut previous = initial_status(&probe.workspace);
        previous.last_commit_subject = Some("current commit".into());
        previous.full_checked_at_unix_millis = Some(42);
        probe.latest_status = Some(previous);
        let head = HeadProbe {
            branch: Some("agent/original".into()),
            detached: false,
            upstream: None,
            ahead: 0,
            behind: 0,
        };
        let baseline = status_from_head(&probe, head.clone());
        assert_eq!(
            baseline.last_commit_subject.as_deref(),
            Some("current commit")
        );

        probe.fingerprint = Some(HeadFingerprint {
            head: Some(FileStamp {
                modified: SystemTime::UNIX_EPOCH,
                length: 1,
            }),
            head_log: None,
        });
        let changed = status_from_head(&probe, head);
        assert_eq!(changed.last_commit_subject, None);
    }

    #[test]
    fn disappearance_and_return_do_not_reuse_old_full_counts() {
        let mut probe = PaneProbe::new(workspace(), PathBuf::from("/tmp/worktree"), Instant::now());
        let mut previous = initial_status(&probe.workspace);
        previous.modified = 7;
        previous.last_commit_subject = Some("old commit".into());
        previous.full_checked_at_unix_millis = Some(42);
        probe.latest_status = Some(previous);
        let missing = missing_status(&probe);
        assert!(missing.missing);
        assert_eq!(missing.modified, 0);
        assert_eq!(missing.full_checked_at_unix_millis, None);
        assert_eq!(missing.last_commit_subject, None);

        probe.latest_status = Some(missing);
        let returned = status_from_head(
            &probe,
            HeadProbe {
                branch: Some("agent/returned".into()),
                detached: false,
                upstream: None,
                ahead: 0,
                behind: 0,
            },
        );
        assert!(!returned.missing);
        assert_eq!(returned.modified, 0);
        assert_eq!(returned.full_checked_at_unix_millis, None);
        assert_eq!(returned.last_commit_subject, None);
    }

    #[tokio::test]
    async fn fingerprint_detects_reflog_change_and_missing_head() {
        let directory = tempfile::tempdir().expect("temporary Git metadata directory");
        let head = directory.path().join("HEAD");
        let head_log = directory.path().join("logs-HEAD");
        tokio::fs::write(&head, b"ref: refs/heads/agent/one\n")
            .await
            .expect("write HEAD");
        tokio::fs::write(&head_log, b"first\n")
            .await
            .expect("write reflog");
        let paths = HeadPaths { head, head_log };
        let first = head_fingerprint(&paths).await.expect("first fingerprint");
        assert!(first.head.is_some());
        tokio::fs::write(&paths.head_log, b"first\nsecond\n")
            .await
            .expect("update reflog");
        let changed = head_fingerprint(&paths).await.expect("changed fingerprint");
        assert_ne!(changed, first);
        tokio::fs::remove_file(&paths.head)
            .await
            .expect("remove HEAD");
        let missing = head_fingerprint(&paths)
            .await
            .expect("missing HEAD fingerprint");
        assert!(missing.head.is_none());
    }
}
