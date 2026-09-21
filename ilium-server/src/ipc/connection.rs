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
    let mut terminal_subscription_guard = TerminalSubscriptionGuard::new(Arc::clone(&state));
    let mut has_terminal_stream_selection = false;
    loop {
        let request: ClientRequest = tokio::select! {
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
            read_result = frame_reader.read() => match read_result {
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
        let should_close = handlers::handle_request(&state, request, &direct_tx).await;
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
                            &event,
                            &mut delivered_terminal_sequences,
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

        let event = tokio::select! {
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
                Some(event) => event,
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
                Ok(event) => event,
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

        if !should_forward_terminal_event(&event, &terminal_stream_selection) {
            continue;
        }

        // Recovery can leave already-queued output at or below the replay
        // watermark in `broadcast_rx`. Do not spend socket bandwidth sending
        // bytes the client must discard, which otherwise helps recreate the
        // same overrun immediately after repair.
        if is_redundant_terminal_event(&event, &delivered_terminal_sequences) {
            continue;
        }

        // A merged frame can overlap a recovery watermark while still ending
        // above it. Replaying the overlapping prefix would duplicate raw
        // terminal bytes, while dropping the whole frame would lose its
        // suffix. Recover again from the authoritative pane journal instead,
        // which emits exactly the missing contiguous tail.
        if screen_update_requires_recovery(&event, &delivered_terminal_sequences) {
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

        if let Err(error) =
            write_server_event(&mut frame_writer, &event, &mut delivered_terminal_sequences).await
        {
            tracing::warn!("connection write failed, closing: {error}");
            break;
        }
    }
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
        if let Err(error) =
            write_server_event(frame_writer, &event, delivered_terminal_sequences).await
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
            if let Err(error) =
                write_server_event(frame_writer, &activity_event, delivered_terminal_sequences)
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
        if let Err(error) =
            write_server_event(frame_writer, &event, delivered_terminal_sequences).await
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
async fn write_server_event<W>(
    frame_writer: &mut FrameWriter<W>,
    event: &ServerEvent,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
) -> Result<(), ilium_ipc::IpcError>
where
    W: AsyncWrite + Unpin,
{
    frame_writer.write(event).await?;
    record_delivered_terminal_sequence(delivered_terminal_sequences, event);
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
        if let Err(error) =
            write_server_event(frame_writer, &event, delivered_terminal_sequences).await
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
        assert!(matches!(repaired, ServerEvent::TreeSnapshot(_)));
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
