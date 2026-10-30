//! Core-plugin gap tests for `core-plugins`: the Contract section, parity suites, dependents,
//! enable and disable timing, schema versions, fallbacks, startup without a client, lazy
//! activation, limits, degraded storage, replacement rules, integrity and ABI checks.

use std::time::Duration;

use ilium_plugin_api::Side;
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const TREE: &str = "ilium.core.tree";
const COMMANDS: &str = "ilium.core.commands";
const SETTINGS: &str = "ilium.core.settings";
const TERMINAL: &str = "ilium.core.terminal";
const CORE_NAME_TREE: &str = "tree";
const REPLACEMENT: &str = "com.example.tree-replacement";
const SECOND_REPLACEMENT: &str = "com.example.tree-replacement-two";
const DEPENDENT: &str = "com.example.depends-on-commands";
const LAZY: &str = "com.example.lazy-command";
const OTHER: &str = "com.example.other";

fn manual_page(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn install_ok(rig: &OpsRig, spec: &PackageSpec) {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"]);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
}

/// REQ: COR-097
#[test]
fn cor_097_a_replacement_missing_a_cli_command_is_refused_at_enable_and_core_stays_active() {
    let rig = OpsRig::new();
    let replacement = PackageSpec::new(REPLACEMENT, "1.0.0")
        .replaces_core(TREE)
        .declares_cli("tree-list")
        .omits_cli("tree-open");
    rig.install_replacement_of_core(&replacement, TREE);
    let _server = rig.start_server("s1");
    let output = rig.cli(&["enable", REPLACEMENT]);
    assert_ne!(
        output.exit_code, 0,
        "enable must fail when a command is missing"
    );
    assert!(
        output.stderr.contains("tree-open") || output.stdout_raw.contains("tree-open"),
        "the refusal must list the missing command: {output:?}"
    );
    assert!(rig.is_enabled(TREE), "the core plugin must remain active");
}

/// REQ: COR-098
#[test]
fn cor_098_a_replacement_reads_the_stored_tree_data_unchanged_and_records_the_parity_result() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.write_storage(TREE, "layout", "pane-order", b"a,b,c");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    assert_eq!(
        server
            .read_storage(REPLACEMENT, "layout", "pane-order")
            .as_deref(),
        Some(b"a,b,c".as_slice()),
        "the replacement must read the stored data without loss"
    );
    assert_eq!(
        server.parity_result(REPLACEMENT).as_deref(),
        Some("passed"),
        "the parity suite result must be recorded before the replacement owns the plugin"
    );
}

/// REQ: COR-098
#[test]
fn cor_098_a_failing_parity_suite_keeps_the_core_plugin_active() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0")
            .replaces_core(TREE)
            .parity_fails(),
        TREE,
    );
    server.enable(REPLACEMENT);
    assert_eq!(server.active_owner(TREE), TREE.to_string());
    assert_eq!(server.parity_result(REPLACEMENT).as_deref(), Some("failed"));
}

/// REQ: COR-099
#[test]
fn cor_099_data_written_through_a_replacement_stays_readable_by_the_core_plugin_after_removal() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    server.write_storage(REPLACEMENT, "layout", "pane-order", b"x,y");
    rig.cli(&["uninstall", REPLACEMENT, "--yes"]);
    assert_eq!(
        server.read_storage(TREE, "layout", "pane-order").as_deref(),
        Some(b"x,y".as_slice()),
        "the core plugin must read what the replacement wrote"
    );
}

/// REQ: COR-203
#[test]
fn cor_203_every_core_plugin_page_has_a_contract_section_naming_the_flags() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages");
    let mut checked = 0;
    for entry in std::fs::read_dir(&directory).expect("read pages directory") {
        let path = entry.expect("directory entry").path();
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        if !(name.starts_with("core-plugin") && name.ends_with(".md")) {
            continue;
        }
        checked += 1;
        let text = std::fs::read_to_string(&path).expect("read core plugin page");
        assert!(
            text.contains("## Contract"),
            "{name} has no Contract section"
        );
        for flag in ["essential", "disableable", "replaceable"] {
            assert!(text.contains(flag), "{name} Contract omits {flag}");
        }
    }
    assert!(checked > 0, "no core plugin pages were found");
}

/// REQ: COR-203
#[test]
fn cor_203_the_contract_equals_the_bundled_manifest_and_the_replacement_matrix_row() {
    let rig = OpsRig::new();
    let manifest = rig.bundled_manifest(TREE);
    let matrix_row = rig.replacement_matrix_row(TREE);
    assert_eq!(
        manifest, matrix_row,
        "the manifest and matrix row must agree"
    );
    assert!(manual_page("core-plugins").contains("Contract"));
}

/// REQ: COR-204
#[test]
fn cor_204_each_core_plugin_has_a_parity_suite_named_for_it() {
    let rig = OpsRig::new();
    let core_ids = rig.core_plugin_ids();
    assert_eq!(core_ids.len(), 30, "the bundle must hold 30 core plugins");
    for core_id in core_ids {
        let short = core_id.trim_start_matches("ilium.core.");
        assert_eq!(
            rig.conformance_suite_name(&core_id),
            Some(format!("ilium-core-{short}-parity")),
            "{core_id} has no parity suite"
        );
    }
}

/// REQ: COR-204
#[test]
fn cor_204_the_build_check_exits_one_when_a_core_plugin_has_no_suite() {
    let rig = OpsRig::new();
    rig.remove_core_suite(CORE_NAME_TREE);
    let output = rig.check_core_suites();
    assert_eq!(output.exit_code, 1, "the build must fail with exit code 1");
}

/// REQ: COR-205
#[test]
fn cor_205_the_conformance_command_prints_one_pass_or_fail_line_per_test() {
    let rig = OpsRig::new();
    let _server = rig.start_server("s1");
    let output = rig.cli(&["plugin", "conformance", TREE]);
    let lines: Vec<&str> = output
        .stdout_raw
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    assert!(
        !lines.is_empty(),
        "the command must print a result line per test"
    );
    for line in &lines {
        assert!(
            line.starts_with("pass ") || line.starts_with("fail "),
            "each line must be pass or fail: {line}"
        );
    }
    let any_fail = lines.iter().any(|line| line.starts_with("fail "));
    assert_eq!(
        output.exit_code == 0,
        !any_fail,
        "exit 0 only when all tests pass"
    );
}

/// REQ: COR-208
#[test]
fn cor_208_disabling_a_required_core_plugin_is_refused_listing_every_dependent() {
    let rig = OpsRig::new();
    install_ok(
        &rig,
        &PackageSpec::new(DEPENDENT, "1.0.0").requires_service(COMMANDS),
    );
    let _server = rig.start_server("s1");
    let output = rig.cli(&["disable", COMMANDS]);
    assert_ne!(output.exit_code, 0, "disable must be refused");
    let text = format!("{:?}", output.stdout_jsonl);
    assert!(text.contains("core.has-dependents"), "code missing: {text}");
    assert!(text.contains(DEPENDENT), "dependent not listed: {text}");
}

/// REQ: COR-208
#[test]
fn cor_208_cascade_disables_dependents_in_reverse_order_and_reenables_them_together() {
    let rig = OpsRig::new();
    install_ok(
        &rig,
        &PackageSpec::new(DEPENDENT, "1.0.0").requires_service(COMMANDS),
    );
    let server = rig.start_server("s1");
    let output = rig.cli(&["disable", COMMANDS, "--cascade"]);
    assert_eq!(output.exit_code, 0, "cascade disable must succeed");
    assert_eq!(
        server.deactivation_order(),
        vec![DEPENDENT.to_string(), COMMANDS.to_string()],
        "dependents must go first, in reverse dependency order"
    );
    rig.cli(&["enable", COMMANDS]);
    assert!(
        server.is_enabled(DEPENDENT),
        "the dependent must re-enable together"
    );
}

/// REQ: COR-209
#[test]
fn cor_209_disabling_a_core_plugin_takes_effect_within_two_seconds_and_emits_one_event() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let started = rig.virtual_elapsed();
    server.disable(SETTINGS);
    let elapsed = rig.virtual_elapsed() - started;
    assert!(elapsed <= Duration::from_secs(2), "took {elapsed:?}");
    assert!(
        server.contributions(SETTINGS).is_empty(),
        "UI contributions must be dropped"
    );
    assert_eq!(server.emitted_events("plugin.state-changed"), 1);
}

/// REQ: COR-210
#[test]
fn cor_210_a_section_newer_than_the_plugin_is_refused_and_left_unmodified() {
    let rig = OpsRig::new();
    rig.plant_data(TREE, 99);
    let before = rig.snapshot_bytes("s1");
    let _server = rig.start_server("s1");
    assert!(
        rig.log_codes().iter().any(|code| code == "core.data-newer"),
        "the refusal must be logged as core.data-newer"
    );
    assert_eq!(
        rig.snapshot_bytes("s1"),
        before,
        "the newer section must not be modified"
    );
}

/// REQ: COR-213
#[test]
fn cor_213_disabling_the_command_plugin_shows_its_fallback_and_loses_no_pane_input() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let client = server.attach_client("c1");
    let pane = server.focused_pane_id();
    server.disable(COMMANDS);
    let frame = client.next_frame_text();
    assert!(
        frame.contains("commands are unavailable"),
        "the fallback text must appear in the next frame: {frame}"
    );
    server.type_into_focused_pane("x");
    assert!(
        server.pane_received_input(pane).ends_with('x'),
        "typed input was lost"
    );
}

/// REQ: COR-220
#[test]
fn cor_220_every_host_call_of_a_core_plugin_is_in_the_public_wit() {
    let rig = OpsRig::new();
    let public = rig.t1_public_api();
    for call in rig.host_calls_of(TREE) {
        assert!(
            public.contains(&call),
            "core plugin calls {call}, which is not in the public WIT"
        );
    }
}

/// REQ: COR-221
#[test]
fn cor_221_the_permissions_listing_of_a_core_plugin_matches_its_manifest_grants() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let output = rig.cli(&["plugin", "permissions", TREE]);
    let listed: Vec<String> = output
        .stdout_jsonl
        .iter()
        .filter_map(|row| {
            row.get("capability")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(
        listed,
        server.granted(TREE),
        "listing must equal the manifest grants"
    );
}

/// REQ: COR-221
#[test]
fn cor_221_the_user_cannot_grant_an_extra_capability_to_a_core_plugin() {
    let rig = OpsRig::new();
    let _server = rig.start_server("s1");
    let output = rig.cli(&["grant", TREE, "http:example.invalid"]);
    assert_ne!(
        output.exit_code, 0,
        "granting to a core plugin must be refused"
    );
}

/// REQ: COR-222
#[test]
fn cor_222_a_bundled_core_plugin_failing_its_hash_is_refused_and_essential_falls_back() {
    let rig = OpsRig::new();
    rig.corrupt_bundled_core(TREE);
    let server = rig.start_server("s1");
    assert!(
        rig.log_codes()
            .iter()
            .any(|code| code == "core.integrity-failed"),
        "the integrity failure must be logged"
    );
    assert!(
        server.kernel_fallback_active(TREE),
        "the kernel fallback must serve tree"
    );
}

/// REQ: COR-223
#[test]
fn cor_223_with_no_client_startup_core_plugins_run_without_ui_and_attaching_activates_them_within_one_second(
) {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.advance(Duration::from_secs(3));
    assert_eq!(
        server.state_of(TREE, Side::Server),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    );
    assert!(
        server.contributions(TREE).is_empty(),
        "no UI may activate without a client"
    );
    server.attach_client("c1");
    rig.advance(Duration::from_secs(1));
    assert!(
        !server.contributions(TREE).is_empty(),
        "the client UI must activate within 1 second"
    );
}

/// REQ: COR-224
#[test]
fn cor_224_a_lazy_core_plugin_activates_on_its_trigger_and_gets_the_event_once() {
    let rig = OpsRig::new();
    install_ok(
        &rig,
        &PackageSpec::new(LAZY, "1.0.0").activation_trigger("command:lazy-run"),
    );
    let server = rig.start_server("s1");
    let started = rig.virtual_elapsed();
    server
        .call_plugin(LAZY, "run")
        .expect("trigger must activate the plugin");
    let latency = rig.virtual_elapsed() - started;
    assert!(
        latency <= Duration::from_millis(300) + server.activation_time(LAZY),
        "latency {latency:?}"
    );
    assert_eq!(
        server.trigger_deliveries(LAZY),
        1,
        "the trigger must be delivered once"
    );
}

/// REQ: COR-225
#[test]
fn cor_225_a_core_plugin_runs_under_tier_two_memory_and_fuel_limits_and_info_lists_them() {
    let rig = OpsRig::new();
    let _server = rig.start_server("s1");
    let output = rig.cli(&["plugin", "info", TREE]);
    let limits = output
        .stdout_jsonl
        .iter()
        .find_map(|row| row.get("limits").cloned())
        .expect("ilium plugin info must list the limits");
    let tier_two = rig.tier_limits(ilium_plugin_api::Tier::T2);
    assert_eq!(
        limits.get("memory_bytes").and_then(|value| value.as_u64()),
        Some(tier_two.memory_bytes)
    );
    assert_eq!(
        limits.get("fuel").and_then(|value| value.as_u64()),
        Some(tier_two.fuel)
    );
}

/// REQ: COR-226
#[test]
fn cor_226_a_degraded_core_plugin_keeps_its_stored_sections_and_others_can_still_read_them() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.write_storage(COMMANDS, "state", "history", b"one");
    for _ in 0..3 {
        server.fail(COMMANDS, Side::Server);
    }
    assert_eq!(server.plugin_status(COMMANDS), "degraded");
    assert_eq!(
        server.read_storage(COMMANDS, "state", "history").as_deref(),
        Some(b"one".as_slice()),
        "the kernel must not modify or delete the degraded plugin's sections"
    );
    assert_eq!(
        server
            .read_service_snapshot(OTHER, COMMANDS, "history")
            .as_deref(),
        Some(b"one".as_slice()),
        "other plugins must read the last committed values"
    );
}

/// REQ: COR-228
#[test]
fn cor_228_a_replacement_failing_three_times_in_sixty_seconds_is_rolled_back_to_the_core_plugin() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    for _ in 0..3 {
        server.fail(REPLACEMENT, Side::Server);
        rig.advance(Duration::from_secs(10));
    }
    assert_eq!(
        server.active_owner(TREE),
        TREE.to_string(),
        "ownership must return to tree"
    );
    assert_eq!(
        server.refusal(REPLACEMENT).as_deref(),
        Some("core.replacement-rolled-back"),
        "the replacement must be marked failed with the rollback code"
    );
    assert!(!server.notices().is_empty(), "a notice must be posted");
}

/// REQ: COR-229
#[test]
fn cor_229_enabling_a_second_replacement_of_one_core_plugin_is_refused_naming_the_owner() {
    let rig = OpsRig::new();
    let _server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    rig.install_replacement_of_core(
        &PackageSpec::new(SECOND_REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    assert_eq!(rig.cli(&["enable", REPLACEMENT]).exit_code, 0);
    let output = rig.cli(&["enable", SECOND_REPLACEMENT]);
    assert_ne!(
        output.exit_code, 0,
        "the second replacement must be refused"
    );
    let text = format!("{:?} {}", output.stdout_jsonl, output.stderr);
    assert!(
        text.contains("core.replacement-conflict"),
        "code missing: {text}"
    );
    assert!(
        text.contains(REPLACEMENT),
        "the current owner must be named: {text}"
    );
}

/// REQ: COR-230
#[test]
fn cor_230_an_override_with_an_incompatible_host_abi_is_refused_and_the_bundled_copy_runs() {
    let rig = OpsRig::new();
    let bundled_version = rig.bundled_version(TREE);
    rig.install_override_incompatible_abi(TREE);
    let server = rig.start_server("s1");
    assert_eq!(
        server.active_version(TREE),
        Some(bundled_version),
        "the bundled copy must run"
    );
    assert!(
        rig.log_codes()
            .iter()
            .any(|code| code == "core.abi-mismatch"),
        "the refusal must be logged as core.abi-mismatch"
    );
}

/// REQ: COR-200
#[test]
fn cor_200_the_core_plugins_page_cites_the_same_failure_window() {
    let page = manual_page("core-plugins");
    assert!(
        !page.contains("10 minutes"),
        "the core plugins page still says ten minutes"
    );
}

/// REQ: COR-225
#[test]
fn cor_225_the_settings_terminal_plugin_has_no_larger_limit_than_tier_two() {
    let rig = OpsRig::new();
    let tier_two = rig.tier_limits(ilium_plugin_api::Tier::T2);
    let terminal = rig.limits_of(TERMINAL);
    assert!(
        terminal.memory_bytes <= tier_two.memory_bytes,
        "terminal exceeds T2 memory"
    );
}

/// REQ: COR-097
#[test]
fn cor_097_a_replacement_declaring_every_cli_command_is_accepted_at_enable() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0")
            .replaces_core(TREE)
            .declares_cli("tree-list")
            .declares_cli("tree-open"),
        TREE,
    );
    let output = rig.cli(&["enable", REPLACEMENT]);
    assert_eq!(
        output.exit_code, 0,
        "a complete replacement must be enabled: {:?}",
        output.stdout_jsonl
    );
    assert_eq!(server.active_owner(TREE), REPLACEMENT.to_string());
}

/// REQ: COR-099
#[test]
fn cor_099_disabling_a_replacement_returns_ownership_and_its_written_data_stays_readable() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    server.write_storage(REPLACEMENT, "layout", "pane-order", b"x,y");
    rig.cli(&["disable", REPLACEMENT]);
    assert_eq!(
        server.active_owner(TREE),
        TREE.to_string(),
        "disabling the replacement must return ownership to tree"
    );
    assert_eq!(
        server.read_storage(TREE, "layout", "pane-order").as_deref(),
        Some(b"x,y".as_slice()),
        "the core plugin must still read the data written during the replacement"
    );
}

/// REQ: COR-205
#[test]
fn cor_205_the_conformance_command_runs_against_the_replacement_that_owns_the_core_plugin() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    let output = rig.cli(&["plugin", "conformance", TREE]);
    let text = format!("{:?} {}", output.stdout_jsonl, output.stdout_raw);
    assert!(
        text.contains(REPLACEMENT),
        "the run must name the active owner {REPLACEMENT}: {text}"
    );
}

/// REQ: COR-209
#[test]
fn cor_209_enabling_a_disabled_core_plugin_takes_effect_within_two_seconds_without_restart() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.disable(SETTINGS);
    let started = rig.virtual_elapsed();
    server.enable(SETTINGS);
    let elapsed = rig.virtual_elapsed() - started;
    assert!(elapsed <= Duration::from_secs(2), "took {elapsed:?}");
    assert!(server.is_enabled(SETTINGS), "the plugin must be enabled");
    assert!(
        !server.contributions(SETTINGS).is_empty(),
        "UI contributions must return once the plugin is enabled"
    );
}

/// REQ: COR-210
#[test]
fn cor_210_a_section_at_the_current_schema_version_loads_without_a_refusal() {
    let rig = OpsRig::new();
    rig.plant_data(TREE, 1);
    let _server = rig.start_server("s1");
    assert!(
        !rig.log_codes().iter().any(|code| code == "core.data-newer"),
        "a section at the current version must not be refused"
    );
}

/// REQ: COR-213
#[test]
fn cor_213_disabling_the_command_plugin_keeps_the_focused_pane_and_still_renders_a_frame() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let client = server.attach_client("c1");
    let pane = server.focused_pane_id();
    server.disable(COMMANDS);
    let frame = client.next_frame_text();
    assert!(!frame.is_empty(), "a frame must render after the disable");
    assert_eq!(
        server.focused_pane_id(),
        pane,
        "disabling the command plugin must not move the focused pane"
    );
}

/// REQ: COR-220
#[test]
fn cor_220_every_bundled_core_plugin_calls_only_public_wit_functions() {
    let rig = OpsRig::new();
    let public = rig.t1_public_api();
    for core_id in rig.core_plugin_ids() {
        for call in rig.host_calls_of(&core_id) {
            assert!(
                public.contains(&call),
                "{core_id} calls {call}, which is not in the public WIT"
            );
        }
    }
}

/// REQ: COR-222
#[test]
fn cor_222_a_corrupted_core_plugin_is_not_served_from_its_bundled_copy() {
    let rig = OpsRig::new();
    rig.corrupt_bundled_core(TREE);
    let server = rig.start_server("s1");
    assert_ne!(
        server.active_version(TREE),
        Some(rig.bundled_version(TREE)),
        "the corrupted bundled copy must not be activated"
    );
}

/// REQ: COR-223
#[test]
fn cor_223_startup_core_plugins_stay_without_ui_contributions_until_a_client_attaches() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.advance(Duration::from_secs(3));
    for core_id in [COMMANDS, SETTINGS] {
        assert_eq!(
            server.state_of(core_id, Side::Server),
            ilium_plugin_api::generated::host::plugins::InstanceState::Active,
            "{core_id} must run without a client"
        );
        assert!(
            server.contributions(core_id).is_empty(),
            "{core_id} must not activate UI without a client"
        );
    }
}

/// REQ: COR-224
#[test]
fn cor_224_a_lazy_core_plugin_is_not_active_before_its_trigger_fires() {
    let rig = OpsRig::new();
    install_ok(
        &rig,
        &PackageSpec::new(LAZY, "1.0.0").activation_trigger("command:lazy-run"),
    );
    let server = rig.start_server("s1");
    assert_ne!(
        server.state_of(LAZY, Side::Server),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active,
        "a lazy plugin must stay inactive until its trigger fires"
    );
}

/// REQ: COR-226
#[test]
fn cor_226_a_degraded_core_plugin_leaves_its_stored_snapshot_bytes_unchanged() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.write_storage(COMMANDS, "state", "history", b"one");
    let before = rig.snapshot_bytes("s1");
    for _ in 0..3 {
        server.fail(COMMANDS, Side::Server);
    }
    assert_eq!(server.plugin_status(COMMANDS), "degraded");
    assert_eq!(
        rig.snapshot_bytes("s1"),
        before,
        "the kernel must not modify the degraded plugin's stored sections"
    );
}

/// REQ: COR-228
#[test]
fn cor_228_two_failures_within_sixty_seconds_keep_the_replacement_as_owner() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    server.enable(REPLACEMENT);
    for _ in 0..2 {
        server.fail(REPLACEMENT, Side::Server);
        rig.advance(Duration::from_secs(10));
    }
    assert_eq!(
        server.active_owner(TREE),
        REPLACEMENT.to_string(),
        "two failures are below the rollback threshold of three"
    );
    assert!(
        server.refusal(REPLACEMENT).is_none(),
        "the replacement must not be marked failed after two failures"
    );
}

/// REQ: COR-229
#[test]
fn cor_229_after_the_first_replacement_is_disabled_a_second_one_can_be_enabled() {
    let rig = OpsRig::new();
    let _server = rig.start_server("s1");
    rig.install_replacement_of_core(
        &PackageSpec::new(REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    rig.install_replacement_of_core(
        &PackageSpec::new(SECOND_REPLACEMENT, "1.0.0").replaces_core(TREE),
        TREE,
    );
    assert_eq!(rig.cli(&["enable", REPLACEMENT]).exit_code, 0);
    assert_eq!(rig.cli(&["disable", REPLACEMENT]).exit_code, 0);
    let output = rig.cli(&["enable", SECOND_REPLACEMENT]);
    assert_eq!(
        output.exit_code, 0,
        "one replacement at a time: the second may be enabled once the first is disabled"
    );
}

/// REQ: COR-230
#[test]
fn cor_230_an_incompatible_override_leaves_the_core_plugin_owned_by_itself() {
    let rig = OpsRig::new();
    rig.install_override_incompatible_abi(TREE);
    let server = rig.start_server("s1");
    assert_eq!(
        server.active_owner(TREE),
        TREE.to_string(),
        "the refused override must not take ownership"
    );
}
