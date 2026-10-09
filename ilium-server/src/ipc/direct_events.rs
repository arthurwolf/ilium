//! Per-connection ordered replies. The optional producer admission stays
//! attached to an event until the connection writer has flushed it.

use ilium_execution::StorageAdmission;
use ilium_ipc::ServerEvent;
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) struct QueuedServerEvent {
    pub(crate) event: ServerEvent,
    pub(crate) producer_storage: Option<Arc<StorageAdmission>>,
}

#[derive(Clone)]
pub(crate) struct DirectEventSender(mpsc::Sender<QueuedServerEvent>);

pub(crate) struct DirectEventReceiver(mpsc::Receiver<QueuedServerEvent>);

impl DirectEventReceiver {
    pub(crate) async fn recv(&mut self) -> Option<ServerEvent> {
        self.recv_queued().await.map(|queued| queued.event)
    }

    pub(crate) async fn recv_queued(&mut self) -> Option<QueuedServerEvent> {
        self.0.recv().await
    }

    pub(crate) fn try_recv(&mut self) -> Result<ServerEvent, mpsc::error::TryRecvError> {
        self.0.try_recv().map(|queued| queued.event)
    }
}

impl DirectEventSender {
    pub(crate) fn channel(capacity: usize) -> (Self, DirectEventReceiver) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self(sender), DirectEventReceiver(receiver))
    }

    pub(crate) async fn send(&self, event: ServerEvent) -> Result<(), ServerEvent> {
        self.send_with_storage(event, None).await
    }

    pub(crate) async fn send_with_storage(
        &self,
        event: ServerEvent,
        producer_storage: Option<Arc<StorageAdmission>>,
    ) -> Result<(), ServerEvent> {
        self.0
            .send(QueuedServerEvent {
                event,
                producer_storage,
            })
            .await
            .map_err(|error| error.0.event)
    }

    /// Non-blocking enqueue for synchronous owners (pane teardown,
    /// monitor replacement) that cannot await writer capacity. Returns
    /// whether the event was queued; a full or closed channel drops it.
    pub(crate) fn try_send(&self, event: ServerEvent) -> bool {
        self.0
            .try_send(QueuedServerEvent {
                event,
                producer_storage: None,
            })
            .is_ok()
    }

    pub(crate) async fn closed(&self) {
        self.0.closed().await;
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.0.is_closed()
    }

    pub(crate) fn same_channel(&self, other: &Self) -> bool {
        self.0.same_channel(&other.0)
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
