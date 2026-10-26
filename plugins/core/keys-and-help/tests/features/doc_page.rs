//! Doc tests (`doc_` rows): rules that the manual page, the traceability table and the user guide
//! state and that only a reading of those pages can check.

use crate::common::*;

fn traceability_row(id: &str) -> String {
    manual_page("reference-feature-traceability.md")
        .lines()
        .find(|line| line.starts_with(&format!("| {id} |")))
        .unwrap_or_else(|| panic!("traceability row {id}"))
        .to_string()
}

/// REQ: F-007
#[test]
fn doc_007_the_traceability_row_splits_the_feature_between_the_kernel_keymap_and_this_plugin() {
    let row = traceability_row("F-007");
    assert!(
        row.contains("| Prefix, keybindings, presets, remap | `settings` | native-kernel | kernel keymap; keys-and-help plugin | client | keymap.* | [keyboard], [keybindings] | none | P1 kernel; 4b | COR-010 |"),
        "{row}"
    );
}

/// REQ: F-010
#[test]
fn doc_010_the_traceability_row_names_help_as_a_wasm_plugin_feature_with_help_topics() {
    let row = traceability_row("F-010");
    assert!(
        row.contains("| Help and key reference | `settings` | plugin-wasm | keys-and-help plugin | client | help.contribute | none | help topics | 4b | COR-011 |"),
        "{row}"
    );
}

/// REQ: F-010
#[test]
fn doc_010_the_page_says_each_settings_option_has_its_own_help_topic() {
    assert!(page_text().contains("each Settings option has its own help topic"));
}

/// REQ: F-007
#[test]
fn doc_007_the_page_says_the_keymap_stays_in_the_kernel() {
    let page = page_text();
    assert!(page.contains("The keymap itself is kernel-native"));
    assert!(page.contains("never changes how a chord is dispatched"));
}

/// REQ: COR-010
#[test]
fn doc_010_the_keymap_is_on_the_kernel_part_list_of_the_kernel_page() {
    let page = manual_page("reference-kernel-and-boot.md");
    assert!(page.contains("keymap"));
    let requirements = manual_page("reference-requirements-core-parity.md");
    assert!(requirements.contains("config store, keymap, layout"));
}

/// REQ: COR-022, F-007
#[test]
fn doc_022_the_disabled_notice_text_is_the_one_the_page_gives() {
    assert!(page_text().contains("\"Keys and help is disabled. Values are kept.\""));
}

/// REQ: COR-030, F-007
#[test]
fn doc_030_the_page_lists_the_keyboard_tab_with_its_config_tables_and_no_readers() {
    let rows = table_rows(&page_text(), "| Tab id |");
    assert_eq!(
        rows,
        vec![vec![
            "`keyboard`",
            "Keyboard",
            "`[keyboard]`, `[keybindings]`",
            "none"
        ]]
    );
}

/// REQ: COR-030
#[test]
fn doc_030_the_config_page_names_keys_and_help_as_the_keyboard_tab_owner() {
    let page = manual_page("reference-config-and-data-migration.md");
    assert!(page.contains("| `keyboard` | Keyboard | `[keyboard]`, `[keybindings]` | Keys and help | none | `[keyboard]`, `[keybindings]` |"));
}

/// REQ: COR-033, F-007
#[test]
fn doc_033_the_user_guide_keeps_the_same_config_tables_the_page_names() {
    let settings = user_doc("settings.md");
    assert!(settings.contains("| Keyboard | Prefix keys and per-action key remapping, complete presets | `[keyboard]`, `[keybindings]` |"));
}

/// REQ: F-007
#[test]
fn doc_007_the_user_guide_states_the_prefix_and_navigation_prefix_keys() {
    let settings = user_doc("settings.md");
    assert!(settings.contains("`shortcut_base`, a `Ctrl`-letter, default `Ctrl+B`"));
    assert!(settings.contains("`navigation_shortcut_base`"));
}

/// REQ: F-007
#[test]
fn doc_007_the_getting_started_guide_describes_two_complete_presets() {
    let guide = user_doc("getting-started.md");
    assert!(guide.contains("Pick a complete preset (**tmux** or **GNU Screen**)"));
    assert!(guide.contains("Applying a preset replaces the whole binding table"));
}

/// REQ: F-010
#[test]
fn doc_010_the_page_lists_the_four_covered_areas() {
    let page = page_text();
    for label in [
        "**Prefix, keybindings, presets and remapping.**",
        "**Help and key reference.**",
        "**Command palette entries.**",
        "**Plugin manager.**",
    ] {
        assert!(page.contains(label), "{label}");
    }
}

/// REQ: F-010
#[test]
fn doc_010_the_page_has_two_render_figures_for_the_reference_and_the_keyboard_tab() {
    let figures = page_figures();
    assert_eq!(figures.len(), 3);
    assert!(figures[0].contains("Key reference"));
    assert!(figures[1].contains("Press the new key chord."));
    assert!(figures[2].contains("Navigation prefix"));
}

/// REQ: F-010
#[test]
fn doc_010_the_help_settings_page_states_that_help_never_performs_the_action_it_describes() {
    assert!(user_doc("settings.md").contains("Help never performs the action it describes"));
}

/// REQ: F-010
#[test]
fn doc_010_the_page_states_the_extension_point_table_with_two_collect_points() {
    let rows = table_rows(&page_text(), "| Point | Kind |");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "`key-hints`");
    assert_eq!(rows[0][1], "collect");
    assert_eq!(rows[1][0], "`help-sources`");
    assert_eq!(rows[1][1], "collect");
    assert_eq!(rows[0][4], "standard");
    assert_eq!(rows[1][4], "standard");
}

/// REQ: F-010
#[test]
fn doc_010_the_plugin_is_replaceable_by_a_plugin_that_lists_replaces() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("\"replaces\": [\"ilium.core.keys-and-help\"]"));
}
