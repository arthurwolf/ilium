//! Server-owned long-task progress probing.
//!
//! After preflight and registration this coordinator is the sole recurring
//! poller. Task-reported failure remains distinct from inability to observe a
//! task through its probe.

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ilium_core::{
    NodeId, PaneProgress, ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus,
};
use ilium_ipc::{
    ProgressMonitorPreflight, ProgressMonitorRejection, ProgressMonitorRejectionCode, ServerEvent,
};
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;

use crate::state::ServerState;

pub const MIN_INTERVAL: Duration = Duration::from_secs(1);
pub const MAX_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAXIMUM_PROGRESS_COMMAND_BYTES: usize = 16 * 1024;
pub const MAXIMUM_PROBE_STDOUT_BYTES: usize = 64 * 1024;
pub const MAXIMUM_PROBE_STDERR_BYTES: usize = 16 * 1024;
pub const MAXIMUM_CONSECUTIVE_OBSERVATION_FAILURES: u32 = 3;

#[derive(Debug, Clone, Copy)]
pub struct ProbeExecutionLimits {
    pub timeout: Duration,
    pub maximum_stdout_bytes: usize,
    pub maximum_stderr_bytes: usize,
}

impl Default for ProbeExecutionLimits {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_PROBE_TIMEOUT,
            maximum_stdout_bytes: MAXIMUM_PROBE_STDOUT_BYTES,
            maximum_stderr_bytes: MAXIMUM_PROBE_STDERR_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressProbeFailureKind {
    InvalidRequest,
    Spawn,
    Timeout,
    Exit,
    OutputTooLarge,
    Io,
    InvalidReport,
    JobIdentityChanged,
    InvalidStatusTransition,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProgressProbeError {
    pub kind: ProgressProbeFailureKind,
    pub message: String,
}

impl ProgressProbeError {
    pub fn rejection(&self) -> ProgressMonitorRejection {
        let code = match self.kind {
            ProgressProbeFailureKind::InvalidRequest => {
                ProgressMonitorRejectionCode::InvalidRequest
            }
            ProgressProbeFailureKind::Spawn => ProgressMonitorRejectionCode::ProbeSpawnFailed,
            ProgressProbeFailureKind::Timeout => ProgressMonitorRejectionCode::ProbeTimedOut,
            ProgressProbeFailureKind::Exit => ProgressMonitorRejectionCode::ProbeExitedNonZero,
            ProgressProbeFailureKind::OutputTooLarge => {
                ProgressMonitorRejectionCode::ProbeOutputTooLarge
            }
            ProgressProbeFailureKind::Io => ProgressMonitorRejectionCode::ProbeIoFailed,
            ProgressProbeFailureKind::InvalidReport
            | ProgressProbeFailureKind::JobIdentityChanged
            | ProgressProbeFailureKind::InvalidStatusTransition => {
                ProgressMonitorRejectionCode::InvalidProbeReport
            }
        };
        ProgressMonitorRejection {
            code,
            message: self.message.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressMonitorRegistration {
    pub monitor_id: u64,
    pub pane_id: NodeId,
    pub command: String,
    pub interval: Duration,
    pub initial_progress: PaneProgress,
}

impl ProgressMonitorRegistration {
    pub fn validate(&self) -> Result<(), ProgressProbeError> {
        validate_command(&self.command)?;
        if self.monitor_id == 0 || self.initial_progress.monitor_id != self.monitor_id {
            return Err(probe_error(
                ProgressProbeFailureKind::InvalidRequest,
                "monitor ID must be non-zero and match the accepted initial progress",
            ));
        }
        if !(MIN_INTERVAL..=MAX_INTERVAL).contains(&self.interval) {
            return Err(probe_error(
                ProgressProbeFailureKind::InvalidRequest,
                format!(
                    "progress interval must be between {} and {} seconds",
                    MIN_INTERVAL.as_secs(),
                    MAX_INTERVAL.as_secs()
                ),
            ));
        }
        self.initial_progress.validate().map_err(|error| {
            probe_error(
                ProgressProbeFailureKind::InvalidReport,
                format!("initial progress is invalid: {error}"),
            )
        })
    }
}

/// Per-pane generation source used to fence late work after replacement.
#[derive(Debug, Clone, Default)]
pub struct ProgressMonitorGeneration(Arc<AtomicU64>);

impl ProgressMonitorGeneration {
    pub fn activate(&self, monitor_id: u64) -> Result<ProgressMonitorFence, ProgressProbeError> {
        if monitor_id == 0 {
            return Err(probe_error(
                ProgressProbeFailureKind::InvalidRequest,
                "monitor ID must be non-zero",
            ));
        }
        self.0.store(monitor_id, Ordering::Release);
        Ok(ProgressMonitorFence {
            active_monitor_id: Arc::clone(&self.0),
            monitor_id,
        })
    }

    pub fn clear_if_current(&self, monitor_id: u64) -> bool {
        self.0
            .compare_exchange(monitor_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn current(&self) -> Option<u64> {
        match self.0.load(Ordering::Acquire) {
            0 => None,
            monitor_id => Some(monitor_id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressMonitorFence {
    active_monitor_id: Arc<AtomicU64>,
    monitor_id: u64,
}

impl ProgressMonitorFence {
    pub fn is_current(&self) -> bool {
        self.active_monitor_id.load(Ordering::Acquire) == self.monitor_id
    }

    pub const fn monitor_id(&self) -> u64 {
        self.monitor_id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProgressMonitorOutcome {
    TaskTerminal(PaneProgress),
    MonitorFailed {
        progress: PaneProgress,
        error: ProgressProbeError,
    },
    Disabled,
    Superseded,
    PaneUnavailable,
}

/// Executes exactly one bounded server-equivalent probe.
pub async fn preflight(command: &str) -> Result<ProgressMonitorPreflight, ProgressProbeError> {
    let checked_at_unix_millis = unix_millis();
    let report = run_probe(command, ProbeExecutionLimits::default()).await?;
    Ok(ProgressMonitorPreflight {
        report,
        checked_at_unix_millis,
    })
}

/// Starts observation after the caller has committed `initial_progress`.
/// The returned task yields one terminal coordinator outcome for delivery.
pub fn spawn(
    state: Arc<ServerState>,
    registration: ProgressMonitorRegistration,
    fence: ProgressMonitorFence,
) -> JoinHandle<ProgressMonitorOutcome> {
    tokio::spawn(async move {
        if registration.validate().is_err() || !fence.is_current() {
            return ProgressMonitorOutcome::Superseded;
        }
        let mut latest = registration.initial_progress;
        if latest.is_terminal() {
            return ProgressMonitorOutcome::TaskTerminal(latest);
        }
        let expected_job_id = latest.report.job_id.clone();
        let mut previous_status = latest.report.status;
        let mut consecutive_failures = 0_u32;

        loop {
            tokio::time::sleep(registration.interval).await;
            if !fence.is_current() {
                return ProgressMonitorOutcome::Superseded;
            }
            if !state.is_progress_monitor_enabled() {
                return ProgressMonitorOutcome::Disabled;
            }

            match run_probe(&registration.command, ProbeExecutionLimits::default()).await {
                Ok(report) => {
                    if !fence.is_current() {
                        return ProgressMonitorOutcome::Superseded;
                    }
                    let report =
                        match validate_next_report(&expected_job_id, previous_status, report) {
                            Ok(report) => report,
                            Err(error) => {
                                if let Some(outcome) = record_observation_failure(
                                    &state,
                                    registration.pane_id,
                                    &fence,
                                    &mut latest,
                                    &mut consecutive_failures,
                                    error,
                                )
                                .await
                                {
                                    return outcome;
                                }
                                continue;
                            }
                        };
                    consecutive_failures = 0;
                    previous_status = report.status;
                    latest = PaneProgress::new(registration.monitor_id, report, unix_millis())
                        .expect("validated report and ID form valid progress");
                    if !publish_progress(&state, registration.pane_id, &fence, &latest).await {
                        return if fence.is_current() {
                            ProgressMonitorOutcome::PaneUnavailable
                        } else {
                            ProgressMonitorOutcome::Superseded
                        };
                    }
                    if latest.is_terminal() {
                        return ProgressMonitorOutcome::TaskTerminal(latest);
                    }
                }
                Err(error) => {
                    if let Some(outcome) = record_observation_failure(
                        &state,
                        registration.pane_id,
                        &fence,
                        &mut latest,
                        &mut consecutive_failures,
                        error,
                    )
                    .await
                    {
                        return outcome;
                    }
                }
            }
        }
    })
}

async fn record_observation_failure(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    fence: &ProgressMonitorFence,
    latest: &mut PaneProgress,
    consecutive_failures: &mut u32,
    error: ProgressProbeError,
) -> Option<ProgressMonitorOutcome> {
    if !fence.is_current() {
        return Some(ProgressMonitorOutcome::Superseded);
    }
    *consecutive_failures = consecutive_failures.saturating_add(1);
    let last_error = bounded_monitor_error(&error.message);
    latest.monitor_health = if *consecutive_failures >= MAXIMUM_CONSECUTIVE_OBSERVATION_FAILURES {
        ProgressMonitorHealth::Failed {
            consecutive_failures: *consecutive_failures,
            last_error,
        }
    } else {
        ProgressMonitorHealth::Degraded {
            consecutive_failures: *consecutive_failures,
            last_error,
        }
    };
    if !publish_progress(state, pane_id, fence, latest).await {
        return Some(if fence.is_current() {
            ProgressMonitorOutcome::PaneUnavailable
        } else {
            ProgressMonitorOutcome::Superseded
        });
    }
    latest
        .monitor_health
        .is_failed()
        .then(|| ProgressMonitorOutcome::MonitorFailed {
            progress: latest.clone(),
            error,
        })
}

async fn publish_progress(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    fence: &ProgressMonitorFence,
    progress: &PaneProgress,
) -> bool {
    if !fence.is_current() {
        return false;
    }
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    if !fence.is_current() {
        return false;
    }
    let Some(crate::pane::PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return false;
    };
    if !runtime.update_progress_monitor_progress(fence.monitor_id(), progress.clone()) {
        return false;
    }
    if tree
        .set_pane_progress(pane_id, Some(progress.clone()))
        .is_err()
    {
        return false;
    }
    drop(panes);
    drop(tree);
    state.request_snapshot_save();
    state.broadcast(ServerEvent::PaneProgressChanged {
        pane_id,
        progress: Some(progress.clone()),
    });
    true
}

fn validate_next_report(
    expected_job_id: &str,
    previous_status: ProgressTaskStatus,
    report: ProgressTaskReport,
) -> Result<ProgressTaskReport, ProgressProbeError> {
    if report.job_id != expected_job_id {
        return Err(probe_error(
            ProgressProbeFailureKind::JobIdentityChanged,
            format!(
                "probe job_id changed from {expected_job_id:?} to {:?}",
                report.job_id
            ),
        ));
    }
    let transition_is_valid = match previous_status {
        ProgressTaskStatus::NotStartedYet => true,
        ProgressTaskStatus::Running => report.status != ProgressTaskStatus::NotStartedYet,
        ProgressTaskStatus::Error | ProgressTaskStatus::Done => false,
    };
    if !transition_is_valid {
        return Err(probe_error(
            ProgressProbeFailureKind::InvalidStatusTransition,
            format!(
                "invalid progress status transition from {previous_status:?} to {:?}",
                report.status
            ),
        ));
    }
    Ok(report)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProgressReport {
    job_id: String,
    status: ProgressTaskStatus,
    percent: f64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    error: Option<String>,
}

async fn run_probe(
    command: &str,
    limits: ProbeExecutionLimits,
) -> Result<ProgressTaskReport, ProgressProbeError> {
    validate_command(command)?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut child_command = tokio::process::Command::new(&shell);
    child_command
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    ilium_platform::process_control::prepare_process_tree(child_command.as_std_mut());
    let mut child = child_command.spawn().map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::Spawn,
            format!("progress probe failed to spawn: {error}"),
        )
    })?;
    let process_id = child.id().ok_or_else(|| {
        probe_error(
            ProgressProbeFailureKind::Spawn,
            "progress probe spawned without a process ID",
        )
    })?;
    let mut process_tree =
        match ilium_platform::process_control::ProcessTreeGuard::attach(process_id) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                // Never run an unattended probe that cannot be bounded as a
                // process tree. kill_on_drop is a backstop; this explicit kill
                // also reaps the direct child before returning the rejection.
                let _ = child.kill().await;
                return Err(probe_error(
                    ProgressProbeFailureKind::Spawn,
                    format!("progress probe process tree could not be secured: {error}"),
                ));
            }
        };

    let stdout = child.stdout.take().ok_or_else(|| {
        probe_error(
            ProgressProbeFailureKind::Io,
            "progress probe stdout was unavailable",
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        probe_error(
            ProgressProbeFailureKind::Io,
            "progress probe stderr was unavailable",
        )
    })?;

    // Bound the complete operation, not merely the direct shell's wait. A
    // shell can exit after starting a background descendant that keeps the
    // inherited stdout/stderr pipes open forever; those readers are part of
    // the probe and must share its timeout and cancellation scope.
    let completed = tokio::time::timeout(limits.timeout, async {
        tokio::join!(
            child.wait(),
            read_bounded(stdout, limits.maximum_stdout_bytes),
            read_bounded(stderr, limits.maximum_stderr_bytes),
        )
    })
    .await;

    let (status, stdout, stderr) = match completed {
        Ok((status, stdout, stderr)) => (status, stdout, stderr),
        Err(_) => {
            let tree_termination_error = process_tree.terminate().err();
            // The group/job termination above is the authoritative descendant
            // cleanup. Kill-and-wait remains useful for deterministically
            // reaping the direct child held by Tokio.
            let _ = child.kill().await;
            let cleanup_detail = tree_termination_error
                .map(|error| format!("; process-tree cleanup also failed: {error}"))
                .unwrap_or_default();
            return Err(probe_error(
                ProgressProbeFailureKind::Timeout,
                format!(
                    "progress probe exceeded its {:?} timeout{cleanup_detail}",
                    limits.timeout
                ),
            ));
        }
    };

    // Even a successfully exited shell may have detached ordinary background
    // descendants. Closing the termination unit here prevents them escaping a
    // completed probe. On an async cancellation, ProcessTreeGuard::drop takes
    // the same path automatically.
    process_tree.terminate().map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::Io,
            format!("progress probe process tree could not be finalized: {error}"),
        )
    })?;

    let status = status.map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::Io,
            format!("progress probe failed while waiting: {error}"),
        )
    })?;
    let stdout = stdout.map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::Io,
            format!("progress probe stdout read failed: {error}"),
        )
    })?;
    let stderr = stderr.map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::Io,
            format!("progress probe stderr read failed: {error}"),
        )
    })?;

    if stdout.exceeded || stderr.exceeded {
        return Err(probe_error(
            ProgressProbeFailureKind::OutputTooLarge,
            format!(
                "progress probe output exceeded its limits (stdout {} bytes, stderr {} bytes)",
                limits.maximum_stdout_bytes, limits.maximum_stderr_bytes
            ),
        ));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr.bytes);
        return Err(probe_error(
            ProgressProbeFailureKind::Exit,
            if detail.trim().is_empty() {
                format!("progress probe exited with {status}")
            } else {
                format!("progress probe exited with {status}: {}", detail.trim())
            },
        ));
    }

    let raw: RawProgressReport = serde_json::from_slice(&stdout.bytes).map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::InvalidReport,
            format!("progress probe did not emit exactly one valid JSON report: {error}"),
        )
    })?;
    if !raw.percent.is_finite() || !(0.0..=100.0).contains(&raw.percent) {
        return Err(probe_error(
            ProgressProbeFailureKind::InvalidReport,
            "progress percent must be finite and in 0..=100",
        ));
    }
    ProgressTaskReport::new(
        raw.job_id,
        raw.status,
        raw.percent as f32,
        raw.message,
        raw.error,
    )
    .map_err(|error| {
        probe_error(
            ProgressProbeFailureKind::InvalidReport,
            format!("progress probe report is invalid: {error}"),
        )
    })
}

struct BoundedOutput {
    bytes: Vec<u8>,
    exceeded: bool,
}

async fn read_bounded<R>(mut reader: R, maximum_bytes: usize) -> std::io::Result<BoundedOutput>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let retained_limit = maximum_bytes.saturating_add(1);
    let mut bytes = Vec::with_capacity(maximum_bytes.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if bytes.len() < retained_limit {
            let retained = (retained_limit - bytes.len()).min(read);
            bytes.extend_from_slice(&buffer[..retained]);
        }
    }
    let exceeded = bytes.len() > maximum_bytes;
    if exceeded {
        bytes.truncate(maximum_bytes);
    }
    Ok(BoundedOutput { bytes, exceeded })
}

fn validate_command(command: &str) -> Result<(), ProgressProbeError> {
    if command.trim().is_empty() {
        return Err(probe_error(
            ProgressProbeFailureKind::InvalidRequest,
            "progress probe command must not be empty",
        ));
    }
    if command.len() > MAXIMUM_PROGRESS_COMMAND_BYTES {
        return Err(probe_error(
            ProgressProbeFailureKind::InvalidRequest,
            format!(
                "progress probe command exceeds its {}-byte limit",
                MAXIMUM_PROGRESS_COMMAND_BYTES
            ),
        ));
    }
    if command.contains('\0') {
        return Err(probe_error(
            ProgressProbeFailureKind::InvalidRequest,
            "progress probe command contains a NUL byte",
        ));
    }
    Ok(())
}

fn probe_error(kind: ProgressProbeFailureKind, message: impl Into<String>) -> ProgressProbeError {
    ProgressProbeError {
        kind,
        message: message.into(),
    }
}

fn bounded_monitor_error(message: &str) -> String {
    let maximum = ilium_core::MAXIMUM_PROGRESS_MONITOR_ERROR_BYTES;
    let sanitized: String = message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if sanitized.len() <= maximum {
        return sanitized;
    }
    let mut end = maximum;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_string()
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_json(json: &str) -> String {
        format!("printf '%s' {}", shell_single_quote(json))
    }

    fn shell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    #[tokio::test]
    async fn preflight_accepts_all_exact_wire_statuses_and_normalizes_done() {
        for status in ["not-started-yet", "running", "error", "done"] {
            let error = if status == "error" {
                r#","error":"render failed""#
            } else {
                ""
            };
            let json = format!(
                r#"{{"job_id":"render-42","status":"{status}","percent":42,"message":"phase 2"{error}}}"#
            );
            let result = preflight(&shell_json(&json)).await.unwrap();
            assert_eq!(
                result.report.status,
                match status {
                    "not-started-yet" => ProgressTaskStatus::NotStartedYet,
                    "running" => ProgressTaskStatus::Running,
                    "error" => ProgressTaskStatus::Error,
                    "done" => ProgressTaskStatus::Done,
                    _ => unreachable!(),
                }
            );
            if status == "done" {
                assert_eq!(result.report.percent, 100.0);
            }
        }
    }

    #[tokio::test]
    async fn probe_rejects_trailing_json_unknown_fields_and_invalid_contracts() {
        for json in [
            r#"{"job_id":"job","status":"running","percent":5} {}"#,
            r#"{"job_id":"job","status":"running","percent":5,"surprise":true}"#,
            r#"{"job_id":"job","status":"error","percent":5}"#,
            r#"{"job_id":"job","status":"running","percent":101}"#,
            r#"{"job_id":"job","status":"running","percent":5,"message":"bad\nline"}"#,
        ] {
            let error = preflight(&shell_json(json)).await.unwrap_err();
            assert_eq!(error.kind, ProgressProbeFailureKind::InvalidReport);
        }
    }

    #[tokio::test]
    async fn probe_bounds_stdout_and_execution_time() {
        let tiny = ProbeExecutionLimits {
            timeout: Duration::from_millis(100),
            maximum_stdout_bytes: 16,
            maximum_stderr_bytes: 16,
        };
        let oversized = run_probe("printf '12345678901234567'", tiny)
            .await
            .unwrap_err();
        assert_eq!(oversized.kind, ProgressProbeFailureKind::OutputTooLarge);
        let timed_out = run_probe("sleep 2", tiny).await.unwrap_err();
        assert_eq!(timed_out.kind, ProgressProbeFailureKind::Timeout);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_timeout_terminates_descendant_processes() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("descendant.pid");
        let command = command_with_waiting_descendant(&marker);
        let limits = ProbeExecutionLimits {
            timeout: Duration::from_millis(250),
            maximum_stdout_bytes: 1024,
            maximum_stderr_bytes: 1024,
        };

        let error = run_probe(&command, limits).await.unwrap_err();
        assert_eq!(error.kind, ProgressProbeFailureKind::Timeout);
        let descendant_process_id = read_descendant_process_id(&marker).await;
        wait_for_process_exit(descendant_process_id).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_probe_future_terminates_descendant_processes() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("descendant.pid");
        let command = command_with_waiting_descendant(&marker);
        let limits = ProbeExecutionLimits {
            timeout: Duration::from_secs(30),
            maximum_stdout_bytes: 1024,
            maximum_stderr_bytes: 1024,
        };
        let probe_task = tokio::spawn(async move { run_probe(&command, limits).await });
        let descendant_process_id = read_descendant_process_id(&marker).await;

        probe_task.abort();
        assert!(probe_task.await.unwrap_err().is_cancelled());
        wait_for_process_exit(descendant_process_id).await;
    }

    #[test]
    fn generation_fence_invalidates_replaced_and_cleared_monitors() {
        let generation = ProgressMonitorGeneration::default();
        let first = generation.activate(1).unwrap();
        assert!(first.is_current());
        let second = generation.activate(2).unwrap();
        assert!(!first.is_current());
        assert!(second.is_current());
        assert!(!generation.clear_if_current(1));
        assert!(generation.clear_if_current(2));
        assert!(!second.is_current());
    }

    #[test]
    fn identity_and_status_regressions_are_observation_failures() {
        let other = ProgressTaskReport::new(
            "other".to_string(),
            ProgressTaskStatus::Running,
            50.0,
            String::new(),
            None,
        )
        .unwrap();
        assert_eq!(
            validate_next_report("expected", ProgressTaskStatus::Running, other)
                .unwrap_err()
                .kind,
            ProgressProbeFailureKind::JobIdentityChanged
        );
        let regressed = ProgressTaskReport::new(
            "expected".to_string(),
            ProgressTaskStatus::NotStartedYet,
            0.0,
            String::new(),
            None,
        )
        .unwrap();
        assert_eq!(
            validate_next_report("expected", ProgressTaskStatus::Running, regressed)
                .unwrap_err()
                .kind,
            ProgressProbeFailureKind::InvalidStatusTransition
        );
    }

    #[cfg(unix)]
    fn command_with_waiting_descendant(marker: &std::path::Path) -> String {
        format!(
            "sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; wait",
            shell_single_quote(marker.to_str().expect("temporary path is UTF-8"))
        )
    }

    #[cfg(unix)]
    async fn read_descendant_process_id(marker: &std::path::Path) -> u32 {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(contents) = tokio::fs::read_to_string(marker).await {
                    if let Ok(process_id) = contents.trim().parse() {
                        break process_id;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("probe descendant wrote its process id")
    }

    #[cfg(unix)]
    async fn wait_for_process_exit(process_id: u32) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while ilium_platform::process_control::is_running(process_id) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("probe descendant {process_id} survived termination"));
    }
}
