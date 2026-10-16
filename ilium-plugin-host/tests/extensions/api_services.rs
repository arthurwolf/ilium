//! api-services.md: grants, limits, versions, timeouts, cancellation and replies (SVC-001..014).

use crate::support::*;

const OK_PAYLOAD: &str = r#"{"text":"hi"}"#;

fn failed_error(outcome: &Option<Outcome>) -> PluginError {
    match outcome {
        Some(Outcome::Failed(error)) => error.clone(),
        other => {
            assert!(false, "expected failed(..), got {other:?}");
            perr(ErrorCode::Internal, "unreachable")
        }
    }
}

fn deferred() -> HandlerScript {
    script(0, HandlerAction::ServiceDeferred)
}

fn never() -> HandlerScript {
    script(0, HandlerAction::ServiceNever)
}

// ---------------------------------------------------------------------------------------------
// SVC-001 grants
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-001, WIT:ilium:host/services.call
#[test]
fn svc_001_call_without_services_use_returns_not_permitted() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    assert_eq!(diag_code(&world.install(Tier::T3, json!({"id": "com.example.nogrant"}), Vec::new())), "ok");
    let error = err_of(world.call_service("com.example.nogrant", SERVICE, "echo", OK_PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

/// REQ: SVC-001
#[test]
fn svc_001_call_with_services_use_is_accepted() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    assert!(call_echo(&mut world, OK_PAYLOAD).is_ok());
}

/// REQ: SVC-001
#[test]
fn svc_001_services_use_of_another_service_does_not_help() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    let other = consumer_manifest("com.example.other", "com.example.prov/other", None);
    assert_eq!(diag_code(&world.install(Tier::T3, other, Vec::new())), "ok");
    let error = err_of(world.call_service("com.example.other", SERVICE, "echo", OK_PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

/// REQ: SVC-001
#[test]
fn svc_001_provider_without_services_provide_fails_the_install() {
    let mut world = ExtWorld::new();
    let mut manifest = provider_manifest("com.example.prov", SERVICE, "0.1.0");
    manifest["capabilities"] = json!({"server": []});
    let result = world.install(Tier::T3, manifest, vec![(method_key(SERVICE, "echo"), done(1, r#""x""#))]);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------------------------
// SVC-002 .. SVC-004
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-002
#[test]
fn svc_002_service_nobody_provides_returns_not_found_at_once() {
    let mut world = ExtWorld::new();
    let consumer = consumer_manifest("com.example.cons", "com.example.none/svc", None);
    assert_eq!(diag_code(&world.install(Tier::T3, consumer, Vec::new())), "ok");
    let error = err_of(world.call_service("com.example.cons", "com.example.none/svc", "echo", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
    assert_eq!(world.now_ms(), 0);
}

/// REQ: SVC-002
#[test]
fn svc_002_disabled_provider_returns_not_found() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    world.set_enabled("com.example.prov", false);
    let error = err_of(call_echo(&mut world, OK_PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: SVC-002
#[test]
fn svc_002_uninstalled_provider_returns_not_found() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    world.uninstall("com.example.prov");
    let error = err_of(call_echo(&mut world, OK_PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: SVC-003
#[test]
fn svc_003_payload_of_1_mib_plus_one_byte_is_invalid_argument_and_provider_not_called() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    let error = err_of(call_echo(&mut world, &echo_payload_of_size(MIB1 + 1)));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(world.service_calls_seen("com.example.prov").is_empty());
}

/// REQ: SVC-003
#[test]
fn svc_003_payload_of_exactly_1_mib_is_accepted() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    assert!(call_echo(&mut world, &echo_payload_of_size(MIB1)).is_ok());
}

/// REQ: SVC-003
#[test]
fn svc_003_payload_of_1_mib_minus_one_byte_is_accepted() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    assert!(call_echo(&mut world, &echo_payload_of_size(MIB1 - 1)).is_ok());
}

/// REQ: SVC-003
#[test]
fn svc_003_payload_failing_the_params_schema_is_invalid_argument_and_provider_not_called() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    let error = err_of(call_echo(&mut world, r#"{"text":5}"#));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(world.service_calls_seen("com.example.prov").is_empty());
}

/// REQ: SVC-003
#[test]
fn svc_003_reply_over_1_mib_completes_failed_invalid_argument() {
    let big = format!("\"{}\"", "a".repeat(MIB1));
    let mut world = service_world("0.1.0", done(1, &big));
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::InvalidArgument);
}

/// REQ: SVC-003
#[test]
fn svc_003_reply_of_exactly_1_mib_is_delivered() {
    let exact = format!("\"{}\"", "a".repeat(MIB1 - 2));
    let mut world = service_world("0.1.0", done(1, &exact));
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.settle();
    assert!(matches!(world.outcome("com.example.cons", id), Some(Outcome::Ok)));
}

/// REQ: SVC-004
#[test]
fn svc_004_method_not_in_the_schema_returns_unsupported() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    let error = err_of(world.call_service("com.example.cons", SERVICE, "x", "{}"));
    assert_eq!(code_of(&error), ErrorCode::Unsupported);
}

/// REQ: SVC-004
#[test]
fn svc_004_method_in_the_schema_is_accepted() {
    let mut world = service_world("0.1.0", done(1, r#""x""#));
    assert!(world.call_service("com.example.cons", SERVICE, "query", "{}").is_ok());
}

// ---------------------------------------------------------------------------------------------
// SVC-005, SVC-014 pending limits
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-005
#[test]
fn svc_005_sixty_four_open_calls_are_accepted() {
    let mut world = service_world("0.1.0", deferred());
    for index in 0..64 {
        assert!(call_echo(&mut world, OK_PAYLOAD).is_ok(), "call {index}");
    }
}

/// REQ: SVC-005
#[test]
fn svc_005_the_65th_open_call_returns_busy() {
    let mut world = service_world("0.1.0", deferred());
    for _ in 0..64 {
        let _ = call_echo(&mut world, OK_PAYLOAD);
    }
    let error = err_of(call_echo(&mut world, OK_PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::Busy);
}

/// REQ: SVC-005
#[test]
fn svc_005_a_completed_call_frees_a_slot() {
    let mut world = service_world("0.1.0", deferred());
    for _ in 0..64 {
        let _ = call_echo(&mut world, OK_PAYLOAD);
    }
    let first = world.service_calls_seen("com.example.prov")[0].0;
    assert!(world.service_reply("com.example.prov", first, Ok(r#""done""#.to_owned())).is_ok());
    world.settle();
    assert!(call_echo(&mut world, OK_PAYLOAD).is_ok());
}

/// REQ: SVC-014
#[test]
fn svc_014_provider_accepts_64_in_flight_calls_and_the_65th_returns_busy() {
    let mut world = service_world("0.1.0", deferred());
    for name in ["c2", "c3"] {
        let manifest = consumer_manifest(&format!("com.example.{name}"), SERVICE, None);
        assert_eq!(diag_code(&world.install(Tier::T3, manifest, Vec::new())), "ok");
    }
    for _ in 0..32 {
        assert!(call_echo(&mut world, OK_PAYLOAD).is_ok());
    }
    for _ in 0..32 {
        assert!(world.call_service("com.example.c2", SERVICE, "echo", OK_PAYLOAD).is_ok());
    }
    let error = err_of(world.call_service("com.example.c3", SERVICE, "echo", OK_PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::Busy);
}

// ---------------------------------------------------------------------------------------------
// SVC-006 versions
// ---------------------------------------------------------------------------------------------

fn consumer_active(provided: &str, required: &str) -> bool {
    let mut world = ExtWorld::new();
    let provider = provider_manifest("com.example.prov", SERVICE, provided);
    assert_eq!(diag_code(&world.install(Tier::T3, provider, vec![(method_key(SERVICE, "echo"), done(1, r#""x""#))])), "ok");
    let consumer = consumer_manifest("com.example.cons", SERVICE, Some(required));
    let result = world.install(Tier::T3, consumer, Vec::new());
    assert_eq!(diag_code(&result), "ok");
    world.settle();
    world.plugin_active("com.example.cons")
}

/// REQ: SVC-006
#[test]
fn svc_006_require_0_1_0_provider_1_0_0_keeps_consumer_inactive() {
    assert!(!consumer_active("1.0.0", "0.1.0"));
}

/// REQ: SVC-006
#[test]
fn svc_006_require_0_1_0_provider_0_1_5_activates() {
    assert!(consumer_active("0.1.5", "0.1.0"));
}

/// REQ: SVC-006
#[test]
fn svc_006_require_0_1_0_provider_0_1_0_activates() {
    assert!(consumer_active("0.1.0", "0.1.0"));
}

/// REQ: SVC-006
#[test]
fn svc_006_require_0_1_0_refuses_0_9_0_and_0_2_x() {
    assert!(!consumer_active("0.9.0", "0.1.0"));
    assert!(!consumer_active("0.2.0", "0.1.0"));
    assert!(!consumer_active("0.2.7", "0.1.0"));
}

/// REQ: SVC-006
#[test]
fn svc_006_require_1_2_0_accepts_1_2_0_and_1_4_0() {
    assert!(consumer_active("1.2.0", "1.2.0"));
    assert!(consumer_active("1.4.0", "1.2.0"));
}

/// REQ: SVC-006
#[test]
fn svc_006_require_1_2_0_refuses_1_1_0_and_2_0_0() {
    assert!(!consumer_active("1.1.0", "1.2.0"));
    assert!(!consumer_active("2.0.0", "1.2.0"));
}

proptest! {
    // REQ: SVC-006, EXT-006
    #[test]
    fn svc_006_version_rule_matches_the_manual_for_all_small_versions(
        provided_major in 0u8..3, provided_minor in 0u8..4, provided_patch in 0u8..3,
        required_major in 0u8..3, required_minor in 0u8..4, required_patch in 0u8..3,
    ) {
        let provided = format!("{provided_major}.{provided_minor}.{provided_patch}");
        let required = format!("{required_major}.{required_minor}.{required_patch}");
        let expected = if provided_major != required_major {
            false
        } else if provided_major == 0 {
            provided_minor == required_minor
        } else {
            provided_minor >= required_minor
        };
        prop_assert_eq!(consumer_active(&provided, &required), expected);
    }
}

// ---------------------------------------------------------------------------------------------
// SVC-007 .. SVC-011
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-007
#[test]
fn svc_007_provider_never_replies_completes_failed_timed_out_at_30_s() {
    let mut world = service_world("0.1.0", never());
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.advance_ms(29_999);
    world.settle();
    assert!(world.outcome("com.example.cons", id).is_none(), "still pending at 29.999 s");
    world.advance_ms(1);
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::TimedOut);
}

/// REQ: SVC-007
#[test]
fn svc_007_reply_just_before_30_s_completes_normally() {
    let mut world = service_world("0.1.0", deferred());
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.advance_ms(29_999);
    let call = world.service_calls_seen("com.example.prov")[0].0;
    assert!(world.service_reply("com.example.prov", call, Ok(r#""late but ok""#.to_owned())).is_ok());
    world.settle();
    assert!(matches!(world.outcome("com.example.cons", id), Some(Outcome::Ok)));
}

/// REQ: SVC-008
#[test]
fn svc_008_provider_restart_mid_call_completes_failed_cancelled() {
    let mut world = service_world("0.1.0", deferred());
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.restart("com.example.prov");
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::Cancelled);
}

/// REQ: SVC-008
#[test]
fn svc_008_provider_disabled_mid_call_completes_failed_cancelled() {
    let mut world = service_world("0.1.0", deferred());
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.set_enabled("com.example.prov", false);
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::Cancelled);
}

/// REQ: SVC-008
#[test]
fn svc_008_provider_uninstalled_mid_call_completes_failed_cancelled() {
    let mut world = service_world("0.1.0", deferred());
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.uninstall("com.example.prov");
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::Cancelled);
}

/// REQ: SVC-008
#[test]
fn svc_008_provider_trap_mid_call_completes_failed_cancelled() {
    let mut world = service_world("0.1.0", script(1, HandlerAction::Trap));
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::Cancelled);
}

/// REQ: SVC-008
#[test]
fn svc_008_every_pending_caller_is_cancelled() {
    let mut world = service_world("0.1.0", deferred());
    let ids: Vec<RequestId> = (0..5).filter_map(|_| call_echo(&mut world, OK_PAYLOAD).ok()).collect();
    world.restart("com.example.prov");
    world.settle();
    for id in ids {
        assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::Cancelled);
    }
}

/// REQ: SVC-009
#[test]
fn svc_009_reply_failing_the_result_schema_completes_failed_invalid_argument_and_provider_logs_error() {
    let mut world = service_world("0.1.0", done(1, "42"));
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.settle();
    assert_eq!(code_of(&failed_error(&world.outcome("com.example.cons", id))), ErrorCode::InvalidArgument);
    let logged = world.all_logs().iter().any(|line| line.plugin == "com.example.prov" && line.level == "error");
    assert!(logged, "the provider logs an error");
}

/// REQ: SVC-009
#[test]
fn svc_009_reply_matching_the_result_schema_completes_ok() {
    let mut world = service_world("0.1.0", done(1, r#""fine""#));
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.settle();
    assert!(matches!(world.outcome("com.example.cons", id), Some(Outcome::Ok)));
}

/// REQ: SVC-010, WIT:ilium:host/services.take-reply
#[test]
fn svc_010_take_reply_returns_the_json_once_and_none_the_second_time() {
    let mut world = service_world("0.1.0", done(1, r#""fine""#));
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    world.settle();
    assert_eq!(world.service_take_reply("com.example.cons", id), Some(r#""fine""#.to_owned()));
    assert_eq!(world.service_take_reply("com.example.cons", id), None);
}

/// REQ: SVC-010
#[test]
fn svc_010_take_reply_before_completion_is_none() {
    let mut world = service_world("0.1.0", deferred());
    let id = call_echo(&mut world, OK_PAYLOAD).unwrap_or(RequestId(0));
    assert_eq!(world.service_take_reply("com.example.cons", id), None);
}

/// REQ: SVC-011, WIT:ilium:host/services.reply
#[test]
fn svc_011_reply_at_31_s_returns_not_found() {
    let mut world = service_world("0.1.0", deferred());
    let _ = call_echo(&mut world, OK_PAYLOAD);
    let call = world.service_calls_seen("com.example.prov")[0].0;
    world.advance_ms(31_000);
    world.settle();
    let error = err_of(world.service_reply("com.example.prov", call, Ok(r#""late""#.to_owned())));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: SVC-011
#[test]
fn svc_011_reply_to_an_unknown_call_id_returns_not_found() {
    let mut world = service_world("0.1.0", deferred());
    let error = err_of(world.service_reply("com.example.prov", 9_999, Ok(r#""x""#.to_owned())));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: SVC-011
#[test]
fn svc_011_second_reply_to_the_same_call_returns_not_found() {
    let mut world = service_world("0.1.0", deferred());
    let _ = call_echo(&mut world, OK_PAYLOAD);
    let call = world.service_calls_seen("com.example.prov")[0].0;
    assert!(world.service_reply("com.example.prov", call, Ok(r#""a""#.to_owned())).is_ok());
    let error = err_of(world.service_reply("com.example.prov", call, Ok(r#""b""#.to_owned())));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

// ---------------------------------------------------------------------------------------------
// SVC-012, SVC-013
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-012
#[test]
fn svc_012_consumer_installed_first_activates_after_the_provider() {
    let mut world = ExtWorld::new();
    let consumer = consumer_manifest("com.example.cons", SERVICE, Some("0.1.0"));
    assert_eq!(diag_code(&world.install(Tier::T3, consumer, Vec::new())), "ok");
    world.settle();
    assert!(!world.plugin_active("com.example.cons"));
    let provider = provider_manifest("com.example.prov", SERVICE, "0.1.0");
    assert_eq!(diag_code(&world.install(Tier::T3, provider, vec![(method_key(SERVICE, "echo"), done(1, r#""x""#))])), "ok");
    world.settle();
    assert!(world.plugin_active("com.example.cons"));
}

/// REQ: SVC-012
#[test]
fn svc_012_consumer_without_requires_activates_without_a_provider() {
    let mut world = ExtWorld::new();
    let consumer = consumer_manifest("com.example.cons", SERVICE, None);
    assert_eq!(diag_code(&world.install(Tier::T3, consumer, Vec::new())), "ok");
    world.settle();
    assert!(world.plugin_active("com.example.cons"));
}

/// REQ: SVC-013
#[test]
fn svc_013_provider_bumped_to_a_new_major_makes_the_old_consumer_inactive_until_updated() {
    let mut world = ExtWorld::new();
    let v1 = provider_manifest("com.example.prov", SERVICE, "1.0.0");
    assert_eq!(diag_code(&world.install(Tier::T3, v1, vec![(method_key(SERVICE, "echo"), done(1, r#""x""#))])), "ok");
    let consumer = consumer_manifest("com.example.cons", SERVICE, Some("1.0.0"));
    assert_eq!(diag_code(&world.install(Tier::T3, consumer, Vec::new())), "ok");
    world.settle();
    assert!(world.plugin_active("com.example.cons"));
    let v2 = provider_manifest("com.example.prov", SERVICE, "2.0.0");
    assert_eq!(diag_code(&world.update(Tier::T3, v2, vec![(method_key(SERVICE, "echo"), done(1, r#""x""#))])), "ok");
    world.settle();
    assert!(!world.plugin_active("com.example.cons"));
    let consumer2 = consumer_manifest("com.example.cons", SERVICE, Some("2.0.0"));
    assert_eq!(diag_code(&world.update(Tier::T3, consumer2, Vec::new())), "ok");
    world.settle();
    assert!(world.plugin_active("com.example.cons"));
}

// ---------------------------------------------------------------------------------------------
// Doc tests
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-003, SVC-005, SVC-007, SVC-014
#[test]
fn doc_services_page_states_the_limits_the_tests_use() {
    let text = page("api-services.md");
    for needle in ["1 MiB", "30 seconds", "64 pending", "64 calls in flight"] {
        let present = text.contains(needle) || text.replace("30 s", "30 seconds").contains(needle);
        assert!(present, "api-services.md no longer mentions `{needle}`");
    }
}
