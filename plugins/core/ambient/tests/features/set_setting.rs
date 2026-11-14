//! The `ilium.core.ambient/set-setting` service on the server side (COR-030, COR-033).
//!
//! The fake kernel settings do not receive a plugin's `kernel-settings.write` yet (API_GAPS.md),
//! so the tests that reach the write accept either outcome of it and pin everything before it.

use ilium_plugin_core_ambient::AmbientServer;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::*;

const SERVICE: &str = "ilium.core.ambient/set-setting";

fn activated_server() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<AmbientServer>();
    host
}

fn call(host: &mut FakeHost, tab: &str, key: &str, value: &str) -> Value {
    let payload = json!({ "tab": tab, "key": key, "value": value }).to_string();
    let reply = host
        .call_service(SERVICE, "set", &payload)
        .expect("the service answers");
    serde_json::from_str(&reply).expect("the reply is JSON")
}

/// REQ: COR-030
#[test]
fn cor_030_a_refused_value_is_answered_and_nothing_is_written() {
    let mut host = activated_server();
    let reply = call(&mut host, "animations", "scene", "waves");
    assert_eq!(reply["error"]["code"], "invalid-argument");
    assert!(kernel_writes(&mut host).is_empty());
}

/// REQ: COR-030
#[test]
fn cor_030_another_tab_is_not_permitted_and_nothing_is_written() {
    let mut host = activated_server();
    let reply = call(&mut host, "terminal", "scene", "stars");
    assert_eq!(reply["error"]["code"], "not-permitted");
    assert!(kernel_writes(&mut host).is_empty());
}

/// REQ: COR-030
#[test]
fn cor_030_an_unknown_method_is_not_found() {
    let mut host = activated_server();
    let reply = host
        .call_service(SERVICE, "get", "{}")
        .expect("the service answers");
    let reply: Value = serde_json::from_str(&reply).expect("JSON");
    assert_eq!(reply["error"]["code"], "not-found");
}

/// A valid call passes the validation and reaches the kernel write; the reply is then the
/// kernel's answer, never one of the validation codes.
///
/// REQ: COR-030, COR-033
#[test]
fn cor_030_a_valid_value_reaches_the_kernel_write() {
    let mut host = activated_server();
    let reply = call(&mut host, "animations", "scene", "clouds");
    if reply["ok"] == true {
        assert_eq!(
            kernel_writes(&mut host),
            vec![(
                "animations".to_owned(),
                "scene".to_owned(),
                "\"clouds\"".to_owned()
            )]
        );
    } else {
        let code = reply["error"]["code"].as_str().expect("an error code");
        assert!(!["invalid-argument", "not-found"].contains(&code), "{code}");
    }
}
