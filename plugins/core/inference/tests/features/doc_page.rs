//! The page itself as a spec: tables of extension points, tabs, services and topics, the
//! traceability rows, the kernel boot rows and the security marks (doc_ tests).

use crate::common::{manual_page, page_text, section, table_rows};

fn point_rows() -> Vec<Vec<String>> {
    table_rows(&section(&page_text(), "### Extension points"))
}

fn row_of(point: &str) -> Vec<String> {
    point_rows()
        .into_iter()
        .find(|row| row[0] == point)
        .unwrap_or_else(|| panic!("row {point}"))
}

fn traceability_row(id: &str) -> Vec<String> {
    let page = manual_page("reference-feature-traceability.md");
    let line = page
        .lines()
        .find(|line| line.starts_with(&format!("| {id} |")))
        .expect("row");
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().replace('`', ""))
        .collect()
}

/// REQ: F-084
#[test]
fn doc_084_the_page_names_exactly_three_extension_points() {
    let names: Vec<String> = point_rows().into_iter().map(|row| row[0].clone()).collect();
    assert_eq!(
        names,
        ["before-request", "after-response", "provider-profiles"]
    );
}

/// REQ: F-084
#[test]
fn doc_084_before_request_is_a_transform_hook_that_rewrites_or_refuses() {
    let row = row_of("before-request");
    assert_eq!(row[1], "hook (transform)");
    assert!(row[2].starts_with("Rewrite or refuse an outgoing model request"));
}

/// REQ: F-084
#[test]
fn doc_084_before_request_payload_and_reply_fields() {
    let row = row_of("before-request");
    assert_eq!(
        row[3],
        "purpose, provider, model, messages_length, text; reply: text"
    );
}

/// REQ: F-084
#[test]
fn doc_084_after_response_is_an_observe_hook_with_token_counts() {
    let row = row_of("after-response");
    assert_eq!(row[1], "hook (observe)");
    assert_eq!(
        row[3],
        "purpose, provider, model, input_tokens, output_tokens, millis"
    );
}

/// REQ: F-084
#[test]
fn doc_084_provider_profiles_is_a_collect_point_with_five_entry_fields() {
    let row = row_of("provider-profiles");
    assert_eq!(row[1], "collect");
    assert_eq!(row[3], "entry: id, title, base_url, models_path, key_name");
}

/// REQ: F-084, SEC-293
#[test]
fn doc_293_before_request_risk_is_high_like_the_security_table() {
    assert_eq!(row_of("before-request")[4], "high");
    let security = manual_page("reference-extension-security.md");
    let line = security
        .lines()
        .find(|line| line.starts_with("| `ilium.core.inference/before-request` |"))
        .expect("security row");
    assert!(line.contains("| high |"));
}

/// REQ: F-084, SEC-293
#[test]
fn doc_293_provider_profiles_risk_equals_the_corrected_security_table() {
    let security = manual_page("reference-extension-security.md");
    let line = security
        .lines()
        .find(|line| line.starts_with("| `ilium.core.inference/provider-profiles` |"))
        .expect("security row");
    let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
    assert_eq!(row_of("provider-profiles")[4], cells[1]);
}

/// REQ: F-084, SEC-293
#[test]
fn doc_293_page_risk_marks_are_only_standard_or_high() {
    for row in point_rows() {
        assert!(row[4] == "standard" || row[4] == "high", "{row:?}");
    }
}

/// REQ: F-084
#[test]
fn doc_084_the_page_offers_the_complete_service_and_the_finished_topic() {
    let page = page_text();
    assert!(page.contains("`ilium.core.inference/complete`: complete: one-shot completion through the chosen provider"));
    assert!(page.contains("`ilium.core.inference/request-finished`: { purpose, provider, millis }"));
}

/// REQ: F-084
#[test]
fn doc_084_the_owner_and_contributor_grants_use_the_extension_names() {
    let page = page_text();
    assert!(page.contains("`extensions.provide:ilium.core.inference/<point>`"));
    assert!(page.contains("`extensions.contribute:ilium.core.inference/<point>`"));
}

/// REQ: F-084
#[test]
fn doc_084_the_plugin_is_replaceable_by_a_package_that_provides_the_whole_surface() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("\"replaces\": [\"ilium.core.inference\"]"));
}

/// REQ: COR-030, COR-033, F-085
#[test]
fn doc_030_the_page_lists_the_two_kernel_tabs_with_their_config_tables() {
    let rows = table_rows(&section(&page_text(), "### Kernel tabs this plugin owns"));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][..3], ["inference", "Inference", "[inference]"]);
    assert_eq!(
        rows[1][..3],
        [
            "llm-instructions",
            "LLM Instructions",
            "[inference.instructions], [voice]"
        ]
    );
}

/// REQ: COR-030, F-085
#[test]
fn doc_030_the_readers_of_the_inference_tab_are_the_six_consumers() {
    let rows = table_rows(&section(&page_text(), "### Kernel tabs this plugin owns"));
    assert_eq!(
        rows[0][3],
        "Titles, Recap, Search, Smart copy, Compaction, Voice"
    );
    assert_eq!(rows[1][3], "Titles, Recap, Smart copy, Voice");
}

/// REQ: COR-033, F-085
#[test]
fn doc_034_keys_written_by_the_inference_tab_exclude_title_style_and_instructions() {
    let migration = manual_page("reference-config-and-data-migration.md");
    let line = migration
        .lines()
        .find(|line| line.starts_with("| `inference` | Inference |"))
        .expect("row");
    assert!(line.ends_with("Every `[inference]` key except `title_style` and `instructions` |"));
}

/// REQ: COR-033, F-085
#[test]
fn doc_034_keys_written_by_the_llm_instructions_tab_are_the_table_and_the_voice_prompt() {
    let migration = manual_page("reference-config-and-data-migration.md");
    let line = migration
        .lines()
        .find(|line| line.starts_with("| `llm-instructions` |"))
        .expect("row");
    assert!(line.ends_with("`[inference.instructions]` and `[voice] custom_prompt` |"));
}

/// REQ: COR-022, F-085
#[test]
fn doc_022_the_disabled_notice_keeps_the_values_and_names_the_plugin() {
    assert!(page_text().contains("\"Inference providers is disabled. Values are kept.\""));
}

/// REQ: COR-022
#[test]
fn doc_022_the_boot_page_says_ai_features_fall_back_and_provider_settings_stay() {
    let boot = manual_page("reference-kernel-and-boot.md");
    let line = boot
        .lines()
        .find(|line| line.starts_with("| Inference | AI features report"))
        .expect("row");
    assert!(line.contains("`inference.unavailable`"));
    assert!(line.contains("non-AI path"));
    assert!(line.contains("Provider settings stay"));
}

/// REQ: F-084
#[test]
fn doc_084_traceability_row_is_a_server_plugin_with_the_inference_table() {
    let row = traceability_row("F-084");
    assert_eq!(row[1], "AI providers");
    assert_eq!(row[3], "plugin-wasm");
    assert_eq!(row[4], "inference plugin");
    assert_eq!(row[5], "server");
    assert_eq!(row[7], "[inference]");
}

/// REQ: F-085
#[test]
fn doc_085_traceability_row_keeps_the_gate_in_the_kernel_and_the_ui_in_the_plugin() {
    let row = traceability_row("F-085");
    assert_eq!(row[3], "native-kernel");
    assert_eq!(row[4], "kernel gate; inference plugin UI");
    assert_eq!(row[5], "both");
    assert_eq!(row[7], "[inference], [onboarding]");
}

/// REQ: F-085
#[test]
fn doc_085_the_page_keeps_the_privacy_controls_in_the_kernel() {
    let page = page_text();
    assert!(page.contains("The privacy controls stay in the kernel."));
    assert!(page.contains("so this plugin cannot widen what is sent."));
}

/// REQ: F-084
#[test]
fn doc_084_the_five_samples_of_the_page_are_present_with_both_languages() {
    let page = page_text();
    for heading in [
        "#### 1. An OpenAI-compatible provider",
        "#### 2. Stream tokens",
        "#### 3. Models as a catalogue",
        "#### 4. Test provider",
        "#### 5. Provider picker panel",
    ] {
        let text = section(&page, heading);
        assert!(
            text.contains("**Rust**") && text.contains("**TypeScript**"),
            "{heading}"
        );
    }
}
