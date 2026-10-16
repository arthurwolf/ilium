//! "Attach a log to an issue": warning dialog, default Cancel, plain copy that keeps the original.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, FsPath, Intent, ViewTarget};

use crate::common::{LOG_SCOPE, PLUGIN_ID, intents, log_line, session, write_log};

const LOG: &str = "log-2026-10-12 09-14-03.txt";
const TARGET: &str = "attach-log";

fn open(session: &mut ilium_plugin_sdk::testing::FakeSession) -> String {
    write_log(session, LOG, log_line("WARN", "x").as_bytes());
    session.client(PLUGIN_ID).send_action(ViewTarget::Popup(TARGET.to_string()), "pick", ActionKind::Activated, ActionValue::Text(LOG.to_string()));
    session.pump().expect("settles");
    session.client(PLUGIN_ID).render(ViewTarget::Popup(TARGET.to_string()), 80, 24).text()
}

/// REQ: COR-050
#[test]
fn cor_050_the_dialog_is_titled_and_warns_about_private_text() {
    let mut session = session();
    let text = open(&mut session);
    assert!(text.contains("Attach a log to an issue"), "{text}");
    assert!(text.contains("can hold prompts, replies and file text. Remove private text before you share it."), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_the_dialog_names_the_picked_file_and_its_size_in_kib() {
    let mut session = session();
    let text = open(&mut session);
    assert!(text.contains(&format!("{LOG} (1 KiB)")), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_the_buttons_are_copy_log_and_cancel_in_that_order() {
    let mut session = session();
    let text = open(&mut session);
    let copy = text.find("Copy log").expect("Copy log");
    let cancel = text.find("Cancel").expect("Cancel");
    assert!(copy < cancel, "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_nothing_is_copied_before_the_user_confirms() {
    let mut session = session();
    open(&mut session);
    assert!(session.client(PLUGIN_ID).submitted().is_empty());
    assert!(session.client(PLUGIN_ID).fs().read("path:/tmp/dest/copy.txt").is_none());
}

/// REQ: COR-050
#[test]
fn cor_050_the_default_button_is_cancel_so_enter_copies_nothing() {
    let mut session = session();
    open(&mut session);
    session.client(PLUGIN_ID).key(ViewTarget::Popup(TARGET.to_string()), "enter");
    session.pump().expect("settles");
    assert!(session.client(PLUGIN_ID).fs().read("path:/tmp/dest/copy.txt").is_none());
}

/// REQ: COR-050
#[test]
fn cor_050_confirm_copies_the_log_and_keeps_the_original() {
    let mut session = session();
    open(&mut session);
    session.client(PLUGIN_ID).send_action(ViewTarget::Popup(TARGET.to_string()), "destination", ActionKind::Changed, ActionValue::Text("path:/tmp/dest/copy.txt".to_string()));
    session.client(PLUGIN_ID).send_action(ViewTarget::Popup(TARGET.to_string()), "confirm", ActionKind::Activated, ActionValue::None);
    session.pump().expect("settles");
    let copy = session.client(PLUGIN_ID).fs().read("path:/tmp/dest/copy.txt").expect("copy exists");
    assert_eq!(copy, log_line("WARN", "x").as_bytes());
    let original = session.server(PLUGIN_ID).fs().read(FsPath { scope: LOG_SCOPE.to_string(), path: LOG.to_string() }).expect("original kept");
    assert_eq!(original, log_line("WARN", "x").as_bytes());
}

/// REQ: COR-050
#[test]
fn cor_050_confirm_submits_a_toast_that_names_the_destination() {
    let mut session = session();
    open(&mut session);
    session.client(PLUGIN_ID).send_action(ViewTarget::Popup(TARGET.to_string()), "destination", ActionKind::Changed, ActionValue::Text("path:/tmp/dest/copy.txt".to_string()));
    session.client(PLUGIN_ID).send_action(ViewTarget::Popup(TARGET.to_string()), "confirm", ActionKind::Activated, ActionValue::None);
    session.pump().expect("settles");
    let toasts: Vec<_> = intents(&session.client(PLUGIN_ID).submitted()).into_iter().filter_map(|intent| if let Intent::Toast(spec) = intent { Some(spec) } else { None }).collect();
    assert_eq!(toasts.len(), 1);
    assert!(format!("{:?}", toasts[0]).contains("/tmp/dest/copy.txt"), "{:?}", toasts[0]);
}

/// REQ: COR-050
#[test]
fn cor_050_cancel_copies_nothing_and_submits_no_toast() {
    let mut session = session();
    open(&mut session);
    session.client(PLUGIN_ID).send_action(ViewTarget::Popup(TARGET.to_string()), "cancel", ActionKind::Activated, ActionValue::None);
    session.pump().expect("settles");
    assert!(session.client(PLUGIN_ID).submitted().iter().all(|submission| !matches!(submission.intent, Intent::Toast(_))));
}
