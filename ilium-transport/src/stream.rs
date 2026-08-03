//! One connected client/server byte stream, and its two owned halves.
//!
//! The client drives a reader task and a writer task independently, each with
//! its own tracked `JoinHandle`, so the halves must be *owned* rather than
//! borrowed -- a borrowing split cannot cross into a `'static` spawned task.
//! `tokio::io::split` provides exactly that for any `AsyncRead + AsyncWrite`,
//! which is why this type implements both by delegation instead of exposing
//! the platform stream directly.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(unix)]
use crate::unix as imp;
#[cfg(windows)]
use crate::windows as imp;

/// An established connection between a client and its session server.
#[derive(Debug)]
pub struct SessionStream {
    pub(crate) inner: imp::Stream,
}

/// The owned reading half of a [`SessionStream`].
pub type SessionReadHalf = tokio::io::ReadHalf<SessionStream>;
/// The owned writing half of a [`SessionStream`].
pub type SessionWriteHalf = tokio::io::WriteHalf<SessionStream>;

impl SessionStream {
    /// Splits into two owned halves that can be moved into separate tasks.
    pub fn into_split(self) -> (SessionReadHalf, SessionWriteHalf) {
        tokio::io::split(self)
    }

    /// The process id on the other end, when the platform can report it.
    ///
    /// The CLI uses this to learn which process owns a live session, so it can
    /// wait for that exact process to exit rather than trusting a socket probe
    /// -- a dying listener can still accept a queued connection and look alive.
    pub fn peer_process_id(&self) -> Option<u32> {
        imp::peer_process_id(&self.inner)
    }
}

impl AsyncRead for SessionStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for SessionStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}
