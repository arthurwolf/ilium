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

/// REQ: F-089
#[test]
fn doc_089_the_traceability_row_names_guided_setup_as_a_settings_plugin_feature() {
    let row = traceability_row("F-089");
    assert!(row.contains("| Guided setup | `getting-started` | plugin-wasm | settings plugin | client | ui.modal | none | none | 4b | COR-011 |"), "{row}");
}

/// REQ: F-090
#[test]
fn doc_090_the_traceability_row_names_the_settings_ui() {
    let row = traceability_row("F-090");
    assert!(row.contains("| Settings UI | `settings` | plugin-wasm | settings plugin | client | settings.* | none | none | 4b | COR-011 |"), "{row}");
}

/// REQ: F-086
#[test]
fn doc_086_the_traceability_row_names_debug_file_logging_with_its_config_key() {
    let row = traceability_row("F-086");
    assert!(row.contains("| Debug file logging | `settings` | native-kernel | kernel | both | logging | [debug] file_logging_enabled | logs | P1 | COR-010 |"), "{row}");
}

/// REQ: F-091
#[test]
fn doc_091_the_traceability_row_names_config_persistence_in_config_toml() {
    let row = traceability_row("F-091");
    assert!(row.contains("| Config persistence and live mirror | `settings` | native-kernel | kernel config store | both | settings.* | config.toml | config.toml | P1 | COR-030 |"), "{row}");
}

/// REQ: F-092
#[test]
fn doc_092_the_traceability_row_names_themes_motion_density_and_stable_glyphs() {
    let row = traceability_row("F-092");
    assert!(row.contains("| Themes, motion, density, stable glyphs | `settings` | native-kernel | kernel | client | theme | [ui] theme keys | none | P1 | COR-010 |"), "{row}");
}

/// REQ: F-089, F-090
#[test]
fn doc_089_onboarding_is_one_flow_owned_by_settings_and_voice_owns_only_its_demo_step() {
    let table = manual_page("reference-feature-traceability.md");
    assert!(table.contains("| Onboarding | settings and voice | [settings](/pages/core-plugin-settings/) | The guided setup is one flow (F-089). Voice owns only the demo step (F-053). |"));
    assert!(table.contains("| Plugins screen keys and help | keys-and-help and settings |"));
}

/// REQ: F-090
#[test]
fn doc_090_the_page_names_the_four_things_the_settings_plugin_owns() {
    let page = page_text();
    assert!(page.contains("It owns the tab strip, the auto-save behaviour, the guided setup shown on first run, the User Interface tab (theme, motion, density, stable glyphs), the Icons, Debug and About tabs, and the file logging switch."));
}

/// REQ: F-090
#[test]
fn doc_090_the_page_states_the_600_ms_text_and_number_rule() {
    assert!(page_text()
        .contains("Toggles and choices save at once. Text and number inputs save after 600 ms."));
}

/// REQ: F-090, F-091
#[test]
fn doc_091_the_files_belong_to_the_kernel() {
    assert!(page_text().contains("The screen itself and the files it writes (`config.toml` in the user's configuration folder and `.ilium/config.yaml` in a project) belong to the kernel, which keeps them valid."));
}

/// REQ: F-090
#[test]
fn doc_090_settings_all_grants_are_limited_to_bundled_first_party_plugins() {
    assert!(page_text().contains(
        "`settings.read-all` and `settings.write-all` are limited to bundled plugins of this tier."
    ));
}

/// REQ: F-089
#[test]
fn doc_089_the_user_guide_lists_seven_guided_setup_steps_in_order() {
    let guide = user_doc("settings.md");
    let start = guide.find("The seven steps are:").expect("steps");
    let titles: Vec<String> = guide[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|line| line.split("**").nth(1).expect("bold title").to_string())
        .collect();
    assert_eq!(
        titles,
        [
            "AI assistance",
            "Connect AI",
            "Your sound",
            "Sound studio",
            "Your controls",
            "Playground",
            "Voice control"
        ]
    );
}

/// REQ: F-089
#[test]
fn doc_089_per_option_help_belongs_to_keys_and_help_not_to_settings() {
    assert!(page_text().contains("The per-option help (the topics of `settings_help/catalog/topics.json`, about 110 topics) belongs to [keys and help]"));
}
