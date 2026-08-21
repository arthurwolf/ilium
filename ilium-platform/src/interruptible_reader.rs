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
    pub fn duplicate(data_fd: RawFd) -> io::Result<(Self, ReaderInterrupt)> {
        // SAFETY: `data_fd` is borrowed only for `fcntl`; successful `dup`
        // returns an independently owned descriptor.
        let duplicated_fd = unsafe { libc::fcntl(data_fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicated_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful `fcntl` above returned exclusive ownership.
        let data = unsafe { File::from_raw_fd(duplicated_fd) };
        // The initial poll below remains the idle wait. Nonblocking mode only
        // affects the bounded drain after that wake, letting one consumer
        // collapse every byte already queued by the kernel into one logical
        // read without ever waiting for a future byte.
        set_descriptor_flags(&data)?;

        let mut wake_fds = [-1; 2];
        // SAFETY: `wake_fds` points to two writable descriptor slots.
        if unsafe { libc::pipe(wake_fds.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful `pipe` returned two independently owned fds.
        let wake_read = unsafe { File::from_raw_fd(wake_fds[0]) };
        // SAFETY: same invariant as `wake_read` for the other pipe end.
        let wake_write = unsafe { File::from_raw_fd(wake_fds[1]) };
        // Nonblocking so `read`'s wake handler can drain every queued byte
        // (see `interrupt`'s doc comment on coalescing) without risking a
        // block on the final read once the pipe is actually empty.
        set_descriptor_flags(&wake_read)?;
        set_descriptor_flags(&wake_write)?;

        Ok((
            Self { data, wake_read },
            ReaderInterrupt {
                wake_write: Arc::new(wake_write),
            },
        ))
    }

    /// Waits indefinitely for data or an explicit owner interruption.
    pub fn read(&mut self, buffer: &mut [u8]) -> io::Result<InterruptibleRead> {
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
                    match self.data.read(&mut buffer[total_bytes_read..]) {
                        Ok(0) if total_bytes_read == 0 => return Ok(InterruptibleRead::Eof),
                        Ok(0) => return Ok(InterruptibleRead::Data(total_bytes_read)),
                        Ok(bytes_read) => {
                            total_bytes_read += bytes_read;
                            if total_bytes_read == buffer.len() {
                                return Ok(InterruptibleRead::Data(total_bytes_read));
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
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

fn set_descriptor_flags(file: &File) -> io::Result<()> {
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
}
