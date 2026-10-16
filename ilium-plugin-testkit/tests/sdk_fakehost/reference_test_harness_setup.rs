//! reference-test-harness.md "Setup": DEV-410 to DEV-417.

use ilium_plugin_sdk_core::types::{DeactivationReason, ErrorCode, Outcome};
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::{FakeHost, Mode};

use crate::common::{
    DEFAULT_CLOCK_MS, ERROR_CODE_NAMES, Spy, assert_fails_with, code_name, manifest_text,
    outcome_code, sound, sound_keyed, spy, spy_reset, spy_script, strings, temp_dir, with_clean_env,
    with_env, write_file,
};

fn active_host() -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new();
    host.activate::<Spy>();
    host
}

fn manifest_host(server: &[&str], client: &[&str], label: &str) -> std::path::PathBuf {
    let dir = temp_dir(label);
    write_file(&dir, "ilium-plugin.json", &manifest_text("example.demo", server, client))
}

// ---- DEV-410 ---------------------------------------------------------------------------------

/// REQ: DEV-410
#[test]
fn dev_410_builder_calls_chain_and_the_result_activates() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new()
            .grant("tree.read")
            .grants(&["agent.read", "ui.panels"])
            .tier("T2")
            .side("server")
            .platform("linux")
            .seed(5)
            .mode(Mode::Native)
            .build();
        host.activate::<Spy>();
        let log = spy();
        assert_eq!(log.activations[0].granted, strings(&["agent.read", "tree.read", "ui.panels"]));
    });
}

/// REQ: DEV-410
#[test]
fn dev_410_each_builder_method_after_activate_fails_with_setup_closed() {
    with_clean_env(|| {
        type Call = Box<dyn FnOnce(FakeHost) -> FakeHost>;
        let calls: Vec<(&str, Call)> = vec![
            ("grant", Box::new(|host| host.grant("tree.read"))),
            ("grants", Box::new(|host| host.grants(&["tree.read"]))),
            ("tier", Box::new(|host| host.tier("T1"))),
            ("side", Box::new(|host| host.side("server"))),
            ("platform", Box::new(|host| host.platform("linux"))),
            ("seed", Box::new(|host| host.seed(2))),
            ("build", Box::new(|host| host.build())),
            ("mode", Box::new(|host| host.mode(Mode::Native))),
        ];
        for (name, call) in calls {
            let host = active_host();
            let result = crate::common::panic_text(move || call(host));
            let text = result.unwrap_or_else(|| panic!("builder method {name} succeeded after activate"));
            assert!(text.contains(codes::SETUP_CLOSED), "{name}: {text}");
        }
    });
}

/// REQ: DEV-410
#[test]
fn dev_410_builder_methods_before_activate_do_not_fail() {
    with_clean_env(|| {
        let host = FakeHost::new().grant("tree.read").seed(3);
        // Chaining before activation is open; the activation proves the host works afterwards.
        spy_reset();
        let mut host = host;
        host.activate::<Spy>();
        assert_eq!(spy().activations.len(), 1);
    });
}

// ---- DEV-411 ---------------------------------------------------------------------------------

/// REQ: DEV-411
#[test]
fn dev_411_for_manifest_takes_the_grants_of_the_server_side_sorted() {
    with_clean_env(|| {
        let path = manifest_host(&["tree.read", "agent.read"], &["notify.desktop"], "m411a");
        let mut host = FakeHost::for_manifest(&path).expect("manifest loads");
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(spy().activations[0].granted, strings(&["agent.read", "tree.read"]));
        assert_eq!(spy().activations[0].plugin.0, "example.demo");
    });
}

/// REQ: DEV-411
#[test]
fn dev_411_for_manifest_with_the_client_side_takes_the_client_grants() {
    with_clean_env(|| {
        let path = manifest_host(&["tree.read"], &["notify.desktop", "ui.panels"], "m411b");
        let mut host = FakeHost::for_manifest(&path).expect("manifest loads").side("client");
        spy_reset();
        host.activate::<Spy>();
        let log = spy();
        let activation = &log.activations[0];
        assert_eq!(activation.side, "client");
        assert_eq!(activation.granted, strings(&["notify.desktop", "ui.panels"]));
    });
}

/// REQ: DEV-411
#[test]
fn dev_411_a_grant_not_in_the_manifest_is_refused_by_grant_with_grant_not_declared() {
    with_clean_env(|| {
        let path = manifest_host(&["tree.read"], &[], "m411c");
        let host = FakeHost::for_manifest(&path).expect("manifest loads");
        assert_fails_with(codes::GRANT_NOT_DECLARED, move || host.grant("tree.write"));
    });
}

/// REQ: DEV-411
#[test]
fn dev_411_a_grant_not_in_the_manifest_is_refused_by_grant_now_with_grant_not_declared() {
    with_clean_env(|| {
        let path = manifest_host(&["tree.read"], &[], "m411d");
        let mut host = FakeHost::for_manifest(&path).expect("manifest loads");
        spy_reset();
        host.activate::<Spy>();
        let error = host.grant_now("tree.write").expect_err("tree.write is not declared");
        assert_eq!(error.code, codes::GRANT_NOT_DECLARED);
    });
}

/// REQ: DEV-411
#[test]
fn dev_411_new_accepts_any_grant_name_of_the_catalogue() {
    with_clean_env(|| {
        let mut host = FakeHost::new().grant("tree.write");
        spy_reset();
        host.activate::<Spy>();
        host.grant_now("tree.decorate").expect("new() has no manifest to refuse it");
        assert_eq!(spy().activations[1].granted, strings(&["tree.decorate", "tree.write"]));
    });
}

// ---- DEV-412 ---------------------------------------------------------------------------------

/// REQ: DEV-412
#[test]
fn dev_412_an_unknown_grant_name_fails_with_grant_unknown() {
    with_clean_env(|| {
        assert_fails_with(codes::GRANT_UNKNOWN, || FakeHost::new().grant("tree.raed"));
    });
}

/// REQ: DEV-412
#[test]
fn dev_412_one_unknown_name_in_a_list_fails_with_grant_unknown() {
    with_clean_env(|| {
        assert_fails_with(codes::GRANT_UNKNOWN, || {
            FakeHost::new().grants(&["tree.read", "tree.raed"])
        });
    });
}

/// REQ: DEV-412
#[test]
fn dev_412_grant_now_with_an_unknown_name_returns_grant_unknown() {
    with_clean_env(|| {
        let mut host = active_host();
        let error = host.grant_now("not.a.grant").expect_err("unknown grant");
        assert_eq!(error.code, codes::GRANT_UNKNOWN);
    });
}

/// REQ: DEV-412
#[test]
fn dev_412_catalogue_names_and_argument_forms_are_accepted() {
    with_clean_env(|| {
        let names = [
            "tree.read",
            "tree.decorate",
            "tree.write",
            "ui.panels",
            "ui.chrome",
            "agent.read",
            "notify.desktop",
            "sound.play",
            "http:api.example.com",
            "process.run:git",
        ];
        let mut host = FakeHost::new().grants(&names);
        spy_reset();
        host.activate::<Spy>();
        let mut expected = strings(&names);
        expected.sort();
        assert_eq!(spy().activations[0].granted, expected);
    });
}

// ---- DEV-413 ---------------------------------------------------------------------------------

/// REQ: DEV-413
#[test]
fn dev_413_the_default_seed_is_one() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        assert_eq!(host.current_seed(), 1);
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(host.snapshot().instance, 1);
    });
}

/// REQ: DEV-413
#[test]
fn dev_413_the_environment_variable_overrides_the_default_seed() {
    with_env(&[("ILIUM_PLUGIN_TEST_SEED", "42")], || {
        let mut host = FakeHost::new();
        assert_eq!(host.current_seed(), 42);
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(host.snapshot().instance, 1);
    });
}

/// REQ: DEV-413
#[test]
fn dev_413_an_explicit_seed_call_wins_over_the_environment_variable() {
    with_env(&[("ILIUM_PLUGIN_TEST_SEED", "42")], || {
        let mut host = FakeHost::new().seed(7);
        assert_eq!(host.current_seed(), 7);
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(host.snapshot().instance, 1);
    });
}

// ---- DEV-414 ---------------------------------------------------------------------------------

/// REQ: DEV-414
#[test]
fn dev_414_the_default_clock_is_2030_01_01_utc() {
    with_clean_env(|| {
        let host = FakeHost::new();
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS);
    });
}

/// REQ: DEV-414
#[test]
fn dev_414_wall_time_never_leaks_into_the_clock() {
    with_clean_env(|| {
        let first = FakeHost::new().clock().now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = FakeHost::new().clock().now();
        assert_eq!(first, second);
        assert_eq!(first, DEFAULT_CLOCK_MS);
    });
}

/// REQ: DEV-414
#[test]
fn dev_414_the_clock_moves_only_by_advance_clock() {
    with_clean_env(|| {
        let mut host = active_host();
        host.advance_clock(1_500);
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 1_500);
    });
}

// ---- DEV-415 ---------------------------------------------------------------------------------

/// REQ: DEV-415
#[test]
fn dev_415_grant_now_deactivates_with_granted_then_activates_with_instance_plus_one() {
    with_clean_env(|| {
        let mut host = active_host();
        host.grant_now("tree.read").expect("grant");
        let log = spy();
        assert_eq!(log.deactivations, vec![DeactivationReason::Granted]);
        assert_eq!(log.activations.len(), 2);
        assert_eq!(log.activations[0].instance, 1);
        assert_eq!(log.activations[1].instance, 2);
        assert_eq!(log.activations[1].granted, strings(&["tree.read"]));
    });
}

/// REQ: DEV-415
#[test]
fn dev_415_revoke_deactivates_with_revoked_and_drops_the_grant() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grants(&["tree.read", "agent.read"]);
        host.activate::<Spy>();
        host.revoke("tree.read");
        let log = spy();
        assert_eq!(log.deactivations, vec![DeactivationReason::Revoked]);
        assert_eq!(log.activations[1].instance, 2);
        assert_eq!(log.activations[1].granted, strings(&["agent.read"]));
    });
}

/// REQ: DEV-415
#[test]
fn dev_415_no_event_is_delivered_by_a_grant_change() {
    with_clean_env(|| {
        let mut host = active_host();
        host.grant_now("tree.read").expect("grant");
        host.revoke("tree.read");
        assert!(spy().batches.is_empty());
    });
}

// ---- DEV-416 ---------------------------------------------------------------------------------

/// REQ: DEV-416
#[test]
fn dev_416_after_revoke_an_intent_that_needs_the_grant_is_refused_not_permitted() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grant("sound.play");
        host.activate::<Spy>();
        host.revoke("sound.play");
        spy_script(vec![vec![sound(1)]]);
        host.deliver(crate::common::client_batch(1));
        host.settle();
        let outcomes = host.outcomes();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcome_code(&outcomes[0]), Some(ErrorCode::NotPermitted));
    });
}

/// REQ: DEV-416
#[test]
fn dev_416_an_intent_for_a_declined_capability_is_refused_not_permitted() {
    with_clean_env(|| {
        let mut host = active_host();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(crate::common::client_batch(1));
        host.settle();
        assert_eq!(outcome_code(&host.outcomes()[0]), Some(ErrorCode::NotPermitted));
    });
}

/// REQ: DEV-416
#[test]
fn dev_416_with_the_grant_the_same_intent_completes_ok() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grant("sound.play");
        host.activate::<Spy>();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(crate::common::client_batch(1));
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok]);
    });
}

// ---- DEV-417 ---------------------------------------------------------------------------------

/// REQ: DEV-417
#[test]
fn dev_417_a_refusal_carries_one_of_the_15_enum_codes() {
    with_clean_env(|| {
        let mut host = active_host();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(crate::common::client_batch(1));
        host.settle();
        let code = outcome_code(&host.outcomes()[0]).expect("refused");
        let name = code_name(code);
        assert!(ERROR_CODE_NAMES.contains(&name.as_str()), "{name}");
        assert!(!name.contains('.'), "dotted code in plugin-error.code: {name}");
    });
}

/// REQ: DEV-417
#[test]
fn dev_417_a_bad_dedup_key_uses_invalid_argument_and_not_a_dotted_code() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grant("sound.play");
        host.activate::<Spy>();
        spy_script(vec![vec![sound_keyed(1, "bad key!")]]);
        host.deliver(crate::common::client_batch(1));
        host.settle();
        match &host.outcomes()[0] {
            Outcome::Refused(error) => {
                assert_eq!(error.code, ErrorCode::InvalidArgument);
                assert!(error.message.starts_with("dedup-key-invalid:"), "{}", error.message);
                assert!(!code_name(error.code).contains('.'));
            }
            other => panic!("expected refused, got {other:?}"),
        }
    });
}

/// REQ: DEV-417
#[test]
fn dev_417_every_error_code_variant_serializes_to_one_of_the_15_names() {
    // The generated enum is the single source of the runtime codes; the harness may not add values.
    let all = [
        ErrorCode::NotPermitted,
        ErrorCode::NotFound,
        ErrorCode::InvalidArgument,
        ErrorCode::Conflict,
        ErrorCode::QuotaExceeded,
        ErrorCode::RateLimited,
        ErrorCode::Busy,
        ErrorCode::TimedOut,
        ErrorCode::Cancelled,
        ErrorCode::Unsupported,
        ErrorCode::PaneNotReady,
        ErrorCode::Revoked,
        ErrorCode::Io,
        ErrorCode::Network,
        ErrorCode::Internal,
    ];
    let names: Vec<String> = all.iter().map(|code| code_name(*code)).collect();
    assert_eq!(names, ERROR_CODE_NAMES.iter().map(|name| name.to_string()).collect::<Vec<_>>());
    // Pair with the host so the check exercises the harness and not only the generated types.
    let host = active_host();
    assert_eq!(host.snapshot().instance, 1);
}
