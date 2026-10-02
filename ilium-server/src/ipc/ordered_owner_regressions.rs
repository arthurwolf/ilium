async fn input_for(state: &ServerState, pane_id: NodeId) -> PtyInput {
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        panic!("test terminal missing");
    };
    runtime.session.input_handle()
}

async fn collect_forwarded_until_closed(
    state: Arc<ServerState>,
    pane_id: NodeId,
    input: PtyInput,
    status: OwnerStatus,
) -> Vec<ServerEvent> {
    let (status_sender, status_receiver) = tokio::sync::watch::channel(status);
    let (output_sender, output_receiver) = tokio::sync::broadcast::channel(4);
    let mut events = state.events.subscribe();
    output_sender
        .send(ilium_pty::PtyOutputChunk {
            sequence: 987,
            bytes: Arc::from(b"FINAL-OWNER-BYTES".as_slice()),
        })
        .unwrap();
    drop(output_sender);
    tokio::time::timeout(
        Duration::from_secs(2),
        forward_output_with_owner_status(state, pane_id, output_receiver, input, status_receiver),
    )
    .await
    .expect("forwarder must drain final bytes and close");
    drop(status_sender);
    let mut captured = Vec::new();
    while let Ok(event) = events.try_recv() {
        captured.push(event);
    }
    captured
}

#[tokio::test(flavor = "current_thread")]
async fn fatal_owner_status_is_reported_once_without_input_and_final_bytes_are_forwarded() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("owner-fatal-forward").await;
    state.replace_terminal_subscriptions(
        false,
        &std::collections::HashSet::new(),
        false,
        &std::collections::HashSet::from([pane_id]),
    );
    let input = input_for(&state, pane_id).await;
    let events = collect_forwarded_until_closed(
        Arc::clone(&state),
        pane_id,
        input,
        OwnerStatus::Stopped {
            reason: ShutdownReason::WriterFailed,
            error: None,
        },
    )
    .await;
    let errors = events
        .iter()
        .filter(|event| {
            matches!(event, ServerEvent::Error { message }
        if message.contains("Terminal input/output stopped"))
        })
        .count();
    let final_bytes = events.iter().any(|event| {
        matches!(event,
        ServerEvent::ScreenUpdate { sequence: 987, bytes, .. } if bytes == b"FINAL-OWNER-BYTES")
    });
    let still_current = input_for(&state, pane_id).await.status();
    teardown_state_panes(&state);
    assert_eq!(errors, 1);
    assert!(
        final_bytes,
        "terminal status must not discard buffered output"
    );
    assert!(
        matches!(still_current, OwnerStatus::Running),
        "diagnostic forwarding must not kill a child"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn changed_owner_status_wakes_a_forwarder_with_no_output_or_keyboard_input() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("owner-status-wakeup").await;
    let input = input_for(&state, pane_id).await;
    let (sender, receiver) = tokio::sync::watch::channel(OwnerStatus::Running);
    let (output_sender, output_receiver) = tokio::sync::broadcast::channel(4);
    let mut events = state.events.subscribe();
    let task = tokio::spawn(forward_output_with_owner_status(
        Arc::clone(&state),
        pane_id,
        output_receiver,
        input,
        receiver,
    ));
    // First observe that Running and an open idle output stream leave the
    // task pending. Then only a status change can wake it to emit the error.
    tokio::task::yield_now().await;
    sender.send_replace(OwnerStatus::Stopped {
        reason: ShutdownReason::ParserUnavailable,
        error: None,
    });
    let received = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(ServerEvent::Error { message }) = events.recv().await {
                if message.contains("ParserUnavailable") {
                    break;
                }
            }
        }
    })
    .await;
    sender.send_replace(OwnerStatus::Stopped {
        reason: ShutdownReason::ParserUnavailable,
        error: None,
    });
    drop(output_sender);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    let repeated = std::iter::from_fn(|| events.try_recv().ok()).any(|event|
        matches!(event, ServerEvent::Error { message } if message.contains("ParserUnavailable")));
    teardown_state_panes(&state);
    assert!(
        received.is_ok(),
        "failure must surface without a later input request"
    );
    assert!(!repeated, "one lifetime failure must be announced once");
}

#[tokio::test(flavor = "current_thread")]
async fn eof_requested_close_and_stale_runtime_do_not_report_owner_failure() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("owner-status-normal").await;
    let input = input_for(&state, pane_id).await;
    for reason in [ShutdownReason::Eof, ShutdownReason::Requested] {
        let events = collect_forwarded_until_closed(
            Arc::clone(&state),
            pane_id,
            input.clone(),
            OwnerStatus::Stopped {
                reason,
                error: None,
            },
        )
        .await;
        assert!(!events
            .iter()
            .any(|event| matches!(event, ServerEvent::Error { message }
            if message.contains("Terminal input/output stopped"))));
    }
    // A different live PTY at the same NodeId must not inherit an old error.
    let (other, other_id, _other_directory) =
        state_with_one_terminal_pane("owner-status-replacement").await;
    let replacement = other.panes.write().await.remove(&other_id).unwrap();
    let previous = state
        .panes
        .write()
        .await
        .insert(pane_id, replacement)
        .unwrap();
    let events = collect_forwarded_until_closed(
        Arc::clone(&state),
        pane_id,
        input,
        OwnerStatus::Stopped {
            reason: ShutdownReason::ReaderFailed,
            error: None,
        },
    )
    .await;
    teardown_pane_resource(pane_id, previous);
    teardown_state_panes(&state);
    teardown_state_panes(&other);
    assert!(!events
        .iter()
        .any(|event| matches!(event, ServerEvent::Error { message }
        if message.contains("Terminal input/output stopped"))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, ServerEvent::ScreenUpdate { sequence: 987, .. })));
}

#[tokio::test(flavor = "current_thread")]
async fn queued_prompt_attempt_is_persisted_before_success_and_restored_without_replay() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("queue-attempt-durable").await;
    let prompt = QueuedPrompt {
        text: "authored-queue-message".into(),
        delivery: ilium_core::PromptQueueDelivery::Once,
        attempted_delivery: false,
    };
    state
        .tree
        .write()
        .await
        .enqueue_prompt(pane_id, prompt.clone())
        .unwrap();
    crate::prompt_queue::deliver_next_after_completion(&state, pane_id).await;
    assert!(state
        .tree
        .read()
        .await
        .next_queued_prompt(pane_id)
        .unwrap()
        .is_none());
    // This helper starts no debounced snapshot writer: the disk image is the
    // authoritative pre-I/O barrier, whereas live state acknowledged success.
    let snapshot = crate::persistence::load_snapshot(&state.snapshot_path)
        .await
        .unwrap()
        .unwrap();
    let retained = snapshot
        .tree
        .next_queued_prompt(pane_id)
        .unwrap()
        .unwrap()
        .clone();
    assert_eq!(retained.text, prompt.text);
    assert!(retained.attempted_delivery);
    *state.tree.write().await = snapshot.tree;
    let input = input_for(&state, pane_id).await;
    crate::prompt_queue::deliver_next_after_completion(&state, pane_id).await;
    assert_eq!(
        state.tree.read().await.next_queued_prompt(pane_id).unwrap(),
        Some(&retained)
    );
    assert_eq!(input.queue_load().commands, 0);
    teardown_state_panes(&state);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_queue_attempt_persistence_prevents_body_and_enter_and_retains_authored_text() {
    let (state, pane_id, _directory) =
        state_with_one_terminal_pane("queue-attempt-write-failure").await;
    tokio::fs::create_dir(&state.snapshot_path).await.unwrap();
    let prompt = QueuedPrompt {
        text: "MUST-NOT-REACH-PTY".into(),
        delivery: ilium_core::PromptQueueDelivery::Once,
        attempted_delivery: false,
    };
    state
        .tree
        .write()
        .await
        .enqueue_prompt(pane_id, prompt.clone())
        .unwrap();
    crate::prompt_queue::deliver_next_after_completion(&state, pane_id).await;
    let kept = state
        .tree
        .read()
        .await
        .next_queued_prompt(pane_id)
        .unwrap()
        .unwrap()
        .clone();
    let screen = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!()
        };
        runtime.session.screen_text()
    };
    teardown_state_panes(&state);
    assert_eq!(kept.text, prompt.text);
    assert!(kept.attempted_delivery);
    assert!(!screen.contains(&prompt.text));
}

#[test]
fn persisted_prompt_attempt_round_trip_preserves_text_and_old_snapshots_default_to_unattempted() {
    let old = r#"{"text":"authored text","delivery":"Once"}"#;
    let unattempted: QueuedPrompt = serde_json::from_str(old).unwrap();
    assert!(!unattempted.attempted_delivery);
    let mut attempted = unattempted;
    attempted.attempted_delivery = true;
    let restored: QueuedPrompt =
        serde_json::from_slice(&serde_json::to_vec(&attempted).unwrap()).unwrap();
    assert_eq!(restored, attempted);
}

#[tokio::test(flavor = "current_thread")]
async fn current_screen_reader_uses_live_pty_and_closed_panes_return_none() {
    let (state, pane_id, _directory) = state_with_one_terminal_pane("current-screen-read").await;
    let current =
        crate::pane::read_current_terminal_screen(&state, pane_id, vt100::Screen::size).await;
    let missing =
        crate::pane::read_current_terminal_screen(&state, NodeId(u64::MAX), vt100::Screen::size)
            .await;
    teardown_state_panes(&state);
    let closed =
        crate::pane::read_current_terminal_screen(&state, pane_id, vt100::Screen::size).await;
    assert_eq!(
        current,
        Some((
            crate::pane::DEFAULT_PANE_ROWS,
            crate::pane::DEFAULT_PANE_COLS
        ))
    );
    assert_eq!(missing, None);
    assert_eq!(closed, None);
}
