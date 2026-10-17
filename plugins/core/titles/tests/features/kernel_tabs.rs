//! Kernel tabs owned by the plugin, the config tables they keep, and the oracle tables of the user documentation.

use crate::common::*;

/// REQ: COR-030, COR-033, F-044
#[test]
fn doc_cor_030_page_lists_exactly_the_titles_and_triggers_tabs() {
    let page = page_text();
    let titles = table_row(&page, "| `titles` |");
    assert_eq!(
        &titles[1..5],
        ["`titles`", "Titles", "`[inference]`", "none"]
    );
    let triggers = table_row(&page, "| `triggers` |");
    assert_eq!(
        &triggers[1..5],
        ["`triggers`", "Triggers", "`[triggers]`", "none"]
    );
}

/// REQ: COR-030, COR-033, F-044
#[test]
fn doc_cor_033_tab_grants_in_the_manifest_match_the_tab_ids() {
    let manifest = manifest_value();
    let client: Vec<String> = manifest["capabilities"]["client"]
        .as_array()
        .expect("client grants")
        .iter()
        .map(|grant| grant.as_str().expect("string").to_string())
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(
        client,
        [
            "settings.write-kernel:titles",
            "settings.write-kernel:triggers"
        ]
    );
}

/// REQ: COR-030, F-041
#[test]
fn doc_cor_030_migration_page_gives_the_titles_tab_only_the_title_style_key() {
    let page = pages_text("reference-config-and-data-migration.md");
    let row = table_row(&page, "| `titles` |");
    assert_eq!(row[2], "Titles");
    assert_eq!(row[3], "`[inference]`");
    assert_eq!(row[4], "Titles");
    assert_eq!(row[5], "none");
    assert_eq!(row[6], "`[inference] title_style` only");
}

/// REQ: COR-030, F-044
#[test]
fn doc_cor_030_migration_page_gives_the_triggers_tab_the_whole_triggers_table() {
    let page = pages_text("reference-config-and-data-migration.md");
    let row = table_row(&page, "| `triggers` |");
    assert_eq!(row[3], "`[triggers]`");
    assert_eq!(row[4], "Titles");
    assert_eq!(row[6], "`[triggers]`");
}

/// REQ: COR-033, F-041
#[test]
fn doc_cor_033_values_stay_in_the_same_config_tables() {
    let page = pages_text("reference-config-and-data-migration.md");
    let inference = table_row(&page, "| `[inference]` |");
    assert!(inference[2].contains("`title_style`"));
    assert_eq!(inference[3], "Inference, Titles");
    let triggers = table_row(&page, "| `[triggers]` |");
    assert!(triggers[2].starts_with("eight event keys"));
    assert_eq!(triggers[3], "Titles");
}

/// REQ: COR-022, F-041
#[test]
fn doc_cor_022_disabled_notice_and_kept_values_are_stated_exactly() {
    let page = page_text();
    assert!(page.contains("\"Titles and organisation is disabled. Values are kept.\""));
    assert!(page.contains("the tab stays in the rail with its stored values shown read-only"));
}

/// REQ: COR-022, F-041
#[test]
fn doc_cor_022_boot_page_says_without_titles_there_is_no_automatic_naming() {
    let boot = pages_text("reference-kernel-and-boot.md");
    let row = table_row(&boot, "| Titles | No automatic naming |");
    assert_eq!(row[2], "No automatic naming");
    assert_eq!(row[3], "Manual rename is a kernel command");
}

/// REQ: F-041, F-043
#[test]
fn doc_f_041_rail_order_places_titles_before_triggers() {
    let page = pages_text("reference-config-and-data-migration.md");
    let at_titles = page
        .find("Inference, LLM Instructions, Titles, Triggers, Text Triggers")
        .expect("rail order");
    let order = "Inference, LLM Instructions, Titles, Triggers, Text Triggers";
    let slice = &page[at_titles..at_titles + order.len()];
    let names: Vec<&str> = slice.split(", ").collect();
    let titles_at = names
        .iter()
        .position(|name| *name == "Titles")
        .expect("Titles in rail");
    let triggers_at = names
        .iter()
        .position(|name| *name == "Triggers")
        .expect("Triggers in rail");
    assert_eq!(titles_at + 1, triggers_at);
}

/// REQ: F-041
#[test]
fn doc_f_041_title_styles_are_labeling_default_and_summarization() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("| Labeling (default) |"));
    assert!(doc.contains("| Summarization |"));
    assert!(doc.contains("`title_style = \"labeling\"` or `\"summarization\"` under `[inference]`"));
}

/// REQ: F-041
#[test]
fn doc_f_041_labeling_limits_are_one_to_three_and_one_to_seven_words() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("short label 1 to 3 words, long label 1 to 7 words"));
}

/// REQ: F-041
#[test]
fn doc_f_041_summarization_limits_are_two_to_three_and_at_most_seven_words() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("short title 2 to 3 words, long title at most 7 words"));
}

/// REQ: F-044
#[test]
fn doc_f_044_triggers_tab_maps_eight_events_with_the_documented_defaults() {
    let doc = user_doc("settings.md");
    let expected = [
        ("Startup complete", "Restructure all projects"),
        ("Agent session ready", "Retitle element"),
        ("Agent receives a prompt", "Retitle element"),
        ("Agent starts working", "Retitle element"),
        ("Agent waits on background work", "Retitle element"),
        ("Agent needs approval", "none"),
        (
            "Agent finishes work",
            "Retitle element and Restructure project",
        ),
        ("Terminal activity checkpoint", "Retitle element"),
    ];
    for (event, actions) in expected {
        let row = table_row(&doc, &format!("| {event} |"));
        assert_eq!(row[2], actions, "{event}");
    }
}

/// REQ: F-044
#[test]
fn doc_f_044_terminal_checkpoint_fires_after_every_second_plain_shell_command() {
    let doc = user_doc("settings.md");
    assert!(doc.contains(
        "The terminal checkpoint fires after every second submitted plain-shell command."
    ));
}

/// REQ: F-044
#[test]
fn doc_f_044_finishing_work_permits_one_restructure_scope_not_both() {
    let doc = user_doc("settings.md");
    assert!(doc.contains("Finishing work permits retitling plus one restructure scope, not both restructure scopes at once."));
}

/// REQ: F-045
#[test]
fn doc_f_045_seven_instruction_inputs_have_the_documented_feature_tabs() {
    let doc = user_doc("titles-and-instructions.md");
    let expected = [
        ("Voice assistant", "Voice control"),
        ("Entry naming", "Titles"),
        ("Organization", "Inference"),
        ("Shared naming and organization context", "Inference"),
        ("Project naming", "Inference"),
        ("Smart Copy", "Inference"),
        ("Ask for update", "Agent Monitoring"),
    ];
    for (input, tab) in expected {
        let row = table_row(&doc, &format!("| {input} |"));
        assert_eq!(row[2], tab, "{input}");
    }
}

/// REQ: F-045
#[test]
fn doc_f_045_storage_keys_of_the_six_non_voice_inputs() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains(
        "`[inference.instructions]` (`entry_naming`, `organization`, `naming_and_organization`, `project_naming`, `smart_copy`, `ask_for_update`)"
    ));
    assert!(doc.contains("the voice input is `[voice].custom_prompt`"));
}

/// REQ: F-045
#[test]
fn doc_f_045_llm_instructions_tab_is_owned_by_inference_not_titles() {
    let page = pages_text("reference-config-and-data-migration.md");
    let row = table_row(&page, "| `llm-instructions` |");
    assert_eq!(row[4], "Inference");
    assert_eq!(row[5], "Titles, Recap, Smart copy, Voice");
}

/// REQ: F-046
#[test]
fn doc_f_046_show_inferred_title_icons_is_a_ui_key_read_by_agent_detect_and_titles() {
    let page = pages_text("reference-config-and-data-migration.md");
    let row = table_row(&page, "| `[ui]` agents |");
    assert!(row[2].contains("`show_inferred_title_icons`"));
    assert_eq!(row[3], "Agent detect, Titles");
}

/// REQ: F-046
#[test]
fn doc_f_046_user_docs_list_show_inferred_title_icons_among_ui_keys() {
    assert!(user_doc("settings.md").contains("`show_inferred_title_icons`"));
}
