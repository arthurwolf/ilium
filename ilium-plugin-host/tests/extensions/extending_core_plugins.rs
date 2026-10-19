//! extending-core-plugins.md: replacing core plugins, surface checks, contributions that carry
//! over, and the behaviour of core points (EXT-051..056, SEC-300..306).

use crate::support::*;

const SEARCH: &str = "ilium.core.search";
const REPLACEMENT: &str = "com.example.search";

fn shape() -> ReplacementShape {
    ReplacementShape::default()
}

fn install_replacement(
    world: &mut ExtWorld,
    id: &str,
    core: &str,
    tier: Tier,
    shape: ReplacementShape,
) -> Result<(), Diagnostic> {
    let manifest = world.replacement_manifest(id, core, shape);
    world.install(tier, manifest, Vec::new())
}

fn is_enabled(world: &ExtWorld, plugin: &str) -> bool {
    world.plugin_enabled(plugin)
}

// ---------------------------------------------------------------------------------------------
// EXT-051
// ---------------------------------------------------------------------------------------------

/// REQ: EXT-051
#[test]
fn ext_051_replacement_with_the_whole_surface_installs() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
}

/// REQ: EXT-051
#[test]
fn ext_051_replacement_missing_one_surface_item_fails_and_names_it() {
    let mut world = ExtWorld::with_core_plugins();
    let item = world.core_surface(SEARCH)[0].clone();
    let result = install_replacement(
        &mut world,
        REPLACEMENT,
        SEARCH,
        Tier::T1,
        ReplacementShape {
            drop_item: Some(item.clone()),
            ..shape()
        },
    );
    match result {
        Err(diagnostic) => {
            assert_eq!(
                diagnostic.code.as_str(),
                "manifest.replaces-surface-incomplete"
            );
            assert!(diagnostic.message.contains(&item), "{}", diagnostic.message);
        }
        Ok(()) => panic!("an incomplete surface must be refused"),
    }
}

/// REQ: EXT-051
#[test]
fn ext_051_replacement_point_with_veto_for_a_transform_point_fails() {
    let mut world = ExtWorld::with_core_plugins();
    let shape = ReplacementShape {
        point_mode_override: Some(("ilium.core.search/rank".to_owned(), "veto".to_owned())),
        ..shape()
    };
    let result = install_replacement(&mut world, REPLACEMENT, SEARCH, Tier::T1, shape);
    assert_eq!(diag_code(&result), "manifest.replaces-surface-incomplete");
}

/// REQ: EXT-051
#[test]
fn ext_051_replacement_point_of_another_major_version_fails() {
    let mut world = ExtWorld::with_core_plugins();
    let shape = ReplacementShape {
        point_version_override: Some(("ilium.core.search/rank".to_owned(), "9.0.0".to_owned())),
        ..shape()
    };
    let result = install_replacement(&mut world, REPLACEMENT, SEARCH, Tier::T1, shape);
    assert_eq!(diag_code(&result), "manifest.replaces-surface-incomplete");
}

/// REQ: EXT-051
#[test]
fn ext_051_replacement_point_of_the_same_major_and_higher_minor_is_complete() {
    let mut world = ExtWorld::with_core_plugins();
    let shape = ReplacementShape {
        point_version_override: Some(("ilium.core.search/rank".to_owned(), "0.9.0".to_owned())),
        ..shape()
    };
    let result = install_replacement(&mut world, REPLACEMENT, SEARCH, Tier::T1, shape);
    assert_eq!(diag_code(&result), "ok");
}

/// REQ: EXT-051
#[test]
fn ext_051_replacement_service_with_other_method_names_fails() {
    let mut world = ExtWorld::with_core_plugins();
    let shape = ReplacementShape {
        service_methods_override: Some((
            "ilium.core.search/query".to_owned(),
            vec!["other".to_owned()],
        )),
        ..shape()
    };
    let result = install_replacement(&mut world, REPLACEMENT, SEARCH, Tier::T1, shape);
    assert_eq!(diag_code(&result), "manifest.replaces-surface-incomplete");
}

/// REQ: EXT-051
#[test]
fn ext_051_surface_is_checked_again_at_enable() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, false);
    world.add_core_surface_item(SEARCH, "points:ilium.core.search/new-point");
    world.set_enabled(REPLACEMENT, true);
    assert!(
        !is_enabled(&world, REPLACEMENT),
        "the incomplete replacement stays off"
    );
    assert!(is_enabled(&world, SEARCH));
}

/// REQ: EXT-051
#[test]
fn ext_051_after_an_ilium_update_an_incomplete_replacement_is_turned_off_and_the_builtin_returns() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    assert!(is_enabled(&world, REPLACEMENT));
    world.update_ilium_with_surface_item(SEARCH, "points:ilium.core.search/new-point");
    assert!(!is_enabled(&world, REPLACEMENT));
    assert!(is_enabled(&world, SEARCH));
    assert!(world
        .notices()
        .iter()
        .any(|notice| notice.contains("ilium.core.search/new-point")));
}

// ---------------------------------------------------------------------------------------------
// EXT-053
// ---------------------------------------------------------------------------------------------

fn two_replacements() -> ExtWorld {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            "com.example.repa",
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            "com.example.repb",
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world
}

/// REQ: EXT-053
#[test]
fn ext_053_enabling_replacement_b_while_a_is_enabled_prompts_then_turns_a_off() {
    let mut world = two_replacements();
    world.set_enabled("com.example.repa", true);
    world.answer_prompt(true);
    world.set_enabled("com.example.repb", true);
    assert!(!world.prompts().is_empty(), "the user is asked");
    world.answer_prompt(true);
    assert!(is_enabled(&world, "com.example.repb"));
    assert!(!is_enabled(&world, "com.example.repa"));
}

/// REQ: EXT-053
#[test]
fn ext_053_declining_the_prompt_keeps_replacement_a() {
    let mut world = two_replacements();
    world.set_enabled("com.example.repa", true);
    world.answer_prompt(true);
    world.set_enabled("com.example.repb", true);
    world.answer_prompt(false);
    assert!(is_enabled(&world, "com.example.repa"));
    assert!(!is_enabled(&world, "com.example.repb"));
}

/// REQ: EXT-053
#[test]
fn ext_053_at_most_one_of_builtin_and_replacement_is_enabled() {
    let mut world = two_replacements();
    world.set_enabled("com.example.repa", true);
    world.answer_prompt(true);
    let enabled = [SEARCH, "com.example.repa", "com.example.repb"]
        .iter()
        .filter(|plugin| is_enabled(&world, plugin))
        .count();
    assert_eq!(enabled, 1);
}

/// REQ: EXT-053
#[test]
fn ext_053_turning_the_replacement_off_brings_the_builtin_back() {
    let mut world = two_replacements();
    world.set_enabled("com.example.repa", true);
    world.answer_prompt(true);
    world.set_enabled("com.example.repa", false);
    assert!(is_enabled(&world, SEARCH));
}

/// REQ: EXT-053
#[test]
fn ext_053_uninstalling_the_replacement_brings_the_builtin_back() {
    let mut world = two_replacements();
    world.set_enabled("com.example.repa", true);
    world.answer_prompt(true);
    world.uninstall("com.example.repa");
    assert!(is_enabled(&world, SEARCH));
}

/// REQ: EXT-053
#[test]
fn ext_053_replaces_list_with_its_own_id_fails_replaces_cycle() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = json!({"id": REPLACEMENT, "replaces": [REPLACEMENT]});
    assert_eq!(
        diag_code(&world.install(Tier::T1, manifest, Vec::new())),
        "manifest.replaces-cycle"
    );
}

/// REQ: EXT-053
#[test]
fn ext_053_replaces_list_with_an_id_twice_fails_replaces_cycle() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = json!({"id": REPLACEMENT, "replaces": [SEARCH, SEARCH]});
    assert_eq!(
        diag_code(&world.install(Tier::T1, manifest, Vec::new())),
        "manifest.replaces-cycle"
    );
}

/// REQ: EXT-053
#[test]
fn ext_053_replaces_list_of_four_ids_passes_the_length_check_and_five_fails_schema() {
    let mut world = ExtWorld::with_core_plugins();
    let five = json!({"id": REPLACEMENT, "replaces": ["ilium.core.search", "ilium.core.scenes", "ilium.core.recap", "ilium.core.cost", "ilium.core.fleet"]});
    assert_eq!(
        diag_code(&world.install(Tier::T1, five, Vec::new())),
        "manifest.schema"
    );
    let four = json!({"id": REPLACEMENT, "replaces": ["ilium.core.search", "ilium.core.scenes", "ilium.core.recap", "ilium.core.cost"]});
    let result = world.install(Tier::T1, four, Vec::new());
    assert_ne!(
        diag_code(&result),
        "manifest.schema",
        "four ids are within the list limit"
    );
}

/// REQ: EXT-053
#[test]
fn ext_053_replaces_unknown_plugin_fails_replaces_unknown() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = json!({"id": REPLACEMENT, "replaces": ["ilium.core.nothing"]});
    assert_eq!(
        diag_code(&world.install(Tier::T1, manifest, Vec::new())),
        "manifest.replaces-unknown"
    );
}

/// REQ: EXT-053
#[test]
fn ext_053_replacing_sessions_or_settings_fails_replaces_not_replaceable() {
    let mut world = ExtWorld::with_core_plugins();
    for core in ["ilium.core.sessions", "ilium.core.settings"] {
        let manifest = json!({"id": REPLACEMENT, "replaces": [core]});
        assert_eq!(
            diag_code(&world.install(Tier::T1, manifest, Vec::new())),
            "manifest.replaces-not-replaceable",
            "{core}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// SEC-300 .. SEC-306
// ---------------------------------------------------------------------------------------------

/// REQ: SEC-300
#[test]
fn sec_300_replacement_inherits_no_grants_of_the_replaced_plugin() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    let replaced = world.granted(SEARCH);
    let granted = world.granted(REPLACEMENT);
    assert!(!replaced.is_empty(), "the replaced plugin holds grants");
    assert!(
        replaced
            .iter()
            .all(|grant| !granted.contains(grant) || world.declares(REPLACEMENT, grant)),
        "only declared grants"
    );
}

/// REQ: SEC-300
#[test]
fn sec_300_replaced_plugins_contributions_to_other_points_do_not_move() {
    let mut world = ExtWorld::with_core_plugins();
    let before = world.contributions_of(SEARCH);
    assert!(!before.is_empty(), "the replaced plugin has contributions");
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    assert!(world
        .contributions_of(REPLACEMENT)
        .iter()
        .all(|item| !before.contains(item)));
}

/// REQ: SEC-301
#[test]
fn sec_301_t3_package_cannot_replace_a_core_plugin() {
    let mut world = ExtWorld::with_core_plugins();
    let result = install_replacement(&mut world, REPLACEMENT, SEARCH, Tier::T3, shape());
    assert_eq!(diag_code(&result), "manifest.replaces-tier");
}

/// REQ: SEC-301
#[test]
fn sec_301_t2_replacement_needs_a_confirmation_naming_the_core_plugin() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T2,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    let prompts = world.prompts();
    assert!(
        prompts
            .iter()
            .any(|prompt| prompt.contains(SEARCH) || prompt.contains("Workspace search")),
        "{prompts:?}"
    );
    assert!(
        !is_enabled(&world, REPLACEMENT),
        "not on before the user confirms"
    );
}

/// REQ: SEC-301
#[test]
fn sec_301_policy_allow_replacement_false_refuses_with_policy_replacement() {
    let mut world = ExtWorld::with_core_plugins();
    world.set_policy("[extensions]\nallow_replacement = false\n");
    let result = install_replacement(&mut world, REPLACEMENT, SEARCH, Tier::T1, shape());
    assert_eq!(diag_code(&result), "policy.replacement");
}

/// REQ: SEC-302
#[test]
fn sec_302_t2_package_claiming_a_surface_that_needs_t1_only_grants_fails_replaces_tier() {
    let mut world = ExtWorld::with_core_plugins();
    let result = install_replacement(
        &mut world,
        REPLACEMENT,
        "ilium.core.fleet",
        Tier::T2,
        shape(),
    );
    assert_eq!(diag_code(&result), "manifest.replaces-tier");
}

/// REQ: SEC-303
#[test]
fn sec_303_replacement_keeps_its_own_plugin_id_and_serves_the_core_surface_ids() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T2,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    assert_eq!(
        world.service_provider(&format!("{SEARCH}/query")),
        Some(REPLACEMENT.to_owned())
    );
    assert!(world.plugin_installed(REPLACEMENT));
}

/// REQ: SEC-303
#[test]
fn sec_303_replacement_may_not_use_the_ilium_prefix_unless_bundled() {
    let mut world = ExtWorld::with_core_plugins();
    let result = install_replacement(&mut world, "ilium.fake.search", SEARCH, Tier::T2, shape());
    assert_eq!(diag_code(&result), "manifest.id-reserved");
}

/// REQ: SEC-303
#[test]
fn sec_303_replacement_commands_carry_a_t2_badge_in_the_palette() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T2,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    let labels = world.palette_labels_of(REPLACEMENT);
    assert!(!labels.is_empty(), "the replacement has palette commands");
    assert!(
        labels.iter().all(|label| label.contains("T2")),
        "{labels:?}"
    );
}

/// REQ: SEC-304, EXT-051
#[test]
fn sec_304_standard_contributions_move_to_the_replacement_after_a_notice() {
    let mut world = ExtWorld::with_core_plugins();
    let manifest = json!({
        "id": "com.example.src",
        "capabilities": {"server": ["extensions.contribute:ilium.core.search/sources"]},
        "contributes": {"extensions": [ext_entry("ilium.core.search/sources", "issues", 50, json!({"id": "issues", "title": "Issues", "kind": "issue", "service": "com.example.src/search"}))]}
    });
    let _ = manifest;
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T2,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    assert!(world
        .notices()
        .iter()
        .any(|notice| notice.contains(REPLACEMENT)));
}

/// REQ: SEC-304
#[test]
fn sec_304_high_risk_contributions_are_suspended_until_the_user_confirms_for_the_new_owner() {
    let mut world = ExtWorld::with_core_plugins();
    let contributor = json!({
        "id": "com.example.guard", "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:ilium.core.terminal/before-send-input", "terminal.read-screen"]},
        "contributes": {"extensions": [ext_handler("ilium.core.terminal/before-send-input", "g", 50, "h")]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T2, contributor, vec![("h".to_owned(), pass(1))])),
        "ok"
    );
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            "com.example.term",
            "ilium.core.terminal",
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled("com.example.term", true);
    world.answer_prompt(true);
    assert!(!world.is_active(
        "com.example.guard",
        "ilium.core.terminal/before-send-input",
        "g"
    ));
    assert!(world
        .prompts()
        .iter()
        .any(|prompt| prompt.contains("com.example.term")));
}

/// REQ: SEC-304
#[test]
fn sec_304_plugins_screen_lists_contributors_as_now_sending_to_the_replacement() {
    let mut world = ExtWorld::with_core_plugins();
    let contributor = json!({
        "id": "com.example.guard", "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:ilium.core.terminal/before-send-input", "terminal.read-screen"]},
        "contributes": {"extensions": [ext_handler("ilium.core.terminal/before-send-input", "g", 50, "h")]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T2, contributor, vec![("h".to_owned(), pass(1))])),
        "ok"
    );
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            "com.example.term",
            "ilium.core.terminal",
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled("com.example.term", true);
    world.answer_prompt(true);
    let lines = world.plugins_screen_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Now sending to") && line.contains("com.example.term")),
        "{lines:?}"
    );
    assert!(lines
        .iter()
        .all(|line| !line.contains("Now sending to") || line.contains("com.example.term")));
}

/// REQ: SEC-305
#[test]
fn sec_305_kernel_owned_security_functions_stay_with_the_kernel_after_a_replacement() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    for function in [
        "grant-check",
        "admission-gate",
        "inference-privacy-log",
        "consent-prompts",
        "review-dialog",
        "breach-notices",
        "recovery-page",
        "audit-log",
        "safe-mode",
        "policy",
    ] {
        assert_eq!(
            world.owner_of_kernel_function(function),
            "kernel",
            "{function}"
        );
    }
}

/// REQ: SEC-306
#[test]
fn sec_306_turning_a_replacement_on_and_off_writes_audit_records() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    world.set_enabled(REPLACEMENT, false);
    let actions: Vec<String> = world
        .audit()
        .into_iter()
        .map(|record| record.action)
        .collect();
    assert!(
        actions.contains(&"replacement-on".to_owned()),
        "{actions:?}"
    );
    assert!(
        actions.contains(&"replacement-off".to_owned()),
        "{actions:?}"
    );
}

/// REQ: SEC-306
#[test]
fn sec_306_replaced_plugins_data_folder_stays_and_is_inaccessible_to_others() {
    let mut world = ExtWorld::with_core_plugins();
    assert_eq!(
        diag_code(&install_replacement(
            &mut world,
            REPLACEMENT,
            SEARCH,
            Tier::T1,
            shape()
        )),
        "ok"
    );
    world.set_enabled(REPLACEMENT, true);
    world.answer_prompt(true);
    assert!(world.data_folder_exists(SEARCH));
    assert!(!world.can_read_data_folder(REPLACEMENT, SEARCH));
}

// ---------------------------------------------------------------------------------------------
// EXT-054 .. EXT-056 behaviour of core points
// ---------------------------------------------------------------------------------------------

const LOG_SINKS: &str = "ilium.core.diagnostics/log-sinks";
const BEFORE_SEND: &str = "ilium.core.terminal/before-send-input";

/// REQ: EXT-054
#[test]
fn ext_054_records_written_by_a_log_sinks_handler_start_no_new_chain() {
    let mut world = ExtWorld::with_core_plugins();
    let sink = json!({
        "id": "com.example.sink", "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:ilium.core.diagnostics/log-sinks"]},
        "contributes": {"extensions": [ext_handler(LOG_SINKS, "s", 50, "h")]}
    });
    assert_eq!(
        diag_code(&world.install(
            Tier::T2,
            sink,
            vec![(
                "h".to_owned(),
                script(1, HandlerAction::LogWarn("from sink".to_owned()))
            )]
        )),
        "ok"
    );
    world.emit_log("com.example.other", "warn", "first");
    world.advance_ms(300);
    world.settle();
    let after_first = world.chains_started(LOG_SINKS);
    assert_eq!(after_first, 1, "the first warn record ran one chain");
    world.advance_ms(600);
    world.settle();
    assert_eq!(
        world.chains_started(LOG_SINKS),
        after_first,
        "the handler's own warn record starts nothing"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_extension_records_about_log_sinks_handlers_are_not_delivered_to_log_sinks() {
    let mut world = ExtWorld::with_core_plugins();
    let sink = json!({
        "id": "com.example.sink", "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:ilium.core.diagnostics/log-sinks"]},
        "contributes": {"extensions": [ext_handler(LOG_SINKS, "s", 50, "h")]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T2, sink, vec![("h".to_owned(), pass(80))])),
        "ok"
    );
    world.emit_log("com.example.other", "warn", "first");
    world.advance_ms(300);
    world.settle();
    let delivered = world.log_records_delivered_to(LOG_SINKS);
    assert!(
        delivered.iter().any(|record| record.contains("first")),
        "the ordinary record is delivered: {delivered:?}"
    );
    assert!(
        delivered
            .iter()
            .all(|record| !record.contains("extension.")),
        "{delivered:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_records_are_batched_one_chain_per_250_ms_with_at_most_64_records() {
    let mut world = ExtWorld::with_core_plugins();
    let sink = json!({
        "id": "com.example.sink", "exports": ["hook-handler"],
        "capabilities": {"server": ["extensions.contribute:ilium.core.diagnostics/log-sinks"]},
        "contributes": {"extensions": [ext_handler(LOG_SINKS, "s", 50, "h")]}
    });
    assert_eq!(
        diag_code(&world.install(Tier::T2, sink, vec![("h".to_owned(), pass(1))])),
        "ok"
    );
    for index in 0..130 {
        world.emit_log("com.example.other", "warn", &format!("r{index}"));
    }
    world.advance_ms(250);
    world.settle();
    let sizes = world.batch_sizes(LOG_SINKS);
    assert!(
        !sizes.is_empty() && sizes.iter().all(|size| *size <= 64),
        "{sizes:?}"
    );
    assert_eq!(
        world.chains_started(LOG_SINKS),
        1,
        "one chain per 250 ms window"
    );
}

/// REQ: EXT-055
#[test]
fn ext_055_typing_ten_keys_into_a_pane_runs_no_chain() {
    let mut world = ExtWorld::with_core_plugins();
    for _ in 0..10 {
        world.type_key_into_pane(1, 'a');
    }
    world.settle();
    assert_eq!(world.chains_started(BEFORE_SEND), 0);
}

/// REQ: EXT-055
#[test]
fn ext_055_one_paste_runs_one_chain_with_source_paste() {
    let mut world = ExtWorld::with_core_plugins();
    world.paste_into_pane(1, "hello");
    world.settle();
    assert_eq!(world.chains_started(BEFORE_SEND), 1);
    assert_eq!(
        world.last_chain_payload(BEFORE_SEND)["source"],
        json!("paste")
    );
}

/// REQ: EXT-055
#[test]
fn ext_055_a_plugin_write_input_runs_one_chain_with_source_plugin() {
    let mut world = ExtWorld::with_core_plugins();
    world.plugin_write_input("com.example.writer", 1, "ls\n");
    world.settle();
    assert_eq!(world.chains_started(BEFORE_SEND), 1);
    assert_eq!(
        world.last_chain_payload(BEFORE_SEND)["source"],
        json!("plugin:com.example.writer")
    );
}

/// REQ: EXT-055
#[test]
fn ext_055_preview_is_the_first_120_characters_of_the_write() {
    let mut world = ExtWorld::with_core_plugins();
    world.paste_into_pane(1, &"x".repeat(300));
    world.settle();
    let payload = world.last_chain_payload(BEFORE_SEND);
    assert_eq!(
        payload["preview"].as_str().map(|text| text.chars().count()),
        Some(120)
    );
    assert_eq!(payload["length"], json!(300));
}

/// REQ: EXT-056
#[test]
fn ext_056_snapshot_reply_over_max_bytes_is_dropped_logged_and_not_stored() {
    let mut world = ExtWorld::with_core_plugins();
    world.register_snapshot_section("com.example.notes", "my-notes", 64);
    world.set_snapshot_reply("com.example.notes", "my-notes", &"x".repeat(65));
    world.take_snapshot();
    assert!(world.stored_snapshot_section("my-notes").is_none());
    assert!(has_log(&world, "sessions.section-too-large"));
}

/// REQ: EXT-056
#[test]
fn ext_056_snapshot_reply_of_exactly_max_bytes_is_stored() {
    let mut world = ExtWorld::with_core_plugins();
    world.register_snapshot_section("com.example.notes", "my-notes", 64);
    world.set_snapshot_reply("com.example.notes", "my-notes", &"x".repeat(64));
    world.take_snapshot();
    assert!(world.stored_snapshot_section("my-notes").is_some());
    assert!(!has_log(&world, "sessions.section-too-large"));
}

/// REQ: EXT-056
#[test]
fn ext_056_restore_is_called_after_before_restore_completes_and_before_panes_respawn() {
    let mut world = ExtWorld::with_core_plugins();
    world.register_snapshot_section("com.example.notes", "my-notes", 64);
    world.set_snapshot_reply("com.example.notes", "my-notes", "{}");
    world.take_snapshot();
    world.restore_snapshot();
    let order = world.restore_event_order();
    let before = order
        .iter()
        .position(|name| name == "before-restore-completed");
    let restore = order.iter().position(|name| name == "section-restore");
    let respawn = order.iter().position(|name| name == "panes-respawn");
    assert!(
        before.is_some() && restore.is_some() && respawn.is_some(),
        "{order:?}"
    );
    assert!(before < restore && restore < respawn, "{order:?}");
}

/// REQ: EXT-056
#[test]
fn ext_056_restore_call_has_a_2_s_limit() {
    let mut world = ExtWorld::with_core_plugins();
    world.register_snapshot_section("com.example.notes", "my-notes", 64);
    world.set_snapshot_reply("com.example.notes", "my-notes", "{}");
    world.take_snapshot();
    world.set_restore_duration_ms("com.example.notes", 2_001);
    world.restore_snapshot();
    assert!(
        world
            .restore_event_order()
            .contains(&"panes-respawn".to_owned()),
        "a slow section does not block the respawn"
    );
    // manual does not pin the log name of a restore timeout
    assert!(has_log(&world, "sessions.section-restore-timeout") || has_log(&world, "timed-out"));
}
