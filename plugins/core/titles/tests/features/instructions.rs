//! The seven custom instruction inputs (F-045) and how the entry naming input steers titles (sample 5 of the page).

use serde_json::json;

use crate::common::*;

const ENTRY_NAMING: &str = "inference.instructions.entry_naming";

fn request_text(host: &mut ilium_plugin_sdk::testing::FakeHost) -> String {
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (_, request) = host.inference_requests().pop().expect("a request");
    request
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// REQ: F-045
#[test]
fn f_045_the_entry_naming_text_reaches_the_title_request() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("Write titles in French."))
        .expect("accepted");
    host.settle();
    assert!(request_text(&mut host).contains("Write titles in French."));
}

/// REQ: F-045
#[test]
fn f_045_whitespace_around_the_text_is_trimmed() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("  \n Write titles in French. \n "))
        .expect("accepted");
    host.settle();
    let text = request_text(&mut host);
    assert!(text.contains("Write titles in French."));
    assert!(!text.contains(" Write titles in French. \n"));
}

/// REQ: F-045
#[test]
fn f_045_the_text_is_interpolated_once() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("UNIQUE-MARKER-TEXT"))
        .expect("accepted");
    host.settle();
    assert_eq!(
        request_text(&mut host)
            .matches("UNIQUE-MARKER-TEXT")
            .count(),
        1
    );
}

/// REQ: F-045
#[test]
fn f_045_an_empty_input_uses_the_built_in_prompt_alone() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("UNIQUE-MARKER-TEXT"))
        .expect("accepted");
    host.settings()
        .set(ENTRY_NAMING, json!(""))
        .expect("accepted");
    host.settle();
    assert!(!request_text(&mut host).contains("UNIQUE-MARKER-TEXT"));
}

/// REQ: F-045
#[test]
fn f_045_a_change_applies_to_the_next_request_without_a_restart() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("FIRST-TEXT"))
        .expect("accepted");
    host.settle();
    assert!(request_text(&mut host).contains("FIRST-TEXT"));
    host.settings()
        .set(ENTRY_NAMING, json!("SECOND-TEXT"))
        .expect("accepted");
    host.settle();
    let text = request_text(&mut host);
    assert!(text.contains("SECOND-TEXT"));
    assert!(!text.contains("FIRST-TEXT"));
}

/// REQ: F-045
#[test]
fn f_045_a_request_already_sent_keeps_the_instructions_it_started_with() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("FIRST-TEXT"))
        .expect("accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    host.settings()
        .set(ENTRY_NAMING, json!("SECOND-TEXT"))
        .expect("accepted");
    host.settle();
    let (_, sent) = host.inference_requests().remove(0);
    let text = sent
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("FIRST-TEXT"));
}

/// REQ: F-045
#[test]
fn f_045_the_instructions_do_not_change_the_shape_of_the_answer_the_plugin_accepts() {
    let mut host = activated_server();
    host.settings()
        .set(ENTRY_NAMING, json!("Answer with a poem."))
        .expect("accepted");
    host.settle();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    assert_eq!(renames(&host.submitted())[0].title, "Login fix");
}

/// REQ: F-045
#[test]
fn f_045_newlines_are_allowed_in_the_multi_line_input() {
    let mut host = activated_server();
    host.settings()
        .set(
            ENTRY_NAMING,
            json!("Keep titles under four words.\nUse the repository name first."),
        )
        .expect("accepted");
    host.settle();
    let text = request_text(&mut host);
    assert!(text.contains("Keep titles under four words.\nUse the repository name first."));
}

/// REQ: F-045
#[test]
fn f_045_every_documented_input_value_is_accepted_by_its_setting_key() {
    let mut host = activated_server();
    for key in [
        "entry_naming",
        "organization",
        "naming_and_organization",
        "project_naming",
        "smart_copy",
        "ask_for_update",
    ] {
        let result = host
            .settings()
            .set(&format!("inference.instructions.{key}"), json!("text"));
        assert!(result.is_ok(), "{key}: {result:?}");
    }
}

/// REQ: F-045
#[test]
fn f_045_the_instruction_text_area_declares_a_maximum_length_of_2000_in_the_page_sample() {
    let page = page_text();
    assert!(page.contains(
        "{ \"type\": \"text-area\", \"key\": \"instructions\", \"title\": \"Instructions\","
    ));
    assert!(page.contains("\"max_length\": 2000, \"default\": \"\" }"));
}

/// REQ: F-045
#[test]
fn f_045_the_page_sample_declares_a_name_panes_toggle_defaulting_to_true() {
    let page = page_text();
    assert!(page.contains(
        "{ \"type\": \"toggle\", \"key\": \"name_from_prompts\", \"title\": \"Name panes\","
    ));
    assert!(page.contains("\"default\": true }"));
}

/// REQ: F-045
#[test]
fn f_045_settings_changed_is_how_the_plugin_rereads_its_values() {
    let page = page_text();
    assert!(page.contains("re-reads its values on `settings-changed`"));
    assert!(page.contains("Re-read on every change so the next title uses the new text."));
}

/// REQ: F-045
#[test]
fn f_045_instructions_are_global_not_per_project() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc
        .contains("Instructions are added to new requests and saved globally (not per project)."));
    assert!(doc.contains("Empty inputs use the built-in defaults"));
}

/// REQ: F-045
#[test]
fn f_045_one_stored_value_serves_the_feature_tab_and_the_llm_instructions_tab() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(
        doc.contains("both locations edit one stored value, so there is no copy to keep in sync.")
    );
}

/// REQ: F-045
#[test]
fn f_045_editing_keys_are_enter_ctrl_s_esc_and_delete() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("Press `Ctrl+S` to apply. `Esc` cancels without changing anything."));
    assert!(doc.contains("select it and press `Delete`."));
    assert!(doc.contains(
        "Each row shows the first part of the current text, or \"Built-in default\" when empty."
    ));
}

/// REQ: F-045
#[test]
fn f_045_the_page_lists_seven_text_inputs_that_steer_titles() {
    assert!(
        page_text().contains("**Custom instruction inputs.** Seven text inputs steer the titles")
    );
}

/// REQ: F-045
#[test]
fn f_045_the_voice_instruction_is_stored_under_the_voice_table() {
    let page = pages_text("reference-config-and-data-migration.md");
    let row = table_row(&page, "| `llm-instructions` |");
    assert_eq!(row[3], "`[inference.instructions]`, `[voice]`");
    assert_eq!(
        row[6],
        "`[inference.instructions]` and `[voice] custom_prompt`"
    );
}
