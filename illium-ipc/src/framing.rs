//! Length-prefixed bincode framing over any `AsyncRead`/`AsyncWrite`.
//!
//! Frame shape on the wire: a 4-byte little-endian `u32` payload length,
//! followed by exactly that many bytes of bincode-encoded payload. Generic
//! over the payload type so `illium-server` and `illium-client` reuse the
//! same code for both the request stream (`ClientRequest`) and the event
//! stream (`ServerEvent`); generic over the stream type so tests can frame
//! into an in-memory buffer instead of a real socket, and so
//! `illium-server` can plug in a Unix domain socket later without this
//! module changing.

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::IpcError;

/// Guards against a desynchronized stream being misread as a single
/// enormous frame: real illium-ipc messages (tree snapshots, terminal
/// output chunks, key input) never approach this size, so a length prefix
/// this large means the stream is corrupt, not that a legitimate frame is
/// this big.
pub const MAX_FRAME_LEN: u32 = 64 * 1024 * 1024; // 64 MiB

const LENGTH_HEADER_BYTES: usize = 4;

/// Serializes `value` with bincode and writes it as one length-prefixed
/// frame, flushing so the peer can read it without waiting on more data.
pub async fn write_frame<T, W>(writer: &mut W, value: &T) -> Result<(), IpcError>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let payload = bincode::serialize(value)?;
    let length: u32 = payload
        .len()
        .try_into()
        .map_err(|_| IpcError::frame_too_large(payload.len()))?;
    if length > MAX_FRAME_LEN {
        return Err(IpcError::frame_too_large(payload.len()));
    }

    writer.write_all(&length.to_le_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
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
    let mut length_bytes = [0u8; LENGTH_HEADER_BYTES];
    // A failure here (including EOF) means no frame was started at all --
    // propagated as-is via `#[from] io::Error` so the caller can tell it
    // apart from a frame that started but didn't finish.
    reader.read_exact(&mut length_bytes).await?;
    let length = u32::from_le_bytes(length_bytes);
    if length > MAX_FRAME_LEN {
        return Err(IpcError::bad_length_prefix(length));
    }

    let mut payload = vec![0u8; length as usize];
    match reader.read_exact(&mut payload).await {
        Ok(_) => {}
        // The header promised `length` bytes but the stream ended first --
        // this is a genuinely truncated frame, not a clean end-of-stream.
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(IpcError::TruncatedFrame { expected: length });
        }
        Err(e) => return Err(IpcError::Io(e)),
    }

    let value = bincode::deserialize(&payload)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn round_trips_a_simple_value_through_a_cursor() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &"hello illium".to_string())
            .await
            .unwrap();

        let mut cursor = Cursor::new(buffer);
        let decoded: String = read_frame(&mut cursor).await.unwrap();
        assert_eq!(decoded, "hello illium");
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
}
