use std::{collections::VecDeque, io, time::Duration};

use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use signal_hook_mio::v1_0::Signals;

#[cfg(feature = "event-stream")]
use crate::event::sys::Waker;
use crate::event::{
    source::EventSource, sys::unix::parse::parse_event, timeout::PollTimeout, Event, InternalEvent,
};
use crate::terminal::sys::file_descriptor::{tty_fd, FileDesc};

// Tokens to identify file descriptor
const TTY_TOKEN: Token = Token(0);
const SIGNAL_TOKEN: Token = Token(1);
#[cfg(feature = "event-stream")]
const WAKE_TOKEN: Token = Token(2);

// I (@zrzka) wasn't able to read more than 1_022 bytes when testing
// reading on macOS/Linux -> we don't need bigger buffer and 1k of bytes
// is enough.
const TTY_BUFFER_SIZE: usize = 1_024;

pub(crate) struct UnixInternalEventSource {
    poll: Poll,
    events: Events,
    parser: Parser,
    native_parser: Option<crate::event::native::NativeParser>,
    pending_start: usize,
    pending_end: usize,
    tty_buffer: [u8; TTY_BUFFER_SIZE],
    tty_fd: FileDesc<'static>,
    signals: Signals,
    #[cfg(feature = "event-stream")]
    waker: Waker,
}

impl UnixInternalEventSource {
    pub fn new() -> io::Result<Self> {
        UnixInternalEventSource::from_file_descriptor(tty_fd()?)
    }

    pub(crate) fn new_owned(
        storage: std::sync::Arc<dyn crate::event::native::NativeStorage>,
    ) -> io::Result<Self> {
        Self::from_descriptor_with_storage(tty_fd()?, Some(storage))
    }

    pub(crate) fn from_file_descriptor(input_fd: FileDesc<'static>) -> io::Result<Self> {
        Self::from_descriptor_with_storage(input_fd, None)
    }

    fn from_descriptor_with_storage(
        input_fd: FileDesc<'static>,
        storage: Option<std::sync::Arc<dyn crate::event::native::NativeStorage>>,
    ) -> io::Result<Self> {
        let poll = Poll::new()?;
        let registry = poll.registry();

        let tty_raw_fd = input_fd.raw_fd();
        let mut tty_ev = SourceFd(&tty_raw_fd);
        registry.register(&mut tty_ev, TTY_TOKEN, Interest::READABLE)?;

        let mut signals = Signals::new([signal_hook::consts::SIGWINCH])?;
        registry.register(&mut signals, SIGNAL_TOKEN, Interest::READABLE)?;

        #[cfg(feature = "event-stream")]
        let waker = Waker::new(registry, WAKE_TOKEN)?;

        Ok(UnixInternalEventSource {
            poll,
            events: Events::with_capacity(3),
            parser: if storage.is_some() {
                Parser::empty()
            } else {
                Parser::default()
            },
            native_parser: storage.map(crate::event::native::NativeParser::new),
            pending_start: 0,
            pending_end: 0,
            tty_buffer: [0u8; TTY_BUFFER_SIZE],
            tty_fd: input_fd,
            signals,
            #[cfg(feature = "event-stream")]
            waker,
        })
    }
}

impl UnixInternalEventSource {
    fn native_pending_event(&mut self) -> io::Result<Option<InternalEvent>> {
        let Some(parser) = self.native_parser.as_mut() else {
            return Ok(None);
        };
        if let Some(event) = parser.next() {
            return Ok(Some(event));
        }
        while self.pending_start < self.pending_end {
            let index = self.pending_start;
            let more = index + 1 < self.pending_end || self.pending_end == TTY_BUFFER_SIZE;
            // Advance acknowledges exactly this byte only after all required
            // reservations succeed. A refusal retains the fixed chunk tail.
            parser.advance(self.tty_buffer[index], more)?;
            self.pending_start += 1;
            if let Some(event) = parser.next() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
}

impl EventSource for UnixInternalEventSource {
    fn set_terminal_query_mode(&mut self, active: bool) {
        if let Some(parser) = self.native_parser.as_mut() {
            parser.set_terminal_query_mode(active);
        } else {
            self.parser.set_terminal_query_mode(active);
        }
    }

    fn take_native_storage(&mut self) -> Option<Box<dyn crate::event::native::NativeStorageLease>> {
        self.native_parser
            .as_mut()
            .and_then(crate::event::native::NativeParser::take_storage)
    }

    fn native_has_retained_input(&self) -> bool {
        self.pending_start < self.pending_end
            || self
                .native_parser
                .as_ref()
                .map_or(true, crate::event::native::NativeParser::has_retained_input)
    }

    fn native_retained_original(&self) -> (&[u8], &[u8]) {
        match self.native_parser.as_ref() {
            Some(parser) => (
                parser.buffer(),
                &self.tty_buffer[self.pending_start..self.pending_end],
            ),
            None => (&[], &[]),
        }
    }

    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>> {
        if let Some(event) = self.native_pending_event()? {
            return Ok(Some(event));
        }

        if let Some(event) = self.parser.next() {
            return Ok(Some(event));
        }

        let timeout = PollTimeout::new(timeout);

        loop {
            if let Err(e) = self.poll.poll(&mut self.events, timeout.leftover()) {
                // Mio will throw an interrupted error in case of cursor position retrieval. We need to retry until it succeeds.
                // Previous versions of Mio (< 0.7) would automatically retry the poll call if it was interrupted (if EINTR was returned).
                // https://docs.rs/mio/0.7.0/mio/struct.Poll.html#notes
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                } else {
                    return Err(e);
                }
            };

            if self.events.is_empty() {
                // No readiness events = timeout
                return Ok(None);
            }

            // Copy one scalar token before mutating the owned parser/source.
            for event_index in 0..self.events.iter().count() {
                let Some(token) = self
                    .events
                    .iter()
                    .nth(event_index)
                    .map(|event| event.token())
                else {
                    continue;
                };
                match token {
                    TTY_TOKEN => {
                        loop {
                            match self.tty_fd.read(&mut self.tty_buffer) {
                                Ok(read_count) => {
                                    if read_count == 0 && self.native_parser.is_some() {
                                        return Err(io::Error::new(
                                            io::ErrorKind::UnexpectedEof,
                                            "native input reached end of stream",
                                        ));
                                    }
                                    if read_count > 0 {
                                        if self.native_parser.is_some() {
                                            self.pending_start = 0;
                                            self.pending_end = read_count;
                                            if let Some(event) = self.native_pending_event()? {
                                                return Ok(Some(event));
                                            }
                                            if timeout.elapsed() {
                                                return Ok(None);
                                            }
                                            // An incomplete Paste must poll again before
                                            // another possibly blocking native read. Rearm
                                            // readiness so already-readable chunk tails
                                            // are not stranded by edge-triggered polling.
                                            let raw_fd = self.tty_fd.raw_fd();
                                            let mut tty = SourceFd(&raw_fd);
                                            self.poll.registry().reregister(
                                                &mut tty,
                                                TTY_TOKEN,
                                                Interest::READABLE,
                                            )?;
                                            break;
                                        } else {
                                            self.parser.advance(
                                                &self.tty_buffer[..read_count],
                                                read_count == TTY_BUFFER_SIZE,
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    // No more data to read at the moment. We will receive another event
                                    if e.kind() == io::ErrorKind::WouldBlock {
                                        break;
                                    }
                                    // once more data is available to read.
                                    else if e.kind() == io::ErrorKind::Interrupted {
                                        continue;
                                    } else if self.native_parser.is_some() {
                                        return Err(e);
                                    }
                                }
                            };

                            if let Some(event) = self.parser.next() {
                                return Ok(Some(event));
                            }
                        }
                    }
                    SIGNAL_TOKEN => {
                        if self.signals.pending().next() == Some(signal_hook::consts::SIGWINCH) {
                            // TODO Should we remove tput?
                            //
                            // This can take a really long time, because terminal::size can
                            // launch new process (tput) and then it parses its output. It's
                            // not a really long time from the absolute time point of view, but
                            // it's a really long time from the mio, async-std/tokio executor, ...
                            // point of view.
                            let new_size = crate::terminal::size()?;
                            return Ok(Some(InternalEvent::Event(Event::Resize(
                                new_size.0, new_size.1,
                            ))));
                        }
                    }
                    #[cfg(feature = "event-stream")]
                    WAKE_TOKEN => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Poll operation was woken up by `Waker::wake`",
                        ));
                    }
                    _ => unreachable!("Synchronize Evented handle registration & token handling"),
                }
            }

            // Processing above can take some time, check if timeout expired
            if timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker {
        self.waker.clone()
    }
}

//
// Following `Parser` structure exists for two reasons:
//
//  * mimic anes Parser interface
//  * move the advancing, parsing, ... stuff out of the `try_read` method
//
#[derive(Debug)]
struct Parser {
    buffer: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
    terminal_query_mode: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            // This buffer is used for -> 1 <- ANSI escape sequence. Are we
            // aware of any ANSI escape sequence that is bigger? Can we make
            // it smaller?
            //
            // Probably not worth spending more time on this as "there's a plan"
            // to use the anes crate parser.
            buffer: Vec::with_capacity(256),
            // TTY_BUFFER_SIZE is 1_024 bytes. How many ANSI escape sequences can
            // fit? What is an average sequence length? Let's guess here
            // and say that the average ANSI escape sequence length is 8 bytes. Thus
            // the buffer size should be 1024/8=128 to avoid additional allocations
            // when processing large amounts of data.
            //
            // There's no need to make it bigger, because when you look at the `try_read`
            // method implementation, all events are consumed before the next TTY_BUFFER
            // is processed -> events pushed.
            internal_events: VecDeque::with_capacity(128),
            terminal_query_mode: false,
        }
    }
}

impl Parser {
    fn empty() -> Self {
        Self {
            buffer: Vec::new(),
            internal_events: VecDeque::new(),
            terminal_query_mode: false,
        }
    }

    fn set_terminal_query_mode(&mut self, active: bool) {
        if !active
            && self.terminal_query_mode
            && crate::event::terminal_query::reply_prefix(&self.buffer)
                == crate::event::terminal_query::ReplyPrefix::Incomplete
        {
            self.internal_events
                .extend(crate::event::terminal_query::literal_prefix_events(
                    &self.buffer,
                ));
            self.buffer.clear();
        }
        self.terminal_query_mode = active;
    }

    fn advance(&mut self, buffer: &[u8], more: bool) {
        for (idx, byte) in buffer.iter().enumerate() {
            let more = idx + 1 < buffer.len() || more || self.terminal_query_mode;

            self.buffer.push(*byte);
            let was_query_prefix = self.terminal_query_mode
                && self.buffer.len() > 1
                && crate::event::terminal_query::reply_prefix(
                    &self.buffer[..self.buffer.len() - 1],
                ) == crate::event::terminal_query::ReplyPrefix::Incomplete;
            let no_longer_query = crate::event::terminal_query::reply_prefix(&self.buffer)
                == crate::event::terminal_query::ReplyPrefix::Other;

            let parsed = if self.terminal_query_mode {
                match crate::event::terminal_query::reply_prefix(&self.buffer) {
                    crate::event::terminal_query::ReplyPrefix::Complete => {
                        Ok(Some(InternalEvent::TerminalReply(self.buffer.clone())))
                    }
                    crate::event::terminal_query::ReplyPrefix::Incomplete => Ok(None),
                    crate::event::terminal_query::ReplyPrefix::Other => {
                        parse_event(&self.buffer, more)
                    }
                }
            } else {
                parse_event(&self.buffer, more)
            };
            match parsed {
                Ok(Some(ie)) => {
                    self.internal_events.push_back(ie);
                    self.buffer.clear();
                }
                Ok(None) => {
                    // A partial reply followed by an invalid final byte must
                    // release that byte as original input.
                    if was_query_prefix && no_longer_query && (0x40..=0x7e).contains(byte) {
                        self.internal_events.extend(
                            crate::event::terminal_query::literal_prefix_events(&self.buffer),
                        );
                        self.buffer.clear();
                    }
                }
                Err(_) => {
                    if was_query_prefix {
                        self.internal_events.extend(
                            crate::event::terminal_query::literal_prefix_events(&self.buffer),
                        );
                    }
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_events.pop_front()
    }
}

#[cfg(test)]
mod owned_terminal_query_tests {
    use super::*;
    use crate::event::KeyCode;

    #[cfg(feature = "bracketed-paste")]
    #[test]
    fn real_parser_keeps_early_key_and_paste_around_complete_replies() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.advance(
            b"n\x1b_Gi=31;OK\x1b\\\x1b[?64;4c\x1b[6;7;14t\x1b[0n\x1b[200~first paste\x1b[201~",
            false,
        );
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(key))) if key.code == KeyCode::Char('n')
        ));
        for expected in [
            b"\x1b_Gi=31;OK\x1b\\".as_slice(),
            b"\x1b[?64;4c",
            b"\x1b[6;7;14t",
            b"\x1b[0n",
        ] {
            assert!(matches!(
                parser.next(),
                Some(InternalEvent::TerminalReply(reply)) if reply == expected
            ));
        }
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::Event(Event::Paste(text))) if text == "first paste"
        ));
        assert!(parser.next().is_none());
    }

    #[cfg(feature = "bracketed-paste")]
    #[test]
    fn keyboard_query_preserves_first_key_and_paste_before_flags_and_device_reply() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.advance(b"n\x1b[200~early paste\x1b[201~\x1b[?1u\x1b[?64;4c", false);
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(key))) if key.code == KeyCode::Char('n')
        ));
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::Event(Event::Paste(text))) if text == "early paste"
        ));
        for expected in [b"\x1b[?1u".as_slice(), b"\x1b[?64;4c"] {
            assert!(matches!(
                parser.next(),
                Some(InternalEvent::TerminalReply(reply)) if reply == expected
            ));
        }
        assert!(parser.next().is_none());
    }

    #[test]
    fn incomplete_keyboard_reply_replays_exact_prefix_after_timeout() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.advance(b"\x1b[?17", false);
        assert!(parser.next().is_none());
        parser.set_terminal_query_mode(false);
        for code in [
            KeyCode::Esc,
            KeyCode::Char('['),
            KeyCode::Char('?'),
            KeyCode::Char('1'),
            KeyCode::Char('7'),
        ] {
            assert!(matches!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(key))) if key.code == code
            ));
        }
        assert!(parser.next().is_none());
    }

    #[test]
    fn late_keyboard_reply_after_query_timeout_is_not_an_ordinary_key() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.set_terminal_query_mode(false);
        parser.advance(b"\x1b[?1u", false);
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::KeyboardEnhancementFlags(_))
        ));
        assert!(parser.next().is_none());
    }

    #[test]
    fn invalid_keyboard_reply_replays_exact_input() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.advance(b"\x1b[?1h", false);
        for code in [
            KeyCode::Esc,
            KeyCode::Char('['),
            KeyCode::Char('?'),
            KeyCode::Char('1'),
            KeyCode::Char('h'),
        ] {
            assert!(matches!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(key))) if key.code == code
            ));
        }
        assert!(parser.next().is_none());
    }

    #[test]
    fn invalid_partial_reply_releases_first_following_key() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.advance(b"\x1b[6;7;n", false);
        for code in [
            KeyCode::Esc,
            KeyCode::Char('['),
            KeyCode::Char('6'),
            KeyCode::Char(';'),
            KeyCode::Char('7'),
            KeyCode::Char(';'),
            KeyCode::Char('n'),
        ] {
            assert!(matches!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(key))) if key.code == code
            ));
        }
        assert!(parser.next().is_none());
    }

    #[test]
    fn partial_reply_timeout_releases_prefix_before_late_user_key() {
        let mut parser = Parser::default();
        parser.set_terminal_query_mode(true);
        parser.advance(b"\x1b[6;7;", false);
        assert!(parser.next().is_none());
        parser.set_terminal_query_mode(false);
        for code in [
            KeyCode::Esc,
            KeyCode::Char('['),
            KeyCode::Char('6'),
            KeyCode::Char(';'),
            KeyCode::Char('7'),
            KeyCode::Char(';'),
        ] {
            assert!(matches!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(key))) if key.code == code
            ));
        }
        parser.advance(b"n", false);
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(key))) if key.code == KeyCode::Char('n')
        ));
    }
}

#[cfg(all(test, feature = "bracketed-paste"))]
mod native_storage_source_tests {
    use super::*;
    use crate::event::native::{
        NativeRefusalKind, NativeStorage, NativeStorageLease, NativeStorageRefusal,
    };
    use std::{
        io::Write,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
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

    #[test]
    fn real_source_retains_refused_byte_and_entire_fixed_chunk_then_retries_in_order() {
        let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        #[cfg(feature = "libc")]
        let descriptor = {
            use std::os::fd::IntoRawFd;
            FileDesc::new(reader.into_raw_fd(), true)
        };
        #[cfg(not(feature = "libc"))]
        let descriptor = FileDesc::Owned(reader.into());
        let budget = Arc::new(Budget {
            used: Arc::new(AtomicUsize::new(0)),
            limit: AtomicUsize::new(256),
        });
        let mut source =
            UnixInternalEventSource::from_descriptor_with_storage(descriptor, Some(budget.clone()))
                .unwrap();
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend_from_slice(&[b'a'; 250]);
        bytes.extend_from_slice(b"z\x1b[201~x");
        writer.write_all(&bytes).unwrap();
        let error = source
            .try_read(Some(Duration::from_millis(100)))
            .unwrap_err();
        assert!(error.get_ref().unwrap().is::<NativeStorageRefusal>());
        let (prefix, unread) = source.native_retained_original();
        assert_eq!(prefix, &bytes[..256]);
        assert_eq!(unread, &bytes[256..]);
        assert_eq!(budget.used.load(Ordering::SeqCst), 256);
        budget.limit.store(768, Ordering::SeqCst);
        let Some(InternalEvent::Event(Event::Paste(text))) =
            source.try_read(Some(Duration::from_millis(100))).unwrap()
        else {
            panic!("expected retained Paste")
        };
        assert_eq!(text, format!("{}z", "a".repeat(250)));
        let guard = source.take_native_storage();
        assert!(
            matches!(source.try_read(Some(Duration::from_millis(100))).unwrap(), Some(InternalEvent::Event(Event::Key(key))) if key.code == crate::event::KeyCode::Char('x'))
        );
        drop(source);
        assert_eq!(budget.used.load(Ordering::SeqCst), text.capacity());
        drop(text);
        drop(guard);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }
    fn blocking_source(
        limit: usize,
    ) -> (
        UnixInternalEventSource,
        std::os::unix::net::UnixStream,
        Arc<Budget>,
    ) {
        let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
        #[cfg(feature = "libc")]
        let descriptor = {
            use std::os::unix::io::IntoRawFd;
            FileDesc::new(reader.into_raw_fd(), true)
        };
        #[cfg(not(feature = "libc"))]
        let descriptor = FileDesc::Owned(reader.into());
        let budget = Arc::new(Budget {
            used: Arc::new(AtomicUsize::new(0)),
            limit: AtomicUsize::new(limit),
        });
        let source =
            UnixInternalEventSource::from_descriptor_with_storage(descriptor, Some(budget.clone()))
                .unwrap();
        (source, writer, budget)
    }

    #[test]
    fn incomplete_paste_repolls_blocking_descriptor_and_returns_on_original_deadline() {
        let (mut source, mut writer, budget) = blocking_source(256);
        writer.write_all(b"\x1b[200~partial original").unwrap();
        assert!(source
            .try_read(Some(Duration::from_millis(20)))
            .unwrap()
            .is_none());
        assert_eq!(
            source.native_retained_original().0,
            b"\x1b[200~partial original"
        );
        assert_eq!(budget.used.load(Ordering::SeqCst), 256);
        drop(source);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
        drop(writer);
    }

    #[test]
    fn already_readable_multichunk_paste_is_not_stranded_by_edge_triggered_poll() {
        let (mut source, mut writer, budget) = blocking_source(32768);
        let mut original = b"\x1b[200~".to_vec();
        original.extend_from_slice(&[b'a'; 5000]);
        original.extend_from_slice(b"\x1b[201~x");
        writer.write_all(&original).unwrap();
        let Some(InternalEvent::Event(Event::Paste(text))) =
            source.try_read(Some(Duration::from_millis(100))).unwrap()
        else {
            panic!("expected complete multichunk Paste")
        };
        assert_eq!(text, "a".repeat(5000));
        let guard = source.take_native_storage();
        assert!(
            matches!(source.try_read(Some(Duration::from_millis(100))).unwrap(), Some(InternalEvent::Event(Event::Key(key))) if key.code == crate::event::KeyCode::Char('x'))
        );
        drop(source);
        assert_eq!(budget.used.load(Ordering::SeqCst), text.capacity());
        drop(text);
        drop(guard);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn retained_predicate_covers_fixed_tail_partial_paste_and_actual_drain() {
        let (mut source, mut writer, _) = blocking_source(32768);
        assert!(!source.native_has_retained_input());
        writer.write_all(b"\x1b[200~partial").unwrap();
        assert!(source
            .try_read(Some(Duration::from_millis(20)))
            .unwrap()
            .is_none());
        assert!(source.native_has_retained_input());
        writer.write_all(b"\x1b[201~x").unwrap();
        let event = source.try_read(Some(Duration::from_millis(100))).unwrap();
        assert!(matches!(event, Some(InternalEvent::Event(Event::Paste(_)))));
        let lease = source.take_native_storage();
        assert!(source.native_has_retained_input());
        let event = source.try_read(Some(Duration::from_millis(100))).unwrap();
        assert!(matches!(event, Some(InternalEvent::Event(Event::Key(_)))));
        assert!(!source.native_has_retained_input());
        drop(lease);
    }
    struct CompactingBudget(Arc<Budget>);
    impl NativeStorage for CompactingBudget {
        fn paste_completion(
            &self,
            _: usize,
            _: usize,
        ) -> crate::event::native::NativePasteCompletion {
            crate::event::native::NativePasteCompletion::Compact
        }
        fn reserve(
            &self,
            bytes: usize,
        ) -> Result<Box<dyn NativeStorageLease>, NativeStorageRefusal> {
            self.0.reserve(bytes)
        }
    }
    #[test]
    fn real_source_completion_compaction_refusal_retains_final_delimiter_and_trailing_key() {
        let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        #[cfg(feature = "libc")]
        let descriptor = {
            use std::os::fd::IntoRawFd;
            FileDesc::new(reader.into_raw_fd(), true)
        };
        #[cfg(not(feature = "libc"))]
        let descriptor = FileDesc::Owned(reader.into());
        let budget = Arc::new(Budget {
            used: Arc::new(AtomicUsize::new(0)),
            limit: AtomicUsize::new(256),
        });
        let mut source = UnixInternalEventSource::from_descriptor_with_storage(
            descriptor,
            Some(Arc::new(CompactingBudget(budget.clone()))),
        )
        .unwrap();
        let bytes = b"\x1b[200~hello\x1b[201~x";
        writer.write_all(bytes).unwrap();
        let error = source
            .try_read(Some(Duration::from_millis(100)))
            .unwrap_err();
        assert!(error.get_ref().unwrap().is::<NativeStorageRefusal>());
        let (prefix, tail) = source.native_retained_original();
        assert_eq!(prefix, &bytes[..bytes.len() - 2]);
        assert_eq!(tail, b"~x");
        assert!(source.native_has_retained_input());
        budget.limit.store(261, Ordering::SeqCst);
        let Some(InternalEvent::Event(Event::Paste(text))) =
            source.try_read(Some(Duration::from_millis(100))).unwrap()
        else {
            panic!("expected retained Paste")
        };
        assert_eq!(text, "hello");
        assert_eq!(text.capacity(), 5);
        let lease = source.take_native_storage();
        assert!(source.native_has_retained_input());
        assert!(
            matches!(source.try_read(Some(Duration::from_millis(100))).unwrap(), Some(InternalEvent::Event(Event::Key(key))) if key.code == crate::event::KeyCode::Char('x'))
        );
        assert!(!source.native_has_retained_input());
        drop(source);
        assert_eq!(budget.used.load(Ordering::SeqCst), 5);
        drop(text);
        drop(lease);
        assert_eq!(budget.used.load(Ordering::SeqCst), 0);
    }
}
