//! Per-connection ordered replies. Production admission stays attached to an
//! event until the connection writer has flushed it.

use ilium_execution::StorageAdmission;
use ilium_ipc::ServerEvent;
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) struct QueuedServerEvent {
    pub(crate) event: ServerEvent,
    pub(crate) producer_storage: Option<Arc<StorageAdmission>>,
}

#[derive(Clone)]
pub(crate) struct DirectEventSender {
    sender: mpsc::Sender<QueuedServerEvent>,
    execution: Option<crate::execution::ExecutionClient>,
    error_scope: Option<RequestErrorScope>,
}

#[derive(Clone)]
struct RequestErrorScope {
    state: std::sync::Weak<crate::state::ServerState>,
    node_id: Option<ilium_core::NodeId>,
    operation: &'static str,
    generation: u64,
    has_failed: Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) struct DirectEventReceiver(mpsc::Receiver<QueuedServerEvent>);

impl DirectEventReceiver {
    #[cfg(test)]
    pub(crate) async fn recv(&mut self) -> Option<ServerEvent> {
        self.recv_queued().await.map(|queued| queued.event)
    }

    pub(crate) async fn recv_queued(&mut self) -> Option<QueuedServerEvent> {
        self.0.recv().await
    }

    #[cfg(test)]
    pub(crate) fn try_recv(&mut self) -> Result<ServerEvent, mpsc::error::TryRecvError> {
        self.0.try_recv().map(|queued| queued.event)
    }
}

impl DirectEventSender {
    pub(crate) fn with_error_scope(
        &self,
        state: &Arc<crate::state::ServerState>,
        node_id: Option<ilium_core::NodeId>,
        operation: &'static str,
    ) -> Self {
        let mut sender = self.clone();
        sender.error_scope = Some(RequestErrorScope {
            state: Arc::downgrade(state),
            node_id,
            operation,
            generation: state.begin_error_operation(node_id, operation),
            has_failed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        sender
    }

    /// Called only after a synchronous mutation has completed; queued work
    /// and read-only probes must wait for their own typed terminal outcome.
    pub(crate) fn finish_synchronous_mutation(&self) {
        let Some(scope) = &self.error_scope else {
            return;
        };
        if !scope.has_failed.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(state) = scope.state.upgrade() {
                state.clear_current_blocking_error(
                    scope.node_id,
                    scope.operation,
                    scope.generation,
                );
            }
        }
    }

    fn retain_blocking_error(&self, event: &ServerEvent) {
        let Some(RequestErrorScope {
            state,
            node_id,
            operation,
            generation,
            has_failed,
        }) = &self.error_scope
        else {
            return;
        };
        let Some(state) = state.upgrade() else {
            return;
        };
        let message = match event {
            ServerEvent::Error { message }
            | ServerEvent::ProjectRestructureRejected { message, .. }
            | ServerEvent::PaneResizeRejected { message, .. }
            | ServerEvent::WorkspaceCreateFailed { error: message, .. } => Some(message.as_str()),
            ServerEvent::RepoFactsReported { result, .. } => {
                result.as_ref().err().map(String::as_str)
            }
            ServerEvent::WorkspaceInventoryReported { result, .. } => {
                result.as_ref().err().map(String::as_str)
            }
            ServerEvent::PaneProcessTerminated { result, .. }
            | ServerEvent::PaneFrozen { result, .. }
            | ServerEvent::AntigravityStatuslineCompleted { result, .. } => {
                result.as_ref().err().map(String::as_str)
            }
            ServerEvent::PaneCreateFinished { result, .. } => {
                result.as_ref().err().map(String::as_str)
            }
            ServerEvent::PaneRecapScreen { result, .. } => {
                result.as_ref().err().map(String::as_str)
            }
            ServerEvent::ProgressMonitorCheckCompleted { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            ServerEvent::ProgressMonitorSetCompleted { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            ServerEvent::ProgressMonitorStatusReported { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            ServerEvent::ProgressMonitorCleared { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            ServerEvent::ProgressWaitCompleted { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            ServerEvent::AgentDetectionSettingsChanged { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            ServerEvent::VoiceTextResult { result, .. } => {
                result.as_ref().err().map(|error| error.message.as_str())
            }
            _ => None,
        };
        if let Some(message) = message {
            has_failed.store(true, std::sync::atomic::Ordering::Relaxed);
            state.record_current_blocking_error(*node_id, operation, *generation, message);
        }
        // Clear only acknowledged successful mutations, never an unrelated
        // read, broadcast or a merely accepted background job.
        let recovered = matches!(
            event,
            ServerEvent::PaneCreateFinished { result: Ok(_), .. }
                | ServerEvent::PaneProcessTerminated { result: Ok(_), .. }
                | ServerEvent::PaneFrozen { result: Ok(_), .. }
                | ServerEvent::WorkspaceCreated { .. }
                | ServerEvent::WorkspaceRemoved { .. }
                | ServerEvent::AgentDetectionSettingsChanged {
                    request_id: Some(_),
                    result: Ok(_),
                    ..
                }
        );
        if recovered {
            state.clear_current_blocking_error(*node_id, operation, *generation);
        }
        if let ServerEvent::WorkspaceRemovalBlocked { reasons, .. } = event {
            state.record_current_blocking_error(
                *node_id,
                operation,
                *generation,
                &reasons.join("\n"),
            );
        }
        if let ServerEvent::WorkspacePruneCompleted { result, .. } = event {
            if result.outcome == ilium_ipc::WorkspacePruneOutcome::Removed {
                state.clear_current_blocking_error(*node_id, operation, *generation);
            } else {
                state.record_current_blocking_error(
                    *node_id,
                    operation,
                    *generation,
                    &result.reasons.join("\n"),
                );
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn channel(capacity: usize) -> (Self, DirectEventReceiver) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                execution: None,
                error_scope: None,
            },
            DirectEventReceiver(receiver),
        )
    }

    pub(crate) fn admitted_channel(
        capacity: usize,
        execution: crate::execution::ExecutionClient,
    ) -> (Self, DirectEventReceiver) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                execution: Some(execution),
                error_scope: None,
            },
            DirectEventReceiver(receiver),
        )
    }

    pub(crate) async fn send(&self, event: ServerEvent) -> Result<(), ServerEvent> {
        self.send_with_storage(event, None).await
    }

    pub(crate) async fn send_with_storage(
        &self,
        event: ServerEvent,
        mut producer_storage: Option<Arc<StorageAdmission>>,
    ) -> Result<(), ServerEvent> {
        self.retain_blocking_error(&event);
        let Some(storage_bytes) = direct_event_storage_bytes(&event) else {
            return Err(event);
        };
        if let Some(execution) = &self.execution {
            if let Some(storage) = &producer_storage {
                if !storage.shares_root(&execution.quota_group())
                    || storage.resident_bytes() < storage_bytes
                {
                    return Err(event);
                }
            } else {
                let admission = tokio::select! {
                    biased;
                    _ = self.sender.closed() => return Err(event),
                    admission = execution.reserve_storage(storage_bytes) => admission,
                };
                producer_storage = Some(match admission {
                    Ok(storage) => storage,
                    Err(_) => return Err(event),
                });
            }
        }
        self.sender
            .send(QueuedServerEvent {
                event,
                producer_storage,
            })
            .await
            .map_err(|error| error.0.event)
    }

    /// Non-blocking enqueue for synchronous owners that cannot await writer
    /// capacity. `false` means queue or byte admission failed, or the receiver
    /// closed; the caller must have a fallback because this consumes the event.
    pub(crate) fn try_send(&self, event: ServerEvent) -> bool {
        self.retain_blocking_error(&event);
        let producer_storage = match &self.execution {
            Some(execution) => {
                let Some(bytes) = direct_event_storage_bytes(&event) else {
                    return false;
                };
                match execution.try_reserve_storage(bytes) {
                    Ok(storage) => Some(storage),
                    Err(_) => return false,
                }
            }
            None => None,
        };
        self.sender
            .try_send(QueuedServerEvent {
                event,
                producer_storage,
            })
            .is_ok()
    }

    pub(crate) async fn closed(&self) {
        self.sender.closed().await;
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    pub(crate) fn same_channel(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}

fn direct_event_storage_bytes(event: &ServerEvent) -> Option<usize> {
    event
        .retained_bytes()
        .checked_mul(2)?
        .checked_add(std::mem::size_of::<StorageAdmission>())?
        .checked_add(std::mem::size_of::<QueuedServerEvent>())?
        .checked_add(2 * std::mem::size_of::<usize>())
}

pub(crate) enum EventReply<'a> {
    Direct(&'a DirectEventSender),
    Legacy(&'a mpsc::Sender<ServerEvent>),
}

impl EventReply<'_> {
    pub(crate) fn is_closed(&self) -> bool {
        match self {
            Self::Direct(sender) => sender.is_closed(),
            Self::Legacy(sender) => sender.is_closed(),
        }
    }

    pub(crate) async fn send(&self, event: ServerEvent) {
        match self {
            Self::Direct(sender) => {
                let _ = sender.send(event).await;
            }
            Self::Legacy(sender) => {
                let _ = sender.send(event).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queued_reply_retains_shared_storage_until_writer_retires_it() {
        let (sender, mut receiver) =
            DirectEventSender::admitted_channel(2, crate::execution::test_general_client());
        let event = ServerEvent::Error {
            message: "admitted reply".to_owned(),
        };
        let expected_bytes = direct_event_storage_bytes(&event).unwrap();

        sender.send(event).await.unwrap();
        let queued = receiver.recv_queued().await.unwrap();
        let storage = queued
            .producer_storage
            .as_ref()
            .expect("production replies retain a shared storage lease");
        assert!(storage.resident_bytes() >= expected_bytes);
        assert!(storage.shares_root(&sender.execution.as_ref().unwrap().quota_group()));

        drop(queued);
    }

    #[tokio::test]
    async fn undersized_producer_storage_returns_the_original_reply() {
        let execution = crate::execution::test_general_client();
        let (sender, mut receiver) = DirectEventSender::admitted_channel(1, execution.clone());
        let event = ServerEvent::Error {
            message: "must remain available to retry".to_owned(),
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let insufficient = loop {
            match execution.try_reserve_storage(1) {
                Ok(storage) => break storage,
                Err(ilium_execution::RejectReason::Busy) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "small fixture lease admission stayed busy"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(reason) => panic!("small fixture lease admission failed: {reason:?}"),
            }
        };

        let returned = sender
            .send_with_storage(event.clone(), Some(insufficient))
            .await
            .expect_err("an undersized lease must not enter the reply queue");
        assert_eq!(returned, event);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn closed_reply_receiver_cancels_a_storage_wait() {
        let mut owner = crate::execution::ServerExecution::start().unwrap();
        let execution = owner.client.clone();
        let quota = owner.quota_group();
        let snapshot = quota.snapshot();
        let remaining_storage = snapshot
            .limits
            .worker_bytes
            .checked_sub(snapshot.worker_bytes)
            .expect("execution residents fit the worker-byte ceiling");
        assert!(remaining_storage > 0);
        let _saturated = quota.reserve_external_storage(remaining_storage).unwrap();
        let (sender, receiver) = DirectEventSender::admitted_channel(1, execution);
        drop(receiver);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            sender.send(ServerEvent::Error {
                message: "receiver already closed".to_owned(),
            }),
        )
        .await;
        assert!(matches!(result, Ok(Err(_))));

        drop(_saturated);
        owner.request_shutdown();
        let report = tokio::task::spawn_blocking(move || {
            owner.test_join_until_background(
                std::time::Instant::now() + crate::execution::PHYSICAL_JOIN_TEST_WAIT,
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert!(report.shutdown_complete, "{report:?}");
    }
}
