//! `ilium:host/sessions` (api-sessions.md).

use ilium_plugin_api::generated::host::sessions::{CrossRequest, HostSessions, LiveSession};
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["sessions.cross"];

fn session(name: &str, is_self: bool) -> LiveSession {
    LiveSession {
        name: name.to_owned(),
        project: "/p".to_owned(),
        socket_id: format!("sock-{name}"),
        panes: 2,
        attached_clients: 1,
        is_self,
    }
}

fn request(name: &str) -> CrossRequest {
    CrossRequest {
        session: name.to_owned(),
        command: "example/ping".to_owned(),
        arguments_json: "{}".to_owned(),
    }
}

/// REQ: WIT:ilium:host/sessions.list-live
#[test]
fn wit_sessions_list_live_reports_the_live_sessions() {
    let rig = rig(GRANTS);
    rig.kernel
        .set_live_sessions(vec![session("default", true), session("other", false)]);
    let live = rig.host.sessions().list_live();
    assert_eq!(live.len(), 2);
    assert!(live.iter().filter(|s| s.is_self).count() == 1);
}

/// REQ: WIT:ilium:host/sessions.list-live
#[test]
fn wit_sessions_list_live_declined_grant_is_empty() {
    let rig = declined_rig(&["sessions.cross"]);
    rig.kernel.set_live_sessions(vec![session("default", true)]);
    assert!(rig.host.sessions().list_live().is_empty());
}

/// REQ: WIT:ilium:host/sessions.send
#[test]
fn wit_sessions_send_returns_a_request_id_and_forwards_the_command() {
    let rig = rig(GRANTS);
    rig.kernel.set_live_sessions(vec![session("other", false)]);
    let id = rig
        .host
        .sessions()
        .send(request("other"))
        .expect("accepted");
    assert!(rig
        .kernel
        .session_commands("other")
        .contains(&"example/ping".to_owned()));
    let _ = id;
}

/// REQ: WIT:ilium:host/sessions.send
#[test]
fn wit_sessions_send_unknown_session_not_found() {
    let rig = rig(GRANTS);
    assert_eq!(
        code_of(rig.host.sessions().send(request("ghost"))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/sessions.send
#[test]
fn wit_sessions_send_declined_grant_not_permitted() {
    let rig = declined_rig(&["sessions.cross"]);
    assert_eq!(
        code_of(rig.host.sessions().send(request("other"))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/sessions.take-reply
#[test]
fn wit_sessions_take_reply_returns_the_reply_once() {
    let rig = rig(GRANTS);
    rig.kernel.set_live_sessions(vec![session("other", false)]);
    rig.kernel.set_session_reply("other", "{\"ok\":true}");
    let id = rig
        .host
        .sessions()
        .send(request("other"))
        .expect("accepted");
    rig.host.pump(8);
    assert_eq!(
        rig.host.sessions().take_reply(id),
        Some("{\"ok\":true}".to_owned())
    );
    assert_eq!(rig.host.sessions().take_reply(id), None);
}

/// REQ: WIT:ilium:host/sessions.take-reply
#[test]
fn wit_sessions_take_reply_unknown_request_is_none() {
    let rig = rig(GRANTS);
    assert_eq!(
        rig.host
            .sessions()
            .take_reply(ilium_plugin_api::generated::types::ids::RequestId(9_999)),
        None
    );
}
