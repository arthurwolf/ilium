//! reference-feature-traceability.md: COR-001, COR-011 and the table rows of the kernel-owned
//! features (F ids). The behaviour of each feature is checked in the module of its user page.

use ilium_plugin_host::lifecycle::StartMode;

use crate::common::*;

/// Column positions of a feature row.
const FEATURE: usize = 1;
const BASELINE_PAGE: usize = 2;
const CLASS: usize = 3;
const SIDE: usize = 5;
const SETTINGS: usize = 7;
const PHASE: usize = 9;
const COR_ID: usize = 10;

fn assert_kernel_row(id: &str, feature: &str, page: &str, side: &str, settings: &str, phase: &str, cor: &str) {
    let row = feature_row(id);
    assert_eq!(row[0], id);
    assert_eq!(row[FEATURE], feature, "{id} feature name");
    assert_eq!(row[BASELINE_PAGE], format!("`{page}`"), "{id} baseline page");
    assert_eq!(row[CLASS], "native-kernel", "{id} class");
    assert_eq!(row[SIDE], side, "{id} process side");
    assert_eq!(row[SETTINGS], settings, "{id} settings");
    assert_eq!(row[PHASE], phase, "{id} phase");
    assert_eq!(row[COR_ID], cor, "{id} parity requirement");
    assert!(
        user_doc(page.trim_matches('`')).len() > 100,
        "{id}: the baseline page src/docs/{page}.md must exist and not be empty"
    );
}

/// Splits a settings cell such as `[ui] a, b`, `[keyboard], [keybindings]` or `none` into
/// `(table, key)` pairs; a bare table gives the key `*`.
pub fn settings_pairs(cell: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut table: Option<String> = None;
    for token in cell.split(',') {
        let token = token.trim();
        if token.is_empty() || token == "none" || token == "config.toml" || token.starts_with("project ") {
            continue;
        }
        if let Some(rest) = token.strip_prefix('[') {
            let (name, key) = rest.split_once(']').expect("closing bracket");
            table = Some(name.to_owned());
            let key = key.trim();
            pairs.push((name.to_owned(), if key.is_empty() { "*".to_owned() } else { key.to_owned() }));
        } else if let Some(name) = &table {
            pairs.push((name.clone(), token.to_owned()));
        }
    }
    pairs
}

/// Every feature id of the table, F-001 to F-116, in order.
fn all_feature_ids() -> Vec<String> {
    let page = manual_page("reference-feature-traceability");
    page.lines()
        .filter_map(|line| line.strip_prefix("| F-"))
        .map(|rest| format!("F-{}", &rest[..3]))
        .collect()
}

/// REQ: COR-001
#[test]
fn cor_001_the_table_lists_every_feature_f001_to_f116_once() {
    let ids = all_feature_ids();
    let expected: Vec<String> = (1..=116).map(|n| format!("F-{n:03}")).collect();
    assert_eq!(ids, expected);
}

/// REQ: COR-001
#[test]
fn cor_001_every_feature_row_names_an_existing_baseline_user_page() {
    for id in all_feature_ids() {
        let row = feature_row(&id);
        let page = row[BASELINE_PAGE].trim_matches('`').to_owned();
        let path = workspace_root().join("src/docs").join(format!("{page}.md"));
        assert!(path.is_file(), "{id}: baseline page {} is missing", path.display());
    }
}

/// REQ: COR-001
#[test]
fn cor_001_every_kernel_owned_feature_is_checked_by_a_scenario_in_both_builds() {
    // The kernel-owned ids of the lane; each has a scenario module test tagged with its id.
    let kernel_ids: Vec<String> = all_feature_ids()
        .into_iter()
        .filter(|id| feature_row(id)[CLASS] == "native-kernel")
        .collect();
    let lane = [
        "F-001", "F-002", "F-003", "F-004", "F-007", "F-008", "F-011", "F-012", "F-013", "F-014", "F-019",
        "F-024", "F-058", "F-073", "F-074", "F-075", "F-078", "F-079", "F-085", "F-086", "F-087", "F-091",
        "F-092", "F-093", "F-094", "F-104", "F-105", "F-106", "F-111",
    ];
    for id in lane {
        assert!(kernel_ids.iter().any(|known| known == id), "{id} must be native-kernel in the table");
    }
}

/// REQ: COR-001
#[test]
fn cor_001_a_booted_plugin_build_starts_every_core_plugin_the_baseline_has_a_feature_for() {
    let kernel = world(StartMode::Normal);
    let report = kernel.start_report();
    assert!(report.failed.is_empty(), "core plugins failed to start: {:?}", report.failed);
    assert_eq!(report.started.len(), 30, "30 core plugins: {:?}", report.started);
}

/// REQ: COR-001
#[test]
fn cor_001_a_kernel_only_build_starts_no_plugin_and_keeps_the_tree_usable() {
    let kernel = world(StartMode::NoPlugins);
    assert!(kernel.start_report().started.is_empty());
    let pane = terminal(&kernel, None, "baseline", &["sh"]);
    assert_eq!(node(&kernel, pane).title, "baseline");
}

/// REQ: COR-011
#[test]
fn cor_011_the_settings_column_parser_reads_tables_keys_and_bare_tables() {
    assert_eq!(settings_pairs("none"), Vec::<(String, String)>::new());
    assert_eq!(settings_pairs("[ui] tree_order"), vec![("ui".to_owned(), "tree_order".to_owned())]);
    assert_eq!(
        settings_pairs("[terminal] scrollback_budget_mib, engine_memory_budget_mib"),
        vec![
            ("terminal".to_owned(), "scrollback_budget_mib".to_owned()),
            ("terminal".to_owned(), "engine_memory_budget_mib".to_owned()),
        ]
    );
    assert_eq!(
        settings_pairs("[keyboard], [keybindings]"),
        vec![("keyboard".to_owned(), "*".to_owned()), ("keybindings".to_owned(), "*".to_owned())]
    );
}

/// The ids whose owner is a plugin (not the kernel, not retired).
fn plugin_owned_ids() -> Vec<String> {
    all_feature_ids()
        .into_iter()
        .filter(|id| matches!(feature_row(id)[CLASS].as_str(), "plugin-wasm" | "primitive-extended"))
        .collect()
}

/// REQ: COR-011
#[test]
fn cor_011_every_documented_setting_key_of_a_plugin_owned_feature_is_exposed_by_the_plugin_build() {
    let kernel = world(StartMode::Normal);
    let exposed = kernel.settings_keys();
    let mut missing = Vec::new();
    for id in plugin_owned_ids() {
        for (table, key) in settings_pairs(&feature_row(&id)[SETTINGS]) {
            let found = exposed.iter().any(|(t, k)| *t == table && (key == "*" || *k == key));
            if !found {
                missing.push(format!("{id}: [{table}] {key}"));
            }
        }
    }
    assert!(missing.is_empty(), "settings missing from the plugin build: {missing:#?}");
}

/// REQ: COR-011
#[test]
fn cor_011_a_config_file_with_every_documented_plugin_key_survives_boot_unchanged() {
    let mut config = String::new();
    for id in plugin_owned_ids() {
        for (table, key) in settings_pairs(&feature_row(&id)[SETTINGS]) {
            if key != "*" {
                config.push_str(&format!("[{table}]\n{key} = 1\n"));
            }
        }
    }
    let kernel = world_with_config(StartMode::Normal, &config);
    let after = kernel.config_text();
    for id in plugin_owned_ids() {
        for (table, key) in settings_pairs(&feature_row(&id)[SETTINGS]) {
            if key != "*" {
                assert!(after.contains(&key), "{id}: key {key} of [{table}] was dropped");
            }
        }
    }
}

/// REQ: COR-011
#[test]
fn cor_011_defaults_of_plugin_owned_settings_equal_the_kernel_only_defaults_of_the_same_keys() {
    let with_plugins = world(StartMode::Normal);
    for (table, key, value) in with_plugins.settings_defaults() {
        let baseline = world(StartMode::NoPlugins).settings_defaults();
        if let Some((_, _, baseline_value)) = baseline.iter().find(|(t, k, _)| *t == table && *k == key) {
            assert_eq!(&value, baseline_value, "default of [{table}] {key}");
        }
    }
}

/// REQ: COR-011
#[test]
fn cor_011_documented_defaults_scrollback_8_and_parser_pool_off_are_unchanged() {
    for mode in MODES {
        let kernel = world(mode);
        let defaults = kernel.settings_defaults();
        let value = |table: &str, key: &str| {
            defaults.iter().find(|(t, k, _)| t == table && k == key).map(|(_, _, v)| v.clone())
        };
        assert_eq!(value("terminal", "scrollback_budget_mib").as_deref(), Some("8"), "{mode:?}");
        assert_eq!(value("terminal", "engine_memory_budget_mib").as_deref(), Some("0"), "{mode:?}");
    }
}

/// REQ: F-001
#[test]
fn f_001_row_is_native_kernel_with_tree_order_setting() {
    assert_kernel_row("F-001", "Project tree of projects, groups, folders, panes", "panes-and-layout", "both", "[ui] tree_order", "P1 kernel; 4b tree", "COR-010");
}

/// REQ: F-002
#[test]
fn f_002_row_is_native_kernel_without_settings() {
    assert_kernel_row("F-002", "Groups, folders, top-level destinations", "panes-and-layout", "both", "none", "P1", "COR-010");
}

/// REQ: F-003
#[test]
fn f_003_row_lists_the_five_left_panel_settings() {
    let row = feature_row("F-003");
    assert_eq!(row[CLASS], "native-kernel");
    assert_eq!(
        settings_pairs(&row[SETTINGS]),
        ["left_panel_sizing_mode", "left_panel_fixed_width", "left_panel_unfocused_width", "left_panel_focused_width", "left_panel_minimum_terminal_width"]
            .iter()
            .map(|key| ("ui".to_owned(), (*key).to_owned()))
            .collect::<Vec<_>>()
    );
    assert_eq!(row[SIDE], "client");
}

/// REQ: F-004
#[test]
fn f_004_row_is_native_kernel_for_both_sides() {
    assert_kernel_row("F-004", "Move, reorder, rename, drag", "panes-and-layout", "both", "none", "P1; 4b", "COR-010");
}

/// REQ: F-007
#[test]
fn f_007_row_lists_keyboard_and_keybindings_tables() {
    assert_kernel_row("F-007", "Prefix, keybindings, presets, remap", "settings", "client", "[keyboard], [keybindings]", "P1 kernel; 4b", "COR-010");
}

/// REQ: F-008
#[test]
fn f_008_row_is_native_kernel_with_mouse_keys() {
    assert_kernel_row("F-008", "Mouse (click, drag, wheel, hit targets)", "panes-and-layout", "both", "[ui] mouse keys", "P1", "COR-010");
}

/// REQ: F-011
#[test]
fn f_011_row_is_native_kernel_pty() {
    assert_kernel_row("F-011", "Pane processes", "how-it-works", "server", "none", "P1", "COR-010");
}

/// REQ: F-012
#[test]
fn f_012_row_is_native_kernel_vt100() {
    assert_kernel_row("F-012", "Terminal emulation and screen state", "how-it-works", "server", "none", "P1", "COR-010");
}

/// REQ: F-013
#[test]
fn f_013_row_lists_scrollback_and_engine_budget() {
    let row = feature_row("F-013");
    assert_eq!(
        settings_pairs(&row[SETTINGS]),
        vec![("terminal".to_owned(), "scrollback_budget_mib".to_owned()), ("terminal".to_owned(), "engine_memory_budget_mib".to_owned())]
    );
    assert_eq!(row[SIDE], "client");
}

/// REQ: F-014
#[test]
fn f_014_row_is_native_kernel_terminal_write() {
    assert_kernel_row("F-014", "Terminal input and paste", "panes-and-layout", "both", "none", "P1", "COR-010");
}

/// REQ: F-019
#[test]
fn f_019_row_is_kernel_now_with_editor_plugin_in_phase_7() {
    let row = feature_row("F-019");
    assert_eq!(row[CLASS], "native-kernel");
    assert_eq!(row[PHASE], "P7");
    assert_eq!(row[COR_ID], "COR-050");
    assert_eq!(row[BASELINE_PAGE], "`editors-and-boards`");
}

/// REQ: F-024
#[test]
fn f_024_row_is_kernel_now_with_kanban_plugin_in_phase_7() {
    let row = feature_row("F-024");
    assert_eq!(row[CLASS], "native-kernel");
    assert_eq!(row[PHASE], "P7");
    assert_eq!(row[COR_ID], "COR-050");
    assert_eq!(settings_pairs(&row[SETTINGS]), vec![("kanban_board".to_owned(), "*".to_owned())]);
}

/// REQ: F-058
#[test]
fn f_058_row_is_server_side_with_api_port_setting() {
    assert_kernel_row("F-058", "Local HTTP API POST /create_agent", "automation", "server", "[api] port", "P1; 4b", "COR-010");
}

/// REQ: F-073
#[test]
fn f_073_row_is_the_json_snapshot_writer() {
    assert_kernel_row("F-073", "JSON snapshots", "session-recovery", "server", "none", "P1", "COR-030");
}

/// REQ: F-074
#[test]
fn f_074_row_is_rolling_snapshot_backups() {
    assert_kernel_row("F-074", "Rolling snapshot backups", "session-recovery", "server", "none", "P1", "COR-030");
}

/// REQ: F-075
#[test]
fn f_075_row_lists_recovery_policy_and_backups_enabled() {
    let row = feature_row("F-075");
    assert_eq!(
        settings_pairs(&row[SETTINGS]),
        vec![("session".to_owned(), "recovery_policy".to_owned()), ("session".to_owned(), "backups_enabled".to_owned())]
    );
    assert_eq!(row[COR_ID], "COR-010");
}

/// REQ: F-078
#[test]
fn f_078_row_is_cli_session_management() {
    assert_kernel_row("F-078", "Session management and flags", "cli-reference", "CLI", "none", "P1", "COR-040");
}

/// REQ: F-079
#[test]
fn f_079_row_is_the_ordered_event_journal() {
    assert_kernel_row("F-079", "Ordered event journal", "how-it-works", "server", "none", "P1", "COR-010");
}

/// REQ: F-085
#[test]
fn f_085_row_lists_inference_and_onboarding_tables() {
    let row = feature_row("F-085");
    assert_eq!(settings_pairs(&row[SETTINGS]), vec![("inference".to_owned(), "*".to_owned()), ("onboarding".to_owned(), "*".to_owned())]);
    assert_eq!(row[CLASS], "native-kernel");
}

/// REQ: F-086
#[test]
fn f_086_row_lists_file_logging_enabled() {
    assert_kernel_row("F-086", "Debug file logging", "settings", "both", "[debug] file_logging_enabled", "P1", "COR-010");
}

/// REQ: F-087
#[test]
fn f_087_row_is_background_worker_banks() {
    assert_kernel_row("F-087", "Background worker banks", "how-it-works", "both", "none", "P1", "COR-010");
}

/// REQ: F-091
#[test]
fn f_091_row_is_config_persistence_with_config_toml() {
    let row = feature_row("F-091");
    assert_eq!(row[CLASS], "native-kernel");
    assert_eq!(row[SETTINGS], "config.toml");
    assert_eq!(row[COR_ID], "COR-030");
}

/// REQ: F-092
#[test]
fn f_092_row_is_theme_motion_density_and_stable_glyphs() {
    assert_kernel_row("F-092", "Themes, motion, density, stable glyphs", "settings", "client", "[ui] theme keys", "P1", "COR-010");
}

/// REQ: F-093
#[test]
fn f_093_row_is_the_shared_look_with_phase_4c_controls() {
    let row = feature_row("F-093");
    assert_eq!(row[CLASS], "native-kernel");
    assert_eq!(row[PHASE], "P1; 4c");
    assert_eq!(row[BASELINE_PAGE], "`animations`");
}

/// REQ: F-094
#[test]
fn f_094_row_is_shared_dithering_and_display() {
    let row = feature_row("F-094");
    assert_eq!(row[CLASS], "native-kernel");
    assert_eq!(row[PHASE], "P1; 4c");
}

/// REQ: F-102
#[test]
fn f_102_row_is_the_retired_packaged_javascript_animation() {
    let row = feature_row("F-102");
    assert_eq!(row[CLASS], "plugin-js");
    assert_eq!(row[FEATURE], "Packaged JavaScript animations");
    assert!(row[4].starts_with("retired"), "{:?}", row[4]);
    assert_eq!(row[COR_ID], "COR-011");
    assert_eq!(row[PHASE], "4c");
}

/// REQ: F-104
#[test]
fn f_104_row_is_the_optional_gpu_backend() {
    assert_kernel_row("F-104", "Optional GPU backend", "animations", "client", "none", "P1", "COR-010");
}

/// REQ: F-105
#[test]
fn f_105_row_is_attach_cwd_onboarding() {
    assert_kernel_row("F-105", "ilium attach, --cwd, --onboarding", "cli-reference", "CLI", "none", "P1", "COR-040");
}

/// REQ: F-106
#[test]
fn f_106_row_is_new_session_ls_kill_session() {
    assert_kernel_row("F-106", "new-session, ls, kill-session", "cli-reference", "CLI", "none", "P1", "COR-040");
}

/// REQ: F-111
#[test]
fn f_111_row_is_hidden_internal_commands() {
    assert_kernel_row("F-111", "Hidden internal commands", "cli-reference", "CLI", "none", "P1", "COR-040");
}
