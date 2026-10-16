//! F-017 screen transfer between panes: the service `ilium.core.terminal/transfer` (send-screen)
//! and the page sample "Screen transfer between panes".

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{Delivery, Intent, NodeId, SendInputSpec, Submission};

use crate::common::workspace_root;

const PLUGIN: &str = "ilium.core.terminal";
const SERVICE: &str = "ilium.core.terminal/transfer";

fn session() -> FakeSession {
    let mut session = FakeSession::new();
    let package = workspace_root().join("plugins/core/terminal");
    session.install_core("kanban");
    session.install(package).start();
    session
}

fn send_screen(session: &mut FakeSession, source: u64, target: u64) -> Result<ServiceReply, ilium_plugin_sdk::types::PluginError> {
    let request = format!("{{\"method\":\"send-screen\",\"source\":{source},\"target\":{target}}}");
    session.call_service("ilium.core.kanban", SERVICE, "^0.1", &request)
}

fn pasted(text: &str, target: u64) -> Intent {
    Intent::SendInput(SendInputSpec {
        pane: NodeId(target),
        text: text.to_string(),
        delivery: Delivery::Pasted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    })
}

fn submitted(session: &mut FakeSession) -> Vec<Submission> {
    session.server(PLUGIN).submitted()
}

/// REQ: F-017
#[test]
fn feature_017_send_screen_pastes_the_visible_rows_into_the_target() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["$ cargo test", "test result: ok", "$"], 5);
    assert!(matches!(send_screen(&mut session, 3, 4), Ok(ServiceReply::Done(_))));
    let sent = submitted(&mut session);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].intent, pasted("$ cargo test\ntest result: ok\n$", 4));
}

/// REQ: F-017
#[test]
fn feature_017_trailing_blank_rows_are_trimmed_before_the_paste() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["one", "two", "", "   ", ""], 1);
    send_screen(&mut session, 3, 4).expect("service call");
    let sent = submitted(&mut session);
    assert_eq!(sent[0].intent, pasted("one\ntwo", 4));
}

/// REQ: F-017
#[test]
fn feature_017_blank_rows_between_text_rows_are_kept() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["one", "", "three"], 1);
    send_screen(&mut session, 3, 4).expect("service call");
    assert_eq!(submitted(&mut session)[0].intent, pasted("one\n\nthree", 4));
}

/// REQ: F-017
#[test]
fn feature_017_an_all_blank_screen_submits_nothing() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["", "  ", ""], 1);
    send_screen(&mut session, 3, 4).expect("service call");
    assert!(submitted(&mut session).is_empty());
}

/// REQ: F-017
#[test]
fn feature_017_a_source_without_a_readable_screen_submits_nothing() {
    let mut session = session();
    let _ = send_screen(&mut session, 99, 4);
    assert!(submitted(&mut session).is_empty());
}

/// REQ: F-017
#[test]
fn feature_017_the_paste_is_keyed_by_source_target_and_revision() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["x"], 42);
    send_screen(&mut session, 3, 4).expect("service call");
    assert_eq!(submitted(&mut session)[0].dedup_key.as_deref(), Some("transfer-3-4-42"));
}

/// REQ: F-017
#[test]
fn feature_017_a_new_screen_revision_gives_a_new_key() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["x"], 42);
    send_screen(&mut session, 3, 4).expect("first");
    session.server(PLUGIN).terminal().set_screen(3, &["x", "y"], 43);
    send_screen(&mut session, 3, 4).expect("second");
    let keys: Vec<Option<String>> = submitted(&mut session).into_iter().map(|s| s.dedup_key).collect();
    assert_eq!(keys, [Some("transfer-3-4-42".to_string()), Some("transfer-3-4-43".to_string())]);
}

/// REQ: F-017
#[test]
fn feature_017_the_paste_goes_to_the_target_not_the_source() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(8, &["hello"], 1);
    send_screen(&mut session, 8, 2).expect("service call");
    match &submitted(&mut session)[0].intent {
        Intent::SendInput(spec) => assert_eq!(spec.pane, NodeId(2)),
        other => panic!("unexpected intent {other:?}"),
    }
}

/// REQ: F-017
#[test]
fn feature_017_the_paste_uses_pasted_delivery_with_no_stages_raw_or_mouse() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["a"], 1);
    send_screen(&mut session, 3, 4).expect("service call");
    match &submitted(&mut session)[0].intent {
        Intent::SendInput(spec) => {
            assert_eq!(spec.delivery, Delivery::Pasted);
            assert!(spec.stages.is_empty());
            assert!(spec.raw.is_none() && spec.mouse.is_none());
            assert!(spec.prompt_epoch.is_none() && spec.if_current.is_none());
            assert!(!spec.lock_user_input && !spec.journal);
        }
        other => panic!("unexpected intent {other:?}"),
    }
}

/// REQ: F-017
#[test]
fn feature_017_history_arrives_before_the_visible_screen_in_one_paste() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_history(3, "old line 1\nold line 2");
    session.server(PLUGIN).terminal().set_screen(3, &["new line"], 7);
    send_screen(&mut session, 3, 4).expect("service call");
    session.pump().expect("pump");
    let sent = submitted(&mut session);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].intent, pasted("old line 1\nold line 2\nnew line", 4));
}

/// REQ: F-017
#[test]
fn feature_017_the_service_is_offered_under_its_page_name_and_version() {
    let mut session = session();
    session.server(PLUGIN).terminal().set_screen(3, &["a"], 1);
    let reply = session.call_service("ilium.core.kanban", SERVICE, "^0.1", "{\"method\":\"send-screen\",\"source\":3,\"target\":4}");
    assert!(reply.is_ok(), "{reply:?}");
}

/// REQ: F-017
#[test]
fn feature_017_a_version_range_the_service_does_not_satisfy_is_not_found() {
    let mut session = session();
    let reply = session.call_service("ilium.core.kanban", SERVICE, "^9.0", "{}");
    assert_eq!(reply.expect_err("no provider").code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}
