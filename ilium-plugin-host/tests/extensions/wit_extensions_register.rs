//! wit/host/extensions.wit: the contributor-side functions register and unregister on a
//! dynamic collect point (EXT-025). Uses the ExtWorld gaps listed in API_GAPS.md.

use crate::support::*;

const COLLECT: &str = "com.example.owner/c";
const CONTRIBUTOR: &str = "com.example.reg";

fn dynamic_world() -> ExtWorld {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![collect_point("c", 8, true)]), Vec::new())),
        "ok"
    );
    let manifest = json!({
        "id": CONTRIBUTOR,
        "capabilities": {"server": ["extensions.contribute:com.example.owner/c"]}
    });
    assert_eq!(diag_code(&world.install(Tier::T3, manifest, Vec::new())), "ok");
    world
}

/// REQ: WIT:ilium:host/extensions.register
#[test]
fn wit_extensions_register_on_a_dynamic_point_makes_the_entry_listed() {
    let mut world = dynamic_world();
    assert!(world.register(CONTRIBUTOR, COLLECT, "e1", 50, r#"{"id":"e1"}"#).is_ok());
    assert_eq!(world.list(OWNER, COLLECT).map(|list| list.len()).unwrap_or(0), 1);
}

/// REQ: WIT:ilium:host/extensions.register
#[test]
fn wit_extensions_register_same_extension_id_replaces_the_entry() {
    let mut world = dynamic_world();
    assert!(world.register(CONTRIBUTOR, COLLECT, "e1", 50, r#"{"id":"a"}"#).is_ok());
    assert!(world.register(CONTRIBUTOR, COLLECT, "e1", 60, r#"{"id":"b"}"#).is_ok());
    assert_eq!(world.list(OWNER, COLLECT).map(|list| list.len()).unwrap_or(0), 1);
}

/// REQ: WIT:ilium:host/extensions.register
#[test]
fn wit_extensions_register_unknown_point_fails_not_found() {
    let mut world = dynamic_world();
    let error = err_of(world.register(CONTRIBUTOR, "com.example.owner/missing", "e1", 50, r#"{"id":"e1"}"#));
    assert!(message_has_prefix(&error, "extension.unknown-point"), "{}", error.message);
}

/// REQ: WIT:ilium:host/extensions.register
#[test]
fn wit_extensions_register_on_a_static_point_fails_not_dynamic() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![collect_point("c", 8, false)]), Vec::new())),
        "ok"
    );
    let manifest = json!({
        "id": CONTRIBUTOR,
        "capabilities": {"server": ["extensions.contribute:com.example.owner/c"]}
    });
    assert_eq!(diag_code(&world.install(Tier::T3, manifest, Vec::new())), "ok");
    let error = err_of(world.register(CONTRIBUTOR, COLLECT, "e1", 50, r#"{"id":"e1"}"#));
    assert!(message_has_prefix(&error, "extension.not-dynamic"), "{}", error.message);
}

/// REQ: WIT:ilium:host/extensions.unregister
#[test]
fn wit_extensions_unregister_removes_a_registered_entry() {
    let mut world = dynamic_world();
    assert!(world.register(CONTRIBUTOR, COLLECT, "e1", 50, r#"{"id":"e1"}"#).is_ok());
    assert!(world.unregister(CONTRIBUTOR, COLLECT, "e1").is_ok());
    assert_eq!(world.list(OWNER, COLLECT).map(|list| list.len()).unwrap_or(99), 0);
}

/// REQ: WIT:ilium:host/extensions.unregister
#[test]
fn wit_extensions_unregister_of_an_unknown_extension_fails_not_found() {
    let mut world = dynamic_world();
    let error = err_of(world.unregister(CONTRIBUTOR, COLLECT, "never-registered"));
    assert!(message_has_prefix(&error, "extension.not-found"), "{}", error.message);
}

/// REQ: WIT:ilium:host/extensions.unregister
#[test]
fn wit_extensions_unregister_twice_fails_not_found_the_second_time() {
    let mut world = dynamic_world();
    assert!(world.register(CONTRIBUTOR, COLLECT, "e1", 50, r#"{"id":"e1"}"#).is_ok());
    assert!(world.unregister(CONTRIBUTOR, COLLECT, "e1").is_ok());
    let error = err_of(world.unregister(CONTRIBUTOR, COLLECT, "e1"));
    assert!(message_has_prefix(&error, "extension.not-found"), "{}", error.message);
}
