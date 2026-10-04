//! Read-only process/foreground preflights. Capture a PTY identity under a
//! registry guard, execute native inspection on the existing finite IO bank,
//! then compare the result with the live runtime under a new guard. A deadline
//! abandons the caller's proof, not the native callback's physical ownership.

use std::convert::Infallible;
use std::fmt;
use std::time::Duration;

use ilium_core::AgentProcessKey;
use ilium_detect::AgentIdentity;
use ilium_execution::{JobCost, Lane};
use ilium_pty::{PtySessionIdentity, PtyShellObserver};

use crate::execution::{ExecutionClient, ExecutionError};
use crate::pane::{TerminalOrigin, TerminalPaneRuntime};
use crate::state::ServerState;

const FOREGROUND_DEADLINE: Duration = Duration::from_millis(500);
const MAXIMUM_IDENTITY_BYTES: usize = 512 * 1024;
const PROBE_INPUT_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub(crate) struct ProbeRequest {
    session: PtySessionIdentity,
    shell: Option<PtyShellObserver>,
    identity: Option<AgentIdentity>,
    runtime_fence: Option<RuntimeFence>,
}

#[derive(Clone)]
struct RuntimeFence {
    agent_generation: u64,
    title_generation: u64,
    session_id: Option<String>,
    process: Option<AgentProcessKey>,
    input_cancel_generation: u64,
}

impl RuntimeFence {
    fn matches(&self, runtime: &TerminalPaneRuntime) -> bool {
        self.agent_generation == runtime.agent_generation
            && self.title_generation == runtime.title_generation
            && self.session_id == runtime.session_id
            && self.process == runtime.agent_process_key
            && self.input_cancel_generation == *runtime.agent_input_cancel.borrow()
    }
}

impl ProbeRequest {
    pub(crate) fn needs_inspection(&self) -> bool {
        self.shell.is_some() || self.identity.is_some()
    }

    pub(crate) fn for_shell(shell: PtyShellObserver) -> Self {
        Self {
            session: shell.identity().clone(),
            shell: Some(shell),
            identity: None,
            runtime_fence: None,
        }
    }

    pub(crate) fn for_runtime(runtime: &TerminalPaneRuntime) -> Self {
        Self {
            session: runtime.session.identity(),
            shell: matches!(&runtime.origin, TerminalOrigin::PlainShell)
                .then(|| runtime.session.shell_observer()),
            identity: runtime
                .agent_process_key
                .as_ref()
                .and_then(|_| runtime.detection_schedule.cached_identity.clone()),
            runtime_fence: Some(RuntimeFence {
                agent_generation: runtime.agent_generation,
                title_generation: runtime.title_generation,
                session_id: runtime.session_id.clone(),
                process: runtime.agent_process_key.clone(),
                input_cancel_generation: *runtime.agent_input_cancel.borrow(),
            }),
        }
    }
}

pub(crate) struct ProbeObservation {
    request: ProbeRequest,
    shell_owns_terminal: Option<bool>,
    identity_matches: Option<bool>,
}

impl ProbeObservation {
    pub(crate) fn same_session(&self, runtime: &TerminalPaneRuntime) -> bool {
        self.request.session == runtime.session.identity()
    }

    pub(crate) fn same_runtime(&self, runtime: &TerminalPaneRuntime) -> bool {
        self.same_session(runtime)
            && self
                .request
                .runtime_fence
                .as_ref()
                .is_none_or(|fence| fence.matches(runtime))
    }

    pub(crate) fn shell_owns_terminal(&self) -> Option<bool> {
        self.shell_owns_terminal
    }

    pub(crate) fn current_agent_identity(&self, runtime: &TerminalPaneRuntime) -> bool {
        self.same_runtime(runtime)
            && self.identity_matches == Some(true)
            && self.request.identity.as_ref() == runtime.detection_schedule.cached_identity.as_ref()
    }
}

#[derive(Debug)]
pub(crate) enum ProbeError {
    BankUnavailable,
    IdentityTooLarge,
    Deadline,
    Execution(ExecutionError<Infallible>),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BankUnavailable => formatter.write_str("server execution is unavailable"),
            Self::IdentityTooLarge => formatter.write_str("agent identity exceeds probe admission"),
            Self::Deadline => formatter.write_str("native foreground probe exceeded its deadline"),
            Self::Execution(error) => write!(formatter, "native foreground probe: {error}"),
        }
    }
}

pub(crate) async fn observe(
    state: &ServerState,
    request: ProbeRequest,
) -> Result<ProbeObservation, ProbeError> {
    let client = state
        .execution
        .get()
        .ok_or(ProbeError::BankUnavailable)?
        .client
        .clone();
    observe_with(
        &client,
        request,
        FOREGROUND_DEADLINE,
        move |shell, identity| {
            (
                shell.and_then(|observer| observer.shell_owns_terminal()),
                identity.map(|identity| {
                    crate::agent_identity_guard::matches_current_agent_identity(&identity)
                }),
            )
        },
    )
    .await
}

pub(crate) async fn observe_with<F>(
    client: &ExecutionClient,
    request: ProbeRequest,
    deadline: Duration,
    work: F,
) -> Result<ProbeObservation, ProbeError>
where
    F: FnOnce(Option<PtyShellObserver>, Option<AgentIdentity>) -> (Option<bool>, Option<bool>)
        + Send
        + 'static,
{
    let identity_bytes =
        request
            .identity
            .as_ref()
            .map_or(0, |identity| {
                identity
                    .process_name
                    .capacity()
                    .saturating_add(identity.matched_signature.capacity())
            })
            .saturating_add(request.runtime_fence.as_ref().map_or(0, |fence| {
                fence
                    .session_id
                    .as_ref()
                    .map_or(0, String::capacity)
                    .saturating_add(fence.process.as_ref().map_or(
                        0,
                        |process| match &process.class {
                            ilium_core::AgentClass::Other(name) => name.capacity(),
                            _ => 0,
                        },
                    ))
            }));
    if identity_bytes > MAXIMUM_IDENTITY_BYTES {
        return Err(ProbeError::IdentityTooLarge);
    }
    let shell = request.shell.clone();
    let identity = request.identity.clone();
    let observation = tokio::time::timeout(
        deadline,
        client.run(
            Lane::Io,
            JobCost {
                input_bytes: PROBE_INPUT_BYTES,
                result_bytes: 128,
            },
            move |_| -> Result<(Option<bool>, Option<bool>), Infallible> {
                Ok(work(shell, identity))
            },
        ),
    )
    .await
    .map_err(|_| ProbeError::Deadline)?
    .map_err(ProbeError::Execution)?;
    let (shell_owns_terminal, identity_matches) = *observation.view();
    Ok(ProbeObservation {
        request,
        shell_owns_terminal,
        identity_matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn empty_request() -> ProbeRequest {
        ProbeRequest {
            session: PtySessionIdentity::default(),
            shell: None,
            identity: None,
            runtime_fence: None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn completed_probe_rejects_every_changed_invocation_field() {
        let directory = tempfile::tempdir().expect("isolated PTY directory");
        let session = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
        )
        .expect("owned fixture PTY");
        let mut runtime = TerminalPaneRuntime::new(
            session,
            TerminalOrigin::Command("cat".to_owned()),
            None,
            Duration::from_secs(1),
        );
        let identity = AgentIdentity {
            class: ilium_core::AgentClass::Codex,
            pid: 123,
            started_at_unix_seconds: 456,
            process_name: "codex".to_owned(),
            matched_signature: "codex".to_owned(),
            process_tree_depth: 1,
        };
        runtime.agent_process_key = Some(crate::pane::agent_process_key(&identity));
        runtime.detection_schedule.cached_identity = Some(identity.clone());
        runtime.session_id = Some("session-a".to_owned());
        let observed = ProbeObservation {
            request: ProbeRequest::for_runtime(&runtime),
            shell_owns_terminal: None,
            identity_matches: Some(true),
        };
        assert!(observed.current_agent_identity(&runtime));
        runtime.agent_generation += 1;
        assert!(!observed.current_agent_identity(&runtime));
        runtime.agent_generation -= 1;
        runtime.title_generation += 1;
        assert!(!observed.current_agent_identity(&runtime));
        runtime.title_generation -= 1;
        runtime.session_id = Some("session-b".to_owned());
        assert!(!observed.current_agent_identity(&runtime));
        runtime.session_id = Some("session-a".to_owned());
        runtime
            .agent_process_key
            .as_mut()
            .unwrap()
            .started_at_unix_seconds += 1;
        assert!(!observed.current_agent_identity(&runtime));
        runtime
            .agent_process_key
            .as_mut()
            .unwrap()
            .started_at_unix_seconds -= 1;
        runtime
            .detection_schedule
            .cached_identity
            .as_mut()
            .unwrap()
            .process_name = "sh".to_owned();
        assert!(!observed.current_agent_identity(&runtime));
        runtime.detection_schedule.cached_identity = Some(identity);
        runtime
            .agent_input_cancel
            .send_modify(|generation| *generation += 1);
        assert!(!observed.current_agent_identity(&runtime));
        runtime.session.kill().expect("close fixture PTY");
    }

    #[tokio::test]
    async fn blocked_native_probe_leaves_server_guards_and_executor_available() {
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let client = owner.client.clone();
        let probe = tokio::spawn(async move {
            observe_with(
                &client,
                empty_request(),
                Duration::from_secs(5),
                move |_, _| {
                    let _ = started_tx.send(std::thread::current().id());
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release blocked native fixture");
                    (Some(true), None)
                },
            )
            .await
        });
        let worker_thread = started_rx.await.expect("native worker started");
        assert_ne!(worker_thread, std::thread::current().id());

        let directory = tempfile::tempdir().expect("isolated state directory");
        let (sound_requests, _sound_rx) = tokio::sync::mpsc::channel(1);
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "foreground-lock-fixture".to_owned(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        tokio::time::timeout(Duration::from_secs(2), async {
            let _tree = state.tree.write().await;
            let _panes = state.panes.write().await;
            tokio::task::yield_now().await;
        })
        .await
        .expect("native foreground work must not own shared guards or Tokio executor");
        release_tx.send(()).expect("release worker");
        let observed = probe
            .await
            .expect("probe task")
            .expect("admitted observation");
        assert_eq!(observed.shell_owns_terminal(), Some(true));
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn deadline_does_not_release_physically_blocked_worker_admission() {
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let client = owner.client.clone();
        let probe = tokio::spawn(async move {
            observe_with(
                &client,
                empty_request(),
                Duration::from_millis(50),
                move |_, _| {
                    let _ = started_tx.send(());
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release blocked callback");
                    (Some(true), None)
                },
            )
            .await
        });
        started_rx.await.expect("native callback started");
        assert!(matches!(
            probe.await.expect("probe task"),
            Err(ProbeError::Deadline)
        ));
        let full_client_cost = JobCost {
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 128,
        };
        assert!(matches!(
            owner
                .client
                .foundation
                .try_reserve(Lane::Io, full_client_cost),
            Err(ilium_execution::RejectReason::InputBytes)
        ));
        release_tx.send(()).expect("release callback");
        let reservation = tokio::time::timeout(
            Duration::from_secs(2),
            owner.client.reserve(Lane::Io, full_client_cost),
        )
        .await
        .expect("physical completion notification")
        .expect("admission released after callback and receipt teardown");
        drop(reservation);
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn explicit_bank_refusal_never_starts_native_probe_and_recovery_is_admitted() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let full_client_cost = JobCost {
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 0,
        };
        let held = owner
            .client
            .foundation
            .try_reserve(Lane::Io, full_client_cost)
            .expect("reserve general-client input budget");
        let ran = Arc::new(AtomicBool::new(false));
        let ran_in_callback = Arc::clone(&ran);
        let refused = observe_with(
            &owner.client,
            empty_request(),
            Duration::from_secs(1),
            move |_, _| {
                ran_in_callback.store(true, Ordering::SeqCst);
                (Some(true), None)
            },
        )
        .await;
        assert!(matches!(
            refused,
            Err(ProbeError::Execution(ExecutionError::Rejected(
                ilium_execution::RejectReason::InputBytes
            )))
        ));
        assert!(!ran.load(Ordering::SeqCst));
        drop(held);
        let admitted = observe_with(
            &owner.client,
            empty_request(),
            Duration::from_secs(1),
            |_, _| (Some(true), None),
        )
        .await
        .expect("same finite bank admits probe after reservation release");
        assert_eq!(admitted.shell_owns_terminal(), Some(true));
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn shutdown_refuses_new_probe_without_releasing_blocked_native_custody() {
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let quota = owner.client.foundation.quota_group();
        let baseline_input_bytes = quota.snapshot().input_bytes;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let client = owner.client.clone();
        let probe = tokio::spawn(async move {
            observe_with(
                &client,
                empty_request(),
                Duration::from_millis(50),
                move |_, _| {
                    let _ = started_tx.send(());
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release native fixture");
                    (Some(true), None)
                },
            )
            .await
        });
        started_rx.await.expect("native callback started");
        assert!(matches!(
            probe.await.expect("probe task"),
            Err(ProbeError::Deadline)
        ));
        let occupied_input_bytes = quota.snapshot().input_bytes;
        assert!(occupied_input_bytes >= baseline_input_bytes + PROBE_INPUT_BYTES);
        owner.request_shutdown();
        let refused = observe_with(
            &owner.client,
            empty_request(),
            Duration::from_secs(1),
            |_, _| panic!("closed bank must not invoke a native callback"),
        )
        .await;
        assert!(matches!(
            refused,
            Err(ProbeError::Execution(ExecutionError::Rejected(
                ilium_execution::RejectReason::Closed
            )))
        ));
        assert_eq!(quota.snapshot().input_bytes, occupied_input_bytes);
        let completed = owner.client.completion_notification();
        release_tx.send(()).expect("release native fixture");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let notified = completed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if quota.snapshot().input_bytes == baseline_input_bytes {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("physical callback retired and quota released");
    }
}
