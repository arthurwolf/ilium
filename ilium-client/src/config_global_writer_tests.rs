//! Regressions for the one global read/merge/publish owner and checked trigger edits.
use super::*;
use ilium_ipc::{TextTrigger, TextTriggerSettings};
use std::sync::mpsc;
use std::time::Duration;
const WAIT: Duration = Duration::from_secs(5);

fn rules(message: &str) -> TextTriggerSettings {
    TextTriggerSettings {
        triggers: vec![TextTrigger {
            id: "authored".to_owned(),
            regexp: "ready$".to_owned(),
            message: message.to_owned(),
            ..TextTrigger::default()
        }],
    }
}
fn read_rules(directory: &Path) -> TextTriggerSettings {
    let path = directory.join("config.toml");
    let document = read_toml_document(&path).unwrap();
    text_trigger_settings_from_document(&path, &document).unwrap()
}

#[test]
fn text_trigger_document_owner_covers_read_through_publication() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let mut document = read_toml_document(&path).unwrap();
    let lock_path = directory.path().join(".agent-detection-settings.lock");
    assert!(ExclusiveFileLock::try_acquire(&lock_path)
        .unwrap()
        .is_none());
    publish_text_trigger_settings(&path, &mut document, &rules("mine")).unwrap();
    assert!(ExclusiveFileLock::try_acquire(&lock_path)
        .unwrap()
        .is_none());
    drop(document);
    assert!(ExclusiveFileLock::try_acquire(&lock_path)
        .unwrap()
        .is_some());
}

#[test]
fn text_trigger_and_actual_detector_writer_preserve_different_tables() {
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
    let publication = publish_text_trigger_settings(&path, &mut document, &rules("mine"));
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
    assert_eq!(read_rules(directory.path()), rules("mine"));
}

type Saver = fn(&Path) -> Result<(), ClientError>;
#[test]
fn text_trigger_all_client_savers_share_the_document_owner() {
    let savers: &[(&str, Saver)] = &[
        ("ui", |dir| save_ui_settings(dir, &UiSettings::default())),
        ("terminal", |dir| {
            save_terminal_settings(dir, &TerminalSettings::default())
        }),
        ("git", |dir| save_git_settings(dir, &GitSettings::default())),
        ("reset_planning", |dir| {
            save_reset_planning_settings(dir, &ResetPlanningSettings::default())
        }),
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
        let publication = publish_text_trigger_settings(&path, &mut document, &rules(name));
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
            read_rules(directory.path()),
            rules(name),
            "{name} lost an unrelated trigger"
        );
    }
}

#[test]
fn text_trigger_refused_edits_leave_bytes_and_identities_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original = rules("old");
    save_text_trigger_settings(directory.path(), &TextTriggerSettings::default(), &original)
        .unwrap();
    let changed = rules("foreign");
    save_text_trigger_settings(directory.path(), &original, &changed).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(save_text_trigger_settings(directory.path(), &original, &rules("stale")).is_err());
    assert!(save_text_trigger_edit(
        directory.path(),
        Some(&original.triggers[0]),
        Some(&rules("draft").triggers[0])
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    for invalid in [
        "not valid [ toml",
        "[text_triggers]",
        "[text_triggers]\ntriggers = 1",
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert!(save_text_trigger_settings(
            directory.path(),
            &TextTriggerSettings::default(),
            &original
        )
        .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
    let lock =
        ExclusiveFileLock::try_acquire(&directory.path().join(".agent-detection-settings.lock"))
            .unwrap();
    assert!(lock.is_some());
}

#[test]
fn text_trigger_invalid_identity_source_is_not_repaired_by_saving() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original = rules("authored");
    save_text_trigger_settings(directory.path(), &TextTriggerSettings::default(), &original)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    for invalid in ["", " ", "bad\nidentity"] {
        let mut candidate = original.clone();
        candidate.triggers[0].id = invalid.to_owned();
        assert!(save_text_trigger_settings(directory.path(), &original, &candidate).is_err());
    }
    let mut duplicate = original.clone();
    duplicate.triggers.push(duplicate.triggers[0].clone());
    assert!(save_text_trigger_settings(directory.path(), &original, &duplicate).is_err());
    let mut invalid_regex = original.clone();
    invalid_regex.triggers[0].regexp = "[".to_owned();
    assert!(save_text_trigger_settings(directory.path(), &original, &invalid_regex).is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}
