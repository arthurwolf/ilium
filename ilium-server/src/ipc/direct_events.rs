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
    #[cfg(test)]
    pub(crate) fn channel(capacity: usize) -> (Self, DirectEventReceiver) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                execution: None,
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
        let insufficient = Arc::new(
            execution
                .quota_group()
                .reserve_external_storage(1)
                .expect("small fixture lease"),
        );

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
        let _saturated = quota
            .reserve_external_storage(quota.snapshot().limits.worker_bytes)
            .unwrap();
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
                std::time::Instant::now() + std::time::Duration::from_secs(5),
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert!(report.shutdown_complete);
    }
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
