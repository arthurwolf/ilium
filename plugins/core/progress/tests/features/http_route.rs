//! `GET /progress.json` on the loopback HTTP listener (F-060 plugin build addition).

use ilium_plugin_sdk::types::{Event, Header, HttpCalledEvent};
use serde_json::Value;

use crate::common::*;

fn call(host: &mut ilium_plugin_sdk::testing::FakeHost, id: u64, method: &str, path: &str) {
    host.deliver_one(Event::HttpCalled(HttpCalledEvent {
        call: id,
        method: method.to_string(),
        path: path.to_string(),
        headers: Vec::<Header>::new(),
        body: Vec::new(),
    }));
    host.settle();
}

fn replies_to(host: &ilium_plugin_sdk::testing::FakeHost, id: u64) -> Vec<(u16, Vec<(String, String)>, Vec<u8>)> {
    http_replies(host).into_iter().filter(|r| r.0 == id).map(|r| (r.1, r.2, r.3)).collect()
}

/// REQ: F-060
#[test]
fn f_060_get_progress_json_answers_200_with_json_content_type() {
    let mut host = activated_server();
    call(&mut host, 7, "GET", "/progress.json");
    let replies = replies_to(&host, 7);
    assert_eq!(replies.len(), 1, "exactly one reply per call");
    assert_eq!(replies[0].0, 200);
    assert!(replies[0].1.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.starts_with("application/json")));
}

/// REQ: F-060
#[test]
fn f_060_with_no_monitors_the_body_is_an_empty_monitors_list() {
    let mut host = activated_server();
    call(&mut host, 1, "GET", "/progress.json");
    let body: Value = serde_json::from_slice(&replies_to(&host, 1)[0].2).expect("json");
    assert_eq!(body, serde_json::json!({"monitors": []}));
}

/// REQ: F-060
#[test]
fn f_060_the_body_lists_each_monitor_as_a_progress_object() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, "/opt/probe.sh", &running("job", 30, "Thirty"), &[]);
    call(&mut host, 2, "GET", "/progress.json");
    let body: Value = serde_json::from_slice(&replies_to(&host, 2)[0].2).expect("json");
    assert_eq!(body["monitors"].as_array().unwrap().len(), 1);
    let listed = &body["monitors"][0];
    assert_eq!(listed["monitor_id"], id);
    assert_eq!(listed["report"]["percent"].as_f64(), Some(30.0));
    assert_eq!(top_level_keys(&listed.to_string().replace(' ', "")).len(), 4);
}

/// REQ: F-060
#[test]
fn f_060_a_post_to_the_route_is_not_answered_with_progress() {
    let mut host = activated_server();
    call(&mut host, 3, "POST", "/progress.json");
    for reply in replies_to(&host, 3) {
        assert_ne!(reply.0, 200);
    }
}

/// REQ: F-060
#[test]
fn f_060_another_path_is_not_answered_with_progress() {
    let mut host = activated_server();
    call(&mut host, 4, "GET", "/progress");
    for reply in replies_to(&host, 4) {
        assert_ne!(reply.0, 200);
    }
}

/// REQ: F-060
#[test]
fn f_060_each_call_is_answered_exactly_once_even_when_many_arrive() {
    let mut host = activated_server();
    for id in 10..15 {
        call(&mut host, id, "GET", "/progress.json");
    }
    for id in 10..15 {
        assert_eq!(replies_to(&host, id).len(), 1, "call {id}");
    }
}
