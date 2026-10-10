//! Bounded transport frames for forwarding diagnostics to the session owner.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use ilium_execution::StorageAdmission;
use ilium_transport::{SessionEndpoint, SessionStream};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    runtime::Runtime,
    sync::{watch, Semaphore},
    task::{JoinHandle, JoinSet},
};

use crate::{LoggerState, LoggingError, MAX_EVENT_BYTES};

const FRAME_VERSION: u8 = 1;
const EVENT_FRAME: u8 = 1;
const FLUSH_FRAME: u8 = 2;
const FRAME_HEADER_BYTES: usize = 6;
const ACK_ACCEPTED: u8 = 1;
const ACK_REJECTED: u8 = 2;
const ACK_FLUSHED: u8 = 3;
const RELAY_OPERATION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RELAY_CLIENTS: usize = 16;
const RELAY_SERVER_STORAGE_BYTES: usize = 4096;
const RELAY_CONNECTION_STORAGE_BYTES: usize = 4096;
const RELAY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Stable endpoint for the one diagnostics file selected for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRelayEndpoint {
    path: PathBuf,
}

impl LogRelayEndpoint {
    /// Derives an endpoint from the shared log directory, not its rotating
    /// timestamped filename.
    pub fn for_log_path(log_path: &Path) -> Self {
        let directory = log_path
            .parent()
            .filter(|directory| !directory.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Self {
            path: directory.join(".log-relay"),
        }
    }

    /// The identity passed to the platform transport.
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    fn transport_endpoint(&self) -> SessionEndpoint {
        SessionEndpoint::from_path(self.path.clone())
    }
}

/// Async listener owned by the detached server. Every client event enters the
/// server's existing ordered file-writer queue before the client is acked.
pub struct LogRelayServer {
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<(), LoggingError>>,
    _storage: StorageAdmission,
}

impl LogRelayServer {
    pub(crate) async fn start(
        state: Arc<LoggerState>,
        endpoint: LogRelayEndpoint,
    ) -> Result<Self, LoggingError> {
        if state.process_role != "server" || state.destination != crate::LogDestination::LocalFile {
            return Err(LoggingError::Relay(
                "only the detached server may own the shared log endpoint".to_owned(),
            ));
        }
        let storage = state
            .quota
            .reserve_external_storage(RELAY_SERVER_STORAGE_BYTES)
            .map_err(|reason| {
                LoggingError::Relay(format!("listener storage refused: {reason:?}"))
            })?;
        let listener = endpoint
            .transport_endpoint()
            .bind()
            .await
            .map_err(|error| LoggingError::Relay(error.to_string()))?;
        let (stop, receiver) = watch::channel(false);
        let task = tokio::spawn(serve(listener, state, receiver));
        Ok(Self {
            stop,
            task,
            _storage: storage,
        })
    }

    /// Stops accepting clients, drains their accepted writes, then flushes the
    /// central file owner before returning.
    pub async fn shutdown(mut self) -> Result<(), LoggingError> {
        let _ = self.stop.send(true);
        let outer_timeout = RELAY_SHUTDOWN_TIMEOUT + Duration::from_secs(1);
        match tokio::time::timeout(outer_timeout, &mut self.task).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(LoggingError::Relay(format!(
                "listener task failed: {error}"
            ))),
            Err(_) => {
                self.task.abort();
                let _ = self.task.await;
                Err(LoggingError::Deadline)
            }
        }
    }
}

struct RelayConnectionAdmission {
    _storage: StorageAdmission,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

async fn serve(
    mut listener: ilium_transport::SessionListener,
    state: Arc<LoggerState>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), LoggingError> {
    let clients = Arc::new(Semaphore::new(MAX_RELAY_CLIENTS));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
            completed = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(error)) = completed {
                    state.service.failed();
                    return Err(LoggingError::Relay(format!("client task failed: {error}")));
                }
            }
            accepted = listener.accept() => {
                let stream = accepted.map_err(|error| LoggingError::Relay(error.to_string()))?;
                let permit = match Arc::clone(&clients).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        state.service.dropped();
                        drop(stream);
                        continue;
                    }
                };
                let storage = match state.quota.reserve_external_storage(RELAY_CONNECTION_STORAGE_BYTES) {
                    Ok(storage) => storage,
                    Err(_) => {
                        state.service.dropped();
                        drop(stream);
                        continue;
                    }
                };
                let admission = RelayConnectionAdmission { _storage: storage, _permit: permit };
                tasks.spawn(serve_client(stream, Arc::clone(&state), admission));
            }
        }
    }

    let deadline = tokio::time::Instant::now() + RELAY_SHUTDOWN_TIMEOUT;
    while !tasks.is_empty() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            return Err(LoggingError::Deadline);
        }
        match tokio::time::timeout(remaining, tasks.join_next()).await {
            Ok(Some(Ok(Ok(())))) | Ok(Some(Ok(Err(_)))) => {}
            Ok(Some(Err(error))) => {
                state.service.failed();
                return Err(LoggingError::Relay(format!("client task failed: {error}")));
            }
            Ok(None) => break,
            Err(_) => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Err(LoggingError::Deadline);
            }
        }
    }
    state
        .service
        .flush()
        .map_err(|error| LoggingError::Relay(error.to_string()))?
        .await?;
    Ok(())
}

async fn serve_client(
    mut stream: SessionStream,
    state: Arc<LoggerState>,
    _admission: RelayConnectionAdmission,
) -> Result<(), LoggingError> {
    loop {
        let header = match read_header(&mut stream).await {
            Ok(Some(header)) => header,
            Ok(None) => return Ok(()),
            Err(error) => {
                state.service.failed();
                return Err(LoggingError::Relay(error.to_string()));
            }
        };
        match header.kind {
            EVENT_FRAME => {
                if !(1..=MAX_EVENT_BYTES).contains(&header.length) {
                    write_ack(&mut stream, ACK_REJECTED)
                        .await
                        .map_err(|error| LoggingError::Relay(error.to_string()))?;
                    return Ok(());
                }
                let Some(mut event) = AdmittedRelayEvent::allocate(&state, header.length) else {
                    write_ack(&mut stream, ACK_REJECTED)
                        .await
                        .map_err(|error| LoggingError::Relay(error.to_string()))?;
                    return Ok(());
                };
                if let Err(error) = stream.read_exact(&mut event.bytes).await {
                    state.service.failed();
                    return Err(LoggingError::Relay(error.to_string()));
                }
                let accepted = state.service.event(event.into_bytes());
                write_ack(
                    &mut stream,
                    if accepted { ACK_ACCEPTED } else { ACK_REJECTED },
                )
                .await
                .map_err(|error| LoggingError::Relay(error.to_string()))?;
                if !accepted {
                    return Ok(());
                }
            }
            FLUSH_FRAME if header.length == 0 => {
                let result = state
                    .service
                    .flush()
                    .map_err(|error| LoggingError::Relay(error.to_string()))?
                    .await;
                match result {
                    Ok(()) => write_ack(&mut stream, ACK_FLUSHED).await,
                    Err(error) => {
                        state.service.failed();
                        write_ack(&mut stream, ACK_REJECTED)
                            .await
                            .map_err(|error| LoggingError::Relay(error.to_string()))?;
                        return Err(LoggingError::Relay(error.to_string()));
                    }
                }
                .map_err(|error| LoggingError::Relay(error.to_string()))?;
            }
            _ => {
                state.service.failed();
                return Err(LoggingError::Relay("invalid client frame".to_owned()));
            }
        }
    }
}

struct AdmittedRelayEvent {
    bytes: Vec<u8>,
    state: Arc<LoggerState>,
    charged_bytes: usize,
}

impl AdmittedRelayEvent {
    fn allocate(state: &Arc<LoggerState>, length: usize) -> Option<Self> {
        if !state.service.reserve(length) {
            state.service.dropped();
            return None;
        }
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(length).is_err() {
            state.service.release(length);
            state.service.dropped();
            return None;
        }
        let capacity = bytes.capacity();
        if capacity > length && !state.service.reserve(capacity - length) {
            state.service.release(length);
            state.service.dropped();
            return None;
        }
        bytes.resize(length, 0);
        Some(Self {
            bytes,
            state: Arc::clone(state),
            charged_bytes: capacity,
        })
    }

    fn into_bytes(mut self) -> Vec<u8> {
        self.charged_bytes = 0;
        std::mem::take(&mut self.bytes)
    }
}

impl Drop for AdmittedRelayEvent {
    fn drop(&mut self) {
        if self.charged_bytes != 0 {
            self.state.service.release(self.charged_bytes);
        }
    }
}

struct RelayHeader {
    kind: u8,
    length: usize,
}

async fn read_header<R>(stream: &mut R) -> io::Result<Option<RelayHeader>>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0; FRAME_HEADER_BYTES];
    if stream.read(&mut header[..1]).await? == 0 {
        return Ok(None);
    }
    stream.read_exact(&mut header[1..]).await?;
    if header[0] != FRAME_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported log relay frame version",
        ));
    }
    let length = u32::from_le_bytes(header[2..6].try_into().expect("fixed header")) as usize;
    Ok(Some(RelayHeader {
        kind: header[1],
        length,
    }))
}

/// A synchronous adapter used only by the logger's existing I/O worker.
/// Each event is acknowledged for bounded admission; `flush` is an ordered
/// durability barrier at the central file owner.
pub(crate) struct RelayWriter {
    endpoint: LogRelayEndpoint,
    runtime: Runtime,
    stream: Option<SessionStream>,
}

impl RelayWriter {
    pub(crate) fn new(endpoint: LogRelayEndpoint) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .thread_name("ilium-log-relay")
            .build()?;
        Ok(Self {
            endpoint,
            runtime,
            stream: None,
        })
    }

    fn with_stream<T>(
        &mut self,
        operation: impl for<'a> FnOnce(&'a mut SessionStream) -> RelayFuture<'a, T>,
    ) -> io::Result<T> {
        let endpoint = self.endpoint.transport_endpoint();
        let stream = &mut self.stream;
        let result = self.runtime.block_on(async move {
            tokio::time::timeout(RELAY_OPERATION_TIMEOUT, async {
                if stream.is_none() {
                    *stream = Some(endpoint.connect().await.map_err(transport_error)?);
                }
                operation(stream.as_mut().expect("connected stream")).await
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "log relay operation timed out"))?
        });
        if result.is_err() {
            self.stream = None;
        }
        result
    }
}

type RelayFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<T>> + 'a>>;

impl Write for RelayWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log relay event exceeds the frame limit",
            ));
        }
        // The higher-ranked stream callback cannot borrow the caller's slice.
        // Move bounded event bytes into the future so they live through its await.
        let event_bytes = bytes.to_vec();
        let ack = self.with_stream(move |stream| {
            Box::pin(async move { write_event(stream, &event_bytes).await })
        })?;
        if ack != ACK_ACCEPTED {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "log relay refused event admission",
            ));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let ack = self.with_stream(|stream| Box::pin(write_flush(stream)))?;
        if ack == ACK_FLUSHED {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "log relay did not acknowledge the flush barrier",
            ))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum RelayFrame {
    Event(Vec<u8>),
    Flush,
}

pub(crate) async fn write_event<W>(stream: &mut W, bytes: &[u8]) -> io::Result<u8>
where
    W: AsyncRead + AsyncWrite + Unpin,
{
    if bytes.is_empty() || bytes.len() > MAX_EVENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid log relay event length",
        ));
    }
    write_header(stream, EVENT_FRAME, bytes.len()).await?;
    stream.write_all(bytes).await?;
    stream.flush().await?;
    read_ack(stream).await
}

pub(crate) async fn write_flush<W>(stream: &mut W) -> io::Result<u8>
where
    W: AsyncRead + AsyncWrite + Unpin,
{
    write_header(stream, FLUSH_FRAME, 0).await?;
    stream.flush().await?;
    read_ack(stream).await
}

#[cfg(test)]
pub(crate) async fn read_frame<R>(stream: &mut R) -> io::Result<RelayFrame>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0; FRAME_HEADER_BYTES];
    stream.read_exact(&mut header).await?;
    if header[0] != FRAME_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported log relay frame version",
        ));
    }
    let length = u32::from_le_bytes(header[2..6].try_into().expect("fixed header")) as usize;
    match header[1] {
        EVENT_FRAME if (1..=MAX_EVENT_BYTES).contains(&length) => {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(length).map_err(io::Error::other)?;
            bytes.resize(length, 0);
            stream.read_exact(&mut bytes).await?;
            Ok(RelayFrame::Event(bytes))
        }
        FLUSH_FRAME if length == 0 => Ok(RelayFrame::Flush),
        EVENT_FRAME => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "log relay event exceeds the frame limit",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown log relay frame kind",
        )),
    }
}

pub(crate) async fn write_ack<W>(stream: &mut W, ack: u8) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    stream.write_all(&[ack]).await?;
    stream.flush().await
}

async fn write_header<W>(stream: &mut W, kind: u8, length: usize) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let length = u32::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "log frame is too large"))?;
    let mut header = [0; FRAME_HEADER_BYTES];
    header[0] = FRAME_VERSION;
    header[1] = kind;
    header[2..6].copy_from_slice(&length.to_le_bytes());
    stream.write_all(&header).await
}

async fn read_ack<R>(stream: &mut R) -> io::Result<u8>
where
    R: AsyncRead + Unpin,
{
    let mut ack = [0];
    stream.read_exact(&mut ack).await?;
    Ok(ack[0])
}

fn transport_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[test]
    fn relay_endpoint_is_stable_across_rotating_log_file_names() {
        let first = LogRelayEndpoint::for_log_path(Path::new(
            "/tmp/.ilium/project/default/log-2026-10-08_04-00-00.000.txt",
        ));
        let next = LogRelayEndpoint::for_log_path(Path::new(
            "/tmp/.ilium/project/default/log-2026-10-08_05-00-00.000.txt",
        ));

        assert_eq!(first, next);
        assert_eq!(
            first.as_path(),
            Path::new("/tmp/.ilium/project/default/.log-relay")
        );
    }

    #[tokio::test]
    async fn typed_event_and_flush_frames_round_trip_in_order() {
        let (mut sender, mut receiver) = duplex(1024);
        let task = tokio::spawn(async move {
            assert_eq!(
                read_frame(&mut receiver).await.expect("event frame"),
                RelayFrame::Event(b"first\n".to_vec())
            );
            write_ack(&mut receiver, ACK_ACCEPTED)
                .await
                .expect("event acknowledgement");
            assert_eq!(
                read_frame(&mut receiver).await.expect("flush frame"),
                RelayFrame::Flush
            );
            write_ack(&mut receiver, ACK_FLUSHED)
                .await
                .expect("flush acknowledgement");
        });

        assert_eq!(
            write_event(&mut sender, b"first\n")
                .await
                .expect("event ack"),
            ACK_ACCEPTED
        );
        assert_eq!(
            write_flush(&mut sender).await.expect("flush ack"),
            ACK_FLUSHED
        );
        task.await.expect("frame reader");
    }

    #[tokio::test]
    async fn oversized_event_header_is_rejected_before_payload_allocation() {
        let (mut sender, mut receiver) = duplex(16);
        let expected = tokio::spawn(async move {
            let error = read_frame(&mut receiver)
                .await
                .expect_err("oversized event must be refused");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        });
        write_header(&mut sender, EVENT_FRAME, MAX_EVENT_BYTES + 1)
            .await
            .expect("write header only");
        expected.await.expect("reader");
    }

    #[tokio::test]
    async fn real_endpoint_acknowledges_central_admission_and_flush_readback() {
        let directory = tempfile::tempdir().expect("log directory");
        let path = directory.path().join("session.log");
        let mut state = LoggerState::new(path.clone()).expect("logger service");
        state.process_role = "server";
        let state = Arc::new(state);
        state
            .service
            .enable(true)
            .expect("enable central file writer")
            .await
            .expect("file writer opened");

        let endpoint = LogRelayEndpoint::for_log_path(&path);
        let relay = LogRelayServer::start(Arc::clone(&state), endpoint.clone())
            .await
            .expect("bind real session endpoint");
        let mut writer = RelayWriter::new(endpoint).expect("client writer");
        tokio::task::spawn_blocking(move || {
            writer.write_all(b"first client event\n")?;
            writer.flush()
        })
        .await
        .expect("client writer task")
        .expect("event and flush acknowledgement");

        relay
            .shutdown()
            .await
            .expect("drain and flush server relay");
        let persisted = std::fs::read(&path).expect("central log readback");
        assert!(persisted
            .windows(b"first client event\n".len())
            .any(|window| window == b"first client event\n"));
        state
            .service
            .shutdown()
            .expect("shutdown logger")
            .wait_timeout(Duration::from_secs(5))
            .expect("logger thread joined");
    }
}
