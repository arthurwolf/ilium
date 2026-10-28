//! Core-plugin gap tests for `reference-kernel-and-boot`: startup deadline, core plugin recovery,
//! audit records, the single failure-window value, and the dependency-cycle refusal.

use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::Side;
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const SPY: &str = "com.example.startup-spy";
const DEPENDENT: &str = "com.example.startup-dependent";
const CORE: &str = "ilium.core.sessions";
const OTHER_CORE: &str = "ilium.core.tree";

fn manual_page(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The text between a `## <heading>` line and the next level-2 heading.
fn section_of<'a>(page: &'a str, heading: &str) -> &'a str {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("section {heading:?} is missing"));
    let rest = &page[start..];
    let end = rest[heading.len()..]
        .find("\n## ")
        .map(|offset| heading.len() + offset)
        .unwrap_or(rest.len());
    &rest[..end]
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

/// REQ: COR-091
#[test]
fn cor_091_a_startup_plugin_past_its_deadline_fails_with_activation_timeout() {
    let rig = OpsRig::new();
    let spy = PackageSpec::new(SPY, "1.0.0")
        .activate_misses_deadline()
        .startup();
    install_ok(&rig, &spy);
    install_ok(
        &rig,
        &PackageSpec::new("com.example.startup-sibling", "1.0.0").startup(),
    );
    let server = rig.start_server("s1");
    rig.advance(Duration::from_secs(61));
    assert_eq!(
        server.state_of(SPY, Side::Server),
        InstanceState::Failed,
        "the spy must be failed after its deadline"
    );
    assert_eq!(
        server.inactive_reason(SPY).as_deref(),
        Some("activation-timeout"),
        "the failure reason must be activation-timeout"
    );
    assert!(
        server
            .activations("com.example.startup-sibling")
            .contains(&"activated".to_string()),
        "the other startup plugin must still be activated"
    );
}

/// REQ: COR-092
#[test]
fn cor_092_a_timed_out_startup_plugin_keeps_the_socket_and_blocks_its_dependent() {
    let rig = OpsRig::new();
    install_ok(
        &rig,
        &PackageSpec::new(SPY, "1.0.0")
            .activate_misses_deadline()
            .startup(),
    );
    install_ok(
        &rig,
        &PackageSpec::new(DEPENDENT, "1.0.0").requires_service(SPY),
    );
    let server = rig.start_server("s1");
    rig.advance(Duration::from_secs(61));
    let client = server.attach_client("c1");
    assert!(
        server.socket_answers(),
        "the client socket must answer after boot"
    );
    assert_eq!(server.plugin_status(DEPENDENT), "blocked");
    assert_eq!(server.plugin_status(SPY), "failed");
    assert!(
        server.render_plugins_screen().row_for(DEPENDENT).is_some(),
        "the dependent must be listed in the plugin list"
    );
    assert!(client.recovery_page_text().contains(SPY));
}

/// REQ: COR-107
#[test]
fn cor_107_a_fourth_core_failure_inside_sixty_seconds_is_refused_and_degraded() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    for _ in 0..4 {
        server.fail(CORE, Side::Server);
        rig.advance(Duration::from_secs(5));
    }
    assert_eq!(server.plugin_status(CORE), "degraded");
    assert_eq!(server.failure_count(CORE, Side::Server), 3);
}

/// REQ: COR-108
#[test]
fn cor_108_a_degraded_core_plugin_refuses_its_commands_and_others_keep_serving() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    for _ in 0..3 {
        server.fail(CORE, Side::Server);
    }
    assert_eq!(server.plugin_status(CORE), "degraded");
    assert!(
        server.call_plugin(OTHER_CORE, "serve").is_ok(),
        "the other core plugin must keep serving"
    );
    let refusal = server
        .call_plugin(CORE, "list")
        .expect_err("a degraded core plugin must refuse commands");
    assert!(
        format!("{refusal:?}").contains("core.degraded"),
        "refusal must be core.degraded, got {refusal:?}"
    );
    assert!(server.render_status_surface().contains("degraded"));
}

/// REQ: COR-110
#[test]
fn cor_110_each_core_failure_writes_one_audit_record_with_time_reason_and_restart_count() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.fail(CORE, Side::Server);
    server.fail(CORE, Side::Server);
    let records = server.audit_records(CORE);
    assert_eq!(records.len(), 2, "one audit record per failure");
    for record in records {
        assert!(record.get("time").is_some(), "record lacks time: {record}");
        assert!(
            record.get("exit_reason").is_some(),
            "record lacks exit_reason: {record}"
        );
        assert!(
            record.get("restart_count").is_some(),
            "record lacks restart_count: {record}"
        );
    }
}

/// REQ: COR-200
#[test]
fn cor_200_the_plugin_off_section_cites_the_sixty_second_rule_and_not_ten_minutes() {
    let page = manual_page("reference-kernel-and-boot");
    let section = section_of(&page, "What the user keeps when a plugin is off");
    assert!(
        !section.contains("10 minutes"),
        "the section still states the ten-minute window"
    );
    assert!(
        section.contains("COR-107"),
        "the section must cite COR-107 for the failure window"
    );
}

/// REQ: COR-207
#[test]
fn cor_207_a_bundled_core_dependency_cycle_refuses_boot_and_names_the_path() {
    let rig = OpsRig::new();
    let first =
        PackageSpec::new("ilium.core.cycle-a", "1.0.0").requires_service("ilium.core.cycle-b");
    let second =
        PackageSpec::new("ilium.core.cycle-b", "1.0.0").requires_service("ilium.core.cycle-a");
    let error = rig
        .boot_with_bundled_cores(&[first, second])
        .expect_err("boot must refuse a core dependency cycle");
    let text = error.to_string();
    assert!(
        text.contains("core.dependency-cycle"),
        "error code missing: {text}"
    );
    assert!(
        text.contains("ilium.core.cycle-a") && text.contains("ilium.core.cycle-b"),
        "the cycle path must be named: {text}"
    );
}

/// REQ: COR-091
#[test]
fn cor_091_a_startup_plugin_within_its_deadline_is_not_failed() {
    let rig = OpsRig::new();
    let quick = "com.example.startup-quick";
    install_ok(&rig, &PackageSpec::new(quick, "1.0.0").startup());
    let server = rig.start_server("s1");
    rig.advance(Duration::from_secs(61));
    assert_eq!(
        server.state_of(quick, Side::Server),
        InstanceState::Active,
        "a startup plugin that meets its deadline must stay active"
    );
    assert!(
        server.inactive_reason(quick).is_none(),
        "no activation-timeout may be recorded for it"
    );
}

/// REQ: COR-092
#[test]
fn cor_092_a_dependent_of_a_timed_out_startup_plugin_is_not_activated() {
    let rig = OpsRig::new();
    install_ok(
        &rig,
        &PackageSpec::new(SPY, "1.0.0")
            .activate_misses_deadline()
            .startup(),
    );
    install_ok(
        &rig,
        &PackageSpec::new(DEPENDENT, "1.0.0").requires_service(SPY),
    );
    let server = rig.start_server("s1");
    rig.advance(Duration::from_secs(61));
    assert_ne!(
        server.state_of(DEPENDENT, Side::Server),
        InstanceState::Active,
        "the dependent must not be activated while its service is failed"
    );
}

/// REQ: COR-107
#[test]
fn cor_107_three_core_failures_spread_over_more_than_sixty_seconds_do_not_degrade_it() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    for _ in 0..3 {
        server.fail(CORE, Side::Server);
        rig.advance(Duration::from_secs(61));
    }
    assert_ne!(
        server.plugin_status(CORE),
        "degraded",
        "failures outside one 60-second window must not degrade the plugin"
    );
}

/// REQ: COR-108
#[test]
fn cor_108_a_degraded_core_plugin_does_not_degrade_its_sibling_core_plugin() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    for _ in 0..3 {
        server.fail(CORE, Side::Server);
    }
    assert_eq!(server.plugin_status(CORE), "degraded");
    assert_ne!(
        server.plugin_status(OTHER_CORE),
        "degraded",
        "the kernel must keep the other core plugins running"
    );
}

/// REQ: COR-110
#[test]
fn cor_110_audit_records_of_repeated_failures_carry_an_increasing_restart_count() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.fail(CORE, Side::Server);
    server.fail(CORE, Side::Server);
    let records = server.audit_records(CORE);
    let restart_counts: Vec<Option<u64>> = records
        .iter()
        .map(|record| record.get("restart_count").and_then(|value| value.as_u64()))
        .collect();
    assert_eq!(restart_counts.len(), 2, "one audit record per failure");
    assert!(
        restart_counts.windows(2).all(|pair| pair[0] < pair[1]),
        "each later record must show a higher restart count: {restart_counts:?}"
    );
}

/// REQ: COR-207
#[test]
fn cor_207_an_acyclic_bundled_core_dependency_chain_boots() {
    let rig = OpsRig::new();
    let first =
        PackageSpec::new("ilium.core.chain-a", "1.0.0").requires_service("ilium.core.chain-b");
    let second = PackageSpec::new("ilium.core.chain-b", "1.0.0");
    rig.boot_with_bundled_cores(&[first, second])
        .expect("a bundle without a cycle must boot");
}
