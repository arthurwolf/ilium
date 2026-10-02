//! Native PTY I/O plumbing. Pumps move bytes only; never parser or geometry.
//!
//! The master control handle has exactly one owner. Unix writes use an owned
//! nonblocking master duplicate. Windows takes the opaque portable writer and
//! cancels synchronous I/O through its owned THREAD handle, not a writer handle.

use crate::owned_worker::{OwnedWorker, StopToken, WorkerTicket};
use portable_pty::{MasterPty, PtySize};
use std::io;
use std::sync::Arc;
use std::time::Instant;

mod async_writer;
pub use async_writer::{AsyncWriter, WriteProgress};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
use unix as native;
#[cfg(windows)]
use windows as native;

pub const OUTPUT_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoFailure {
    pub kind: io::ErrorKind,
    pub message: String,
}

impl From<io::Error> for IoFailure {
    fn from(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFailureKind {
    Timeout,
    Cancelled,
    Io(IoFailure),
    WorkerStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFailure {
    pub kind: WriteFailureKind,
    pub definitely_written: usize,
    pub possibly_written: usize,
    /// False means a Windows operation has not acknowledged cancellation yet.
    pub settled: bool,
    pub reusable: bool,
}

impl WriteFailure {
    pub fn exact(kind: WriteFailureKind, written: usize, reusable: bool) -> Self {
        Self {
            kind,
            definitely_written: written,
            possibly_written: written,
            settled: true,
            reusable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteSuccess {
    pub written: usize,
    /// A completed write can win a cancellation race while its pump is retiring.
    pub reusable: bool,
}

pub trait PtyWriter: Send {
    /// Success means all bytes and flush completed, NEVER merely queued.
    fn write_until(
        &mut self,
        bytes: Arc<[u8]>,
        deadline: Instant,
        stop: StopToken,
    ) -> Result<WriteSuccess, WriteFailure>;
    fn worker_ticket(&self) -> Option<WorkerTicket> {
        None
    }
}

pub trait PtyControl: Send {
    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), IoFailure>;
    fn size(&self) -> Result<(u16, u16), IoFailure>;
}

struct NativeControl(Box<dyn MasterPty + Send>);

impl PtyControl for NativeControl {
    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), IoFailure> {
        self.0
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| IoFailure {
                kind: io::ErrorKind::Other,
                message: error.to_string(),
            })
    }
    fn size(&self) -> Result<(u16, u16), IoFailure> {
        self.0
            .get_size()
            .map(|size| (size.rows, size.cols))
            .map_err(|error| IoFailure {
                kind: io::ErrorKind::Other,
                message: error.to_string(),
            })
    }
}

#[derive(Debug)]
pub enum ReadMessage {
    Data(Arc<[u8]>),
    Eof,
    Error(IoFailure),
}

pub struct OutputReader {
    #[cfg(any(unix, windows))]
    inner: native::Reader,
}

impl OutputReader {
    pub fn spawn(
        self,
        stop: StopToken,
        sink: impl FnMut(ReadMessage, &StopToken) -> bool + Send + 'static,
    ) -> io::Result<OwnedWorker> {
        #[cfg(any(unix, windows))]
        {
            self.inner.spawn(stop, sink)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (self, stop, sink);
            Err(unsupported())
        }
    }
}

pub struct ShellProbe {
    #[cfg(any(unix, windows))]
    inner: native::ShellProbe,
}

impl ShellProbe {
    /// Observational only: no parser, resize, or input side effects.
    pub fn shell_owns_terminal(&self, process_id: u32) -> Option<bool> {
        #[cfg(any(unix, windows))]
        {
            self.inner.shell_owns_terminal(process_id)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = process_id;
            None
        }
    }
}

pub struct TransportParts {
    pub control: Box<dyn PtyControl>,
    pub writer: Box<dyn PtyWriter>,
    pub reader: OutputReader,
    pub shell_probe: ShellProbe,
}

/// Prepared I/O deliberately BORROWS the master during fallible setup. The
/// caller retains the master so it can kill/reap a just-spawned child before
/// native control destruction on an error path.
pub struct PreparedTransport {
    writer: Box<dyn PtyWriter>,
    reader: OutputReader,
    shell_probe: ShellProbe,
}

impl PreparedTransport {
    pub fn attach_master(self, master: Box<dyn MasterPty + Send>) -> TransportParts {
        TransportParts {
            control: Box::new(NativeControl(master)),
            writer: self.writer,
            reader: self.reader,
            shell_probe: self.shell_probe,
        }
    }
}

pub fn prepare(master: &(dyn MasterPty + Send), stop: StopToken) -> io::Result<PreparedTransport> {
    #[cfg(any(unix, windows))]
    {
        let (reader, writer, shell_probe) = native::open(master, stop)?;
        Ok(PreparedTransport {
            writer,
            reader: OutputReader { inner: reader },
            shell_probe: ShellProbe { inner: shell_probe },
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (master, stop);
        Err(unsupported())
    }
}

#[cfg(not(any(unix, windows)))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "bounded PTY I/O requires the Unix or Windows platform adapter",
    )
}
