//! Extension points `log-sinks` and `report-sections`, the report service row, the example contributor
//! and the security rules that touch them (EXT-054, SEC-249).

use serde_json::{json, Value};

use crate::common::{
    build_report, fenced_block_after, manifest_json, page, page_text, provide_section, session,
    table_row, write_log,
};

fn points() -> Vec<Value> {
    manifest_json()["contributes"]["extension_points"]
        .as_array()
        .expect("points")
        .clone()
}

fn point(id: &str) -> Value {
    points()
        .into_iter()
        .find(|point| point["id"] == id)
        .unwrap_or_else(|| panic!("point {id}"))
}

/// REQ: COR-050
#[test]
fn cor_050_the_plugin_owns_exactly_two_points_in_page_order() {
    let ids: Vec<String> = points()
        .iter()
        .map(|point| point["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["log-sinks", "report-sections"]);
}

/// REQ: COR-050
#[test]
fn cor_050_log_sinks_is_an_observe_hook_with_the_documented_limits() {
    let sinks = point("log-sinks");
    assert_eq!(sinks["kind"], "hook");
    assert_eq!(sinks["mode"], "observe");
    assert_eq!(sinks["risk"], "standard");
    assert_eq!(sinks["deadline_ms"], 20);
    assert_eq!(sinks["max_handlers"], 8);
    assert_eq!(sinks["version"], "0.1.0");
    assert_eq!(sinks["title"], "Log sinks");
}

/// REQ: COR-050
#[test]
fn cor_050_log_sinks_payload_has_five_string_fields_and_levels_warn_and_error_only() {
    let schema = &point("log-sinks")["payload_schema"];
    let properties = schema["properties"].as_object().expect("properties");
    let mut names: Vec<&String> = properties.keys().collect();
    names.sort();
    assert_eq!(names, ["level", "message", "plugin", "target", "time"]);
    assert_eq!(properties["level"]["enum"], json!(["warn", "error"]));
    for name in ["plugin", "target", "message", "time"] {
        assert_eq!(properties[name]["type"], "string", "{name}");
    }
}

/// REQ: COR-050
#[test]
fn cor_050_report_sections_is_a_collect_point_with_at_most_32_static_entries() {
    let sections = point("report-sections");
    assert_eq!(sections["kind"], "collect");
    assert_eq!(sections["max_entries"], 32);
    assert_eq!(sections["dynamic"], false);
    assert_eq!(sections["version"], "0.1.0");
    assert_eq!(sections["title"], "Report sections");
}

/// REQ: COR-050
#[test]
fn cor_050_report_sections_entries_require_id_title_and_service() {
    let schema = &point("report-sections")["entry_schema"];
    assert_eq!(schema["required"], json!(["id", "title", "service"]));
    for name in ["id", "title", "service"] {
        assert_eq!(schema["properties"][name]["type"], "string", "{name}");
    }
}

/// REQ: COR-050
#[test]
fn cor_050_the_page_table_lists_both_points_with_kind_and_risk() {
    let text = page_text();
    let sinks = table_row(&text, "| `log-sinks` |");
    assert_eq!(sinks[1], "hook (observe)");
    assert_eq!(sinks[4], "high");
    let sections = table_row(&text, "| `report-sections` |");
    assert_eq!(sections[1], "collect");
    assert_eq!(sections[4], "standard");
}

/// REQ: COR-050
#[test]
fn cor_050_the_page_payload_cells_name_the_documented_fields() {
    let text = page_text();
    let sinks = table_row(&text, "| `log-sinks` |")[3].clone();
    assert!(sinks.contains("level, plugin, target, message, time"));
    assert!(sinks.contains("records at warn and above only"));
    let sections = table_row(&text, "| `report-sections` |")[3].clone();
    assert!(sections.contains("entry: id, title, service (returns text, at most 64 KiB)"));
}

/// REQ: COR-050
#[test]
fn cor_050_the_page_kinds_agree_with_the_core_points_table() {
    let core = page("reference-core-extension-points.md");
    assert_eq!(
        table_row(&core, "| `ilium.core.diagnostics/log-sinks` |")[1],
        "hook: observe"
    );
    assert_eq!(
        table_row(&core, "| `ilium.core.diagnostics/report-sections` |")[1],
        "collect"
    );
    assert_eq!(
        table_row(&core, "| `ilium.core.diagnostics/log-sinks` |")[2],
        "high"
    );
    assert_eq!(
        table_row(&core, "| `ilium.core.diagnostics/report-sections` |")[2],
        "standard"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_core_services_table_lists_the_report_build_row() {
    let core = page("reference-core-extension-points.md");
    assert_eq!(
        table_row(&core, "| `ilium.core.diagnostics/report` |")[1],
        "build: build the diagnostics report"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_page_lists_the_service_and_replaceable_yes() {
    let text = page_text();
    assert!(text.contains("`ilium.core.diagnostics/report`: build: build the diagnostics report"));
    assert!(text.contains("**Replaceable:** yes."));
}

/// REQ: COR-050
#[test]
fn cor_050_the_example_contributor_adds_a_gpu_section_with_priority_50() {
    let example: Value = serde_json::from_str(&fenced_block_after(
        &page_text(),
        "#### Example: Adds GPU facts to the report",
        "json",
    ))
    .expect("example is JSON");
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.diagnostics/report-sections");
    assert_eq!(entry["id"], "gpu");
    assert_eq!(entry["priority"], 50);
    assert_eq!(
        entry["entry"],
        json!({ "title": "GPU", "service": "example.gpu/report" })
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_example_contributor_holds_the_contribute_grant_for_the_point() {
    let example: Value = serde_json::from_str(&fenced_block_after(
        &page_text(),
        "#### Example: Adds GPU facts to the report",
        "json",
    ))
    .expect("example is JSON");
    assert_eq!(
        example["capabilities"]["server"],
        json!(["extensions.contribute:ilium.core.diagnostics/report-sections"])
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_example_entry_satisfies_the_entry_schema_of_the_point() {
    let example: Value = serde_json::from_str(&fenced_block_after(
        &page_text(),
        "#### Example: Adds GPU facts to the report",
        "json",
    ))
    .expect("example is JSON");
    let entry = &example["contributes"]["extensions"][0]["entry"];
    let schema = &point("report-sections")["entry_schema"];
    for required in schema["required"].as_array().unwrap() {
        let name = required.as_str().unwrap();
        // The id of the entry is the contribution id; title and service are inside the entry value.
        if name != "id" {
            assert!(entry.get(name).is_some(), "entry lacks {name}");
        }
    }
}

/// REQ: EXT-054
#[test]
fn ext_054_the_requirement_row_sets_a_250_ms_batch_of_up_to_64_records() {
    let requirements = page("reference-requirements-extension-points.md");
    let row = requirements
        .lines()
        .find(|line| line.starts_with("| [EXT-054]"))
        .expect("EXT-054 row");
    assert!(row.contains("one chain per 250 ms with up to 64 records"));
    assert!(row.contains("MUST NOT be delivered to log-sinks"));
}

/// REQ: SEC-249
#[test]
fn sec_249_a_secret_in_a_contributed_section_is_redacted_in_the_report() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        "token=sk-live-0123456789abcdef",
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(!text.contains("sk-live-0123456789abcdef"), "{text}");
    assert!(text.contains("## GPU (from example.gpu)"));
}

/// REQ: SEC-249
#[test]
fn sec_249_a_secret_in_the_log_tail_is_redacted_in_the_report() {
    let mut session = session();
    write_log(
        &mut session,
        "log-2026-10-12 09-14-03.txt",
        b"2026-10-12 09:14:03 WARN Authorization: Bearer abc.def.ghi\n",
    );
    let text = build_report(&mut session, true).expect("report builds");
    assert!(!text.contains("abc.def.ghi"), "{text}");
}

/// REQ: SEC-249
#[test]
fn sec_249_the_security_page_row_names_the_report_sections_point() {
    let secrets = page("reference-secrets.md");
    assert!(secrets.contains("**[SEC-249]** Diagnostics reports (`ilium.core.diagnostics/report-sections`) pass through redaction before they are shown or saved."));
}
