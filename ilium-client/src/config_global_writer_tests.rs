//! Regressions for the one global read/merge/publish owner of `config.toml`.
use super::*;
use std::sync::mpsc;
use std::time::Duration;
const WAIT: Duration = Duration::from_secs(5);

/// Publishes one synthetic `[marker]` table through the held document, the way every saver does.
fn publish_marker(
    path: &Path,
    document: &mut ConfigDocument,
    value: &str,
) -> Result<(), ClientError> {
    let mut table = toml::map::Map::new();
    table.insert("value".to_owned(), toml::Value::String(value.to_owned()));
    document
        .as_table_mut()
        .expect("TOML document root")
        .insert("marker".to_owned(), toml::Value::Table(table));
    write_toml_document(path, document)
}

fn read_marker(directory: &Path) -> Option<String> {
    let path = directory.join("config.toml");
    let document = read_toml_document(&path).unwrap();
    document
        .get("marker")
        .and_then(|marker| marker.get("value"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

#[test]
fn document_owner_covers_read_through_publication() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let mut document = read_toml_document(&path).unwrap();
    let lock_path = directory.path().join(".agent-detection-settings.lock");
    assert!(ExclusiveFileLock::try_acquire(&lock_path)
        .unwrap()
        .is_none());
    publish_marker(&path, &mut document, "mine").unwrap();
    assert!(ExclusiveFileLock::try_acquire(&lock_path)
        .unwrap()
        .is_none());
    drop(document);
    assert!(ExclusiveFileLock::try_acquire(&lock_path)
        .unwrap()
        .is_some());
}

#[test]
fn marker_and_actual_detector_writer_preserve_different_tables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let mut document = read_toml_document(&path).unwrap();
    let worker_directory = directory.path().to_path_buf();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let settings = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 3,
            idle_poll_seconds: 9,
            custom_signatures: Vec::new(),
        };
        let result =
            ilium_server::config::save_agent_detection_settings(&worker_directory, &settings);
        done_tx.send(result.map_err(|error| error.message)).unwrap();
    });
    let started = started_rx.recv_timeout(WAIT);
    let early = done_rx.recv_timeout(Duration::from_millis(40));
    let was_blocked = matches!(&early, Err(mpsc::RecvTimeoutError::Timeout));
    let publication = publish_marker(&path, &mut document, "mine");
    drop(document);
    let completed = early.or_else(|_| done_rx.recv_timeout(WAIT));
    if worker.is_finished() || completed.is_ok() {
        worker.join().unwrap();
    }
    started.unwrap();
    publication.unwrap();
    completed.unwrap().unwrap();
    assert!(was_blocked);
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        saved["detection"]["working_poll_seconds"].as_integer(),
        Some(3)
    );
    assert_eq!(read_marker(directory.path()).as_deref(), Some("mine"));
}

type Saver = fn(&Path) -> Result<(), ClientError>;
#[test]
fn all_client_savers_share_the_document_owner() {
    let savers: &[(&str, Saver)] = &[
        ("ui", |dir| save_ui_settings(dir, &UiSettings::default())),
        ("terminal", |dir| {
            save_terminal_settings(dir, &TerminalSettings::default())
        }),
        ("git", |dir| save_git_settings(dir, &GitSettings::default())),
        ("cost", |dir| {
            save_cost_settings(dir, &CostSettings::default())
        }),
        ("editor", |dir| {
            save_editor_settings(dir, &EditorSettings::default())
        }),
        ("session", |dir| {
            save_session_settings(
                dir,
                &SessionSettings::default(),
                &SessionSettings::default(),
            )
            .map(|_| ())
        }),
        ("onboarding", |dir| {
            save_onboarding_progress(
                dir,
                &crate::onboarding::progress::OnboardingProgress::default(),
            )
        }),
        ("keyboard", |dir| {
            save_keyboard_settings(dir, &KeyboardSettings::default())
        }),
        ("keymap", |dir| {
            save_keymap_settings(dir, &KeyboardSettings::default(), LEADER_BINDINGS)
        }),
        ("kanban", |dir| {
            save_kanban_board_settings(dir, &KanbanBoardSettings::default())
        }),
        ("sound", |dir| {
            save_sound_settings(dir, &SoundSettings::default())
        }),
        ("inference", |dir| {
            save_inference_settings(dir, &InferenceSettings::default())
        }),
        ("triggers", |dir| {
            save_trigger_settings(dir, &TriggerSettings::default())
        }),
        ("setup", |dir| {
            save_agent_setup_settings(dir, &AgentSetupSettings::default())
        }),
        ("setup_delta", |dir| {
            update_agent_setup_settings(
                dir,
                &AgentSetupSettings::default(),
                &AgentSetupSettings::default(),
            )
            .map(|_| ())
        }),
        ("voice", |dir| {
            save_voice_settings(dir, &VoiceSettings::default())
        }),
        ("debug", |dir| {
            save_debug_settings(dir, &DebugSettings::default())
        }),
        ("api", |dir| save_api_settings(dir, &ApiSettings::default())),
    ];
    for &(name, saver) in savers {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut document = read_toml_document(&path).unwrap();
        let worker_directory = directory.path().to_path_buf();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx
                .send(saver(&worker_directory).map_err(|error| error.to_string()))
                .unwrap();
        });
        let started = started_rx.recv_timeout(WAIT);
        let early = done_rx.recv_timeout(Duration::from_millis(20));
        let was_blocked = matches!(&early, Err(mpsc::RecvTimeoutError::Timeout));
        let publication = publish_marker(&path, &mut document, name);
        drop(document);
        let completed = early.or_else(|_| done_rx.recv_timeout(WAIT));
        if worker.is_finished() || completed.is_ok() {
            worker.join().unwrap();
        }
        started.unwrap();
        publication.unwrap();
        completed.unwrap().unwrap();
        assert!(was_blocked, "{name} bypassed the global owner");
        assert_eq!(
            read_marker(directory.path()).as_deref(),
            Some(name),
            "{name} lost an unrelated table"
        );
    }
}
