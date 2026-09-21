//! Wake-driven Unix file-descriptor reads for background worker ownership.
//!
//! A private nonblocking pipe is polled beside the duplicated data descriptor.
//! Dropping an owner writes one byte through [`ReaderInterrupt`], waking an
//! otherwise indefinite `poll()` without periodic timer interrupts.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::Arc;

/// Result of one interruptible read attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptibleRead {
    Data(usize),
    Eof,
    Interrupted,
}

/// Reader-side ownership of a duplicated data descriptor and wake pipe.
pub struct InterruptibleReader {
    data: File,
    wake_read: File,
}

/// Cloneable owner-side handle that wakes an [`InterruptibleReader`].
#[derive(Clone)]
pub struct ReaderInterrupt {
    wake_write: Arc<File>,
}

impl InterruptibleReader {
    /// Duplicates `data_fd` and creates a private close-on-exec wake pipe.
    ///
    /// The duplicate is deliberately left exactly as the caller configured its
    /// descriptor. `dup`/`F_DUPFD_CLOEXEC` produces a second descriptor onto
    /// the *same* open file description, and `O_NONBLOCK` lives on that shared
    /// description -- so switching this duplicate to nonblocking mode would
    /// also switch the caller's own descriptor, and every writer cloned from
    /// it. For a pty master that turns a momentarily full input buffer into a
    /// failed `write_all` and silently dropped user input. `read` therefore
    /// bounds its drain with a zero-timeout `poll` instead of relying on
    /// `EAGAIN` (see `data_has_queued_bytes`).
    pub fn duplicate(data_fd: RawFd) -> io::Result<(Self, ReaderInterrupt)> {
        // SAFETY: `data_fd` is borrowed only for `fcntl`; successful `dup`
        // returns an independently owned descriptor.
        let duplicated_fd = unsafe { libc::fcntl(data_fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicated_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful `fcntl` above returned exclusive ownership.
        // `F_DUPFD_CLOEXEC` already set close-on-exec atomically, so no further
        // `fcntl` on this descriptor is needed -- or wanted, per the note above.
        let data = unsafe { File::from_raw_fd(duplicated_fd) };

        let (wake_read, wake_write) = create_wake_pipe()?;

        Ok((
            Self { data, wake_read },
            ReaderInterrupt {
                wake_write: Arc::new(wake_write),
            },
        ))
    }

    /// Waits indefinitely for data or an explicit owner interruption.
    pub fn read(&mut self, buffer: &mut [u8]) -> io::Result<InterruptibleRead> {
        // Mirrors `Read::read`: a zero-length buffer can hold no bytes, which
        // is not the same thing as the descriptor having reached end of file.
        // Reporting `Eof` here would tell a caller its pty had hung up.
        if buffer.is_empty() {
            return Ok(InterruptibleRead::Data(0));
        }

        loop {
            let mut poll_fds = [
                libc::pollfd {
                    fd: self.data.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.wake_read.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: the array contains exactly two valid poll descriptors
            // and remains alive for the complete call.
            let poll_result = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, -1) };
            if poll_result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            // Also fires on `POLLHUP`, i.e. once every `ReaderInterrupt` clone
            // has been dropped. Reporting `Interrupted` is exactly right then:
            // nobody is left who could ever wake this reader again, so the
            // owner has effectively asked it to stop.
            if poll_fds[1].revents != 0 {
                // Drains every byte the wake pipe has queued, not just the
                // one that woke this poll: `interrupt` writes are only
                // best-effort coalesced (a `write` fails only once the pipe
                // is entirely full, not merely non-empty), so more than one
                // byte can be queued by the time this fires. Leaving extras
                // behind would make a future `read` observe stale POLLIN and
                // report a spurious `Interrupted` with no new interrupt.
                let mut wake_byte = [0_u8; 1];
                loop {
                    match self.wake_read.read(&mut wake_byte) {
                        // A closed write end reports EOF here forever; stop
                        // draining rather than spin at 100% CPU on `Ok(0)`.
                        Ok(0) => break,
                        Ok(_) => continue,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
                return Ok(InterruptibleRead::Interrupted);
            }
            if poll_fds[0].revents != 0 {
                let mut total_bytes_read = 0;
                loop {
                    // Bound as its own statement so the borrow of `self.data`
                    // ends before an arm re-borrows `self` for the readiness
                    // check below.
                    let read_result = self.data.read(&mut buffer[total_bytes_read..]);
                    match read_result {
                        Ok(0) if total_bytes_read == 0 => return Ok(InterruptibleRead::Eof),
                        Ok(0) => return Ok(InterruptibleRead::Data(total_bytes_read)),
                        Ok(bytes_read) => {
                            total_bytes_read += bytes_read;
                            if total_bytes_read == buffer.len() {
                                return Ok(InterruptibleRead::Data(total_bytes_read));
                            }
                            // The descriptor keeps whatever blocking mode the
                            // caller gave it, so a second `read` is only safe
                            // while the kernel still holds queued bytes. This
                            // collapses one already-arrived burst into a single
                            // logical read without ever waiting for a byte that
                            // has not arrived yet.
                            if !self.data_has_queued_bytes() {
                                return Ok(InterruptibleRead::Data(total_bytes_read));
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        // Reachable only when the caller's own open file
                        // description is already nonblocking; the readiness
                        // check above otherwise keeps this drain off the
                        // `EAGAIN` path entirely.
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            if total_bytes_read > 0 {
                                return Ok(InterruptibleRead::Data(total_bytes_read));
                            }
                            break;
                        }
                        // PTY masters report EIO after their slave closes on
                        // Unix; preserve bytes drained immediately before the
                        // hangup, then report EOF on the next call.
                        Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                            return if total_bytes_read == 0 {
                                Ok(InterruptibleRead::Eof)
                            } else {
                                Ok(InterruptibleRead::Data(total_bytes_read))
                            };
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }

    /// Zero-timeout readiness check bounding `read`'s drain loop.
    ///
    /// A poll failure ends the drain instead of being reported: the bytes
    /// already sitting in the caller's buffer must not be discarded, and the
    /// next `read` call polls the same descriptor indefinitely and surfaces
    /// the very same failure there.
    fn data_has_queued_bytes(&self) -> bool {
        loop {
            let mut poll_fds = [libc::pollfd {
                fd: self.data.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            }];
            // SAFETY: the array contains exactly one valid poll descriptor and
            // remains alive for the complete call, which returns immediately
            // because the timeout is zero.
            let poll_result = unsafe { libc::poll(poll_fds.as_mut_ptr(), 1, 0) };
            if poll_result < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return false;
            }
            return poll_result > 0 && poll_fds[0].revents != 0;
        }
    }
}

impl ReaderInterrupt {
    /// Wakes the reader. Repeated calls pile bytes into the kernel pipe
    /// buffer, which the reader drains completely on each wake.
    pub fn interrupt(&self) {
        let byte = [1_u8; 1];
        loop {
            // SAFETY: `wake_write` owns a live pipe descriptor for the whole
            // call, and `byte` outlives the write.
            let written = unsafe {
                libc::write(
                    self.wake_write.as_raw_fd(),
                    byte.as_ptr().cast(),
                    byte.len(),
                )
            };
            if written >= 0 {
                return;
            }

            // A signal-interrupted write transferred nothing: retrying is the
            // only way the wake actually reaches an empty pipe. Every other
            // failure is safe to ignore -- the write end is nonblocking, so a
            // full pipe (`WouldBlock`) already holds a pending wake, and any
            // remaining error means the pipe is unusable and unrecoverable.
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return;
            }
        }
    }
}

/// Creates the private wake pipe.
///
/// Both ends are close-on-exec, so a pty spawned on another thread can never
/// inherit them, and both are nonblocking: the write end so `interrupt` cannot
/// stall an owner on a full pipe, the read end so `read`'s drain can stop on
/// `EAGAIN` once the pipe is actually empty.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn create_wake_pipe() -> io::Result<(File, File)> {
    let mut wake_fds = [-1; 2];
    // SAFETY: `wake_fds` points to two writable descriptor slots. `pipe2`
    // applies both flags atomically, leaving no window in which a concurrent
    // `fork`/`exec` on another thread could inherit these descriptors.
    if unsafe { libc::pipe2(wake_fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `pipe2` returned two independently owned fds.
    let wake_read = unsafe { File::from_raw_fd(wake_fds[0]) };
    // SAFETY: same invariant as `wake_read` for the other pipe end.
    let wake_write = unsafe { File::from_raw_fd(wake_fds[1]) };
    Ok((wake_read, wake_write))
}

/// Fallback for the Unix targets without `pipe2`, which have to set the same
/// two flags in a second step and therefore keep a small inheritance window.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn create_wake_pipe() -> io::Result<(File, File)> {
    let mut wake_fds = [-1; 2];
    // SAFETY: `wake_fds` points to two writable descriptor slots.
    if unsafe { libc::pipe(wake_fds.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `pipe` returned two independently owned fds.
    let wake_read = unsafe { File::from_raw_fd(wake_fds[0]) };
    // SAFETY: same invariant as `wake_read` for the other pipe end.
    let wake_write = unsafe { File::from_raw_fd(wake_fds[1]) };
    set_close_on_exec_and_nonblocking(&wake_read)?;
    set_close_on_exec_and_nonblocking(&wake_write)?;
    Ok((wake_read, wake_write))
}

/// Marks one *privately owned* descriptor close-on-exec and nonblocking.
///
/// Only ever called on the wake pipe: `O_NONBLOCK` is a property of the open
/// file description, so this must never be pointed at a descriptor whose
/// description is shared with a caller (see [`InterruptibleReader::duplicate`]).
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn set_close_on_exec_and_nonblocking(file: &File) -> io::Result<()> {
    let descriptor = file.as_raw_fd();
    // SAFETY: `descriptor` is live for this call and `F_SETFD` mutates only
    // its close-on-exec flag.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `F_GETFL` only reads the live descriptor's status flags.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `F_SETFL` updates status flags on this live descriptor.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    #[test]
    fn interrupt_wakes_an_idle_reader_without_a_poll_timeout() {
        let (data_source, _idle_peer) = UnixStream::pair().unwrap();
        let (mut reader, interrupt) =
            InterruptibleReader::duplicate(data_source.as_raw_fd()).unwrap();
        let started_at = Instant::now();
        let thread = std::thread::spawn(move || reader.read(&mut [0_u8; 1]).unwrap());
        std::thread::sleep(Duration::from_millis(10));
        interrupt.interrupt();
        assert_eq!(thread.join().unwrap(), InterruptibleRead::Interrupted);
        assert!(started_at.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn repeated_interrupts_before_a_drain_do_not_leave_a_stale_wake_pending() {
        let (data_source, mut idle_peer) = UnixStream::pair().unwrap();
        let (mut reader, interrupt) =
            InterruptibleReader::duplicate(data_source.as_raw_fd()).unwrap();

        // Two wakes queued before anything ever reads the pipe: a pipe
        // write only fails once its whole kernel buffer is full, so both
        // bytes land, not just one -- this is what a real `Drop` racing an
        // in-flight `interrupt` from another owner clone can produce.
        interrupt.interrupt();
        interrupt.interrupt();
        assert_eq!(
            reader.read(&mut [0_u8; 1]).unwrap(),
            InterruptibleRead::Interrupted
        );

        // A left-over queued byte would make this second, unrelated read
        // observe stale POLLIN on the wake pipe and report `Interrupted`
        // again with no further interrupt ever having happened.
        idle_peer.write_all(b"x").unwrap();
        assert_eq!(
            reader.read(&mut [0_u8; 1]).unwrap(),
            InterruptibleRead::Data(1)
        );
    }

    #[test]
    fn duplicating_leaves_the_callers_own_descriptor_blocking() {
        let (data_source, _idle_peer) = UnixStream::pair().unwrap();
        let original_fd = data_source.as_raw_fd();
        // SAFETY: `original_fd` is owned by the live `data_source` and
        // `F_GETFL` only reads its status flags.
        let flags_before = unsafe { libc::fcntl(original_fd, libc::F_GETFL) };
        assert!(flags_before >= 0);
        assert_eq!(flags_before & libc::O_NONBLOCK, 0);

        let (_reader, _interrupt) = InterruptibleReader::duplicate(original_fd).unwrap();

        // `F_SETFL` acts on the shared open file description, so a duplicate
        // that made itself nonblocking would drag the caller's descriptor --
        // and, for a pty master, every writer cloned from it -- along too.
        // SAFETY: same live descriptor, same read-only `fcntl`.
        let flags_after = unsafe { libc::fcntl(original_fd, libc::F_GETFL) };
        assert!(flags_after >= 0);
        assert_eq!(flags_after & libc::O_NONBLOCK, 0);
    }

    #[test]
    fn a_partly_filled_buffer_returns_the_queued_burst_without_waiting_for_more() {
        let (data_source, mut peer) = UnixStream::pair().unwrap();
        let (mut reader, _interrupt) =
            InterruptibleReader::duplicate(data_source.as_raw_fd()).unwrap();
        peer.write_all(b"abc").unwrap();

        // The descriptor stays blocking, so draining until `EAGAIN` would
        // park this thread on the fourth byte, which nobody will ever send.
        let mut buffer = [0_u8; 64];
        let started_at = Instant::now();
        assert_eq!(
            reader.read(&mut buffer).unwrap(),
            InterruptibleRead::Data(3)
        );
        assert_eq!(&buffer[..3], b"abc");
        assert!(started_at.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn an_empty_buffer_reports_no_bytes_rather_than_end_of_file() {
        let (data_source, _idle_peer) = UnixStream::pair().unwrap();
        let (mut reader, _interrupt) =
            InterruptibleReader::duplicate(data_source.as_raw_fd()).unwrap();
        assert_eq!(reader.read(&mut []).unwrap(), InterruptibleRead::Data(0));
    }
}
