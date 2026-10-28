//! api-extension-points.md: manifest-stage refusals, grants, error codes, versions, ordering,
//! the changed topic, dormant contributions and entry-bound services.

use crate::support::*;

fn points_16(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| hook_point(&format!("p{index}"), "observe"))
        .collect()
}

fn extensions_64(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| ext_handler(POINT, &format!("x{index}"), 50, "h"))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// EXT-001
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-001
#[test]
fn ext_001_sixteen_points_install_ok() {
    let mut world = ExtWorld::new();
    let result = world.install(Tier::T3, owner_manifest(OWNER, points_16(16)), Vec::new());
    assert_eq!(diag_code(&result), "ok");
}

/// REQ: EXT-001
#[test]
fn ext_001_seventeen_points_fail_manifest_extension_limit() {
    let mut world = ExtWorld::new();
    let result = world.install(Tier::T3, owner_manifest(OWNER, points_16(17)), Vec::new());
    assert_eq!(diag_code(&result), "manifest.extension-limit");
}

/// REQ: EXT-001
#[test]
fn ext_001_sixty_four_extensions_install_ok() {
    let mut world = ExtWorld::new();
    let manifest = contributor_manifest("com.example.many", extensions_64(64));
    let result = world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))]);
    assert_eq!(diag_code(&result), "ok");
}

/// REQ: EXT-001
#[test]
fn ext_001_sixty_five_extensions_fail_manifest_extension_limit() {
    let mut world = ExtWorld::new();
    let manifest = contributor_manifest("com.example.many", extensions_64(65));
    let result = world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))]);
    assert_eq!(diag_code(&result), "manifest.extension-limit");
}

// ---------------------------------------------------------------------------------------------
// EXT-002 grants
// ---------------------------------------------------------------------------------------------

fn world_with_ungranted_plugin() -> ExtWorld {
    let mut world = hook_world("veto", vec![contrib("c1", 50, pass(1))]);
    let bare = json!({"id": "com.example.nogrant"});
    let installed = world.install(Tier::T3, bare, Vec::new());
    assert_eq!(diag_code(&installed), "ok");
    world
}

/// REQ: EXT-002
#[test]
fn ext_002_run_hook_without_provide_grant_not_permitted() {
    let mut world = world_with_ungranted_plugin();
    let error = err_of(world.run_hook("com.example.nogrant", POINT, r#"{"text":"a"}"#));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
    assert!(
        message_has_prefix(&error, "extension.not-granted"),
        "{}",
        error.message
    );
}

/// REQ: EXT-002
#[test]
fn ext_002_list_without_provide_grant_not_permitted() {
    let world = world_with_ungranted_plugin();
    let error = err_of(world.list("com.example.nogrant", POINT));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
    assert!(message_has_prefix(&error, "extension.not-granted"));
}

/// REQ: EXT-002
#[test]
fn ext_002_register_without_contribute_grant_not_permitted() {
    let mut world = ExtWorld::new();
    let point = with(collect_point("c", 8, true), "dynamic", json!(true));
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, json!({"id": "com.example.nogrant"}), Vec::new())),
        "ok"
    );
    let error = err_of(world.register(
        "com.example.nogrant",
        "com.example.owner/c",
        "e1",
        50,
        r#"{"id":"e1"}"#,
    ));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
    assert!(message_has_prefix(&error, "extension.not-granted"));
}

/// REQ: EXT-002
#[test]
fn ext_002_contributor_cannot_run_hook_of_the_owner_not_permitted() {
    let mut world = hook_world("veto", vec![contrib("c1", 50, pass(1))]);
    let error = err_of(world.run_hook("com.example.c1", POINT, r#"{"text":"a"}"#));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
    assert!(message_has_prefix(&error, "extension.not-granted"));
}

/// REQ: EXT-002
#[test]
fn ext_002_owner_with_grant_runs_hook() {
    let mut world = hook_world("veto", vec![contrib("c1", 50, pass(1))]);
    assert!(world.run_hook(OWNER, POINT, r#"{"text":"a"}"#).is_ok());
}

/// REQ: EXT-002
#[test]
fn ext_002_manifest_without_provide_grant_fails_grant_missing_extension() {
    let mut world = ExtWorld::new();
    let manifest = json!({
        "id": OWNER,
        "contributes": {"extension_points": [hook_point("p", "observe")]}
    });
    let result = world.install(Tier::T3, manifest, Vec::new());
    assert_eq!(diag_code(&result), "manifest.grant-missing-extension");
}

/// REQ: EXT-002
#[test]
fn ext_002_wildcard_grant_fails_grant_wildcard() {
    let mut world = ExtWorld::new();
    let manifest = json!({
        "id": "com.example.wild",
        "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:com.example.owner/*"]},
        "contributes": {"extensions": [ext_handler(POINT, "x", 50, "h")]}
    });
    let result = world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))]);
    assert_eq!(diag_code(&result), "manifest.grant-wildcard");
}

// ---------------------------------------------------------------------------------------------
// EXT-003 message prefix
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-003
#[test]
fn ext_003_veto_message_starts_with_code_plugin_id_and_reason() {
    let mut world = hook_world("veto", vec![contrib("quiet-hours", 50, refuse(1, "night"))]);
    let (outcome, _) = run_chain(&mut world, r#"{"text":"a"}"#);
    let error = refused_error(&outcome);
    assert_eq!(code_of(&error), ErrorCode::Conflict);
    assert!(
        error
            .message
            .starts_with("extension.vetoed: com.example.quiet-hours: "),
        "{}",
        error.message
    );
}

/// REQ: EXT-003
#[test]
fn ext_003_unknown_point_message_starts_with_code_colon_space() {
    let mut world = hook_world("veto", Vec::new());
    let error = err_of(world.run_hook(OWNER, "com.example.owner/missing", "{}"));
    assert!(
        message_has_prefix(&error, "extension.unknown-point"),
        "{}",
        error.message
    );
}

/// REQ: EXT-003
#[test]
fn ext_003_not_granted_message_starts_with_code_colon_space() {
    let mut world = world_with_ungranted_plugin();
    let error = err_of(world.run_hook("com.example.nogrant", POINT, "{}"));
    assert!(message_has_prefix(&error, "extension.not-granted"));
}

/// REQ: EXT-003
#[test]
fn ext_003_reentrant_message_starts_with_code_colon_space() {
    let mut world = hook_world(
        "observe",
        vec![contrib("c1", 50, runs_hook(1, POINT, r#"{"text":"a"}"#))],
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    let errors = world.hook_errors();
    let reentrant: Vec<_> = errors
        .iter()
        .filter(|(_, error)| message_has_prefix(error, "extension.reentrant"))
        .collect();
    assert!(!reentrant.is_empty(), "{errors:?}");
    // base code conflict, as EXT-022 states for a reentrant run
    assert!(
        reentrant
            .iter()
            .all(|(_, error)| code_of(error) == ErrorCode::Conflict),
        "{errors:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// EXT-006 versions
// ---------------------------------------------------------------------------------------------

/// Installs an owner with the given point version and one contributor naming `requested`.
fn active_for(owner_version: &str, requested: Option<&str>) -> bool {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "observe"), "version", json!(owner_version));
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    let target = match requested {
        Some(version) => format!("{POINT}@{version}"),
        None => POINT.to_owned(),
    };
    let manifest = contributor_manifest("com.example.v", vec![ext_handler(&target, "x", 50, "h")]);
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))])),
        "ok"
    );
    world.settle();
    world.is_active("com.example.v", POINT, "x")
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_1_2_0_accepts_requested_1_0_0() {
    assert!(active_for("1.2.0", Some("1.0.0")));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_1_2_0_accepts_requested_1_2_0() {
    assert!(active_for("1.2.0", Some("1.2.0")));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_1_2_0_ignores_patch_of_requested_1_2_9() {
    assert!(active_for("1.2.0", Some("1.2.9")));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_1_2_0_dormant_for_requested_1_3_0() {
    assert!(!active_for("1.2.0", Some("1.3.0")));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_1_2_0_dormant_for_requested_2_0_0() {
    assert!(!active_for("1.2.0", Some("2.0.0")));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_1_2_0_dormant_for_requested_0_9_0() {
    assert!(!active_for("1.2.0", Some("0.9.0")));
}

/// REQ: EXT-006
#[test]
fn ext_006_contribution_without_version_matches_any_owner_version() {
    assert!(active_for("1.2.0", None));
    assert!(active_for("0.1.2", None));
    assert!(active_for("7.0.0", None));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_0_1_2_accepts_requested_0_1_0_and_0_1_9() {
    assert!(active_for("0.1.2", Some("0.1.0")));
    assert!(active_for("0.1.2", Some("0.1.9")));
}

/// REQ: EXT-006
#[test]
fn ext_006_owner_0_1_2_dormant_for_requested_0_2_0_and_0_0_5() {
    assert!(!active_for("0.1.2", Some("0.2.0")));
    assert!(!active_for("0.1.2", Some("0.0.5")));
}

/// REQ: EXT-006
#[test]
fn ext_006_dormant_contribution_becomes_active_when_owner_updates() {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "observe"), "version", json!("1.0.0"));
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    let manifest = contributor_manifest(
        "com.example.v",
        vec![ext_handler(&format!("{POINT}@1.3.0"), "x", 50, "h")],
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))])),
        "ok"
    );
    assert!(!world.is_active("com.example.v", POINT, "x"));
    let newer = with(hook_point("p", "observe"), "version", json!("1.3.0"));
    assert_eq!(
        diag_code(&world.update(Tier::T3, owner_manifest(OWNER, vec![newer]), Vec::new())),
        "ok"
    );
    world.settle();
    assert!(world.is_active("com.example.v", POINT, "x"));
}

proptest! {
    // REQ: EXT-006
    #[test]
    fn ext_006_version_rule_matches_the_manual_for_all_small_versions(
        owner_major in 0u8..3, owner_minor in 0u8..4, owner_patch in 0u8..3,
        req_major in 0u8..3, req_minor in 0u8..4, req_patch in 0u8..3,
    ) {
        let owner = format!("{owner_major}.{owner_minor}.{owner_patch}");
        let requested = format!("{req_major}.{req_minor}.{req_patch}");
        let expected = if owner_major != req_major {
            false
        } else if owner_major == 0 {
            owner_minor == req_minor
        } else {
            req_minor <= owner_minor
        };
        prop_assert_eq!(active_for(&owner, Some(&requested)), expected);
    }
}

// ---------------------------------------------------------------------------------------------
// EXT-007 / EXT-008 ordering
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-007
#[test]
fn ext_007_same_priority_runs_in_plugin_id_order() {
    let mut world = hook_world(
        "observe",
        vec![contrib("zeta", 10, pass(1)), contrib("alpha", 10, pass(1))],
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(
        run_order(&world),
        vec!["com.example.alpha", "com.example.zeta"]
    );
}

/// REQ: EXT-007
#[test]
fn ext_007_same_plugin_and_priority_orders_by_extension_id_bytewise() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![hook_point("p", "observe")]),
            Vec::new()
        )),
        "ok"
    );
    let manifest = contributor_manifest(
        "com.example.two",
        vec![
            ext_handler(POINT, "ext-9", 10, "h9"),
            ext_handler(POINT, "ext-10", 10, "h10"),
        ],
    );
    let handlers = vec![("h9".to_owned(), pass(1)), ("h10".to_owned(), pass(1))];
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    let handlers_run: Vec<String> = world
        .handler_runs()
        .into_iter()
        .map(|run| run.handler)
        .collect();
    assert_eq!(handlers_run, vec!["h10", "h9"]);
}

/// REQ: EXT-007
#[test]
fn ext_007_t1_runs_before_t3_of_any_priority() {
    let mut world = hook_world(
        "observe",
        vec![
            contrib_tier("late", 0, Tier::T3, pass(1)),
            contrib_tier("early", 90, Tier::T1, pass(1)),
        ],
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(
        run_order(&world),
        vec!["com.example.early", "com.example.late"]
    );
}

/// REQ: EXT-007
#[test]
fn ext_007_t1_runs_before_t2_of_any_priority() {
    let mut world = hook_world(
        "observe",
        vec![
            contrib_tier("t2", 0, Tier::T2, pass(1)),
            contrib_tier("t1", 100, Tier::T1, pass(1)),
        ],
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(run_order(&world), vec!["com.example.t1", "com.example.t2"]);
}

proptest! {
    // REQ: EXT-007, SEC-288
    #[test]
    fn ext_007_run_order_is_tier_band_then_priority_then_plugin_id(
        specs in proptest::collection::vec((0u8..=100, 0usize..3, 0usize..6), 1..6)
    ) {
        let tiers = [Tier::T1, Tier::T2, Tier::T3];
        let mut contribs = Vec::new();
        let mut expected: Vec<(u8, u8, String)> = Vec::new();
        for (index, (priority, tier_index, name_index)) in specs.iter().enumerate() {
            let name = format!("n{name_index}x{index}");
            let band = if *tier_index == 0 { 0u8 } else { 1u8 };
            expected.push((band, *priority, format!("com.example.{name}")));
            contribs.push(contrib_tier(&name, *priority, tiers[*tier_index], pass(1)));
        }
        let mut world = hook_world("observe", contribs);
        let _ = run_chain(&mut world, r#"{"text":"a"}"#);
        expected.sort();
        let expected_ids: Vec<String> = expected.into_iter().map(|row| row.2).collect();
        prop_assert_eq!(run_order(&world), expected_ids);
    }
}

fn two_handler_world() -> ExtWorld {
    hook_world(
        "observe",
        vec![contrib("a", 10, pass(1)), contrib("b", 20, pass(1))],
    )
}

/// REQ: EXT-008
#[test]
fn ext_008_lower_priority_number_runs_first() {
    let mut world = two_handler_world();
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(run_order(&world), vec!["com.example.a", "com.example.b"]);
}

/// REQ: EXT-008
#[test]
fn ext_008_user_order_moves_b_above_a() {
    let mut world = two_handler_world();
    world.set_user_order(
        POINT,
        vec![
            ("com.example.b".to_owned(), "e-b".to_owned()),
            ("com.example.a".to_owned(), "e-a".to_owned()),
        ],
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(run_order(&world), vec!["com.example.b", "com.example.a"]);
}

/// REQ: EXT-008
#[test]
fn ext_008_contributor_installed_after_reorder_runs_after_both() {
    let mut world = two_handler_world();
    world.set_user_order(
        POINT,
        vec![
            ("com.example.b".to_owned(), "e-b".to_owned()),
            ("com.example.a".to_owned(), "e-a".to_owned()),
        ],
    );
    assert!(install_contrib(&mut world, POINT, &contrib("c", 0, pass(1))).is_ok());
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(
        run_order(&world),
        vec!["com.example.b", "com.example.a", "com.example.c"]
    );
}

/// REQ: EXT-008
#[test]
fn ext_008_update_keeps_stored_position() {
    let mut world = two_handler_world();
    world.set_user_order(
        POINT,
        vec![
            ("com.example.b".to_owned(), "e-b".to_owned()),
            ("com.example.a".to_owned(), "e-a".to_owned()),
        ],
    );
    let again = contrib("a", 10, pass(1));
    let manifest = contributor_manifest(
        &again.plugin,
        vec![ext_handler(POINT, &again.extension, 10, "h")],
    );
    assert_eq!(
        diag_code(&world.update(Tier::T3, manifest, vec![("h".to_owned(), pass(1))])),
        "ok"
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(run_order(&world), vec!["com.example.b", "com.example.a"]);
}

/// REQ: EXT-008
#[test]
fn ext_008_uninstall_removes_the_stored_position() {
    let mut world = two_handler_world();
    world.set_user_order(
        POINT,
        vec![
            ("com.example.b".to_owned(), "e-b".to_owned()),
            ("com.example.a".to_owned(), "e-a".to_owned()),
        ],
    );
    world.uninstall("com.example.b");
    assert!(install_contrib(&mut world, POINT, &contrib("b", 20, pass(1))).is_ok());
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(run_order(&world), vec!["com.example.a", "com.example.b"]);
}

/// REQ: EXT-008
#[test]
fn ext_008_list_on_collect_point_follows_the_user_order() {
    let mut world = ExtWorld::new();
    let point = collect_point("c", 8, false);
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    for (name, priority) in [("a", 10u8), ("b", 20u8)] {
        let target = "com.example.owner/c";
        let manifest = json!({
            "id": format!("com.example.{name}"),
            "capabilities": {"server": [format!("extensions.contribute:{target}")]},
            "contributes": {"extensions": [ext_entry(target, &format!("e-{name}"), priority, json!({"id": name}))]}
        });
        assert_eq!(
            diag_code(&world.install(Tier::T3, manifest, Vec::new())),
            "ok"
        );
    }
    let before: Vec<String> = match world.list(OWNER, "com.example.owner/c") {
        Ok(entries) => entries.into_iter().map(|entry| entry.extension).collect(),
        Err(error) => vec![format!("{error:?}")],
    };
    assert_eq!(before, vec!["e-a", "e-b"]);
    world.set_user_order(
        "com.example.owner/c",
        vec![
            ("com.example.b".to_owned(), "e-b".to_owned()),
            ("com.example.a".to_owned(), "e-a".to_owned()),
        ],
    );
    let after: Vec<String> = match world.list(OWNER, "com.example.owner/c") {
        Ok(entries) => entries.into_iter().map(|entry| entry.extension).collect(),
        Err(error) => vec![format!("{error:?}")],
    };
    assert_eq!(after, vec!["e-b", "e-a"]);
}

/// REQ: EXT-008
#[test]
fn ext_008_list_returns_declared_priority_unchanged_by_tier() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![collect_point("c", 8, false)]),
            Vec::new()
        )),
        "ok"
    );
    let target = "com.example.owner/c";
    let manifest = json!({
        "id": "com.example.t3",
        "capabilities": {"server": [format!("extensions.contribute:{target}")]},
        "contributes": {"extensions": [ext_entry(target, "e", 77, json!({"id": "x"}))]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "ok"
    );
    let entries = world.list(OWNER, target);
    assert!(
        matches!(&entries, Ok(list) if list.len() == 1 && list[0].priority == 77),
        "{entries:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// EXT-024 / EXT-025 unknown and static points
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-024
#[test]
fn ext_024_run_hook_on_undeclared_point_not_found() {
    let mut world = hook_world("veto", Vec::new());
    let error = err_of(world.run_hook(OWNER, "x/y", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
    assert!(message_has_prefix(&error, "extension.unknown-point"));
}

/// REQ: EXT-024
#[test]
fn ext_024_point_of_disabled_owner_is_unknown() {
    let mut world = hook_world("veto", Vec::new());
    world.set_enabled(OWNER, false);
    let error = err_of(world.run_hook(OWNER, POINT, "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: EXT-024
#[test]
fn ext_024_point_of_uninstalled_owner_is_unknown() {
    let mut world = hook_world("veto", Vec::new());
    world.uninstall(OWNER);
    let error = err_of(world.run_hook(OWNER, POINT, "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

/// REQ: EXT-024
#[test]
fn ext_024_list_on_undeclared_point_not_found() {
    let world = hook_world("veto", Vec::new());
    let error = err_of(world.list(OWNER, "x/y"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
    assert!(message_has_prefix(&error, "extension.unknown-point"));
}

fn collect_world(dynamic: bool) -> ExtWorld {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![collect_point("c", 8, dynamic)]),
            Vec::new()
        )),
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

const COLLECT: &str = "com.example.owner/c";

/// REQ: EXT-025
#[test]
fn ext_025_register_on_static_point_unsupported_not_dynamic() {
    let mut world = collect_world(false);
    let error = err_of(world.register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"e1"}"#));
    assert_eq!(code_of(&error), ErrorCode::Unsupported);
    assert!(message_has_prefix(&error, "extension.not-dynamic"));
}

/// REQ: EXT-025
#[test]
fn ext_025_unregister_on_static_point_unsupported_not_dynamic() {
    let mut world = collect_world(false);
    let error = err_of(world.unregister("com.example.reg", COLLECT, "e1"));
    assert_eq!(code_of(&error), ErrorCode::Unsupported);
    assert!(message_has_prefix(&error, "extension.not-dynamic"));
}

/// REQ: EXT-025
#[test]
fn ext_025_register_on_dynamic_point_ok_and_listed() {
    let mut world = collect_world(true);
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"e1"}"#)
        .is_ok());
    let listed = world.list(OWNER, COLLECT);
    assert!(
        matches!(&listed, Ok(list) if list.len() == 1 && list[0].extension == "e1"),
        "{listed:?}"
    );
}

/// REQ: EXT-025
#[test]
fn ext_025_register_same_id_replaces_the_entry() {
    let mut world = collect_world(true);
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"first"}"#)
        .is_ok());
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 60, r#"{"id":"second"}"#)
        .is_ok());
    let listed = world.list(OWNER, COLLECT);
    assert!(
        matches!(&listed, Ok(list) if list.len() == 1 && list[0].priority == 60 && list[0].entry_json.contains("second")),
        "{listed:?}"
    );
}

/// REQ: EXT-025
#[test]
fn ext_025_unregister_removes_only_own_entry_and_unknown_is_not_found() {
    let mut world = collect_world(true);
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"e1"}"#)
        .is_ok());
    assert!(world.unregister("com.example.reg", COLLECT, "e1").is_ok());
    let error = err_of(world.unregister("com.example.reg", COLLECT, "e1"));
    assert_eq!(code_of(&error), ErrorCode::NotFound);
}

// ---------------------------------------------------------------------------------------------
// EXT-026 changed topic
// ---------------------------------------------------------------------------------------------

fn changed_topics(world: &ExtWorld, plugin: &str) -> Vec<Value> {
    world
        .topics_received(plugin)
        .into_iter()
        .filter(|(topic, _)| topic == "ilium.extensions/changed")
        .map(|(_, payload)| serde_json::from_str(&payload).unwrap_or(Value::Null))
        .collect()
}

/// REQ: EXT-026
#[test]
fn ext_026_contributor_install_sends_one_changed_topic_to_the_owner() {
    let mut world = hook_world("observe", Vec::new());
    assert!(install_contrib(&mut world, POINT, &contrib("c1", 50, pass(1))).is_ok());
    world.advance_ms(100);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER), vec![json!({"point": POINT})]);
}

/// REQ: EXT-026
#[test]
fn ext_026_nobody_but_the_owner_gets_the_topic() {
    let mut world = hook_world("observe", Vec::new());
    assert!(install_contrib(&mut world, POINT, &contrib("c1", 50, pass(1))).is_ok());
    world.advance_ms(100);
    world.settle();
    assert!(changed_topics(&world, "com.example.c1").is_empty());
}

/// REQ: EXT-026
#[test]
fn ext_026_payload_carries_the_global_point_name() {
    let mut world = hook_world("observe", Vec::new());
    assert!(install_contrib(&mut world, POINT, &contrib("c1", 50, pass(1))).is_ok());
    world.advance_ms(100);
    world.settle();
    let topics = changed_topics(&world, OWNER);
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0]["point"], json!("com.example.owner/p"));
}

/// REQ: EXT-026
#[test]
fn ext_026_three_registers_within_50_ms_merge_into_one_topic() {
    let mut world = collect_world(true);
    world.advance_ms(100);
    world.settle();
    let before = changed_topics(&world, OWNER).len();
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
        world.advance_ms(10);
    }
    world.advance_ms(100);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len() - before, 1);
}

/// REQ: EXT-026
#[test]
fn ext_026_registers_more_than_50_ms_apart_send_two_topics() {
    let mut world = collect_world(true);
    world.advance_ms(100);
    world.settle();
    let before = changed_topics(&world, OWNER).len();
    assert!(world
        .register("com.example.reg", COLLECT, "e0", 50, r#"{"id":"x"}"#)
        .is_ok());
    world.advance_ms(200);
    world.settle();
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"x"}"#)
        .is_ok());
    world.advance_ms(200);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len() - before, 2);
}

/// REQ: EXT-026
#[test]
fn ext_026_uninstall_disable_enable_and_unregister_each_send_a_topic() {
    let mut world = collect_world(true);
    assert!(world
        .register("com.example.reg", COLLECT, "e0", 50, r#"{"id":"x"}"#)
        .is_ok());
    world.advance_ms(200);
    world.settle();
    let mut seen = changed_topics(&world, OWNER).len();
    world.set_enabled("com.example.reg", false);
    world.advance_ms(200);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len(), seen + 1, "disable");
    seen += 1;
    world.set_enabled("com.example.reg", true);
    world.advance_ms(200);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len(), seen + 1, "enable");
    seen += 1;
    assert!(world.unregister("com.example.reg", COLLECT, "e0").is_ok());
    world.advance_ms(200);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len(), seen + 1, "unregister");
    seen += 1;
    assert!(world
        .register("com.example.reg", COLLECT, "e1", 50, r#"{"id":"x"}"#)
        .is_ok());
    world.advance_ms(200);
    world.settle();
    seen += 1;
    assert_eq!(changed_topics(&world, OWNER).len(), seen, "re-register");
    world.uninstall("com.example.reg");
    world.advance_ms(200);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len(), seen + 1, "uninstall");
}

/// REQ: EXT-026
#[test]
fn ext_026_user_reorder_sends_a_topic() {
    let mut world = hook_world(
        "observe",
        vec![contrib("a", 10, pass(1)), contrib("b", 20, pass(1))],
    );
    world.advance_ms(200);
    world.settle();
    let before = changed_topics(&world, OWNER).len();
    world.set_user_order(
        POINT,
        vec![
            ("com.example.b".to_owned(), "e-b".to_owned()),
            ("com.example.a".to_owned(), "e-a".to_owned()),
        ],
    );
    world.advance_ms(200);
    world.settle();
    assert_eq!(changed_topics(&world, OWNER).len() - before, 1);
}

/// REQ: EXT-026
#[test]
fn ext_026_owner_needs_no_topics_subscribe_grant() {
    let mut world = hook_world("observe", Vec::new());
    assert!(install_contrib(&mut world, POINT, &contrib("c1", 50, pass(1))).is_ok());
    world.advance_ms(100);
    world.settle();
    let granted = &world.audit();
    assert!(!changed_topics(&world, OWNER).is_empty());
    assert!(granted
        .iter()
        .all(|record| record.action != "grant-topics-subscribe"));
}

// ---------------------------------------------------------------------------------------------
// EXT-027 dormant
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-027
#[test]
fn ext_027_contributor_installed_before_owner_succeeds_and_is_dormant() {
    let mut world = ExtWorld::new();
    let installed = install_contrib(&mut world, POINT, &contrib("c1", 50, pass(1)));
    assert!(installed.is_ok(), "{installed:?}");
    assert!(!world.is_active("com.example.c1", POINT, "e-c1"));
}

/// REQ: EXT-027
#[test]
fn ext_027_dormant_contribution_becomes_active_when_owner_appears() {
    let mut world = ExtWorld::new();
    assert!(install_contrib(&mut world, POINT, &contrib("c1", 50, pass(1))).is_ok());
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![hook_point("p", "observe")]),
            Vec::new()
        )),
        "ok"
    );
    world.settle();
    assert!(world.is_active("com.example.c1", POINT, "e-c1"));
}

/// REQ: EXT-027
#[test]
fn ext_027_disabled_owner_makes_contribution_dormant() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    world.set_enabled(OWNER, false);
    world.settle();
    assert!(!world.is_active("com.example.c1", POINT, "e-c1"));
}

/// REQ: EXT-027
#[test]
fn ext_027_uninstalled_owner_makes_contribution_dormant() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    world.uninstall(OWNER);
    world.settle();
    assert!(!world.is_active("com.example.c1", POINT, "e-c1"));
}

/// REQ: EXT-027
#[test]
fn ext_027_inactive_owner_does_not_make_contribution_dormant() {
    let mut world = hook_world("observe", vec![contrib("c1", 50, pass(1))]);
    world.restart(OWNER);
    assert!(world.is_active("com.example.c1", POINT, "e-c1"));
}

// ---------------------------------------------------------------------------------------------
// EXT-028 side
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-028
#[test]
fn ext_028_client_handler_for_server_point_fails_extension_side() {
    let mut world = hook_world("observe", Vec::new());
    let manifest = contributor_manifest_on(
        "com.example.cli",
        "client",
        vec![ext_handler(POINT, "x", 50, "h")],
    );
    let result = world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))]);
    assert_eq!(diag_code(&result), "manifest.extension-side");
}

/// REQ: EXT-028
#[test]
fn ext_028_server_handler_for_client_point_fails_extension_side() {
    let mut world = ExtWorld::new();
    let owner = owner_manifest_on(OWNER, "client", vec![hook_point("p", "observe")]);
    assert_eq!(diag_code(&world.install(Tier::T3, owner, Vec::new())), "ok");
    let manifest = contributor_manifest_on(
        "com.example.srv",
        "server",
        vec![ext_handler(POINT, "x", 50, "h")],
    );
    let result = world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))]);
    assert_eq!(diag_code(&result), "manifest.extension-side");
}

/// REQ: EXT-028
#[test]
fn ext_028_handler_on_the_owner_side_installs() {
    let mut world = ExtWorld::new();
    let owner = owner_manifest_on(OWNER, "client", vec![hook_point("p", "observe")]);
    assert_eq!(diag_code(&world.install(Tier::T3, owner, Vec::new())), "ok");
    let manifest = contributor_manifest_on(
        "com.example.cli",
        "client",
        vec![ext_handler(POINT, "x", 50, "h")],
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))])),
        "ok"
    );
}

// ---------------------------------------------------------------------------------------------
// EXT-043 entry-bound services (on a core point), EXT-044/045 manifest rules
// ---------------------------------------------------------------------------------------------

fn service_entry_manifest(id: &str, service: &str, provides: bool) -> Value {
    let provided = if provides {
        vec![service.to_owned()]
    } else {
        Vec::new()
    };
    let mut grants = vec!["extensions.contribute:ilium.core.search/sources".to_owned()];
    grants.extend(
        provided
            .iter()
            .map(|name| format!("services.provide:{name}")),
    );
    let services: Vec<Value> = provided
        .iter()
        .map(|name| json!({"name": name, "version": "0.1.0", "schema": service_schema()}))
        .collect();
    json!({
        "id": id,
        "exports": ["service-provider"],
        "capabilities": {"server": grants},
        "contributes": {
            "services": {"provides": services},
            "extensions": [ext_entry(
                "ilium.core.search/sources", "issues", 50,
                json!({"id": "issues", "title": "Issues", "kind": "issue", "service": service})
            )]
        }
    })
}

/// REQ: EXT-043
#[test]
fn ext_043_entry_service_of_own_prefix_that_it_provides_installs() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = service_entry_manifest("com.example.issues", "com.example.issues/search", true);
    let result = world.install(
        Tier::T3,
        manifest,
        vec![(
            "com.example.issues/search#query".to_owned(),
            script(1, HandlerAction::ServiceDone("[]".to_owned())),
        )],
    );
    assert_eq!(diag_code(&result), "ok");
}

/// REQ: EXT-043
#[test]
fn ext_043_entry_service_of_another_plugin_fails_extension_entry_invalid() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = service_entry_manifest("com.example.issues", "com.example.other/search", false);
    let result = world.install(Tier::T3, manifest, Vec::new());
    assert_eq!(diag_code(&result), "manifest.extension-entry-invalid");
}

/// REQ: EXT-043
#[test]
fn ext_043_entry_service_with_own_prefix_but_not_provided_fails_extension_entry_invalid() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = service_entry_manifest("com.example.issues", "com.example.issues/search", false);
    let result = world.install(Tier::T3, manifest, Vec::new());
    assert_eq!(diag_code(&result), "manifest.extension-entry-invalid");
}

/// REQ: EXT-043
#[test]
fn ext_043_owner_calls_entry_service_without_services_use() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = service_entry_manifest("com.example.issues", "com.example.issues/search", true);
    let handlers = vec![(
        "com.example.issues/search#query".to_owned(),
        script(1, HandlerAction::ServiceDone("[]".to_owned())),
    )];
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    world.settle();
    let request = world.call_service(
        "ilium.core.search",
        "com.example.issues/search",
        "query",
        "{}",
    );
    assert!(request.is_ok(), "{request:?}");
}

/// REQ: EXT-043
#[test]
fn ext_043_other_plugin_without_services_use_is_still_not_permitted() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = service_entry_manifest("com.example.issues", "com.example.issues/search", true);
    let handlers = vec![(
        "com.example.issues/search#query".to_owned(),
        script(1, HandlerAction::ServiceDone("[]".to_owned())),
    )];
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, handlers)),
        "ok"
    );
    assert_eq!(
        diag_code(&world.install(Tier::T3, json!({"id": "com.example.nogrant"}), Vec::new())),
        "ok"
    );
    let error = err_of(world.call_service(
        "com.example.nogrant",
        "com.example.issues/search",
        "query",
        "{}",
    ));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

fn high_veto_point(extra: Value) -> Value {
    let mut point = with(hook_point("p", "veto"), "risk", json!("high"));
    if let (Some(target), Some(source)) = (point.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    point
}

fn install_owner(point: Value) -> Result<(), Diagnostic> {
    let mut world = ExtWorld::new();
    world.install(Tier::T1, owner_manifest(OWNER, vec![point]), Vec::new())
}

/// REQ: EXT-044
#[test]
fn ext_044_on_failure_on_high_veto_point_is_allowed() {
    assert_eq!(
        diag_code(&install_owner(high_veto_point(
            json!({"on_failure": "refuse"})
        ))),
        "ok"
    );
    assert_eq!(
        diag_code(&install_owner(high_veto_point(
            json!({"on_failure": "pass"})
        ))),
        "ok"
    );
}

/// REQ: EXT-044
#[test]
fn ext_044_on_failure_on_high_transform_point_is_allowed() {
    let point = with(
        with(hook_point("p", "transform"), "risk", json!("high")),
        "on_failure",
        json!("refuse"),
    );
    assert_eq!(diag_code(&install_owner(point)), "ok");
}

/// REQ: EXT-044
#[test]
fn ext_044_on_failure_on_observe_point_fails_extension_point_invalid() {
    let point = with(
        with(hook_point("p", "observe"), "risk", json!("high")),
        "on_failure",
        json!("pass"),
    );
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-044
#[test]
fn ext_044_on_failure_on_standard_veto_point_fails_extension_point_invalid() {
    let point = with(hook_point("p", "veto"), "on_failure", json!("pass"));
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-044
#[test]
fn ext_044_on_failure_on_first_point_fails_extension_point_invalid() {
    let point = with(
        with(hook_point("p", "first"), "risk", json!("high")),
        "on_failure",
        json!("pass"),
    );
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-044
#[test]
fn ext_044_final_check_on_transform_point_is_allowed() {
    let point = with(hook_point("p", "transform"), "final_check", json!(true));
    assert_eq!(diag_code(&install_owner(point)), "ok");
}

/// REQ: EXT-044
#[test]
fn ext_044_final_check_on_veto_point_fails_extension_point_invalid() {
    let point = with(hook_point("p", "veto"), "final_check", json!(true));
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-044
#[test]
fn ext_044_guard_on_point_with_final_check_installs() {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "transform"), "final_check", json!(true));
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    let manifest = contributor_manifest("com.example.g", vec![ext_guard(POINT, "g", 50, "h")]);
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))])),
        "ok"
    );
}

/// REQ: EXT-044
#[test]
fn ext_044_guard_on_point_without_final_check_fails_extension_point_invalid() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest(OWNER, vec![hook_point("p", "transform")]),
            Vec::new()
        )),
        "ok"
    );
    let manifest = contributor_manifest("com.example.g", vec![ext_guard(POINT, "g", 50, "h")]);
    let result = world.install(Tier::T3, manifest, vec![("h".to_owned(), pass(0))]);
    assert_eq!(diag_code(&result), "manifest.extension-point-invalid");
}

/// REQ: EXT-044
#[test]
fn ext_044_guard_without_handler_fails_extension_point_invalid() {
    let mut world = ExtWorld::new();
    let point = with(hook_point("p", "transform"), "final_check", json!(true));
    assert_eq!(
        diag_code(&world.install(Tier::T3, owner_manifest(OWNER, vec![point]), Vec::new())),
        "ok"
    );
    let guard = json!({"point": POINT, "id": "g", "priority": 50, "guard": true});
    let result = world.install(
        Tier::T3,
        contributor_manifest("com.example.g", vec![guard]),
        Vec::new(),
    );
    assert_eq!(diag_code(&result), "manifest.extension-point-invalid");
}

/// REQ: EXT-045
#[test]
fn ext_045_veto_point_with_reply_schema_fails_extension_point_invalid() {
    let point = with(hook_point("p", "veto"), "reply_schema", reply_schema_text());
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-045
#[test]
fn ext_045_observe_point_with_reply_schema_fails_extension_point_invalid() {
    let point = with(
        hook_point("p", "observe"),
        "reply_schema",
        reply_schema_text(),
    );
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-045
#[test]
fn ext_045_transform_point_without_reply_schema_fails_extension_point_invalid() {
    let mut point = hook_point("p", "transform");
    if let Some(object) = point.as_object_mut() {
        object.remove("reply_schema");
    }
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-045
#[test]
fn ext_045_first_point_without_reply_schema_fails_extension_point_invalid() {
    let mut point = hook_point("p", "first");
    if let Some(object) = point.as_object_mut() {
        object.remove("reply_schema");
    }
    assert_eq!(
        diag_code(&install_owner(point)),
        "manifest.extension-point-invalid"
    );
}

/// REQ: EXT-045
#[test]
fn ext_045_transform_and_first_points_with_reply_schema_install() {
    assert_eq!(
        diag_code(&install_owner(hook_point("p", "transform"))),
        "ok"
    );
    assert_eq!(diag_code(&install_owner(hook_point("p", "first"))), "ok");
}

/// REQ: EXT-045
#[test]
fn ext_045_observe_and_veto_points_without_reply_schema_install() {
    assert_eq!(diag_code(&install_owner(hook_point("p", "observe"))), "ok");
    assert_eq!(diag_code(&install_owner(hook_point("p", "veto"))), "ok");
}

/// REQ: SEC-288
#[test]
fn sec_288_equal_tier_and_priority_orders_by_plugin_id() {
    let mut world = hook_world(
        "observe",
        vec![contrib("b", 10, pass(1)), contrib("a", 10, pass(1))],
    );
    let _ = run_chain(&mut world, r#"{"text":"a"}"#);
    assert_eq!(run_order(&world), vec!["com.example.a", "com.example.b"]);
}
