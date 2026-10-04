//! Owned native input with storage admission before parser allocation.
//!
//! This reader must be the sole consumer of terminal input. Its ownership is
//! independent of the process-static reader used by the conventional API.
use std::{fmt, io, sync::Arc, time::Duration};

#[cfg(unix)]
use super::timeout::PollTimeout;
use super::{source::EventSource, Event, InternalEvent};

/// A caller-owned reservation. Drop releases the reservation only after the
/// allocation using it is released or transferred to another owner.
pub trait NativeStorageLease: Send + Sync {}
impl<T: Send + Sync> NativeStorageLease for T {}

/// Ownership policy for the backing of one completed valid UTF8 Paste.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePasteCompletion {
    KeepBacking,
    Compact,
}

/// Supplies storage reservations without coupling the terminal adapter to a
/// scheduler or a particular quota implementation.
pub trait NativeStorage: Send + Sync {
    /// Caller-owned resident-capacity policy. Compaction still needs a full
    /// new backing reservation while the raw original remains live. Default
    /// ownership transfer preserves the conventional zero-copy path.
    fn paste_completion(
        &self,
        _payload_bytes: usize,
        _backing_capacity: usize,
    ) -> NativePasteCompletion {
        NativePasteCompletion::KeepBacking
    }

    fn reserve(&self, bytes: usize) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal>;

    /// The caller cannot release this intrinsic old backing until replacement
    /// succeeds. Completed envelopes that other consumers can release are not
    /// part of retained_bytes. Default adapters preserve their existing reserve.
    fn reserve_replacement(
        &self,
        new_bytes: usize,
        _retained_bytes: usize,
    ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
        self.reserve(new_bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRefusalKind {
    Busy,
    Closed,
    Limit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeStorageRefusal {
    pub kind: NativeRefusalKind,
    pub requested: usize,
}
impl fmt::Display for NativeStorageRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "native input storage {:?}: {} bytes",
            self.kind, self.requested
        )
    }
}
impl std::error::Error for NativeStorageRefusal {}

#[derive(Debug)]
pub enum NativeReadError {
    Admission(NativeStorageRefusal),
    Io(io::Error),
}
impl fmt::Display for NativeReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Admission(refusal) => refusal.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}
impl std::error::Error for NativeReadError {}
impl From<io::Error> for NativeReadError {
    fn from(error: io::Error) -> Self {
        match error
            .get_ref()
            .and_then(|error| error.downcast_ref::<NativeStorageRefusal>())
        {
            Some(refusal) => Self::Admission(*refusal),
            None => Self::Io(error),
        }
    }
}

/// Completed data and the reservation owning its backing allocation.
/// Keep these together through every queue and semantic consumer.
pub struct NativeInputEvent {
    input: NativeInput,
    storage: Option<Box<dyn NativeStorageLease>>,
}
#[derive(Debug)]
pub enum NativeInput {
    Event(Event),
    Reply(Vec<u8>),
}
impl NativeInputEvent {
    pub fn input(&self) -> &NativeInput {
        &self.input
    }
    pub fn into_parts(self) -> (NativeInput, Option<Box<dyn NativeStorageLease>>) {
        (self.input, self.storage)
    }
}

/// The complete native source is movable to shutdown/error custody. In
/// particular, an admission refusal never consumes the refused byte or the
/// remainder of the already-read fixed-size chunk. Retry uses the same reader.
pub struct NativeInputReader {
    source: Box<dyn EventSource>,
}
impl NativeInputReader {
    #[cfg(unix)]
    pub fn new(storage: Arc<dyn NativeStorage>) -> io::Result<Self> {
        Ok(Self {
            source: Box::new(super::source::unix::UnixInternalEventSource::new_owned(
                storage,
            )?),
        })
    }

    #[cfg(windows)]
    pub fn new(storage: Arc<dyn NativeStorage>) -> io::Result<Self> {
        Ok(Self {
            source: Box::new(super::source::windows::WindowsEventSource::new_owned(
                storage,
            )?),
        })
    }

    pub fn set_terminal_query_mode(&mut self, active: bool) {
        self.source.set_terminal_query_mode(active);
    }

    #[cfg(windows)]
    pub fn read(&mut self, timeout: Duration) -> Result<Option<NativeInputEvent>, NativeReadError> {
        let Some(event) = self.source.try_read(Some(timeout))? else {
            return Ok(None);
        };
        let storage = self.source.take_native_storage();
        let input = match event {
            InternalEvent::Event(event) => NativeInput::Event(event),
            InternalEvent::TerminalReply(reply) => NativeInput::Reply(reply),
        };
        Ok(Some(NativeInputEvent { input, storage }))
    }

    #[cfg(unix)]
    pub fn read(&mut self, timeout: Duration) -> Result<Option<NativeInputEvent>, NativeReadError> {
        let timeout = PollTimeout::new(Some(timeout));
        loop {
            let Some(event) = self.source.try_read(timeout.leftover())? else {
                return Ok(None);
            };
            let storage = self.source.take_native_storage();
            let input = match event {
                InternalEvent::Event(event) => NativeInput::Event(event),
                InternalEvent::TerminalReply(reply) => NativeInput::Reply(reply),
                // These reports are terminal metadata, not user input. Late
                // replies have no retaining side queue in this owned reader.
                #[cfg(unix)]
                _ => {
                    drop(storage);
                    if timeout.elapsed() {
                        return Ok(None);
                    }
                    continue;
                }
            };
            return Ok(Some(NativeInputEvent { input, storage }));
        }
    }

    /// Includes pending decoded original records and surrogate state, not just
    /// the borrowed diagnostic byte view. False means all source-owned input
    /// has been returned; unread OS bytes remain owned by the OS.
    pub fn has_retained_input(&self) -> bool {
        self.source.native_has_retained_input()
    }

    /// Borrow both parts of consumed-but-not-yet-delivered input without
    /// allocating a diagnostic copy. These slices are not independent events:
    /// an incomplete Paste remains one semantic operation.
    pub fn retained_original(&self) -> (&[u8], &[u8]) {
        self.source.native_retained_original()
    }
}

#[cfg(unix)]
pub(crate) struct NativeParser {
    buffer: Vec<u8>,
    storage: Option<Box<dyn NativeStorageLease>>,
    admission: Arc<dyn NativeStorage>,
    event: Option<InternalEvent>,
    event_storage: Option<Box<dyn NativeStorageLease>>,
    query_mode: bool,
    replay_offset: Option<usize>,
}

#[cfg(unix)]
impl NativeParser {
    pub(crate) fn new(admission: Arc<dyn NativeStorage>) -> Self {
        Self {
            buffer: Vec::new(),
            storage: None,
            admission,
            event: None,
            event_storage: None,
            query_mode: false,
            replay_offset: None,
        }
    }
    pub(crate) fn has_retained_input(&self) -> bool {
        self.event.is_some() || !self.buffer().is_empty()
    }
    pub(crate) fn buffer(&self) -> &[u8] {
        &self.buffer[self.replay_offset.unwrap_or(0)..]
    }
    pub(crate) fn set_terminal_query_mode(&mut self, active: bool) {
        if !active
            && self.query_mode
            && !self.buffer.is_empty()
            && super::terminal_query::reply_prefix(&self.buffer)
                == super::terminal_query::ReplyPrefix::Incomplete
        {
            self.replay_offset = Some(0);
        }
        self.query_mode = active;
    }
    pub(crate) fn next(&mut self) -> Option<InternalEvent> {
        if let Some(event) = self.event.take() {
            return Some(event);
        }
        let index = self.replay_offset?;
        if index == self.buffer.len() {
            self.replay_offset = None;
            self.buffer.clear();
            return None;
        }
        let byte = self.buffer[index];
        self.replay_offset = Some(index + 1);
        let code = if byte == 0x1b {
            super::KeyCode::Esc
        } else {
            super::KeyCode::Char(char::from(byte))
        };
        Some(InternalEvent::Event(Event::Key(super::KeyEvent::new(
            code,
            super::KeyModifiers::NONE,
        ))))
    }
    pub(crate) fn take_storage(&mut self) -> Option<Box<dyn NativeStorageLease>> {
        self.event_storage.take()
    }

    /// Admission failure leaves the parser exactly unchanged and leaves the
    /// caller responsible for the current byte. No speculative growth occurs.
    fn ensure_capacity(&mut self) -> io::Result<()> {
        if self.buffer.len() < self.buffer.capacity() {
            return Ok(());
        }
        let requested = self.buffer.capacity().saturating_mul(2).max(256);
        let lease = self
            .admission
            .reserve_replacement(requested, self.buffer.capacity())
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(requested)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        // Rust capacity is the allocation extent observable to this adapter.
        // Reject unexpected capacity without consuming another original byte.
        // The replacement allocation is dropped while its guard is still live;
        // the original parser allocation and reservation remain intact.
        if replacement.capacity() != requested {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "native allocator returned unexpected capacity",
            ));
        }
        replacement.extend_from_slice(&self.buffer);
        let old = std::mem::replace(&mut self.buffer, replacement);
        drop(old);
        self.storage = Some(lease);
        Ok(())
    }

    pub(crate) fn advance(&mut self, byte: u8, more: bool) -> io::Result<()> {
        debug_assert!(self.event.is_none() && self.replay_offset.is_none());
        // A complete Paste needs its potentially larger lossy output charged
        // before consuming its final delimiter byte. Valid UTF-8 transfers the
        // existing allocation instead of allocating another String.
        #[cfg(feature = "bracketed-paste")]
        if byte == b'~'
            && self.buffer.starts_with(b"\x1b[200~")
            && self.buffer.ends_with(b"\x1b[201")
        {
            return self.finish_paste();
        }
        self.ensure_capacity()?;
        self.buffer.push(byte);
        let previous_query = self.query_mode
            && self.buffer.len() > 1
            && super::terminal_query::reply_prefix(&self.buffer[..self.buffer.len() - 1])
                == super::terminal_query::ReplyPrefix::Incomplete;
        let prefix = super::terminal_query::reply_prefix(&self.buffer);
        let parsed = if self.query_mode {
            match prefix {
                super::terminal_query::ReplyPrefix::Complete => {
                    let bytes = std::mem::take(&mut self.buffer);
                    self.event_storage = self.storage.take();
                    Ok(Some(InternalEvent::TerminalReply(bytes)))
                }
                super::terminal_query::ReplyPrefix::Incomplete => Ok(None),
                super::terminal_query::ReplyPrefix::Other => {
                    super::sys::unix::parse::parse_event(&self.buffer, more || self.query_mode)
                }
            }
        } else {
            super::sys::unix::parse::parse_event(&self.buffer, more)
        };
        match parsed {
            Ok(Some(event)) => {
                self.event = Some(event);
                self.buffer.clear();
            }
            Ok(None)
                if previous_query
                    && prefix == super::terminal_query::ReplyPrefix::Other
                    && (0x40..=0x7e).contains(&byte) =>
            {
                self.replay_offset = Some(0)
            }
            Err(_) if previous_query => self.replay_offset = Some(0),
            Err(_) => self.buffer.clear(),
            Ok(None) => {}
        }
        Ok(())
    }

    #[cfg(feature = "bracketed-paste")]
    fn finish_paste(&mut self) -> io::Result<()> {
        let end = self.buffer.len() - 5;
        let body = &self.buffer[6..end];
        let valid = std::str::from_utf8(body).ok();
        if valid.is_some()
            && self
                .admission
                .paste_completion(body.len(), self.buffer.capacity())
                == NativePasteCompletion::KeepBacking
        {
            self.buffer.copy_within(6..end, 0);
            self.buffer.truncate(end - 6);
            let bytes = std::mem::take(&mut self.buffer);
            // The same bytes were validated above; no allocation or fallible
            // conversion is needed after ownership transfer.
            let text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => unreachable!("Paste bytes were validated before moving"),
            };
            self.event_storage = self.storage.take();
            self.event = Some(InternalEvent::Event(Event::Paste(text)));
            return Ok(());
        }
        let output_bytes = valid
            .map(str::len)
            .or_else(|| lossy_length(body))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Other,
                    NativeStorageRefusal {
                        kind: NativeRefusalKind::Limit,
                        requested: usize::MAX,
                    },
                )
            })?;
        let lease = if output_bytes == 0 {
            None // An empty compacted Paste needs no backing allocation.
        } else {
            Some(
                self.admission
                    .reserve_replacement(output_bytes, self.buffer.capacity())
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?,
            )
        };
        let mut text = String::new();
        text.try_reserve_exact(output_bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        if text.capacity() != output_bytes {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "native allocator returned unexpected capacity",
            ));
        }
        match valid {
            Some(valid) => text.push_str(valid),
            None => append_lossy(&mut text, body),
        }
        // Raw and output allocations coexist only while both reservations are
        // live. Release the original allocation before releasing its guard.
        drop(std::mem::take(&mut self.buffer));
        drop(self.storage.take());
        self.event_storage = lease;
        self.event = Some(InternalEvent::Event(Event::Paste(text)));
        Ok(())
    }
}

#[cfg(all(unix, feature = "bracketed-paste"))]
fn lossy_length(mut bytes: &[u8]) -> Option<usize> {
    let mut length = 0usize;
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => return length.checked_add(valid.len()),
            Err(error) => {
                length = length.checked_add(error.valid_up_to())?.checked_add(3)?;
                match error.error_len() {
                    Some(invalid) => bytes = &bytes[error.valid_up_to() + invalid..],
                    None => return Some(length),
                }
            }
        }
    }
}
#[cfg(all(unix, feature = "bracketed-paste"))]
fn append_lossy(output: &mut String, mut bytes: &[u8]) {
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                output.push_str(valid);
                return;
            }
            Err(error) => {
                if let Ok(valid) = std::str::from_utf8(&bytes[..error.valid_up_to()]) {
                    output.push_str(valid);
                }
                output.push('\u{fffd}');
                match error.error_len() {
                    Some(invalid) => bytes = &bytes[error.valid_up_to() + invalid..],
                    None => return,
                }
            }
        }
    }
}

#[cfg(all(test, unix, feature = "bracketed-paste"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Accounting {
        used: Arc<AtomicUsize>,
        limit: AtomicUsize,
        peak: AtomicUsize,
        calls: AtomicUsize,
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
    impl NativeStorage for Accounting {
        fn reserve(
            &self,
            bytes: usize,
        ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let used = self.used.load(Ordering::SeqCst);
            if bytes > self.limit.load(Ordering::SeqCst).saturating_sub(used) {
                return Err(NativeStorageRefusal {
                    kind: NativeRefusalKind::Busy,
                    requested: bytes,
                });
            }
            let now = self.used.fetch_add(bytes, Ordering::SeqCst) + bytes;
            self.peak.fetch_max(now, Ordering::SeqCst);
            Ok(Box::new(Lease {
                used: self.used.clone(),
                bytes,
            }))
        }
    }
    fn accounting(limit: usize) -> Arc<Accounting> {
        Arc::new(Accounting {
            used: Arc::new(AtomicUsize::new(0)),
            limit: AtomicUsize::new(limit),
            peak: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        })
    }
    fn advance(parser: &mut NativeParser, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            parser.advance(*byte, index + 1 < bytes.len()).unwrap();
        }
    }

    #[test]
    fn growth_refusal_preserves_original_prefix_before_consuming_current_byte() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(budget.clone());
        advance(&mut parser, b"\x1b[200~");
        advance(&mut parser, &[b'a'; 250]);
        let pointer = parser.buffer.as_ptr();
        let original = parser.buffer.clone();
        let error = NativeReadError::from(parser.advance(b'z', true).unwrap_err());
        assert!(matches!(
            error,
            NativeReadError::Admission(NativeStorageRefusal { requested: 512, .. })
        ));
        assert_eq!(parser.buffer, original);
        assert_eq!(parser.buffer.as_ptr(), pointer);
        assert_eq!(budget.used.load(Ordering::SeqCst), 256);
        budget.limit.store(768, Ordering::SeqCst);
        parser.advance(b'z', true).unwrap();
        assert_eq!(parser.buffer.last(), Some(&b'z'));
        assert_eq!(budget.peak.load(Ordering::SeqCst), 768);
        assert_eq!(budget.used.load(Ordering::SeqCst), 512);
        drop(parser);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn valid_paste_transfers_original_allocation_and_guard_without_copy() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(budget.clone());
        advance(&mut parser, b"\x1b[200~hello \xf0\x9f\x8c\x8d\x1b[201");
        let pointer = parser.buffer.as_ptr();
        parser.advance(b'~', false).unwrap();
        let Some(InternalEvent::Event(Event::Paste(text))) = parser.next() else {
            panic!("expected Paste")
        };
        assert_eq!(text, "hello 🌍");
        assert_eq!(text.as_ptr(), pointer);
        let guard = parser.take_storage();
        drop(parser);
        assert_eq!(budget.used.load(Ordering::SeqCst), text.capacity());
        assert_eq!(budget.calls.load(Ordering::SeqCst), 1);
        drop(text);
        drop(guard);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lossy_output_refusal_preserves_raw_body_and_final_delimiter_for_retry() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(budget.clone());
        advance(&mut parser, b"\x1b[200~a\xff\xf0\x9f\x1b[201");
        let original = parser.buffer.clone();
        let error = NativeReadError::from(parser.advance(b'~', false).unwrap_err());
        assert!(matches!(
            error,
            NativeReadError::Admission(NativeStorageRefusal { requested: 7, .. })
        ));
        assert_eq!(parser.buffer, original);
        assert!(parser.next().is_none());
        budget.limit.store(263, Ordering::SeqCst);
        parser.advance(b'~', false).unwrap();
        let Some(InternalEvent::Event(Event::Paste(text))) = parser.next() else {
            panic!("expected Paste")
        };
        assert_eq!(text, String::from_utf8_lossy(b"a\xff\xf0\x9f"));
        assert_eq!(text.capacity(), 7);
        let guard = parser.take_storage();
        assert_eq!(budget.used.load(Ordering::SeqCst), 7);
        assert_eq!(budget.peak.load(Ordering::SeqCst), 263);
        drop(parser);
        drop(text);
        drop(guard);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn every_byte_lossy_length_and_output_match_standard_conversion() {
        for byte in 0..=u8::MAX {
            for bytes in [
                vec![byte],
                vec![b'a', byte, 0xf0, 0x9f],
                vec![0xe0, byte, 0x80],
            ] {
                let expected = String::from_utf8_lossy(&bytes);
                assert_eq!(lossy_length(&bytes), Some(expected.len()));
                let mut actual = String::with_capacity(expected.len());
                append_lossy(&mut actual, &bytes);
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn partial_query_replay_keeps_original_lease_until_parser_release() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(budget.clone());
        parser.set_terminal_query_mode(true);
        advance(&mut parser, b"\x1b[?17");
        parser.set_terminal_query_mode(false);
        for expected in [
            super::super::KeyCode::Esc,
            super::super::KeyCode::Char('['),
            super::super::KeyCode::Char('?'),
            super::super::KeyCode::Char('1'),
            super::super::KeyCode::Char('7'),
        ] {
            assert!(
                matches!(parser.next(), Some(InternalEvent::Event(Event::Key(key))) if key.code == expected)
            );
        }
        assert!(parser.next().is_none());
        assert_eq!(budget.used.load(Ordering::SeqCst), 256);
        advance(&mut parser, b"x");
        assert!(
            matches!(parser.next(), Some(InternalEvent::Event(Event::Key(key))) if key.code == super::super::KeyCode::Char('x'))
        );
        drop(parser);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn completed_query_reply_transfers_actual_buffer_lease() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(budget.clone());
        parser.set_terminal_query_mode(true);
        advance(&mut parser, b"\x1b[?1u");
        let Some(InternalEvent::TerminalReply(reply)) = parser.next() else {
            panic!("expected reply")
        };
        assert_eq!(reply, b"\x1b[?1u");
        let guard = parser.take_storage();
        drop(parser);
        assert_eq!(budget.used.load(Ordering::SeqCst), reply.capacity());
        drop(reply);
        drop(guard);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn incomplete_paste_survives_moving_owner_and_releases_on_actual_drop() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(budget.clone());
        advance(&mut parser, b"\x1b[200~original incomplete");
        let returned = std::thread::spawn(move || parser).join().unwrap();
        assert_eq!(returned.buffer(), b"\x1b[200~original incomplete");
        assert_eq!(budget.used.load(Ordering::SeqCst), 256);
        drop(returned);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }
    struct CompactingAccounting {
        inner: Arc<Accounting>,
        refusal: Option<NativeRefusalKind>,
    }
    impl NativeStorage for CompactingAccounting {
        fn paste_completion(&self, _: usize, _: usize) -> NativePasteCompletion {
            NativePasteCompletion::Compact
        }
        fn reserve(
            &self,
            bytes: usize,
        ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
            if bytes == 5 {
                if let Some(kind) = self.refusal {
                    return Err(NativeStorageRefusal {
                        kind,
                        requested: bytes,
                    });
                }
            }
            self.inner.reserve(bytes)
        }
    }

    #[test]
    fn completion_compaction_busy_retains_prefix_final_byte_and_old_allocation_for_retry() {
        let budget = accounting(256);
        let policy = Arc::new(CompactingAccounting {
            inner: budget.clone(),
            refusal: None,
        });
        let mut parser = NativeParser::new(policy);
        advance(&mut parser, b"\x1b[200~hello\x1b[201");
        let pointer = parser.buffer.as_ptr();
        let original = parser.buffer.clone();
        let error = NativeReadError::from(parser.advance(b'~', false).unwrap_err());
        assert!(matches!(
            error,
            NativeReadError::Admission(NativeStorageRefusal {
                kind: NativeRefusalKind::Busy,
                requested: 5
            })
        ));
        assert_eq!(parser.buffer, original);
        assert_eq!(parser.buffer.as_ptr(), pointer);
        assert!(parser.next().is_none());
        assert_eq!(budget.used.load(Ordering::SeqCst), 256);
        budget.limit.store(261, Ordering::SeqCst);
        parser.advance(b'~', false).unwrap();
        let Some(InternalEvent::Event(Event::Paste(text))) = parser.next() else {
            panic!("expected one original Paste")
        };
        assert_eq!(text, "hello");
        assert_eq!(text.capacity(), 5);
        assert_ne!(text.as_ptr(), pointer);
        assert_eq!(budget.peak.load(Ordering::SeqCst), 261);
        assert_eq!(budget.used.load(Ordering::SeqCst), 5);
        let lease = parser.take_storage();
        assert!(parser.take_storage().is_none());
        drop(parser);
        assert_eq!(budget.used.load(Ordering::SeqCst), 5);
        drop(text);
        drop(lease);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn completion_compaction_hard_and_closed_refusals_keep_entire_original() {
        for kind in [NativeRefusalKind::Limit, NativeRefusalKind::Closed] {
            let budget = accounting(261);
            let mut parser = NativeParser::new(Arc::new(CompactingAccounting {
                inner: budget.clone(),
                refusal: Some(kind),
            }));
            advance(&mut parser, b"\x1b[200~hello\x1b[201");
            let original = parser.buffer.clone();
            let error = NativeReadError::from(parser.advance(b'~', false).unwrap_err());
            assert!(
                matches!(error, NativeReadError::Admission(refusal) if refusal.kind == kind && refusal.requested == 5)
            );
            assert_eq!(parser.buffer, original);
            assert!(parser.has_retained_input());
            assert_eq!(budget.used.load(Ordering::SeqCst), 256);
            drop(parser);
            assert_eq!(budget.used.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn empty_completion_compaction_has_no_new_allocation_or_zero_byte_reservation() {
        let budget = accounting(256);
        let mut parser = NativeParser::new(Arc::new(CompactingAccounting {
            inner: budget.clone(),
            refusal: None,
        }));
        advance(&mut parser, b"\x1b[200~\x1b[201~");
        let Some(InternalEvent::Event(Event::Paste(text))) = parser.next() else {
            panic!("expected empty Paste")
        };
        assert_eq!(text, "");
        assert_eq!(text.capacity(), 0);
        assert!(parser.take_storage().is_none());
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
        assert_eq!(budget.calls.load(Ordering::SeqCst), 1);
    }
}
