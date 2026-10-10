//! `ilium progress wait` / `ilium wait`: blocks the calling command until one
//! progress monitor settles, so an agent can wait the way it waits for any
//! other long command instead of polling or expecting a typed notification.
//!
//! A pane can hold several monitors. Without an explicit monitor ID the wait
//! picks the only unsettled one (or the only one at all); when that choice is
//! ambiguous it refuses and lists the IDs instead of guessing.
//!
//! The server holds the request open and, while this process stays connected,
//! hands the outcome to it instead of typing a notification into the agent's
//! composer. If this process dies, the server falls back to that
//! notification. A periodic status check covers the cases the held request
//! cannot see (pane closed, monitoring switched off, an older server).

use std::time::{Duration, Instant};

use ilium_client::connection::Connection;
use ilium_core::{NodeId, PaneProgress, ProgressTaskStatus};
use ilium_ipc::{ClientRequest, ProgressWaitEnd, ServerEvent};

use crate::{json_string, next_progress_request_id, pane_progress_json, CliError};

/// Exit statuses of a wait. 1 (CLI or connection failure) and 2 (usage) keep
/// their usual meaning; the rest name the monitor outcome.
pub(crate) const EXIT_TASK_DONE: u8 = 0;
pub(crate) const EXIT_TASK_ERROR: u8 = 3;
pub(crate) const EXIT_MONITOR_FAILED: u8 = 4;
pub(crate) const EXIT_MONITOR_ENDED: u8 = 5;
pub(crate) const EXIT_WAIT_TIMED_OUT: u8 = 6;

/// How often the wait double-checks the monitor through a status request.
const LIVENESS_CHECK_INTERVAL: Duration = Duration::from_secs(15);
/// After a status check shows the monitor settled, how long to give the
/// server's held reply to arrive before reporting the outcome from status.
const SETTLED_REPLY_GRACE: Duration = Duration::from_secs(3);

/// How one wait ended, ready to print.
pub(crate) struct WaitReport {
    pub(crate) pane_id: NodeId,
    pub(crate) monitor_id: Option<u64>,
    pub(crate) end: WaitReportEnd,
    pub(crate) progress: Option<PaneProgress>,
    pub(crate) composer_notice_suppressed: bool,
    pub(crate) waited: Duration,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaitReportEnd {
    Settled,
    Cleared,
    NoMonitor,
    TimedOut,
}

/// Why `wait` without a monitor ID could not choose one.
pub(crate) struct AmbiguousMonitor {
    pub(crate) monitors: Vec<PaneProgress>,
}

impl AmbiguousMonitor {
    pub(crate) fn message(&self) -> String {
        let listed = self
            .monitors
            .iter()
            .map(|progress| {
                format!(
                    "{} (job {}, {:.0}%)",
                    progress.monitor_id, progress.report.job_id, progress.report.percent
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "this pane has several progress monitors: {listed}. Name one: `ilium progress wait \
             <monitor_id>`"
        )
    }
}

/// How `wait` resolved the monitor to wait for.
pub(crate) enum MonitorChoice {
    Chosen(u64),
    NoMonitor,
    Ambiguous(AmbiguousMonitor),
}

/// Picks the monitor a wait without an explicit ID refers to: the only
/// unsettled monitor, else the only monitor; anything else is ambiguous.
pub(crate) fn choose_monitor(monitors: &[PaneProgress]) -> MonitorChoice {
    let mut unsettled = monitors.iter().filter(|progress| !is_settled(progress));
    match (unsettled.next(), unsettled.next()) {
        (Some(progress), None) => return MonitorChoice::Chosen(progress.monitor_id),
        (Some(_), Some(_)) => {
            return MonitorChoice::Ambiguous(AmbiguousMonitor {
                monitors: monitors
                    .iter()
                    .filter(|progress| !is_settled(progress))
                    .cloned()
                    .collect(),
            })
        }
        (None, _) => {}
    }
    match monitors {
        [] => MonitorChoice::NoMonitor,
        [only] => MonitorChoice::Chosen(only.monitor_id),
        _ => MonitorChoice::Ambiguous(AmbiguousMonitor {
            monitors: monitors.to_vec(),
        }),
    }
}

impl WaitReport {
    fn outcome_name(&self) -> &'static str {
        match self.end {
            WaitReportEnd::Settled => match self.progress.as_ref() {
                Some(progress) if progress.monitor_health.is_failed() => "monitor-failed",
                Some(progress) if progress.report.status == ProgressTaskStatus::Done => "done",
                Some(progress) if progress.report.status == ProgressTaskStatus::Error => "error",
                _ => "monitor-failed",
            },
            WaitReportEnd::Cleared => "cleared",
            WaitReportEnd::NoMonitor => "no-monitor",
            WaitReportEnd::TimedOut => "still-running",
        }
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self.outcome_name() {
            "done" => EXIT_TASK_DONE,
            "error" => EXIT_TASK_ERROR,
            "monitor-failed" => EXIT_MONITOR_FAILED,
            "still-running" => EXIT_WAIT_TIMED_OUT,
            _ => EXIT_MONITOR_ENDED,
        }
    }

    fn next_step(&self) -> &'static str {
        match self.outcome_name() {
            "done" => "The task finished. Verify its output, then continue.",
            "error" => "The task reported failure. Inspect its log or error, fix, and retry.",
            "monitor-failed" => {
                "The probe stopped working, so the task outcome is unknown. Check the task \
                 directly; do not assume success."
            }
            "cleared" => {
                "The monitor was cleared, or is not registered on this pane. Check `ilium \
                 progress status`."
            }
            "no-monitor" => {
                "This pane has no progress monitor. Register one with `ilium progress set`."
            }
            _ => "The task is still running. Run the same wait command again to keep waiting.",
        }
    }

    pub(crate) fn print(&self) {
        let monitor_id = self
            .monitor_id
            .map_or_else(|| "null".to_string(), |monitor_id| monitor_id.to_string());
        let progress = self
            .progress
            .as_ref()
            .map_or_else(|| "null".to_string(), pane_progress_json);
        let composer_notice = if self.composer_notice_suppressed {
            "suppressed"
        } else if self.end == WaitReportEnd::Settled {
            "may-also-arrive"
        } else {
            "none"
        };
        println!(
            "{{\"type\":\"progress_wait\",\"pane_id\":{},\"monitor_id\":{monitor_id},\"outcome\":{},\"exit_code\":{},\"waited_seconds\":{},\"composer_notice\":{},\"next\":{},\"progress\":{progress}}}",
            self.pane_id.0,
            json_string(self.outcome_name()),
            self.exit_code(),
            self.waited.as_secs(),
            json_string(composer_notice),
            json_string(self.next_step()),
        );
    }
}

fn is_settled(progress: &PaneProgress) -> bool {
    progress.is_terminal() || progress.monitor_health.is_failed()
}

/// Reads every monitor registered on the pane, in registration order.
pub(crate) async fn current_monitors(
    connection: &mut Connection,
    pane_id: NodeId,
) -> Result<Vec<PaneProgress>, CliError> {
    let request_id = next_progress_request_id();
    send(
        connection,
        ClientRequest::GetPaneProgressMonitorStatus {
            request_id,
            pane_id,
        },
    )
    .await?;
    tokio::time::timeout(crate::PROGRESS_REQUEST_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _retention) = event.into_parts();
            if let ServerEvent::ProgressMonitorStatusReported {
                request_id: response_id,
                result,
                ..
            } = event
            {
                if response_id == request_id {
                    return result
                        .map(|status| status.progress_monitors)
                        .map_err(|rejection| CliError::ServerReportedError(rejection.message));
                }
            }
        }
        Err(CliError::ServerReportedError(
            "connection closed before the progress status arrived".to_string(),
        ))
    })
    .await
    .map_err(|_| {
        CliError::ServerReportedError("timed out waiting for the progress status".to_string())
    })?
}

async fn send(connection: &Connection, request: ClientRequest) -> Result<(), CliError> {
    connection.requests.send(request).await.map_err(|_| {
        CliError::ServerReportedError("connection closed before the request was sent".to_string())
    })
}

/// Waits for `monitor_id` (or the pane's only candidate monitor, see
/// `choose_monitor`) to settle.
pub(crate) async fn wait_for_monitor(
    connection: &mut Connection,
    pane_id: NodeId,
    monitor_id: Option<u64>,
    timeout: Option<Duration>,
) -> Result<Result<WaitReport, AmbiguousMonitor>, CliError> {
    let started = Instant::now();
    let report = |monitor_id, end, progress, composer_notice_suppressed| WaitReport {
        pane_id,
        monitor_id,
        end,
        progress,
        composer_notice_suppressed,
        waited: started.elapsed(),
    };
    let monitor_id = match monitor_id {
        Some(monitor_id) => monitor_id,
        None => match choose_monitor(&current_monitors(connection, pane_id).await?) {
            MonitorChoice::Chosen(monitor_id) => monitor_id,
            MonitorChoice::NoMonitor => {
                return Ok(Ok(report(None, WaitReportEnd::NoMonitor, None, false)))
            }
            MonitorChoice::Ambiguous(ambiguous) => return Ok(Err(ambiguous)),
        },
    };
    let wait_request_id = next_progress_request_id();
    send(
        connection,
        ClientRequest::WaitPaneProgressMonitor {
            request_id: wait_request_id,
            pane_id,
            monitor_id,
        },
    )
    .await?;

    let deadline = timeout.map(|timeout| tokio::time::Instant::now() + timeout);
    let mut next_check = tokio::time::Instant::now() + LIVENESS_CHECK_INTERVAL;
    let mut status_request_id = None;
    let mut settled_from_status: Option<PaneProgress> = None;
    // The latest report seen by a liveness check, returned on a timeout so the
    // caller sees how far the task got without a separate status request.
    let mut last_seen: Option<PaneProgress> = None;
    loop {
        let timeout_sleep = async {
            match deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            event = connection.events.recv() => {
                let Some(event) = event else {
                    return Err(CliError::ServerReportedError(
                        "the ilium server closed the connection while waiting. It may have \
                         restarted, or the running server is older than this `ilium` command \
                         (restart Ilium after installing a new build); the monitor's normal \
                         composer notification still applies"
                            .to_string(),
                    ));
                };
                let (event, _retention) = event.into_parts();
                match event {
                    ServerEvent::ProgressWaitCompleted { request_id, result, .. }
                        if request_id == wait_request_id =>
                    {
                        let outcome = result
                            .map_err(|rejection| CliError::ServerReportedError(rejection.message))?;
                        let end = match outcome.end {
                            ProgressWaitEnd::Settled => WaitReportEnd::Settled,
                            // An older server reports a replaced monitor as
                            // `Superseded`; for the waiter it is gone either way.
                            ProgressWaitEnd::Superseded | ProgressWaitEnd::Cleared => {
                                WaitReportEnd::Cleared
                            }
                        };
                        return Ok(Ok(report(
                            Some(monitor_id),
                            end,
                            outcome.progress,
                            outcome.composer_notice_suppressed,
                        )));
                    }
                    ServerEvent::ProgressMonitorStatusReported { request_id, result, .. }
                        if Some(request_id) == status_request_id =>
                    {
                        status_request_id = None;
                        let progress = match result {
                            Ok(status) => status
                                .progress_monitors
                                .into_iter()
                                .find(|progress| progress.monitor_id == monitor_id),
                            Err(rejection) => {
                                return Err(CliError::ServerReportedError(rejection.message))
                            }
                        };
                        match progress {
                            None => {
                                return Ok(Ok(report(
                                    Some(monitor_id),
                                    WaitReportEnd::Cleared,
                                    None,
                                    false,
                                )))
                            }
                            Some(progress) if is_settled(&progress) => {
                                if settled_from_status.is_some() {
                                    return Ok(Ok(report(
                                        Some(monitor_id),
                                        WaitReportEnd::Settled,
                                        Some(progress),
                                        false,
                                    )));
                                }
                                settled_from_status = Some(progress);
                                next_check = tokio::time::Instant::now() + SETTLED_REPLY_GRACE;
                            }
                            Some(progress) => last_seen = Some(progress),
                        }
                    }
                    _ => {}
                }
            }
            () = tokio::time::sleep_until(next_check), if status_request_id.is_none() => {
                let request_id = next_progress_request_id();
                send(
                    connection,
                    ClientRequest::GetPaneProgressMonitorStatus { request_id, pane_id },
                )
                .await?;
                status_request_id = Some(request_id);
                next_check = tokio::time::Instant::now() + LIVENESS_CHECK_INTERVAL;
            }
            () = timeout_sleep => {
                return Ok(Ok(report(
                    Some(monitor_id),
                    WaitReportEnd::TimedOut,
                    last_seen,
                    false,
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::{ProgressMonitorHealth, ProgressTaskReport};

    use super::*;

    fn monitor(monitor_id: u64, status: ProgressTaskStatus) -> PaneProgress {
        PaneProgress {
            monitor_id,
            report: ProgressTaskReport {
                job_id: format!("job-{monitor_id}"),
                status,
                percent: 10.0,
                message: String::new(),
                details: String::new(),
                error: (status == ProgressTaskStatus::Error).then(|| "failed".to_string()),
            },
            monitor_health: ProgressMonitorHealth::Healthy,
            last_observed_unix_millis: 1,
            attention: Default::default(),
        }
    }

    #[test]
    fn a_wait_without_an_id_picks_the_only_unsettled_monitor_or_refuses_to_guess() {
        assert!(matches!(choose_monitor(&[]), MonitorChoice::NoMonitor));
        let done = monitor(1, ProgressTaskStatus::Done);
        let running = monitor(2, ProgressTaskStatus::Running);
        let other_running = monitor(3, ProgressTaskStatus::Running);
        assert!(matches!(
            choose_monitor(std::slice::from_ref(&done)),
            MonitorChoice::Chosen(1)
        ));
        assert!(matches!(
            choose_monitor(&[done.clone(), running.clone()]),
            MonitorChoice::Chosen(2)
        ));
        let MonitorChoice::Ambiguous(ambiguous) =
            choose_monitor(&[done.clone(), running, other_running])
        else {
            panic!("two running monitors must be ambiguous");
        };
        assert_eq!(
            ambiguous
                .monitors
                .iter()
                .map(|progress| progress.monitor_id)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(ambiguous.message().contains("ilium progress wait"));
        assert!(matches!(
            choose_monitor(&[done, monitor(4, ProgressTaskStatus::Error)]),
            MonitorChoice::Ambiguous(_)
        ));
    }
}
