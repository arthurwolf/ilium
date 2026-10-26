use super::*;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn completion_delayed_in_durability_cannot_claim_or_send_into_new_turn() {
    let directory = tempfile::tempdir().expect("isolated completion fixture");
    let (sound_requests, sound_task) = crate::sounds::spawn(
        Arc::new(crate::NoopSoundPlayer),
        crate::execution::test_general_client(),
    );
    let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
        session_name: "delayed-completion".to_string(),
        session_cwd: directory.path().to_path_buf(),
        home_dir: directory.path().to_path_buf(),
        snapshot_path: directory.path().join("snapshot.json"),
        socket_path: directory.path().join("test.sock"),
        detection_config: crate::config::DetectionConfig::default(),
        notifications_config: crate::config::NotificationsConfig::default(),
        sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
        sound_requests,
        custom_signatures: Vec::new(),
        agent_debug_menu_enabled: false,
        progress_monitor_enabled: true,
    }));
    let idle = ilium_core::PaneStatus::from_activity(
        ilium_core::AgentClass::Claude,
        ilium_core::AgentActivity::Idle,
        None,
    );
    let pane_id = {
        let mut tree = state.tree.write().await;
        let project = tree.project_ids()[0];
        let pane = tree
            .add_pane(
                project,
                "completion fixture",
                ilium_core::PaneContentKind::Terminal,
            )
            .expect("terminal node");
        tree.set_pane_status(pane, idle).expect("idle status");
        tree.enqueue_prompt(
            pane,
            QueuedPrompt {
                text: "must remain unsent".to_string(),
                delivery: ilium_core::PromptQueueDelivery::Once,
                attempted_delivery: false,
            },
        )
        .expect("queued request");
        pane
    };
    let session = ilium_pty::PtySession::spawn(
        ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
            .arg("-c")
            .arg("exec cat"),
    )
    .expect("owned isolated PTY");
    let runtime = crate::pane::TerminalPaneRuntime::new(
        session,
        crate::pane::TerminalOrigin::PlainShell,
        None,
        Duration::from_secs(1),
    );
    let epoch = runtime.detection_completion.current();
    let generation = runtime.agent_generation;
    let input = runtime.session.input_handle();
    state
        .panes
        .write()
        .await
        .insert(pane_id, PaneResource::Terminal(Box::new(runtime)));
    let (entered, barrier_entered) = tokio::sync::oneshot::channel();
    let (release, barrier) = tokio::sync::oneshot::channel();
    let delivery_state = Arc::clone(&state);
    let delivery = tokio::spawn(async move {
        deliver_completion_with_barrier(
            &delivery_state,
            pane_id,
            Some((generation, &input, epoch)),
            async {
                entered.send(()).expect("announce durability wait");
                barrier.await.expect("release durability barrier");
                Ok(())
            },
        )
        .await;
    });
    barrier_entered.await.expect("delivery reached durability");
    {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        tree.set_pane_status(
            pane_id,
            ilium_core::PaneStatus::from_activity(
                ilium_core::AgentClass::Claude,
                ilium_core::AgentActivity::Working,
                None,
            ),
        )
        .expect("new active turn");
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            panic!("retained terminal");
        };
        runtime.invalidate_detection_completion();
    }
    release.send(()).expect("durability finished");
    delivery.await.expect("completion follow-up");
    let tree = state.tree.read().await;
    let head = tree
        .next_queued_prompt(pane_id)
        .expect("queued head")
        .expect("retained request");
    assert!(
        head.attempted_delivery,
        "unsent stale attempt remains fenced"
    );
    drop(tree);
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        panic!("retained terminal");
    };
    assert!(!runtime.detection_completion.is_claimed_current(epoch));
    assert!(!runtime
        .session
        .screen_snapshot()
        .text
        .contains("must remain unsent"));
    runtime.session.kill().expect("owned PTY cleanup");
    drop(panes);
    sound_task.abort();
}
