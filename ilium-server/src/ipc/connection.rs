//! One attached client connection: reads `ClientRequest` frames, dispatches
//! them via [`crate::ipc::handlers`], and writes back both this
//! connection's own direct replies and every broadcast `ServerEvent` the
//! session produces while attached.
//!
//! Structured as two concurrently-polled loops (reader, writer) inside a
//! *single* spawned task, not two separately-spawned tasks -- `tokio::join!`
//! polls both as plain futures without an extra `tokio::spawn` each, so
//! the one `JoinHandle` the caller tracks (`ServerState::track_connection_task`)
//! cancels both at once. Splitting into two independently-spawned tasks
//! would mean two handles to track and cancel together for what is really
//! one logical connection.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ilium_ipc::{ClientRequest, FrameReader, FrameWriter, ServerEvent};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, watch};

use crate::ipc::handlers;
use crate::state::ServerState;

/// Bound on this connection's own direct-reply queue (`Attach` snapshots,
/// per-request errors, resize/key/mouse acks). A slow or stalled client
/// still gets backpressured -- `handlers::send_direct` awaits capacity --
/// rather than letting the queue grow without bound while `write_replies`
/// is stuck on a blocked socket write.
const DIRECT_CHANNEL_CAPACITY: usize = 64;

/// Bound on low-frequency right-panel subscription transitions. The client
/// coalesces repeated values before sending; this queue exists only to carry
/// focus/split changes from the request reader to the connection writer that
/// owns terminal delivery watermarks.
const STREAM_CONTROL_CHANNEL_CAPACITY: usize = 8;

/// Upper bound on how many panes one interactive client may subscribe to at
/// once. The TUI's right panel shows at most a 2x2 grid, so anything past the
/// first four requested panes is stream demand no client can actually render.
/// Both the reader-side demand registration and the writer-side selection must
/// truncate identically, or the session-wide demand index and this
/// connection's delivery filter would disagree about which panes stream.
const MAX_VISIBLE_TERMINAL_SUBSCRIPTIONS: usize = 4;

enum StreamControl {
    /// A compatibility attach or diagnostic consumer needs every terminal's
    /// live output after its complete replay has established parser state.
    StreamAllTerminals,
    /// An interactive attach starts metadata-only. Until the client names its
    /// right-panel panes, no raw terminal bytes may cross this connection.
    StreamNoTerminals,
    SetVisiblePanes(Vec<ilium_core::NodeId>),
}

/// Returns whether request intake must wait until the connection writer has
/// applied this control change. Attach controls establish the replay cutover,
/// whereas a subsequent visible-pane selection only changes output demand and
/// must never delay PTY input behind that output.
fn stream_control_requires_request_barrier(control: &StreamControl) -> bool {
    !matches!(control, StreamControl::SetVisiblePanes(_))
}

struct StreamControlCommand {
    control: StreamControl,
    applied: oneshot::Sender<()>,
}

enum TerminalStreamSelection {
    /// The safe initial state for a freshly accepted connection. This avoids
    /// doing client-side parsing work for every pane during the interactive
    /// attach handshake, before the client has reported its viewport.
    None,
    All,
    Visible(HashSet<ilium_core::NodeId>),
}

impl TerminalStreamSelection {
    fn includes(&self, pane_id: ilium_core::NodeId) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Visible(pane_ids) => pane_ids.contains(&pane_id),
        }
    }
}

/// RAII contribution of one connection to the session-wide raw-terminal
/// demand index. Socket errors, detach, and task cancellation all drop this
/// guard and therefore cannot leave a pane permanently marked as visible.
struct TerminalSubscriptionGuard {
    state: Arc<ServerState>,
    is_all_panes: bool,
    pane_ids: HashSet<ilium_core::NodeId>,
}

impl TerminalSubscriptionGuard {
    fn new(state: Arc<ServerState>) -> Self {
        Self {
            state,
            is_all_panes: false,
            pane_ids: HashSet::new(),
        }
    }

    fn set_all(&mut self) {
        self.replace(true, HashSet::new());
    }

    fn set_visible(&mut self, pane_ids: HashSet<ilium_core::NodeId>) {
        self.replace(false, pane_ids);
    }

    fn replace(&mut self, is_all_panes: bool, pane_ids: HashSet<ilium_core::NodeId>) {
        self.state.replace_terminal_subscriptions(
            self.is_all_panes,
            &self.pane_ids,
            is_all_panes,
            &pane_ids,
        );
        self.is_all_panes = is_all_panes;
        self.pane_ids = pane_ids;
    }
}

impl Drop for TerminalSubscriptionGuard {
    fn drop(&mut self) {
        self.state.replace_terminal_subscriptions(
            self.is_all_panes,
            &self.pane_ids,
            false,
            &HashSet::new(),
        );
    }
}

/// Per-connection replay phase shared by the request reader and event writer.
/// Connections may issue lifecycle commands before attaching, so only the
/// interval in which an Attach replay is actively being assembled is gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachPhase {
    Open,
    Replaying,
    Ready,
}

/// Drives one accepted connection to completion: concurrently reads
/// requests (dispatching each one) and writes replies/broadcasts, until
/// either side signals the connection is done (client disconnected, a
/// `Detach`/`KillSession` request was handled, or the write side's stream
/// closed).
pub async fn handle<S>(state: Arc<ServerState>, stream: S)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read_half, write_half) = tokio::io::split(stream);
    let (direct_tx, direct_rx) = mpsc::channel::<ServerEvent>(DIRECT_CHANNEL_CAPACITY);
    let (stream_control_tx, stream_control_rx) = mpsc::channel(STREAM_CONTROL_CHANNEL_CAPACITY);
    let broadcast_rx = state.events.subscribe();
    // A connection subscribes to broadcasts before its Attach request is
    // handled so it cannot miss output produced during the handshake. The
    // writer must nevertheless hold those broadcasts until the complete
    // attach replay is queued: otherwise a busy pane can send sequence N+1,
    // then the client's later replay through N resets its parser and erases
    // N+1 permanently. A watch channel lets the writer keep draining the
    // bounded direct queue while the attach handler fills it, without ever
    // admitting a broadcast across that cutover boundary.
    let (attach_phase_tx, attach_phase_rx) = watch::channel(AttachPhase::Open);

    let reader = read_requests(
        Arc::clone(&state),
        read_half,
        direct_tx,
        attach_phase_tx,
        stream_control_tx,
    );
    let writer = write_replies(
        write_half,
        broadcast_rx,
        direct_rx,
        attach_phase_rx,
        stream_control_rx,
        Some(Arc::clone(&state)),
    );
    tokio::join!(reader, writer);
}

/// Reads and dispatches `ClientRequest` frames until the stream ends, a
/// frame fails to decode, or a request signals the connection should
/// close. A per-request handling failure is reported back to this
/// connection alone (via `direct_tx`, inside `handlers::handle_request`)
/// and never stops the loop -- one malformed or rejected request must not
/// end the whole connection, only the request that caused it.
async fn read_requests<R>(
    state: Arc<ServerState>,
    read_half: R,
    direct_tx: mpsc::Sender<ServerEvent>,
    attach_phase_tx: watch::Sender<AttachPhase>,
    stream_control_tx: mpsc::Sender<StreamControlCommand>,
) where
    R: AsyncRead + Unpin,
{
    let mut frame_reader = FrameReader::new(read_half);
    let preparation = match codec_client(Some(&state), true) {
        Ok(preparation) => preparation,
        Err(error) => {
            tracing::warn!(%error, "server decoder bootstrap failed");
            return;
        }
    };
    let mut terminal_subscription_guard = TerminalSubscriptionGuard::new(Arc::clone(&state));
    let mut has_terminal_stream_selection = false;
    loop {
        let decoded = tokio::select! {
            biased;
            // `write_replies` drops `direct_rx` on every exit path, not only
            // the ones already covered by this loop's own EOF/decode-error
            // breaks -- a failed socket write, a lost broadcast sender, or
            // server shutdown all end it while this reader could otherwise
            // stay parked on `frame_reader.read()` indefinitely (the peer's
            // read half can outlive our write failure). Once the writer is
            // gone, every reply this loop would produce is silently dropped
            // by `handlers::send_direct` anyway, so continuing to read only
            // leaks this task's socket half and this connection's
            // `terminal_subscription_guard` contribution until the peer
            // eventually disconnects or the whole server shuts down.
            () = direct_tx.closed() => break,
            read_result = decode_client_request(&mut frame_reader, &preparation) => match read_result {
                Ok(request) => request,
                Err(ilium_ipc::IpcError::Io(io_error))
                    if io_error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // The peer closed the connection between frames -- the
                    // normal way a client disconnects, not an error.
                    break;
                }
                Err(error) => {
                    tracing::warn!("connection closed after a frame read/decode error: {error}");
                    break;
                }
            },
        };

        let request = decoded.view();
        let request_name = request.diagnostic_name();
        if request.is_high_frequency_diagnostic() {
            tracing::debug!(request_name, "client request received");
        } else {
            tracing::info!(request_name, "client request received");
        }
        if let ClientRequest::KeyInput {
            pane_id,
            bytes,
            submission: Some(submission),
        }
        | ClientRequest::UserKeyInput {
            pane_id,
            bytes,
            submission: Some(submission),
            ..
        } = &request
        {
            tracing::info!(
                request_name,
                ?pane_id,
                ?submission,
                byte_count = bytes.len(),
                "terminal submission received"
            );
        }
        if let ClientRequest::SubmitTerminalText {
            pane_id,
            text,
            source,
        } = &request
        {
            tracing::info!(
                request_name,
                ?pane_id,
                ?source,
                text_bytes = text.len(),
                "terminal text submission received"
            );
        }

        let stream_control = match &request {
            ClientRequest::SetVisiblePanes { pane_ids } => {
                terminal_subscription_guard.set_visible(
                    pane_ids
                        .iter()
                        .copied()
                        .take(MAX_VISIBLE_TERMINAL_SUBSCRIPTIONS)
                        .collect(),
                );
                has_terminal_stream_selection = true;
                Some(StreamControl::SetVisiblePanes(pane_ids.clone()))
            }
            ClientRequest::Attach { .. } => {
                terminal_subscription_guard.set_all();
                has_terminal_stream_selection = true;
                Some(StreamControl::StreamAllTerminals)
            }
            ClientRequest::AttachInteractive { .. } => {
                terminal_subscription_guard.set_visible(HashSet::new());
                has_terminal_stream_selection = true;
                Some(StreamControl::StreamNoTerminals)
            }
            _ if !has_terminal_stream_selection => {
                // Lifecycle clients historically issue commands before an
                // Attach. Preserve their all-terminal stream contract, while
                // the TUI's first AttachInteractive request opts out before
                // it can create or write a pane.
                terminal_subscription_guard.set_all();
                has_terminal_stream_selection = true;
                Some(StreamControl::StreamAllTerminals)
            }
            _ => None,
        };
        if let Some(stream_control) = stream_control {
            // A visible-pane change can require replaying the whole retained
            // terminal journal.  The writer owns that ordered replay, but a
            // following KeyInput must reach the PTY while the user's terminal
            // is still draining it; waiting for `applied_rx` here used to
            // make keyboard latency proportional to replay size.
            let requires_request_barrier = stream_control_requires_request_barrier(&stream_control);
            let (applied_tx, applied_rx) = oneshot::channel();
            if stream_control_tx
                .send(StreamControlCommand {
                    control: stream_control,
                    applied: applied_tx,
                })
                .await
                .is_err()
            {
                break;
            }
            if !requires_request_barrier {
                // The writer still applies selections in FIFO order and
                // repairs their journals before live bytes resume.  Only the
                // request reader proceeds independently, so keyboard input
                // is not a hostage to output bandwidth.
                continue;
            }
            if applied_rx.await.is_err() {
                break;
            }
        }

        let completes_attach = matches!(
            &request,
            ClientRequest::Attach { .. } | ClientRequest::AttachInteractive { .. }
        );
        if completes_attach {
            // Switch phases before the handler awaits or snapshots replay so
            // output produced after the cutover cannot pass the direct batch.
            attach_phase_tx.send_replace(AttachPhase::Replaying);
        }
        let should_close = decoded
            .map_async(|request| async {
                handlers::handle_request(&state, request, &direct_tx).await
            })
            .await;
        let should_close = *should_close.view();
        // `handle_attach` only returns after every tree/replay/metadata event
        // has entered `direct_tx`. Publishing the phase transition here gives
        // the writer a precise barrier rather than relying on a momentarily
        // empty direct queue or scheduler timing.
        if completes_attach {
            attach_phase_tx.send_replace(AttachPhase::Ready);
        }
        if should_close {
            break;
        }
    }
}

/// Forwards both this connection's direct replies and every session-wide
/// broadcast event to the client, until the underlying stream errors (the
/// client is gone), the broadcast sender is gone (the whole server is
/// gone), or the reader loop ends (see `read_requests`) -- at which point
/// any broadcast already queued for this connection is drained and sent
/// before returning.
async fn write_replies<W>(
    write_half: W,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<ServerEvent>,
    mut direct_rx: mpsc::Receiver<ServerEvent>,
    mut attach_phase_rx: watch::Receiver<AttachPhase>,
    mut stream_control_rx: mpsc::Receiver<StreamControlCommand>,
    resynchronization_state: Option<Arc<ServerState>>,
) where
    W: AsyncWrite + Unpin,
{
    // This belongs to one connection writer, not the session: it records the
    // newest terminal journal sequence successfully written to this client so
    // a broadcast overrun repairs only genuinely missing panes.
    let mut delivered_terminal_sequences = HashMap::new();
    // Do not send a newly connected interactive client every pane's raw
    // output before it has declared the panes it can draw. A legacy Attach
    // switches this to `All` before its full replay is assembled.
    let mut terminal_stream_selection = TerminalStreamSelection::None;
    let mut is_stream_control_open = true;
    let mut frame_writer = FrameWriter::new(write_half);

    loop {
        // While an Attach handler is building its replay batch, drain direct
        // events but leave broadcasts queued. A later Attach returns to this
        // phase because reattachment has the same ordering contract.
        if *attach_phase_rx.borrow() == AttachPhase::Replaying {
            tokio::select! {
                biased;
                attach_changed = attach_phase_rx.changed() => {
                    if attach_changed.is_err() {
                        return;
                    }
                    continue;
                },
                direct_event = direct_rx.recv() => match direct_event {
                    Some(event) => {
                        if let Err(error) = write_server_event(
                            &mut frame_writer,
                            event,
                            &mut delivered_terminal_sequences,
                            resynchronization_state.as_deref(),
                        )
                        .await
                        {
                            tracing::warn!("connection write failed during attach, closing: {error}");
                            return;
                        }
                        continue;
                    }
                    None => return,
                },
            }
        }

        let (event, is_broadcast) = tokio::select! {
            biased;
            // Checked ahead of `attach_changed`: this connection's reader
            // task owns both `direct_tx` and `attach_phase_tx` and drops
            // them together when it returns, so on every normal disconnect
            // `direct_rx` (buffered replies, then `None`) and the watch
            // channel closing become ready in the same poll. `changed()`
            // resolves to `Err` immediately and forever once its sender is
            // gone, so under `biased` it would otherwise win this race on
            // every single disconnect -- listing it first previously made
            // the `None` arm below (and its broadcast drain) unreachable.
            // Draining `direct_rx` first guarantees that never happens.
            direct_event = direct_rx.recv() => match direct_event {
                Some(event) => (event, false),
                // The reader loop ended (Detach/KillSession/EOF/decode
                // error): no more requests will ever be dispatched on this
                // connection, so no more direct replies are coming either.
                // Blocking on future `broadcast_rx.recv()`s from here would
                // park this task -- and leak its write-half fd and
                // broadcast subscription -- for as long as the session
                // stays otherwise idle, since nothing would ever wake this
                // select to notice the reader is gone (see
                // `ServerState::track_connection_task`/
                // `abort_all_connection_tasks`, which only reap tracked
                // handles lazily on the next accept or at full server
                // shutdown). Any event already queued for this connection
                // (e.g. `KillSession`'s final `TreeSnapshot`, sent before
                // its handler returns and the reader loop exits) is still
                // worth flushing, so drain what's already pending and then
                // stop, rather than waiting indefinitely for more.
                None => {
                    drain_pending_broadcasts(
                        &mut broadcast_rx,
                        &mut frame_writer,
                        &terminal_stream_selection,
                        &mut delivered_terminal_sequences,
                        resynchronization_state.as_deref(),
                    )
                    .await;
                    break;
                }
            },
            // Still checked ahead of `broadcast_result`: on reattachment
            // this must engage the replay gate before one more broadcast
            // slips across the replay boundary (see the module-level
            // comment on `attach_phase_tx`).
            attach_changed = attach_phase_rx.changed() => {
                if attach_changed.is_err() {
                    // Unreachable today -- `direct_event`'s `None` arm above
                    // always wins this race first, since both channels close
                    // together (see the comment on that branch). Drain
                    // defensively anyway so a future change that decouples
                    // `attach_phase_tx` from `direct_tx` cannot silently
                    // drop already-queued broadcasts instead of merely
                    // hitting dead code.
                    drain_pending_broadcasts(
                        &mut broadcast_rx,
                        &mut frame_writer,
                        &terminal_stream_selection,
                        &mut delivered_terminal_sequences,
                        resynchronization_state.as_deref(),
                    )
                    .await;
                    break;
                }
                continue;
            },
            stream_control = stream_control_rx.recv(), if is_stream_control_open => {
                match stream_control {
                    Some(command) => {
                        let can_continue = match command.control {
                            StreamControl::StreamAllTerminals => {
                                terminal_stream_selection = TerminalStreamSelection::All;
                                true
                            }
                            StreamControl::StreamNoTerminals => {
                                terminal_stream_selection = TerminalStreamSelection::None;
                                true
                            }
                            StreamControl::SetVisiblePanes(pane_ids) => {
                                if let Some(state) = &resynchronization_state {
                                    apply_visible_pane_selection(
                                        &mut frame_writer,
                                        state,
                                        &mut delivered_terminal_sequences,
                                        &mut terminal_stream_selection,
                                        pane_ids,
                                    )
                                    .await
                                } else {
                                    terminal_stream_selection = TerminalStreamSelection::Visible(
                                        pane_ids
                                            .into_iter()
                                            .take(MAX_VISIBLE_TERMINAL_SUBSCRIPTIONS)
                                            .collect(),
                                    );
                                    true
                                }
                            }
                        };
                        let _ = command.applied.send(());
                        if !can_continue {
                            break;
                        }
                    }
                    None => is_stream_control_open = false,
                }
                continue;
            },
            broadcast_result = broadcast_rx.recv() => match broadcast_result {
                Ok(event) => (event, true),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("connection lagged behind the session broadcast, skipped {skipped} event(s)");
                    if let Some(state) = &resynchronization_state {
                        if !write_resynchronization(
                            &mut frame_writer,
                            state,
                            &mut delivered_terminal_sequences,
                            &terminal_stream_selection,
                        )
                        .await
                        {
                            break;
                        }
                    }
                    continue;
                }
                // The session's broadcast sender only drops with
                // `ServerState` itself, i.e. the whole server is gone --
                // nothing more this connection can do.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
        };

        if is_broadcast && !should_forward_terminal_event(&event, &terminal_stream_selection) {
            continue;
        }

        // Recovery can leave already-queued output at or below the replay
        // watermark in `broadcast_rx`. Do not spend socket bandwidth sending
        // bytes the client must discard, which otherwise helps recreate the
        // same overrun immediately after repair.
        if is_broadcast && is_redundant_terminal_event(&event, &delivered_terminal_sequences) {
            continue;
        }

        // A broadcast replay describes the forwarder's gap, not this
        // connection's gap. Repair from this writer's flushed watermark so
        // a current client does not serialize and parse an old full journal.
        // Direct Attach replays never pass through this branch.
        // An Attach phase change can race this select's direct reply. Origin,
        // rather than the sampled phase, keeps the full direct replay intact.
        let event = if is_broadcast {
            let Some(event) = normalize_broadcast_terminal_replay(
                event,
                &delivered_terminal_sequences,
                resynchronization_state.as_deref(),
            )
            .await
            else {
                continue;
            };
            event
        } else {
            event
        };

        // A merged frame can overlap a recovery watermark while still ending
        // above it. Replaying the overlapping prefix would duplicate raw
        // terminal bytes, while dropping the whole frame would lose its
        // suffix. Recover again from the authoritative pane journal instead,
        // which emits exactly the missing contiguous tail.
        if is_broadcast && screen_update_requires_recovery(&event, &delivered_terminal_sequences) {
            if let Some(state) = &resynchronization_state {
                tracing::warn!("terminal output frame was not contiguous; resynchronizing");
                if !write_resynchronization(
                    &mut frame_writer,
                    state,
                    &mut delivered_terminal_sequences,
                    &terminal_stream_selection,
                )
                .await
                {
                    break;
                }
                continue;
            }
        }

        if let Err(error) = write_server_event(
            &mut frame_writer,
            event,
            &mut delivered_terminal_sequences,
            resynchronization_state.as_deref(),
        )
        .await
        {
            tracing::warn!("connection write failed, closing: {error}");
            break;
        }
    }
}

async fn normalize_broadcast_terminal_replay(
    event: ServerEvent,
    delivered_terminal_sequences: &HashMap<ilium_core::NodeId, u64>,
    state: Option<&ServerState>,
) -> Option<ServerEvent> {
    let ServerEvent::TerminalReplay { pane_id, .. } = event else {
        return Some(event);
    };
    let Some(state) = state else {
        // Only isolated writer tests lack a server authority; production
        // connections always receive one when they attach.
        return Some(event);
    };
    let after_sequence = delivered_terminal_sequences
        .get(&pane_id)
        .copied()
        .unwrap_or_default();
    handlers::terminal_recovery_event(state, pane_id, after_sequence).await
}

/// Rebuilds a lagging attached client's render cache from the current server
/// authority. This deliberately does not include `InitialStateSyncComplete`:
/// that event is an attach-only trigger boundary, whereas a replay repair
/// must be transparent to automatic action routing.
async fn write_resynchronization<W>(
    frame_writer: &mut FrameWriter<W>,
    state: &ServerState,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
    terminal_stream_selection: &TerminalStreamSelection,
) -> bool
where
    W: AsyncWrite + Unpin,
{
    for event in handlers::resynchronization_events(state, delivered_terminal_sequences).await {
        if !should_forward_terminal_event(&event, terminal_stream_selection) {
            continue;
        }
        if let Err(error) = write_server_event(
            frame_writer,
            event,
            delivered_terminal_sequences,
            Some(state),
        )
        .await
        {
            tracing::warn!(
                "connection write failed during lag resynchronization, closing: {error}"
            );
            return false;
        }
    }
    true
}

/// Applies one complete right-panel subscription at the writer, which is the
/// only task allowed to advance this connection's delivery watermarks. Each
/// selected pane is repaired through the current journal sequence before
/// queued live frames resume; duplicate queued prefixes are then discarded by
/// the existing watermark checks.
async fn apply_visible_pane_selection<W>(
    frame_writer: &mut FrameWriter<W>,
    state: &ServerState,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
    terminal_stream_selection: &mut TerminalStreamSelection,
    pane_ids: Vec<ilium_core::NodeId>,
) -> bool
where
    W: AsyncWrite + Unpin,
{
    let visible_pane_ids: HashSet<_> = pane_ids
        .into_iter()
        .take(MAX_VISIBLE_TERMINAL_SUBSCRIPTIONS)
        .collect();
    *terminal_stream_selection = TerminalStreamSelection::Visible(visible_pane_ids.clone());
    let activity_revisions: HashMap<_, _> = {
        let tree = state.tree.read().await;
        visible_pane_ids
            .iter()
            .filter_map(|pane_id| {
                tree.get(*pane_id)
                    .map(|node| (*pane_id, node.activity_revision))
            })
            .collect()
    };
    for pane_id in &visible_pane_ids {
        if let Some(activity_revision) = activity_revisions.get(pane_id) {
            let activity_event = ServerEvent::NodeActivityChanged {
                node_id: *pane_id,
                activity_revision: *activity_revision,
            };
            if let Err(error) = write_server_event(
                frame_writer,
                activity_event,
                delivered_terminal_sequences,
                Some(state),
            )
            .await
            {
                tracing::warn!("connection write failed during activity synchronization: {error}");
                return false;
            }
        }
        let after_sequence = delivered_terminal_sequences
            .get(pane_id)
            .copied()
            .unwrap_or_default();
        let Some(event) = handlers::terminal_recovery_event(state, *pane_id, after_sequence).await
        else {
            continue;
        };
        if let Err(error) = write_server_event(
            frame_writer,
            event,
            delivered_terminal_sequences,
            Some(state),
        )
        .await
        {
            tracing::warn!("connection write failed during pane subscription: {error}");
            return false;
        }
    }
    true
}

fn should_forward_terminal_event(
    event: &ServerEvent,
    terminal_stream_selection: &TerminalStreamSelection,
) -> bool {
    match event {
        ServerEvent::ScreenUpdate { pane_id, .. } | ServerEvent::TerminalReplay { pane_id, .. } => {
            terminal_stream_selection.includes(*pane_id)
        }
        _ => true,
    }
}

/// Writes one event and advances the per-connection output watermark only
/// after the frame reached the socket. Failed writes must not claim delivery.
const SERVER_CODEC_COST: ilium_execution::JobCost = ilium_execution::JobCost {
    input_bytes: 128 * 1024 * 1024,
    result_bytes: 192 * 1024 * 1024,
};

fn codec_client(
    state: Option<&ServerState>,
    is_decoder: bool,
) -> Result<crate::execution::ExecutionClient, ilium_ipc::IpcError> {
    if let Some(owner) = state.and_then(|state| state.execution.get()) {
        return Ok(if is_decoder {
            owner.decoder.clone()
        } else {
            owner.encoder.clone()
        });
    }
    #[cfg(test)]
    {
        Ok(crate::execution::test_codec_client(is_decoder))
    }
    #[cfg(not(test))]
    {
        Err(ilium_ipc::IpcError::Io(std::io::Error::other(
            "server codec execution owner is not initialized",
        )))
    }
}

async fn decode_client_request<R: AsyncRead + Unpin>(
    reader: &mut FrameReader<R>,
    preparation: &crate::execution::ExecutionClient,
) -> Result<ilium_execution::Retained<ClientRequest>, ilium_ipc::IpcError> {
    let length = reader.read_encoded_length().await?;
    let reservation = preparation
        .reserve(ilium_execution::Lane::Cpu, SERVER_CODEC_COST)
        .await
        .map_err(|error| {
            ilium_ipc::IpcError::Io(std::io::Error::other(format!(
                "server decoder admission: {error:?}"
            )))
        })?;
    let frame = reader.read_encoded_payload(length).await?;
    preparation
        .run_reserved(reservation, move |_| {
            ilium_ipc::decode_bounded_frame::<ClientRequest>(&frame)
        })
        .await
        .map_err(|error| ilium_ipc::IpcError::Io(std::io::Error::other(error)))
}

async fn write_server_event<W>(
    frame_writer: &mut FrameWriter<W>,
    mut event: ServerEvent,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
    state: Option<&ServerState>,
) -> Result<(), ilium_ipc::IpcError>
where
    W: AsyncWrite + Unpin,
{
    // Sample authoritative trigger settings before the worker, releasing the
    // read lock before any CPU admission or potentially blocked output.
    if matches!(&event, ServerEvent::TextTriggersChanged { .. }) {
        let authority = state.ok_or_else(|| {
            ilium_ipc::IpcError::Io(std::io::Error::other(
                "Text Trigger output requires server authority",
            ))
        })?;
        event = crate::text_trigger_config::snapshot(authority).await;
    }
    let preparation = codec_client(state, false)?;
    let reservation = preparation
        .reserve(ilium_execution::Lane::Cpu, SERVER_CODEC_COST)
        .await
        .map_err(|error| {
            ilium_ipc::IpcError::Io(std::io::Error::other(format!(
                "server codec admission: {error:?}"
            )))
        })?;
    let encoded = preparation
        .run_reserved(reservation, move |_| {
            let frame = ilium_ipc::encode_frame(&event)?;
            Ok::<_, ilium_ipc::IpcError>((frame, event))
        })
        .await
        .map_err(|error| ilium_ipc::IpcError::Io(std::io::Error::other(error)))?;
    frame_writer.write_encoded(&encoded.view().0).await?;
    record_delivered_terminal_sequence(delivered_terminal_sequences, &encoded.view().1);
    Ok(())
}

/// Records the newest raw-output sequence represented by a live update or a
/// replay. Both variants establish the same deduplication watermark.
fn record_delivered_terminal_sequence(
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
    event: &ServerEvent,
) {
    if let ServerEvent::TreeSnapshot(tree) = event {
        // Node ids are never reused, but one long-lived attachment can still
        // create and close many panes. Prune connection-local watermarks with
        // the same authoritative snapshot that removes their render caches.
        delivered_terminal_sequences
            .retain(|pane_id, _| tree.get(*pane_id).is_some_and(ilium_core::Node::is_pane));
        return;
    }

    let (pane_id, sequence) = match event {
        ServerEvent::ScreenUpdate {
            pane_id, sequence, ..
        } => (*pane_id, *sequence),
        ServerEvent::TerminalReplay {
            pane_id,
            through_sequence,
            ..
        } => (*pane_id, *through_sequence),
        _ => return,
    };
    delivered_terminal_sequences
        .entry(pane_id)
        .and_modify(|delivered| *delivered = (*delivered).max(sequence))
        .or_insert(sequence);
}

/// Returns whether a queued terminal event is wholly covered by a replay or
/// newer live frame already written to this connection.
fn is_redundant_terminal_event(
    event: &ServerEvent,
    delivered_terminal_sequences: &HashMap<ilium_core::NodeId, u64>,
) -> bool {
    let (pane_id, sequence) = match event {
        ServerEvent::ScreenUpdate {
            pane_id, sequence, ..
        } => (*pane_id, *sequence),
        ServerEvent::TerminalReplay {
            pane_id,
            through_sequence,
            ..
        } => (*pane_id, *through_sequence),
        _ => return false,
    };
    delivered_terminal_sequences
        .get(&pane_id)
        .is_some_and(|delivered| sequence <= *delivered)
}

/// Returns whether a live frame starts anywhere other than the next exact
/// pane-local journal sequence already delivered to this connection.
///
/// This catches both gaps and partial overlap after lag recovery. A wholly
/// covered frame is handled separately by [`is_redundant_terminal_event`].
fn screen_update_requires_recovery(
    event: &ServerEvent,
    delivered_terminal_sequences: &HashMap<ilium_core::NodeId, u64>,
) -> bool {
    let ServerEvent::ScreenUpdate {
        pane_id,
        first_sequence,
        ..
    } = event
    else {
        return false;
    };
    let Some(delivered_sequence) = delivered_terminal_sequences.get(pane_id) else {
        return false;
    };
    *first_sequence != delivered_sequence.saturating_add(1)
}

/// Flushes every broadcast event already sitting in `broadcast_rx`'s buffer
/// (non-blocking) to `write_half`, then returns -- used only once the
/// reader loop has ended and no more direct replies or requests are coming,
/// so there is no reason left to keep waiting on *future* broadcasts (see
/// `write_replies`).
///
/// The same per-connection delivery watermarks that guard the live path apply
/// here: after a lag resynchronization, this queue can still hold terminal
/// frames at or below the watermark, and replaying them would duplicate raw
/// bytes in the client's parser in the final state it renders. A
/// non-contiguous frame is dropped rather than resynchronized -- the
/// connection is ending, and applying misordered bytes is strictly worse than
/// omitting a tail the client will never observe settle anyway.
async fn drain_pending_broadcasts<W>(
    broadcast_rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    frame_writer: &mut FrameWriter<W>,
    terminal_stream_selection: &TerminalStreamSelection,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
    state: Option<&ServerState>,
) where
    W: AsyncWrite + Unpin,
{
    loop {
        let event = match broadcast_rx.try_recv() {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "connection lagged while draining final broadcasts, skipped {skipped} event(s)"
                );
                continue;
            }
            // Empty: nothing left queued. Closed: the whole server is gone.
            // Either way, nothing more to flush.
            Err(_) => return,
        };
        if !should_forward_terminal_event(&event, terminal_stream_selection) {
            continue;
        }
        if is_redundant_terminal_event(&event, delivered_terminal_sequences)
            || screen_update_requires_recovery(&event, delivered_terminal_sequences)
        {
            continue;
        }
        let Some(event) =
            normalize_broadcast_terminal_replay(event, delivered_terminal_sequences, state).await
        else {
            continue;
        };
        if let Err(error) =
            write_server_event(frame_writer, event, delivered_terminal_sequences, state).await
        {
            tracing::warn!("connection write failed while draining final broadcasts: {error}");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ilium_core::{NodeId, Tree};
    use ilium_ipc::read_frame;
    use tokio::io::duplex;
    use tokio::sync::{broadcast, mpsc, watch};
    use tokio::time::{timeout, Duration};

    use super::*;

    #[test]
    fn visible_pane_selection_does_not_block_request_intake() {
        assert!(!stream_control_requires_request_barrier(
            &StreamControl::SetVisiblePanes(vec![NodeId(7)]),
        ));
        assert!(stream_control_requires_request_barrier(
            &StreamControl::StreamAllTerminals,
        ));
        assert!(stream_control_requires_request_barrier(
            &StreamControl::StreamNoTerminals,
        ));
    }

    /// A live chunk produced during Attach must remain behind the replay
    /// cutover even when it reaches the broadcast receiver first. If it
    /// overtakes replay, `TerminalView::apply_replay` resets the parser and
    /// permanently erases that already-applied newer chunk.
    #[tokio::test]
    async fn live_broadcast_waits_for_complete_attach_replay() {
        let (server_stream, mut client_stream) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = broadcast::channel(8);
        let (direct_tx, direct_rx) = mpsc::channel(8);
        // A normal Attach queues its all-pane subscription before it starts
        // the replay barrier. Model that ordering here: otherwise the
        // writer correctly holds the subscription control while replaying
        // and the already-queued live frame would be filtered as `None`.
        let (attach_phase_tx, attach_phase_rx) = watch::channel(AttachPhase::Open);
        let (stream_control_tx, stream_control_rx) = mpsc::channel(1);
        let (applied_tx, _applied_rx) = oneshot::channel();
        stream_control_tx
            .send(StreamControlCommand {
                control: StreamControl::StreamAllTerminals,
                applied: applied_tx,
            })
            .await
            .unwrap();
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            attach_phase_rx,
            stream_control_rx,
            None,
        ));
        tokio::task::yield_now().await;
        attach_phase_tx.send_replace(AttachPhase::Replaying);

        let live_event = ServerEvent::ScreenUpdate {
            pane_id: NodeId(2),
            first_sequence: 2,
            sequence: 2,
            bytes: b"live-after-replay".to_vec(),
        };
        broadcast_tx.send(live_event.clone()).unwrap();
        // Give the writer an opportunity to observe the broadcast before any
        // direct event exists. The attach barrier, not select timing, must be
        // what holds it back.
        tokio::task::yield_now().await;

        let tree_event = ServerEvent::TreeSnapshot(Tree::new());
        let replay_event = ServerEvent::TerminalReplay {
            pane_id: NodeId(2),
            through_sequence: 1,
            bytes: b"retained-history".to_vec(),
            is_complete: true,
        };
        direct_tx.send(tree_event.clone()).await.unwrap();
        direct_tx.send(replay_event.clone()).await.unwrap();
        attach_phase_tx.send_replace(AttachPhase::Ready);

        let first = timeout(
            Duration::from_secs(1),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .unwrap()
        .unwrap();
        let second = timeout(
            Duration::from_secs(1),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .unwrap()
        .unwrap();
        let third = timeout(
            Duration::from_secs(1),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(first, tree_event);
        assert_eq!(second, replay_event);
        assert_eq!(third, live_event);

        drop(direct_tx);
        drop(attach_phase_tx);
        drop(broadcast_tx);
        timeout(Duration::from_secs(1), writer)
            .await
            .unwrap()
            .unwrap();
    }

    /// Lifecycle-only clients issue commands before Attach and still need
    /// their resulting broadcasts. The replay barrier must not turn the
    /// connection's initial open phase into an implicit Attach requirement.
    #[tokio::test]
    async fn broadcast_before_attach_is_forwarded() {
        let (server_stream, mut client_stream) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = broadcast::channel(8);
        let (_direct_tx, direct_rx) = mpsc::channel(8);
        let (_attach_phase_tx, attach_phase_rx) = watch::channel(AttachPhase::Open);
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            attach_phase_rx,
            mpsc::channel(1).1,
            None,
        ));

        let tree_event = ServerEvent::TreeSnapshot(Tree::new());
        broadcast_tx.send(tree_event.clone()).unwrap();

        let received = timeout(
            Duration::from_secs(1),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(received, tree_event);

        writer.abort();
    }

    /// A raw PTY frame is not useful until an interactive client has named a
    /// visible terminal. Starting disconnected from terminal output prevents
    /// a busy many-pane session from recreating every hidden parser during
    /// the attach handshake.
    #[tokio::test]
    async fn terminal_broadcast_before_subscription_is_not_forwarded() {
        let (server_stream, mut client_stream) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = broadcast::channel(8);
        let (_direct_tx, direct_rx) = mpsc::channel(8);
        let (_attach_phase_tx, attach_phase_rx) = watch::channel(AttachPhase::Open);
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            attach_phase_rx,
            mpsc::channel(1).1,
            None,
        ));

        broadcast_tx
            .send(ServerEvent::ScreenUpdate {
                pane_id: NodeId(2),
                first_sequence: 1,
                sequence: 1,
                bytes: b"hidden-before-subscription".to_vec(),
            })
            .unwrap();
        assert!(timeout(
            Duration::from_millis(50),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .is_err());

        let tree_event = ServerEvent::TreeSnapshot(Tree::new());
        broadcast_tx.send(tree_event.clone()).unwrap();
        let received = timeout(
            Duration::from_secs(1),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(received, tree_event);

        writer.abort();
    }

    #[tokio::test]
    async fn broadcast_lag_rebuilds_state_without_retriggering_startup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "lag-repair".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("lag-repair.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let (server_stream, mut client_stream) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = broadcast::channel(1);
        let (direct_tx, direct_rx) = mpsc::channel(8);
        let (attach_phase_tx, attach_phase_rx) = watch::channel(AttachPhase::Ready);
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            attach_phase_rx,
            mpsc::channel(1).1,
            Some(Arc::clone(&state)),
        ));

        for sequence in 1..=3 {
            broadcast_tx
                .send(ServerEvent::ScreenUpdate {
                    pane_id: NodeId(9),
                    first_sequence: sequence,
                    sequence,
                    bytes: vec![sequence as u8],
                })
                .expect("writer is subscribed before broadcasts begin");
        }

        let repaired = timeout(
            Duration::from_secs(1),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .expect("lag repair did not write a state snapshot")
        .expect("read repaired state event");
        assert!(matches!(repaired, ServerEvent::PaneStateSnapshot { .. }));
        assert!(
            !handlers::initial_state_events(&state, false, true)
                .await
                .contains(&ServerEvent::InitialStateSyncComplete),
            "a lag repair must not create another startup trigger boundary"
        );

        drop(direct_tx);
        drop(attach_phase_tx);
        drop(broadcast_tx);
        sound_task.abort();
        timeout(Duration::from_secs(1), writer)
            .await
            .expect("writer did not stop")
            .expect("writer task panicked");
    }

    #[test]
    fn delivered_replay_suppresses_only_covered_terminal_events() {
        let pane_id = NodeId(9);
        let mut delivered = HashMap::new();
        let replay = ServerEvent::TerminalReplay {
            pane_id,
            through_sequence: 7,
            bytes: b"replay".to_vec(),
            is_complete: true,
        };
        record_delivered_terminal_sequence(&mut delivered, &replay);

        assert!(is_redundant_terminal_event(
            &ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 7,
                sequence: 7,
                bytes: b"duplicate".to_vec(),
            },
            &delivered,
        ));
        assert!(!is_redundant_terminal_event(
            &ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 8,
                sequence: 8,
                bytes: b"new".to_vec(),
            },
            &delivered,
        ));
        assert!(!is_redundant_terminal_event(
            &ServerEvent::TreeSnapshot(Tree::new()),
            &delivered,
        ));

        record_delivered_terminal_sequence(&mut delivered, &ServerEvent::TreeSnapshot(Tree::new()));
        assert!(delivered.is_empty());
    }

    #[tokio::test]
    async fn broadcast_replay_for_closed_pane_is_not_sent_to_a_new_parser() {
        let directory = tempfile::tempdir().expect("tempdir");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = ServerState::new(crate::state::ServerStateOptions {
            session_name: "closed-replay".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("closed-replay.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        });
        let replay = ServerEvent::TerminalReplay {
            pane_id: NodeId(9),
            through_sequence: 8,
            bytes: b"stale journal".to_vec(),
            is_complete: true,
        };
        let delivered = HashMap::from([(NodeId(9), 7)]);
        assert_eq!(
            normalize_broadcast_terminal_replay(replay.clone(), &delivered, Some(&state)).await,
            None,
        );
        assert_eq!(
            normalize_broadcast_terminal_replay(replay.clone(), &delivered, None).await,
            Some(replay),
            "isolated writer tests without authority retain their supplied event"
        );
        sound_task.abort();
    }

    #[tokio::test]
    async fn live_pane_broadcast_replay_sends_only_missing_bytes_and_attach_stays_full() {
        let directory = tempfile::tempdir().expect("private directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "live-replay-normalization".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let group_id = state
            .tree
            .write()
            .await
            .add_group(ilium_core::ROOT_ID, "writer test")
            .expect("root accepts a group");
        let (request_tx, _request_rx) = mpsc::channel(128);
        // Each `read` blocks the owned command until this test sends input.
        // This provides two stable, real PTY journal boundaries without a
        // timing-based flood or a synthetic journal mutation.
        assert!(
            !handlers::handle_request(
                &state,
                ClientRequest::NewPane {
                    parent_group: group_id,
                    kind: ilium_ipc::NewPaneKind::Command(
                        "printf 'writer-prefix\\n'; read phase; printf 'writer-tail\\n'; read hold"
                            .to_string(),
                    ),
                    working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
                },
                &request_tx,
            )
            .await
        );
        let pane_id = {
            let panes = state.panes.read().await;
            assert_eq!(panes.len(), 1, "fixture must own exactly one live pane");
            *panes.keys().next().expect("registered terminal pane")
        };
        let (prefix_sequence, prefix_bytes) = timeout(Duration::from_secs(5), async {
            loop {
                let replay = handlers::initial_state_events(&state, false, true)
                    .await
                    .into_iter()
                    .find(|event| matches!(event, ServerEvent::TerminalReplay { pane_id: id, .. } if *id == pane_id));
                if let Some(ServerEvent::TerminalReplay {
                    through_sequence,
                    bytes,
                    is_complete: true,
                    ..
                }) = replay
                {
                    if bytes.windows(b"writer-prefix".len()).any(|window| window == b"writer-prefix") {
                        break (through_sequence, bytes);
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("first owned PTY output did not arrive");
        assert!(prefix_sequence > 0);

        assert!(
            !handlers::handle_request(
                &state,
                ClientRequest::KeyInput {
                    pane_id,
                    bytes: b"continue\n".to_vec(),
                    submission: None,
                },
                &request_tx,
            )
            .await
        );
        let (full_sequence, full_bytes, missing_tail) = timeout(Duration::from_secs(5), async {
            loop {
                let replay = handlers::initial_state_events(&state, false, true)
                    .await
                    .into_iter()
                    .find(|event| matches!(event, ServerEvent::TerminalReplay { pane_id: id, .. } if *id == pane_id));
                if let Some(ServerEvent::TerminalReplay {
                    through_sequence,
                    bytes,
                    is_complete: true,
                    ..
                }) = replay
                {
                    if through_sequence > prefix_sequence
                        && bytes.windows(b"writer-tail".len()).any(|window| window == b"writer-tail")
                    {
                        if let Some(ServerEvent::ScreenUpdate {
                            first_sequence,
                            sequence,
                            bytes: tail,
                            ..
                        }) = handlers::terminal_recovery_event(&state, pane_id, prefix_sequence).await
                        {
                            let mut joined = prefix_bytes.clone();
                            joined.extend_from_slice(&tail);
                            if first_sequence == prefix_sequence + 1
                                && sequence == through_sequence
                                && joined == bytes
                            {
                                break (through_sequence, bytes, tail);
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("second owned PTY output did not form a contiguous tail");
        assert!(!missing_tail.is_empty());

        let (server_stream, mut client_stream) = duplex(64 * 1024);
        let (broadcast_tx, broadcast_rx) = broadcast::channel(8);
        let (direct_tx, direct_rx) = mpsc::channel(8);
        let (phase_tx, phase_rx) = watch::channel(AttachPhase::Replaying);
        let (control_tx, control_rx) = mpsc::channel(1);
        let (applied_tx, applied_rx) = oneshot::channel();
        control_tx
            .send(StreamControlCommand {
                control: StreamControl::StreamAllTerminals,
                applied: applied_tx,
            })
            .await
            .expect("queue all-terminal selection");
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            phase_rx,
            control_rx,
            Some(Arc::clone(&state)),
        ));
        let direct_prefix = ServerEvent::TerminalReplay {
            pane_id,
            through_sequence: prefix_sequence,
            bytes: prefix_bytes.clone(),
            is_complete: true,
        };
        direct_tx
            .send(direct_prefix.clone())
            .await
            .expect("direct replay");
        let first = timeout(
            Duration::from_secs(5),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .expect("direct prefix timed out")
        .expect("direct prefix frame");
        assert_eq!(first, direct_prefix);
        phase_tx.send_replace(AttachPhase::Ready);
        timeout(Duration::from_secs(5), applied_rx)
            .await
            .expect("terminal selection timed out")
            .expect("terminal selection not applied");

        let full_replay = ServerEvent::TerminalReplay {
            pane_id,
            through_sequence: full_sequence,
            bytes: full_bytes.clone(),
            is_complete: true,
        };
        broadcast_tx
            .send(full_replay.clone())
            .expect("broadcast replay");
        let second = timeout(
            Duration::from_secs(5),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .expect("missing tail timed out")
        .expect("missing tail frame");
        assert_eq!(
            second,
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: prefix_sequence + 1,
                sequence: full_sequence,
                bytes: missing_tail.clone(),
            },
            "broadcast recovery must append only the missing raw bytes"
        );
        let mut combined = prefix_bytes;
        combined.extend_from_slice(&missing_tail);
        assert_eq!(
            combined, full_bytes,
            "the delivered raw stream changed byte order or count"
        );

        // An old replay through the already-delivered sequence must vanish,
        // while later metadata still crosses the same ordered writer.
        broadcast_tx
            .send(full_replay.clone())
            .expect("duplicate replay");
        let marker = ServerEvent::Error {
            message: "after-tail".to_string(),
        };
        broadcast_tx.send(marker.clone()).expect("metadata marker");
        let next = timeout(
            Duration::from_secs(5),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .expect("metadata after redundant replay timed out")
        .expect("metadata after redundant replay frame");
        assert_eq!(next, marker);

        // A new Attach intentionally resets from a full direct replay. Only
        // broadcasts are normalized by the connection writer.
        phase_tx.send_replace(AttachPhase::Replaying);
        direct_tx
            .send(full_replay.clone())
            .await
            .expect("new attach replay");
        let attached = timeout(
            Duration::from_secs(5),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .expect("full attach replay timed out")
        .expect("full attach replay frame");
        assert_eq!(attached, full_replay);
        phase_tx.send_replace(AttachPhase::Ready);
        broadcast_tx
            .send(full_replay)
            .expect("up-to-date broadcast replay");
        let final_marker = ServerEvent::Error {
            message: "after-attach".to_string(),
        };
        broadcast_tx
            .send(final_marker.clone())
            .expect("final metadata marker");
        let next = timeout(
            Duration::from_secs(5),
            read_frame::<ServerEvent, _>(&mut client_stream),
        )
        .await
        .expect("metadata after up-to-date replay timed out")
        .expect("metadata after up-to-date replay frame");
        assert_eq!(next, final_marker);

        drop(direct_tx);
        drop(phase_tx);
        drop(broadcast_tx);
        drop(control_tx);
        timeout(Duration::from_secs(5), writer)
            .await
            .expect("writer did not stop")
            .expect("writer task panicked");
        assert!(
            !handlers::handle_request(&state, ClientRequest::ClosePane { pane_id }, &request_tx,)
                .await
        );
        sound_task.abort();
    }

    #[test]
    fn partial_overlap_and_gaps_require_journal_recovery() {
        let pane_id = NodeId(9);
        let delivered = HashMap::from([(pane_id, 7)]);

        assert!(screen_update_requires_recovery(
            &ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 6,
                sequence: 8,
                bytes: b"overlap-and-new-suffix".to_vec(),
            },
            &delivered,
        ));
        assert!(screen_update_requires_recovery(
            &ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 9,
                sequence: 9,
                bytes: b"gap".to_vec(),
            },
            &delivered,
        ));
        assert!(!screen_update_requires_recovery(
            &ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 8,
                sequence: 9,
                bytes: b"contiguous-merged-frame".to_vec(),
            },
            &delivered,
        ));
    }
}

#[cfg(test)]
mod text_trigger_writer_tests {
    use super::*;
    use ilium_ipc::{TextTrigger, TextTriggerSettings};
    use tokio::io::{duplex, AsyncReadExt};
    use tokio::time::{timeout, Duration};
    const WAIT: Duration = Duration::from_secs(5);
    struct Task<T>(tokio::task::JoinHandle<T>);
    impl<T> Drop for Task<T> {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    impl<T> Task<T> {
        async fn joined(&mut self) -> T {
            timeout(WAIT, &mut self.0)
                .await
                .expect("owned task timed out")
                .expect("owned task panicked")
        }
    }
    fn rules(message: &str) -> TextTriggerSettings {
        TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: "stable".to_owned(),
                regexp: "ready$".to_owned(),
                message: message.to_owned(),
                ..TextTrigger::default()
            }],
        }
    }
    fn state_at(directory: &std::path::Path) -> (Arc<ServerState>, Task<()>) {
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "trigger-writer".to_owned(),
            session_cwd: directory.to_path_buf(),
            home_dir: directory.to_path_buf(),
            snapshot_path: directory.join("snapshot.json"),
            socket_path: directory.join("unused.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: false,
        }));
        (state, Task(sound_task))
    }
    async fn accept(state: &ServerState, message: &str, revision: u64) {
        let mut current = state.text_trigger_settings.write().await;
        current.settings = rules(message);
        current.revision = revision;
    }
    fn event(message: &str) -> ServerEvent {
        ServerEvent::TextTriggersChanged {
            settings: rules(message),
        }
    }
    async fn read<S: AsyncRead + Unpin>(stream: &mut S) -> ServerEvent {
        timeout(WAIT, ilium_ipc::read_frame(stream))
            .await
            .expect("frame timed out")
            .expect("frame decode failed")
    }

    #[tokio::test]
    async fn text_trigger_attach_and_live_broadcast_use_current_authority() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        accept(&state, "B", 2).await;
        let (server, mut client) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel(8);
        let (direct_tx, direct_rx) = mpsc::channel(8);
        let (phase_tx, phase_rx) = watch::channel(AttachPhase::Replaying);
        direct_tx.send(event("A")).await.unwrap();
        broadcast_tx.send(event("A")).unwrap();
        let mut writer = Task(tokio::spawn(write_replies(
            server,
            broadcast_rx,
            direct_rx,
            phase_rx,
            mpsc::channel(1).1,
            Some(Arc::clone(&state)),
        )));
        assert_eq!(read(&mut client).await, event("B"));
        phase_tx.send_replace(AttachPhase::Ready);
        assert_eq!(read(&mut client).await, event("B"));
        drop(direct_tx);
        writer.joined().await;
    }

    #[tokio::test]
    async fn text_trigger_reader_exit_drain_cannot_restore_a_stale_payload() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        accept(&state, "B", 2).await;
        let (server, mut client) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel(8);
        let (direct_tx, direct_rx) = mpsc::channel(8);
        let (_phase_tx, phase_rx) = watch::channel(AttachPhase::Ready);
        direct_tx.send(event("A")).await.unwrap();
        broadcast_tx.send(event("A")).unwrap();
        drop(direct_tx);
        let mut writer = Task(tokio::spawn(write_replies(
            server,
            broadcast_rx,
            direct_rx,
            phase_rx,
            mpsc::channel(1).1,
            Some(Arc::clone(&state)),
        )));
        assert_eq!(read(&mut client).await, event("B"));
        assert_eq!(read(&mut client).await, event("B"));
        writer.joined().await;
    }

    #[tokio::test]
    async fn text_trigger_stalled_socket_releases_settings_and_orders_later_frames() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        accept(&state, "A", 1).await;
        let (server, mut client) = duplex(1);
        let writer_state = Arc::clone(&state);
        let mut writer = Task(tokio::spawn(async move {
            let mut framed = FrameWriter::new(server);
            let mut sequences = HashMap::new();
            write_server_event(
                &mut framed,
                event("stale"),
                &mut sequences,
                Some(&writer_state),
            )
            .await
            .unwrap();
            write_server_event(&mut framed, event("A"), &mut sequences, Some(&writer_state))
                .await
                .unwrap();
            assert!(sequences.is_empty());
        }));
        let mut prefix = [0_u8; 1];
        timeout(WAIT, client.read_exact(&mut prefix))
            .await
            .unwrap()
            .unwrap();
        let mut current = timeout(WAIT, state.text_trigger_settings.write())
            .await
            .expect("socket write retained the settings lock");
        current.settings = rules("B");
        current.revision = 2;
        drop(current);
        let mut reconstructed = (&prefix[..]).chain(&mut client);
        assert_eq!(read(&mut reconstructed).await, event("A"));
        assert_eq!(read(&mut reconstructed).await, event("B"));
        writer.joined().await;
    }

    #[tokio::test]
    async fn text_trigger_lag_repair_then_old_event_cannot_regress_settings() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        accept(&state, "B", 2).await;
        let (server, mut client) = duplex(64 * 1024);
        let mut writer = FrameWriter::new(server);
        let mut sequences = HashMap::new();
        let expected_count = handlers::resynchronization_events(&state, &sequences)
            .await
            .len();
        assert!(timeout(
            WAIT,
            write_resynchronization(
                &mut writer,
                &state,
                &mut sequences,
                &TerminalStreamSelection::None
            )
        )
        .await
        .unwrap());
        write_server_event(&mut writer, event("A"), &mut sequences, Some(&state))
            .await
            .unwrap();
        let mut saw_rules = false;
        for _ in 0..expected_count {
            let received = read(&mut client).await;
            assert!(!matches!(received, ServerEvent::InitialStateSyncComplete));
            saw_rules |= received == event("B");
        }
        assert!(saw_rules);
        assert_eq!(read(&mut client).await, event("B"));
    }

    #[tokio::test]
    async fn text_trigger_writer_refuses_snapshots_without_authority() {
        let mut writer = FrameWriter::new(tokio::io::sink());
        let mut sequences = HashMap::new();
        assert!(
            write_server_event(&mut writer, event("untrusted"), &mut sequences, None)
                .await
                .is_err()
        );
        assert!(sequences.is_empty());
    }
}

#[cfg(test)]
mod text_trigger_ordering_regressions {
    use super::*;
    use ilium_ipc::{read_frame, TextTrigger, TextTriggerSettings};
    use tokio::io::duplex;
    use tokio::time::{timeout, Duration};

    async fn assert_newer_rules_survive_queued_old_event(lag: bool, drain: bool) {
        let directory = tempfile::tempdir().expect("private directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "trigger-ordering".to_owned(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let newer = TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: "retained-newer-rule".to_owned(),
                regexp: "synthetic-ready".to_owned(),
                message: "synthetic-reply".to_owned(),
                ..TextTrigger::default()
            }],
        };
        {
            let mut accepted = state.text_trigger_settings.write().await;
            accepted.settings = newer.clone();
            accepted.revision = 2;
        }
        let (server_stream, mut client_stream) = duplex(16384);
        let (broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel(if lag { 2 } else { 8 });
        let (direct_tx, direct_rx) = mpsc::channel(32);
        let (phase_tx, phase_rx) = watch::channel(AttachPhase::Ready);
        // Queue an older accepted broadcast before constructing the newer
        // authoritative attach snapshot. In the lag case force exactly one
        // dropped frame, retaining the older settings event after recovery.
        if lag {
            for _ in 0..2 {
                broadcast_tx
                    .send(ServerEvent::TreeSnapshot(ilium_core::Tree::new()))
                    .unwrap();
            }
        }
        broadcast_tx
            .send(ServerEvent::TextTriggersChanged {
                settings: TextTriggerSettings::default(),
            })
            .unwrap();
        if !lag {
            for event in handlers::initial_state_events(&state, true, false).await {
                direct_tx.send(event).await.unwrap();
            }
        }
        // Dropping the request side exercises the real final-broadcast drain;
        // keeping it open exercises the normal single-writer broadcast path.
        let keep_direct = if drain {
            drop(direct_tx);
            None
        } else {
            Some(direct_tx)
        };
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            phase_rx,
            mpsc::channel(1).1,
            Some(Arc::clone(&state)),
        ));
        let mut delivered = Vec::new();
        timeout(Duration::from_secs(2), async {
            while delivered.len() < 2 {
                let event = read_frame::<ServerEvent, _>(&mut client_stream)
                    .await
                    .unwrap();
                if let ServerEvent::TextTriggersChanged { settings } = event {
                    delivered.push(settings);
                }
            }
        })
        .await
        .expect("two settings frames not delivered");
        drop(keep_direct);
        drop(phase_tx);
        drop(broadcast_tx);
        timeout(Duration::from_secs(2), writer)
            .await
            .expect("writer did not stop")
            .unwrap();
        sound_task.abort();
        let _ = sound_task.await;
        let authoritative = state.text_trigger_settings.read().await.settings.clone();
        println!(
            "{}",
            serde_json::json!({
                "type":"result", "lag":lag, "drain":drain,
                "delivered_rule_counts":delivered.iter().map(|settings| settings.triggers.len()).collect::<Vec<_>>(),
                "authoritative_rule_count":authoritative.triggers.len(),
            })
        );
        assert_eq!(
            delivered.first(),
            Some(&newer),
            "newer authority was not seeded first"
        );
        assert_eq!(
            delivered.last(),
            Some(&authoritative),
            "queued older TextTriggersChanged rolled the client list back after authoritative synchronization"
        );
    }

    #[tokio::test]
    async fn newer_attach_snapshot_survives_queued_old_trigger_broadcast() {
        assert_newer_rules_survive_queued_old_event(false, false).await;
    }
    #[tokio::test]
    async fn newer_attach_snapshot_survives_final_old_trigger_drain() {
        assert_newer_rules_survive_queued_old_event(false, true).await;
    }
    #[tokio::test]
    async fn newer_lag_snapshot_survives_retained_old_trigger_broadcast() {
        assert_newer_rules_survive_queued_old_event(true, false).await;
    }
}
