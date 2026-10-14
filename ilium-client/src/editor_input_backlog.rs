//! Bounded custody for terminal events read while an editor model is loaned
//! to the CPU execution bank. Events retain their original storage leases.

use crate::terminal_input_owner::{InputEvent, InputFailure};
use ilium_core::NodeId;
use ilium_execution::{QuotaGroup, RejectReason, StorageAdmission};
use std::collections::VecDeque;
use std::sync::Arc;

pub(crate) const MAX_EDITOR_DEFERRED_EVENTS: usize = 256;
pub(crate) const MAX_EDITOR_DEFERRED_BYTES: usize = 32 * 1024 * 1024;
const EVENT_SLOT_BYTES: usize = std::mem::size_of::<QueuedEditorInput>();

pub(crate) struct QueuedEditorInput {
    pub(crate) original: Result<InputEvent, InputFailure>,
    pub(crate) target_pane: Option<NodeId>,
    pub(crate) layout_revision: Option<u64>,
    pub(crate) loan_identity: Option<Arc<()>>,
}

impl QueuedEditorInput {
    pub(crate) fn layout_revision_matches(&self, current: Option<u64>) -> bool {
        self.layout_revision
            .is_none_or(|revision| current == Some(revision))
    }

    pub(crate) fn loan_identity_matches(&self, current: &Arc<()>) -> bool {
        self.loan_identity
            .as_ref()
            .is_some_and(|identity| Arc::ptr_eq(identity, current))
    }
}

pub(crate) struct EditorInputBacklog {
    events: VecDeque<QueuedEditorInput>,
    payload_bytes: usize,
    max_events: usize,
    max_bytes: usize,
    _storage: StorageAdmission,
}

impl EditorInputBacklog {
    pub(crate) fn begin(quota: QuotaGroup) -> Result<Self, RejectReason> {
        Self::begin_with_limits(quota, MAX_EDITOR_DEFERRED_EVENTS, MAX_EDITOR_DEFERRED_BYTES)
    }

    fn begin_with_limits(
        quota: QuotaGroup,
        max_events: usize,
        max_bytes: usize,
    ) -> Result<Self, RejectReason> {
        let storage_bytes = EVENT_SLOT_BYTES
            .checked_mul(max_events)
            .ok_or(RejectReason::WorkerBytes)?;
        let storage = quota.reserve_external_storage(storage_bytes)?;
        let mut events = VecDeque::new();
        events
            .try_reserve_exact(max_events)
            .map_err(|_| RejectReason::WorkerBytes)?;
        Ok(Self {
            events,
            payload_bytes: 0,
            max_events,
            max_bytes,
            _storage: storage,
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub(crate) fn is_full(&self) -> bool {
        self.events.len() >= self.max_events
    }

    pub(crate) fn can_accept(&self, event: &Result<InputEvent, InputFailure>) -> bool {
        if self.events.len() >= self.max_events {
            return false;
        }
        let Ok(event) = event else {
            // Failures may recursively retain an original event. Keep them in
            // the existing input backlog, whose custody already includes it.
            return false;
        };
        let payload_bytes = event.retained_payload_bytes();
        self.payload_bytes
            .checked_add(payload_bytes)
            .is_some_and(|total| total <= self.max_bytes)
    }

    pub(crate) fn push(
        &mut self,
        event: Result<InputEvent, InputFailure>,
        target_pane: Option<NodeId>,
        layout_revision: Option<u64>,
        loan_identity: Option<Arc<()>>,
    ) -> Result<(), Result<InputEvent, InputFailure>> {
        let payload_bytes = match &event {
            Ok(event) => event.retained_payload_bytes(),
            Err(_) => return Err(event),
        };
        let Some(total_payload_bytes) = self.payload_bytes.checked_add(payload_bytes) else {
            return Err(event);
        };
        if self.events.len() >= self.max_events || total_payload_bytes > self.max_bytes {
            return Err(event);
        }
        self.payload_bytes = total_payload_bytes;
        self.events.push_back(QueuedEditorInput {
            original: event,
            target_pane,
            layout_revision,
            loan_identity,
        });
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<QueuedEditorInput> {
        let event = self.events.pop_front()?;
        if let Ok(event) = &event.original {
            self.payload_bytes = self
                .payload_bytes
                .saturating_sub(event.retained_payload_bytes());
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn queue_retains_original_input_envelopes_in_fifo_order() {
        let (first, quota) = crate::terminal_input_owner::key_fixture(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ));
        let (second, _) = crate::terminal_input_owner::key_fixture(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        ));
        let mut queue = EditorInputBacklog::begin_with_limits(quota, 4, 1024).unwrap();

        queue
            .push(Ok(first), Some(NodeId(1)), Some(3), None)
            .unwrap();
        queue
            .push(Ok(second), Some(NodeId(2)), Some(3), None)
            .unwrap();

        assert_eq!(
            queue
                .pop()
                .unwrap()
                .original
                .unwrap()
                .dispatch(|event| event),
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        );
        assert_eq!(
            queue
                .pop()
                .unwrap()
                .original
                .unwrap()
                .dispatch(|event| event),
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn full_queue_returns_the_original_event_without_disposing_it() {
        let (event, quota) = crate::terminal_input_owner::key_fixture(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ));
        let (first, _) = crate::terminal_input_owner::key_fixture(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));
        let mut queue = EditorInputBacklog::begin_with_limits(quota, 1, 0).unwrap();
        queue.push(Ok(first), None, None, None).unwrap();

        let returned = queue.push(Ok(event), None, None, None).unwrap_err();
        assert_eq!(
            queue
                .pop()
                .unwrap()
                .original
                .unwrap()
                .dispatch(|event| event),
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
        );
        assert_eq!(
            returned.unwrap().dispatch(|event| event),
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        );
    }

    #[test]
    fn byte_overload_returns_original_paste_and_keeps_prior_fifo_entries() {
        let (first, quota) = crate::terminal_input_owner::key_fixture(KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
        ));
        let (paste, _) = crate::terminal_input_owner::paste_fixture("large enough".into());
        let mut queue = EditorInputBacklog::begin_with_limits(quota, 4, 4).unwrap();
        queue.push(Ok(first), None, None, None).unwrap();

        let returned = queue.push(Ok(paste), None, None, None).unwrap_err();
        assert_eq!(
            queue
                .pop()
                .unwrap()
                .original
                .unwrap()
                .dispatch(|event| event),
            Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE))
        );
        assert_eq!(
            returned.unwrap().dispatch_paste(|text| text),
            "large enough"
        );
    }

    #[test]
    fn input_failure_with_retained_original_stays_out_of_byte_accounted_queue() {
        let (original, quota) = crate::terminal_input_owner::paste_fixture("retained".into());
        let failure = InputFailure::undispatched(original, None);
        let mut queue = EditorInputBacklog::begin_with_limits(quota, 4, 1024).unwrap();
        let rejected = Err(failure);

        assert!(!queue.can_accept(&rejected));
        let returned = queue.push(rejected, None, None, None).unwrap_err();
        assert!(returned.is_err());
    }

    #[test]
    fn replay_fences_stale_geometry_and_replaced_pane_identity() {
        let (event, quota) = crate::terminal_input_owner::key_fixture(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));
        let mut queue = EditorInputBacklog::begin_with_limits(quota, 1, 1024).unwrap();
        let original_identity = Arc::new(());
        queue
            .push(
                Ok(event),
                Some(NodeId(7)),
                Some(11),
                Some(Arc::clone(&original_identity)),
            )
            .unwrap();
        let queued = queue.pop().unwrap();

        assert!(queued.layout_revision_matches(Some(11)));
        assert!(!queued.layout_revision_matches(Some(12)));
        assert!(queued.loan_identity_matches(&original_identity));
        assert!(!queued.loan_identity_matches(&Arc::new(())));
        assert!(queued.original.is_ok());
    }
}
