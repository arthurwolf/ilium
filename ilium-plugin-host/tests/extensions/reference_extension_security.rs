//! reference-extension-security.md: risk marks, data classes, validation, vetoes, reasons and
//! project trust (SEC-280..283, 284, 286, 287, 291, 292, 293, 310..315).

use crate::support::*;

/// The shape of the hook point of the chain tests: mode, risk and final_check.
#[derive(Clone)]
struct HookShape {
    mode: &'static str,
    high: bool,
    final_check: bool,
}

impl HookShape {
    fn veto() -> HookShape {
        HookShape {
            mode: "veto",
            high: false,
            final_check: false,
        }
    }
    fn transform() -> HookShape {
        HookShape {
            mode: "transform",
            high: false,
            final_check: false,
        }
    }
    fn high(self) -> HookShape {
        HookShape { high: true, ..self }
    }
    fn with_final_check(self) -> HookShape {
        HookShape {
            final_check: true,
            ..self
        }
    }
}

/// The standard owner world with a point of the given shape and no contributor yet.
fn hook_world(shape: HookShape) -> ExtWorld {
    let mut point = hook_point("p", shape.mode);
    if shape.high {
        point["risk"] = json!("high");
    }
    if shape.final_check {
        point["final_check"] = json!(true);
    }
    hook_world_with(point, Vec::new())
}

/// What the chain tests observe of one run.
struct ChainResult {
    outcome: Option<Outcome>,
    reason: Option<String>,
    ui_text: String,
}

impl ChainResult {
    fn refused(&self) -> bool {
        matches!(self.outcome, Some(Outcome::Refused(_)))
    }
    fn reason(&self) -> Option<String> {
        self.reason.clone()
    }
    fn ui_text(&self) -> String {
        self.ui_text.clone()
    }
}

/// Installs the contributors on the standard point, runs the chain once as the owner and reads
/// the veto reason and its user-interface text (`_source` is the chain's `source` marker, 1 = plugin).
fn run_chain(world: &mut ExtWorld, _source: u32, contribs: Vec<Contrib>) -> ChainResult {
    for c in &contribs {
        let installed = install_contrib(world, POINT, c);
        assert!(
            installed.is_ok(),
            "contributor {} install failed: {installed:?}",
            c.plugin
        );
    }
    world.settle();
    let (outcome, _reply) = crate::support::run_chain(world, r#"{"text":"start","locked":"x"}"#);
    ChainResult {
        outcome,
        reason: world.last_veto_reason(),
        ui_text: world.last_veto_ui_text(),
    }
}

fn high_points_in_security_page() -> Vec<String> {
    let text = page("reference-extension-security.md");
    table_after(&text, "## Risk marks of core points")
        .into_iter()
        .filter(|row| {
            row.get(1)
                .map(|risk| risk.trim() == "high")
                .unwrap_or(false)
        })
        .map(|row| row[0].trim_matches('`').to_owned())
        .collect()
}

/// REQ: SEC-280, SEC-293
#[test]
fn doc_sec_280_security_page_lists_19_high_risk_points() {
    assert_eq!(high_points_in_security_page().len(), 19);
}

/// REQ: SEC-280, SEC-293
#[test]
fn doc_sec_293_core_points_table_marks_exactly_the_points_the_security_page_marks_high() {
    let mut from_table: Vec<String> = core_points()
        .into_iter()
        .filter(|(_, _, risk)| risk.trim() == "high")
        .map(|(point, _, _)| point)
        .collect();
    let mut from_security = high_points_in_security_page();
    from_table.sort();
    from_security.sort();
    assert_eq!(from_table, from_security);
}

/// REQ: SEC-293
#[test]
fn doc_sec_293_every_point_of_the_security_table_exists_in_the_core_points_table() {
    let known: std::collections::BTreeSet<String> = core_points()
        .into_iter()
        .map(|(point, _, _)| point)
        .collect();
    for point in high_points_in_security_page() {
        assert!(known.contains(&point), "{point} is not a core point");
    }
}

/// REQ: SEC-293
#[test]
fn doc_sec_293_each_core_page_repeats_the_marks_of_its_own_points() {
    for (point, _, risk) in core_points() {
        let (owner, name) = point.rsplit_once('/').unwrap_or(("", ""));
        let slug = owner.trim_start_matches("ilium.core.");
        let body = page(&format!("core-plugin-{slug}.md"));
        let row = body
            .lines()
            .find(|line| line.starts_with('|') && line.contains(&format!("`{name}`")));
        let row = row.unwrap_or("");
        assert!(
            row.contains(risk.trim()),
            "{point}: page row `{row}` lacks risk {risk}"
        );
    }
}

/// REQ: SEC-280
#[test]
fn sec_280_third_party_point_with_standard_risk_and_terminal_text_fails_point_risk() {
    let mut world = ExtWorld::new();
    let mut point = collect_point("sends", 8, false);
    point["entry_schema"] = json!({"type":"object","properties":{"text":{"type":"string","x-ilium-data":"terminal-text"}}});
    point["risk"] = json!("standard");
    let manifest = owner_manifest("com.example.risk", vec![point]);
    assert_eq!(
        diag_code(&world.install(Tier::T3, manifest, Vec::new())),
        "manifest.point-risk"
    );
}

/// REQ: SEC-280, SEC-281
#[test]
fn sec_281_each_sensitive_data_class_raises_the_point_to_high() {
    for class in ["terminal-text", "agent-prompt", "file-content", "secret"] {
        let mut world = ExtWorld::new();
        let mut point = collect_point("sens", 8, false);
        point["entry_schema"] =
            json!({"type":"object","properties":{"v":{"type":"string","x-ilium-data":class}}});
        let manifest = owner_manifest("com.example.cls", vec![point]);
        let _ = world.install(Tier::T3, manifest, Vec::new());
        assert_eq!(
            world.point_risk("com.example.cls/sens"),
            Some("high".to_owned()),
            "{class}"
        );
    }
}

/// REQ: SEC-281
#[test]
fn sec_281_non_sensitive_classes_stay_standard() {
    for class in ["path", "user-text"] {
        let mut world = ExtWorld::new();
        let mut point = collect_point("plain", 8, false);
        point["entry_schema"] =
            json!({"type":"object","properties":{"v":{"type":"string","x-ilium-data":class}}});
        let manifest = owner_manifest("com.example.cls", vec![point]);
        let _ = world.install(Tier::T3, manifest, Vec::new());
        assert_eq!(
            world.point_risk("com.example.cls/plain"),
            Some("standard".to_owned()),
            "{class}"
        );
    }
}

/// REQ: SEC-281
#[test]
fn sec_281_unknown_data_class_fails_schema() {
    let mut world = ExtWorld::new();
    let mut point = collect_point("odd", 8, false);
    point["entry_schema"] =
        json!({"type":"object","properties":{"v":{"type":"string","x-ilium-data":"bogus"}}});
    let manifest = owner_manifest("com.example.odd", vec![point]);
    let result = world.install(Tier::T3, manifest, Vec::new());
    assert!(result.is_err(), "{result:?}");
    // manual does not pin the code for an unknown data class; it must be a manifest diagnostic
    assert!(diag_code(&result).starts_with("manifest."), "{result:?}");
}

/// REQ: SEC-281
#[test]
fn sec_281_review_dialog_lists_the_classes_in_the_adds_to_other_plugins_row() {
    let mut world = ExtWorld::new();
    let mut point = collect_point("sens", 8, false);
    point["entry_schema"] =
        json!({"type":"object","properties":{"v":{"type":"string","x-ilium-data":"secret"}}});
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest("com.example.cls", vec![point]),
            Vec::new()
        )),
        "ok"
    );
    let contributor = contributor_manifest_on("com.example.con", "com.example.cls/sens", vec![]);
    let row = world.review_row("Adds to other plugins", &contributor);
    assert!(row.contains("secret"), "{row}");
}

/// REQ: SEC-282
#[test]
fn sec_282_contributor_without_the_matching_read_grant_fails_grant_dependency() {
    let mut world = ExtWorld::new();
    let mut point = collect_point("term", 8, false);
    point["entry_schema"] = json!({"type":"object","properties":{"v":{"type":"string","x-ilium-data":"terminal-text"}}});
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest("com.example.cls", vec![point]),
            Vec::new()
        )),
        "ok"
    );
    let contributor = contributor_manifest_on("com.example.con", "com.example.cls/term", vec![]);
    assert_eq!(
        diag_code(&world.install(Tier::T3, contributor, Vec::new())),
        "manifest.grant-dependency"
    );
}

/// REQ: SEC-282
#[test]
fn sec_282_each_data_class_accepts_its_own_grant() {
    for (class, grant) in [
        ("terminal-text", "terminal.read-screen"),
        ("terminal-text", "terminal.history"),
        ("agent-prompt", "agent.read-prompts"),
    ] {
        let mut world = ExtWorld::new();
        let mut point = collect_point("sens", 8, false);
        point["entry_schema"] =
            json!({"type":"object","properties":{"v":{"type":"string","x-ilium-data":class}}});
        assert_eq!(
            diag_code(&world.install(
                Tier::T3,
                owner_manifest("com.example.cls", vec![point]),
                Vec::new()
            )),
            "ok"
        );
        let mut contributor =
            contributor_manifest_on("com.example.con", "com.example.cls/sens", vec![]);
        if let Some(list) = contributor["capabilities"]["server"].as_array_mut() {
            list.push(json!(grant));
        }
        assert_eq!(
            diag_code(&world.install(Tier::T3, contributor, Vec::new())),
            "ok",
            "{class} with {grant}"
        );
    }
}

/// REQ: SEC-283
#[test]
fn sec_283_review_dialog_shows_a_can_make_line_per_privileged_entry_type() {
    let world = ExtWorld::with_core_plugins();
    let contributor = json!({
        "id": "com.example.steps",
        "capabilities": {"server": ["extensions.contribute:ilium.core.worktrees/post-create-steps", "process.run:git"]},
        "contributes": {"extensions": [ext_entry("ilium.core.worktrees/post-create-steps", "s", 50, json!({"id":"s","title":"S","program":"git","args":["status"]}))]}
    });
    let row = world.review_lines(&contributor);
    assert!(
        row.iter()
            .any(|line| line.starts_with("Can make ilium.core.worktrees")
                || line.starts_with("Can make Worktrees")),
        "{row:?}"
    );
}

/// REQ: SEC-283
#[test]
fn sec_283_owner_rejects_an_entry_outside_its_allowlist() {
    let mut world = ExtWorld::with_core_plugins();
    let contributor = json!({
        "id": "com.example.steps",
        "capabilities": {"server": ["extensions.contribute:ilium.core.worktrees/post-create-steps", "process.run:git"]},
        "contributes": {"extensions": [ext_entry("ilium.core.worktrees/post-create-steps", "s", 50, json!({"id":"s","title":"S","program":"rm","args":["-rf","/"]}))]}
    });
    let entries_before = world
        .entries_of("ilium.core.worktrees/post-create-steps")
        .len();
    // The install itself may succeed; the owner rejects the entry afterwards (SEC-283).
    let _install = world.install(Tier::T2, contributor, Vec::new());
    world.settle();
    let entries = world.entries_of("ilium.core.worktrees/post-create-steps");
    assert!(entries.iter().all(|entry| entry.id != "s"));
    assert_eq!(entries.len(), entries_before, "no entry was added");
    assert!(has_log(&world, "extension.entry-rejected"));
}

/// REQ: SEC-284
#[test]
fn sec_284_transform_changing_a_field_outside_the_reply_schema_counts_as_pass_and_logs_schema_mismatch(
) {
    let mut world = hook_world(HookShape::transform());
    run_chain(
        &mut world,
        1,
        vec![Contrib::new(
            "com.example.c1",
            50,
            script(
                1,
                HandlerAction::Replace(json!({"text":"ok","locked":"changed"})),
            ),
        )],
    );
    assert!(has_log(&world, "extension.schema-mismatch"));
}

/// REQ: SEC-284
#[test]
fn sec_284_output_failing_payload_schema_is_not_passed_to_the_next_handler() {
    let mut world = hook_world(HookShape::transform());
    run_chain(
        &mut world,
        1,
        vec![
            Contrib::new(
                "com.example.c1",
                50,
                script(1, HandlerAction::Replace(json!({"text": 5}))),
            ),
            Contrib::new("com.example.c2", 50, script(1, HandlerAction::Pass)),
        ],
    );
    assert_eq!(world.seen_payload("com.example.c2")["text"], json!("start"));
}

/// REQ: SEC-284
#[test]
fn sec_284_final_check_guard_runs_after_all_transform_handlers() {
    let mut world = hook_world(HookShape::transform().with_final_check());
    run_chain(
        &mut world,
        1,
        vec![Contrib::guard(
            "com.example.g",
            50,
            script(1, HandlerAction::Refuse("no".to_owned())),
        )],
    );
    assert!(world.guard_ran_after_transforms());
}

/// REQ: SEC-286
#[test]
fn sec_286_veto_handler_sees_the_final_payload() {
    let mut world = hook_world(HookShape::veto());
    run_chain(
        &mut world,
        1,
        vec![Contrib::new(
            "com.example.c1",
            50,
            script(1, HandlerAction::Pass),
        )],
    );
    assert_eq!(
        world.seen_payload("com.example.c1"),
        world.initial_payload()
    );
}

/// REQ: SEC-286
#[test]
fn sec_286_guard_on_a_final_check_point_sees_the_transform_output() {
    let mut world = hook_world(HookShape::transform().with_final_check());
    run_chain(
        &mut world,
        1,
        vec![
            Contrib::new(
                "com.example.t",
                50,
                script(1, HandlerAction::Replace(json!({"text":"changed"}))),
            ),
            Contrib::guard("com.example.g", 50, script(1, HandlerAction::Pass)),
        ],
    );
    assert_eq!(
        world.seen_payload("com.example.g")["text"],
        json!("changed")
    );
}

/// REQ: SEC-287
#[test]
fn sec_287_timed_out_trapping_and_erroring_handlers_count_as_pass() {
    for action in [
        HandlerAction::Trap,
        HandlerAction::ErrorReturn,
        HandlerAction::Sleep(500),
    ] {
        let mut world = hook_world(HookShape::veto());
        let outcome = run_chain(
            &mut world,
            1,
            vec![Contrib::new("com.example.c1", 50, script(1, action))],
        );
        assert!(
            !outcome.refused(),
            "a failing handler must not refuse a standard point"
        );
    }
}

/// REQ: SEC-287
#[test]
fn sec_287_on_failure_on_a_standard_point_fails_extension_point_invalid() {
    let mut world = ExtWorld::new();
    let mut point = hook_point("p", "veto");
    point["on_failure"] = json!("refuse");
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            owner_manifest("com.example.of", vec![point]),
            Vec::new()
        )),
        "manifest.extension-point-invalid"
    );
}

/// REQ: SEC-287
#[test]
fn sec_287_on_failure_is_allowed_on_high_veto_and_transform_points_only() {
    for (mode, expected) in [
        ("veto", "ok"),
        ("transform", "ok"),
        ("observe", "manifest.extension-point-invalid"),
        ("first", "manifest.extension-point-invalid"),
    ] {
        let mut world = ExtWorld::new();
        let mut point = hook_point("p", mode);
        point["risk"] = json!("high");
        point["on_failure"] = json!("pass");
        assert_eq!(
            diag_code(&world.install(
                Tier::T3,
                owner_manifest("com.example.of", vec![point]),
                Vec::new()
            )),
            expected,
            "{mode}"
        );
    }
}

/// REQ: SEC-287
#[test]
fn sec_287_a_high_veto_point_defaults_to_refuse_and_a_guard_failure_vetoes_with_guard_failed() {
    let mut world = hook_world(HookShape::veto().high());
    let outcome = run_chain(
        &mut world,
        1,
        vec![Contrib::guard(
            "com.example.g",
            50,
            script(1, HandlerAction::Trap),
        )],
    );
    assert!(outcome.refused());
    assert_eq!(outcome.reason(), Some("guard-failed".to_owned()));
}

const OVERRIDABLE: [&str; 6] = [
    "ilium.core.tree/before-close",
    "ilium.core.tree/before-move",
    "ilium.core.worktrees/before-remove",
    "ilium.core.layout/before-split",
    "ilium.core.compaction/before-compact",
    "ilium.core.terminal/before-send-input",
];

/// REQ: SEC-291
#[test]
fn sec_291_a_user_started_veto_on_the_six_listed_points_can_be_overridden() {
    let world = ExtWorld::with_core_plugins();
    for point in OVERRIDABLE {
        assert!(world.veto_overridable(point, "user"), "{point}");
    }
}

/// REQ: SEC-291
#[test]
fn sec_291_a_plugin_started_action_cannot_be_overridden_by_the_user() {
    let world = ExtWorld::with_core_plugins();
    for point in OVERRIDABLE {
        assert!(
            !world.veto_overridable(point, "plugin:com.example.x"),
            "{point}"
        );
    }
}

/// REQ: SEC-291
#[test]
fn sec_291_each_user_source_value_counts_as_user_started() {
    let world = ExtWorld::with_core_plugins();
    for source in ["user", "paste", "mouse", "keys", "manual-trigger"] {
        assert!(
            world.veto_overridable("ilium.core.tree/before-close", source),
            "{source}"
        );
    }
    assert!(!world.veto_overridable("ilium.core.tree/before-close", "scheduler"));
}

/// REQ: SEC-291
#[test]
fn sec_291_other_points_have_no_user_override() {
    let world = ExtWorld::with_core_plugins();
    assert!(!world.veto_overridable("ilium.core.fleet/before-broadcast", "user"));
}

/// REQ: SEC-292
#[test]
fn sec_292_reason_of_120_characters_is_kept_and_121_is_cut_to_120() {
    let mut world = hook_world(HookShape::veto());
    for (length, expected) in [(119usize, 119usize), (120, 120), (121, 120)] {
        let outcome = run_chain(
            &mut world,
            1,
            vec![Contrib::new(
                "com.example.c1",
                50,
                script(1, HandlerAction::Refuse("r".repeat(length))),
            )],
        );
        assert_eq!(
            outcome.reason().map(|text| text.chars().count()),
            Some(expected),
            "length {length}"
        );
    }
}

/// REQ: SEC-292
#[test]
fn sec_292_control_characters_and_bidi_overrides_are_removed_from_reasons() {
    let mut world = hook_world(HookShape::veto());
    let outcome = run_chain(
        &mut world,
        1,
        vec![Contrib::new(
            "com.example.c1",
            50,
            script(1, HandlerAction::Refuse("a\u{0007}b\u{202e}c\n".to_owned())),
        )],
    );
    assert_eq!(outcome.reason(), Some("abc".to_owned()));
}

/// REQ: SEC-292
#[test]
fn sec_292_ui_text_is_prefixed_with_the_contributors_display_name() {
    let mut world = hook_world(HookShape::veto());
    let outcome = run_chain(
        &mut world,
        1,
        vec![Contrib::new(
            "com.example.c1",
            50,
            script(1, HandlerAction::Refuse("blocked".to_owned())),
        )],
    );
    let text = outcome.ui_text();
    // manual does not pin the display-name format or separator; the manifest declares no display
    // name, so the prefix is the plugin id
    assert!(text.starts_with("com.example.c1"), "{text}");
    assert!(text.ends_with("blocked"), "{text}");
    assert!(text.len() > "blocked".len(), "prefix is non-empty: {text}");
}

proptest! {
    // REQ: SEC-292
    #[test]
    fn sec_292_reason_text_never_exceeds_120_characters_or_contains_controls(raw in "\\PC{0,300}") {
        let mut world = hook_world(HookShape::veto());
        let expected: String = raw.chars().filter(|c| !c.is_control()).take(120).collect();
        let outcome = run_chain(&mut world, 1, vec![Contrib::new("com.example.c1", 50, script(1, HandlerAction::Refuse(raw)))]);
        prop_assert!(outcome.refused());
        let text = outcome.reason();
        prop_assert!(text.is_some(), "a refusal always carries a reason");
        let text = text.unwrap_or_default();
        prop_assert!(text.chars().count() <= 120);
        prop_assert!(text.chars().all(|c| !c.is_control()));
        prop_assert_eq!(text, expected);
    }
}

/// REQ: SEC-310
#[test]
fn sec_310_untrusted_project_asks_once_and_stores_the_answer_per_canonical_path() {
    let mut world = ExtWorld::new();
    world.open_project("/p/one", true);
    assert_eq!(
        world.trust_prompts(),
        vec![
            "Trust this project? It contains plugin settings that could change how plugins behave."
                .to_owned()
        ]
    );
    world.answer_trust(true);
    world.open_project("/p/one/../one", true);
    assert_eq!(world.trust_prompts().len(), 1, "asked once");
    assert!(world.project_trusted("/p/one"));
}

/// REQ: SEC-310
#[test]
fn sec_310_trust_can_be_revoked_in_settings() {
    let mut world = ExtWorld::new();
    world.open_project("/p/one", true);
    world.answer_trust(true);
    world.revoke_trust("/p/one");
    assert!(!world.project_trusted("/p/one"));
}

/// REQ: SEC-311
#[test]
fn sec_311_untrusted_project_values_are_ignored_and_get_reports_default_or_user() {
    let mut world = ExtWorld::new();
    world.open_project("/p/two", true);
    world.answer_trust(false);
    world.set_project_value("com.example.cfg", "mode", json!("project"));
    let value = world.get_setting("com.example.cfg", "mode");
    assert!(
        value.source == "default" || value.source == "user",
        "{}",
        value.source
    );
    assert!(world.ignored_project_values_notice().is_some());
}

/// REQ: SEC-312
#[test]
fn sec_312_trust_sensitive_rows_show_the_first_use_of_each_project_value() {
    let mut world = ExtWorld::new();
    world.open_project("/p/three", true);
    world.answer_trust(true);
    world.set_project_value_of_type(
        "com.example.cfg",
        "endpoint",
        "url",
        json!("https://example.org"),
    );
    assert!(world
        .first_use_prompt("com.example.cfg", "endpoint")
        .is_some());
    assert_eq!(
        world.get_setting("com.example.cfg", "endpoint").source,
        "user"
    );
    world.answer_first_use(true);
    assert_eq!(
        world.get_setting("com.example.cfg", "endpoint").source,
        "project"
    );
}

/// REQ: SEC-312
#[test]
fn sec_312_each_row_type_path_url_host_and_command_needs_the_extra_step() {
    for kind in ["path", "url", "host", "command"] {
        let mut world = ExtWorld::new();
        world.open_project("/p/four", true);
        world.answer_trust(true);
        world.set_project_value_of_type("com.example.cfg", "row", kind, json!("x"));
        assert!(
            world.first_use_prompt("com.example.cfg", "row").is_some(),
            "{kind}"
        );
    }
}

/// REQ: SEC-313
#[test]
fn sec_313_secret_rows_never_take_project_values() {
    let mut world = ExtWorld::new();
    world.open_project("/p/five", true);
    world.answer_trust(true);
    world.set_project_value_of_type("com.example.cfg", "token", "secret", json!("hunter2"));
    assert_ne!(
        world.get_setting("com.example.cfg", "token").source,
        "project"
    );
}

/// REQ: SEC-314
#[test]
fn sec_314_policy_allow_project_values_false_ignores_values_even_when_trusted() {
    let mut world = ExtWorld::new();
    world.set_policy("[projects]\nallow_project_values = false\n");
    world.open_project("/p/six", true);
    world.answer_trust(true);
    world.set_project_value("com.example.cfg", "mode", json!("project"));
    assert_ne!(
        world.get_setting("com.example.cfg", "mode").source,
        "project"
    );
}

/// REQ: SEC-315
#[test]
fn sec_315_project_trusted_is_false_until_the_user_trusts_the_folder() {
    let mut world = ExtWorld::new();
    world.open_project("/p/seven", true);
    assert!(!world.sdk_project_trusted("com.example.reader"));
    world.answer_trust(true);
    assert!(world.sdk_project_trusted("com.example.reader"));
}

/// REQ: SEC-315
#[test]
fn doc_sec_315_page_states_64_kib_200_rules_and_the_regex_limits() {
    let text = page("reference-extension-security.md");
    let line = text
        .lines()
        .find(|line| line.contains("[SEC-315]"))
        .unwrap_or("");
    for needle in ["64 KiB", "200 rules", "SEC-046"] {
        assert!(line.contains(needle), "{needle}");
    }
}
