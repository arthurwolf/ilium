//! api-extension-points.md: size caps, full points, priorities, rate limits, entry identity and
//! entry commands (EXT-023, 030, 047..050, SEC-290).

use crate::support::*;

const PAYLOAD: &str = r#"{"text":"a","n":1}"#;
const COLLECT: &str = "com.example.owner/c";

fn collect_world(max_entries: u32) -> ExtWorld {
    let mut world = ExtWorld::new();
    let point = collect_point("c", max_entries, true);
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    let manifest = json!({
        "id": "com.example.reg",
        "capabilities": {"server": ["extensions.contribute:com.example.owner/c"]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "ok"
    );
    world
}

fn entry_of_size(total: usize) -> String {
    let overhead = r#"{"id":"e","regex":""}"#.len();
    format!(
        r#"{{"id":"e","regex":"{}"}}"#,
        "a".repeat(total.saturating_sub(overhead))
    )
}

// ---------------------------------------------------------------------------------------------
// EXT-023
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-023
#[test]
fn ext_023_register_entry_number_max_entries_plus_one_fails_quota_exceeded() {
    let mut world = collect_world(3);
    for index in 0..3 {
        assert!(world
            .register(
                "com.example.reg",
                COLLECT,
                &format!("e{index}"),
                50,
                r#"{"id":"x"}"#
            )
            .is_ok());
    }
    let error = err_of(world.register("com.example.reg", COLLECT, "e3", 50, r#"{"id":"x"}"#));
    assert_eq!(code_of(&error), ErrorCode::QuotaExceeded);
    assert!(message_has_prefix(&error, "extension.cap-exceeded"));
}

/// REQ: EXT-023
#[test]
fn ext_023_register_entry_number_max_entries_succeeds() {
    let mut world = collect_world(3);
    for index in 0..3 {
        assert!(world
            .register(
                "com.example.reg",
                COLLECT,
                &format!("e{index}"),
                50,
                r#"{"id":"x"}"#
            )
            .is_ok());
    }
    assert_eq!(
        world
            .list(OWNER, COLLECT)
            .map(|list| list.len())
            .unwrap_or(0),
        3
    );
}

/// REQ: EXT-023
#[test]
fn ext_023_replacing_an_existing_id_at_the_limit_does_not_need_a_new_slot() {
    let mut world = collect_world(2);
    assert!(world
        .register("com.example.reg", COLLECT, "e0", 50, r#"{"id":"x"}"#)
        .is_ok());
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"x"}"#)
        .is_ok());
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 60, r#"{"id":"y"}"#)
        .is_ok());
}

/// REQ: EXT-023
#[test]
fn ext_023_entry_of_exactly_16_kib_is_accepted() {
    let mut world = collect_world(8);
    assert!(world
        .register("com.example.reg", COLLECT, "e", 50, &entry_of_size(KIB16))
        .is_ok());
}

/// REQ: EXT-023
#[test]
fn ext_023_entry_of_16_kib_plus_one_byte_fails_cap_exceeded_invalid_argument() {
    let mut world = collect_world(8);
    let error = err_of(world.register(
        "com.example.reg",
        COLLECT,
        "e",
        50,
        &entry_of_size(KIB16 + 1),
    ));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(message_has_prefix(&error, "extension.cap-exceeded"));
}

/// REQ: EXT-023
#[test]
fn ext_023_entry_of_16_kib_minus_one_byte_is_accepted() {
    let mut world = collect_world(8);
    assert!(world
        .register(
            "com.example.reg",
            COLLECT,
            "e",
            50,
            &entry_of_size(KIB16 - 1)
        )
        .is_ok());
}

/// REQ: EXT-023
#[test]
fn ext_023_reply_of_64_kib_plus_one_byte_is_not_accepted() {
    let big = format!(r#"{{"text":"{}","n":1}}"#, "a".repeat(KIB64));
    let mut world = hook_world("transform", vec![contrib("a", 1, replace(1, &big))]);
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert_eq!(reply, None);
}

/// REQ: EXT-023
#[test]
fn ext_023_point_with_max_entries_257_fails_the_manifest() {
    let mut world = ExtWorld::new();
    let result = world.install(
        Tier::T3,
        owner_manifest(OWNER, vec![collect_point("c", 257, false)]),
        Vec::new(),
    );
    assert!(result.is_err());
}

/// REQ: EXT-023
#[test]
fn ext_023_point_with_max_entries_256_installs() {
    let mut world = ExtWorld::new();
    let result = world.install(
        Tier::T3,
        owner_manifest(OWNER, vec![collect_point("c", 256, false)]),
        Vec::new(),
    );
    assert_eq!(diag_code(&result), "ok");
}

/// REQ: EXT-023
#[test]
fn ext_023_point_with_max_handlers_9_fails_the_manifest() {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "observe"), "max_handlers", json!(9));
    assert!(world
        .install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())
        .is_err());
}

/// REQ: EXT-023
#[test]
fn ext_023_point_with_max_handlers_8_installs() {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "observe"), "max_handlers", json!(8));
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
}

/// REQ: EXT-023
#[test]
fn ext_023_chain_result_over_64_kib_is_not_returned() {
    let almost = format!(r#"{{"text":"{}","n":1}}"#, "a".repeat(KIB64 - 64));
    let mut world = hook_world(
        "transform",
        vec![
            contrib("a", 1, replace(1, &almost)),
            contrib(
                "b",
                2,
                replace(
                    1,
                    &format!(r#"{{"text":"{}","n":1}}"#, "b".repeat(KIB64 - 64)),
                ),
            ),
        ],
    );
    let (_, reply) = run_chain(&mut world, PAYLOAD);
    assert!(reply.map(|text| text.len() <= KIB64).unwrap_or(true));
}

// ---------------------------------------------------------------------------------------------
// EXT-030 point full
// ---------------------------------------------------------------------------------------------

fn full_world(contributors: usize, max_handlers: u32) -> ExtWorld {
    let point = with(
        hook_point("p", "observe"),
        "max_handlers",
        json!(max_handlers),
    );
    let contribs = (0..contributors)
        .map(|index| contrib(&format!("c{index}"), index as u8, pass(1)))
        .collect();
    hook_world_with(point, contribs)
}

/// REQ: EXT-030
#[test]
fn ext_030_ninth_contributor_installs_and_is_dormant_with_point_full_reason() {
    let world = full_world(9, 8);
    assert_eq!(
        world.dormant_reason("com.example.c8", POINT, "e-c8"),
        Some("point full (8)".to_owned())
    );
}

/// REQ: EXT-030
#[test]
fn ext_030_eight_contributors_are_all_active() {
    let world = full_world(8, 8);
    for index in 0..8 {
        assert!(world.is_active(
            &format!("com.example.c{index}"),
            POINT,
            &format!("e-c{index}")
        ));
    }
}

/// REQ: EXT-030
#[test]
fn ext_030_the_first_eight_in_run_order_stay_active() {
    let world = full_world(9, 8);
    for index in 0..8 {
        assert!(world.is_active(
            &format!("com.example.c{index}"),
            POINT,
            &format!("e-c{index}")
        ));
    }
    assert!(!world.is_active("com.example.c8", POINT, "e-c8"));
}

/// REQ: EXT-030
#[test]
fn ext_030_dormant_handler_is_not_called() {
    let mut world = full_world(9, 8);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(run_order(&world).len(), 8);
}

/// REQ: EXT-030
#[test]
fn ext_030_reorder_recomputes_the_active_set_and_sends_changed() {
    let mut world = full_world(9, 8);
    world.advance_ms(100);
    world.settle();
    let before = world.topics_received(OWNER).len();
    let mut order: Vec<(String, String)> = vec![("com.example.c8".to_owned(), "e-c8".to_owned())];
    order.extend((0..8).map(|index| (format!("com.example.c{index}"), format!("e-c{index}"))));
    world.set_user_order(POINT, order);
    world.advance_ms(100);
    world.settle();
    assert!(world.is_active("com.example.c8", POINT, "e-c8"));
    assert!(!world.is_active("com.example.c7", POINT, "e-c7"));
    assert!(world.topics_received(OWNER).len() > before);
}

/// REQ: EXT-030
#[test]
fn ext_030_removal_of_an_active_contributor_promotes_the_dormant_one() {
    let mut world = full_world(9, 8);
    world.uninstall("com.example.c0");
    world.advance_ms(100);
    world.settle();
    assert!(world.is_active("com.example.c8", POINT, "e-c8"));
}

/// REQ: EXT-030
#[test]
fn ext_030_collect_point_keeps_the_first_max_entries_in_run_order() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![collect_point("c", 2, false)]),
            Vec::new()
        )),
        "ok"
    );
    for index in 0..3u8 {
        let manifest = json!({
            "id": format!("com.example.n{index}"),
            "capabilities": {"server": [format!("extensions.contribute:{COLLECT}")]},
            "contributes": {"extensions": [ext_entry(COLLECT, "e", index, json!({"id": "x"}))]}
        });
        assert_eq!(
            diag_code(&world.install(Tier::T3, manifest, Vec::new())),
            "ok"
        );
    }
    let listed = world
        .list(OWNER, COLLECT)
        .map(|list| list.len())
        .unwrap_or(0);
    assert_eq!(listed, 2);
    assert_eq!(
        world.dormant_reason("com.example.n2", COLLECT, "e"),
        Some("point full (2)".to_owned())
    );
}

// ---------------------------------------------------------------------------------------------
// EXT-047
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-047
#[test]
fn ext_047_register_priority_101_fails_invalid_argument_schema_mismatch() {
    let mut world = collect_world(8);
    let error = err_of(world.register("com.example.reg", COLLECT, "e", 101, r#"{"id":"x"}"#));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
    assert!(message_has_prefix(&error, "extension.schema-mismatch"));
}

/// REQ: EXT-047
#[test]
fn ext_047_register_priority_100_succeeds() {
    let mut world = collect_world(8);
    assert!(world
        .register("com.example.reg", COLLECT, "e", 100, r#"{"id":"x"}"#)
        .is_ok());
}

/// REQ: EXT-047
#[test]
fn ext_047_register_priority_0_succeeds() {
    let mut world = collect_world(8);
    assert!(world
        .register("com.example.reg", COLLECT, "e", 0, r#"{"id":"x"}"#)
        .is_ok());
}

/// REQ: EXT-047
#[test]
fn ext_047_manifest_priority_101_fails_the_install() {
    let mut world = hook_world("observe", Vec::new());
    let manifest = contributor_manifest(
        "com.example.hi",
        vec![json!({"point": POINT, "id": "x", "priority": 101, "handler": "h"})],
    );
    assert!(world
        .install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))])
        .is_err());
}

// ---------------------------------------------------------------------------------------------
// EXT-048 rate limit
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-048, SEC-290
#[test]
fn ext_048_two_hundred_run_hook_calls_in_one_second_all_succeed() {
    let mut world = hook_world("observe", Vec::new());
    for _ in 0..200 {
        assert!(world.run_hook(OWNER, POINT, PAYLOAD).is_ok());
    }
}

/// REQ: EXT-048, SEC-290
#[test]
fn ext_048_call_201_in_one_second_returns_rate_limited_with_retry_after() {
    let mut world = hook_world("observe", Vec::new());
    for _ in 0..200 {
        let _ = world.run_hook(OWNER, POINT, PAYLOAD);
    }
    let error = err_of(world.run_hook(OWNER, POINT, PAYLOAD));
    assert_eq!(code_of(&error), ErrorCode::RateLimited);
    assert!(error.retry_after_ms.is_some());
}

/// REQ: EXT-048
#[test]
fn ext_048_calls_succeed_again_after_the_second_passes() {
    let mut world = hook_world("observe", Vec::new());
    for _ in 0..201 {
        let _ = world.run_hook(OWNER, POINT, PAYLOAD);
    }
    world.advance_ms(1001);
    assert!(world.run_hook(OWNER, POINT, PAYLOAD).is_ok());
}

/// REQ: EXT-048
#[test]
fn ext_048_limit_is_per_point() {
    let mut world = ExtWorld::new();
    let owner = owner_manifest(
        OWNER,
        vec![hook_point("p", "observe"), hook_point("q", "observe")],
    );
    assert_eq!(diag_code(&world.install(Tier::T3, owner, Vec::new())), "ok");
    for _ in 0..201 {
        let _ = world.run_hook(OWNER, POINT, PAYLOAD);
    }
    assert!(world
        .run_hook(OWNER, "com.example.owner/q", PAYLOAD)
        .is_ok());
}

/// REQ: EXT-048
#[test]
fn ext_048_rate_limit_is_not_logged_as_a_breach_of_a_contributor() {
    let mut world = hook_world("observe", vec![contrib("a", 1, pass(1))]);
    for _ in 0..205 {
        let _ = world.run_hook(OWNER, POINT, PAYLOAD);
    }
    world.settle();
    assert_eq!(world.breach_count("com.example.a", POINT, "e-a"), 0);
}

/// REQ: EXT-048
#[test]
fn ext_048_depth_exceeded_entry_rejected_and_veto_overridden_are_log_only_codes() {
    let text = page("api-extension-points.md");
    for code in [
        "extension.depth-exceeded",
        "extension.entry-rejected",
        "extension.veto-overridden",
    ] {
        assert!(text.contains(code), "{code} is listed in the code table");
    }
}

/// REQ: SEC-290
#[test]
fn sec_290_cpu_overrun_skips_the_handler_for_the_rest_of_the_second_and_counts_a_breach() {
    let mut world = hook_world("observe", vec![contrib("hog", 1, pass(20))]);
    world.set_cpu_ms_per_call("com.example.hog", 30);
    let _ = run_chain(&mut world, PAYLOAD);
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(
        world.handler_runs().len(),
        2,
        "50 ms spent in the first two calls, the third is skipped"
    );
    let _ = run_chain(&mut world, PAYLOAD);
    assert_eq!(world.handler_runs().len(), 2);
    assert!(world.breach_count("com.example.hog", POINT, "e-hog") >= 1);
}

/// REQ: SEC-290
#[test]
fn sec_290_cpu_budget_resets_after_one_second() {
    let mut world = hook_world("observe", vec![contrib("hog", 1, pass(20))]);
    world.set_cpu_ms_per_call("com.example.hog", 50);
    let _ = run_chain(&mut world, PAYLOAD);
    let _ = run_chain(&mut world, PAYLOAD);
    world.advance_ms(1001);
    let before = world.handler_runs().len();
    let _ = run_chain(&mut world, PAYLOAD);
    assert!(world.handler_runs().len() > before);
}

/// REQ: SEC-290, EXT-021
#[test]
fn sec_290_breach_stop_draws_a_kernel_notice_and_writes_the_audit_record() {
    let mut world = hook_world("veto", vec![contrib("slow", 1, pass(80))]);
    for _ in 0..3 {
        let _ = run_chain(&mut world, PAYLOAD);
    }
    let notice = world
        .notices()
        .into_iter()
        .find(|text| text.contains("com.example.slow"));
    let notice = notice.unwrap_or_default();
    assert!(notice.contains(POINT), "{notice}");
    assert!(
        notice.to_lowercase().contains("timeout") || notice.to_lowercase().contains("deadline"),
        "{notice}"
    );
    assert!(world
        .audit()
        .iter()
        .any(|record| record.action == "extension-disabled-after-breaches"));
}

// ---------------------------------------------------------------------------------------------
// EXT-049 identity
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-049, WIT:ilium:host/extensions.list
#[test]
fn ext_049_equal_ids_from_two_plugins_both_stay_with_different_contributors() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![collect_point("c", 8, false)]),
            Vec::new()
        )),
        "ok"
    );
    for name in ["one", "two"] {
        let manifest = json!({
            "id": format!("com.example.{name}"),
            "capabilities": {"server": [format!("extensions.contribute:{COLLECT}")]},
            "contributes": {"extensions": [ext_entry(COLLECT, "ocean", 50, json!({"id": "x"}))]}
        });
        assert_eq!(
            diag_code(&world.install(Tier::T3, manifest, Vec::new())),
            "ok"
        );
    }
    let listed = world.list(OWNER, COLLECT).unwrap_or_default();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|entry| entry.extension == "ocean"));
    assert_ne!(listed[0].contributor, listed[1].contributor);
}

/// REQ: EXT-049
#[test]
fn ext_049_extension_field_is_the_contribution_id_of_the_manifest() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![collect_point("c", 8, false)]),
            Vec::new()
        )),
        "ok"
    );
    let manifest = json!({
        "id": "com.example.one",
        "capabilities": {"server": [format!("extensions.contribute:{COLLECT}")]},
        "contributes": {"extensions": [ext_entry(COLLECT, "my-entry-id", 50, json!({"id": "other"}))]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "ok"
    );
    let listed = world.list(OWNER, COLLECT).unwrap_or_default();
    assert_eq!(listed[0].extension, "my-entry-id");
    assert_eq!(listed[0].contributor.0, "com.example.one");
}

/// REQ: EXT-049
#[test]
fn ext_049_same_plugin_repeating_one_extension_id_on_a_point_fails_the_install() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![collect_point("c", 8, false)]),
            Vec::new()
        )),
        "ok"
    );
    let manifest = json!({
        "id": "com.example.one",
        "capabilities": {"server": [format!("extensions.contribute:{COLLECT}")]},
        "contributes": {"extensions": [
            ext_entry(COLLECT, "dup", 50, json!({"id": "a"})),
            ext_entry(COLLECT, "dup", 60, json!({"id": "b"}))
        ]}
    });
    assert!(world.install(Tier::T3, manifest, Vec::new()).is_err());
}

// ---------------------------------------------------------------------------------------------
// EXT-050 entry commands
// ---------------------------------------------------------------------------------------------

fn terminal_link_manifest(
    id: &str,
    command: &str,
    extra_grants: Vec<&str>,
    commands: Vec<&str>,
) -> Value {
    let mut grants = vec!["extensions.contribute:ilium.core.terminal/link-actions".to_owned()];
    grants.extend(extra_grants.into_iter().map(str::to_owned));
    let declared: Vec<Value> = commands
        .into_iter()
        .map(|name| json!({"id": name, "title": name}))
        .collect();
    json!({
        "id": id,
        "capabilities": {"server": grants},
        "contributes": {
            "commands": declared,
            "extensions": [ext_entry(
                "ilium.core.terminal/link-actions", "open-ticket", 50,
                json!({"id": "open-ticket", "link_kind": "ticket", "title": "Open ticket", "command": command})
            )]
        }
    })
}

/// REQ: EXT-050
#[test]
fn ext_050_entry_command_of_the_contributor_installs() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = terminal_link_manifest(
        "com.example.tk",
        "open-ticket",
        Vec::new(),
        vec!["open-ticket"],
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "ok"
    );
}

/// REQ: EXT-050
#[test]
fn ext_050_entry_command_of_another_plugin_without_invoke_grant_fails_entry_invalid() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = terminal_link_manifest(
        "com.example.tk",
        "ilium.core.fleet/broadcast",
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "manifest.extension-entry-invalid"
    );
}

/// REQ: EXT-050
#[test]
fn ext_050_entry_command_of_another_plugin_with_invoke_grant_installs() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = terminal_link_manifest(
        "com.example.tk",
        "ilium.core.fleet/broadcast",
        vec!["commands.invoke:ilium.core.fleet/broadcast"],
        Vec::new(),
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "ok"
    );
}

/// REQ: EXT-050
#[test]
fn ext_050_program_running_entry_without_process_run_fails_grant_dependency() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = json!({
        "id": "com.example.steps",
        "capabilities": {"server": ["extensions.contribute:ilium.core.worktrees/post-create-steps"]},
        "contributes": {"extensions": [ext_entry(
            "ilium.core.worktrees/post-create-steps", "step", 50,
            json!({"id": "step", "title": "Install", "command": "npm", "args": ["install"]})
        )]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "manifest.grant-dependency"
    );
}

/// REQ: EXT-050
#[test]
fn ext_050_program_running_entry_with_matching_process_run_installs() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = json!({
        "id": "com.example.steps",
        "capabilities": {"server": ["extensions.contribute:ilium.core.worktrees/post-create-steps", "process.run:npm"]},
        "contributes": {"extensions": [ext_entry(
            "ilium.core.worktrees/post-create-steps", "step", 50,
            json!({"id": "step", "title": "Install", "command": "npm", "args": ["install"]})
        )]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "ok"
    );
}
