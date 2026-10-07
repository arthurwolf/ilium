//! Incremental mapping of one immutable paste into its existing key contract.
//! The dispatch owner retains the original input envelope across bounded turns.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ilium_execution::{RejectReason, RetirementHandle, RetirementReservation, Retiring};
use std::mem::size_of;

use crate::terminal_input_owner::InputEvent;

#[derive(Default)]
pub(crate) struct PasteCursor {
    consumed_bytes: usize,
}

impl PasteCursor {
    /// Always ends at a UTF-8 boundary; CRLF is consumed atomically as one key.
    pub(crate) fn next_key(&mut self, original: &str) -> Option<KeyEvent> {
        let suffix = &original[self.consumed_bytes..];
        let character = suffix.chars().next()?;
        self.consumed_bytes += character.len_utf8();
        let code = match character {
            '\r' => {
                if suffix.as_bytes().get(1) == Some(&b'\n') {
                    self.consumed_bytes += 1;
                }
                KeyCode::Enter
            }
            '\n' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            character => KeyCode::Char(character),
        };
        Some(KeyEvent::new(code, KeyModifiers::NONE))
    }
}

/// Owns the original admitted input while key events are replayed over
/// multiple event-loop turns. Keeping the envelope here preserves its storage
/// and slot leases until the final derived key has been dispatched.
pub(crate) struct PasteReplay {
    original: InputEvent,
    cursor: PasteCursor,
    retirement: Option<RetirementReservation<RetiredPasteInput>>,
    #[cfg(test)]
    drop_probe: Option<std::sync::mpsc::Sender<std::thread::ThreadId>>,
}

struct RetiredPasteInput {
    original: Option<InputEvent>,
    #[cfg(test)]
    drop_probe: Option<std::sync::mpsc::Sender<std::thread::ThreadId>>,
}

impl Drop for RetiredPasteInput {
    fn drop(&mut self) {
        drop(self.original.take());
        #[cfg(test)]
        if let Some(probe) = self.drop_probe.take() {
            let _ = probe.send(std::thread::current().id());
        }
    }
}

impl PasteReplay {
    pub(crate) fn new(original: InputEvent) -> Self {
        assert!(
            matches!(original.view(), crossterm::event::Event::Paste(_)),
            "paste replay requires an original Paste input"
        );
        Self {
            original,
            cursor: PasteCursor::default(),
            retirement: None,
            #[cfg(test)]
            drop_probe: None,
        }
    }

    /// Reserve CPU-owner disposal before consuming the first pasted character.
    /// The input envelope keeps its own storage admission while retired, so
    /// this extra declaration covers only the bounded retirement envelope.
    pub(crate) fn reserve_retirement(
        &mut self,
        retirement: &RetirementHandle,
    ) -> Result<(), RejectReason> {
        self.retirement =
            Some(retirement.try_reserve::<RetiredPasteInput>(size_of::<RetiredPasteInput>())?);
        Ok(())
    }

    pub(crate) fn next_keys(&mut self, maximum: usize) -> Vec<KeyEvent> {
        let crossterm::event::Event::Paste(text) = self.original.view() else {
            unreachable!("PasteReplay retains its original Paste event")
        };
        let mut keys = Vec::with_capacity(maximum);
        for _ in 0..maximum {
            let Some(key) = self.cursor.next_key(text) else {
                break;
            };
            keys.push(key);
        }
        keys
    }

    pub(crate) fn consumed_bytes(&self) -> usize {
        self.cursor.consumed_bytes
    }

    pub(crate) fn is_complete(&self) -> bool {
        let crossterm::event::Event::Paste(text) = self.original.view() else {
            unreachable!("PasteReplay retains its original Paste event")
        };
        self.cursor.consumed_bytes == text.len()
    }

    pub(crate) fn into_parts(self) -> (InputEvent, usize) {
        (self.original, self.cursor.consumed_bytes)
    }

    /// Transfer the completed original envelope to the existing CPU owner's
    /// bounded retirement queue before releasing it from the UI loop.
    pub(crate) fn retire_original(self) {
        let Some(reservation) = self.retirement else {
            panic!("completed paste replay must own retirement admission");
        };
        let retired = reservation.attach(RetiredPasteInput {
            original: Some(self.original),
            #[cfg(test)]
            drop_probe: self.drop_probe,
        });
        drop(retired);
    }

    #[cfg(test)]
    fn set_drop_probe(&mut self, probe: std::sync::mpsc::Sender<std::thread::ThreadId>) {
        self.drop_probe = Some(probe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_replay_keeps_the_original_input_lease_until_all_keys_are_dispatched() {
        let (original, quota) = crate::terminal_input_owner::paste_fixture("a\r\né".into());
        let original_pointer = match original.view() {
            crossterm::event::Event::Paste(text) => text.as_ptr(),
            _ => unreachable!("paste fixture contains a paste"),
        };
        let admitted_bytes = quota.snapshot().worker_bytes;
        let mut replay = PasteReplay::new(original);

        assert_eq!(
            replay.next_keys(2),
            vec![
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            ]
        );
        assert_eq!(replay.consumed_bytes(), 3);
        assert!(!replay.is_complete());
        assert_eq!(quota.snapshot().worker_bytes, admitted_bytes);

        assert_eq!(
            replay.next_keys(2),
            vec![KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE)]
        );
        assert!(replay.is_complete());
        assert_eq!(replay.next_keys(2), Vec::<KeyEvent>::new());
        assert_eq!(quota.snapshot().worker_bytes, admitted_bytes);

        let (original, consumed_bytes) = replay.into_parts();
        assert_eq!(consumed_bytes, 5);
        assert!(
            matches!(original.view(), crossterm::event::Event::Paste(text) if text.as_ptr() == original_pointer && text == "a\r\né")
        );
        drop(original);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn unicode_prefix_and_crlf_are_complete_at_each_yield_boundary() {
        let original = "é\r\n🦀\t\rZ\n";
        let mut cursor = PasteCursor::default();
        let expected = [
            (KeyCode::Char('é'), 2),
            (KeyCode::Enter, 4),
            (KeyCode::Char('🦀'), 8),
            (KeyCode::Tab, 9),
            (KeyCode::Enter, 10),
            (KeyCode::Char('Z'), 11),
            (KeyCode::Enter, 12),
        ];
        for (code, consumed_bytes) in expected {
            let key = cursor.next_key(original).unwrap();
            assert_eq!(key, KeyEvent::new(code, KeyModifiers::NONE));
            assert_eq!(cursor.consumed_bytes, consumed_bytes);
            assert!(original.is_char_boundary(consumed_bytes));
        }
        assert_eq!(cursor.next_key(original), None);
        assert_eq!(cursor.next_key(original), None);
        assert_eq!(cursor.consumed_bytes, original.len());
    }

    #[test]
    fn interrupted_replay_failure_retains_full_paste_and_reports_consumed_prefix() {
        let (original, quota) = crate::terminal_input_owner::paste_fixture("a\r\né".into());
        let original_pointer = match original.view() {
            crossterm::event::Event::Paste(text) => text.as_ptr(),
            _ => unreachable!("paste fixture contains a paste"),
        };
        let failure =
            crate::terminal_input_owner::InputFailure::undispatched_after_paste(original, 3, None);

        assert_eq!(failure.consumed_paste_bytes(), Some(3));
        assert!(
            matches!(failure.original().unwrap().view(), crossterm::event::Event::Paste(text) if text == "a\r\né" && text.as_ptr() == original_pointer)
        );
        assert!(failure
            .to_string()
            .contains("consumed paste prefix 3 bytes"));
        assert!(quota.snapshot().worker_bytes > 0);

        drop(failure);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn trailing_cr_and_empty_paste_do_not_require_another_source_event() {
        let mut empty = PasteCursor::default();
        assert_eq!(empty.next_key(""), None);
        assert_eq!(empty.consumed_bytes, 0);
        let mut trailing = PasteCursor::default();
        assert_eq!(trailing.next_key("\r").unwrap().code, KeyCode::Enter);
        assert_eq!(trailing.consumed_bytes, 1);
        assert_eq!(trailing.next_key("\r"), None);
    }

    #[test]
    fn replay_chunk_respects_the_interactive_turn_key_ceiling() {
        let (original, _quota) = crate::terminal_input_owner::paste_fixture(
            "x".repeat(crate::MAX_PASTE_KEYS_PER_TURN + 1),
        );
        let mut replay = PasteReplay::new(original);

        let first_turn = replay.next_keys(crate::MAX_PASTE_KEYS_PER_TURN);

        assert_eq!(first_turn.len(), crate::MAX_PASTE_KEYS_PER_TURN);
        assert_eq!(replay.consumed_bytes(), crate::MAX_PASTE_KEYS_PER_TURN);
        assert!(!replay.is_complete());
        assert_eq!(replay.next_keys(crate::MAX_PASTE_KEYS_PER_TURN).len(), 1);
        assert!(replay.is_complete());
    }

    #[test]
    fn completed_replay_retires_original_input_on_the_cpu_bank() {
        let client = crate::execution::test_client();
        let retirement = client.retirement();
        let (original, quota) = crate::terminal_input_owner::paste_fixture("a paste".into());
        let (drop_sender, drop_receiver) = std::sync::mpsc::channel();
        let ui_thread = std::thread::current().id();
        let mut replay = PasteReplay::new(original);
        replay.reserve_retirement(&retirement).unwrap();
        replay.set_drop_probe(drop_sender);
        assert_eq!(replay.next_keys(64).len(), 7);
        assert!(replay.is_complete());

        replay.retire_original();

        let drop_thread = drop_receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the CPU retirement owner drops the admitted original");
        assert_ne!(drop_thread, ui_thread);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
