//! api-commands.md: set-state and invoke of commands (WIT host/commands).

use crate::support::*;

const OWNER_PLUGIN: &str = "example.notes";
const CALLER: &str = "example.caller";

fn commands_world() -> ExtWorld {
    let mut world = ExtWorld::new();
    let owner = json!({
        "id": OWNER_PLUGIN,
        "contributes": {"commands": [
            {"id": "save", "title": "Save note"},
            {"id": "strict", "title": "Strict", "arguments_schema": {"type":"object","required":["n"],"properties":{"n":{"type":"integer"}}}}
        ]}
    });
    assert_eq!(diag_code(&world.install_on(Tier::T3, Side::Client, owner, Vec::new())), "ok");
    let caller = json!({"id": CALLER, "capabilities": {"client": ["commands.invoke:example.notes/*"]}});
    assert_eq!(diag_code(&world.install_on(Tier::T3, Side::Client, caller, Vec::new())), "ok");
    world.settle();
    world
}

/// REQ: WIT:ilium:host/commands.set-state
#[test]
fn wit_commands_set_state_on_own_command_returns_ok() {
    let mut world = commands_world();
    assert!(world.set_command_state(OWNER_PLUGIN, "example.notes/save", false, Some(true)).is_ok());
}

/// REQ: WIT:ilium:host/commands.set-state
#[test]
fn wit_commands_set_state_on_another_plugins_command_fails_not_found() {
    let mut world = commands_world();
    let error = err_of(world.set_command_state(CALLER, "example.notes/save", false, None));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/commands.set-state
#[test]
fn wit_commands_set_state_on_an_unknown_command_fails_not_found() {
    let mut world = commands_world();
    let error = err_of(world.set_command_state(OWNER_PLUGIN, "example.notes/none", true, None));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/commands.set-state
#[test]
fn wit_commands_set_state_from_a_server_instance_fails_unsupported() {
    let mut world = ExtWorld::new();
    let owner = json!({"id": OWNER_PLUGIN, "contributes": {"commands": [{"id": "save", "title": "Save note"}]}});
    assert_eq!(diag_code(&world.install_on(Tier::T3, Side::Server, owner, Vec::new())), "ok");
    let error = err_of(world.set_command_state(OWNER_PLUGIN, "example.notes/save", false, None));
    assert_eq!(code_of(&error), ErrorCode::Unsupported);
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_with_grant_returns_a_request_id() {
    let mut world = commands_world();
    assert!(world.invoke_command(CALLER, "example.notes/save", "{}").is_ok());
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_of_an_unknown_command_fails_not_found() {
    let mut world = commands_world();
    let error = err_of(world.invoke_command(CALLER, "example.notes/none", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_outside_the_granted_pattern_fails_not_permitted() {
    let mut world = commands_world();
    let other = json!({"id": "example.other", "contributes": {"commands": [{"id": "go", "title": "Go"}]}});
    assert_eq!(diag_code(&world.install_on(Tier::T3, Side::Client, other, Vec::new())), "ok");
    let error = err_of(world.invoke_command(CALLER, "example.other/go", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_of_a_disabled_command_fails_conflict() {
    let mut world = commands_world();
    assert!(world.set_command_state(OWNER_PLUGIN, "example.notes/save", false, None).is_ok());
    let error = err_of(world.invoke_command(CALLER, "example.notes/save", "{}"));
    assert_eq!(code_of(&error), ErrorCode::Conflict);
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_works_again_after_the_command_is_enabled() {
    let mut world = commands_world();
    assert!(world.set_command_state(OWNER_PLUGIN, "example.notes/save", false, None).is_ok());
    assert!(world.set_command_state(OWNER_PLUGIN, "example.notes/save", true, None).is_ok());
    assert!(world.invoke_command(CALLER, "example.notes/save", "{}").is_ok());
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_arguments_must_be_one_json_object() {
    let mut world = commands_world();
    for bad in ["[]", "5", "not json", "\"text\""] {
        let error = err_of(world.invoke_command(CALLER, "example.notes/save", bad));
        assert_eq!(code_of(&error), ErrorCode::InvalidArgument, "{bad}");
    }
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_arguments_of_64_kib_are_accepted_and_one_byte_more_is_not() {
    let mut world = commands_world();
    let overhead = r#"{"a":""}"#.len();
    let ok = format!(r#"{{"a":"{}"}}"#, "x".repeat(KIB64 - overhead));
    let too_big = format!(r#"{{"a":"{}"}}"#, "x".repeat(KIB64 - overhead + 1));
    assert!(world.invoke_command(CALLER, "example.notes/save", &ok).is_ok());
    let error = err_of(world.invoke_command(CALLER, "example.notes/save", &too_big));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_arguments_failing_the_schema_fail_invalid_argument() {
    let mut world = commands_world();
    let error = err_of(world.invoke_command(CALLER, "example.notes/strict", r#"{"n":"x"}"#));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(world.invoke_command(CALLER, "example.notes/strict", r#"{"n":3}"#).is_ok());
}

/// REQ: WIT:ilium:host/commands.invoke
#[test]
fn wit_commands_invoke_not_answered_in_30_s_completes_failed_timed_out() {
    let mut world = commands_world();
    world.script_command("example.notes/save", HandlerScript { elapsed_ms: 0, action: HandlerAction::ServiceNever });
    let id = world.invoke_command(CALLER, "example.notes/save", "{}").unwrap_or(RequestId(0));
    world.advance_ms(30_000);
    world.settle();
    match world.outcome(CALLER, id) {
        Some(Outcome::Failed(error)) => assert_eq!(code_of(&error), ErrorCode::TimedOut),
        other => assert!(false, "expected failed(timed-out), got {other:?}"),
    }
}
