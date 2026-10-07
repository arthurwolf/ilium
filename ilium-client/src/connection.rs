//! Owns the client's transport connection to `ilium-server` for
//! one session: sends the initial `Attach`, then drives a reader task
//! (decodes `ServerEvent` frames into a channel `crate::run`'s event loop
//! selects on) and a writer task (encodes queued `ClientRequest`s) each
//! with a tracked `JoinHandle` -- both are aborted together on `Drop`, so
//! a session detach or connection failure can never leave either task
//! running past the connection's own lifetime (see `CLAUDE.md`'s
//! async-task-ownership rule).

use std::path::Path;

use ilium_ipc::{ClientRequest, FrameReader, FrameWriter, IpcError, ServerEvent};
use ilium_transport::SessionEndpoint;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

/// Bounded capacity for both the decoded-`ServerEvent` channel (fed by
/// `read_loop`) and the outbound-`ClientRequest` channel (fed by
/// `crate::run`'s event loop, drained by `write_loop`). A "few hundred"
/// headroom for interactive latency while still giving each producer real
/// backpressure -- rather than an unbounded backlog -- if the consumer
/// ever falls behind. See `crate::run`'s `apply_server_events` for how a
/// backlog of `ScreenUpdate`s specifically gets coalesced rather than
/// relying on capacity alone.
const CHANNEL_CAPACITY: usize = 256;

#[cfg(test)]
#[path = "shutdown_requests_tests.rs"]
mod shutdown_requests_tests;

/// Charged decoded ownership. Map moves the original allocations and keeps
/// storage through consumers; allocating clones needs separate admission.
#[derive(Debug)]
pub struct Received<T> {
    value: T,
    retention: Option<EventRetention>,
}
impl<T> Received<T> {
    pub fn view(&self) -> &T {
        &self.value
    }
    pub fn into_parts(self) -> (T, Option<EventRetention>) {
        (self.value, self.retention)
    }
    pub fn map<U>(self, transform: impl FnOnce(T) -> U) -> Received<U> {
        Received {
            value: transform(self.value),
            retention: self.retention,
        }
    }
    pub fn with_retention(value: T, retention: Option<EventRetention>) -> Self {
        Self { value, retention }
    }
}
impl<T: std::fmt::Display> std::fmt::Display for Received<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}
impl<T> std::ops::Deref for Received<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
#[derive(Clone, Debug)]
pub struct EventRetention {
    allocation: std::sync::Arc<crate::ipc_preparation::DecodedAllocation>,
}
impl EventRetention {
    pub(crate) fn new(
        allocation: std::sync::Arc<crate::ipc_preparation::DecodedAllocation>,
    ) -> Self {
        Self { allocation }
    }
    pub fn retain<T>(self, value: T) -> Received<T> {
        Received {
            value,
            retention: Some(self),
        }
    }
    pub(crate) fn note_projection_result(&self, reason: Option<ilium_execution::RejectReason>) {
        self.allocation.note_projection_result(reason);
    }
    pub fn declared_bytes(&self) -> usize {
        self.allocation.declared_bytes()
    }
    /// Admit a NEW heap allocation before copying; cloning this guard alone
    /// never authorizes a copied payload. Shares the original directional cap.
    pub(crate) fn try_reserve_derived(
        &self,
        bytes: usize,
    ) -> Result<Self, ilium_execution::RejectReason> {
        self.allocation.try_reserve_derived(bytes).map(Self::new)
    }
}

enum RequestCommand {
    Request(Box<crate::ipc_preparation::AdmittedRequest>),
    /// FIFO barrier: every preceding frame completed its actual stream flush.
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct RequestSender {
    sender: mpsc::Sender<RequestCommand>,
    admission: ilium_execution::Client,
}
/// Reserve the FIFO slot before moving any admitted request out of its owner.
pub(crate) struct RequestPublicationPermit<'a> {
    permit: mpsc::Permit<'a, RequestCommand>,
}
impl RequestPublicationPermit<'_> {
    pub(crate) fn publish(self, request: crate::ipc_preparation::AdmittedRequest) {
        self.permit.send(RequestCommand::Request(Box::new(request)));
    }
    pub(crate) fn publish_flush(self) -> RequestFlushReceipt {
        let (sent, received) = oneshot::channel();
        self.permit.send(RequestCommand::Flush(sent));
        RequestFlushReceipt {
            received,
            acknowledged: false,
            failure: None,
        }
    }
}
/// Keep the actual flush receiver across cancellation; never replay its prefix.
pub(crate) struct RequestFlushReceipt {
    received: oneshot::Receiver<()>,
    acknowledged: bool,
    failure: Option<oneshot::error::RecvError>,
}
impl RequestFlushReceipt {
    pub(crate) async fn observe(&mut self) -> Result<(), RequestPublicationClosed> {
        if self.acknowledged {
            return Ok(());
        }
        if self.failure.is_some() {
            return Err(RequestPublicationClosed);
        }
        match (&mut self.received).await {
            Ok(()) => {
                self.acknowledged = true;
                Ok(())
            }
            Err(error) => {
                self.failure = Some(error);
                Err(RequestPublicationClosed)
            }
        }
    }
    pub(crate) fn acknowledged(&self) -> bool {
        self.acknowledged
    }
    pub(crate) fn failure(&self) -> Option<&oneshot::error::RecvError> {
        self.failure.as_ref()
    }
}
impl RequestSender {
    pub(crate) async fn reserve_publication(
        &self,
    ) -> Result<RequestPublicationPermit<'_>, RequestPublicationClosed> {
        self.sender
            .reserve()
            .await
            .map(|permit| RequestPublicationPermit { permit })
            .map_err(|_| RequestPublicationClosed)
    }
    pub async fn send(&self, request: ClientRequest) -> Result<(), Box<RequestSendError>> {
        let mut original = request;
        loop {
            let wake = crate::execution::admission_notification();
            let notified = wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match crate::ipc_preparation::admit_request(&self.admission, original) {
                Ok(request) => {
                    return self.send_admitted(request).await.map_err(|request| {
                        let (request, retention) = request.into_parts();
                        Box::new(RequestSendError {
                            request,
                            reason: RequestSendFailure::WriterClosed,
                            _retention: Some(retention),
                        })
                    })
                }
                Err(error) => {
                    let error = *error;
                    original = error.value;
                    if self.sender.is_closed()
                        || matches!(
                            error.reason,
                            ilium_execution::RejectReason::Closed
                                | ilium_execution::RejectReason::InvalidCost
                                | ilium_execution::RejectReason::AccountingPoisoned
                        )
                        || crate::ipc_preparation::request_retained_bytes(&original)
                            > crate::ipc_preparation::request_limits().input_bytes
                    {
                        return Err(Box::new(RequestSendError {
                            request: original,
                            reason: RequestSendFailure::Admission(error.reason),
                            _retention: None,
                        }));
                    }
                    if error.reason == ilium_execution::RejectReason::Busy {
                        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    } else {
                        notified.await;
                    }
                }
            }
        }
    }

    pub(crate) async fn send_admitted(
        &self,
        request: crate::ipc_preparation::AdmittedRequest,
    ) -> Result<(), crate::ipc_preparation::AdmittedRequest> {
        self.sender
            .send(RequestCommand::Request(Box::new(request)))
            .await
            .map_err(|error| match error.0 {
                RequestCommand::Request(request) => *request,
                RequestCommand::Flush(_) => unreachable!("send constructs only Request"),
            })
    }

    /// Nonblocking public producer admission; clones share this exact tenant.
    pub fn try_send(&self, request: ClientRequest) -> Result<(), Box<RequestSendError>> {
        let request =
            crate::ipc_preparation::admit_request(&self.admission, request).map_err(|error| {
                Box::new(RequestSendError {
                    request: error.value,
                    reason: RequestSendFailure::Admission(error.reason),
                    _retention: None,
                })
            })?;
        self.try_send_admitted(request).map_err(|error| {
            let (request, is_closed) = match *error {
                mpsc::error::TrySendError::Full(request) => (request, false),
                mpsc::error::TrySendError::Closed(request) => (request, true),
            };
            let (request, retention) = request.into_parts();
            Box::new(RequestSendError {
                request,
                reason: if is_closed {
                    RequestSendFailure::WriterClosed
                } else {
                    RequestSendFailure::QueueFull
                },
                _retention: Some(retention),
            })
        })
    }

    pub(crate) fn try_send_admitted(
        &self,
        request: crate::ipc_preparation::AdmittedRequest,
    ) -> Result<(), Box<mpsc::error::TrySendError<crate::ipc_preparation::AdmittedRequest>>> {
        self.sender
            .try_send(RequestCommand::Request(Box::new(request)))
            .map_err(|error| {
                Box::new(match error {
                    mpsc::error::TrySendError::Full(RequestCommand::Request(request)) => {
                        mpsc::error::TrySendError::Full(*request)
                    }
                    mpsc::error::TrySendError::Closed(RequestCommand::Request(request)) => {
                        mpsc::error::TrySendError::Closed(*request)
                    }
                    _ => unreachable!("try_send constructs only Request"),
                })
            })
    }

    /// This acknowledges bytes flushed to the transport, not server handling
    /// or durable application effects. A failed writer closes the receipt.
    pub async fn flush(&self) -> Result<(), IpcError> {
        let (sent, received) = oneshot::channel();
        self.sender
            .send(RequestCommand::Flush(sent))
            .await
            .map_err(|_| {
                IpcError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "request writer stopped before flush barrier",
                ))
            })?;
        received.await.map_err(|_| {
            IpcError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "request writer failed before flushing preceding frames",
            ))
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub struct RequestSendError {
    request: ClientRequest,
    pub reason: RequestSendFailure,
    _retention: Option<ilium_execution::Retention>,
}
impl RequestSendError {
    pub fn request(&self) -> &ClientRequest {
        &self.request
    }
    /// Return original ownership and any admission already acquired before
    /// queue refusal. Keep the guard until the original allocation is freed.
    pub fn into_parts(self) -> (ClientRequest, Option<ilium_execution::Retention>) {
        (self.request, self._retention)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestSendFailure {
    #[error("request admission refused: {0:?}")]
    Admission(ilium_execution::RejectReason),
    #[error("request queue full")]
    QueueFull,
    #[error("request writer closed")]
    WriterClosed,
}

#[derive(Debug, thiserror::Error)]
#[error("server request writer closed before delivery")]
pub(crate) struct RequestPublicationClosed;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("failed to connect to ilium-server socket {path}: {source}")]
    Connect {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("failed to send the initial Attach request: {0}")]
    InitialAttach(IpcError),
    #[error("failed to start the bounded IPC codec: {0}")]
    CodecBootstrap(IpcError),
    #[error("ordered request drain failed: {0}")]
    RequestDrain(#[source] IpcError),
    #[error("ordered request drain exceeded its shutdown deadline")]
    DrainDeadline,
}

/// One live connection to a session's `ilium-server`. `events` yields
/// every decoded `ServerEvent` in arrival order; `requests` is cloned into
/// anything that needs to send a `ClientRequest` (the event-dispatch
/// layer, naming workers via `crate::app::App::take_outbound_requests`).
pub struct Connection {
    pub events: mpsc::Receiver<Received<ServerEvent>>,
    pub requests: RequestSender,
    reader_task: JoinHandle<Result<(), IpcError>>,
    stop_reader: watch::Sender<bool>,
    writer_task: JoinHandle<()>,
    _codec_owner: Option<ilium_execution::Execution>,
}

impl Connection {
    /// Connects to `socket_path` and immediately queues the metadata-only
    /// interactive attach for `session`. The main loop follows with its
    /// current right-panel pane set, allowing the server to replay and stream
    /// only terminals this client can actually display.
    pub async fn connect(socket_path: &Path, session: String) -> Result<Self, ConnectionError> {
        // The public standalone entry point may precede all other process owners.
        // Fixture construction calls the private helper without this designation.
        crate::execution::bootstrap_process_quota()
            .map_err(|error| ConnectionError::CodecBootstrap(IpcError::Io(error)))?;
        let (owner, preparation) = crate::ipc_preparation::IpcPreparation::standalone()
            .map_err(ConnectionError::CodecBootstrap)?;
        Self::connect_prepared(socket_path, session, preparation, Some(owner), None).await
    }

    pub(crate) async fn connect_admitted(
        socket_path: &Path,
        session: String,
        execution: &crate::execution::ClientExecution,
        admission: ilium_execution::Client,
    ) -> Result<Self, ConnectionError> {
        let preparation = execution.ipc_preparation(admission).map_err(|reason| {
            ConnectionError::CodecBootstrap(IpcError::Io(std::io::Error::other(format!(
                "codec admission: {reason:?}"
            ))))
        })?;
        Self::connect_prepared(socket_path, session, preparation, None, None).await
    }

    async fn connect_prepared(
        socket_path: &Path,
        session: String,
        preparation: crate::ipc_preparation::IpcPreparation,
        owner: Option<ilium_execution::Execution>,
        admission: Option<ilium_execution::Client>,
    ) -> Result<Self, ConnectionError> {
        let stream = SessionEndpoint::from_path(socket_path)
            .connect()
            .await
            .map_err(|source| ConnectionError::Connect {
                path: socket_path.to_path_buf(),
                source: std::io::Error::other(source),
            })?;
        let (read_half, write_half) = stream.into_split();

        let (event_tx, event_rx) = mpsc::channel::<Received<ServerEvent>>(CHANNEL_CAPACITY);
        let (request_sender, request_rx) = mpsc::channel::<RequestCommand>(CHANNEL_CAPACITY);
        let request_tx = RequestSender {
            sender: request_sender,
            admission: admission.unwrap_or_else(|| preparation.outbound_client()),
        };

        request_tx
            .send(ClientRequest::AttachInteractive { session })
            .await
            // The receiver can only be closed if `writer_task` already
            // panicked before this line, which can't happen -- it hasn't
            // been spawned yet. A freshly created bounded channel always
            // has spare capacity for its very first item, so this can
            // never actually block waiting for `write_loop` (not spawned
            // yet either) to drain it. Treated as an invariant, not a real
            // error path, but surfaced via the typed error rather than
            // `.unwrap()` per `CLAUDE.md`'s no-panics rule.
            .map_err(|error| {
                ConnectionError::InitialAttach(IpcError::Io(std::io::Error::other(
                    error.to_string(),
                )))
            })?;

        let (stop_reader, read_stop) = watch::channel(false);
        let reader_task = tokio::spawn(read_loop(
            read_half,
            event_tx,
            preparation.clone(),
            read_stop,
        ));
        let writer_task = tokio::spawn(write_loop(write_half, request_rx, preparation));

        Ok(Self {
            events: event_rx,
            requests: request_tx,
            reader_task,
            stop_reader,
            writer_task,
            _codec_owner: owner,
        })
    }
    /// Stop at a frame boundary. An admitted body still finishes decoding and
    /// hands off when storage is available. A blocked storage handoff is instead
    /// cancelled explicitly and reported by finish_read_shutdown; no partial
    /// frame is retried and cancellation never claims delivery.
    pub(crate) fn request_read_shutdown(&self) {
        let _ = self.stop_reader.send(true);
    }

    pub(crate) async fn finish_read_shutdown(&mut self) -> Result<(), IpcError> {
        (&mut self.reader_task)
            .await
            .map_err(|error| IpcError::Io(std::io::Error::other(error)))?
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.reader_task.abort();
        self.writer_task.abort();
    }
}

/// Decodes `ServerEvent` frames off `read_half` until the connection ends
/// (a clean `UnexpectedEof` between frames) or a real protocol error
/// occurs, forwarding each to `event_tx`.
///
/// `event_tx.send` is awaited (not `try_send`'d) deliberately: if the main
/// loop is ever behind, applying backpressure here -- pausing this read
/// loop rather than reading further frames off the socket into an
/// unbounded backlog -- is the correct behavior. This task's only job is
/// forwarding frames, so awaiting here never risks stalling anything else.
/// (The main loop likewise awaits its outbound-request send for lossless
/// backpressure -- see the comment at `crate::run`'s
/// `connection.requests.send` call.)
async fn read_loop<R: tokio::io::AsyncRead + Unpin>(
    read_half: R,
    event_tx: mpsc::Sender<Received<ServerEvent>>,
    preparation: crate::ipc_preparation::IpcPreparation,
    mut stop: watch::Receiver<bool>,
) -> Result<(), IpcError> {
    let mut frame_reader = FrameReader::new(read_half);
    loop {
        if *stop.borrow() {
            break;
        }
        // The fixed-size header can wait without occupying a CPU job or
        // payload bytes. Keep this one reader's header/payload order intact.
        let header = tokio::select! {
            biased;
            _ = stop.changed() => break,
            header = frame_reader.read_encoded_length() => header,
        };
        let length = match header {
            Ok(length) => length,
            Err(IpcError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => {
                tracing::warn!(%error, "connection header failed");
                return Err(error);
            }
        };
        // Admission precedes payload allocation and CPU decoding.
        let reservation = match preparation.reserve_decoder().await {
            Ok(reservation) => reservation,
            Err(error) => {
                tracing::warn!(%error, "connection decoder admission failed");
                return Err(error);
            }
        };
        let event = match frame_reader.read_encoded_payload(length).await {
            Ok(event) => event,
            Err(IpcError::Io(io_error)) if io_error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(IpcError::Io(io_error));
            }
            Err(error) => {
                tracing::warn!("connection read failed, closing: {error}");
                return Err(error);
            }
        };
        let event = match preparation
            .run_reserved(reservation, move |_| {
                let event = ilium_ipc::decode_bounded_frame::<ServerEvent>(&event)?;
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
        {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(%error, "connection CPU decoding failed");
                return Err(error);
            }
        };
        let cancelled = async {
            let read_stopped = async {
                loop {
                    if *stop.borrow() {
                        return "read shutdown requested";
                    }
                    if stop.changed().await.is_err() {
                        return "read shutdown owner closed";
                    }
                }
            };
            tokio::select! {
                biased;
                reason = read_stopped => reason,
                () = event_tx.closed() => "event receiver closed",
            }
        };
        let event = match preparation
            .retain_decoded_cancellable(event, cancelled)
            .await
        {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(%error, "connection decoded storage handoff failed");
                return Err(error);
            }
        };
        if event_tx.send(event).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// Encodes and sends every `ClientRequest` received on `request_rx`, until
/// the sender side is dropped (session shutdown) or a write fails (server
/// gone).
async fn write_loop<W: tokio::io::AsyncWrite + Unpin>(
    write_half: W,
    mut request_rx: mpsc::Receiver<RequestCommand>,
    preparation: crate::ipc_preparation::IpcPreparation,
) {
    let mut frame_writer = FrameWriter::new(write_half);
    while let Some(command) = request_rx.recv().await {
        // Capacity release wakes the UI publisher; it never waits here for
        // a blocked write or requires another keyboard event to retry.
        crate::execution::admission_notification().notify_waiters();
        match command {
            RequestCommand::Request(request) => {
                let (request, request_retention) = (*request).into_parts();
                let result = async {
                    let encoded = preparation.prepare_encoded(request).await?;
                    frame_writer.write_encoded(&encoded.frame).await
                }
                .await;
                drop(request_retention);
                if let Err(error) = result {
                    tracing::warn!("connection write failed, closing: {error}");
                    break;
                }
            }
            RequestCommand::Flush(acknowledgement) => {
                // FrameWriter::write awaits flush before returning. Keeping
                // this control in the SAME FIFO establishes the byte barrier.
                let _ = acknowledgement.send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use tokio::io::AsyncWrite;

    #[derive(Default)]
    struct FlushState {
        bytes: Vec<u8>,
        is_released: bool,
        waiter: Option<Waker>,
    }
    struct ControlledWriter {
        state: Arc<Mutex<FlushState>>,
        entered: Arc<tokio::sync::Notify>,
    }
    impl AsyncWrite for ControlledWriter {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.state
                .lock()
                .expect("state")
                .bytes
                .extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            context: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            let mut state = self.state.lock().expect("state");
            if state.is_released {
                return Poll::Ready(Ok(()));
            }
            state.waiter = Some(context.waker().clone());
            self.entered.notify_one();
            Poll::Pending
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            context: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            self.poll_flush(context)
        }
    }

    struct BodyGateReader {
        bytes: Vec<u8>,
        position: usize,
        gate: Arc<Mutex<FlushState>>,
        entered: Arc<tokio::sync::Notify>,
    }
    impl tokio::io::AsyncRead for BodyGateReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            context: &mut Context<'_>,
            buffer: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.position >= 4 {
                let mut gate = self.gate.lock().expect("body gate");
                if !gate.is_released {
                    gate.waiter = Some(context.waker().clone());
                    self.entered.notify_one();
                    return Poll::Pending;
                }
            }
            let end = if self.position < 4 {
                4
            } else {
                self.bytes.len()
            };
            let count = buffer.remaining().min(end.saturating_sub(self.position));
            buffer.put_slice(&self.bytes[self.position..self.position + count]);
            self.position += count;
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn detach_finishes_an_admitted_frame_body_before_closing_events() {
        let wire = Arc::new(Mutex::new(FlushState {
            is_released: true,
            ..Default::default()
        }));
        FrameWriter::new(ControlledWriter {
            state: wire.clone(),
            entered: Arc::new(tokio::sync::Notify::new()),
        })
        .write(&ServerEvent::DebugLoggingChanged { enabled: true })
        .await
        .expect("wire");
        let gate = Arc::new(Mutex::new(FlushState::default()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let reader = BodyGateReader {
            bytes: wire.lock().expect("wire").bytes.clone(),
            position: 0,
            gate: gate.clone(),
            entered: entered.clone(),
        };
        let (_owner, preparation) = test_codec();
        let (sent, mut received) = mpsc::channel(1);
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(read_loop(reader, sent, preparation, stopped));
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
            .await
            .expect("body admitted");
        stop.send(true).expect("detach");
        assert!(
            !task.is_finished(),
            "detach must not cancel the accepted body"
        );
        let wake = {
            let mut gate = gate.lock().expect("gate");
            gate.is_released = true;
            gate.waiter.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
            .await
            .expect("delivery")
            .expect("event");
        assert!(matches!(
            event.view(),
            ServerEvent::DebugLoggingChanged { enabled: true }
        ));
        task.await.expect("reader join").expect("clean boundary");
        assert!(received.recv().await.is_none());
    }

    fn test_codec() -> (
        ilium_execution::Execution,
        crate::ipc_preparation::IpcPreparation,
    ) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match crate::ipc_preparation::IpcPreparation::standalone() {
                Ok(codec) => return codec,
                Err(IpcError::Io(error))
                    if error
                        .get_ref()
                        .and_then(|source| source.downcast_ref::<ilium_execution::StartError>())
                        .is_some_and(|source| {
                            matches!(
                                source,
                                ilium_execution::StartError::Admission(
                                    ilium_execution::RejectReason::Busy
                                )
                            )
                        })
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("test codec startup: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn request_barrier_cannot_acknowledge_a_frame_with_blocked_stream_flush() {
        let (_codec_owner, preparation) = test_codec();
        let state = Arc::new(Mutex::new(FlushState::default()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let (sent, received) = mpsc::channel(4);
        let writer = tokio::spawn(write_loop(
            ControlledWriter {
                state: state.clone(),
                entered: entered.clone(),
            },
            received,
            preparation.clone(),
        ));
        sent.send(RequestCommand::Request(Box::new(
            crate::ipc_preparation::admit_request(
                &preparation.outbound_client(),
                ClientRequest::UpdateDebugLogging { enabled: true },
            )
            .expect("admission"),
        )))
        .await
        .expect("request");
        let (ack, mut receipt) = oneshot::channel();
        sent.send(RequestCommand::Flush(ack))
            .await
            .expect("barrier");
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
            .await
            .expect("actual flush reached");
        assert!(matches!(
            receipt.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let wake = {
            let mut state = state.lock().expect("state");
            state.is_released = true;
            state.waiter.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), receipt)
            .await
            .expect("bounded receipt")
            .expect("flushed");
        let bytes = state.lock().expect("state").bytes.clone();
        let request: ClientRequest = FrameReader::new(bytes.as_slice())
            .read()
            .await
            .expect("actual framed bytes");
        assert!(matches!(
            request,
            ClientRequest::UpdateDebugLogging { enabled: true }
        ));
        drop(sent);
        writer.await.expect("ordered writer exits");
    }

    #[tokio::test]
    async fn failed_output_closes_barrier_without_reporting_success() {
        let (write_half, peer) = tokio::io::duplex(8);
        drop(peer);
        let (sent, received) = mpsc::channel(4);
        let (_codec_owner, preparation) = test_codec();
        let admission = preparation.outbound_client();
        let writer = tokio::spawn(write_loop(write_half, received, preparation));
        sent.send(RequestCommand::Request(Box::new(
            crate::ipc_preparation::admit_request(
                &admission,
                ClientRequest::UpdateDebugLogging { enabled: false },
            )
            .expect("admission"),
        )))
        .await
        .expect("request");
        let (ack, receipt) = oneshot::channel();
        sent.send(RequestCommand::Flush(ack))
            .await
            .expect("barrier admitted");
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), receipt)
                .await
                .expect("writer closes")
                .is_err()
        );
        writer.await.expect("failure contained");
    }
    #[tokio::test]
    async fn blocked_writer_returns_original_without_stalling_ui_and_keeps_fifo() {
        let (_codec_owner, preparation) = test_codec();
        let state = Arc::new(Mutex::new(FlushState::default()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let (sent, received) = mpsc::channel(2);
        let sender = RequestSender {
            sender: sent,
            admission: preparation.outbound_client(),
        };
        let encoder_observer = preparation.clone();
        let writer = tokio::spawn(write_loop(
            ControlledWriter {
                state: state.clone(),
                entered: entered.clone(),
            },
            received,
            preparation,
        ));
        let request = |text: &str| ClientRequest::SubmitTerminalText {
            pane_id: ilium_core::NodeId(71),
            text: text.into(),
            source: ilium_ipc::PromptSubmissionSource::Keyboard,
        };
        sender
            .send(request("first"))
            .await
            .expect("first admission");
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
            .await
            .expect("actual blocked stream flush");
        assert_eq!(
            encoder_observer.encoder_jobs(),
            0,
            "blocked flush retains independent encoded storage, not finite CPU/input admission"
        );
        sender
            .try_send(request("second"))
            .expect("second admission");
        sender
            .clone()
            .try_send(request("third"))
            .expect("same bounded queue");
        let mut original = String::with_capacity(8192);
        original.push_str("fourth unicode λ\n");
        let pointer = original.as_ptr();
        assert_eq!(sender.admission.usage().jobs, 3);
        let error = sender
            .try_send(ClientRequest::SubmitTerminalText {
                pane_id: ilium_core::NodeId(71),
                text: original,
                source: ilium_ipc::PromptSubmissionSource::Keyboard,
            })
            .expect_err("backpressure preserves original");
        assert_eq!(error.reason, RequestSendFailure::QueueFull);
        assert_eq!(sender.admission.usage().jobs, 4);
        let (refused, refused_retention) = (*error).into_parts();
        let ClientRequest::SubmitTerminalText { text, .. } = &refused else {
            panic!("original type");
        };
        assert_eq!(text.as_ptr(), pointer);
        // A UI task remains independently runnable while stream output is stuck.
        let ui = tokio::spawn(async {
            tokio::task::yield_now().await;
            71
        });
        assert_eq!(ui.await.expect("responsive UI"), 71);
        let wake = {
            let mut state = state.lock().expect("state");
            state.is_released = true;
            state.waiter.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        sender.send(refused).await.expect("retry original");
        drop(refused_retention);
        sender.flush().await.expect("actual flush receipt");
        assert_eq!(sender.admission.usage().jobs, 0);
        let bytes = state.lock().expect("state").bytes.clone();
        let mut reader = FrameReader::new(bytes.as_slice());
        for expected in ["first", "second", "third", "fourth unicode λ\n"] {
            let event: ClientRequest = reader.read().await.expect("ordered actual bytes");
            assert!(
                matches!(event, ClientRequest::SubmitTerminalText { text, .. } if text == expected)
            );
        }
        drop(sender);
        writer.await.expect("writer exit");
    }
    #[tokio::test]
    async fn decoded_storage_follows_channel_and_consumer_after_codec_join() {
        let (mut owner, preparation) = test_codec();
        let mut message = String::with_capacity(8192);
        message.push_str("original decoded allocation");
        let pointer = message.as_ptr();
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("decoder admission");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::Error { message };
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("real CPU completion");
        let received = preparation
            .retain_decoded(decoded)
            .await
            .expect("storage transfer");
        let declared = received
            .retention
            .as_ref()
            .expect("charged result")
            .declared_bytes();
        assert!(declared >= 8192);
        assert_eq!(
            preparation.decoder_usage_jobs(),
            0,
            "finite job released after storage transfer"
        );
        let (sent, mut events) = mpsc::channel(1);
        sent.send(received).await.expect("queued owner");
        let joined = tokio::task::spawn_blocking(move || {
            owner.request_shutdown(ilium_execution::ShutdownMode::Drain);
            owner
                .join_until_background(
                    std::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .expect("physical join")
        })
        .await
        .expect("join helper");
        assert_eq!(
            joined.remaining_workers, 0,
            "all codec threads physically exited"
        );
        assert_eq!(
            preparation.decoded_storage_bytes(),
            declared,
            "queued bytes remain charged after join"
        );
        let received = events.recv().await.expect("consumer owner");
        let received = received.map(|event| match event {
            ServerEvent::Error { message } => message,
            _ => panic!("original type"),
        });
        assert_eq!(
            received.view().as_ptr(),
            pointer,
            "original allocation moved through channel"
        );
        assert_eq!(preparation.decoded_storage_bytes(), declared);
        drop(received);
        assert_eq!(
            preparation.decoded_storage_bytes(),
            0,
            "last actual consumer releases storage"
        );
    }
    #[tokio::test]
    async fn installed_tree_charge_releases_only_after_actual_projection_replacement() {
        let (_owner, preparation) = test_codec();
        let mut app = crate::app::App::new(
            "charged-tree".into(),
            std::path::PathBuf::from("/tmp/charged-tree-fixture"),
        );
        let mut tree = ilium_core::Tree::new();
        let mut name = String::with_capacity(65536);
        name.push_str("original tree root");
        let pointer = name.as_ptr();
        tree.rename_node(ilium_core::ROOT_ID, name, None, None)
            .expect("root rename preserves original String allocation");
        let reservation = preparation.reserve_decoder().await.expect("initial codec");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::TreeSnapshot(tree);
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("initial decode");
        let received = preparation
            .retain_decoded(decoded)
            .await
            .expect("initial storage");
        let (event, retention) = received.into_parts();
        let old_bytes = retention.as_ref().expect("owner").declared_bytes();
        let update = crate::incoming_projection::ProjectionRetention::prepare_owned(
            &event,
            &mut app,
            retention.as_ref(),
        )
        .expect("registry storage before actual consumer");
        let metadata_bytes = preparation.decoded_storage_bytes() - old_bytes;
        assert!(
            metadata_bytes > 0,
            "registry spare capacity has independent credit"
        );
        crate::render_cache::apply(&mut app, event);
        app.incoming_projection.commit(update, retention.as_ref());
        drop(retention);
        assert_eq!(
            app.tree
                .get(ilium_core::ROOT_ID)
                .expect("installed root")
                .name
                .as_ptr(),
            pointer
        );
        assert_eq!(
            preparation.decoded_storage_bytes(),
            old_bytes + metadata_bytes
        );
        assert_eq!(
            preparation.decoder_usage_jobs(),
            0,
            "installed tree cannot block attach marker decoding"
        );
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("replacement codec");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::TreeSnapshot(ilium_core::Tree::new());
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("replacement decode");
        let (event, retention) = preparation
            .retain_decoded(decoded)
            .await
            .expect("replacement storage")
            .into_parts();
        let new_bytes = retention
            .as_ref()
            .expect("replacement owner")
            .declared_bytes();
        let update = crate::incoming_projection::ProjectionRetention::prepare_owned(
            &event,
            &mut app,
            retention.as_ref(),
        )
        .expect("existing registry capacity reuses its owner");
        assert_eq!(
            preparation.decoded_storage_bytes(),
            old_bytes + new_bytes + metadata_bytes
        );
        crate::render_cache::apply(&mut app, event);
        assert_eq!(
            preparation.decoded_storage_bytes(),
            old_bytes + new_bytes + metadata_bytes,
            "old guard survives physical payload replacement boundary"
        );
        app.incoming_projection.commit(update, retention.as_ref());
        drop(retention);
        assert_eq!(
            preparation.decoded_storage_bytes(),
            new_bytes + metadata_bytes
        );
        drop(app);
        assert_eq!(preparation.decoded_storage_bytes(), 0);
    }
    #[tokio::test]
    async fn accepted_dialog_error_owner_survives_stale_same_kind_response() {
        let (_owner, preparation) = test_codec();
        let mut app =
            crate::app::App::new("dialog-owner".into(), "/tmp/dialog-owner-fixture".into());
        app.open_create_agent_workspace_dialog(
            ilium_core::BuiltinAgentProvider::Claude,
            ilium_core::ROOT_ID,
            false,
        );
        let requests = app.take_outbound_requests();
        let (request_id, project) = requests
            .into_iter()
            .find_map(|request| match request {
                ClientRequest::QueryRepoFacts {
                    request_id,
                    project,
                } => Some((request_id, project)),
                _ => None,
            })
            .expect("admitted dialog query");
        let mut message = String::with_capacity(32768);
        message.push_str("original correlated dialog error");
        let pointer = message.as_ptr();
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("codec admission");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::RepoFactsReported {
                    request_id,
                    project,
                    result: Err(message),
                };
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("real codec receipt");
        let (event, retention) = preparation
            .retain_decoded(decoded)
            .await
            .expect("independent storage")
            .into_parts();
        let old_bytes = retention.as_ref().expect("original owner").declared_bytes();
        app.processing_event_retention = retention;
        crate::render_cache::apply(&mut app, event);
        app.processing_event_retention = None;
        match &app.mode {
            crate::app::Mode::CreateAgentWorkspace(state) => match &state.status {
                crate::worktree_dialog::WorktreeDialogStatus::Error(error) => {
                    assert_eq!(error.as_ptr(), pointer)
                }
                _ => panic!("accepted error installed"),
            },
            _ => panic!("original dialog preserved"),
        }
        assert_eq!(preparation.decoded_storage_bytes(), old_bytes);
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("next codec admission");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::RepoFactsReported {
                    request_id: request_id + 1,
                    project,
                    result: Err("stale response".into()),
                };
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("stale codec receipt");
        let (event, retention) = preparation
            .retain_decoded(decoded)
            .await
            .expect("stale storage")
            .into_parts();
        app.processing_event_retention = retention;
        crate::render_cache::apply(&mut app, event);
        app.processing_event_retention = None;
        assert_eq!(
            preparation.decoded_storage_bytes(),
            old_bytes,
            "stale response cannot replace the accepted owner's guard"
        );
        app.mode = crate::app::Mode::Normal;
        assert_eq!(
            preparation.decoded_storage_bytes(),
            0,
            "closing actual dialog releases original storage"
        );
    }

    #[tokio::test]
    async fn derived_heap_copies_need_independent_credit_before_allocation() {
        let (_owner, preparation) = test_codec();
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("codec admission");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::Error {
                    message: "original".into(),
                };
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("codec receipt");
        let received = preparation
            .retain_decoded(decoded)
            .await
            .expect("original storage");
        let owner = received.retention.as_ref().expect("original owner");
        let original_bytes = owner.declared_bytes();
        let independent = owner
            .try_reserve_derived(8192)
            .expect("new heap admission before copy");
        assert_eq!(
            preparation.decoded_storage_bytes(),
            original_bytes + independent.declared_bytes()
        );
        assert!(matches!(
            owner.try_reserve_derived(64 * 1024 * 1024),
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        let copy = String::with_capacity(8192);
        let copy = independent.retain(copy);
        drop(received);
        assert_eq!(
            preparation.decoded_storage_bytes(),
            copy.retention
                .as_ref()
                .expect("copy owner")
                .declared_bytes()
        );
        drop(copy);
        assert_eq!(preparation.decoded_storage_bytes(), 0);
    }
    #[tokio::test]
    async fn refused_projection_metadata_preserves_original_and_changes_no_consumer_state() {
        let (_owner, preparation) = test_codec();
        let mut app = crate::app::App::new(
            "metadata-pressure".into(),
            "/tmp/metadata-pressure-fixture".into(),
        );
        let previous_name = app
            .tree
            .get(ilium_core::ROOT_ID)
            .expect("root")
            .name
            .clone();
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("codec admission");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let mut tree = ilium_core::Tree::new();
                tree.rename_node(ilium_core::ROOT_ID, "retained incoming tree", None, None)
                    .expect("rename");
                let event = ServerEvent::TreeSnapshot(tree);
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("codec receipt");
        let received = preparation
            .retain_decoded(decoded)
            .await
            .expect("original queued owner");
        let original_bytes = received.retention.as_ref().expect("owner").declared_bytes();
        let fill = received
            .retention
            .as_ref()
            .expect("owner")
            .try_reserve_derived(64 * 1024 * 1024 - original_bytes - 512)
            .expect("fill remaining exact directional credit");
        let pointer = match received.view() {
            ServerEvent::TreeSnapshot(tree) => tree
                .get(ilium_core::ROOT_ID)
                .expect("incoming root")
                .name
                .as_ptr(),
            _ => panic!("tree"),
        };
        assert!(matches!(
            crate::incoming_projection::ProjectionRetention::prepare_owned(
                received.view(),
                &mut app,
                received.retention.as_ref()
            ),
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        assert_eq!(
            app.tree.get(ilium_core::ROOT_ID).expect("root").name,
            previous_name,
            "refusal precedes all render/projection mutation"
        );
        assert_eq!(preparation.decoded_storage_bytes(), 64 * 1024 * 1024);
        drop(fill);
        let update = crate::incoming_projection::ProjectionRetention::prepare_owned(
            received.view(),
            &mut app,
            received.retention.as_ref(),
        )
        .expect("retry exact original after real credit release");
        let (event, retention) = received.into_parts();
        crate::render_cache::apply(&mut app, event);
        app.incoming_projection.commit(update, retention.as_ref());
        drop(retention);
        assert_eq!(
            app.tree
                .get(ilium_core::ROOT_ID)
                .expect("installed root")
                .name
                .as_ptr(),
            pointer,
            "retry moved original allocation"
        );
        drop(app);
        assert_eq!(preparation.decoded_storage_bytes(), 0);
    }
    #[tokio::test]
    async fn facts_copy_refusal_keeps_original_draft_and_retries_original_received_owner() {
        let (_owner, preparation) = test_codec();
        let mut app = crate::app::App::new(
            "facts-pressure".into(),
            "/tmp/facts-pressure-fixture".into(),
        );
        app.open_create_agent_workspace_dialog(
            ilium_core::BuiltinAgentProvider::Claude,
            ilium_core::ROOT_ID,
            false,
        );
        let (request_id, project) = app
            .take_outbound_requests()
            .into_iter()
            .find_map(|request| match request {
                ClientRequest::QueryRepoFacts {
                    request_id,
                    project,
                } => Some((request_id, project)),
                _ => None,
            })
            .expect("actual query admission");
        if let crate::app::Mode::CreateAgentWorkspace(state) = &mut app.mode {
            state.branch.buf = "authored/original-λ".into();
            state.branch_is_auto = false;
            state.base_ref.buf = "authored-base".into();
        }
        let reservation = preparation
            .reserve_decoder()
            .await
            .expect("codec admission");
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ServerEvent::RepoFactsReported {
                    request_id,
                    project,
                    result: Ok(ilium_ipc::RepoFacts {
                        repo_common_dir: "/tmp/repository/.git".into(),
                        checkout_root: "/tmp/repository".into(),
                        project_subpath: Default::default(),
                        current_branch: Some("server-base".into()),
                        default_base_ref: "main".into(),
                        default_base_commit: "commit".into(),
                        local_branches: Vec::new(),
                        worktrees: vec![ilium_ipc::WorkspaceWorktreeFact {
                            path: "/tmp/repository".into(),
                            branch: None,
                            created_by_ilium: false,
                            is_dirty: false,
                            occupied_pane_id: None,
                        }],
                        source_dirty_count: 0,
                        main_dirty_count: 0,
                        has_gitmodules: false,
                        git_version: ilium_ipc::WorkspaceGitVersion {
                            major: 2,
                            minor: 45,
                            patch: 0,
                        },
                    }),
                };
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .expect("codec receipt");
        let received = preparation
            .retain_decoded(decoded)
            .await
            .expect("original received owner");
        let original_bytes = received.retention.as_ref().expect("owner").declared_bytes();
        let pressure = received
            .retention
            .as_ref()
            .expect("owner")
            .try_reserve_derived(64 * 1024 * 1024 - original_bytes - 512)
            .expect("fill directional credit");
        assert!(matches!(
            crate::incoming_projection::ProjectionRetention::prepare_owned(
                received.view(),
                &mut app,
                received.retention.as_ref()
            ),
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        let crate::app::Mode::CreateAgentWorkspace(state) = &app.mode else {
            panic!("same dialog")
        };
        assert_eq!(state.branch.buf, "authored/original-λ");
        assert_eq!(state.base_ref.buf, "authored-base");
        assert!(
            state.facts.is_none(),
            "no partial apply before derivation admission"
        );
        drop(pressure);
        let mut update = crate::incoming_projection::ProjectionRetention::prepare_owned(
            received.view(),
            &mut app,
            received.retention.as_ref(),
        )
        .expect("retry same received owner");
        assert!(update.derived_retention.is_some());
        let (event, retention) = received.into_parts();
        app.processing_event_retention = retention;
        app.processing_derivation_retention = update.derived_retention.take();
        crate::render_cache::apply(&mut app, event);
        app.processing_event_retention = None;
        app.processing_derivation_retention = None;
        let crate::app::Mode::CreateAgentWorkspace(state) = &app.mode else {
            panic!("same dialog")
        };
        assert_eq!(
            state.branch.buf, "authored/original-λ",
            "authored branch survives accepted defaulting"
        );
        assert_eq!(state.base_ref.buf, "server-base");
        assert!(
            state.derivation_retention.is_some(),
            "derived TextPrompt copies have their own last owner"
        );
        assert!(preparation.decoded_storage_bytes() > original_bytes);
        app.mode = crate::app::Mode::Normal;
        assert_eq!(preparation.decoded_storage_bytes(), 0);
    }

    #[tokio::test]
    async fn legitimate_replay_pressure_waits_for_consumption_and_keeps_original_bytes() {
        let (_owner, preparation) = test_codec();
        let mut first_bytes = vec![b'a'; 32 * 1024 * 1024 + 2];
        first_bytes[..2].copy_from_slice(b"\x1bc");
        let first_reservation = preparation.reserve_decoder().await.expect("first decoder");
        let first_decoded = preparation
            .run_reserved(first_reservation, move |_| {
                let event = ServerEvent::TerminalReplay {
                    pane_id: ilium_core::NodeId(71),
                    through_sequence: 1,
                    bytes: first_bytes,
                    is_complete: false,
                };
                let retained = event.retained_bytes();
                Ok((event, retained))
            })
            .await
            .expect("first decoded replay");
        let first = preparation
            .retain_decoded(first_decoded)
            .await
            .expect("first replay storage");
        let mut second_bytes = vec![b'b'; 32 * 1024 * 1024 + 2];
        second_bytes[..2].copy_from_slice(b"\x1bc");
        let original_pointer = second_bytes.as_ptr();
        let second_reservation = preparation.reserve_decoder().await.expect("second decoder");
        let second_decoded = preparation
            .run_reserved(second_reservation, move |_| {
                let event = ServerEvent::TerminalReplay {
                    pane_id: ilium_core::NodeId(71),
                    through_sequence: 2,
                    bytes: second_bytes,
                    is_complete: false,
                };
                let retained = event.retained_bytes();
                Ok((event, retained))
            })
            .await
            .expect("second decoded replay");
        let mut waiting = Box::pin(preparation.retain_decoded(second_decoded));
        tokio::select! {
            outcome = &mut waiting => panic!("legitimate replay must wait for retained predecessor consumption, not terminate intake: {outcome:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {},
        }
        assert!(preparation.decoded_storage_bytes() <= 64 * 1024 * 1024);
        drop(first);
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .expect("credit release wakes intake")
            .expect("same replay admitted");
        let ServerEvent::TerminalReplay {
            bytes,
            through_sequence,
            ..
        } = second.view()
        else {
            panic!("original replay type must survive pressure");
        };
        assert_eq!(*through_sequence, 2);
        assert_eq!(
            bytes.as_ptr(),
            original_pointer,
            "pressure must not recopy or discard original allocation"
        );
        assert_eq!(bytes.len(), 32 * 1024 * 1024 + 2);
        assert_eq!(&bytes[..2], b"\x1bc");
        assert!(bytes[2..].iter().all(|byte| *byte == b'b'));
        drop(second);
        assert_eq!(preparation.decoded_storage_bytes(), 0);
        assert_eq!(preparation.decoder_usage_jobs(), 0);
    }

    fn pressure_replay(sequence: u64, length: usize, fill: u8) -> ServerEvent {
        let mut bytes = vec![fill; length];
        if length >= 2 {
            bytes[..2].copy_from_slice(b"\x1bc");
        }
        ServerEvent::TerminalReplay {
            pane_id: ilium_core::NodeId(71),
            through_sequence: sequence,
            bytes,
            is_complete: false,
        }
    }
    async fn pressure_decoded(
        preparation: &crate::ipc_preparation::IpcPreparation,
        event: ServerEvent,
    ) -> ilium_execution::Retained<(ServerEvent, usize)> {
        let reservation = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            preparation.reserve_decoder(),
        )
        .await
        .expect("decoder deadline")
        .expect("decoder admission");
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            preparation.run_reserved(reservation, move |_| {
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            }),
        )
        .await
        .expect("codec deadline")
        .expect("codec result")
    }
    async fn pressure_received(
        preparation: &crate::ipc_preparation::IpcPreparation,
        event: ServerEvent,
    ) -> Received<ServerEvent> {
        let decoded = pressure_decoded(preparation, event).await;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            preparation.retain_decoded(decoded),
        )
        .await
        .expect("storage deadline")
        .expect("storage admission")
    }
    fn pressure_pending<F: std::future::Future>(future: std::pin::Pin<&mut F>) {
        assert!(
            std::future::Future::poll(future, &mut Context::from_waker(Waker::noop())).is_pending(),
            "capacity pressure must preserve the original pending receipt"
        );
    }
    fn pressure_bytes(event: &Received<ServerEvent>, sequence: u64, fill: u8) -> &[u8] {
        let ServerEvent::TerminalReplay {
            pane_id,
            through_sequence,
            bytes,
            is_complete,
        } = event.view()
        else {
            panic!("original replay variant");
        };
        assert_eq!(*pane_id, ilium_core::NodeId(71));
        assert_eq!(*through_sequence, sequence);
        assert!(!*is_complete);
        assert_eq!(&bytes[..2], b"\x1bc");
        assert!(bytes[2..].iter().all(|byte| *byte == fill));
        bytes
    }

    #[tokio::test]
    async fn decoded_pressure_original_credit_and_cancellation_controls() {
        let (_owner, base) = test_codec();
        let capacity = 64 * 1024;
        // Force FIFO handoff and last-clone retention.
        {
            let preparation = base.clone().with_test_decoded_capacity(capacity);
            let first = pressure_received(&preparation, pressure_replay(1, 32770, b'a')).await;
            let first_pointer = pressure_bytes(&first, 1, b'a').as_ptr();
            let owner = first.retention.as_ref().expect("first storage").clone();
            let first_charge = owner.declared_bytes();
            let (sent, mut received) = mpsc::channel(1);
            sent.send(first).await.expect("first queued");
            let second_event = pressure_replay(2, 32770, b'b');
            let second_pointer = match &second_event {
                ServerEvent::TerminalReplay { bytes, .. } => bytes.as_ptr(),
                _ => unreachable!(),
            };
            let second = pressure_decoded(&preparation, second_event).await;
            let mut waiting = Box::pin(preparation.retain_decoded(second));
            pressure_pending(waiting.as_mut());
            assert_eq!(preparation.decoder_usage_jobs(), 1);
            assert_eq!(preparation.decoded_storage_bytes(), first_charge);
            let first = received.recv().await.expect("first remains FIFO head");
            assert_eq!(pressure_bytes(&first, 1, b'a').as_ptr(), first_pointer);
            drop(first);
            pressure_pending(waiting.as_mut());
            assert_eq!(preparation.decoded_storage_bytes(), first_charge);
            drop(owner);
            let second = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
                .await
                .expect("release wake")
                .expect("same successor admitted");
            assert_eq!(pressure_bytes(&second, 2, b'b').as_ptr(), second_pointer);
            assert_eq!(preparation.decoder_usage_jobs(), 0);
            drop(second);
            assert_eq!(preparation.decoded_storage_bytes(), 0);
        }
        // Force release AFTER refusal and BEFORE the notification is first awaited.
        {
            let preparation = base.clone().with_test_decoded_capacity(capacity);
            let first = pressure_received(&preparation, pressure_replay(3, 32770, b'c')).await;
            let event = pressure_replay(4, 32770, b'd');
            let pointer = match &event {
                ServerEvent::TerminalReplay { bytes, .. } => bytes.as_ptr(),
                _ => unreachable!(),
            };
            let decoded = pressure_decoded(&preparation, event).await;
            let first_owner = Arc::new(Mutex::new(Some(first)));
            let weak_owner = Arc::downgrade(&first_owner);
            preparation.set_test_decoded_refusal_hook(move || {
                let owner = weak_owner.upgrade().expect("live fixture consumer");
                let first = owner
                    .lock()
                    .expect("fixture consumer")
                    .take()
                    .expect("release exactly once");
                drop(first);
            });
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                preparation.retain_decoded(decoded),
            )
            .await
            .expect("registered release must not be lost")
            .expect("original admitted");
            assert!(first_owner.lock().expect("fixture consumer").is_none());
            assert_eq!(preparation.decoded_capacity_waits(), 1);
            assert_eq!(pressure_bytes(&result, 4, b'd').as_ptr(), pointer);
            drop(result);
            assert_eq!(preparation.decoded_storage_bytes(), 0);
            assert_eq!(preparation.decoder_usage_jobs(), 0);
        }
        // A ready cancellation must win even when credit is released simultaneously.
        {
            let preparation = base.clone().with_test_decoded_capacity(capacity);
            let first = pressure_received(&preparation, pressure_replay(5, 32770, b'e')).await;
            let decoded = pressure_decoded(&preparation, pressure_replay(6, 32770, b'f')).await;
            let (cancel, cancelled) = oneshot::channel();
            let mut waiting = Box::pin(preparation.retain_decoded_cancellable(decoded, async {
                cancelled.await.expect("cancel sender");
                "forced cancellation"
            }));
            pressure_pending(waiting.as_mut());
            cancel.send(()).expect("signal cancellation");
            drop(first);
            let error = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
                .await
                .expect("cancellation wake")
                .expect_err("cancellation cannot claim delivery");
            assert!(
                matches!(error, IpcError::Io(ref error) if error.kind() == std::io::ErrorKind::Interrupted && error.to_string().contains("without a delivery acknowledgement"))
            );
            assert_eq!(preparation.decoded_storage_bytes(), 0);
            assert_eq!(preparation.decoder_usage_jobs(), 0);
        }
        // Dropping an uncompleted wait must also release finite ownership.
        {
            let preparation = base.clone().with_test_decoded_capacity(capacity);
            let first = pressure_received(&preparation, pressure_replay(7, 32770, b'g')).await;
            let charged = preparation.decoded_storage_bytes();
            let decoded = pressure_decoded(&preparation, pressure_replay(8, 32770, b'h')).await;
            let mut waiting = Box::pin(preparation.retain_decoded(decoded));
            pressure_pending(waiting.as_mut());
            drop(waiting);
            assert_eq!(preparation.decoder_usage_jobs(), 0);
            assert_eq!(preparation.decoded_storage_bytes(), charged);
            let decoded = pressure_decoded(&preparation, pressure_replay(8, 32770, b'h')).await;
            let unpolled = preparation.retain_decoded(decoded);
            drop(unpolled);
            assert_eq!(preparation.decoder_usage_jobs(), 0);
            assert_eq!(preparation.decoded_storage_bytes(), charged);
            pressure_bytes(&first, 7, b'g');
            drop(first);
            assert_eq!(preparation.decoded_storage_bytes(), 0);
        }
        // A new nonterminal derivation can invalidate a previously eligible wait.
        {
            let preparation = base.clone().with_test_decoded_capacity(capacity);
            let first = pressure_received(&preparation, pressure_replay(9, 20 * 1024, b'i')).await;
            let decoded =
                pressure_decoded(&preparation, pressure_replay(10, 50 * 1024, b'j')).await;
            let mut waiting = Box::pin(preparation.retain_decoded(decoded));
            pressure_pending(waiting.as_mut());
            let derived = first
                .retention
                .as_ref()
                .expect("owner")
                .try_reserve_derived(16 * 1024)
                .expect("independent derivation");
            let charged = preparation.decoded_storage_bytes();
            let error = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
                .await
                .expect("floor change wake")
                .expect_err("new floor cannot fit successor");
            assert!(error.to_string().contains("LocalCapacity"));
            assert_eq!(preparation.decoder_usage_jobs(), 0);
            assert_eq!(preparation.decoded_storage_bytes(), charged);
            drop(first);
            drop(derived);
            assert_eq!(preparation.decoded_storage_bytes(), 0);
        }
        // Impossible size and persistent overlap must return rather than await replacement.
        {
            let preparation = base.clone().with_test_decoded_capacity(capacity);
            let decoded = pressure_decoded(&preparation, pressure_replay(11, capacity, b'k')).await;
            let error = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                preparation.retain_decoded(decoded),
            )
            .await
            .expect("impossible item cannot wait")
            .expect_err("overhead exceeds cap");
            assert!(error.to_string().contains("TooLarge"));
            assert_eq!(preparation.decoded_storage_bytes(), 0);
            assert_eq!(preparation.decoder_usage_jobs(), 0);
            let mut tree = ilium_core::Tree::new();
            let mut name = String::with_capacity(40 * 1024);
            name.push_str("persistent original");
            tree.rename_node(ilium_core::ROOT_ID, name, None, None)
                .expect("tree name");
            let original_event = ServerEvent::TreeSnapshot(tree);
            let mut replacement_tree = ilium_core::Tree::new();
            let mut replacement_name = String::with_capacity(32 * 1024);
            replacement_name.push_str("proposed replacement");
            replacement_tree
                .rename_node(ilium_core::ROOT_ID, replacement_name, None, None)
                .expect("replacement name");
            let replacement_event = ServerEvent::TreeSnapshot(replacement_tree);
            let declared = |event: &ServerEvent| {
                event.retained_bytes()
                    + crate::incoming_projection::projection_metadata_bytes(event)
                    + 512
            };
            let overlap_capacity = declared(&original_event) + declared(&replacement_event) - 1;
            let preparation = base.clone().with_test_decoded_capacity(overlap_capacity);
            let original = pressure_received(&preparation, original_event).await;
            let mut app =
                crate::app::App::new("pressure-overlap".into(), "/tmp/pressure-overlap".into());
            let (event, retention) = original.into_parts();
            let update = crate::incoming_projection::ProjectionRetention::prepare_owned(
                &event,
                &mut app,
                retention.as_ref(),
            )
            .expect("original projection metadata");
            crate::render_cache::apply(&mut app, event);
            app.incoming_projection.commit(update, retention.as_ref());
            drop(retention);
            let charged = preparation.decoded_storage_bytes();
            let decoded = pressure_decoded(&preparation, replacement_event).await;
            let error = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                preparation.retain_decoded(decoded),
            )
            .await
            .expect("replacement dependency cannot wait")
            .expect_err("persistent floor refusal");
            assert!(error.to_string().contains("LocalCapacity"));
            assert_eq!(preparation.decoded_storage_bytes(), charged);
            assert_eq!(
                app.tree.get(ilium_core::ROOT_ID).expect("root").name,
                "persistent original"
            );
            assert_eq!(preparation.decoder_usage_jobs(), 0);
            drop(app);
            assert_eq!(preparation.decoded_storage_bytes(), 0);
        }
    }

    #[tokio::test]
    async fn decoded_pressure_fifo_projection_refusal_revokes_an_existing_wait() {
        let (_owner, base) = test_codec();
        let capacity = 64 * 1024;
        let preparation = base.with_test_decoded_capacity(capacity);
        let mut app = crate::app::App::new("pressure-fifo".into(), "/tmp/pressure-fifo".into());
        let previous = app
            .tree
            .get(ilium_core::ROOT_ID)
            .expect("existing root")
            .name
            .clone();
        let mut tree = ilium_core::Tree::new();
        tree.rename_node(ilium_core::ROOT_ID, "pending original tree", None, None)
            .expect("rename");
        let head = pressure_received(&preparation, ServerEvent::TreeSnapshot(tree)).await;
        let head_charge = preparation.decoded_storage_bytes();
        let empty = pressure_replay(1, 0, b'x');
        let overhead = empty.retained_bytes()
            + crate::incoming_projection::projection_metadata_bytes(&empty)
            + 512;
        let length = capacity
            .checked_sub(head_charge + overhead)
            .expect("room for nonvacuous terminal follower");
        assert!(length > 2);
        let tail = pressure_received(&preparation, pressure_replay(1, length, b'x')).await;
        let tail_pointer = pressure_bytes(&tail, 1, b'x').as_ptr();
        assert_eq!(preparation.decoded_storage_bytes(), capacity);
        app.pending_terminal_events.push_back(head);
        let (sent, mut events) = mpsc::channel(CHANNEL_CAPACITY);
        sent.send(tail).await.expect("later terminal queued");
        let decoded = pressure_decoded(&preparation, pressure_replay(2, 1024, b'y')).await;
        let mut waiting = Box::pin(preparation.retain_decoded(decoded));
        pressure_pending(waiting.as_mut());
        assert_eq!(preparation.decoder_usage_jobs(), 1);
        let head = app
            .pending_terminal_events
            .pop_front()
            .expect("original FIFO head");
        let error = crate::incoming_projection::ProjectionRetention::prepare_owned(
            head.view(),
            &mut app,
            head.retention.as_ref(),
        )
        .err()
        .expect("real table admission refuses");
        assert_eq!(error, ilium_execution::RejectReason::WorkerBytes);
        app.pending_terminal_events.push_front(head);
        app.pending_terminal_events
            .push_back(events.try_recv().expect("original following terminal"));
        assert_eq!(preparation.decoded_blocked_projections(), 1);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .expect("projection refusal must wake receive admission")
            .expect_err("blocked FIFO credit cannot justify waiting");
        assert!(error.to_string().contains("blocked_projections: 1"));
        let decoded = pressure_decoded(&preparation, pressure_replay(3, 1024, b'z')).await;
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            preparation.retain_decoded(decoded),
        )
        .await
        .expect("existing barrier cannot wait")
        .expect_err("existing barrier refusal");
        assert!(error.to_string().contains("blocked_projections: 1"));
        assert_eq!(preparation.decoded_capacity_waits(), 1);
        assert_eq!(preparation.decoder_usage_jobs(), 0);
        assert_eq!(preparation.decoded_storage_bytes(), capacity);
        assert_eq!(
            app.tree.get(ilium_core::ROOT_ID).expect("root").name,
            previous
        );
        assert_eq!(app.pending_terminal_events.len(), 2);
        assert!(matches!(
            app.pending_terminal_events.front().expect("head").view(),
            ServerEvent::TreeSnapshot(_)
        ));
        assert_eq!(
            pressure_bytes(app.pending_terminal_events.back().expect("tail"), 1, b'x').as_ptr(),
            tail_pointer
        );
        drop(app);
        assert_eq!(preparation.decoded_storage_bytes(), 0);
        assert_eq!(preparation.decoded_blocked_projections(), 0);
        let mut app = crate::app::App::new("pressure-retry".into(), "/tmp/pressure-retry".into());
        let head = pressure_received(
            &preparation,
            ServerEvent::TreeSnapshot(ilium_core::Tree::new()),
        )
        .await;
        let owner = head.retention.as_ref().expect("retry owner");
        let fill = owner
            .try_reserve_derived(capacity - preparation.decoded_storage_bytes() - 512)
            .expect("independent fill");
        assert!(matches!(
            crate::incoming_projection::ProjectionRetention::prepare_owned(
                head.view(),
                &mut app,
                Some(owner)
            ),
            Err(ilium_execution::RejectReason::WorkerBytes)
        ));
        assert_eq!(preparation.decoded_blocked_projections(), 1);
        drop(fill);
        let update = crate::incoming_projection::ProjectionRetention::prepare_owned(
            head.view(),
            &mut app,
            Some(owner),
        )
        .expect("actual retry");
        assert_eq!(preparation.decoded_blocked_projections(), 0);
        drop(update);
        drop(head);
        drop(app);
        assert_eq!(preparation.decoded_storage_bytes(), 0);
    }

    #[tokio::test]
    async fn decoded_pressure_reader_fifo_shutdown_and_receiver_controls() {
        let (_owner, base) = test_codec();
        for action in 0..5 {
            let preparation = base.clone().with_test_decoded_capacity(64 * 1024);
            let wire = Arc::new(Mutex::new(FlushState {
                is_released: true,
                ..Default::default()
            }));
            let mut writer = FrameWriter::new(ControlledWriter {
                state: wire.clone(),
                entered: Arc::new(tokio::sync::Notify::new()),
            });
            writer
                .write(&pressure_replay(1, 32770, b'a'))
                .await
                .expect("first framed replay");
            writer
                .write(&pressure_replay(2, 32770, b'b'))
                .await
                .expect("second framed replay");
            writer
                .write(&ServerEvent::DebugLoggingChanged { enabled: true })
                .await
                .expect("following control frame");
            drop(writer);
            let bytes = std::mem::take(&mut wire.lock().expect("wire").bytes);
            let (sent, mut received) = mpsc::channel(CHANNEL_CAPACITY);
            let (stop, stopped) = watch::channel(false);
            let mut stop = Some(stop);
            let observer = preparation.clone();
            let mut task = tokio::spawn(async move {
                read_loop(bytes.as_slice(), sent, preparation, stopped).await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while observer.decoded_capacity_waits() == 0 {
                    tokio::select! {
                        result = &mut task => panic!("reader ended before actual pressure: {result:?}"),
                        () = tokio::time::sleep(std::time::Duration::from_millis(1)) => {}
                    }
                }
            }).await.expect("reader must reach local storage pressure");
            assert_eq!(received.len(), 1);
            assert_eq!(observer.decoder_usage_jobs(), 1);
            assert!(observer.decoded_storage_bytes() <= 64 * 1024);
            if action == 0 {
                let first = received.recv().await.expect("first original");
                assert_eq!(pressure_bytes(&first, 1, b'a').len(), 32770);
                drop(first);
                let second =
                    tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
                        .await
                        .expect("second wake")
                        .expect("second original");
                assert_eq!(pressure_bytes(&second, 2, b'b').len(), 32770);
                drop(second);
                let marker =
                    tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
                        .await
                        .expect("marker deadline")
                        .expect("following marker");
                assert!(matches!(
                    marker.view(),
                    ServerEvent::DebugLoggingChanged { enabled: true }
                ));
                drop(marker);
                tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
                    .await
                    .expect("reader deadline")
                    .expect("reader join")
                    .expect("clean EOF");
            } else {
                match action {
                    1 => stop
                        .as_ref()
                        .expect("stop owner")
                        .send(true)
                        .expect("read shutdown"),
                    2 => received.close(),
                    3 => drop(stop.take()),
                    4 => task.abort(),
                    _ => unreachable!(),
                }
                let joined = tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
                    .await
                    .expect("blocked reader must terminate");
                if action == 4 {
                    assert!(joined
                        .expect_err("aborted reader cannot succeed")
                        .is_cancelled());
                } else {
                    let error = joined
                        .expect("reader joined")
                        .expect_err("interrupted handoff cannot claim clean EOF");
                    let expected = match action {
                        1 => "read shutdown requested",
                        2 => "event receiver closed",
                        3 => "read shutdown owner closed",
                        _ => unreachable!(),
                    };
                    assert!(
                        matches!(error, IpcError::Io(ref error) if error.kind() == std::io::ErrorKind::Interrupted && error.to_string().contains(expected))
                    );
                }
                assert_eq!(observer.decoder_usage_jobs(), 0);
                assert!(observer.decoded_storage_bytes() > 0);
                let first = received
                    .recv()
                    .await
                    .expect("accepted predecessor survived shutdown");
                assert_eq!(pressure_bytes(&first, 1, b'a').len(), 32770);
                drop(first);
            }
            assert!(received.recv().await.is_none());
            assert_eq!(observer.decoded_storage_bytes(), 0);
            assert_eq!(observer.decoder_usage_jobs(), 0);
            drop(stop);
        }
    }
}
