//! F-066: the `smart-copy` command holds a terminal snapshot while the picker is open and drops it on close.

use ilium_plugin_sdk::types::{GrabId, Intent, KeyEvent};

use crate::common::{
    add_terminal_pane, deliver_candidates, client_host, intents, menu_command, palette_command,
    server_host,
};

fn key(name: &str) -> ilium_plugin_sdk::types::Event {
    ilium_plugin_sdk::types::Event::KeyPressed(KeyEvent {
        grab: GrabId(0),
        pane: None,
        chord: name.to_string(),
        repeat: false,
    })
}

#[test]
fn refused_scan_releases_the_picker_and_reports_failure() {
    use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome, PluginError, RequestRef, RequestResult};
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let submitted = host.deliver_one(palette_command("smart-copy", Some(pane)));
    let scan = submitted.iter().find(|submission| matches!(&submission.intent,
        Intent::Message(message) if message.channel == "scan")).expect("scan request");
    let result = host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(scan.correlation),
        outcome: Outcome::Refused(PluginError::new(ErrorCode::Busy, "queue full")),
    }));
    assert_eq!(host.terminal().live_snapshots(), 0);
    let _ = pane;
    assert!(crate::common::status_contains(&result, "Smart Copy scan failed"));
}

#[test]
fn late_refusal_cannot_close_a_newer_picker() {
    use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome, PluginError, RequestRef, RequestResult};
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let first = host.deliver_one(palette_command("smart-copy", Some(pane)));
    let correlation = first.iter().find(|submission| matches!(&submission.intent,
        Intent::Message(message) if message.channel == "scan")).unwrap().correlation;
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    let result = host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(correlation),
        outcome: Outcome::Refused(PluginError::new(ErrorCode::Busy, "queue full")),
    }));
    assert_eq!(host.terminal().live_snapshots(), 1);
    assert!(!crate::common::status_contains(&result, "Smart Copy scan failed"));
}

#[test]
fn opening_smart_copy_does_not_request_agent_process_freezing() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let out = intents(&host.deliver_one(palette_command("smart-copy", Some(pane))));
    assert!(!out.iter().any(|intent| matches!(intent, Intent::FreezePane(_))), "{out:?}");
}

/// REQ: F-066
#[test]
fn feature_066_palette_command_with_a_pane_holds_a_snapshot() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    assert_eq!(host.terminal().live_snapshots(), 1);
}

/// REQ: F-066
#[test]
fn feature_066_menu_command_with_a_pane_holds_a_snapshot() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(menu_command("smart-copy", pane, "smart-copy"));
    assert_eq!(host.terminal().live_snapshots(), 1);
}

/// REQ: F-066
#[test]
fn feature_066_command_without_a_pane_holds_no_snapshot() {
    let mut host = client_host();
    host.deliver_one(palette_command("smart-copy", None));
    assert_eq!(host.terminal().live_snapshots(), 0);
    assert!(host.failures().is_empty());
}

/// REQ: F-066
#[test]
fn feature_066_command_holds_exactly_one_snapshot_even_when_repeated() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    assert_eq!(host.terminal().live_snapshots(), 1);
}

/// REQ: F-066
#[test]
fn feature_066_escape_in_the_picker_drops_the_snapshot() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["src/a.rs"]);
    host.deliver_one(key("esc"));
    assert_eq!(host.terminal().live_snapshots(), 0);
}

#[test]
fn empty_scan_after_failed_ranking_releases_the_pane() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.inference().fail_next("Unavailable");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    deliver_candidates(&mut host, &[]);
    assert_eq!(host.terminal().live_snapshots(), 0);
}

#[test]
fn failed_ranking_after_empty_scan_releases_the_pane() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.inference().fail_next("Unavailable");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &[]);
    host.settle();
    assert_eq!(host.terminal().live_snapshots(), 0);
}

#[test]
fn a_delayed_scan_cannot_open_a_newer_picker_on_the_same_pane() {
    use ilium_plugin_sdk::types::{Event, MessageEvent, PluginId, PluginSide};

    let mut client = client_host();
    let mut server = server_host();
    let pane = add_terminal_pane(&mut client, "api");
    assert_eq!(add_terminal_pane(&mut server, "api"), pane);
    server.terminal().set_rows(pane, &["src/old.rs"]);
    let first = client.deliver_one(palette_command("smart-copy", Some(pane)));
    let scan = first.into_iter().find_map(|submission| match submission.intent {
        Intent::Message(message) if message.channel == "scan" => Some(message),
        _ => None,
    }).expect("the first picker requests a scan");
    let reply = server.deliver_one(Event::Message(MessageEvent {
        from: PluginId("ilium.core.smart-copy".into()),
        from_side: PluginSide::Client,
        from_client: Some(ilium_plugin_sdk::types::ClientId(1)),
        channel: scan.channel,
        payload: scan.payload,
    })).into_iter().find_map(|submission| match submission.intent {
        Intent::Message(message) if message.channel == "candidates" => Some(message),
        _ => None,
    }).expect("the server answers the first scan");

    let second = client.deliver_one(palette_command("smart-copy", Some(pane)));
    let late = client.deliver_one(Event::Message(MessageEvent {
        from: PluginId("ilium.core.smart-copy".into()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: reply.channel,
        payload: reply.payload,
    }));
    assert!(!intents(&late).iter().any(|intent| matches!(intent, Intent::OpenView(_))),
        "a reply for the first scan must not open the second picker: {late:?}");
    let scan = second.into_iter().find_map(|submission| match submission.intent {
        Intent::Message(message) if message.channel == "scan" => Some(message),
        _ => None,
    }).expect("the second picker requests a scan");
    let reply = server.deliver_one(Event::Message(MessageEvent {
        from: PluginId("ilium.core.smart-copy".into()),
        from_side: PluginSide::Client,
        from_client: Some(ilium_plugin_sdk::types::ClientId(1)),
        channel: scan.channel,
        payload: scan.payload,
    })).into_iter().find_map(|submission| match submission.intent {
        Intent::Message(message) if message.channel == "candidates" => Some(message),
        _ => None,
    }).expect("the server answers the second scan");
    let current = client.deliver_one(Event::Message(MessageEvent {
        from: PluginId("ilium.core.smart-copy".into()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: reply.channel,
        payload: reply.payload,
    }));
    assert!(intents(&current).iter().any(|intent| matches!(intent, Intent::OpenView(_))),
        "the current scan must still open its picker: {current:?}");
}

#[test]
fn a_multiline_candidate_is_copied_as_one_value() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["first line\nsecond line"]);
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("first line\nsecond line"));
}

#[test]
fn a_reply_from_a_closed_picker_cannot_open_a_reopened_picker() {
    use ilium_plugin_sdk::types::{Event, MessageEvent, PluginId, PluginSide};

    let mut client = client_host();
    let mut server = server_host();
    let pane = add_terminal_pane(&mut client, "api");
    assert_eq!(add_terminal_pane(&mut server, "api"), pane);
    server.terminal().set_rows(pane, &["src/old.rs"]);
    let scan = client.deliver_one(palette_command("smart-copy", Some(pane)))
        .into_iter().find_map(|submission| match submission.intent {
            Intent::Message(message) if message.channel == "scan" => Some(message),
            _ => None,
        }).expect("first scan");
    let reply = server.deliver_one(Event::Message(MessageEvent {
        from: PluginId("ilium.core.smart-copy".into()), from_side: PluginSide::Client,
        from_client: Some(ilium_plugin_sdk::types::ClientId(1)), channel: scan.channel, payload: scan.payload,
    })).into_iter().find_map(|submission| match submission.intent {
        Intent::Message(message) if message.channel == "candidates" => Some(message),
        _ => None,
    }).expect("first reply");
    let message = Event::Message(MessageEvent {
        from: PluginId("ilium.core.smart-copy".into()), from_side: PluginSide::Server,
        from_client: None, channel: reply.channel, payload: reply.payload,
    });
    assert!(intents(&client.deliver_one(message.clone())).iter()
        .any(|intent| matches!(intent, Intent::OpenView(_))));
    client.deliver_one(key("esc"));
    client.deliver_one(palette_command("smart-copy", Some(pane)));
    let late = client.deliver_one(message);
    assert!(!intents(&late).iter().any(|intent| matches!(intent, Intent::OpenView(_))));
}

/// REQ: F-066
#[test]
fn feature_066_copying_a_candidate_also_drops_the_snapshot() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["src/a.rs"]);
    host.deliver_one(key("1"));
    host.settle();
    assert_eq!(host.terminal().live_snapshots(), 0);
}

/// REQ: F-066
#[test]
fn feature_066_escape_without_an_open_picker_changes_nothing() {
    let mut host = client_host();
    host.deliver_one(key("esc"));
    assert_eq!(host.terminal().live_snapshots(), 0);
}
