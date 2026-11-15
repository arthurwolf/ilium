//! Authoritative numeric reopen and asynchronous ownership regressions.
use super::*;
use crate::app::SettingsState;
use crate::value_dialog::{DialogOutcome, ValueDialogState};

/// An app on the Editor tab with the autosave-delay number selected: the numeric field every
/// read test below drives (its stored value lives in `[editor] autosave_delay_ms`).
fn editor_app(directory: &std::path::Path) -> App {
    let mut app = App::new("numeric-authoritative-read".into(), directory.into());
    app.config_dir = Some(directory.into());
    app.mode = Mode::Settings(Box::new(SettingsState {
        tab: SettingsTab::Editor,
        selected_row: 4,
        ..SettingsState::default()
    }));
    app
}

fn number(app: &App) -> &crate::value_dialog::NumberDialogState {
    let Mode::ValueDialog(host) = &app.mode else {
        panic!("number host");
    };
    let ValueDialogState::Number(number) = &host.dialog else {
        panic!("numeric draft");
    };
    number
}

fn submit(app: &mut App, outcome: DialogOutcome) {
    let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
        panic!("number host");
    };
    app.finish_value_dialog(host, outcome);
}

#[test]
fn authoritative_number_read_failure_blocks_commit_and_stays_visible_during_edits() {
    for directory_instead_of_file in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        if directory_instead_of_file {
            std::fs::create_dir(&path).unwrap();
        } else {
            std::fs::write(&path, "[editor\n").unwrap();
        }
        let mut app = editor_app(directory.path());
        app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
        assert!(
            number(&app).draft.buf.is_empty(),
            "runtime preview is not saved readback"
        );
        // A pending read must not admit an unsafe write either.
        submit(&mut app, DialogOutcome::CommitNumber("1530".into()));
        assert_eq!(app.configuration_admission.attempts, 0);
        app.settle_filesystem_for_test();
        assert!(number(&app)
            .notice
            .as_deref()
            .unwrap()
            .contains("unavailable"));
        crate::keys::handle_event(&mut app, crossterm::event::Event::Paste("1530".into()));
        assert!(
            number(&app).notice.is_some(),
            "read error survives ordinary draft edits"
        );
        submit(&mut app, DialogOutcome::CommitNumber("1530".into()));
        assert_eq!(app.configuration_admission.attempts, 0);
        assert_eq!(app.editor_settings.autosave_delay_ms, 1000);
        // Cancel and reopen is an explicit fresh read, not an automatic write retry.
        submit(&mut app, DialogOutcome::Cancel);
        if directory_instead_of_file {
            std::fs::remove_dir(&path).unwrap();
        }
        std::fs::write(&path, "[editor]\nautosave_delay_ms = 2070\n").unwrap();
        app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
        app.settle_filesystem_for_test();
        assert_eq!(number(&app).draft.buf, "2070");
        assert!(number(&app).notice.is_none());
    }
}

#[test]
fn authoritative_number_read_preserves_a_draft_typed_while_loading() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("config.toml"),
        "[editor]\nautosave_delay_ms = 2070\n",
    )
    .unwrap();
    let mut app = editor_app(directory.path());
    app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
    crate::keys::handle_event(&mut app, crossterm::event::Event::Paste("01530".into()));
    app.settle_filesystem_for_test();
    assert_eq!(number(&app).draft.buf, "01530");
    assert!(number(&app).notice.is_none());
    submit(&mut app, DialogOutcome::CommitNumber("01530".into()));
    app.settle_filesystem_for_test();
    assert!(matches!(app.mode, Mode::Settings(_)));
    assert_eq!(
        crate::config::load(directory.path())
            .unwrap()
            .editor
            .autosave_delay_ms,
        1530
    );
}

#[test]
fn authoritative_number_read_does_not_restore_a_draft_the_user_cleared() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = editor_app(directory.path());
    app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
    crate::keys::handle_event(&mut app, crossterm::event::Event::Paste("1".into()));
    crate::keys::handle_event(
        &mut app,
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        )),
    );
    app.settle_filesystem_for_test();
    assert!(number(&app).draft.buf.is_empty());
    assert!(number(&app).notice.is_none());
}

#[test]
fn authoritative_number_read_fences_replacement_dialog_destination_and_newer_runtime() {
    for changed_destination in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut app = editor_app(directory.path());
        app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("number");
        };
        let obsolete_token = host.number_read().unwrap().token.clone();
        submit(&mut app, DialogOutcome::Cancel);
        app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
        app.collect_settings_number_read(
            &obsolete_token,
            crate::filesystem::ordered::WriteCompletion::Lost {
                id: crate::filesystem::ordered::WriteId(0),
            },
        );
        assert!(
            number(&app).notice.as_deref().unwrap().contains("Loading"),
            "obsolete receipt did not settle replacement"
        );
        if changed_destination {
            app.config_dir = Some(other.path().into());
        } else {
            app.editor_settings.autosave_delay_ms = 1550;
        }
        app.settle_filesystem_for_test();
        assert!(number(&app).draft.buf.is_empty());
        assert!(number(&app).notice.as_deref().unwrap().contains("changed"));
        submit(&mut app, DialogOutcome::CommitNumber("1530".into()));
        assert_eq!(app.configuration_admission.attempts, 0);
        assert!(!directory.path().join("config.toml").exists());
        assert!(!other.path().join("config.toml").exists());
    }
}

#[test]
fn authoritative_number_read_rejects_a_later_admitted_write_even_when_value_is_equal() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = editor_app(directory.path());
    app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
    app.save_settings_number(
        SettingsNumber::EditorAutosaveDelay,
        "1000",
        directory.path().into(),
        None,
    )
    .unwrap();
    app.settle_filesystem_for_test();
    assert!(number(&app).draft.buf.is_empty());
    assert!(number(&app).notice.as_deref().unwrap().contains("changed"));
    assert_eq!(
        crate::config::load(directory.path())
            .unwrap()
            .editor
            .autosave_delay_ms,
        1000
    );
}

#[test]
fn authoritative_number_read_refuses_an_inaccessible_configuration_parent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("not-a-directory");
    std::fs::write(&path, "not a directory").unwrap();
    let mut app = editor_app(&path);
    app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
    app.settle_filesystem_for_test();
    assert!(number(&app)
        .notice
        .as_deref()
        .unwrap()
        .contains("unavailable"));
    submit(&mut app, DialogOutcome::CommitNumber("1530".into()));
    assert_eq!(app.configuration_admission.attempts, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a directory");
}

#[test]
fn authoritative_number_read_is_ordered_after_admitted_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = editor_app(directory.path());
    app.save_settings_number(
        SettingsNumber::EditorAutosaveDelay,
        "1530",
        directory.path().into(),
        None,
    )
    .unwrap();
    app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
    app.settle_filesystem_for_test();
    assert_eq!(number(&app).draft.buf, "1530");
    assert!(number(&app).notice.is_none());
    assert_eq!(
        crate::config::load(directory.path())
            .unwrap()
            .editor
            .autosave_delay_ms,
        1530
    );
}
