//! These tests use the real shared OS worker bank and isolated owned files.
use super::boards::BoardFiles;
use super::configuration::ConfigurationChange;
use crate::app::{App, Mode};
use ilium_platform::file_lock::ExclusiveFileLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn configuration_command_normalizes_spare_capacity_before_retention() {
    let mut value = crate::config::GitSettings {
        setup_command: String::with_capacity(2 * 1024 * 1024),
        ..Default::default()
    };
    value.setup_command.push_str("true");
    let normalized = ConfigurationChange::Git(value).normalize().unwrap();
    let ConfigurationChange::Git(value) = &normalized else {
        panic!("wrong command");
    };
    assert!(value.setup_command.capacity() < 1024);
    normalized.checked_bytes().unwrap();
}

#[test]
fn frozen_agent_icon_capacity_is_included_in_ui_configuration_admission() {
    let mut settings = crate::config::UiSettings::default();
    let baseline_capacity = settings.icons.frozen_agent.capacity();
    let baseline_bytes = ConfigurationChange::Ui(Box::new(settings.clone()))
        .checked_bytes()
        .unwrap();

    settings.icons.frozen_agent.reserve(256);
    let expanded_capacity = settings.icons.frozen_agent.capacity();
    let expanded_bytes = ConfigurationChange::Ui(Box::new(settings))
        .checked_bytes()
        .unwrap();

    assert!(expanded_capacity >= baseline_capacity + 256);
    assert_eq!(
        expanded_bytes - baseline_bytes,
        expanded_capacity - baseline_capacity
    );
}

#[test]
fn text_trigger_modal_waits_for_real_durable_writer_and_server_request() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new("filesystem-fixture".into(), directory.path().to_path_buf());
    app.config_dir = Some(directory.path().to_path_buf());
    let trigger = ilium_ipc::TextTrigger {
        id: "authored-worker".into(),
        regexp: "ready$".into(),
        message: "continue".into(),
        ..Default::default()
    };
    let state = crate::text_trigger_dialog::TextTriggerDialogState::new(None);
    let mut state = state;
    state.regexp.buf = trigger.regexp.clone();
    state.message.buf = trigger.message.clone();
    let trigger = state.candidate();
    app.mode = Mode::TextTriggerDialog(Box::new(state));
    let lock = ExclusiveFileLock::acquire(&directory.path().join(".agent-detection-settings.lock"))
        .unwrap();
    let started = Instant::now();
    app.commit_text_trigger(None, None, trigger).unwrap();
    app.collect_editor_files();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(matches!(app.mode, Mode::TextTriggerDialog(_)));
    assert!(!app
        .take_outbound_requests()
        .iter()
        .any(|request| matches!(request, ilium_ipc::ClientRequest::UpdateTextTriggers { .. })));
    drop(lock);
    app.settle_filesystem_for_test();
    assert!(!matches!(app.mode, Mode::TextTriggerDialog(_)));
    assert!(app
        .take_outbound_requests()
        .iter()
        .any(|request| matches!(request, ilium_ipc::ClientRequest::UpdateTextTriggers { .. })));
    let saved = crate::config::load(directory.path()).unwrap();
    assert_eq!(saved.text_triggers.triggers.len(), 1);
    assert_eq!(saved.text_triggers.triggers[0].regexp, "ready$");
}

#[test]
fn board_ordered_snapshots_advance_only_after_real_publication() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("board.md");
    let storage = ilium_core::BoardStorage::MarkdownFile { path: path.clone() };
    let source = crate::board::read_source(storage, true).unwrap();
    let mut files = BoardFiles::new(
        crate::execution::test_client(),
        Arc::new(tokio::sync::Notify::new()),
    );
    let mut board = files.attach(source).unwrap();
    let lock =
        ExclusiveFileLock::acquire(&directory.path().join(".board.md.ilium-write.lock")).unwrap();
    board.add_card("first".into()).unwrap();
    board.add_card("second".into()).unwrap();
    assert_eq!(board.content_revision(), 0);
    assert!(files.poll_write().is_none());
    assert!(!std::fs::read_to_string(&path).unwrap().contains("first"));
    drop(lock);
    let deadline = Instant::now() + Duration::from_secs(10);
    while files.pending() != 0 {
        if let Some(ack) = files.poll_write() {
            board.acknowledge_revision(ack.result.unwrap());
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(board.content_revision(), 2);
    let saved = crate::board::read_source(board.storage.clone(), false).unwrap();
    assert_eq!(saved.columns[0].cards.len(), 2);
    assert_eq!(saved.columns[0].cards[0].title, "first");
    assert_eq!(saved.columns[0].cards[1].title, "second");
}

#[test]
fn board_conflict_preserves_foreign_file_and_newer_authored_updates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("board.md");
    let storage = ilium_core::BoardStorage::MarkdownFile { path: path.clone() };
    let source = crate::board::read_source(storage, true).unwrap();
    let mut files = BoardFiles::new(
        crate::execution::test_client(),
        Arc::new(tokio::sync::Notify::new()),
    );
    let mut board = files.attach(source).unwrap();
    board.add_card("authored one".into()).unwrap();
    board.add_card("authored two".into()).unwrap();
    let external = "## Foreign\n\n- preserve me\n";
    std::fs::write(&path, external).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut failures = 0;
    while files.pending() != 0 {
        if let Some(ack) = files.poll_write() {
            assert!(ack.result.is_err());
            failures += 1;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(failures, 2);
    assert_eq!(board.content_revision(), 0);
    assert_eq!(board.columns[0].cards.len(), 2);
    assert_eq!(std::fs::read_to_string(path).unwrap(), external);
}

#[test]
fn isolated_board_conflict_rolls_back_only_exact_revision_and_retains_authored_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("board.md");
    let source = crate::board::read_source(
        ilium_core::BoardStorage::MarkdownFile { path: path.clone() },
        true,
    )
    .unwrap();
    let mut files = BoardFiles::new(
        crate::execution::test_client(),
        Arc::new(tokio::sync::Notify::new()),
    );
    let mut board = files.attach(source).unwrap();
    board
        .add_card("keep this failed authored intent".into())
        .unwrap();
    let external = "## Foreign\n\n- untouched\n";
    std::fs::write(&path, external).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(ack) = files.poll_write() {
            let error = ack.result.unwrap_err();
            assert!(error.unchanged);
            board.apply_failed_rollback(ack.revision.unwrap(), ack.rollback.unwrap());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(board.columns[0].cards.is_empty());
    assert_eq!(
        board.failed_authored_columns.as_ref().unwrap()[0].cards[0].title,
        "keep this failed authored intent"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), external);
}

#[test]
fn board_read_retains_exact_target_under_overload_and_deduplicates() {
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, JobCost, Lane, LaneConfig, QuotaGroup,
        QuotaLimits, ShutdownMode,
    };
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 64 * 1024 * 1024,
        result_bytes: 8 * 1024 * 1024,
        worker_threads: 2,
        worker_bytes: 1024 * 1024,
    });
    let lane = |threads| LaneConfig {
        threads,
        queue_slots: 1,
        priority: None,
        resident_bytes_per_thread: 4096,
    };
    let mut execution = Execution::start(
        quota,
        ExecutionConfig {
            cpu: lane(1),
            io: lane(1),
            service: LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            },
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 64 * 1024 * 1024,
            result_bytes: 8 * 1024 * 1024,
        })
        .unwrap();
    let held = client
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 16 * 1024 * 1024,
                result_bytes: 2 * 1024 * 1024,
            },
        )
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("retained.md");
    let storage = ilium_core::BoardStorage::MarkdownFile { path: path.clone() };
    let target = super::boards::BoardLoadTarget::Pane(ilium_core::NodeId(321), storage.clone());
    let mut files = BoardFiles::new(client, Arc::new(tokio::sync::Notify::new()));
    files
        .request(target.clone(), storage.clone(), true)
        .unwrap();
    files.request(target, storage, true).unwrap();
    assert_eq!(files.pending(), 1);
    assert!(files.poll_read().is_none());
    assert!(!path.exists());
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(loaded) = files.poll_read() {
            let source = loaded.result.unwrap();
            assert_eq!(source.storage.path(), path);
            assert!(path.exists());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retained board read never resumed"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(files.pending(), 0);
    drop(files);
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
}

#[test]
fn integration_append_is_nonblocking_ordered_and_acknowledges_readback() {
    use super::integrations::{IntegrationFiles, IntegrationIntent, RoomTarget};
    use super::ordered::WriteCompletion;
    use ilium_execution::JobOutcome;
    let directory = tempfile::tempdir().unwrap();
    crate::chatroom::initialize(directory.path()).unwrap();
    let room = RoomTarget {
        id: ilium_core::NodeId(42),
        path: directory.path().to_path_buf(),
    };
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.path().join(".ilium/chatroom.lock"))
        .unwrap();
    lock.lock().unwrap();
    let mut files = IntegrationFiles::new(
        crate::execution::test_client(),
        Arc::new(tokio::sync::Notify::new()),
    );
    let started = Instant::now();
    files
        .enqueue(IntegrationIntent::Append {
            room: room.clone(),
            draft: "first authored message".into(),
        })
        .unwrap();
    assert!(files.poll().is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(crate::chatroom::read_messages(directory.path(), 200)
        .unwrap()
        .is_empty());
    // The first intent remains pending while the worker waits on the actual
    // filesystem lock. A retry cannot admit a duplicate append.
    assert!(files
        .enqueue(IntegrationIntent::Append {
            room: room.clone(),
            draft: "first authored message".into()
        })
        .is_err());
    drop(lock);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some((intent, completion)) = files.poll() {
            assert!(matches!(intent, IntegrationIntent::Append { .. }));
            let WriteCompletion::Outcome { outcome, .. } = completion else {
                panic!("append lost");
            };
            let JobOutcome::Finished(Ok(result)) = outcome.view() else {
                panic!("append failed");
            };
            let (_, messages) = result.rooms[0].result.as_ref().unwrap();
            assert_eq!(messages.as_ref().unwrap().len(), 1);
            break;
        }
        assert!(Instant::now() < deadline, "append never acknowledged");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        crate::chatroom::read_messages(directory.path(), 200)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(files.pending(), 0);
}

#[test]
fn chatroom_fingerprint_is_returned_by_ordered_io_job() {
    use super::integrations::{IntegrationFiles, IntegrationIntent, RoomTarget};

    let directory = tempfile::tempdir().unwrap();
    crate::chatroom::initialize(directory.path()).unwrap();
    let room = RoomTarget {
        id: ilium_core::NodeId(43),
        path: directory.path().to_path_buf(),
    };
    let mut files = IntegrationFiles::new(
        crate::execution::test_client(),
        Arc::new(tokio::sync::Notify::new()),
    );

    files
        .enqueue(IntegrationIntent::CheckRoom(room.clone()))
        .unwrap();
    let first = collect_room_check(&mut files);
    let expected_first = std::fs::metadata(crate::chatroom::path_for_project(&room.path)).unwrap();
    assert_eq!(first.0, room.id);
    assert_eq!(first.1, expected_first.len());

    crate::chatroom::append_message(directory.path(), "external", "second message").unwrap();
    files
        .enqueue(IntegrationIntent::CheckRoom(room.clone()))
        .unwrap();
    let second = collect_room_check(&mut files);
    assert_eq!(second.0, room.id);
    assert!(
        second.1 > first.1,
        "changed room must have a new fingerprint"
    );
    assert_eq!(files.pending(), 0);
}

fn collect_room_check(
    files: &mut super::integrations::IntegrationFiles,
) -> (ilium_core::NodeId, u64) {
    use super::integrations::IntegrationIntent;
    use super::ordered::WriteCompletion;
    use ilium_execution::JobOutcome;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((intent, completion)) = files.poll() {
            assert!(matches!(intent, IntegrationIntent::CheckRoom(_)));
            let WriteCompletion::Outcome { outcome, .. } = completion else {
                panic!("room check receipt was lost");
            };
            let JobOutcome::Finished(Ok(result)) = outcome.view() else {
                panic!("room check failed");
            };
            let room = result.rooms.first().expect("room result");
            assert!(room.result.as_ref().unwrap().0);
            let (_, length) = room.stamp.expect("room stamp");
            return (room.room.id, length);
        }
        assert!(Instant::now() < deadline, "room check did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn sidebar_worker_prepares_only_expanded_rows_and_fences_removed_roots() {
    let directory = tempfile::tempdir().unwrap();
    let nested = directory.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let file = nested.join("authored.rs");
    std::fs::write(&file, "fn authored() {}\n").unwrap();
    let mut tree = ilium_core::Tree::new();
    let group = tree.add_group(ilium_core::ROOT_ID, "fixture").unwrap();
    let folder = tree
        .add_folder(group, directory.path().to_path_buf())
        .unwrap();
    let nested_id = crate::tree_ui::virtual_folder_node_id(folder, &nested);
    let file_id = crate::tree_ui::virtual_folder_node_id(folder, &file);
    let closed = super::sidebar::prepared_for_test(&tree, &std::collections::HashSet::new());
    assert!(closed.view().rows.is_empty());
    drop(closed);
    let snapshot = super::sidebar::prepared_for_test(
        &tree,
        &std::collections::HashSet::from([vec![group, folder], vec![group, folder, nested_id]]),
    );
    let entry = crate::tree_ui::folder_entry(&tree, file_id, snapshot.view()).unwrap();
    assert_eq!(entry.path, file);
    assert_eq!(
        entry.identifier_path,
        vec![group, folder, nested_id, file_id]
    );
    tree.remove_node(folder).unwrap();
    assert!(crate::tree_ui::folder_entry(&tree, file_id, snapshot.view()).is_none());
    assert_eq!(
        std::fs::read_to_string(entry.path).unwrap(),
        "fn authored() {}\n"
    );
}

fn isolated_load_execution() -> (
    ilium_execution::Execution,
    ilium_execution::Client,
    ilium_execution::QuotaGroup,
) {
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
    };
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 8,
        service_jobs: 0,
        input_bytes: 256 * 1024 * 1024,
        result_bytes: 128 * 1024 * 1024,
        worker_threads: 1,
        worker_bytes: 640 * 1024 * 1024,
    });
    let bank = |threads| LaneConfig {
        threads,
        queue_slots: if threads == 0 { 0 } else { 4 },
        priority: None,
        resident_bytes_per_thread: if threads == 0 { 0 } else { 1024 * 1024 },
    };
    let owner = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: bank(0),
            io: bank(1),
            service: bank(0),
        },
    )
    .unwrap();
    let client = owner
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 256 * 1024 * 1024,
            result_bytes: 128 * 1024 * 1024,
        })
        .unwrap();
    (owner, client, quota)
}

fn isolated_editor_save_execution() -> (ilium_execution::Execution, ilium_execution::Client) {
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
    };
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 8,
        service_jobs: 0,
        input_bytes: 192 * 1024 * 1024,
        result_bytes: 128 * 1024 * 1024,
        worker_threads: 2,
        worker_bytes: 32 * 1024 * 1024,
    });
    let bank = LaneConfig {
        threads: 1,
        queue_slots: 4,
        priority: None,
        resident_bytes_per_thread: 1024 * 1024,
    };
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let owner = Execution::start(
        quota,
        ExecutionConfig {
            cpu: bank,
            io: bank,
            service: disabled,
        },
    )
    .unwrap();
    let client = owner
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 192 * 1024 * 1024,
            result_bytes: 128 * 1024 * 1024,
        })
        .unwrap();
    (owner, client)
}

#[test]
fn editor_snapshot_is_captured_on_cpu_and_durably_read_back_before_ack() {
    use super::editors::{EditorCompletion, EditorFiles, SavePurpose, SaveTarget};
    use ilium_execution::{JobOutcome, ShutdownMode};

    let (mut owner, client) = isolated_editor_save_execution();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned-editor.md");
    let pane_id = ilium_core::NodeId(90);
    let mut pane = crate::editor_pane::EditorPane::empty();
    pane.textarea = ratatui_textarea::TextArea::from(vec![
        "first authored line".to_string(),
        "second authored line".to_string(),
    ]);
    let identity = pane.instance_identity();
    let operation = Arc::new(());
    let target = SaveTarget {
        pane_id,
        identity,
        revision: pane.content_revision(),
        old_path: None,
        purpose: SavePurpose::Explicit,
        operation,
        prompt_input: None,
    };
    let caller = std::thread::current().id();
    let mut files = EditorFiles::new(client);
    assert!(
        files.save(target, path.clone(), Box::new(pane)).is_ok(),
        "editor save should be admitted"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut restored = false;
    let mut acknowledged = false;
    while files.pending() > 0 {
        if let Some(completion) = files.poll() {
            match completion {
                EditorCompletion::SaveModelReturned {
                    pane,
                    result,
                    cpu_thread,
                    ..
                } => {
                    result.unwrap();
                    assert_ne!(
                        cpu_thread.unwrap(),
                        caller,
                        "capture must run on the CPU bank"
                    );
                    assert_eq!(pane.textarea.lines()[0], "first authored line");
                    restored = true;
                }
                EditorCompletion::Saved { completion, .. } => {
                    let super::ordered::WriteCompletion::Outcome { outcome, .. } = completion
                    else {
                        panic!("ordered editor writer did not produce an outcome")
                    };
                    let (outcome, _retention) = outcome.into_parts();
                    let JobOutcome::Finished(Ok(saved)) = outcome else {
                        panic!("ordered editor writer did not durably save")
                    };
                    assert_eq!(saved.source_revision, 0);
                    acknowledged = true;
                }
                other => panic!(
                    "unexpected editor completion: {:?}",
                    completion_kind(&other)
                ),
            }
        }
        assert!(Instant::now() < deadline, "editor save did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        restored,
        "the editable pane must return before the save settles"
    );
    assert!(
        acknowledged,
        "durable writer acknowledgement must be observed"
    );
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "first authored line\nsecond authored line\n"
    );
    drop(files);
    owner.request_shutdown(ShutdownMode::Drain);
    assert!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .shutdown_complete
    );
}

fn completion_kind(completion: &super::editors::EditorCompletion) -> &'static str {
    match completion {
        super::editors::EditorCompletion::FrozenScreenSaved { .. } => "frozen screen saved",
        super::editors::EditorCompletion::FrozenScreenLoaded { .. } => "frozen screen loaded",
        super::editors::EditorCompletion::FrozenScreenLoadFailed { .. } => {
            "frozen screen load failed"
        }
        super::editors::EditorCompletion::Loaded { .. } => "loaded",
        super::editors::EditorCompletion::LoadLost { .. } => "load lost",
        super::editors::EditorCompletion::Saved { .. } => "saved",
        super::editors::EditorCompletion::SaveModelReturned { .. } => "model returned",
        super::editors::EditorCompletion::SaveModelLost { .. } => "model lost",
        super::editors::EditorCompletion::UnmatchedWrite(_) => "unmatched write",
    }
}
#[test]
fn loaded_editor_source_credit_survives_receipt_and_real_worker_join_until_pane_drop() {
    use ilium_execution::{JobOutcome, JobPoll, Lane, ShutdownMode};
    let (mut owner, client, quota) = isolated_load_execution();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("authored.txt");
    std::fs::write(&path, "authored first line\nsecond line\n").unwrap();
    let job = super::editor::EditorRead {
        path: path.clone(),
        source_hold: Arc::new(quota.reserve_external_storage(64 * 1024 * 1024).unwrap()),
    };
    let mut receipt = client.try_submit(Lane::Io, job.cost(), job).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        match receipt.try_take() {
            JobPoll::Ready(result) => break result,
            JobPoll::Pending => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            _ => panic!("editor read receipt lost"),
        }
    };
    let (result, retention) = result.into_parts();
    let JobOutcome::Finished(Ok(source)) = result else {
        panic!("editor source preparation failed")
    };
    let editor = crate::editor_pane::EditorPane::from_source(source);
    assert_eq!(
        editor.textarea.lines(),
        &["authored first line".to_string(), "second line".to_string()]
    );
    drop(retention);
    drop(receipt);
    owner.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(
        quota.snapshot().jobs,
        0,
        "installed buffer does not occupy a completed finite job slot"
    );
    assert_eq!(quota.snapshot().result_bytes, 0);
    assert!(
        quota.snapshot().worker_bytes > 64 * 1024 * 1024,
        "live execution handles retain charged bank metadata"
    );
    drop(client);
    drop(owner);
    assert_eq!(quota.snapshot().worker_bytes, 64 * 1024 * 1024);
    drop(editor);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "authored first line\nsecond line\n"
    );
}
#[test]
fn loaded_board_source_credit_survives_reader_retirement_until_last_pane_clone() {
    use ilium_execution::ShutdownMode;
    let (mut owner, client, quota) = isolated_load_execution();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("board.md");
    let original = "## To do\n- [ ] Authored card\n";
    std::fs::write(&path, original).unwrap();
    let storage = ilium_core::BoardStorage::MarkdownFile { path: path.clone() };
    let mut files = BoardFiles::new(client, Arc::new(tokio::sync::Notify::new()));
    files.set_storage_quota(quota.clone());
    files
        .request_pane(ilium_core::NodeId(81), &storage)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let source = loop {
        if let Some(loaded) = files.poll_read() {
            break loaded.result.unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let board = files.attach(source).unwrap();
    assert_eq!(board.columns[0].cards[0].title, "[ ] Authored card");
    let retained = board.clone();
    drop(board);
    owner.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(
        quota.snapshot().jobs,
        0,
        "installed board releases its completed finite read slot"
    );
    assert!(quota.snapshot().worker_bytes >= 2 * 1024 * 1024);
    drop(owner);
    drop(files);
    assert!(quota.snapshot().worker_bytes >= 2 * 1024 * 1024);
    drop(retained);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[test]
fn installed_editors_do_not_exhaust_finite_read_slots() {
    use ilium_execution::{JobOutcome, JobPoll, Lane, ShutdownMode};
    let (mut owner, client, quota) = isolated_load_execution();
    let directory = tempfile::tempdir().unwrap();
    let mut editors = Vec::new();
    // Nine installed documents exceed this client's eight finite job slots.
    for index in 0..9 {
        let path = directory.path().join(format!("authored-{index}.txt"));
        let authored = format!("authored document {index}\n");
        std::fs::write(&path, &authored).unwrap();
        let job = super::editor::EditorRead {
            path: path.clone(),
            source_hold: Arc::new(quota.reserve_external_storage(64 * 1024 * 1024).unwrap()),
        };
        let mut receipt = client.try_submit(Lane::Io, job.cost(), job).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let outcome = loop {
            match receipt.try_take() {
                JobPoll::Ready(outcome) => break outcome,
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                _ => panic!("editor read receipt lost"),
            }
        };
        let (outcome, retention) = outcome.into_parts();
        let JobOutcome::Finished(Ok(source)) = outcome else {
            panic!("editor source preparation failed")
        };
        editors.push(crate::editor_pane::EditorPane::from_source(source));
        drop(retention);
        drop(receipt);
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().result_bytes, 0);
        assert_eq!(std::fs::read_to_string(path).unwrap(), authored);
    }
    owner.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
    assert!(
        quota.snapshot().worker_bytes > 9 * 64 * 1024 * 1024,
        "live execution handles retain charged bank metadata"
    );
    drop(client);
    drop(owner);
    assert_eq!(quota.snapshot().worker_bytes, 9 * 64 * 1024 * 1024);
    assert_eq!(editors.len(), 9);
    drop(editors);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn refused_editor_bank_admission_keeps_original_source_guard_for_retry() {
    use ilium_execution::{JobCost, JobOutcome};
    let (mut owner, client, quota) = isolated_load_execution();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("retained-authored.txt");
    std::fs::write(&path, "retained authored line\n").unwrap();
    let blocker = client
        .try_reserve_external(JobCost {
            input_bytes: 256 * 1024 * 1024,
            result_bytes: 0,
        })
        .unwrap()
        .retain(())
        .unwrap();
    let mut files = super::editors::EditorFiles::new(client);
    files.set_storage_quota(quota.clone());
    let mut target = super::editors::LoadTarget {
        pane_id: ilium_core::NodeId(82),
        path: path.clone(),
        line: None,
        column: None,
        source_hold: None,
    };
    assert!(files
        .request_load(&mut target)
        .unwrap_err()
        .contains("InputBytes"));
    let first_guard = Arc::clone(target.source_hold.as_ref().unwrap());
    let bytes = quota.snapshot().worker_bytes;
    for _ in 0..16 {
        assert!(files
            .request_load(&mut target)
            .unwrap_err()
            .contains("InputBytes"));
        assert!(Arc::ptr_eq(
            &first_guard,
            target.source_hold.as_ref().unwrap()
        ));
        assert_eq!(quota.snapshot().worker_bytes, bytes);
    }
    drop(blocker);
    files.request_load(&mut target).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let outcome = loop {
        if let Some(super::editors::EditorCompletion::Loaded { outcome, .. }) = files.poll() {
            break outcome;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let (outcome, hold) = outcome.into_parts();
    let JobOutcome::Finished(Ok(source)) = outcome else {
        panic!("actual retry read failed")
    };
    assert_eq!(source.lines, ["retained authored line"]);
    drop(source);
    drop(hold);
    assert_eq!(quota.snapshot().jobs, 0);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "retained authored line\n"
    );
    drop(files);
    drop(target);
    drop(first_guard);
    owner.request_shutdown(ilium_execution::ShutdownMode::Drain);
    assert_eq!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .remaining_workers,
        0
    );
    // Joined workers leave bank metadata charged until the final owner drops.
    assert!(quota.snapshot().worker_bytes > 0);
    drop(owner);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[tokio::test]
async fn frozen_screen_restore_reads_and_parses_before_retained_publication() {
    let directory = tempfile::tempdir().unwrap();
    let frozen_directory = directory.path().join("frozen-screens");
    std::fs::create_dir_all(&frozen_directory).unwrap();
    let mut app = App::new(
        "frozen-screen-restore".into(),
        directory.path().to_path_buf(),
    );
    app.config_dir = Some(directory.path().to_path_buf());
    let group = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
    let pane_id = app
        .tree
        .add_pane(group, "frozen", ilium_core::PaneContentKind::Terminal)
        .unwrap();
    app.frozen_panes.insert(pane_id);
    let mut snapshot = 2u16.to_le_bytes().to_vec();
    snapshot.extend_from_slice(&24u16.to_le_bytes());
    snapshot.extend_from_slice(b"\x1b[32mrestored\x1b[0m");
    std::fs::write(
        frozen_directory.join(format!("{}.bin", pane_id.0)),
        snapshot,
    )
    .unwrap();

    app.restore_frozen_screen(pane_id);
    assert!(!app.frozen_screens.contains_key(&pane_id));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.frozen_screens.contains_key(&pane_id) {
        app.collect_editor_files();
        assert!(
            Instant::now() < deadline,
            "frozen screen worker did not publish"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let restored = app.frozen_screens.get(&pane_id).unwrap();
    assert_eq!(restored.with_screen(|screen| screen.size()), (2, 24));
    assert_eq!(restored.with_screen(|screen| screen.contents()), "restored");
    assert!(app.frozen_screen_holds.contains_key(&pane_id));
}

#[tokio::test]
async fn stale_frozen_screen_restore_cannot_replace_the_new_pane_generation() {
    let directory = tempfile::tempdir().unwrap();
    let frozen_directory = directory.path().join("frozen-screens");
    std::fs::create_dir_all(&frozen_directory).unwrap();
    let mut app = App::new("stale-frozen-screen".into(), directory.path().to_path_buf());
    app.config_dir = Some(directory.path().to_path_buf());
    let group = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
    let pane_id = app
        .tree
        .add_pane(group, "frozen", ilium_core::PaneContentKind::Terminal)
        .unwrap();
    app.frozen_panes.insert(pane_id);
    let mut snapshot = 2u16.to_le_bytes().to_vec();
    snapshot.extend_from_slice(&24u16.to_le_bytes());
    snapshot.extend_from_slice(b"stale");
    std::fs::write(
        frozen_directory.join(format!("{}.bin", pane_id.0)),
        snapshot,
    )
    .unwrap();

    app.restore_frozen_screen(pane_id);
    app.frozen_screen_restore_identities
        .insert(pane_id, Arc::new(()));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.collect_editor_files();
        if app
            .editor_files
            .as_ref()
            .is_none_or(|files| files.pending() == 0)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stale frozen screen worker did not retire"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(!app.frozen_screens.contains_key(&pane_id));
    assert!(app.frozen_screen_restore_identities.contains_key(&pane_id));
}

#[test]
fn frozen_screen_save_runs_on_workers_and_acknowledges_durable_readback() {
    use super::editors::{EditorCompletion, EditorFiles, FrozenScreenTarget};
    use ilium_execution::{JobOutcome, ShutdownMode};

    let (mut owner, client) = isolated_editor_save_execution();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frozen-screens").join("91.bin");
    let pane_id = ilium_core::NodeId(91);
    let mut input = 2u16.to_le_bytes().to_vec();
    input.extend_from_slice(&24u16.to_le_bytes());
    input.extend_from_slice(b"\x1b[32mworker-owned snapshot\x1b[0m");
    let screen = crate::terminal_view::PaintedTerminal::from_frozen_bytes(&input).unwrap();
    let target = FrozenScreenTarget {
        pane_id,
        identity: Arc::new(()),
    };
    let caller = std::thread::current().id();
    let mut files = EditorFiles::new(client);

    files
        .save_frozen_screen(target, path.clone(), screen)
        .expect("bounded frozen-screen save should be admitted");
    files.close_admission();

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut durable_acknowledgement = false;
    while files.pending() > 0 {
        if let Some(EditorCompletion::FrozenScreenSaved { completion, .. }) = files.poll() {
            let super::ordered::WriteCompletion::Outcome { outcome, .. } = completion else {
                panic!("ordered frozen-screen write did not produce a result")
            };
            let (outcome, _retention) = outcome.into_parts();
            let JobOutcome::Finished(Ok(saved)) = outcome else {
                panic!("frozen-screen save failed before durable acknowledgement")
            };
            assert_ne!(saved.serialization_thread, caller);
            assert_ne!(saved.writer_thread, caller);
            assert_ne!(
                saved.serialization_thread, saved.writer_thread,
                "CPU serialization and ordered I/O must have distinct owners"
            );
            let bytes = std::fs::read(&path).expect("acknowledged snapshot must exist");
            let restored = crate::terminal_view::PaintedTerminal::from_frozen_bytes(&bytes)
                .expect("acknowledged snapshot must parse");
            assert_eq!(
                restored.with_screen(|screen| screen.contents()),
                "worker-owned snapshot"
            );
            durable_acknowledgement = true;
        }
        assert!(
            Instant::now() < deadline,
            "frozen-screen save did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(durable_acknowledgement, "durable write receipt is required");

    drop(files);
    owner.request_shutdown(ShutdownMode::Drain);
    assert!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .shutdown_complete
    );
}

#[test]
fn frozen_screen_save_reports_durable_writer_failure() {
    use super::editors::{EditorCompletion, EditorFiles, FrozenScreenTarget};
    use ilium_execution::{JobOutcome, ShutdownMode};

    let (mut owner, client) = isolated_editor_save_execution();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frozen-screens").join("blocked.bin");
    std::fs::create_dir_all(&path).unwrap();
    let screen = crate::terminal_view::PaintedTerminal::from_frozen_bytes(&[
        24, 0, 80, 0, b'f', b'a', b'i', b'l',
    ])
    .unwrap();
    let target = FrozenScreenTarget {
        pane_id: ilium_core::NodeId(92),
        identity: Arc::new(()),
    };
    let mut files = EditorFiles::new(client);
    files
        .save_frozen_screen(target, path, screen)
        .expect("bounded frozen-screen save should be admitted");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut reported_failure = false;
    while files.pending() > 0 {
        if let Some(EditorCompletion::FrozenScreenSaved { completion, .. }) = files.poll() {
            let super::ordered::WriteCompletion::Outcome { outcome, .. } = completion else {
                panic!("durable writer failure must retain its ordered result")
            };
            assert!(matches!(outcome.view(), JobOutcome::Finished(Err(_))));
            reported_failure = true;
        }
        assert!(
            Instant::now() < deadline,
            "frozen-screen writer failure did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(reported_failure, "writer failure must reach its owner");

    drop(files);
    owner.request_shutdown(ShutdownMode::Drain);
    assert!(
        owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .shutdown_complete
    );
}
