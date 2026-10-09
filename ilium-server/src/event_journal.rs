//! Bounded, session-local ownership for ordered server events.
//!
//! The journal owns only admitted entries. A subscriber advances its cursor
//! after the connection writer flushes an event (or deliberately filters it),
//! and the associated process storage lease is released once every cursor has
//! advanced or disconnected.

use ilium_execution::{QuotaGroup, RejectReason, StorageAdmission};
use ilium_ipc::ServerEvent;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
};
use tokio::sync::Notify;

const JOURNAL_ENTRY_OVERHEAD_BYTES: usize =
    std::mem::size_of::<ServerEvent>() + 2 * std::mem::size_of::<usize>() + 64;
pub(crate) const DEFAULT_MAXIMUM_ENTRIES: usize = 4096;
pub(crate) const DEFAULT_MAXIMUM_BYTES: usize = 128 * 1024 * 1024;
const MAXIMUM_SUBSCRIBERS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalRefusal {
    NotInitialized,
    Closed,
    EntryLimit,
    ByteLimit,
    InvalidSize,
    CursorGap,
    InvalidReservation,
    OutOfOrder,
    SubscriberLimit,
    SequenceExhausted,
    Storage(RejectReason),
}

pub(crate) struct PublishFailure {
    pub(crate) event: ServerEvent,
    pub(crate) refusal: JournalRefusal,
}

#[derive(Debug)]
pub(crate) struct ReservationFailure {
    pub(crate) producer_storage: Option<Arc<StorageAdmission>>,
    pub(crate) refusal: JournalRefusal,
}

impl std::fmt::Debug for PublishFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PublishFailure")
            .field("event_kind", &std::mem::discriminant(&self.event))
            .field("refusal", &self.refusal)
            .finish()
    }
}

struct JournalEntry {
    sequence: u64,
    event: Arc<ServerEvent>,
    _storage: Arc<StorageAdmission>,
    _local_storage: LocalStorageAdmission,
}

struct JournalSlot {
    sequence: u64,
    state: JournalSlotState,
}

enum JournalSlotState {
    Pending {
        storage: Arc<StorageAdmission>,
        local_storage: LocalStorageAdmission,
    },
    Committed(Arc<JournalEntry>),
    Cancelled,
}

struct JournalState {
    next_sequence: u64,
    next_subscriber: u64,
    closed: bool,
    entries: VecDeque<JournalSlot>,
    cursors: HashMap<u64, u64>,
}

struct JournalShared {
    state: Mutex<JournalState>,
    changed: Notify,
    quota: QuotaGroup,
    retention: Arc<RetentionCounters>,
}

struct RetentionCounters {
    entries: AtomicUsize,
    bytes: AtomicUsize,
    maximum_entries: usize,
    maximum_bytes: usize,
}

struct LocalStorageAdmission {
    counters: Arc<RetentionCounters>,
    bytes: usize,
}

impl RetentionCounters {
    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<LocalStorageAdmission, JournalRefusal> {
        if !reserve_atomic(&self.entries, 1, self.maximum_entries) {
            return Err(JournalRefusal::EntryLimit);
        }
        if !reserve_atomic(&self.bytes, bytes, self.maximum_bytes) {
            self.entries.fetch_sub(1, Ordering::AcqRel);
            return Err(JournalRefusal::ByteLimit);
        }
        Ok(LocalStorageAdmission {
            counters: Arc::clone(self),
            bytes,
        })
    }
}

impl Drop for LocalStorageAdmission {
    fn drop(&mut self) {
        self.counters.entries.fetch_sub(1, Ordering::AcqRel);
        self.counters.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

fn reserve_atomic(counter: &AtomicUsize, amount: usize, maximum: usize) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(amount).filter(|next| *next <= maximum)
        })
        .is_ok()
}

/// A bounded journal shared by the server's IPC connections.
#[derive(Clone)]
pub(crate) struct EventJournal(Arc<JournalShared>);

impl EventJournal {
    pub(crate) fn new(quota: QuotaGroup, maximum_entries: usize, maximum_bytes: usize) -> Self {
        Self(Arc::new(JournalShared {
            state: Mutex::new(JournalState {
                next_sequence: 1,
                next_subscriber: 1,
                closed: false,
                entries: VecDeque::new(),
                cursors: HashMap::new(),
            }),
            changed: Notify::new(),
            quota,
            retention: Arc::new(RetentionCounters {
                entries: AtomicUsize::new(0),
                bytes: AtomicUsize::new(0),
                maximum_entries,
                maximum_bytes,
            }),
        }))
    }

    /// Register at the current tail. Call before attach/replay so every event
    /// accepted after this cut follows that connection's initial state batch.
    pub(crate) fn subscribe(&self) -> Result<EventSubscription, JournalRefusal> {
        let mut state = lock(&self.0.state);
        if state.closed {
            return Err(JournalRefusal::Closed);
        }
        if state.cursors.len() >= MAXIMUM_SUBSCRIBERS {
            return Err(JournalRefusal::SubscriberLimit);
        }
        let subscriber = state.next_subscriber;
        state.next_subscriber = state
            .next_subscriber
            .checked_add(1)
            .ok_or(JournalRefusal::SequenceExhausted)?;
        let next_sequence = state.next_sequence;
        state.cursors.insert(subscriber, next_sequence);
        Ok(EventSubscription {
            shared: Arc::clone(&self.0),
            subscriber,
            delivered: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Reserve a bounded slot and sequence before preparing an event or
    /// crossing its associated commit point. Later slots cannot pass this one.
    pub(crate) fn reserve(&self, payload_bytes: usize) -> Result<EventReservation, JournalRefusal> {
        self.reserve_with_storage(payload_bytes, None)
            .map_err(|failure| failure.refusal)
    }

    /// Transfer an existing same-process lease when the producer already
    /// accounted for this retained allocation. Refusal returns that lease.
    pub(crate) fn reserve_with_storage(
        &self,
        payload_bytes: usize,
        producer_storage: Option<Arc<StorageAdmission>>,
    ) -> Result<EventReservation, ReservationFailure> {
        let Some(charged_bytes) = payload_bytes.checked_add(JOURNAL_ENTRY_OVERHEAD_BYTES) else {
            return Err(ReservationFailure {
                producer_storage,
                refusal: JournalRefusal::InvalidSize,
            });
        };
        let mut state = lock(&self.0.state);
        if state.closed {
            return Err(ReservationFailure {
                producer_storage,
                refusal: JournalRefusal::Closed,
            });
        }
        let sequence = state.next_sequence;
        let Some(next_sequence) = sequence.checked_add(1) else {
            return Err(ReservationFailure {
                producer_storage,
                refusal: JournalRefusal::SequenceExhausted,
            });
        };

        // Unattached sessions do not retain events. Still assign a sequence
        // so a later subscription starts after this operation's cut.
        if state.cursors.is_empty() {
            state.next_sequence = next_sequence;
            return Ok(EventReservation {
                shared: Arc::clone(&self.0),
                sequence,
                retained: false,
                active: false,
                producer_storage,
            });
        }

        if state.entries.len() >= self.0.retention.maximum_entries {
            return Err(ReservationFailure {
                producer_storage,
                refusal: JournalRefusal::EntryLimit,
            });
        }
        let local_storage = match self.0.retention.reserve(charged_bytes) {
            Ok(storage) => storage,
            Err(refusal) => {
                return Err(ReservationFailure {
                    producer_storage,
                    refusal,
                });
            }
        };
        let storage = match producer_storage {
            Some(storage)
                if storage.shares_root(&self.0.quota)
                    && storage.resident_bytes() >= charged_bytes =>
            {
                storage
            }
            Some(storage) => {
                return Err(ReservationFailure {
                    producer_storage: Some(storage),
                    refusal: JournalRefusal::InvalidReservation,
                });
            }
            None => match self.0.quota.reserve_external_storage(charged_bytes) {
                Ok(storage) => Arc::new(storage),
                Err(reason) => {
                    return Err(ReservationFailure {
                        producer_storage: None,
                        refusal: JournalRefusal::Storage(reason),
                    });
                }
            },
        };

        state.next_sequence = next_sequence;
        state.entries.push_back(JournalSlot {
            sequence,
            state: JournalSlotState::Pending {
                storage,
                local_storage,
            },
        });
        drop(state);
        self.0.changed.notify_waiters();
        Ok(EventReservation {
            shared: Arc::clone(&self.0),
            sequence,
            retained: true,
            active: true,
            producer_storage: None,
        })
    }

    /// Test helper for exercising publication and refusal without a producer.
    #[cfg(test)]
    pub(crate) fn try_publish(
        &self,
        event: ServerEvent,
        payload_bytes: usize,
    ) -> Result<u64, PublishFailure> {
        let reservation = match self.reserve(payload_bytes) {
            Ok(reservation) => reservation,
            Err(refusal) => return Err(PublishFailure { event, refusal }),
        };
        reservation.commit(event)
    }

    /// Refuse new events and wake every cursor. Existing entries remain
    /// available to subscribers until acknowledged or disconnected.
    pub(crate) fn close(&self) {
        lock(&self.0.state).closed = true;
        self.0.changed.notify_waiters();
    }

    #[cfg(test)]
    fn retained(&self) -> (usize, usize) {
        (
            self.0.retention.entries.load(Ordering::Acquire),
            self.0.retention.bytes.load(Ordering::Acquire),
        )
    }
}

pub(crate) struct EventReservation {
    shared: Arc<JournalShared>,
    sequence: u64,
    retained: bool,
    active: bool,
    producer_storage: Option<Arc<StorageAdmission>>,
}

impl EventReservation {
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn commit(mut self, event: ServerEvent) -> Result<u64, PublishFailure> {
        if !self.retained {
            self.active = false;
            drop(event);
            drop(self.producer_storage.take());
            return Ok(self.sequence);
        }

        let mut state = lock(&self.shared.state);
        let Some(index) = slot_index(&state.entries, self.sequence) else {
            self.active = false;
            return Ok(self.sequence);
        };
        let Some(slot) = state.entries.get_mut(index) else {
            self.active = false;
            return Ok(self.sequence);
        };
        let previous = std::mem::replace(&mut slot.state, JournalSlotState::Cancelled);
        let JournalSlotState::Pending {
            storage,
            local_storage,
        } = previous
        else {
            slot.state = previous;
            self.active = false;
            return Err(PublishFailure {
                event,
                refusal: JournalRefusal::OutOfOrder,
            });
        };
        slot.state = JournalSlotState::Committed(Arc::new(JournalEntry {
            sequence: self.sequence,
            event: Arc::new(event),
            _storage: storage,
            _local_storage: local_storage,
        }));
        self.active = false;
        let released = reap(&mut state);
        drop(state);
        drop(released);
        self.shared.changed.notify_waiters();
        Ok(self.sequence)
    }

    pub(crate) fn cancel(self) {
        drop(self);
    }
}

impl Drop for EventReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = lock(&self.shared.state);
        let old_state = slot_index(&state.entries, self.sequence)
            .and_then(|index| state.entries.get_mut(index))
            .and_then(|slot| {
                matches!(&slot.state, JournalSlotState::Pending { .. })
                    .then(|| std::mem::replace(&mut slot.state, JournalSlotState::Cancelled))
            });
        let released = reap(&mut state);
        drop(state);
        drop(old_state);
        drop(released);
        self.shared.changed.notify_waiters();
    }
}

fn slot_index(entries: &VecDeque<JournalSlot>, sequence: u64) -> Option<usize> {
    let first_sequence = entries.front()?.sequence;
    let index = usize::try_from(sequence.checked_sub(first_sequence)?).ok()?;
    entries
        .get(index)
        .filter(|slot| slot.sequence == sequence)
        .map(|_| index)
}

pub(crate) struct JournalEvent {
    pub(crate) sequence: u64,
    entry: Arc<JournalEntry>,
}

impl JournalEvent {
    pub(crate) fn event(&self) -> &ServerEvent {
        &self.entry.event
    }
}

pub(crate) struct EventSubscription {
    shared: Arc<JournalShared>,
    subscriber: u64,
    // A cursor advances only for an entry this single-consumer subscription
    // actually returned. Sequence zero is reserved as "nothing delivered".
    delivered: std::sync::atomic::AtomicU64,
}

enum SlotReadiness {
    Pending,
    Cancelled,
    Committed(Arc<JournalEntry>),
}

impl EventSubscription {
    /// Return the event at this subscriber's current cursor without advancing
    /// it. The caller must acknowledge only after flush or intentional filter.
    pub(crate) async fn recv(&self) -> Result<Option<JournalEvent>, JournalRefusal> {
        loop {
            let notified = self.shared.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let (readiness, released) = {
                let mut state = lock(&self.shared.state);
                let Some(cursor) = state.cursors.get(&self.subscriber).copied() else {
                    return Err(JournalRefusal::Closed);
                };
                let readiness = match state.entries.front() {
                    None if state.closed => return Ok(None),
                    None => SlotReadiness::Pending,
                    Some(first) if cursor < first.sequence => {
                        return Err(JournalRefusal::CursorGap);
                    }
                    Some(first) => {
                        let offset = usize::try_from(cursor - first.sequence)
                            .map_err(|_| JournalRefusal::CursorGap)?;
                        match state.entries.get(offset) {
                            None if state.closed => return Ok(None),
                            None => SlotReadiness::Pending,
                            Some(slot) => {
                                if slot.sequence != cursor {
                                    return Err(JournalRefusal::CursorGap);
                                }
                                match &slot.state {
                                    JournalSlotState::Pending { .. } => SlotReadiness::Pending,
                                    JournalSlotState::Committed(entry) => {
                                        SlotReadiness::Committed(Arc::clone(entry))
                                    }
                                    JournalSlotState::Cancelled => SlotReadiness::Cancelled,
                                }
                            }
                        }
                    }
                };
                match readiness {
                    SlotReadiness::Cancelled => {
                        let Some(next_cursor) = cursor.checked_add(1) else {
                            return Err(JournalRefusal::SequenceExhausted);
                        };
                        *state
                            .cursors
                            .get_mut(&self.subscriber)
                            .ok_or(JournalRefusal::Closed)? = next_cursor;
                        let released = reap(&mut state);
                        drop(state);
                        (SlotReadiness::Cancelled, released)
                    }
                    SlotReadiness::Committed(entry) => {
                        self.delivered.store(cursor, Ordering::Release);
                        drop(state);
                        (SlotReadiness::Committed(entry), Vec::new())
                    }
                    SlotReadiness::Pending => {
                        drop(state);
                        (SlotReadiness::Pending, Vec::new())
                    }
                }
            };
            drop(released);
            match readiness {
                SlotReadiness::Committed(entry) => {
                    return Ok(Some(JournalEvent {
                        sequence: entry.sequence,
                        entry,
                    }));
                }
                SlotReadiness::Cancelled => {
                    self.shared.changed.notify_waiters();
                    continue;
                }
                SlotReadiness::Pending => notified.await,
            }
        }
    }

    pub(crate) fn acknowledge(&self, sequence: u64) -> Result<(), JournalRefusal> {
        if self.delivered.load(Ordering::Acquire) != sequence {
            return Err(JournalRefusal::OutOfOrder);
        }
        let mut state = lock(&self.shared.state);
        let Some(cursor) = state.cursors.get_mut(&self.subscriber) else {
            return Err(JournalRefusal::Closed);
        };
        if *cursor != sequence {
            return Err(JournalRefusal::OutOfOrder);
        }
        let Some(next_sequence) = sequence.checked_add(1) else {
            return Err(JournalRefusal::SequenceExhausted);
        };
        *cursor = next_sequence;
        self.delivered.store(0, Ordering::Release);
        let released = reap(&mut state);
        drop(state);
        drop(released);
        self.shared.changed.notify_waiters();
        Ok(())
    }
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        let mut state = lock(&self.shared.state);
        state.cursors.remove(&self.subscriber);
        let released = reap(&mut state);
        drop(state);
        drop(released);
        self.shared.changed.notify_waiters();
    }
}

fn reap(state: &mut JournalState) -> Vec<JournalSlot> {
    let mut released = Vec::new();
    let Some(minimum_cursor) = state.cursors.values().copied().min() else {
        released.extend(state.entries.drain(..));
        return released;
    };
    while state
        .entries
        .front()
        .is_some_and(|slot| slot.sequence < minimum_cursor)
    {
        if let Some(slot) = state.entries.pop_front() {
            released.push(slot);
        }
    }
    released
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::NodeId;
    use ilium_execution::QuotaLimits;
    use ilium_ipc::PromptSubmissionSource;

    fn quota(worker_bytes: usize) -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes,
        })
    }

    fn state_event(revision: u64) -> ServerEvent {
        ServerEvent::NodeActivityChanged {
            node_id: NodeId(3),
            activity_revision: revision,
        }
    }

    #[tokio::test]
    async fn slow_subscriber_preserves_prompt_after_state_burst() {
        let quota = quota(2 * 1024 * 1024);
        let journal = EventJournal::new(quota.clone(), 1100, 2 * 1024 * 1024);
        let subscription = journal.subscribe().unwrap();
        for revision in 1..=1025 {
            journal.try_publish(state_event(revision), 128).unwrap();
        }
        journal
            .try_publish(
                ServerEvent::PanePromptSubmitted {
                    pane_id: NodeId(3),
                    source: PromptSubmissionSource::Keyboard,
                },
                128,
            )
            .unwrap();

        for expected_sequence in 1..=1026 {
            let event = subscription.recv().await.unwrap().unwrap();
            assert_eq!(event.sequence, expected_sequence);
            subscription.acknowledge(event.sequence).unwrap();
            if expected_sequence == 1026 {
                assert!(matches!(
                    event.event(),
                    ServerEvent::PanePromptSubmitted { .. }
                ));
            }
            drop(event);
        }
        assert_eq!(journal.retained(), (0, 0));
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn refusal_returns_the_same_event_for_ordered_retry() {
        let journal = EventJournal::new(quota(4096), 1, 4096);
        let subscription = journal.subscribe().unwrap();
        let first_sequence = journal.try_publish(state_event(1), 64).unwrap();
        let original = ServerEvent::Error {
            message: "preserve this exact refusal".to_owned(),
        };
        let failure = journal.try_publish(original.clone(), 64).unwrap_err();
        assert_eq!(failure.refusal, JournalRefusal::EntryLimit);
        assert_eq!(failure.event, original);
        assert_eq!(journal.retained().0, 1);

        let first = subscription.recv().await.unwrap().unwrap();
        subscription.acknowledge(first_sequence).unwrap();
        drop(first);
        let retry_sequence = journal.try_publish(failure.event, 64).unwrap();
        assert_eq!(retry_sequence, first_sequence + 1);
        let retried = subscription.recv().await.unwrap().unwrap();
        assert!(matches!(
            retried.event(),
            ServerEvent::Error { message } if message == "preserve this exact refusal"
        ));
        subscription.acknowledge(retry_sequence).unwrap();
        drop(retried);
    }

    #[tokio::test]
    async fn subscription_rejects_a_second_delivery_until_the_first_is_acknowledged() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let sequence = journal.try_publish(state_event(1), 64).unwrap();

        let first = subscription.recv().await.unwrap().unwrap();
        assert_eq!(first.sequence, sequence);
        assert!(matches!(
            subscription.recv().await,
            Err(JournalRefusal::OutOfOrder)
        ));

        subscription.acknowledge(sequence).unwrap();
        drop(first);
        assert!(subscription.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn acknowledged_entries_release_process_storage() {
        let quota = quota(4096);
        let journal = EventJournal::new(quota.clone(), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let sequence = journal.try_publish(state_event(1), 64).unwrap();
        let before = quota.snapshot().worker_bytes;
        assert!(before > 0);
        let event = subscription.recv().await.unwrap().unwrap();
        subscription.acknowledge(sequence).unwrap();
        assert!(journal.retained().0 > 0);
        assert!(quota.snapshot().worker_bytes > 0);
        drop(event);
        assert_eq!(journal.retained(), (0, 0));
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn reservation_transfers_existing_same_root_storage_without_double_charge() {
        let quota = quota(4096);
        let journal = EventJournal::new(quota.clone(), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let producer_storage = Arc::new(quota.reserve_external_storage(1024).unwrap());
        let before_publish = quota.snapshot().worker_bytes;
        let reservation = journal
            .reserve_with_storage(64, Some(Arc::clone(&producer_storage)))
            .unwrap();
        let sequence = reservation.commit(state_event(1)).unwrap();

        assert_eq!(quota.snapshot().worker_bytes, before_publish);
        let event = subscription.recv().await.unwrap().unwrap();
        subscription.acknowledge(sequence).unwrap();
        assert_eq!(quota.snapshot().worker_bytes, before_publish);
        drop(event);
        assert_eq!(quota.snapshot().worker_bytes, before_publish);
        drop(producer_storage);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn reservation_refusal_returns_existing_producer_storage() {
        let quota = quota(4096);
        let journal = EventJournal::new(quota.clone(), 4, 100);
        let _subscription = journal.subscribe().unwrap();
        let producer_storage = Arc::new(quota.reserve_external_storage(1024).unwrap());
        let before_refusal = quota.snapshot().worker_bytes;

        let failure = match journal.reserve_with_storage(64, Some(Arc::clone(&producer_storage))) {
            Ok(_) => panic!("oversized journal payload unexpectedly reserved"),
            Err(failure) => failure,
        };
        assert_eq!(failure.refusal, JournalRefusal::ByteLimit);
        assert!(Arc::ptr_eq(
            failure.producer_storage.as_ref().unwrap(),
            &producer_storage
        ));
        assert_eq!(
            failure
                .producer_storage
                .as_ref()
                .map(|storage| storage.resident_bytes()),
            Some(1024)
        );
        assert_eq!(quota.snapshot().worker_bytes, before_refusal);
        drop(failure);
        assert_eq!(quota.snapshot().worker_bytes, before_refusal);
        drop(producer_storage);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn storage_remains_charged_until_every_subscriber_releases_the_delivery() {
        let quota = quota(4096);
        let journal = EventJournal::new(quota.clone(), 4, 4096);
        let first = journal.subscribe().unwrap();
        let second = journal.subscribe().unwrap();
        let sequence = journal.try_publish(state_event(1), 64).unwrap();

        let first_event = first.recv().await.unwrap().unwrap();
        first.acknowledge(sequence).unwrap();
        assert!(quota.snapshot().worker_bytes > 0);

        let second_event = second.recv().await.unwrap().unwrap();
        assert!(std::ptr::eq(first_event.event(), second_event.event()));
        second.acknowledge(sequence).unwrap();
        drop(first_event);
        drop(second_event);
        assert_eq!(quota.snapshot().worker_bytes, 0);
        assert_eq!(journal.retained(), (0, 0));
    }

    #[test]
    fn shared_quota_refusal_preserves_event_and_reports_byte_resource() {
        let quota = quota(128);
        let journal = EventJournal::new(quota, 4, 4096);
        let _subscription = journal.subscribe().unwrap();
        let original = state_event(1);
        let failure = journal.try_publish(original.clone(), 128).unwrap_err();
        assert_eq!(failure.event, original);
        assert_eq!(
            failure.refusal,
            JournalRefusal::Storage(RejectReason::WorkerBytes)
        );
    }

    #[test]
    fn local_byte_limit_refusal_preserves_event() {
        let journal = EventJournal::new(quota(4096), 4, 200);
        let _subscription = journal.subscribe().unwrap();
        let original = state_event(1);
        let failure = journal.try_publish(original.clone(), 128).unwrap_err();
        assert_eq!(failure.event, original);
        assert_eq!(failure.refusal, JournalRefusal::ByteLimit);
    }

    #[tokio::test]
    async fn close_drains_accepted_entries_then_ends_receivers() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let sequence = journal.try_publish(state_event(1), 64).unwrap();
        journal.close();
        assert_eq!(
            journal.try_publish(state_event(2), 64).unwrap_err().refusal,
            JournalRefusal::Closed
        );
        let event = subscription.recv().await.unwrap().unwrap();
        subscription.acknowledge(sequence).unwrap();
        assert!(subscription.recv().await.unwrap().is_none());
        drop(event);
    }

    #[test]
    fn disconnect_releases_only_the_disconnected_subscriber_claim() {
        let quota = quota(4096);
        let journal = EventJournal::new(quota.clone(), 4, 4096);
        let first = journal.subscribe().unwrap();
        let second = journal.subscribe().unwrap();
        journal.try_publish(state_event(1), 64).unwrap();

        drop(first);
        assert!(journal.retained().0 > 0);
        assert!(quota.snapshot().worker_bytes > 0);

        drop(second);
        assert_eq!(journal.retained(), (0, 0));
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn subscriber_cannot_acknowledge_past_its_current_event() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let sequence = journal.try_publish(state_event(1), 64).unwrap();
        let event = subscription.recv().await.unwrap().unwrap();

        assert_eq!(
            subscription.acknowledge(sequence + 1),
            Err(JournalRefusal::OutOfOrder)
        );
        assert_eq!(journal.retained().0, 1);
        subscription.acknowledge(sequence).unwrap();
        drop(event);
    }

    #[tokio::test]
    async fn later_committed_event_waits_for_earlier_reserved_slot() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let first = journal.reserve(64).unwrap();
        let second = journal.reserve(64).unwrap();
        let first_sequence = first.sequence();
        let second_sequence = second.sequence();

        second.commit(state_event(2)).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), subscription.recv())
                .await
                .is_err()
        );

        first.commit(state_event(1)).unwrap();
        let first_event = subscription.recv().await.unwrap().unwrap();
        assert_eq!(first_event.sequence, first_sequence);
        assert!(matches!(
            first_event.event(),
            ServerEvent::NodeActivityChanged {
                activity_revision: 1,
                ..
            }
        ));
        subscription.acknowledge(first_sequence).unwrap();
        drop(first_event);

        let second_event = subscription.recv().await.unwrap().unwrap();
        assert_eq!(second_event.sequence, second_sequence);
        assert!(matches!(
            second_event.event(),
            ServerEvent::NodeActivityChanged {
                activity_revision: 2,
                ..
            }
        ));
        subscription.acknowledge(second_sequence).unwrap();
    }

    #[tokio::test]
    async fn canceled_reservation_advances_without_exposing_a_cursor_gap() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let canceled = journal.reserve(64).unwrap();
        let accepted = journal.reserve(64).unwrap();
        let accepted_sequence = accepted.sequence();

        canceled.cancel();
        accepted.commit(state_event(2)).unwrap();

        let event = subscription.recv().await.unwrap().unwrap();
        assert_eq!(event.sequence, accepted_sequence);
        assert!(matches!(
            event.event(),
            ServerEvent::NodeActivityChanged {
                activity_revision: 2,
                ..
            }
        ));
        subscription.acknowledge(accepted_sequence).unwrap();
    }

    #[test]
    fn subscriber_cannot_acknowledge_an_event_it_never_received() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscription = journal.subscribe().unwrap();
        let sequence = journal.try_publish(state_event(1), 64).unwrap();

        assert_eq!(
            subscription.acknowledge(sequence),
            Err(JournalRefusal::OutOfOrder)
        );
        assert_eq!(journal.retained().0, 1);
    }

    #[test]
    fn subscriber_count_is_bounded() {
        let journal = EventJournal::new(quota(4096), 4, 4096);
        let subscriptions: Vec<_> = (0..MAXIMUM_SUBSCRIBERS)
            .map(|_| journal.subscribe().unwrap())
            .collect();

        assert!(matches!(
            journal.subscribe(),
            Err(JournalRefusal::SubscriberLimit)
        ));
        drop(subscriptions);
        assert!(journal.subscribe().is_ok());
    }
}
