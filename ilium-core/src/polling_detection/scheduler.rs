//! Pure admission order; payload admission and physical task custody belong
//! to the coordinator. Never release an active slot merely because its source closed.
use super::AgentBinding;

#[derive(Debug, PartialEq, Eq)]
pub struct PendingDecision<T> {
    pub binding: AgentBinding,
    pub window_id: String,
    pub payload: T,
}

#[derive(Debug, PartialEq, Eq)]
pub enum QueueOffer<T> {
    Queued,
    Coalesced(T),
    Duplicate(T),
    Full(T),
    Invalid(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchSlot {
    epoch: u128,
    sequence: u64,
}

pub struct ScheduledDecision<T> {
    pub slot: DispatchSlot,
    pub candidate: PendingDecision<T>,
}

struct ActiveDecision {
    slot: DispatchSlot,
    binding: AgentBinding,
    window_id: String,
}

/// Own only already-admitted payloads. `complete` is called after physical
/// provider work has settled, even when its source or leader became obsolete.
pub struct DecisionQueue<T> {
    epoch: u128,
    sequence: u64,
    pending: std::collections::VecDeque<PendingDecision<T>>,
    active: Vec<ActiveDecision>,
    coalesced: u64,
    dropped_capacity: u64,
}

impl<T> DecisionQueue<T> {
    pub fn new(epoch: u128) -> Option<Self> {
        (epoch != 0).then_some(Self {
            epoch,
            sequence: 0,
            pending: std::collections::VecDeque::new(),
            active: Vec::new(),
            coalesced: 0,
            dropped_capacity: 0,
        })
    }

    pub fn offer(&mut self, candidate: PendingDecision<T>) -> QueueOffer<T> {
        if candidate.binding.pane == crate::ROOT_ID
            || [
                &candidate.binding.session,
                &candidate.binding.incarnation,
                &candidate.window_id,
            ]
            .iter()
            .any(|value| value.is_empty() || value.len() > 256)
        {
            return QueueOffer::Invalid(candidate.payload);
        }
        if self.active.iter().any(|active| {
            active.binding == candidate.binding && active.window_id == candidate.window_id
        }) {
            return QueueOffer::Duplicate(candidate.payload);
        }
        if let Some(pending) = self
            .pending
            .iter_mut()
            .find(|pending| pending.binding == candidate.binding)
        {
            if pending.window_id == candidate.window_id {
                return QueueOffer::Duplicate(candidate.payload);
            }
            let previous = std::mem::replace(pending, candidate);
            self.coalesced = self.coalesced.saturating_add(1);
            return QueueOffer::Coalesced(previous.payload);
        }
        if self.pending.len() >= 128 {
            self.dropped_capacity = self.dropped_capacity.saturating_add(1);
            return QueueOffer::Full(candidate.payload);
        }
        self.pending.push_back(candidate);
        QueueOffer::Queued
    }

    pub fn take_ready(&mut self) -> Option<ScheduledDecision<T>> {
        if self.active.len() >= 2 {
            return None;
        }
        let index = self.pending.iter().position(|pending| {
            !self
                .active
                .iter()
                .any(|active| active.binding == pending.binding)
        })?;
        let sequence = self.sequence.checked_add(1)?;
        let candidate = self.pending.remove(index)?;
        let slot = DispatchSlot {
            epoch: self.epoch,
            sequence,
        };
        self.sequence = sequence;
        self.active.push(ActiveDecision {
            slot,
            binding: candidate.binding.clone(),
            window_id: candidate.window_id.clone(),
        });
        Some(ScheduledDecision { slot, candidate })
    }

    pub fn complete(&mut self, slot: DispatchSlot) -> bool {
        if slot.epoch != self.epoch {
            return false;
        }
        let Some(index) = self.active.iter().position(|active| active.slot == slot) else {
            return false;
        };
        self.active.swap_remove(index);
        true
    }

    /// Closing a generation cannot release its still-running provider slot.
    pub fn discard_pending(&mut self, binding: &AgentBinding) -> Option<T> {
        let index = self
            .pending
            .iter()
            .position(|pending| &pending.binding == binding)?;
        self.pending.remove(index).map(|pending| pending.payload)
    }

    pub fn coalesced(&self) -> u64 {
        self.coalesced
    }
    pub fn dropped_capacity(&self) -> u64 {
        self.dropped_capacity
    }
}
