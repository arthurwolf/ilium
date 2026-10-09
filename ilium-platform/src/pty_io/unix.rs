use super::*;
use crate::interruptible_reader::{InterruptibleRead, InterruptibleReader, ReaderInterrupt};
use crate::owned_worker::{WorkerKind, WorkerReservation};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::time::Duration;

const WRITE_POLL: Duration = Duration::from_millis(10);

pub(super) struct Reader {
    reader: InterruptibleReader,
    interrupt: ReaderInterrupt,
}

impl Reader {
    pub(super) fn spawn_reserved<C: Send + 'static>(
        mut self,
        stop: StopToken,
        mut sink: impl FnMut(ReadMessage, &StopToken) -> bool + Send + 'static,
        reservation: WorkerReservation<C>,
    ) -> io::Result<OwnedWorker> {
        let interrupt = self.interrupt.clone();
        reservation.spawn(
            "ilium-pty-read",
            WorkerKind::Cooperative,
            stop,
            move || interrupt.interrupt(),
            move |stop| {
                let mut buffer = [0; OUTPUT_CHUNK_BYTES];
                while !stop.is_stopped() {
                    let message = match self.reader.read(&mut buffer) {
                        Ok(InterruptibleRead::Data(0)) => continue,
                        Ok(InterruptibleRead::Data(n)) => {
                            ReadMessage::Data(Arc::from(&buffer[..n]))
                        }
                        Ok(InterruptibleRead::Eof) => ReadMessage::Eof,
                        Ok(InterruptibleRead::Interrupted) => {
                            if stop.is_stopped() {
                                break;
                            }
                            continue;
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                            ) =>
                        {
                            continue;
                        }
                        Err(error) => ReadMessage::Error(error.into()),
                    };
                    let terminal = !matches!(message, ReadMessage::Data(_));
                    if stop.is_stopped() || !sink(message, &stop) || terminal {
                        break;
                    }
                }
            },
        )
    }
}

#[derive(Clone)]
pub(super) struct ShellProbe(Arc<File>);
impl ShellProbe {
    pub(super) fn shell_owns_terminal(&self, process_id: u32) -> Option<bool> {
        // SAFETY: this independently owned descriptor remains live for the call.
        let group = unsafe { libc::tcgetpgrp(self.0.as_raw_fd()) };
        u32::try_from(group)
            .ok()
            .filter(|group| *group > 0)
            .map(|group| group == process_id)
    }
}

pub(super) fn open<C: Send + 'static>(
    master: &(dyn MasterPty + Send),
    _stop: StopToken,
    reservation: Option<WorkerReservation<C>>,
) -> io::Result<(Reader, Box<dyn PtyWriter>, ShellProbe)> {
    drop(reservation);
    let fd = master.as_raw_fd().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "PTY master exposes no Unix descriptor",
        )
    })?;
    let (reader, interrupt) = InterruptibleReader::duplicate(fd)?;
    // The duplicate belongs to the session, while cheap Arc clones let
    // observational callbacks retain it after a pane registry guard drops.
    let probe = ShellProbe(Arc::new(duplicate(fd)?));
    let writer = NonblockingWriter::duplicate(fd)?;
    Ok((Reader { reader, interrupt }, Box::new(writer), probe))
}

fn duplicate(fd: RawFd) -> io::Result<File> {
    loop {
        // SAFETY: fd is borrowed for fcntl. Success creates an owned CLOEXEC fd.
        let result = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if result >= 0 {
            // SAFETY: fcntl returned a fresh independently owned descriptor.
            return Ok(unsafe { File::from_raw_fd(result) });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

struct NonblockingWriter(File);

impl NonblockingWriter {
    fn duplicate(fd: RawFd) -> io::Result<Self> {
        let file = duplicate(fd)?;
        // O_NONBLOCK belongs to the OPEN FILE DESCRIPTION. Intentionally also
        // changes the master/reader. This adapter must be the ONLY writer, and
        // InterruptibleReader already handles a readiness race returning EAGAIN.
        let flags = loop {
            // SAFETY: file owns this live descriptor.
            let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
            if flags >= 0 {
                break flags;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        };
        loop {
            // SAFETY: file owns this descriptor and this session owns its mode.
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                >= 0
            {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        Ok(Self(file))
    }

    fn writable(&self, deadline: Instant, stop: &StopToken) -> Result<(), WriteFailureKind> {
        loop {
            if stop.is_stopped() {
                return Err(WriteFailureKind::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(WriteFailureKind::Timeout);
            }
            let milliseconds = remaining.min(WRITE_POLL).as_millis().max(1) as i32;
            let mut descriptor = libc::pollfd {
                fd: self.0.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            // SAFETY: one valid poll descriptor, live across the bounded call.
            let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(WriteFailureKind::Io(error.into()));
            }
            if result == 0 {
                continue;
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(WriteFailureKind::Io(
                    io::Error::new(io::ErrorKind::BrokenPipe, "PTY write descriptor closed").into(),
                ));
            }
            if descriptor.revents & libc::POLLOUT != 0 {
                return Ok(());
            }
        }
    }
}

impl PtyWriter for NonblockingWriter {
    fn write_until(
        &mut self,
        bytes: Arc<[u8]>,
        deadline: Instant,
        stop: StopToken,
    ) -> Result<WriteSuccess, WriteFailure> {
        let mut written = 0;
        while written < bytes.len() {
            let interrupted = if stop.is_stopped() {
                Some(WriteFailureKind::Cancelled)
            } else if Instant::now() >= deadline {
                Some(WriteFailureKind::Timeout)
            } else {
                None
            };
            if let Some(kind) = interrupted {
                return Err(WriteFailure::exact(kind, written, written == 0));
            }
            match self.0.write(&bytes[written..]) {
                Ok(0) => {
                    return Err(WriteFailure::exact(
                        WriteFailureKind::Io(
                            io::Error::new(io::ErrorKind::WriteZero, "zero-length PTY write")
                                .into(),
                        ),
                        written,
                        false,
                    ));
                }
                Ok(n) => written += n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if let Err(kind) = self.writable(deadline, &stop) {
                        let reusable = written == 0
                            && matches!(
                                kind,
                                WriteFailureKind::Timeout | WriteFailureKind::Cancelled
                            );
                        return Err(WriteFailure::exact(kind, written, reusable));
                    }
                }
                Err(error) => {
                    return Err(WriteFailure::exact(
                        WriteFailureKind::Io(error.into()),
                        written,
                        false,
                    ));
                }
            }
        }
        // std::fs::File has no userspace write buffer. flush does not tcdrain:
        // the acknowledgement is kernel acceptance, not child consumption.
        self.0.flush().map_err(|error| {
            WriteFailure::exact(WriteFailureKind::Io(error.into()), written, false)
        })?;
        Ok(WriteSuccess {
            written,
            reusable: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    #[test]
    fn stalled_nonblocking_write_reports_exact_prefix_and_reader_remains_interruptible() {
        let (master, mut peer) = UnixStream::pair().unwrap();
        let (mut reader, interrupt) = InterruptibleReader::duplicate(master.as_raw_fd()).unwrap();
        let mut writer = NonblockingWriter::duplicate(master.as_raw_fd()).unwrap();
        let bytes: Arc<[u8]> = vec![b'x'; 8 * 1024 * 1024].into();
        let failure = writer
            .write_until(
                bytes,
                Instant::now() + Duration::from_millis(50),
                StopToken::default(),
            )
            .unwrap_err();
        assert_eq!(failure.kind, WriteFailureKind::Timeout);
        assert!(failure.definitely_written > 0);
        assert_eq!(failure.definitely_written, failure.possibly_written);
        assert!(failure.settled);
        let mut received = vec![0; failure.definitely_written];
        peer.read_exact(&mut received).unwrap();
        assert!(received.iter().all(|byte| *byte == b'x'));
        interrupt.interrupt();
        assert_eq!(
            reader.read(&mut [0; 8]).unwrap(),
            InterruptibleRead::Interrupted
        );
        peer.write_all(b"output").unwrap();
        let mut output = [0; 8];
        assert_eq!(
            reader.read(&mut output).unwrap(),
            InterruptibleRead::Data(6)
        );
        assert_eq!(&output[..6], b"output");
    }

    #[test]
    fn pre_cancelled_write_delivers_no_bytes() {
        let (master, mut peer) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let mut writer = NonblockingWriter::duplicate(master.as_raw_fd()).unwrap();
        let stop = StopToken::default();
        stop.stop();
        let failure = writer
            .write_until(
                Arc::from(b"never".as_slice()),
                Instant::now() + Duration::from_secs(1),
                stop,
            )
            .unwrap_err();
        assert_eq!(failure.definitely_written, 0);
        assert!(failure.reusable);
        assert_eq!(
            peer.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}
