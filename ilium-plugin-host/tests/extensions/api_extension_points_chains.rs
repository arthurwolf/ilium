//! api-extension-points.md: run-hook argument checks, the four hook modes, the chain cap,
//! deadlines, breaches, nesting and the owner action (EXT-004, 005, 010..014, 020..022, 029, 046).

use crate::support::*;

const PAYLOAD: &str = r#"{"text":"a","n":1}"#;

fn text_of(json_text: &str) -> String {
    let value: Value = serde_json::from_str(json_text).unwrap_or(Value::Null);
    value["text"].as_str().unwrap_or("").to_owned()
}

fn breach_free(world: &ExtWorld) -> bool {
    !has_log(world, "extension.disabled")
}

// ---------------------------------------------------------------------------------------------
// EXT-004
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-004, WIT:ilium:host/extensions.run-hook
#[test]
fn ext_004_payload_of_65_kib_fails_cap_exceeded_invalid_argument() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    let error = err_of(world.run_hook(OWNER, POINT, &payload_of_size(KIB64 + 1024)));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(message_has_prefix(&error, "extension.cap-exceeded"));
    assert!(world.handler_runs().is_empty());
}

/// REQ: EXT-004
#[test]
fn ext_004_payload_of_64_kib_plus_one_byte_fails_cap_exceeded() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    let error = err_of(world.run_hook(OWNER, POINT, &payload_of_size(KIB64 + 1)));
    assert!(message_has_prefix(&error, "extension.cap-exceeded"));
}

/// REQ: EXT-004
#[test]
fn ext_004_payload_of_exactly_64_kib_is_accepted() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    assert!(world
        .run_hook(OWNER, POINT, &payload_of_size(KIB64))
        .is_ok());
}

/// REQ: EXT-004
#[test]
fn ext_004_payload_of_64_kib_minus_one_byte_is_accepted() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    assert!(world
        .run_hook(OWNER, POINT, &payload_of_size(KIB64 - 1))
        .is_ok());
}

/// REQ: EXT-004
#[test]
fn ext_004_one_kib_payload_of_wrong_shape_fails_schema_mismatch() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    let wrong = format!(r#"{{"text":{}}}"#, "1".repeat(1000));
    let error = err_of(world.run_hook(OWNER, POINT, &wrong));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(message_has_prefix(&error, "extension.schema-mismatch"));
    assert!(world.handler_runs().is_empty());
}

/// REQ: EXT-004
#[test]
fn ext_004_payload_that_is_not_json_fails_schema_mismatch() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    let error = err_of(world.run_hook(OWNER, POINT, "not json"));
    assert!(message_has_prefix(&error, "extension.schema-mismatch"));
}

// ---------------------------------------------------------------------------------------------
// EXT-005 and HST-472 chain cap
// ---------------------------------------------------------------------------------------------

fn cap_world(handlers: usize, ms: u32, deadline: u64) -> ExtWorld {
    let point = with(hook_point("p", "observe"), "deadline_ms", json!(deadline));
    let contribs = (0..handlers)
        .map(|index| contrib(&format!("h{index}"), index as u8, pass(ms)))
        .collect();
    hook_world_with(point, contribs)
}

/// REQ: EXT-005, HST-472
#[test]
fn ext_005_four_handlers_of_40_ms_start_at_0_40_80_and_fourth_is_skipped() {
    let mut world = cap_world(4, 40, 50);
    let _ = run_chain(&mut world, PAYLOAD);
    let starts: Vec<u64> = world
        .handler_runs()
        .iter()
        .map(|run| run.start_ms)
        .collect();
    assert_eq!(starts, vec![0, 40, 80]);
    assert!(has_log(&world, "extension.chain-cap"));
}

/// REQ: EXT-005, HST-472
#[test]
fn ext_005_skipped_handlers_are_not_breaches() {
    let mut world = cap_world(6, 40, 50);
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    assert!(breach_free(&world));
    assert_eq!(
        world.handler_runs().len(),
        9,
        "three handlers per chain, three chains"
    );
}

/// REQ: EXT-005, HST-472
#[test]
fn ext_005_handler_started_before_100_ms_runs_to_its_own_deadline() {
    let point = with(hook_point("p", "observe"), "deadline_ms", json!(50));
    let mut world = hook_world_with(
        point,
        vec![contrib("a", 0, pass(95)), contrib("b", 1, pass(50))],
    );
    let _ = run_chain(&mut world, PAYLOAD);
    let starts: Vec<u64> = world
        .handler_runs()
        .iter()
        .map(|run| run.start_ms)
        .collect();
    assert_eq!(
        starts,
        vec![0, 95],
        "the second handler starts at 95 ms and is not cut at 100 ms"
    );
    assert!(!has_log(&world, "extension.handler-timeout"));
}

/// REQ: EXT-005, HST-472
#[test]
fn ext_005_handler_due_exactly_at_100_ms_is_skipped() {
    let point = with(hook_point("p", "observe"), "deadline_ms", json!(50));
    let mut world = hook_world_with(
        point,
        vec![
            contrib("a", 0, pass(50)),
            contrib("b", 1, pass(50)),
            contrib("c", 2, pass(1)),
        ],
    );
    let _ = run_chain(&mut world, PAYLOAD);
    let starts: Vec<u64> = world
        .handler_runs()
        .iter()
        .map(|run| run.start_ms)
        .collect();
    assert_eq!(starts, vec![0, 50]);
}

/// REQ: EXT-005, HST-472
#[test]
fn ext_005_handler_due_at_99_ms_still_starts() {
    let point = with(hook_point("p", "observe"), "deadline_ms", json!(50));
    let mut world = hook_world_with(
        point,
        vec![
            contrib("a", 0, pass(50)),
            contrib("b", 1, pass(49)),
            contrib("c", 2, pass(1)),
        ],
    );
    let _ = run_chain(&mut world, PAYLOAD);
    let starts: Vec<u64> = world
        .handler_runs()
        .iter()
        .map(|run| run.start_ms)
        .collect();
    assert_eq!(starts, vec![0, 50, 99]);
}

/// REQ: EXT-005, HST-472
#[test]
fn ext_005_cap_is_measured_from_the_first_handler_not_from_run_hook() {
    let mut world = cap_world(3, 40, 50);
    world.advance_ms(500);
    let _ = run_chain(&mut world, PAYLOAD);
    let first = world
        .handler_runs()
        .first()
        .map(|run| run.start_ms)
        .unwrap_or(0);
    let last = world
        .handler_runs()
        .last()
        .map(|run| run.start_ms)
        .unwrap_or(0);
    assert_eq!(last - first, 80);
}

// ---------------------------------------------------------------------------------------------
// HST-470, HST-471
// ---------------------------------------------------------------------------------------------

/// REQ: HST-470, WIT:ilium:host/extensions.run-hook
#[test]
fn hst_470_run_hook_returns_a_request_id_before_any_handler_runs() {
    let mut world = hook_world("veto", vec![contrib("c1", 50, pass(30))]);
    let request = world.run_hook(OWNER, POINT, PAYLOAD);
    assert!(request.is_ok());
    assert!(
        world.handler_runs().is_empty(),
        "no handler ran inside the call"
    );
    assert_eq!(world.now_ms(), 0, "the call consumed no virtual time");
}

/// REQ: HST-470
#[test]
fn hst_470_completion_arrives_as_request_completed_later() {
    let mut world = hook_world("veto", vec![contrib("c1", 50, pass(30))]);
    let request = world.run_hook(OWNER, POINT, PAYLOAD);
    let id = match request {
        Ok(id) => id,
        Err(error) => {
            assert!(false, "{error:?}");
            return;
        }
    };
    assert!(world.outcome(OWNER, id).is_none());
    world.settle();
    assert!(matches!(world.outcome(OWNER, id), Some(Outcome::Ok)));
}

/// REQ: HST-471
#[test]
fn hst_471_handlers_of_one_chain_run_strictly_one_after_another() {
    let mut world = hook_world(
        "observe",
        vec![
            contrib("a", 1, pass(10)),
            contrib("b", 2, pass(10)),
            contrib("c", 3, pass(10)),
        ],
    );
    let _ = run_chain(&mut world, PAYLOAD);
    let starts: Vec<u64> = world
        .handler_runs()
        .iter()
        .map(|run| run.start_ms)
        .collect();
    assert_eq!(starts, vec![0, 10, 20]);
}

/// REQ: HST-471, EXT-021
#[test]
fn hst_471_busy_instance_is_skipped_as_pass_and_counts_a_breach() {
    let mut world = hook_world(
        "observe",
        vec![contrib("busy", 1, pass(1)), contrib("ok", 2, pass(1))],
    );
    world.set_instance_busy("com.example.busy", true);
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(run_order(&world), vec!["com.example.ok"]);
    assert_eq!(world.breach_count("com.example.busy", POINT, "e-busy"), 1);
}

/// REQ: HST-471
#[test]
fn hst_471_activating_instance_is_skipped_as_pass_without_a_breach() {
    let mut world = hook_world(
        "observe",
        vec![contrib("slow", 1, pass(1)), contrib("ok", 2, pass(1))],
    );
    world.set_instance_activating("com.example.slow", true);
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(run_order(&world), vec!["com.example.ok"]);
    assert_eq!(world.breach_count("com.example.slow", POINT, "e-slow"), 0);
}

/// REQ: HST-473
#[test]
fn hst_473_helper_round_trip_of_a_t2_handler_counts_inside_the_deadline() {
    let mut world = hook_world("observe", vec![contrib_tier("t2", 1, Tier::T2, pass(8))]);
    world.set_helper_round_trip_ms(15);
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(
        has_log(&world, "extension.handler-timeout"),
        "8 ms of work plus 15 ms of helper exceeds 20 ms"
    );
}

/// REQ: HST-473
#[test]
fn hst_473_t2_handler_within_deadline_including_round_trip_passes_without_timeout() {
    let mut world = hook_world("observe", vec![contrib_tier("t2", 1, Tier::T2, pass(4))]);
    world.set_helper_round_trip_ms(15);
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(!has_log(&world, "extension.handler-timeout"));
}

/// REQ: HST-474
#[test]
fn hst_474_client_owned_point_runs_only_client_side_handlers() {
    let mut world = ExtWorld::new();
    let owner = owner_manifest_on(OWNER, "client", vec![hook_point("p", "observe")]);
    assert_eq!(diag_code(&world.install(Tier::T3, owner, Vec::new())), "ok");
    let client = contributor_manifest_on(
        "com.example.cli",
        "client",
        vec![ext_handler(POINT, "x", 50, "h")],
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, client, vec![("h".to_owned(), pass(1))])),
        "ok"
    );
    let server_plugin = contributor_manifest_on(
        "com.example.srv",
        "server",
        vec![json!({"point": "other.plugin/q", "id": "y", "priority": 50, "handler": "h"})],
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, server_plugin, vec![("h".to_owned(), pass(1))])),
        "ok"
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(run_order(&world), vec!["com.example.cli"]);
}

// ---------------------------------------------------------------------------------------------
// EXT-010..013 modes
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-010
#[test]
fn ext_010_observe_calls_all_handlers_and_ignores_replies() {
    let mut world = hook_world(
        "observe",
        vec![
            contrib("a", 1, replace(1, r#"{"text":"b","n":1}"#)),
            contrib("b", 2, pass(1)),
            contrib("c", 3, refuse(1, "no")),
        ],
    );
    let (outcome, reply) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(reply, None);
    assert_eq!(run_order(&world).len(), 3);
    assert!(has_log(&world, "extension.reply-not-allowed"));
}

/// REQ: EXT-010
#[test]
fn ext_010_observe_with_no_handlers_completes_ok() {
    let mut world = hook_world("observe", Vec::new());
    let (outcome, reply) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(reply, None);
}

/// REQ: EXT-011
#[test]
fn ext_011_second_transform_handler_sees_the_output_of_the_first() {
    let mut world = hook_world(
        "transform",
        vec![
            contrib("a", 1, replace(1, r#"{"text":"b","n":1}"#)),
            contrib("b", 2, replace(1, r#"{"text":"c","n":1}"#)),
        ],
    );
    let (outcome, reply) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    let runs = world.handler_runs();
    assert_eq!(text_of(&runs[0].payload_json), "a");
    assert_eq!(text_of(&runs[1].payload_json), "b");
    assert_eq!(reply.map(|text| text_of(&text)), Some("c".to_owned()));
}

/// REQ: EXT-011
#[test]
fn ext_011_take_reply_is_none_when_every_handler_passes() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, pass(1)), contrib("b", 2, pass(1))],
    );
    let (outcome, reply) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(reply, None);
}

/// REQ: EXT-011
#[test]
fn ext_011_one_accepted_replace_among_passes_gives_that_payload() {
    let mut world = hook_world(
        "transform",
        vec![
            contrib("a", 1, pass(1)),
            contrib("b", 2, replace(1, r#"{"text":"z","n":1}"#)),
            contrib("c", 3, pass(1)),
        ],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert_eq!(reply.map(|text| text_of(&text)), Some("z".to_owned()));
}

/// REQ: EXT-011, WIT:ilium:host/extensions.take-reply
#[test]
fn ext_011_take_reply_returns_the_payload_only_once() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, replace(1, r#"{"text":"b","n":1}"#))],
    );
    let request = world.run_hook(OWNER, POINT, PAYLOAD);
    world.settle();
    let id = request.unwrap_or(RequestId(0));
    assert!(world.take_reply(OWNER, id).is_some());
    assert!(world.take_reply(OWNER, id).is_none());
}

/// REQ: EXT-012
#[test]
fn ext_012_first_refuse_stops_the_chain_with_conflict_vetoed() {
    let mut world = hook_world(
        "veto",
        vec![contrib("a", 1, refuse(1, "no")), contrib("b", 2, pass(1))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    let error = refused_error(&outcome);
    assert_eq!(code_of(&error), ErrorCode::Conflict);
    assert!(error.message.starts_with("extension.vetoed: "));
    assert_eq!(run_order(&world), vec!["com.example.a"]);
}

/// REQ: EXT-012
#[test]
fn ext_012_second_handler_refusing_runs_both_and_refuses() {
    let mut world = hook_world(
        "veto",
        vec![contrib("a", 1, pass(1)), contrib("b", 2, refuse(1, "late"))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert_eq!(code_of(&refused_error(&outcome)), ErrorCode::Conflict);
    assert_eq!(run_order(&world).len(), 2);
}

/// REQ: EXT-012
#[test]
fn ext_012_veto_without_refuse_completes_ok() {
    let mut world = hook_world(
        "veto",
        vec![contrib("a", 1, pass(1)), contrib("b", 2, pass(1))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
}

/// REQ: EXT-013
#[test]
fn ext_013_first_answer_stops_the_chain_and_take_reply_returns_it() {
    let mut world = hook_world(
        "first",
        vec![
            contrib("a", 1, answer(1, r#"{"answer":"x"}"#)),
            contrib("b", 2, pass(1)),
        ],
    );
    let (outcome, reply) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(reply, Some(r#"{"answer":"x"}"#.to_owned()));
    assert_eq!(run_order(&world).len(), 1);
}

/// REQ: EXT-013
#[test]
fn ext_013_no_answer_completes_ok_and_take_reply_is_nothing() {
    let mut world = hook_world(
        "first",
        vec![contrib("a", 1, pass(1)), contrib("b", 2, pass(1))],
    );
    let (outcome, reply) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(reply, None);
    assert_eq!(run_order(&world).len(), 2);
}

// ---------------------------------------------------------------------------------------------
// EXT-014 reply checks
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-014
#[test]
fn ext_014_refuse_in_observe_mode_counts_as_pass_with_one_log_line() {
    let mut world = hook_world("observe", vec![contrib("a", 1, refuse(1, "no"))]);
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(count_logs(&world, "extension.reply-not-allowed"), 1);
}

/// REQ: EXT-014
#[test]
fn ext_014_replace_in_veto_mode_counts_as_pass_with_a_log_line() {
    let mut world = hook_world(
        "veto",
        vec![contrib("a", 1, replace(1, r#"{"text":"b","n":1}"#))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert!(has_log(&world, "extension.reply-not-allowed"));
}

/// REQ: EXT-014
#[test]
fn ext_014_answer_in_transform_mode_counts_as_pass_with_a_log_line() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, answer(1, r#"{"answer":"x"}"#))],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert_eq!(reply, None);
    assert!(has_log(&world, "extension.reply-not-allowed"));
}

/// REQ: EXT-014
#[test]
fn ext_014_transform_replace_changing_a_field_outside_reply_schema_is_schema_mismatch() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, replace(1, r#"{"text":"a","n":2}"#))],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert_eq!(reply, None);
    assert!(has_log(&world, "extension.schema-mismatch"));
}

/// REQ: EXT-014
#[test]
fn ext_014_transform_replace_changing_only_listed_field_is_accepted() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, replace(1, r#"{"text":"b","n":1}"#))],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert_eq!(reply.map(|text| text_of(&text)), Some("b".to_owned()));
    assert!(!has_log(&world, "extension.schema-mismatch"));
}

/// REQ: EXT-014
#[test]
fn ext_014_transform_replace_failing_payload_schema_is_schema_mismatch() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, replace(1, r#"{"text":5,"n":1}"#))],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert_eq!(reply, None);
    assert!(has_log(&world, "extension.schema-mismatch"));
}

/// REQ: EXT-014
#[test]
fn ext_014_first_answer_failing_reply_schema_is_schema_mismatch_and_chain_continues() {
    let mut world = hook_world(
        "first",
        vec![
            contrib("a", 1, answer(1, r#"{"nope":1}"#)),
            contrib("b", 2, answer(1, r#"{"answer":"ok"}"#)),
        ],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert!(has_log(&world, "extension.schema-mismatch"));
    assert_eq!(reply, Some(r#"{"answer":"ok"}"#.to_owned()));
}

/// REQ: EXT-014, EXT-021
#[test]
fn ext_014_schema_mismatch_counts_as_a_breach() {
    let mut world = hook_world(
        "transform",
        vec![contrib("a", 1, replace(1, r#"{"text":"a","n":2}"#))],
    );
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.a", POINT, "e-a"), 1);
}

/// REQ: EXT-014, EXT-021
#[test]
fn ext_014_reply_not_allowed_is_not_a_breach() {
    let mut world = hook_world("observe", vec![contrib("a", 1, refuse(1, "no"))]);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.a", POINT, "e-a"), 0);
}

// ---------------------------------------------------------------------------------------------
// EXT-020 deadlines
// ---------------------------------------------------------------------------------------------

fn install_with_deadline(deadline: u64) -> Result<(), Diagnostic> {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "observe"), "deadline_ms", json!(deadline));
    world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())
}

/// REQ: EXT-020
#[test]
fn ext_020_deadline_4_ms_fails_the_manifest() {
    // manual lists extension-point-invalid for bad point fields; it does not pin a deadline code
    assert_eq!(
        diag_code(&install_with_deadline(4)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-020
#[test]
fn ext_020_deadline_5_ms_installs() {
    assert_eq!(diag_code(&install_with_deadline(5)), "ok");
}

/// REQ: EXT-020
#[test]
fn ext_020_deadline_50_ms_installs() {
    assert_eq!(diag_code(&install_with_deadline(50)), "ok");
}

/// REQ: EXT-020
#[test]
fn ext_020_deadline_51_ms_fails_the_manifest() {
    // manual lists extension-point-invalid for bad point fields; it does not pin a deadline code
    assert_eq!(
        diag_code(&install_with_deadline(51)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-020
#[test]
fn ext_020_missing_deadline_defaults_to_20_ms() {
    let mut point = hook_point("p", "veto");
    if let Some(object) = point.as_object_mut() {
        object.remove("deadline_ms");
    }
    let mut world = hook_world_with(
        point,
        vec![
            contrib("slow", 1, refuse(21, "late")),
            contrib("fast", 2, refuse(20, "in time")),
        ],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(
        matches!(outcome, Some(Outcome::Ok)),
        "the 21 ms refuse timed out and did not veto: {outcome:?}"
    );
    assert!(has_log(&world, "extension.handler-timeout"));
}

/// REQ: EXT-020
#[test]
fn ext_020_handler_of_80_ms_at_the_default_counts_as_pass_and_logs_timeout() {
    let mut world = hook_world("veto", vec![contrib("slow", 1, refuse(80, "late"))]);
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert!(has_log(&world, "extension.handler-timeout"));
}

/// REQ: EXT-020
#[test]
fn ext_020_handler_finishing_at_the_deadline_is_not_a_timeout() {
    let mut world = hook_world("veto", vec![contrib("edge", 1, refuse(20, "edge"))]);
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert_eq!(code_of(&refused_error(&outcome)), ErrorCode::Conflict);
    assert!(!has_log(&world, "extension.handler-timeout"));
}

/// REQ: EXT-020
#[test]
fn ext_020_trap_counts_as_pass() {
    let mut world = hook_world(
        "veto",
        vec![contrib("t", 1, trap(1)), contrib("n", 2, pass(1))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(run_order(&world).len(), 2);
}

/// REQ: EXT-020, WIT:ilium:exports/hook-handler.handle-hook
#[test]
fn ext_020_error_return_counts_as_pass() {
    let mut world = hook_world(
        "veto",
        vec![contrib("e", 1, error_return(1)), contrib("n", 2, pass(1))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
}

// ---------------------------------------------------------------------------------------------
// EXT-021 breaches
// ---------------------------------------------------------------------------------------------

fn slow_world() -> ExtWorld {
    hook_world(
        "veto",
        vec![contrib("slow", 1, pass(80)), contrib("other", 2, pass(1))],
    )
}

/// REQ: EXT-021
#[test]
fn ext_021_three_timeouts_stop_the_extension_and_log_disabled_on_the_next_run() {
    let mut world = slow_world();
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(has_log(&world, "extension.disabled"));
    assert!(!world.is_active("com.example.slow", POINT, "e-slow"));
    let slow_runs = |world: &ExtWorld| {
        world
            .handler_runs()
            .iter()
            .filter(|run| run.plugin == "com.example.slow")
            .count()
    };
    let runs_before = slow_runs(&world);
    assert_eq!(runs_before, 3, "the slow handler ran exactly three times");
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(
        slow_runs(&world),
        runs_before,
        "a stopped extension never runs again"
    );
}

/// REQ: EXT-021
#[test]
fn ext_021_two_timeouts_do_not_stop_the_extension() {
    let mut world = slow_world();
    for _ in 0..2 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.slow", POINT, "e-slow"), 3);
}

/// REQ: EXT-021
#[test]
fn ext_021_stopped_extension_is_not_called_again() {
    let mut world = slow_world();
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    let before = world
        .handler_runs()
        .iter()
        .filter(|run| run.plugin == "com.example.slow")
        .count();
    let _ = run_chain(&mut world, PAYLOAD);
    let after = world
        .handler_runs()
        .iter()
        .filter(|run| run.plugin == "com.example.slow")
        .count();
    assert_eq!(before, after);
}

/// REQ: EXT-021
#[test]
fn ext_021_accepted_reply_within_deadline_resets_the_counter() {
    let mut world = hook_world("veto", vec![contrib("flaky", 1, pass(80))]);
    let _ = run_chain(&mut world, PAYLOAD);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.flaky", POINT, "e-flaky"), 2);
    world.set_script("com.example.flaky", "h", pass(1));
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.flaky", POINT, "e-flaky"), 0);
    world.set_script("com.example.flaky", "h", pass(80));
    let _ = run_chain(&mut world, PAYLOAD);
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(
        breach_free(&world),
        "two timeouts after a good call do not stop the extension"
    );
}

/// REQ: EXT-021
#[test]
fn ext_021_trap_error_and_oversize_reply_are_breaches() {
    let mut world = hook_world("transform", vec![contrib("t", 1, trap(1))]);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.t", POINT, "e-t"), 1);
    let mut world = hook_world("transform", vec![contrib("e", 1, error_return(1))]);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.e", POINT, "e-e"), 1);
    let big = format!(r#"{{"text":"{}","n":1}}"#, "a".repeat(KIB64));
    let mut world = hook_world("transform", vec![contrib("b", 1, replace(1, &big))]);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.breach_count("com.example.b", POINT, "e-b"), 1);
}

/// REQ: EXT-021
#[test]
fn ext_021_chain_cap_skip_is_not_a_breach() {
    let mut world = cap_world(5, 40, 50);
    for _ in 0..4 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    assert_eq!(world.breach_count("com.example.h4", POINT, "e-h4"), 0);
}

/// REQ: EXT-021
#[test]
fn ext_021_stop_is_not_persisted_and_ends_when_the_instance_restarts() {
    let mut world = slow_world();
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    world.restart("com.example.slow");
    world.set_script("com.example.slow", "h", pass(1));
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(
        world
            .handler_runs()
            .iter()
            .filter(|run| run.plugin == "com.example.slow")
            .count()
            >= 4
    );
}

/// REQ: EXT-021
#[test]
fn ext_021_reset_ends_the_stop_and_the_notice_offers_it() {
    let mut world = slow_world();
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    assert!(world
        .notices()
        .iter()
        .any(|notice| notice.contains("com.example.slow") && notice.contains(POINT)));
    world.reset_extension("com.example.slow", POINT, "e-slow");
    world.set_script("com.example.slow", "h", pass(1));
    let before = world.handler_runs().len();
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(world.handler_runs().len() > before);
    assert_eq!(world.breach_count("com.example.slow", POINT, "e-slow"), 0);
}

/// REQ: EXT-021
#[test]
fn ext_021_counter_is_per_extension_id() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![hook_point("p", "veto")]),
            Vec::new()
        )),
        "ok"
    );
    let manifest = contributor_manifest(
        "com.example.two",
        vec![
            ext_handler(POINT, "bad", 1, "hb"),
            ext_handler(POINT, "good", 2, "hg"),
        ],
    );
    let handlers = vec![("hb".to_owned(), pass(80)), ("hg".to_owned(), pass(1))];
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    assert_eq!(world.breach_count("com.example.two", POINT, "bad"), 3);
    assert_eq!(world.breach_count("com.example.two", POINT, "good"), 0);
}

// ---------------------------------------------------------------------------------------------
// EXT-022 reentrancy
// ---------------------------------------------------------------------------------------------

/// A world with points a and b of one owner, a's handler runs b and b's handler runs a.
fn mutual_world() -> ExtWorld {
    let mut world = ExtWorld::new();
    let owner = owner_manifest(
        OWNER,
        vec![hook_point("a", "observe"), hook_point("b", "observe")],
    );
    assert_eq!(diag_code(&world.install(Tier::T3, owner, Vec::new())), "ok");
    let manifest = json!({
        "id": "com.example.loop",
        "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:com.example.owner/a", "extensions.contribute:com.example.owner/b", "extensions.provide:com.example.owner/a"]},
        "contributes": {"extensions": [
            ext_handler("com.example.owner/a", "xa", 50, "ha"),
            ext_handler("com.example.owner/b", "xb", 50, "hb")
        ]}
    });
    let handlers = vec![
        (
            "ha".to_owned(),
            runs_hook(1, "com.example.owner/b", PAYLOAD),
        ),
        (
            "hb".to_owned(),
            runs_hook(1, "com.example.owner/a", PAYLOAD),
        ),
    ];
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    world
}

/// REQ: EXT-022
#[test]
fn ext_022_indirect_loop_fails_reentrant_with_conflict() {
    let mut world = mutual_world();
    let _ = run_chain_on(&mut world, OWNER, "com.example.owner/a", PAYLOAD);
    let errors = world.hook_errors();
    let found = errors
        .iter()
        .find(|(_, error)| message_has_prefix(error, "extension.reentrant"));
    assert!(found.is_some(), "{errors:?}");
    assert_eq!(
        found.map(|(_, error)| code_of(error)),
        Some(ErrorCode::Conflict)
    );
}

/// REQ: EXT-022
#[test]
fn ext_022_direct_self_run_fails_reentrant() {
    let mut world = hook_world(
        "observe",
        vec![contrib("self", 1, runs_hook(1, POINT, PAYLOAD))],
    );
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(world
        .hook_errors()
        .iter()
        .any(|(_, error)| message_has_prefix(error, "extension.reentrant")));
}

/// REQ: EXT-022
#[test]
fn ext_022_loop_counts_as_pass_and_not_as_a_breach() {
    let mut world = hook_world(
        "observe",
        vec![contrib("self", 1, runs_hook(1, POINT, PAYLOAD))],
    );
    let (outcome, _) = run_chain(&mut world, PAYLOAD);
    assert!(matches!(outcome, Some(Outcome::Ok)));
    assert_eq!(world.breach_count("com.example.self", POINT, "e-self"), 0);
}

// ---------------------------------------------------------------------------------------------
// EXT-046, HST-475 depth
// ---------------------------------------------------------------------------------------------

/// A world with points p1..p4 where the handler of p(n) runs p(n+1).
fn depth_world() -> ExtWorld {
    let mut world = ExtWorld::new();
    let points: Vec<Value> = (1..=4)
        .map(|index| hook_point(&format!("p{index}"), "observe"))
        .collect();
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, points), Vec::new())),
        "ok"
    );
    let mut exts = Vec::new();
    let mut grants = Vec::new();
    let mut handlers = Vec::new();
    for index in 1..=4 {
        let point = format!("com.example.owner/p{index}");
        grants.push(format!("extensions.contribute:{point}"));
        exts.push(ext_handler(
            &point,
            &format!("x{index}"),
            50,
            &format!("h{index}"),
        ));
        let script_for = if index < 4 {
            runs_hook(1, &format!("com.example.owner/p{}", index + 1), PAYLOAD)
        } else {
            pass(1)
        };
        handlers.push((format!("h{index}"), script_for));
    }
    grants.push("extensions.provide:com.example.owner/p1".to_owned());
    let manifest = json!({
        "id": "com.example.nest", "exports": ["hook-handler"],
        "capabilities": {"server": grants}, "contributes": {"extensions": exts}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    world
}

/// REQ: EXT-046, SEC-289, HST-475
#[test]
fn ext_046_outermost_chain_is_depth_1_and_inner_chains_are_2_and_3() {
    let mut world = depth_world();
    let _ = run_chain_on(&mut world, OWNER, "com.example.owner/p1", PAYLOAD);
    let depths: Vec<u8> = world.handler_runs().iter().map(|run| run.depth).collect();
    assert_eq!(&depths[..3], &[1, 2, 3]);
}

/// REQ: EXT-046, SEC-289, HST-475
#[test]
fn ext_046_run_hook_from_depth_3_fails_with_depth_exceeded_conflict() {
    let mut world = depth_world();
    let _ = run_chain_on(&mut world, OWNER, "com.example.owner/p1", PAYLOAD);
    let errors = world.hook_errors();
    let found = errors
        .iter()
        .find(|(_, error)| message_has_prefix(error, "extension.depth-exceeded"));
    assert!(found.is_some(), "{errors:?}");
    assert_eq!(
        found.map(|(_, error)| code_of(error)),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        world.handler_runs().len(),
        3,
        "the fourth handler never ran"
    );
}

/// REQ: EXT-046, SEC-289
#[test]
fn ext_046_depth_exceeded_counts_as_pass_and_no_breach() {
    let mut world = depth_world();
    let _ = run_chain_on(&mut world, OWNER, "com.example.owner/p1", PAYLOAD);
    assert_eq!(
        world.breach_count("com.example.nest", "com.example.owner/p3", "x3"),
        0
    );
}

/// REQ: EXT-046, SEC-289, HST-475
#[test]
fn ext_046_inner_chain_runs_after_the_handler_returns_and_outside_its_deadline() {
    let mut world = ExtWorld::new();
    let points = vec![
        with(hook_point("p1", "observe"), "deadline_ms", json!(20)),
        with(hook_point("p2", "observe"), "deadline_ms", json!(50)),
    ];
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, points), Vec::new())),
        "ok"
    );
    let outer = "com.example.owner/p1";
    let inner = "com.example.owner/p2";
    let manifest = json!({
        "id": "com.example.nest", "exports": ["hook-handler"],
        "capabilities": {"server": [
            format!("extensions.contribute:{outer}"),
            format!("extensions.contribute:{inner}"),
            format!("extensions.provide:{outer}"),
        ]},
        "contributes": {"extensions": [
            ext_handler(outer, "x1", 50, "h1"),
            ext_handler(inner, "x2", 50, "h2"),
        ]}
    });
    let handlers = vec![
        ("h1".to_owned(), runs_hook(15, inner, PAYLOAD)),
        // The inner chain takes 30 ms: longer than the outer 20 ms deadline.
        ("h2".to_owned(), pass(30)),
    ];
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    let _ = run_chain_on(&mut world, OWNER, outer, PAYLOAD);
    let depths: Vec<u8> = world.handler_runs().iter().map(|run| run.depth).collect();
    assert_eq!(
        depths,
        vec![1, 2],
        "the inner chain ran after the outer handler"
    );
    assert!(!has_log(&world, "extension.handler-timeout"));
}

// ---------------------------------------------------------------------------------------------
// EXT-029 owner action
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-029
#[test]
fn ext_029_veto_refusing_after_30_ms_prevents_the_action() {
    let point = with(hook_point("p", "veto"), "deadline_ms", json!(50));
    let mut world = hook_world_with(point, vec![contrib("v", 1, refuse(30, "no"))]);
    let request = world
        .run_hook(OWNER, POINT, PAYLOAD)
        .unwrap_or(RequestId(0));
    world.advance_ms(29);
    assert!(
        world.outcome(OWNER, request).is_none(),
        "the action is still held at 29 ms"
    );
    world.settle();
    assert_eq!(
        code_of(&refused_error(&world.outcome(OWNER, request))),
        ErrorCode::Conflict
    );
}

/// REQ: EXT-029
#[test]
fn ext_029_owner_keeps_serving_other_events_while_a_veto_chain_runs() {
    let point = with(hook_point("p", "veto"), "deadline_ms", json!(50));
    let mut world = hook_world_with(point, vec![contrib("v", 1, pass(30))]);
    let first = world.run_hook(OWNER, POINT, PAYLOAD);
    let second = world.run_hook(OWNER, POINT, PAYLOAD);
    assert!(
        first.is_ok() && second.is_ok(),
        "owner calls are not blocked"
    );
}

/// REQ: EXT-029
#[test]
fn ext_029_observe_completes_before_a_slow_handler_finishes() {
    let mut world = hook_world("observe", vec![contrib("o", 1, pass(30))]);
    let request = world
        .run_hook(OWNER, POINT, PAYLOAD)
        .unwrap_or(RequestId(0));
    let action_done_at = world.now_ms();
    world.settle();
    assert_eq!(
        action_done_at, 0,
        "an observe action does not wait for its handlers"
    );
    assert!(matches!(world.outcome(OWNER, request), Some(Outcome::Ok)));
}

/// REQ: EXT-029
#[test]
fn ext_029_chain_result_is_available_within_cap_plus_one_deadline() {
    let point = with(hook_point("p", "veto"), "deadline_ms", json!(50));
    let contribs = (0..6)
        .map(|index| contrib(&format!("w{index}"), index as u8, pass(45)))
        .collect();
    let mut world = hook_world_with(point, contribs);
    let request = world
        .run_hook(OWNER, POINT, PAYLOAD)
        .unwrap_or(RequestId(0));
    world.settle();
    assert!(matches!(world.outcome(OWNER, request), Some(Outcome::Ok)));
    assert!(world.now_ms() <= 150, "chain took {} ms", world.now_ms());
}

proptest! {
    // REQ: EXT-005, HST-472, EXT-021
    #[test]
    fn ext_005_chain_never_starts_a_handler_at_or_after_100_ms_and_never_breaches(
        durations in proptest::collection::vec(1u32..=50, 1..8)
    ) {
        let contribs = durations.iter().enumerate().map(|(index, ms)| contrib(&format!("h{index}"), index as u8, pass(*ms))).collect();
        let point = with(hook_point("p", "observe"), "deadline_ms", json!(50));
        let mut world = hook_world_with(point, contribs);
        let _ = run_chain(&mut world, PAYLOAD);
        for run in world.handler_runs() {
            prop_assert!(run.start_ms < 100);
        }
        prop_assert!(!has_log(&world, "extension.disabled"));
    }
}
