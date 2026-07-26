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

use std::collections::HashMap;
use std::sync::Arc;

use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};

use crate::ipc::handlers;
use crate::state::ServerState;

/// Bound on this connection's own direct-reply queue (`Attach` snapshots,
/// per-request errors, resize/key/mouse acks). A slow or stalled client
/// still gets backpressured -- `handlers::send_direct` awaits capacity --
/// rather than letting the queue grow without bound while `write_replies`
/// is stuck on a blocked socket write.
const DIRECT_CHANNEL_CAPACITY: usize = 64;

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

    let reader = read_requests(Arc::clone(&state), read_half, direct_tx, attach_phase_tx);
    let writer = write_replies(
        write_half,
        broadcast_rx,
        direct_rx,
        attach_phase_rx,
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
    mut read_half: R,
    direct_tx: mpsc::Sender<ServerEvent>,
    attach_phase_tx: watch::Sender<AttachPhase>,
) where
    R: AsyncRead + Unpin,
{
    loop {
        let request: ClientRequest = match read_frame(&mut read_half).await {
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

        let completes_attach = matches!(&request, ClientRequest::Attach { .. });
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
    mut write_half: W,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<ServerEvent>,
    mut direct_rx: mpsc::Receiver<ServerEvent>,
    mut attach_phase_rx: watch::Receiver<AttachPhase>,
    resynchronization_state: Option<Arc<ServerState>>,
) where
    W: AsyncWrite + Unpin,
{
    // This belongs to one connection writer, not the session: it records the
    // newest terminal journal sequence successfully written to this client so
    // a broadcast overrun repairs only genuinely missing panes.
    let mut delivered_terminal_sequences = HashMap::new();

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
                            &mut write_half,
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
            attach_changed = attach_phase_rx.changed() => {
                if attach_changed.is_err() {
                    return;
                }
                continue;
            },
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
                    drain_pending_broadcasts(&mut broadcast_rx, &mut write_half).await;
                    break;
                }
            },
            broadcast_result = broadcast_rx.recv() => match broadcast_result {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("connection lagged behind the session broadcast, skipped {skipped} event(s)");
                    if let Some(state) = &resynchronization_state {
                        if !write_resynchronization(
                            &mut write_half,
                            state,
                            &mut delivered_terminal_sequences,
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
                    &mut write_half,
                    state,
                    &mut delivered_terminal_sequences,
                )
                .await
                {
                    break;
                }
                continue;
            }
        }

        if let Err(error) =
            write_server_event(&mut write_half, &event, &mut delivered_terminal_sequences).await
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
    write_half: &mut W,
    state: &ServerState,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
) -> bool
where
    W: AsyncWrite + Unpin,
{
    for event in handlers::resynchronization_events(state, delivered_terminal_sequences).await {
        if let Err(error) =
            write_server_event(write_half, &event, delivered_terminal_sequences).await
        {
            tracing::warn!(
                "connection write failed during lag resynchronization, closing: {error}"
            );
            return false;
        }
    }
    true
}

/// Writes one event and advances the per-connection output watermark only
/// after the frame reached the socket. Failed writes must not claim delivery.
async fn write_server_event<W>(
    write_half: &mut W,
    event: &ServerEvent,
    delivered_terminal_sequences: &mut HashMap<ilium_core::NodeId, u64>,
) -> Result<(), ilium_ipc::IpcError>
where
    W: AsyncWrite + Unpin,
{
    write_frame(write_half, event).await?;
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
async fn drain_pending_broadcasts<W>(
    broadcast_rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    write_half: &mut W,
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
        if let Err(error) = write_frame(write_half, &event).await {
            tracing::warn!("connection write failed while draining final broadcasts: {error}");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ilium_core::{NodeId, Tree};
    use tokio::io::duplex;
    use tokio::sync::{broadcast, mpsc, watch};
    use tokio::time::{timeout, Duration};

    use super::*;

    /// A live chunk produced during Attach must remain behind the replay
    /// cutover even when it reaches the broadcast receiver first. If it
    /// overtakes replay, `TerminalView::apply_replay` resets the parser and
    /// permanently erases that already-applied newer chunk.
    #[tokio::test]
    async fn live_broadcast_waits_for_complete_attach_replay() {
        let (server_stream, mut client_stream) = duplex(4096);
        let (broadcast_tx, broadcast_rx) = broadcast::channel(8);
        let (direct_tx, direct_rx) = mpsc::channel(8);
        let (attach_phase_tx, attach_phase_rx) = watch::channel(AttachPhase::Replaying);
        let writer = tokio::spawn(write_replies(
            server_stream,
            broadcast_rx,
            direct_rx,
            attach_phase_rx,
            None,
        ));

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

    #[tokio::test]
    async fn broadcast_lag_rebuilds_state_without_retriggering_startup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "lag-repair".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("lag-repair.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
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
            !handlers::initial_state_events(&state, false)
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
