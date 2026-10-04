//! Ordered input waiting for the terminal owner's negotiated-mode barrier.
use ilium_core::NodeId;
use ilium_ipc::ClientRequest;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub(crate) type BarrierModes = (bool, bool, u64, u64);
type ReadyFront<'a> = (&'a InputIntent, BarrierModes);

const MAX_INPUTS: usize = 256;
pub(crate) const MAX_BYTES: usize = 64 * 1024 * 1024;

/// One FIFO head awaiting local capacity. Identity is captured once, before
/// backpressure; focus changes never redirect the original semantic Paste.
pub(crate) struct NativePaste {
    pub(crate) pane_id: NodeId,
    pub(crate) identity: Arc<()>,
    pub(crate) event: crate::terminal_input_owner::InputEvent,
}

pub(crate) enum InputIntent {
    AdmittedRequest(crate::ipc_preparation::AdmittedRequest),
    Paste(String),
    NativePaste(crate::terminal_input_owner::InputEvent),
    LinkProbe {
        id: u64,
        request: crate::ipc_preparation::AdmittedRequest,
        completion: Option<crate::terminal_context_preparation::LinkCompletion>,
        result_capacity: usize,
        layout_revision: Option<u64>,
        /// The original click's recovery policy, before later server updates.
        forward_mouse: bool,
    },
    ClipboardPaste {
        id: u64,
        completion: Option<ilium_execution::Retained<crate::terminal_clipboard::Completion>>,
    },
    Wheel {
        request: ClientRequest,
        up: bool,
        recovery: bool,
        layout_revision: Option<u64>,
    },
}
impl InputIntent {
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::AdmittedRequest(request) => {
                crate::ipc_preparation::request_retained_bytes(request.view())
            }
            Self::LinkProbe {
                request,
                result_capacity,
                ..
            } => crate::ipc_preparation::request_retained_bytes(request.view())
                .saturating_add(*result_capacity),
            Self::Paste(text) => text.capacity().saturating_add(12),
            Self::NativePaste(event) => event.retained_payload_bytes().saturating_add(12),
            Self::ClipboardPaste {
                completion: Some(completion),
                ..
            } => match &completion.view().result {
                Ok(text) | Err(text) => text.capacity().saturating_add(12),
            },
            _ => std::mem::size_of::<Self>(),
        }
    }
}
struct PendingInput {
    identity: Arc<()>,
    intent: InputIntent,
    generation: Option<u64>,
    bytes: usize,
    ready_barrier: Option<(bool, bool, u64, u64)>,
}
#[derive(Default)]
pub(crate) struct TerminalInput {
    panes: HashMap<NodeId, VecDeque<PendingInput>>,
    bytes: usize,
    count: usize,
    generation: u64,
}
impl TerminalInput {
    /// Exceptional client shutdown transfers original native envelopes into
    /// the returned failure, rather than destroying accepted Paste silently.
    pub(crate) fn take_native_paste(&mut self) -> Option<crate::terminal_input_owner::InputEvent> {
        let (pane, index) = self.panes.iter().find_map(|(pane, queue)| {
            queue
                .iter()
                .position(|pending| matches!(pending.intent, InputIntent::NativePaste(_)))
                .map(|index| (*pane, index))
        })?;
        let queue = self.panes.get_mut(&pane)?;
        let pending = queue.remove(index)?;
        self.bytes -= pending.bytes;
        self.count -= 1;
        if queue.is_empty() {
            self.panes.remove(&pane);
        }
        match pending.intent {
            InputIntent::NativePaste(event) => Some(event),
            _ => unreachable!("the located original is a native Paste"),
        }
    }

    pub(crate) fn can_admit(&self, bytes: usize) -> bool {
        self.count < MAX_INPUTS && self.bytes.saturating_add(bytes) <= MAX_BYTES
    }
    pub(crate) fn admit_admitted_request(
        &mut self,
        pane: NodeId,
        identity: Arc<()>,
        request: crate::ipc_preparation::AdmittedRequest,
    ) -> Result<(), Box<crate::ipc_preparation::AdmittedRequest>> {
        match self.admit(pane, identity, InputIntent::AdmittedRequest(request)) {
            Ok(()) => Ok(()),
            Err(intent) => match *intent {
                InputIntent::AdmittedRequest(request) => Err(Box::new(request)),
                _ => unreachable!("admit returns the exact supplied intent"),
            },
        }
    }

    pub(crate) fn admit(
        &mut self,
        pane: NodeId,
        identity: Arc<()>,
        intent: InputIntent,
    ) -> Result<(), Box<InputIntent>> {
        let bytes = intent.bytes();
        if !self.can_admit(bytes) {
            return Err(Box::new(intent));
        }
        self.panes.entry(pane).or_default().push_back(PendingInput {
            identity,
            intent,
            generation: None,
            bytes,
            ready_barrier: None,
        });
        self.count += 1;
        self.bytes += bytes;
        Ok(())
    }
    pub(crate) fn needs_barriers(&self) -> Vec<(NodeId, Arc<()>)> {
        self.panes
            .iter()
            .filter_map(|(id, queue)| {
                queue
                    .front()
                    .filter(|pending| pending.generation.is_none())
                    .map(|pending| (*id, pending.identity.clone()))
            })
            .collect()
    }
    pub(crate) fn next_generation(&mut self) -> Option<u64> {
        self.generation = self.generation.checked_add(1)?;
        Some(self.generation)
    }
    pub(crate) fn barrier_started(&mut self, pane: NodeId, generation: u64) {
        if let Some(pending) = self
            .panes
            .get_mut(&pane)
            .and_then(|queue| queue.front_mut())
        {
            pending.generation = Some(generation);
        }
    }
    pub(crate) fn complete(
        &mut self,
        pane: NodeId,
        identity: &Arc<()>,
        generation: u64,
    ) -> Option<InputIntent> {
        let queue = self.panes.get_mut(&pane)?;
        let pending = queue.front()?;
        if pending.generation != Some(generation) || !Arc::ptr_eq(identity, &pending.identity) {
            return None;
        }
        if matches!(
            &pending.intent,
            InputIntent::ClipboardPaste {
                completion: None,
                ..
            } | InputIntent::LinkProbe {
                completion: None,
                ..
            }
        ) {
            return None;
        }
        let pending = queue.pop_front()?;
        self.bytes -= pending.bytes;
        self.count -= 1;
        if queue.is_empty() {
            self.panes.remove(&pane);
        }
        Some(pending.intent)
    }
    /// Preserve the negotiated modes at the paste's original ordered barrier;
    /// later keys remain behind this head until the native read acknowledges.
    #[cfg(test)]
    pub(crate) fn record_clipboard_barrier(
        &mut self,
        pane: NodeId,
        identity: &Arc<()>,
        generation: u64,
        modes: (bool, bool, u64, u64),
    ) -> bool {
        let Some(pending) = self
            .panes
            .get_mut(&pane)
            .and_then(|queue| queue.front_mut())
        else {
            return false;
        };
        if pending.generation != Some(generation)
            || !Arc::ptr_eq(identity, &pending.identity)
            || !matches!(&pending.intent, InputIntent::ClipboardPaste { .. })
        {
            return false;
        }
        pending.ready_barrier = Some(modes);
        true
    }
    /// Record readiness without consuming accepted input. The outbound owner
    /// must reserve capacity before calling complete_ready.
    pub(crate) fn record_ready_barrier(
        &mut self,
        pane: NodeId,
        identity: &Arc<()>,
        generation: u64,
        modes: (bool, bool, u64, u64),
    ) -> bool {
        let Some(pending) = self
            .panes
            .get_mut(&pane)
            .and_then(|queue| queue.front_mut())
        else {
            return false;
        };
        if pending.generation != Some(generation) || !Arc::ptr_eq(identity, &pending.identity) {
            return false;
        }
        pending.ready_barrier = Some(modes);
        true
    }
    pub(crate) fn ready_front(&self, pane: NodeId) -> Option<ReadyFront<'_>> {
        let pending = self.panes.get(&pane)?.front()?;
        if matches!(
            &pending.intent,
            InputIntent::ClipboardPaste {
                completion: None,
                ..
            } | InputIntent::LinkProbe {
                completion: None,
                ..
            }
        ) {
            return None;
        }
        Some((&pending.intent, pending.ready_barrier?))
    }
    pub(crate) fn ready_panes(&self) -> Vec<NodeId> {
        self.panes
            .keys()
            .filter(|pane| self.ready_front(**pane).is_some())
            .copied()
            .collect()
    }
    pub(crate) fn complete_ready(
        &mut self,
        pane: NodeId,
    ) -> Option<(InputIntent, (bool, bool, u64, u64))> {
        let pending = self.panes.get(&pane)?.front()?;
        let modes = self.ready_front(pane)?.1;
        let identity = pending.identity.clone();
        let generation = pending.generation?;
        self.complete(pane, &identity, generation)
            .map(|intent| (intent, modes))
    }
    pub(crate) fn finish_link(
        &mut self,
        completion: crate::terminal_context_preparation::LinkCompletion,
    ) -> bool {
        let Some(queue) = self.panes.get_mut(&completion.pane_id) else {
            return false;
        };
        for pending in queue {
            if !Arc::ptr_eq(&pending.identity, &completion.identity) {
                continue;
            }
            if let InputIntent::LinkProbe {
                id,
                completion: result,
                ..
            } = &mut pending.intent
            {
                if *id == completion.id && result.is_none() {
                    *result = Some(completion);
                    return true;
                }
            }
        }
        false
    }
    pub(crate) fn finish_clipboard(
        &mut self,
        id: u64,
        completion: ilium_execution::Retained<crate::terminal_clipboard::Completion>,
    ) -> Option<NodeId> {
        for (pane, queue) in &mut self.panes {
            for pending in queue {
                if let InputIntent::ClipboardPaste {
                    id: expected,
                    completion: result,
                } = &mut pending.intent
                {
                    if *expected == id {
                        *result = Some(completion);
                        return Some(*pane);
                    }
                }
            }
        }
        None
    }
    #[cfg(test)]
    pub(crate) fn take_ready_clipboard(
        &mut self,
        pane: NodeId,
    ) -> Option<(InputIntent, (bool, bool, u64, u64))> {
        let pending = self.panes.get(&pane)?.front()?;
        if !matches!(
            &pending.intent,
            InputIntent::ClipboardPaste {
                completion: Some(_),
                ..
            }
        ) {
            return None;
        }
        let modes = pending.ready_barrier?;
        let identity = pending.identity.clone();
        let generation = pending.generation?;
        self.complete(pane, &identity, generation)
            .map(|intent| (intent, modes))
    }
    /// Authoritative removal/replacement explicitly cancels the old destination.
    pub(crate) fn retain(&mut self, mut live: impl FnMut(NodeId, &Arc<()>) -> bool) -> usize {
        let mut removed = 0;
        self.panes.retain(|id, queue| {
            queue.retain(|pending| {
                if live(*id, &pending.identity) {
                    return true;
                }
                self.bytes -= pending.bytes;
                self.count -= 1;
                removed += 1;
                false
            });
            !queue.is_empty()
        });
        removed
    }
    pub(crate) fn pending_count(&self) -> usize {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn link_request(client: &ilium_execution::Client) -> crate::ipc_preparation::AdmittedRequest {
        let mut original = ClientRequest::MouseInput {
            pane_id: NodeId(41),
            kind: ilium_ipc::MouseEventKind::Down(ilium_ipc::MouseButton::Left),
            column: 17,
            row: 6,
            modifiers: ilium_ipc::MouseModifiers {
                control: true,
                ..Default::default()
            },
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::ipc_preparation::admit_request(client, original) {
                Ok(request) => return request,
                Err(rejected) => {
                    assert_eq!(rejected.reason, ilium_execution::RejectReason::Busy);
                    assert!(std::time::Instant::now() < deadline);
                    original = rejected.value;
                    std::thread::yield_now();
                }
            }
        }
    }
    fn no_link(
        client: &ilium_execution::Client,
    ) -> ilium_execution::Retained<crate::terminal_context_preparation::PreparedLink> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let reservation = loop {
            match client.try_reserve_external(ilium_execution::JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            }) {
                Ok(reservation) => break reservation,
                Err(ilium_execution::RejectReason::Busy)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::yield_now()
                }
                Err(reason) => panic!("link fixture admission: {reason:?}"),
            }
        };
        reservation
            .retain(crate::terminal_context_preparation::PreparedLink {
                worker_thread: std::thread::current().id(),
                link: None,
                storage: None,
            })
            .unwrap()
    }
    #[test]
    fn no_link_retains_original_mouse_before_later_key_and_ignores_new_geometry() {
        let client = crate::execution::test_client();
        let identity = Arc::new(());
        let mut queue = TerminalInput::default();
        assert!(queue
            .admit(
                NodeId(41),
                identity.clone(),
                InputIntent::LinkProbe {
                    id: 9,
                    request: link_request(&client),
                    completion: None,
                    result_capacity: 128 * 1024,
                    layout_revision: Some(77),
                    forward_mouse: true,
                }
            )
            .is_ok());
        assert!(queue
            .admit(
                NodeId(41),
                identity.clone(),
                InputIntent::Paste("later key".into())
            )
            .is_ok());
        queue.barrier_started(NodeId(41), 5);
        assert!(queue.record_ready_barrier(NodeId(41), &identity, 5, (true, false, 31, 32)));
        assert!(queue.complete_ready(NodeId(41)).is_none());
        assert!(queue.needs_barriers().is_empty());
        assert!(
            !queue.finish_link(crate::terminal_context_preparation::LinkCompletion {
                id: 9,
                pane_id: NodeId(41),
                identity: Arc::new(()),
                result: Ok(no_link(&client)),
            })
        );
        assert!(
            queue.finish_link(crate::terminal_context_preparation::LinkCompletion {
                id: 9,
                pane_id: NodeId(41),
                identity: identity.clone(),
                result: Ok(no_link(&client)),
            })
        );
        // A duplicate/failure may not replace the accepted successful result.
        assert!(
            !queue.finish_link(crate::terminal_context_preparation::LinkCompletion {
                id: 9,
                pane_id: NodeId(41),
                identity: identity.clone(),
                result: Err("duplicate".into()),
            })
        );
        let (intent, modes) = queue.complete_ready(NodeId(41)).unwrap();
        assert_eq!(modes, (true, false, 31, 32));
        match intent {
            InputIntent::LinkProbe {
                request,
                completion,
                layout_revision,
                forward_mouse,
                ..
            } => {
                assert!(
                    forward_mouse,
                    "later state must not overwrite the captured click policy"
                );
                assert_eq!(layout_revision, Some(77));
                assert!(completion.unwrap().result.unwrap().view().link.is_none());
                assert!(
                    matches!(request.view(), ClientRequest::MouseInput { pane_id: NodeId(41), column: 17, row: 6, modifiers, .. } if modifiers.control)
                );
            }
            _ => panic!("original link placeholder changed"),
        }
        assert!(queue.ready_front(NodeId(41)).is_none());
        queue.barrier_started(NodeId(41), 6);
        assert!(queue.record_ready_barrier(NodeId(41), &identity, 6, (false, true, 35, 36)));
        assert!(
            matches!(queue.complete_ready(NodeId(41)), Some((InputIntent::Paste(text), (false,true,35,36))) if text == "later key")
        );
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.bytes, 0);
    }
    #[test]
    fn link_completion_before_barrier_and_confirmed_replacement_preserve_custody() {
        let client = crate::execution::test_client();
        let identity = Arc::new(());
        let mut queue = TerminalInput::default();
        assert!(queue
            .admit(
                NodeId(41),
                identity.clone(),
                InputIntent::LinkProbe {
                    id: 10,
                    request: link_request(&client),
                    completion: None,
                    result_capacity: 128 * 1024,
                    layout_revision: Some(78),
                    forward_mouse: false,
                }
            )
            .is_ok());
        assert!(
            queue.finish_link(crate::terminal_context_preparation::LinkCompletion {
                id: 10,
                pane_id: NodeId(41),
                identity: identity.clone(),
                result: Err("worker failure".into()),
            })
        );
        assert!(queue.ready_front(NodeId(41)).is_none());
        assert_eq!(
            queue.retain(|_, current| Arc::ptr_eq(current, &identity)),
            0
        );
        assert_eq!(queue.pending_count(), 1);
        // Only authoritative instance replacement cancels the original head.
        let replacement = Arc::new(());
        assert_eq!(
            queue.retain(|_, current| Arc::ptr_eq(current, &replacement)),
            1
        );
        assert!(
            !queue.finish_link(crate::terminal_context_preparation::LinkCompletion {
                id: 10,
                pane_id: NodeId(41),
                identity,
                result: Ok(no_link(&client)),
            })
        );
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.bytes, 0);
    }
    #[test]
    fn outbound_readiness_never_pops_admitted_bytes_before_capacity_reservation() {
        let mut queue = TerminalInput::default();
        let identity = Arc::new(());
        assert!(queue
            .admit(
                NodeId(1),
                identity.clone(),
                InputIntent::Paste("retained".into())
            )
            .is_ok());
        queue.barrier_started(NodeId(1), 5);
        assert!(queue.record_ready_barrier(NodeId(1), &identity, 5, (true, false, 7, 9)));
        for _ in 0..3 {
            assert!(
                matches!(queue.ready_front(NodeId(1)),Some((InputIntent::Paste(text),(true,false,7,9))) if text=="retained")
            );
            assert_eq!(queue.pending_count(), 1);
        }
        assert_eq!(queue.ready_panes(), vec![NodeId(1)]);
        assert!(
            matches!(queue.complete_ready(NodeId(1)),Some((InputIntent::Paste(text),(true,false,7,9))) if text=="retained")
        );
        assert_eq!(queue.pending_count(), 0);
    }
    #[test]
    fn barriers_preserve_original_destination_and_fifo() {
        let mut queue = TerminalInput::default();
        let identity = Arc::new(());
        assert!(queue
            .admit(
                NodeId(1),
                identity.clone(),
                InputIntent::Paste("first".into())
            )
            .is_ok());
        assert!(queue
            .admit(
                NodeId(1),
                identity.clone(),
                InputIntent::Paste("second".into())
            )
            .is_ok());
        queue.barrier_started(NodeId(1), 9);
        assert!(queue.complete(NodeId(1), &Arc::new(()), 9).is_none());
        assert!(queue.complete(NodeId(1), &identity, 8).is_none());
        assert!(
            matches!(queue.complete(NodeId(1),&identity,9),Some(InputIntent::Paste(text)) if text=="first")
        );
        assert_eq!(queue.needs_barriers().len(), 1);
        queue.barrier_started(NodeId(1), 10);
        assert!(
            matches!(queue.complete(NodeId(1),&identity,10),Some(InputIntent::Paste(text)) if text=="second")
        );
        assert_eq!(queue.pending_count(), 0);
    }
    #[test]
    fn clipboard_placeholder_holds_order_and_original_negotiated_modes() {
        let mut queue = TerminalInput::default();
        let identity = Arc::new(());
        assert!(queue
            .admit(
                NodeId(1),
                identity.clone(),
                InputIntent::ClipboardPaste {
                    id: 7,
                    completion: None
                }
            )
            .is_ok());
        assert!(queue
            .admit(
                NodeId(1),
                identity.clone(),
                InputIntent::Paste("later key".into())
            )
            .is_ok());
        queue.barrier_started(NodeId(1), 4);
        assert!(queue.record_clipboard_barrier(NodeId(1), &identity, 4, (true, true, 11, 13)));
        assert!(queue.complete(NodeId(1), &identity, 4).is_none());
        assert!(queue.take_ready_clipboard(NodeId(1)).is_none());
        assert!(queue.needs_barriers().is_empty());
        assert_eq!(queue.pending_count(), 2);
        assert_eq!(
            queue.panes[&NodeId(1)][0].ready_barrier,
            Some((true, true, 11, 13))
        );
        assert_eq!(queue.retain(|_, _| false), 2);
    }
    #[test]
    fn native_paste_local_refusal_retry_and_shutdown_transfer_preserve_original_lease() {
        let mut queue = TerminalInput::default();
        let identity = Arc::new(());
        for _ in 0..MAX_INPUTS {
            assert!(queue
                .admit(NodeId(1), identity.clone(), InputIntent::Paste("x".into()))
                .is_ok());
        }
        let mut text = String::from("whole original");
        text.reserve(1024);
        let pointer = text.as_ptr();
        let (event, quota) = crate::terminal_input_owner::paste_fixture(text);
        let charged = quota.snapshot().worker_bytes;
        let rejected = queue
            .admit(NodeId(1), identity.clone(), InputIntent::NativePaste(event))
            .unwrap_err();
        let InputIntent::NativePaste(event) = *rejected else {
            panic!("exact original intent required")
        };
        assert!(
            matches!(event.view(), crossterm::event::Event::Paste(text) if text.as_ptr() == pointer)
        );
        assert_eq!(quota.snapshot().worker_bytes, charged);
        queue.barrier_started(NodeId(1), 1);
        assert!(queue.complete(NodeId(1), &identity, 1).is_some());
        assert!(queue
            .admit(NodeId(1), identity, InputIntent::NativePaste(event))
            .is_ok());
        assert_eq!(queue.pending_count(), MAX_INPUTS);
        let original = queue.take_native_paste().unwrap();
        assert_eq!(queue.pending_count(), MAX_INPUTS - 1);
        assert!(
            matches!(original.view(), crossterm::event::Event::Paste(text) if text.as_ptr() == pointer)
        );
        assert_eq!(quota.snapshot().worker_bytes, charged);
        let failure = crate::terminal_input_owner::InputFailure::undispatched(original, None);
        assert_eq!(quota.snapshot().worker_bytes, charged);
        drop(failure);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn full_input_queue_returns_exact_original_and_confirmed_removal_cancels_it() {
        let mut queue = TerminalInput::default();
        let identity = Arc::new(());
        for _ in 0..MAX_INPUTS {
            assert!(queue
                .admit(NodeId(1), identity.clone(), InputIntent::Paste("x".into()))
                .is_ok());
        }
        let rejected = queue.admit(
            NodeId(1),
            identity,
            InputIntent::Paste("retain this".into()),
        );
        assert!(
            matches!(rejected.map_err(|intent| *intent),Err(InputIntent::Paste(text)) if text=="retain this")
        );
        assert_eq!(queue.pending_count(), MAX_INPUTS);
        assert_eq!(queue.retain(|_, _| false), MAX_INPUTS);
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.bytes, 0);
    }
}
