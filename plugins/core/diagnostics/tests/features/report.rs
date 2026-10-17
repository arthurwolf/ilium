//! The service `ilium.core.diagnostics/report` (method `build`): the page sample "Build the report in a
//! service", the 64 KiB limit for each section, the 256 KiB limit for the result, the `Plugins` section
//! (LIF-138) and the log tail option.

use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::json;

use crate::common::{
    build_report, call, fail_section, provide_section, session, stall_section, write_log,
    PLUGIN_ID, REPORT_SERVICE,
};

const SECTION_LIMIT: usize = 64 * 1024;
const REPORT_LIMIT: usize = 256 * 1024;

fn failure(code: ErrorCode) -> PluginError {
    PluginError {
        code,
        message: "section failed".to_string(),
        retry_after_ms: None,
    }
}

/// REQ: COR-050
#[test]
fn cor_050_report_starts_with_the_ilium_facts_block() {
    let mut session = session();
    let text = build_report(&mut session, false).expect("report builds");
    assert_eq!(text.lines().next(), Some("Ilium"));
}

/// REQ: COR-050
#[test]
fn cor_050_report_core_facts_hold_version_and_platform_lines() {
    let mut session = session();
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains("Version"), "{text}");
    assert!(text.contains("Platform"), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_report_without_contributors_has_no_contributed_section_heading() {
    let mut session = session();
    let text = build_report(&mut session, false).expect("report builds");
    assert!(!text.contains("(from "), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_one_contributed_section_is_named_with_its_source_and_followed_by_its_text() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        "Renderer: software",
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(
        text.contains("\n## GPU (from example.gpu)\nRenderer: software\n"),
        "{text}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_two_contributed_sections_both_appear() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        "Renderer: software",
    );
    provide_section(&mut session, "example.disk", "disk", "Disk", "Free: 12 GiB");
    let text = build_report(&mut session, false).expect("report builds");
    assert!(
        text.contains("## GPU (from example.gpu)\nRenderer: software\n"),
        "{text}"
    );
    assert!(
        text.contains("## Disk (from example.disk)\nFree: 12 GiB\n"),
        "{text}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_a_section_text_of_65535_characters_is_kept_whole() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        &"a".repeat(SECTION_LIMIT - 1),
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains(&format!(
        "(from example.gpu)\n{}\n",
        "a".repeat(SECTION_LIMIT - 1)
    )));
}

/// REQ: COR-050
#[test]
fn cor_050_a_section_text_of_65536_characters_is_kept_whole() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        &"a".repeat(SECTION_LIMIT),
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains(&format!(
        "(from example.gpu)\n{}\n",
        "a".repeat(SECTION_LIMIT)
    )));
}

/// REQ: COR-050
#[test]
fn cor_050_a_section_text_of_65537_characters_is_cut_to_65536() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        &"a".repeat(SECTION_LIMIT + 1),
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains(&format!(
        "(from example.gpu)\n{}\n",
        "a".repeat(SECTION_LIMIT)
    )));
    assert!(!text.contains(&"a".repeat(SECTION_LIMIT + 1)));
}

/// REQ: COR-050
#[test]
fn cor_050_the_cut_of_a_section_counts_characters_not_bytes() {
    let mut session = session();
    provide_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        &"\u{e9}".repeat(SECTION_LIMIT + 5),
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains(&format!(
        "(from example.gpu)\n{}\n",
        "\u{e9}".repeat(SECTION_LIMIT)
    )));
}

/// REQ: COR-050
#[test]
fn cor_050_a_failing_section_is_named_and_marked_not_available() {
    let mut session = session();
    fail_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        failure(ErrorCode::Internal),
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains("\n## GPU\n(not available)\n"), "{text}");
    assert!(!text.contains("## GPU (from"), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_a_failing_section_does_not_stop_the_others() {
    let mut session = session();
    fail_section(
        &mut session,
        "example.gpu",
        "gpu",
        "GPU",
        failure(ErrorCode::NotPermitted),
    );
    provide_section(&mut session, "example.disk", "disk", "Disk", "Free: 12 GiB");
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains("## GPU\n(not available)\n"), "{text}");
    assert!(
        text.contains("## Disk (from example.disk)\nFree: 12 GiB\n"),
        "{text}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_a_section_whose_service_is_missing_is_not_available_not_found() {
    let mut session = session();
    session.contribute(
        "example.gpu",
        "ilium.core.diagnostics/report-sections",
        "gpu",
        50,
        json!({ "title": "GPU", "service": "example.gpu/missing" }),
    );
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains("## GPU\n(not available)\n"), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_a_slow_section_is_named_and_skipped_after_2000_ms() {
    let mut session = session();
    stall_section(&mut session, "example.gpu", "gpu", "GPU");
    provide_section(&mut session, "example.disk", "disk", "Disk", "Free: 12 GiB");
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.contains("## GPU\n(not available)\n"), "{text}");
    assert!(text.contains("## Disk (from example.disk)"), "{text}");
}

/// REQ: COR-050
#[test]
fn cor_050_the_result_is_cut_at_262144_characters() {
    let mut session = session();
    for name in ["a", "b", "c", "d", "e"] {
        provide_section(
            &mut session,
            &format!("example.{name}"),
            name,
            &name.to_uppercase(),
            &"x".repeat(SECTION_LIMIT),
        );
    }
    let text = build_report(&mut session, false).expect("report builds");
    assert_eq!(text.chars().count(), REPORT_LIMIT);
}

/// REQ: COR-050
#[test]
fn cor_050_a_result_below_the_limit_is_not_padded_or_cut() {
    let mut session = session();
    provide_section(&mut session, "example.gpu", "gpu", "GPU", "short");
    let text = build_report(&mut session, false).expect("report builds");
    assert!(text.chars().count() < REPORT_LIMIT);
    assert!(text.ends_with("short\n"), "{text:?}");
}

/// REQ: COR-050
#[test]
fn cor_050_the_service_result_object_has_the_text_key() {
    let mut session = session();
    let reply = call(&mut session, REPORT_SERVICE, json!({ "method": "build" }))
        .expect("build without params");
    assert!(reply["text"].is_string());
}

/// REQ: COR-050
#[test]
fn cor_050_the_log_tail_is_added_only_when_include_log_tail_is_true() {
    let mut session = session();
    write_log(
        &mut session,
        "log-2026-10-12 09-14-03.txt",
        b"2026-10-12 09:14:03 WARN disk almost full\n",
    );
    let without = build_report(&mut session, false).expect("report builds");
    let with = build_report(&mut session, true).expect("report builds");
    assert!(!without.contains("disk almost full"), "{without}");
    assert!(with.contains("disk almost full"), "{with}");
}

/// REQ: COR-050
#[test]
fn cor_050_include_log_tail_of_the_wrong_type_is_refused_invalid_argument() {
    let mut session = session();
    let error = call(
        &mut session,
        REPORT_SERVICE,
        json!({ "method": "build", "include_log_tail": "yes" }),
    )
    .expect_err("wrong type");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: COR-050
#[test]
fn cor_050_an_unknown_method_is_refused_not_found() {
    let mut session = session();
    let error = call(&mut session, REPORT_SERVICE, json!({ "method": "export" }))
        .expect_err("unknown method");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: LIF-138
#[test]
fn lif_138_a_degraded_plugin_appears_in_the_plugins_section_with_its_health() {
    let mut session = session();
    for _ in 0..5 {
        session
            .plugin("ilium.core.kanban")
            .breach_deadline("handler.on-events");
    }
    let text = build_report(&mut session, false).expect("report builds");
    let plugins = text.split("Plugins").nth(1).expect("Plugins section");
    assert!(plugins.contains("ilium.core.kanban"), "{text}");
    assert!(plugins.contains("degraded"), "{text}");
}

/// REQ: LIF-138
#[test]
fn lif_138_a_healthy_plugin_is_not_listed_in_the_plugins_section() {
    let mut session = session();
    for _ in 0..5 {
        session
            .plugin("ilium.core.kanban")
            .breach_deadline("handler.on-events");
    }
    let text = build_report(&mut session, false).expect("report builds");
    let plugins = text.split("Plugins").nth(1).expect("Plugins section");
    assert!(!plugins.contains(PLUGIN_ID), "{text}");
}

/// REQ: LIF-138
#[test]
fn lif_138_with_every_plugin_healthy_the_plugins_section_lists_no_plugin() {
    let mut session = session();
    let text = build_report(&mut session, false).expect("report builds");
    let plugins = text.split("Plugins").nth(1).expect("Plugins section");
    assert!(!plugins.contains("degraded"), "{text}");
    assert!(!plugins.contains("failed"), "{text}");
}
