use crossterm_winapi::KeyEventRecord;
use std::{collections::VecDeque, time::Duration};

use crossterm_winapi::{Console, Handle, InputRecord};

use crate::event::{
    sys::windows::{parse::MouseButtonsPressed, poll::WinApiPoll},
    Event,
};

#[cfg(feature = "event-stream")]
use crate::event::sys::Waker;
use crate::event::{
    source::EventSource,
    sys::windows::parse::{handle_key_event, handle_mouse_event},
    timeout::PollTimeout,
    InternalEvent,
};

pub(crate) struct WindowsEventSource {
    console: Console,
    poll: WinApiPoll,
    surrogate_buffer: Option<u16>,
    mouse_buttons_pressed: MouseButtonsPressed,
    terminal_query_mode: bool,
    query_bytes: Vec<u8>,
    query_records: Vec<KeyEventRecord>,
    pending: VecDeque<InternalEvent>,
    native: Option<NativeWindowsStorage>,
    delivered_storage: Option<Box<dyn crate::event::native::NativeStorageLease>>,
}

impl WindowsEventSource {
    pub(crate) fn new() -> std::io::Result<WindowsEventSource> {
        let console = Console::from(Handle::current_in_handle()?);
        Ok(WindowsEventSource {
            console,

            #[cfg(not(feature = "event-stream"))]
            poll: WinApiPoll::new(),
            #[cfg(feature = "event-stream")]
            poll: WinApiPoll::new()?,

            surrogate_buffer: None,
            mouse_buttons_pressed: MouseButtonsPressed::default(),
            terminal_query_mode: false,
            query_bytes: Vec::new(),
            query_records: Vec::new(),
            pending: VecDeque::new(),
            native: None,
            delivered_storage: None,
        })
    }
}

struct NativeWindowsStorage {
    admission: std::sync::Arc<dyn crate::event::native::NativeStorage>,
    bytes: Option<Box<dyn crate::event::native::NativeStorageLease>>,
    _records: Box<dyn crate::event::native::NativeStorageLease>,
    _pending: Box<dyn crate::event::native::NativeStorageLease>,
    reply: Option<Box<dyn crate::event::native::NativeStorageLease>>,
}

// One classification byte can exceed the existing reply bound before Other
// flushes it. Eligible query records have exactly one byte each (see query_key).
const OWNED_QUERY_CAPACITY: usize = crate::event::terminal_query::MAX_REPLY_BYTES + 1;

impl WindowsEventSource {
    pub(crate) fn new_owned(
        admission: std::sync::Arc<dyn crate::event::native::NativeStorage>,
    ) -> std::io::Result<Self> {
        let records = admission
            .reserve(OWNED_QUERY_CAPACITY * std::mem::size_of::<KeyEventRecord>())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        let pending = admission
            .reserve_replacement(
                OWNED_QUERY_CAPACITY * std::mem::size_of::<InternalEvent>(),
                OWNED_QUERY_CAPACITY * std::mem::size_of::<KeyEventRecord>(),
            )
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        let mut source = Self::new()?;
        source
            .query_records
            .try_reserve_exact(OWNED_QUERY_CAPACITY)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        source
            .pending
            .try_reserve_exact(OWNED_QUERY_CAPACITY)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        if source.query_records.capacity() != OWNED_QUERY_CAPACITY
            || source.pending.capacity() != OWNED_QUERY_CAPACITY
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "native console allocator returned unexpected capacity",
            ));
        }
        source.native = Some(NativeWindowsStorage {
            admission,
            bytes: None,
            _records: records,
            _pending: pending,
            reply: None,
        });
        source.ensure_native_query_bytes()?;
        Ok(source)
    }

    fn ensure_native_query_bytes(&mut self) -> std::io::Result<()> {
        let Some(native) = self.native.as_mut() else {
            return Ok(());
        };
        if self.query_bytes.capacity() != 0 {
            return Ok(());
        }
        let lease = native
            .admission
            .reserve_replacement(
                OWNED_QUERY_CAPACITY,
                self.query_records.capacity() * std::mem::size_of::<KeyEventRecord>()
                    + self.pending.capacity() * std::mem::size_of::<InternalEvent>(),
            )
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(OWNED_QUERY_CAPACITY)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        if bytes.capacity() != OWNED_QUERY_CAPACITY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "native console allocator returned unexpected capacity",
            ));
        }
        self.query_bytes = bytes;
        native.bytes = Some(lease);
        Ok(())
    }

    fn pop_pending(&mut self) -> Option<InternalEvent> {
        let event = self.pending.pop_front()?;
        if matches!(event, InternalEvent::TerminalReply(_)) {
            self.delivered_storage = self.native.as_mut().and_then(|native| native.reply.take());
        }
        Some(event)
    }

    fn flush_query_records(&mut self) {
        self.query_bytes.clear();
        for record in self.query_records.drain(..) {
            if let Some(event) = handle_key_event(record, &mut self.surrogate_buffer) {
                self.pending.push_back(InternalEvent::Event(event));
            }
        }
    }

    fn query_key(&mut self, record: KeyEventRecord) {
        use crate::event::terminal_query::{reply_prefix, ReplyPrefix};
        let byte = if record.key_down && record.repeat_count == 1 {
            u8::try_from(record.u_char).ok().filter(u8::is_ascii)
        } else {
            None
        };
        if self.query_records.is_empty() && byte != Some(b'\x1b') {
            if let Some(event) = handle_key_event(record, &mut self.surrogate_buffer) {
                self.pending.push_back(InternalEvent::Event(event));
            }
            return;
        }
        let Some(byte) = byte else {
            // A record without a reply byte cannot extend an ASCII query.
            // Replay the prefix first, then convert this original record once
            // with the same surrogate state. Ineligible records never enter
            // the reply buffer, so its byte bound also bounds record storage.
            self.flush_query_records();
            if let Some(event) = handle_key_event(record, &mut self.surrogate_buffer) {
                self.pending.push_back(InternalEvent::Event(event));
            }
            return;
        };
        self.query_bytes.push(byte);
        self.query_records.push(record);
        match reply_prefix(&self.query_bytes) {
            ReplyPrefix::Incomplete => {}
            ReplyPrefix::Complete => {
                self.query_records.clear();
                if let Some(native) = self.native.as_mut() {
                    native.reply = native.bytes.take();
                }
                self.pending
                    .push_back(InternalEvent::TerminalReply(std::mem::take(
                        &mut self.query_bytes,
                    )));
            }
            ReplyPrefix::Other => self.flush_query_records(),
        }
    }
}

impl EventSource for WindowsEventSource {
    fn set_terminal_query_mode(&mut self, active: bool) {
        if !active {
            self.flush_query_records();
        }
        self.terminal_query_mode = active;
    }
    fn take_native_storage(&mut self) -> Option<Box<dyn crate::event::native::NativeStorageLease>> {
        self.delivered_storage.take()
    }

    fn native_has_retained_input(&self) -> bool {
        !self.query_bytes.is_empty()
            || !self.query_records.is_empty()
            || !self.pending.is_empty()
            || self.surrogate_buffer.is_some()
    }

    fn native_retained_original(&self) -> (&[u8], &[u8]) {
        (&self.query_bytes, &[])
    }

    fn try_read(&mut self, timeout: Option<Duration>) -> std::io::Result<Option<InternalEvent>> {
        let poll_timeout = PollTimeout::new(timeout);

        if let Some(event) = self.pop_pending() {
            return Ok(Some(event));
        }
        loop {
            if let Some(event_ready) = self.poll.poll(poll_timeout.leftover())? {
                let number = self.console.number_of_console_input_events()?;
                if event_ready && number != 0 {
                    // Acquire replacement reply storage BEFORE consuming another
                    // OS-owned console record. Refusal leaves that record unread.
                    if self.terminal_query_mode {
                        self.ensure_native_query_bytes()?;
                    }
                    let event = match self.console.read_single_input_event()? {
                        InputRecord::KeyEvent(record) => {
                            if self.terminal_query_mode {
                                self.query_key(record);
                                None
                            } else {
                                handle_key_event(record, &mut self.surrogate_buffer)
                            }
                        }
                        InputRecord::MouseEvent(record) => {
                            let mouse_event =
                                handle_mouse_event(record, &self.mouse_buttons_pressed);
                            self.mouse_buttons_pressed = MouseButtonsPressed {
                                left: record.button_state.left_button(),
                                right: record.button_state.right_button(),
                                middle: record.button_state.middle_button(),
                            };

                            mouse_event
                        }
                        InputRecord::WindowBufferSizeEvent(record) => {
                            // windows starts counting at 0, unix at 1, add one to replicate unix behaviour.
                            Some(Event::Resize(
                                (record.size.x as i32 + 1) as u16,
                                (record.size.y as i32 + 1) as u16,
                            ))
                        }
                        InputRecord::FocusEvent(record) => {
                            let event = if record.set_focus {
                                Event::FocusGained
                            } else {
                                Event::FocusLost
                            };
                            Some(event)
                        }
                        _ => None,
                    };

                    if let Some(event) = event {
                        self.pending.push_back(InternalEvent::Event(event));
                    }
                    if let Some(event) = self.pop_pending() {
                        return Ok(Some(event));
                    }
                }
            }

            if poll_timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker {
        self.poll.waker()
    }
}

#[cfg(test)]
mod native_storage_tests {
    use super::*;
    use crate::event::native::{
        NativeRefusalKind, NativeStorage, NativeStorageLease, NativeStorageRefusal,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Budget {
        used: Arc<AtomicUsize>,
        limit: AtomicUsize,
    }
    struct Lease {
        used: Arc<AtomicUsize>,
        bytes: usize,
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            self.used.fetch_sub(self.bytes, Ordering::SeqCst);
        }
    }
    impl NativeStorage for Budget {
        fn reserve(
            &self,
            bytes: usize,
        ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
            let used = self.used.load(Ordering::SeqCst);
            if bytes > self.limit.load(Ordering::SeqCst).saturating_sub(used) {
                return Err(NativeStorageRefusal {
                    kind: NativeRefusalKind::Busy,
                    requested: bytes,
                });
            }
            self.used.fetch_add(bytes, Ordering::SeqCst);
            Ok(Box::new(Lease {
                used: self.used.clone(),
                bytes,
            }))
        }
    }
    fn budget() -> Arc<Budget> {
        Arc::new(Budget {
            used: Arc::new(AtomicUsize::new(0)),
            limit: AtomicUsize::new(usize::MAX),
        })
    }
    fn key(byte: u8) -> KeyEventRecord {
        KeyEventRecord {
            key_down: true,
            repeat_count: 1,
            virtual_key_code: 0,
            virtual_scan_code: 0,
            u_char: u16::from(byte),
            control_key_state: crossterm_winapi::ControlKeyState(0),
        }
    }

    #[test]
    fn reply_buffer_guard_transfers_and_replacement_refusal_keeps_state_unread() {
        let budget = budget();
        let mut source = WindowsEventSource::new_owned(budget.clone()).unwrap();
        source.set_terminal_query_mode(true);
        let pointer = source.query_bytes.as_ptr();
        for byte in b"\x1b[?1u" {
            source.query_key(key(*byte));
        }
        let Some(InternalEvent::TerminalReply(reply)) = source.pop_pending() else {
            panic!("expected owned reply")
        };
        assert_eq!(reply, b"\x1b[?1u");
        assert_eq!(reply.as_ptr(), pointer);
        let guard = source.take_native_storage();
        let used = budget.used.load(Ordering::SeqCst);
        budget.limit.store(used, Ordering::SeqCst);
        let error = source.ensure_native_query_bytes().unwrap_err();
        assert!(error.get_ref().unwrap().is::<NativeStorageRefusal>());
        assert!(source.query_bytes.is_empty());
        assert_eq!(source.query_bytes.capacity(), 0);
        assert_eq!(budget.used.load(Ordering::SeqCst), used);
        drop(source);
        assert_eq!(budget.used.load(Ordering::SeqCst), reply.capacity());
        drop(reply);
        drop(guard);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn maximum_ascii_prefix_replays_original_records_with_fixed_capacities() {
        let budget = budget();
        let mut source = WindowsEventSource::new_owned(budget.clone()).unwrap();
        source.set_terminal_query_mode(true);
        let used = budget.used.load(Ordering::SeqCst);
        let mut original = vec![key(b'\x1b'), key(b'[')];
        original.extend((0..OWNED_QUERY_CAPACITY - 2).map(|_| key(b'1')));
        let mut surrogate = None;
        let expected: Vec<_> = original
            .iter()
            .cloned()
            .filter_map(|record| handle_key_event(record, &mut surrogate))
            .map(InternalEvent::Event)
            .collect();
        for record in original {
            source.query_key(record);
        }
        assert!(source.query_records.is_empty());
        assert_eq!(source.query_records.capacity(), OWNED_QUERY_CAPACITY);
        assert_eq!(source.pending.capacity(), OWNED_QUERY_CAPACITY);
        assert_eq!(source.query_bytes.capacity(), OWNED_QUERY_CAPACITY);
        assert_eq!(source.pending.iter().cloned().collect::<Vec<_>>(), expected);
        assert_eq!(budget.used.load(Ordering::SeqCst), used);
        drop(source);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn retained_predicate_includes_decoded_queue_records_and_surrogate() {
        let mut source = WindowsEventSource::new_owned(budget()).unwrap();
        assert!(!source.native_has_retained_input());
        source.set_terminal_query_mode(true);
        source.query_key(key(b'\x1b'));
        assert!(source.native_has_retained_input());
        source.set_terminal_query_mode(false);
        assert!(source.query_bytes.is_empty());
        assert!(source.query_records.is_empty());
        assert!(source.native_has_retained_input());
        assert!(source.pop_pending().is_some());
        assert!(!source.native_has_retained_input());
        source.surrogate_buffer = Some(0xd800);
        assert!(source.native_has_retained_input());
        assert!(source.native_retained_original().0.is_empty());
        source.surrogate_buffer = None;
        assert!(!source.native_has_retained_input());
    }
    struct RecordingStorage {
        inner: Arc<Budget>,
        replacements: std::sync::Mutex<Vec<(usize, usize)>>,
    }
    impl NativeStorage for RecordingStorage {
        fn reserve(
            &self,
            bytes: usize,
        ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
            self.inner.reserve(bytes)
        }
        fn reserve_replacement(
            &self,
            new_bytes: usize,
            retained_bytes: usize,
        ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
            self.replacements
                .lock()
                .unwrap()
                .push((new_bytes, retained_bytes));
            self.inner.reserve(new_bytes)
        }
    }
    #[test]
    fn query_replacement_names_only_its_intrinsic_record_and_pending_backings() {
        let storage = Arc::new(RecordingStorage {
            inner: budget(),
            replacements: std::sync::Mutex::new(Vec::new()),
        });
        let mut source = WindowsEventSource::new_owned(storage.clone()).unwrap();
        let records = OWNED_QUERY_CAPACITY * std::mem::size_of::<KeyEventRecord>();
        let pending = OWNED_QUERY_CAPACITY * std::mem::size_of::<InternalEvent>();
        assert_eq!(
            *storage.replacements.lock().unwrap(),
            vec![
                (pending, records),
                (OWNED_QUERY_CAPACITY, records + pending)
            ]
        );
        source.set_terminal_query_mode(true);
        for byte in b"\x1b[?1u" {
            source.query_key(key(*byte));
        }
        let Some(InternalEvent::TerminalReply(reply)) = source.pop_pending() else {
            panic!("expected completed reply")
        };
        let lease = source.take_native_storage();
        source.ensure_native_query_bytes().unwrap();
        assert_eq!(
            storage.replacements.lock().unwrap().last().copied(),
            Some((OWNED_QUERY_CAPACITY, records + pending))
        );
        // The returned reply remains independently owned by its consumer and
        // can release; it is not intrinsic source replacement storage.
        assert_eq!(reply, b"\x1b[?1u");
        drop(reply);
        drop(lease);
    }
}
