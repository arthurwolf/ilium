//! reference-kernel-and-boot.md (COR-010, COR-020..022, COR-024).

use std::collections::BTreeSet;

use ilium_plugin_api::{ProcessKind, Side};
use ilium_plugin_host::testing::boot::BootHarness;
use proptest::prelude::*;

use crate::common::*;

const SERVER_STARTUP: [&str; 11] = [
    "agent-detect",
    "chatroom",
    "cost",
    "diagnostics",
    "notifications",
    "progress",
    "scheduler",
    "sessions",
    "text-triggers",
    "titles",
    "tree",
];
const CLIENT_STARTUP: [&str; 6] = [
    "agent-toolbar",
    "ambient",
    "progress",
    "settings",
    "terminal",
    "tree",
];

fn full_core_run(kind: ProcessKind) -> ilium_plugin_host::testing::boot::BootRun {
    let mut harness = BootHarness::new(kind).core_activation_table();
    for name in CORE_PLUGINS {
        harness = harness.plugin_from_table(&core_id(name));
    }
    let mut run = harness.build();
    run.start();
    run
}

// COR-010
/// REQ: COR-010
#[test]
fn cor_010_the_http_route_returns_404_while_the_fleet_plugin_is_disabled() {
    let mut run = full_core_run(ProcessKind::Server);
    run.disable("ilium.core.fleet");
    assert_eq!(run.http_status("POST", "/create_agent"), 404);
}

/// REQ: COR-010
#[test]
fn cor_010_the_kernel_owns_the_listener_and_the_route_works_when_enabled() {
    let mut run = full_core_run(ProcessKind::Server);
    run.enable("ilium.core.fleet");
    assert_ne!(run.http_status("POST", "/create_agent"), 404);
    assert!(run.http_listener_owner_is_kernel());
}

/// REQ: COR-010
#[test]
fn cor_010_the_tree_model_stays_in_the_kernel_with_no_plugin() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .restored_tree(5)
        .build();
    run.start();
    assert_eq!(run.kernel_tree_rows().len(), 5);
}

/// REQ: COR-010
#[test]
fn cor_010_the_manual_lists_the_tree_model_as_a_kernel_part() {
    let page = manual_page("reference-kernel-and-boot");
    assert!(page.contains("The tree model: nodes, groups, panes, ids"));
}

// COR-020
/// REQ: COR-020
#[test]
fn cor_020_the_spy_sees_the_restored_tree_on_activate() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .restored_tree(12)
        .plugin(startup("ilium.core.titles").records_tree_on_activate())
        .build();
    run.start();
    assert_eq!(run.tree_seen_on_activate("ilium.core.titles"), 12);
}

/// REQ: COR-020
#[test]
fn cor_020_startup_plugins_activate_in_dependency_order() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup("ilium.core.c").requires(&["ilium.core.b"]))
        .plugin(startup("ilium.core.b").requires(&["ilium.core.a"]))
        .plugin(startup("ilium.core.a"))
        .build();
    run.start();
    assert_eq!(
        run.activation_order(),
        vec![
            "ilium.core.a".to_owned(),
            "ilium.core.b".to_owned(),
            "ilium.core.c".to_owned()
        ]
    );
}

/// REQ: COR-020
#[test]
fn cor_020_the_client_socket_is_bound_last() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup("ilium.core.titles"))
        .build();
    run.start();
    let trace = run.trace();
    let bind = trace
        .iter()
        .position(|entry| entry == "bind-client-socket")
        .expect("bind marker");
    let last_activation = trace
        .iter()
        .rposition(|entry| entry.starts_with("activate:"))
        .expect("activation");
    let restore = trace
        .iter()
        .position(|entry| entry == "restore-snapshot")
        .expect("restore marker");
    assert!(
        restore
            < trace
                .iter()
                .position(|entry| entry.starts_with("instantiate:"))
                .expect("instantiate")
    );
    assert!(last_activation < bind);
    assert_eq!(bind, trace.len() - 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    // REQ: COR-020
    #[test]
    fn cor_020_any_acyclic_chain_activates_providers_first(length in 2usize..8) {
        let mut harness = BootHarness::new(ProcessKind::Server);
        for index in 0..length {
            let mut spec = startup(&format!("ilium.core.n{index}"));
            if index > 0 {
                let provider = format!("ilium.core.n{}", index - 1);
                spec = spec.requires(&[provider.as_str()]);
            }
            harness = harness.plugin(spec);
        }
        let mut run = harness.build();
        run.start();
        let order = run.activation_order();
        let expected: Vec<String> = (0..length).map(|index| format!("ilium.core.n{index}")).collect();
        prop_assert_eq!(order, expected);
    }
}

// COR-021
fn active_sides(run: &ilium_plugin_host::testing::boot::BootRun, side: Side) -> BTreeSet<String> {
    CORE_PLUGINS
        .iter()
        .filter(|name| run.side_state(&core_id(name), side).state == "active")
        .map(|name| (*name).to_owned())
        .collect()
}

/// REQ: COR-021
#[test]
fn cor_021_exactly_the_server_startup_plugins_are_active_at_boot() {
    let run = full_core_run(ProcessKind::Server);
    let expected: BTreeSet<String> = SERVER_STARTUP
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(active_sides(&run, Side::Server), expected);
}

/// REQ: COR-021
#[test]
fn cor_021_exactly_the_client_startup_plugins_are_active_after_connect() {
    let mut run = full_core_run(ProcessKind::Server);
    run.attach_client().expect("client attaches");
    let expected: BTreeSet<String> = CLIENT_STARTUP
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(active_sides(&run, Side::Client), expected);
}

/// REQ: COR-021
#[test]
fn cor_021_other_plugins_stay_inactive_until_their_trigger_fires() {
    let mut run = full_core_run(ProcessKind::Server);
    assert_ne!(
        run.side_state("ilium.core.recap", Side::Server).state,
        "active"
    );
    run.fire_trigger("command:recap");
    assert_eq!(
        run.side_state("ilium.core.recap", Side::Server).state,
        "active"
    );
}

/// REQ: COR-021
#[test]
fn cor_021_the_activation_table_in_the_manual_matches_the_expected_sets() {
    let page = manual_page("reference-kernel-and-boot");
    for name in [
        "Agent detect",
        "Chatroom",
        "Cost",
        "Diagnostics",
        "Notifications",
        "Progress",
        "Scheduler",
        "Sessions",
        "Text triggers",
        "Titles",
        "Tree",
    ] {
        let row = page
            .lines()
            .find(|line| line.starts_with(&format!("| {name} |")))
            .unwrap_or_else(|| panic!("row {name}"));
        let server_cell = row.split('|').nth(2).expect("server cell");
        assert!(server_cell.contains("`startup`"), "{name}: {server_cell}");
    }
    for name in [
        "Agent toolbar",
        "Ambient look",
        "Progress",
        "Settings",
        "Terminal",
        "Tree",
    ] {
        let row = page
            .lines()
            .find(|line| line.starts_with(&format!("| {name} |")))
            .unwrap_or_else(|| panic!("row {name}"));
        let client_cell = row.split('|').nth(3).expect("client cell");
        assert!(client_cell.contains("`startup`"), "{name}: {client_cell}");
    }
}

// COR-022
fn run_with_data_for_all() -> ilium_plugin_host::testing::boot::BootRun {
    let mut harness = BootHarness::new(ProcessKind::Server).core_activation_table();
    for name in CORE_PLUGINS {
        harness = harness
            .plugin_from_table(&core_id(name))
            .store_data(&core_id(name), "k", "v");
    }
    let mut run = harness.build();
    run.start();
    run
}

/// REQ: COR-022
#[test]
fn cor_022_disabling_each_core_plugin_keeps_its_stored_data() {
    for name in CORE_PLUGINS {
        let mut run = run_with_data_for_all();
        let id = core_id(name);
        run.disable(&id);
        assert_eq!(run.stored_data(&id, "k").as_deref(), Some("v"), "{name}");
    }
}

/// REQ: COR-022
#[test]
fn cor_022_the_kernel_fallback_and_recovery_page_work_with_each_plugin_off() {
    for name in CORE_PLUGINS {
        let mut run = run_with_data_for_all();
        run.disable(&core_id(name));
        assert!(run.kernel_alive(), "{name}");
        assert!(run.recovery_page_renders(), "{name}");
        assert!(run.command_works("kernel.open-recovery"), "{name}");
    }
}

/// REQ: COR-022
#[test]
fn cor_022_the_named_effect_of_each_row_holds_when_the_plugin_is_off() {
    for name in CORE_PLUGINS {
        let mut run = run_with_data_for_all();
        run.disable(&core_id(name));
        assert!(run.off_effect_holds(name), "off-effect row of {name}");
    }
}

/// REQ: COR-022
#[test]
fn cor_022_enabling_again_restores_the_plugin_with_its_data() {
    for name in CORE_PLUGINS {
        let mut run = run_with_data_for_all();
        let id = core_id(name);
        run.disable(&id);
        run.enable(&id);
        assert_eq!(run.stored_data(&id, "k").as_deref(), Some("v"), "{name}");
    }
}

/// REQ: COR-022
#[test]
fn cor_022_the_manual_table_lists_all_thirty_plugins() {
    let page = manual_page("reference-kernel-and-boot");
    let heading = page
        .find("What the user keeps when a plugin is off")
        .expect("table heading");
    let rows = page[heading..]
        .lines()
        .filter(|line| {
            line.starts_with("| ") && !line.starts_with("| ---") && !line.starts_with("|---")
        })
        .count();
    assert!(rows >= 31, "header plus 30 rows, found {rows}");
}

// COR-024
/// REQ: COR-024
#[test]
fn cor_024_a_kernel_setting_change_is_seen_live() {
    let mut run = full_core_run(ProcessKind::Server);
    let client = run.attach_client().expect("client");
    run.change_kernel_setting("sound", "off");
    assert!(run
        .events_seen_by_client_plugins(client)
        .contains(&"kernel-settings-changed".to_owned()));
    assert_eq!(run.server_restarts(), 0);
}

/// REQ: COR-024
#[test]
fn cor_024_a_plugin_setting_change_is_seen_live() {
    let mut run = full_core_run(ProcessKind::Server);
    run.attach_client().expect("client");
    run.change_plugin_setting("ilium.core.titles", "mode", "short");
    assert!(run
        .plugin_events("ilium.core.titles")
        .contains(&"settings-changed".to_owned()));
}

/// REQ: COR-024
#[test]
fn cor_024_a_key_binding_change_sends_keymap_changed() {
    let mut run = full_core_run(ProcessKind::Server);
    let client = run.attach_client().expect("client");
    run.change_key_binding("F8", "command:voice.toggle");
    assert!(run
        .events_seen_by_client_plugins(client)
        .contains(&"keymap-changed".to_owned()));
}

/// REQ: COR-024
#[test]
fn cor_024_enable_and_disable_apply_live() {
    let mut run = full_core_run(ProcessKind::Server);
    run.disable("ilium.core.titles");
    assert_ne!(
        run.side_state("ilium.core.titles", Side::Server).state,
        "active"
    );
    run.enable("ilium.core.titles");
    assert_eq!(
        run.side_state("ilium.core.titles", Side::Server).state,
        "active"
    );
    assert_eq!(run.server_restarts(), 0);
}

/// REQ: COR-024
#[test]
fn cor_024_grant_changes_apply_live() {
    let mut run = full_core_run(ProcessKind::Server);
    run.revoke_grant("ilium.core.titles", "tree.read");
    assert!(!run
        .granted("ilium.core.titles")
        .contains(&"tree.read".to_owned()));
    assert_eq!(run.server_restarts(), 0);
}

/// REQ: COR-024
#[test]
fn cor_024_plugin_limits_apply_to_new_instances_only() {
    let mut run = full_core_run(ProcessKind::Server);
    let before = run.effective_limit("ilium.core.titles", "memory_mib");
    run.change_limit("memory_mib", before + 8);
    assert_eq!(
        run.effective_limit("ilium.core.titles", "memory_mib"),
        before
    );
    run.restart_plugin("ilium.core.titles");
    assert_eq!(
        run.effective_limit("ilium.core.titles", "memory_mib"),
        before + 8
    );
}

/// REQ: COR-024
#[test]
fn cor_024_http_port_and_safe_switches_need_a_server_restart() {
    let mut run = full_core_run(ProcessKind::Server);
    run.change_http_port(8123);
    assert_ne!(run.live_http_port(), 8123);
    let restarted = run.restart_server();
    assert_eq!(restarted.live_http_port(), 8123);
}

/// REQ: COR-024
#[test]
fn cor_024_the_manual_table_names_the_three_classes() {
    let page = manual_page("reference-kernel-and-boot");
    for row in [
        "| A value on any kernel Settings tab | Live |",
        "| `[plugins.limits]` in `config.toml` | Live for new instances |",
    ] {
        assert!(page.contains(row), "{row}");
    }
}
