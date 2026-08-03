//! Typed errors for the wire-framing layer. A malformed or truncated frame
//! must always come back as an `Err` the caller can match on -- never a
//! panic and never a value silently decoded from garbage bytes. This is
//! what lets a client/server version mismatch fail loudly (ARCHITECTURE.md M2)
//! instead of misparsing whatever bytes happened to be on the wire.

use crate::framing::MAX_FRAME_LEN;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// The stream itself failed (socket closed, reset, etc), or hit EOF
    /// while reading a frame's 4-byte length header -- the latter is the
    /// normal way a peer signals "no more frames" between messages, so
    /// callers reading a stream in a loop should treat an `Io` error whose
    /// `ErrorKind` is `UnexpectedEof` as "the other side hung up" rather
    /// than a corrupt payload.
    #[error("ipc stream io error: {0}")]
    Io(#[from] std::io::Error),

    /// The 4-byte length prefix decoded to a value larger than
    /// [`MAX_FRAME_LEN`]. A real frame never gets this big; a value this
    /// large means the stream is desynchronized (reading garbage as a
    /// length header) rather than a legitimately huge message, so this is
    /// treated as corruption, not "read more."
    #[error("frame length prefix {actual} exceeds the maximum allowed frame size ({max} bytes)")]
    BadLengthPrefix { actual: u32, max: u32 },

    /// The length header was read successfully (so the peer started a
    /// frame) but the connection closed or errored before the promised
    /// number of payload bytes arrived. Distinct from a header-position
    /// `Io` EOF, which just means "no more frames are coming."
    #[error("frame payload truncated: expected {expected} bytes, connection closed early")]
    TruncatedFrame { expected: u32 },

    /// A frame we're about to write is too large to send: either it
    /// exceeds [`MAX_FRAME_LEN`] (the same corruption-guard bound
    /// `read_frame` enforces) or it doesn't even fit in the `u32` length
    /// prefix. Not expected to happen in practice for this protocol's
    /// message sizes, but guarded rather than left to panic on the cast.
    #[error(
        "frame payload of {actual} bytes exceeds the maximum encodable frame size ({max} bytes)"
    )]
    FrameTooLarge { actual: usize, max: u32 },

    /// The payload bytes did not decode as the expected type -- either a
    /// genuinely corrupt frame or a client/server built from mismatched
    /// protocol versions.
    #[error("failed to (de)serialize frame payload: {0}")]
    Bincode(#[from] bincode::Error),
}

impl IpcError {
    pub(crate) fn bad_length_prefix(actual: u32) -> Self {
        IpcError::BadLengthPrefix {
            actual,
            max: MAX_FRAME_LEN,
        }
    }

    pub(crate) fn frame_too_large(actual: usize) -> Self {
        IpcError::FrameTooLarge {
            actual,
            max: MAX_FRAME_LEN,
        }
    }
}
