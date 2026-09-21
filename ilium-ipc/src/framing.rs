//! Length-prefixed bincode framing over any `AsyncRead`/`AsyncWrite`.
//!
//! Frame shape on the wire: a 4-byte little-endian `u32` payload length,
//! followed by exactly that many bytes of bincode-encoded payload. Generic
//! over the payload type so `ilium-server` and `ilium-client` reuse the
//! same code for both the request stream (`ClientRequest`) and the event
//! stream (`ServerEvent`); generic over the stream type so tests can frame
//! into an in-memory buffer instead of a real socket, and so
//! `ilium-server` can plug in a Unix domain socket later without this
//! module changing.

use bincode::Options;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::IpcError;

/// The bincode configuration used to decode a frame payload: fixed-width
/// integers (wire-compatible with what `bincode::serialize` already
/// produces on the write side), but -- unlike `bincode::deserialize`'s
/// convenience default, which silently allows and ignores trailing bytes
/// -- keeping bincode's own default of erroring if the payload isn't fully
/// consumed. Without this, a corrupted frame whose bytes happen to decode
/// as a valid but truncated `T` (e.g. a flipped length field inside the
/// payload) would silently misparse instead of surfacing as an error, the
/// exact failure mode `read_frame`'s doc comment promises callers it won't
/// hit.
fn frame_decode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new().with_fixint_encoding()
}

/// Guards against a desynchronized stream being misread as a single
/// enormous frame: real ilium-ipc messages (tree snapshots, terminal
/// output chunks, key input) never approach this size, so a length prefix
/// this large means the stream is corrupt, not that a legitimate frame is
/// this big.
pub const MAX_FRAME_LEN: u32 = 64 * 1024 * 1024; // 64 MiB

const LENGTH_HEADER_BYTES: usize = 4;

/// A connection-owned encoder that retains its serialization allocation
/// across frames, so long-lived connections don't pay a fresh `Vec`
/// allocation per message.
pub struct FrameWriter<W> {
    writer: W,
    payload: Vec<u8>,
}

impl<W> FrameWriter<W>
where
    W: AsyncWrite + Unpin,
{
    /// Wraps a stream half with an initially empty reusable frame buffer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            payload: Vec::new(),
        }
    }

    /// Serializes and submits one complete frame with a single write path,
    /// flushing before returning so the frame is actually delivered even
    /// when the underlying stream buffers writes.
    ///
    /// Not cancel safe: dropping this future after the first partial write
    /// leaves a half-written frame on the wire, which desynchronizes the
    /// peer's reader for the rest of the connection. Never use it directly
    /// as a `select!` branch -- awaiting it inside a branch *body* (which
    /// runs after the select has already resolved) is fine, and is what
    /// every caller does today.
    pub async fn write<T>(&mut self, value: &T) -> Result<(), IpcError>
    where
        T: Serialize,
    {
        // `serialize_into` uses the same legacy fixint wire config as
        // `bincode::serialize`, but appends into the retained buffer
        // instead of allocating a fresh Vec per frame.
        self.payload.clear();
        bincode::serialize_into(&mut self.payload, value)?;
        let length: u32 = self
            .payload
            .len()
            .try_into()
            .map_err(|_| IpcError::frame_too_large(self.payload.len()))?;
        if length > MAX_FRAME_LEN {
            return Err(IpcError::frame_too_large(self.payload.len()));
        }

        let length_header = length.to_le_bytes();
        let buffers = [
            std::io::IoSlice::new(&length_header),
            std::io::IoSlice::new(&self.payload),
        ];
        let first_write = self.writer.write_vectored(&buffers).await?;
        if first_write == 0 {
            return Err(IpcError::Io(std::io::Error::from(
                std::io::ErrorKind::WriteZero,
            )));
        }

        let frame_len = LENGTH_HEADER_BYTES.saturating_add(self.payload.len());
        if first_write < LENGTH_HEADER_BYTES {
            self.writer.write_all(&length_header[first_write..]).await?;
            self.writer.write_all(&self.payload).await?;
        } else if first_write < frame_len {
            self.writer
                .write_all(&self.payload[first_write - LENGTH_HEADER_BYTES..])
                .await?;
        }

        // A raw socket half's flush is a free no-op; a buffered writer
        // (this module is generic over any `AsyncWrite`) would otherwise
        // hold the frame indefinitely and the peer would never see it.
        self.writer.flush().await?;
        Ok(())
    }
}

/// A connection-owned decoder that retains its payload allocation.
pub struct FrameReader<R> {
    reader: R,
    payload: Vec<u8>,
}

impl<R> FrameReader<R>
where
    R: AsyncRead + Unpin,
{
    /// Wraps a stream half with an initially empty reusable payload buffer.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            payload: Vec::new(),
        }
    }

    /// Reads and decodes one frame, retaining payload capacity for the next.
    ///
    /// Not cancel safe: bytes already consumed from the stream are lost if
    /// this future is dropped mid-frame, so a caller that races it in a
    /// `select!` must abandon the stream when another branch wins rather
    /// than calling `read` again on it.
    pub async fn read<T>(&mut self) -> Result<T, IpcError>
    where
        T: DeserializeOwned,
    {
        let mut length_bytes = [0u8; LENGTH_HEADER_BYTES];
        self.reader.read_exact(&mut length_bytes).await?;
        let length = u32::from_le_bytes(length_bytes);
        if length > MAX_FRAME_LEN {
            return Err(IpcError::bad_length_prefix(length));
        }

        self.payload.resize(length as usize, 0);
        match self.reader.read_exact(&mut self.payload).await {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(IpcError::TruncatedFrame { expected: length });
            }
            Err(error) => return Err(IpcError::Io(error)),
        }

        Ok(frame_decode_options().deserialize(&self.payload)?)
    }
}

/// Serializes `value` and writes it as one length-prefixed frame.
///
/// Long-lived connections should use [`FrameWriter`] directly so its
/// allocation survives across frames. This convenience function preserves
/// the simple one-shot API used by CLI control requests and tests.
pub async fn write_frame<T, W>(writer: &mut W, value: &T) -> Result<(), IpcError>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let mut frame_writer = FrameWriter::new(writer);
    frame_writer.write(value).await
}

/// Reads one length-prefixed frame and decodes it as `T`. Returns `Err`
/// rather than panicking or silently misparsing on a bad length prefix, a
/// connection that closes mid-payload, or bytes that don't decode as `T`
/// (e.g. a client/server built from mismatched protocol versions).
///
/// An `Err(IpcError::Io(e))` where `e.kind() ==
/// std::io::ErrorKind::UnexpectedEof` while reading the length header
/// itself is the normal way a peer signals "no more frames" -- callers
/// reading a stream in a loop should treat that as end-of-stream, not a
/// protocol error. An `Err(IpcError::TruncatedFrame { .. })` means a frame
/// was started but never completed, which is always a real problem.
pub async fn read_frame<T, R>(reader: &mut R) -> Result<T, IpcError>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    let mut frame_reader = FrameReader::new(reader);
    frame_reader.read().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// Counts framing-layer write and flush calls while accepting every byte.
    #[derive(Default)]
    struct CountingWriter {
        write_calls: usize,
        flush_calls: usize,
    }

    impl AsyncWrite for CountingWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.write_calls += 1;
            Poll::Ready(Ok(bytes.len()))
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffers: &[std::io::IoSlice<'_>],
        ) -> Poll<std::io::Result<usize>> {
            self.write_calls += 1;
            Poll::Ready(Ok(buffers.iter().map(|buffer| buffer.len()).sum()))
        }

        fn is_write_vectored(&self) -> bool {
            true
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            self.flush_calls += 1;
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    #[ignore = "manual performance benchmark"]
    async fn benchmark_frame_write() {
        const ITERATIONS: usize = 20_000;
        let value = "x".repeat(256);
        let mut writer = CountingWriter::default();
        let started_at = std::time::Instant::now();
        for _iteration in 0..ITERATIONS {
            write_frame(&mut writer, &value).await.unwrap();
        }
        let elapsed = started_at.elapsed();
        println!(
            "PERF ipc.frame_write mean_ns={} write_calls_per_frame={} flush_calls_per_frame={}",
            elapsed.as_nanos() / ITERATIONS as u128,
            writer.write_calls / ITERATIONS,
            writer.flush_calls / ITERATIONS,
        );
    }

    #[tokio::test]
    #[ignore = "manual performance benchmark"]
    async fn benchmark_frame_read_buffer_reuse() {
        const ITERATIONS: usize = 20_000;
        let value = "x".repeat(256);
        let payload = bincode::serialize(&value).unwrap();
        let mut encoded = Vec::with_capacity((payload.len() + LENGTH_HEADER_BYTES) * ITERATIONS);
        for _iteration in 0..ITERATIONS {
            encoded.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            encoded.extend_from_slice(&payload);
        }

        let mut allocating_cursor = Cursor::new(encoded.clone());
        let allocating_started_at = std::time::Instant::now();
        for _iteration in 0..ITERATIONS {
            let decoded: String = read_frame(&mut allocating_cursor).await.unwrap();
            std::hint::black_box(decoded);
        }
        let allocating_elapsed = allocating_started_at.elapsed();

        let mut frame_reader = FrameReader::new(Cursor::new(encoded));
        let reused_started_at = std::time::Instant::now();
        for _iteration in 0..ITERATIONS {
            let decoded: String = frame_reader.read().await.unwrap();
            std::hint::black_box(decoded);
        }
        let reused_elapsed = reused_started_at.elapsed();
        println!(
            "PERF ipc.frame_read allocating_ns={} reused_ns={}",
            allocating_elapsed.as_nanos() / ITERATIONS as u128,
            reused_elapsed.as_nanos() / ITERATIONS as u128,
        );
    }

    #[tokio::test]
    async fn round_trips_a_simple_value_through_a_cursor() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &"hello ilium".to_string())
            .await
            .unwrap();

        let mut cursor = Cursor::new(buffer);
        let decoded: String = read_frame(&mut cursor).await.unwrap();
        assert_eq!(decoded, "hello ilium");
    }

    #[tokio::test]
    async fn read_frame_on_empty_stream_is_an_io_eof_not_a_panic() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        match result {
            Err(IpcError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof),
            other => panic!("expected Io(UnexpectedEof), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_frame_on_truncated_payload_errors_instead_of_panicking() {
        // A full length header promising 100 bytes, but no payload at all
        // -- simulates a connection dying mid-frame.
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&100u32.to_le_bytes());

        let mut cursor = Cursor::new(buffer);
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        match result {
            Err(IpcError::TruncatedFrame { expected: 100 }) => {}
            other => panic!("expected TruncatedFrame {{ expected: 100 }}, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_frame_on_partially_delivered_payload_errors() {
        let payload = bincode::serialize(&"a longer payload than what arrives".to_string())
            .expect("serializable");
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        // Only send half the promised payload bytes.
        buffer.extend_from_slice(&payload[..payload.len() / 2]);

        let mut cursor = Cursor::new(buffer);
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        assert!(matches!(result, Err(IpcError::TruncatedFrame { .. })));
    }

    #[tokio::test]
    async fn read_frame_rejects_an_implausible_length_prefix() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&(MAX_FRAME_LEN + 1).to_le_bytes());

        let mut cursor = Cursor::new(buffer);
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        match result {
            Err(IpcError::BadLengthPrefix { actual, .. }) => {
                assert_eq!(actual, MAX_FRAME_LEN + 1)
            }
            other => panic!("expected BadLengthPrefix, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_frame_rejects_bytes_that_dont_decode_as_the_target_type() {
        // A well-formed frame (correct length, fully delivered) whose
        // payload is not valid bincode for the type we ask it to decode
        // as -- must error, not silently misparse.
        let mut buffer = Vec::new();
        let garbage = vec![0xFFu8; 8];
        buffer.extend_from_slice(&(garbage.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&garbage);

        let mut cursor = Cursor::new(buffer);
        // A String decode expects a valid-length-prefixed UTF-8 body;
        // 0xFF bytes as a bincode-encoded String are not that.
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        assert!(matches!(result, Err(IpcError::Bincode(_))));
    }

    #[tokio::test]
    async fn read_frame_rejects_a_payload_with_unconsumed_trailing_bytes() {
        // A length prefix that promises more bytes than the encoded value
        // actually needs -- e.g. a desynchronized stream that happens to
        // land on a byte sequence which decodes as a valid but short `T`.
        // Must error rather than silently accept the value and drop the
        // extra bytes.
        let mut payload = bincode::serialize(&"short".to_string()).expect("serializable");
        payload.extend_from_slice(&[0u8; 4]);
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&payload);

        let mut cursor = Cursor::new(buffer);
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        assert!(matches!(result, Err(IpcError::Bincode(_))));
    }
}
