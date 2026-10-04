//! Length-prefixed bincode framing over any `AsyncRead`/`AsyncWrite`.
//!
//! Frame shape: a 4-byte little-endian schema-tagged `u32` payload length,
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

/// Owned wire payload suitable for preparation on a CPU worker. Construction
/// enforces the same limit as the on-wire header before growing its buffer.
#[derive(Debug)]
pub struct EncodedFrame {
    payload: Vec<u8>,
}

impl EncodedFrame {
    pub fn retained_bytes(&self) -> usize {
        self.payload.capacity()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("IPC payload exceeds frame limit at {0} bytes")]
struct PayloadLimit(usize);

fn encode_error(error: bincode::Error) -> IpcError {
    if let bincode::ErrorKind::Io(io_error) = error.as_ref() {
        if let Some(limit) = io_error
            .get_ref()
            .and_then(|source| source.downcast_ref::<PayloadLimit>())
        {
            return IpcError::frame_too_large(limit.0);
        }
    }
    IpcError::Bincode(error)
}

struct LimitedPayload<'a>(&'a mut Vec<u8>);
impl std::io::Write for LimitedPayload<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let required = self
            .0
            .len()
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_FRAME_LEN as usize)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    PayloadLimit(self.0.len().saturating_add(bytes.len())),
                )
            })?;
        if required > self.0.capacity() {
            let capacity = required
                .max(self.0.capacity().saturating_mul(2).max(8192))
                .min(MAX_FRAME_LEN as usize);
            self.0
                .try_reserve_exact(capacity - self.0.len())
                .map_err(std::io::Error::other)?;
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Pure CPU work; callers with an interactive or coordination loop must run
/// this on their bounded execution bank, keeping its owned result charged.
pub fn encode_frame<T: Serialize>(value: &T) -> Result<EncodedFrame, IpcError> {
    let mut payload = Vec::new();
    bincode::serialize_into(LimitedPayload(&mut payload), value).map_err(encode_error)?;
    Ok(EncodedFrame { payload })
}

/// Conservative allocation capacity for this module's `LimitedPayload`
/// growth rule. Sizing is pure and must run on the same CPU worker as encoding.
/// The estimate includes the 8192-byte initial growth floor and each doubling;
/// it preserves the wire limit and does not allocate the serialized payload.
pub fn encoded_capacity_bound<T: Serialize>(value: &T) -> Result<usize, IpcError> {
    let length = usize::try_from(bincode::serialized_size(value)?)
        .map_err(|_| IpcError::frame_too_large(usize::MAX))?;
    if length > MAX_FRAME_LEN as usize {
        return Err(IpcError::frame_too_large(length));
    }
    Ok(if length == 0 {
        0
    } else {
        length
            .saturating_mul(2)
            .max(8192)
            .min(MAX_FRAME_LEN as usize)
    })
}

/// Pure ordered decoding, separated from transport reads for worker ownership.
pub fn decode_frame<T: DeserializeOwned>(frame: &EncodedFrame) -> Result<T, IpcError> {
    Ok(frame_decode_options()
        .with_limit(u64::from(MAX_FRAME_LEN))
        .deserialize(&frame.payload)?)
}

/// Allocation-bounded production decoding for the closed audited IPC graph.
/// Generic framing remains available for ordinary serializer fixtures.
pub fn decode_bounded_frame<T: crate::BoundedMessage>(frame: &EncodedFrame) -> Result<T, IpcError> {
    crate::bounded_decode::decode_bounded(&frame.payload)
}

// Persisted presentation revisions and captured title observations require
// a matching peer. The tag must occupy only bits in FRAME_SCHEMA_MASK.
const FRAME_SCHEMA_TAG: u32 = 0xC000_0000;
const FRAME_SCHEMA_MASK: u32 = 0xF800_0000;
fn frame_length_word(length: u32) -> u32 {
    FRAME_SCHEMA_TAG | length
}
fn frame_payload_length(word: u32) -> Result<u32, IpcError> {
    if word & FRAME_SCHEMA_MASK != FRAME_SCHEMA_TAG {
        return Err(IpcError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "incompatible Ilium IPC schema; client and server must use matching builds",
        )));
    }
    let length = word & !FRAME_SCHEMA_MASK;
    if length > MAX_FRAME_LEN {
        return Err(IpcError::bad_length_prefix(length));
    }
    Ok(length)
}

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
        bincode::serialize_into(LimitedPayload(&mut self.payload), value).map_err(encode_error)?;
        self.write_payload().await
    }

    /// Emits an already prepared complete payload without encoding on this
    /// task. The caller retains the charged result through the final flush.
    pub async fn write_encoded(&mut self, frame: &EncodedFrame) -> Result<(), IpcError> {
        Self::write_payload_bytes(&mut self.writer, &frame.payload).await
    }

    async fn write_payload(&mut self) -> Result<(), IpcError> {
        Self::write_payload_bytes(&mut self.writer, &self.payload).await
    }

    async fn write_payload_bytes(writer: &mut W, payload: &[u8]) -> Result<(), IpcError> {
        let length: u32 = payload
            .len()
            .try_into()
            .map_err(|_| IpcError::frame_too_large(payload.len()))?;
        if length > MAX_FRAME_LEN {
            return Err(IpcError::frame_too_large(payload.len()));
        }

        let length_header = frame_length_word(length).to_le_bytes();
        let buffers = [
            std::io::IoSlice::new(&length_header),
            std::io::IoSlice::new(payload),
        ];
        let first_write = writer.write_vectored(&buffers).await?;
        if first_write == 0 {
            return Err(IpcError::Io(std::io::Error::from(
                std::io::ErrorKind::WriteZero,
            )));
        }

        let frame_len = LENGTH_HEADER_BYTES.saturating_add(payload.len());
        if first_write < LENGTH_HEADER_BYTES {
            writer.write_all(&length_header[first_write..]).await?;
            writer.write_all(payload).await?;
        } else if first_write < frame_len {
            writer
                .write_all(&payload[first_write - LENGTH_HEADER_BYTES..])
                .await?;
        }

        // A raw socket half's flush is a free no-op; a buffered writer
        // (this module is generic over any `AsyncWrite`) would otherwise
        // hold the frame indefinitely and the peer would never see it.
        writer.flush().await?;
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
        self.read_payload_into_buffer().await?;
        Ok(frame_decode_options()
            .with_limit(u64::from(MAX_FRAME_LEN))
            .deserialize(&self.payload)?)
    }

    /// Reads every payload byte in order without doing CPU deserialization.
    /// Reserve input and result admission before calling this method.
    pub async fn read_encoded(&mut self) -> Result<EncodedFrame, IpcError> {
        self.read_payload_into_buffer().await?;
        Ok(EncodedFrame {
            payload: std::mem::take(&mut self.payload),
        })
    }

    /// Read only the fixed-size header while idle. This must precede CPU/byte
    /// admission so an idle peer cannot monopolize a finite execution bank.
    pub async fn read_encoded_length(&mut self) -> Result<u32, IpcError> {
        let mut length_bytes = [0u8; LENGTH_HEADER_BYTES];
        self.reader.read_exact(&mut length_bytes).await?;
        frame_payload_length(u32::from_le_bytes(length_bytes))
    }

    /// Read the previously validated header's payload after byte admission.
    /// The caller must retain ordered ownership between header and payload;
    /// abandoning either operation requires closing that stream.
    pub async fn read_encoded_payload(&mut self, length: u32) -> Result<EncodedFrame, IpcError> {
        self.read_payload_length_into_buffer(length).await?;
        Ok(EncodedFrame {
            payload: std::mem::take(&mut self.payload),
        })
    }

    async fn read_payload_into_buffer(&mut self) -> Result<(), IpcError> {
        let length = self.read_encoded_length().await?;
        self.read_payload_length_into_buffer(length).await
    }

    async fn read_payload_length_into_buffer(&mut self, length: u32) -> Result<(), IpcError> {
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

        Ok(())
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
    #[tokio::test]
    async fn prepared_encoding_matches_legacy_wire_and_decodes_after_transport_read() {
        let value = vec!["first".to_owned(), "second".to_owned()];
        let prepared = encode_frame(&value).expect("CPU encoding");
        let mut bytes = Vec::new();
        FrameWriter::new(&mut bytes)
            .write_encoded(&prepared)
            .await
            .expect("prepared emission");
        let payload = bincode::serialize(&value).expect("legacy encoding");
        assert_eq!(
            &bytes[..4],
            &frame_length_word(payload.len() as u32).to_le_bytes()
        );
        assert_eq!(&bytes[4..], payload.as_slice());
        let mut reader = FrameReader::new(bytes.as_slice());
        let received = reader.read_encoded().await.expect("ordered transport read");
        assert_eq!(
            decode_frame::<Vec<String>>(&received).expect("CPU decoding"),
            value
        );
    }

    #[tokio::test]
    async fn header_can_complete_before_payload_admission_or_allocation() {
        let value = vec!["ordered".to_owned()];
        let payload = bincode::serialize(&value).unwrap();
        let (mut peer, stream) = tokio::io::duplex(64);
        peer.write_all(&frame_length_word(payload.len() as u32).to_le_bytes())
            .await
            .unwrap();
        let mut reader = FrameReader::new(stream);
        let length = reader.read_encoded_length().await.unwrap();
        assert_eq!(length as usize, payload.len());
        assert_eq!(
            reader.payload.capacity(),
            0,
            "no payload allocation before admission"
        );
        peer.write_all(&payload).await.unwrap();
        let frame = reader.read_encoded_payload(length).await.unwrap();
        assert_eq!(decode_frame::<Vec<String>>(&frame).unwrap(), value);
    }

    #[test]
    fn oversized_preparation_preserves_typed_frame_error() {
        let error = encode_frame(&vec![0_u8; MAX_FRAME_LEN as usize]).unwrap_err();
        assert!(matches!(error, IpcError::FrameTooLarge { actual, max }
            if actual > MAX_FRAME_LEN as usize && max == MAX_FRAME_LEN));
    }

    #[test]
    fn payload_limit_rejects_growth_before_retaining_excess_bytes() {
        use std::io::Write;
        let mut payload = vec![0; MAX_FRAME_LEN as usize];
        let capacity = payload.capacity();
        assert!(LimitedPayload(&mut payload).write_all(&[1]).is_err());
        assert_eq!(payload.len(), MAX_FRAME_LEN as usize);
        assert_eq!(payload.capacity(), capacity);
    }
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
            encoded.extend_from_slice(&frame_length_word(payload.len() as u32).to_le_bytes());
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
        buffer.extend_from_slice(&frame_length_word(100).to_le_bytes());

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
        buffer.extend_from_slice(&frame_length_word(payload.len() as u32).to_le_bytes());
        // Only send half the promised payload bytes.
        buffer.extend_from_slice(&payload[..payload.len() / 2]);

        let mut cursor = Cursor::new(buffer);
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        assert!(matches!(result, Err(IpcError::TruncatedFrame { .. })));
    }

    #[tokio::test]
    async fn read_frame_rejects_an_implausible_length_prefix() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&frame_length_word(MAX_FRAME_LEN + 1).to_le_bytes());

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
        buffer.extend_from_slice(&frame_length_word(garbage.len() as u32).to_le_bytes());
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
        buffer.extend_from_slice(&frame_length_word(payload.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&payload);

        let mut cursor = Cursor::new(buffer);
        let result: Result<String, IpcError> = read_frame(&mut cursor).await;
        assert!(matches!(result, Err(IpcError::Bincode(_))));
    }
    #[tokio::test]
    async fn legacy_schema_is_rejected_before_allocation() {
        let mut reader = FrameReader::new(Cursor::new(100u32.to_le_bytes().to_vec()));
        let result: Result<String, IpcError> = reader.read().await;
        assert!(
            matches!(result, Err(IpcError::Io(ref error)) if error.kind() == std::io::ErrorKind::InvalidData)
        );
        assert!(reader.payload.is_empty());
        assert_eq!(reader.reader.position(), 4);
        assert!(frame_length_word(1) > MAX_FRAME_LEN);
    }
    #[tokio::test]
    async fn recommended_plan_and_tree_snapshot_round_trip() {
        use ilium_core::animation_recommendation::{
            AnimationParameter, AnimationRecommendation, AnimationValue, PlanAnimationEntry,
            RecommendedRestructurePlan, ResourcePolicy,
        };
        use ilium_core::{PaneContentKind, RestructureNode, RestructurePlan, Tree};
        let mut tree = Tree::new();
        let project_id = tree
            .add_project(std::env::temp_dir().join("ilium-semantic-wire"))
            .unwrap();
        let group = tree.add_group(project_id, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let recommendation = AnimationRecommendation {
            version: 1,
            kind: "carpet".into(),
            resources: ResourcePolicy::Catalog,
            parameters: vec![AnimationParameter {
                id: "carpet_mode".into(),
                value: AnimationValue::Choice {
                    index: 1,
                    label: "Autonomous Snake".into(),
                },
            }],
        };
        let plan = RecommendedRestructurePlan {
            structure: RestructurePlan {
                children: vec![RestructureNode::Pane {
                    id: pane,
                    title: "Work".into(),
                    short_title: None,
                    icon: None,
                }],
            },
            expected_animation_generation: 0,
            project: recommendation.clone(),
            entries: vec![PlanAnimationEntry {
                path: vec![0],
                recommendation,
            }],
        };
        let revisions = tree.project_activity_revisions(project_id).unwrap();
        let request = crate::ClientRequest::ApplyRecommendedProjectRestructurePlan {
            project_id,
            plan: plan.clone(),
            inference_activity_revisions: revisions.clone(),
            title_observations: Vec::new(),
        };
        let bytes = bincode::serialize(&request).unwrap();
        let decoded: crate::ClientRequest = frame_decode_options().deserialize(&bytes).unwrap();
        assert_eq!(decoded, request);
        tree.apply_recommended_project_restructure(project_id, plan, &revisions)
            .unwrap();
        let event = crate::ServerEvent::TreeSnapshot(tree.clone());
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &event).await.unwrap();
        let decoded: crate::ServerEvent = read_frame(&mut Cursor::new(buffer)).await.unwrap();
        assert_eq!(decoded, event);
        assert_eq!(tree.project_animation_generation(project_id).unwrap(), 1);
    }
}

#[cfg(test)]
mod prompt_attempt_wire_tests {
    use super::*;
    #[test]
    fn previous_schema_is_rejected_before_decoding_changed_prompt_queue_shape() {
        assert!(frame_payload_length(0xA800_0000 | 16).is_err());
        assert!(frame_payload_length(0xB800_0000 | 16).is_err());
        assert!(frame_payload_length(0xB000_0000 | 16).is_err());
        assert_eq!(frame_payload_length(frame_length_word(16)).unwrap(), 16);
    }
    #[test]
    fn declared_encoded_capacity_covers_real_growth_at_floor_and_doubling_boundaries() {
        for length in [0, 1, 8191, 8192, 8193, 16383, 16384, 16385, 65537] {
            let value = vec![0xA5u8; length];
            let bound = encoded_capacity_bound(&value).expect("sizing");
            let frame = encode_frame(&value).expect("encoding");
            assert!(
                frame.retained_bytes() <= bound,
                "actual payload growth at {length} exceeds preadmission"
            );
            assert_eq!(
                decode_frame::<Vec<u8>>(&frame).expect("unchanged wire"),
                value
            );
        }
    }
}
