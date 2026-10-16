//! The kernel tab this plugin owns (COR-030, COR-033, COR-022, COR-010, COR-011).

use ilium_plugin_sdk::types::{Event, KernelSettingsEvent};
use serde_json::json;

use crate::common::*;

/// REQ: COR-030, F-093
#[test]
fn doc_cor_030_page_lists_the_animations_tab_with_its_config_table_and_reader() {
    let row = table_row(&page_text(), "`animations`");
    assert_eq!(&row[1..], ["Animations", "project YAML `animation`", "Scenes"]);
}

/// REQ: COR-030, F-093
#[test]
fn doc_cor_030_page_says_the_plugin_is_the_single_owner_with_write_kernel_for_each_tab() {
    let text = page_text();
    assert!(text.contains("This plugin is the single owner of the following built-in Settings tabs."));
    assert!(text.contains("It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab"));
}

/// REQ: COR-033, F-093
#[test]
fn doc_cor_033_page_says_values_stay_in_the_same_tables_so_no_setting_moves() {
    assert!(page_text().contains("The values stay in the same `config.toml` tables as before, so no user setting moves."));
}

/// REQ: COR-030, F-093
#[test]
fn doc_cor_030_migration_page_names_ambient_look_as_holder_of_the_animations_tab() {
    let row = table_row(&page("reference-config-and-data-migration"), "`animations`");
    assert_eq!(
        &row[1..],
        ["Animations", "project YAML `animation`", "Ambient look", "Scenes", "project YAML `animation`"]
    );
}

/// REQ: COR-033, F-093
#[test]
fn doc_cor_033_migration_page_keeps_the_animation_key_with_ambient_look_as_writer() {
    let row = table_row(&page("reference-config-and-data-migration"), "`animation`");
    assert_eq!(&row[1..], ["Ambient look", "Scenes, kernel painter"]);
}

/// REQ: COR-030, F-090, F-093
#[test]
fn doc_cor_030_settings_page_assigns_the_animations_tab_to_ambient_look() {
    let row = table_row(&page("core-plugin-settings"), "13");
    assert_eq!(&row[1..], ["`animations`", "Animations", "[Ambient look](/pages/core-plugin-ambient/)"]);
}

/// REQ: COR-022, F-093
#[test]
fn doc_cor_022_page_states_the_disabled_notice_verbatim() {
    assert!(page_text().contains("\"Ambient look is disabled. Values are kept.\""));
}

/// REQ: COR-022, F-093
#[test]
fn doc_cor_022_page_says_the_tab_stays_in_the_rail_read_only_when_disabled() {
    assert!(page_text().contains("If the plugin is disabled, the tab stays in the rail with its stored values shown read-only"));
}

/// REQ: COR-022, F-093
#[test]
fn doc_cor_022_kernel_and_boot_names_the_off_effect_and_the_stored_values() {
    let boot = page("reference-kernel-and-boot");
    assert!(boot.contains(
        "| Ambient look | The animation uses the kernel default look | Stored look values stay and apply when the plugin returns |"
    ));
}

/// REQ: COR-010, F-093, F-094
#[test]
fn doc_cor_010_kernel_keeps_the_look_painter_and_the_plugin_owns_only_the_controls() {
    let boot = page("reference-kernel-and-boot");
    assert!(boot.contains(
        "| Animation look application, dithering and cell painting | The look is global and applies after the scene draws | Scenes draw tone into a raster. The Ambient look plugin owns the controls |"
    ));
    assert!(page_text().contains("Painting the background and applying colour, filters and dithering to each cell is done by the client kernel."));
}

/// REQ: COR-010, F-093, F-094
#[test]
fn doc_cor_010_traceability_keeps_features_93_and_94_native_kernel_with_plugin_controls() {
    let traceability = page("reference-feature-traceability");
    let look = table_row(&traceability, "F-093");
    let display = table_row(&traceability, "F-094");
    assert_eq!(look[1], "Shared look");
    assert_eq!(look[3], "native-kernel");
    assert_eq!(look[4], "kernel look painter; ambient plugin for controls");
    assert_eq!(display[1], "Shared dithering and display");
    assert_eq!(display[3], "native-kernel");
    assert_eq!(display[4], "kernel; ambient plugin for controls");
}

/// REQ: COR-011, F-093, F-094
#[test]
fn doc_cor_011_traceability_rows_keep_the_animation_config_key_and_no_data_files() {
    let traceability = page("reference-feature-traceability");
    for id in ["F-093", "F-094"] {
        let row = table_row(&traceability, id);
        assert_eq!(row[7], "project YAML animation", "{id}");
        assert_eq!(row[8], "none", "{id}");
        assert_eq!(row[9], "P1; 4c", "{id}");
    }
}

/// REQ: COR-011, F-103
#[test]
fn doc_cor_011_traceability_row_of_semantic_animation_is_plugin_wasm_on_both_sides() {
    let row = table_row(&page("reference-feature-traceability"), "F-103");
    assert_eq!(row[1], "Semantic animation");
    assert_eq!(row[3], "plugin-wasm");
    assert_eq!(row[5], "both");
}

/// The kernel starts the plugin at startup, in the client group (reference-kernel-and-boot.md).
///
/// REQ: COR-050, F-093
#[test]
fn doc_cor_050_boot_page_activates_ambient_look_at_startup_with_no_dependencies() {
    let boot = page("reference-kernel-and-boot");
    assert!(boot.contains("| Ambient look | none | `startup` | The look is applied from the first frame |"));
}

/// REQ: COR-030, F-093
#[test]
fn cor_030_activation_makes_no_kernel_settings_write() {
    let mut host = client_host();
    set_kernel_tab(&mut host, "animations", json!({ "kind": "stars" }));
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    assert!(kernel_writes(&mut host).is_empty());
}

/// REQ: COR-033, F-093
#[test]
fn cor_033_a_kernel_settings_event_for_the_animations_tab_writes_nothing_back() {
    let mut host = activated_client();
    let event = Event::KernelSettingsChanged(KernelSettingsEvent { tab: "animations".to_string(), keys: vec!["kind".to_string()] });
    host.deliver(vec![event]);
    host.settle();
    assert!(kernel_writes(&mut host).is_empty());
}

/// REQ: COR-030, F-093
#[test]
fn cor_030_a_kernel_settings_event_for_another_tab_is_ignored() {
    let mut host = activated_client();
    let event = Event::KernelSettingsChanged(KernelSettingsEvent { tab: "themes".to_string(), keys: vec!["theme".to_string()] });
    let submissions = host.deliver(vec![event]);
    host.settle();
    assert!(submissions.is_empty());
    assert!(kernel_writes(&mut host).is_empty());
}

/// Every kernel write the plugin makes targets the one tab it holds.
///
/// REQ: COR-030, F-093
#[test]
fn cor_030_kernel_writes_after_a_settings_change_only_target_the_animations_tab() {
    let mut host = activated_client();
    host.settings().set("scene", json!("clouds")).expect("set");
    host.deliver(vec![settings_changed(&["scene"])]);
    host.settle();
    for (tab, _key, _value) in kernel_writes(&mut host) {
        assert_eq!(tab, "animations");
    }
}

/// Without the grant a kernel write is refused with not-permitted (COR-030: one holder per tab).
///
/// REQ: COR-030
#[test]
fn cor_030_a_host_without_the_write_kernel_grant_still_activates_the_plugin_without_writes() {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest").side("client").build();
    host.revoke("settings.write-kernel:animations");
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    assert!(kernel_writes(&mut host).is_empty());
}

use ilium_plugin_sdk::testing::FakeHost;
