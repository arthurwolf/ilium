//! IPC layer: accepts connections on the session's endpoint and drives each one
//! (see [`connection`]), dispatching requests through [`handlers`].

mod connection;
mod direct_events;
pub(crate) use direct_events::DirectEventSender;
pub(crate) use direct_events::EventReply;
// `pub(crate)`, not private: `crate::run`'s crash-recovery restore path
// (in `lib.rs`) calls `handlers::spawn_and_register_pane` directly, the
// same function `handle_new_pane` uses for a live client's `NewPane`
// request -- see that function's doc comment for why the two share it.
pub(crate) mod handlers;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use ilium_transport::{is_transient_accept_error, SessionListener};

use crate::state::ServerState;

/// Maximum number of accepted IPC streams owned by one session. This is a
/// socket/task bound, independent of the execution bank's client/job quotas.
/// Leave room for several interactive attaches and short-lived CLI commands;
/// a peer that occupies a slot indefinitely can still delay later clients.
const MAX_CONCURRENT_CONNECTIONS: usize = 64;

/// Accepts connections on `listener` forever, spawning at most
/// `MAX_CONCURRENT_CONNECTIONS` tracked tasks. Capacity is acquired before
/// `accept`, so a saturated session leaves additional peers in the transport
/// backlog (Unix) or waiting for a named-pipe instance (Windows); it never
/// accepts a stream and then discards its already-sent bytes. A transient
/// per-attempt accept failure is retried. Other failures end the loop so
/// `run` can shut down the session.
pub async fn accept_loop(state: Arc<ServerState>, listener: SessionListener) {
    accept_loop_with_limit(state, listener, MAX_CONCURRENT_CONNECTIONS).await;
}

async fn accept_loop_with_limit(
    state: Arc<ServerState>,
    mut listener: SessionListener,
    connection_limit: usize,
) {
    if connection_limit == 0 {
        tracing::error!("IPC connection limit must be positive");
        return;
    }

    loop {
        wait_for_connection_slot(&state, connection_limit).await;
        let stream = match listener.accept().await {
            Ok(accepted) => accepted,
            // A peer that aborts/resets its connection mid-handshake (or an
            // interrupted syscall) is a per-connection blip, not a problem
            // with the listener itself -- retry accepting. Anything else
            // (e.g. EMFILE/ENFILE from fd exhaustion, or the listener socket
            // going away) is unrecoverable: looping on it would busy-spin
            // forever instead of honoring this function's documented
            // contract of returning so `run`'s top-level select! can shut
            // the session down.
            Err(error) if is_transient_accept_error(&error) => {
                tracing::warn!("transient error accepting a connection, retrying: {error}");
                continue;
            }
            Err(error) => {
                tracing::error!(
                    "failed to accept a connection, listener is no longer usable: {error}"
                );
                return;
            }
        };

        let connection_state = Arc::clone(&state);
        tracing::info!(session_name = %state.session_name, "client connection accepted");
        let handle = tokio::spawn(async move {
            connection::handle(connection_state, stream).await;
        });
        state.track_connection_task(handle);
    }
}

/// A `JoinHandle` is the existing shutdown owner of a connection task. Poll
/// those handles when the registry reaches its limit; Tokio wakes this future
/// when any task completes. Removing a completed handle before the next
/// `accept` keeps both live owners and retained/retiring handles within the
/// same finite limit, including the interval after a task drops its stream but
/// before its JoinHandle reports completion. No polling timer or second task
/// per connection is needed.
async fn wait_for_connection_slot(state: &ServerState, connection_limit: usize) {
    let mut reported_saturation = false;
    std::future::poll_fn(|context| {
        // Match the registry's insertion contract: poisoning means a prior
        // in-memory registry operation panicked, which is unrecoverable here.
        let mut tasks = state.connection_tasks.lock().unwrap();
        let mut index = 0;
        while index < tasks.len() {
            // Poll even a handle that already reports `is_finished()`: that
            // observation cannot reveal whether its task panicked or was
            // cancelled. Only one poll may consume the original JoinError.
            match Pin::new(&mut tasks[index]).poll(context) {
                Poll::Ready(result) => {
                    if let Err(error) = result {
                        tracing::warn!(session_name = %state.session_name, %error, "IPC connection task ended abnormally");
                    }
                    drop(tasks.swap_remove(index));
                }
                Poll::Pending => index += 1,
            }
        }

        if tasks.len() < connection_limit {
            if reported_saturation {
                tracing::info!(
                    session_name = %state.session_name,
                    tracked_connections = tasks.len(),
                    connection_limit,
                    "IPC connection capacity resumed"
                );
            }
            Poll::Ready(())
        } else {
            if !reported_saturation {
                tracing::warn!(
                    session_name = %state.session_name,
                    tracked_connections = tasks.len(),
                    connection_limit,
                    "IPC connection limit reached; waiting before accepting another stream"
                );
                reported_saturation = true;
            }
            Poll::Pending
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ilium_core::NodeId;
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    use ilium_transport::{SessionEndpoint, SessionStream};
    use tokio::task::JoinHandle;

    use super::*;
    use crate::config::{DetectionConfig, NotificationsConfig};
    use crate::state::ServerStateOptions;

    const TEST_DEADLINE: Duration = Duration::from_secs(5);

    struct TestListener {
        _directory: tempfile::TempDir,
        endpoint: SessionEndpoint,
        state: Arc<ServerState>,
        accept_task: JoinHandle<()>,
        sound_task: JoinHandle<()>,
    }

    impl TestListener {
        async fn start(connection_limit: usize) -> Self {
            // The isolated runner supplies a short temporary directory for
            // socket identities; transport owns platform-specific handling.
            let parent = std::env::temp_dir();
            let directory = tempfile::Builder::new()
                .prefix("ia")
                .tempdir_in(parent)
                .expect("short temporary socket directory");
            let socket_path = directory.path().join("admission.sock");
            let endpoint = SessionEndpoint::from_path(&socket_path);
            let listener = endpoint.bind().await.expect("bind test endpoint");
            let (sound_requests, sound_task) = crate::sounds::spawn(
                Arc::new(crate::NoopSoundPlayer),
                crate::execution::test_general_client(),
            );
            let state = Arc::new(ServerState::new(ServerStateOptions {
                session_name: "admission-test".to_string(),
                session_cwd: directory.path().to_path_buf(),
                home_dir: directory.path().to_path_buf(),
                snapshot_path: directory.path().join("snapshot.json"),
                socket_path,
                detection_config: DetectionConfig::default(),
                notifications_config: NotificationsConfig::default(),
                sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
                sound_requests,
                custom_signatures: Vec::new(),
                agent_debug_menu_enabled: false,
                progress_monitor_enabled: true,
            }));
            let loop_state = Arc::clone(&state);
            // Mirrors `run`'s outer shutdown select: a saturated accept loop
            // must remain cancellable even though it does not call accept.
            let accept_task = tokio::spawn(async move {
                tokio::select! {
                    () = accept_loop_with_limit(Arc::clone(&loop_state), listener, connection_limit) => {}
                    () = loop_state.shutdown.notified() => {}
                }
            });
            Self {
                _directory: directory,
                endpoint,
                state,
                accept_task,
                sound_task,
            }
        }

        async fn connect(&self) -> SessionStream {
            tokio::time::timeout(TEST_DEADLINE, self.endpoint.connect())
                .await
                .expect("connect did not complete")
                .expect("connect to test endpoint")
        }

        fn tracked_count(&self) -> usize {
            self.state.connection_tasks.lock().unwrap().len()
        }
    }

    impl Drop for TestListener {
        fn drop(&mut self) {
            self.accept_task.abort();
            self.state.abort_all_connection_tasks();
            self.sound_task.abort();
        }
    }

    async fn attach(stream: &mut SessionStream) -> Vec<ServerEvent> {
        write_frame(
            stream,
            &ClientRequest::Attach {
                session: "admission-test".to_string(),
            },
        )
        .await
        .expect("send Attach");
        read_attach_events(stream).await
    }

    async fn read_attach_events(stream: &mut SessionStream) -> Vec<ServerEvent> {
        tokio::time::timeout(TEST_DEADLINE, async {
            let mut events = Vec::new();
            loop {
                let event: ServerEvent = read_frame(stream).await.expect("read attach event");
                let is_complete = matches!(event, ServerEvent::InitialStateSyncComplete);
                events.push(event);
                if is_complete {
                    return events;
                }
            }
        })
        .await
        .expect("attach reply did not complete")
    }

    #[tokio::test]
    async fn full_connection_registry_preserves_queued_bytes_then_reaps_and_reaccepts() {
        let listener = TestListener::start(1).await;
        let mut admitted = listener.connect().await;
        assert!(matches!(
            attach(&mut admitted).await.first(),
            Some(ServerEvent::PaneStateSnapshot { .. })
        ));

        for cycle in 0..4 {
            let mut queued = listener.connect().await;
            let wrong_session = format!("wrong-session-{cycle}");
            // Both frames are submitted while the previous admitted stream
            // occupies the only slot. Their bytes must survive in transport.
            write_frame(
                &mut queued,
                &ClientRequest::Attach {
                    session: wrong_session.clone(),
                },
            )
            .await
            .expect("queue wrong-session Attach");
            write_frame(
                &mut queued,
                &ClientRequest::Attach {
                    session: "admission-test".to_string(),
                },
            )
            .await
            .expect("queue valid Attach");
            assert_eq!(listener.tracked_count(), 1, "cycle {cycle} exceeded limit");
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(150),
                    read_frame::<ServerEvent, _>(&mut queued),
                )
                .await
                .is_err(),
                "cycle {cycle} received a reply before capacity was released"
            );

            write_frame(&mut admitted, &ClientRequest::Detach)
                .await
                .expect("detach admitted connection");
            let admitted_end =
                tokio::time::timeout(TEST_DEADLINE, read_frame::<ServerEvent, _>(&mut admitted))
                    .await
                    .expect("admitted connection did not close");
            assert!(admitted_end.is_err(), "Detach must end the prior stream");

            let first_reply: ServerEvent =
                tokio::time::timeout(TEST_DEADLINE, read_frame(&mut queued))
                    .await
                    .expect("queued request was not admitted")
                    .expect("read queued error reply");
            assert!(
                matches!(first_reply, ServerEvent::Error { message } if message.contains(wrong_session.as_str())),
                "cycle {cycle} lost or reordered its first request"
            );
            let remaining = read_attach_events(&mut queued).await;
            assert!(matches!(
                remaining.first(),
                Some(ServerEvent::PaneStateSnapshot { .. })
            ));
            assert_eq!(
                listener.tracked_count(),
                1,
                "cycle {cycle} retained a completed handle"
            );
            admitted = queued;
        }
    }

    #[tokio::test]
    async fn shutdown_wakes_while_all_connection_slots_are_occupied() {
        let mut listener = TestListener::start(1).await;
        let mut admitted = listener.connect().await;
        attach(&mut admitted).await;
        assert!(
            listener.state.has_terminal_subscribers(NodeId(999)),
            "legacy Attach must own an all-pane terminal subscription"
        );
        let mut queued = listener.connect().await;
        write_frame(
            &mut queued,
            &ClientRequest::Attach {
                session: "admission-test".to_string(),
            },
        )
        .await
        .expect("queue request during saturation");
        assert_eq!(listener.tracked_count(), 1);

        listener.state.shutdown.notify_one();
        tokio::time::timeout(TEST_DEADLINE, &mut listener.accept_task)
            .await
            .expect("shutdown did not cancel saturated accept")
            .expect("accept task panicked");
        listener.state.abort_all_connection_tasks();
        let admitted_end =
            tokio::time::timeout(TEST_DEADLINE, read_frame::<ServerEvent, _>(&mut admitted))
                .await
                .expect("aborted admitted peer remained open");
        assert!(admitted_end.is_err());
        assert!(
            !listener.state.has_terminal_subscribers(NodeId(999)),
            "aborting the joined connection must drop its terminal subscription"
        );
        assert_eq!(
            listener.tracked_count(),
            1,
            "the aborted owner remains tracked until admission reaps its handle"
        );

        // The old listener is gone, so bind a replacement endpoint with the
        // same state. The old aborted JoinHandle must be reaped before a new
        // stream gets the only slot; no test-side registry clearing helps it.
        drop(admitted);
        drop(queued);
        let replacement = listener.endpoint.bind().await.expect("rebind endpoint");
        let replacement_state = Arc::clone(&listener.state);
        listener.accept_task = tokio::spawn(async move {
            accept_loop_with_limit(replacement_state, replacement, 1).await;
        });
        let mut fresh = listener.connect().await;
        let fresh_attach = attach(&mut fresh).await;
        assert!(matches!(
            fresh_attach.first(),
            Some(ServerEvent::PaneStateSnapshot { .. })
        ));
        assert_eq!(
            listener.tracked_count(),
            1,
            "aborted handle was not reaped before replacement admission"
        );
        assert!(listener.state.has_terminal_subscribers(NodeId(999)));
    }
}
