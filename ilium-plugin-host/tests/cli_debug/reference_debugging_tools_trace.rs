//! reference-debugging-tools.md: the trace, the widget tree inspector, the profiler, the memory
//! view and the `--json` record catalogue (DEV-670..676, 680..684, 690..693, 700..702, 720..722).

use ilium_plugin_api::Side;
use ilium_plugin_host::cli::ProfileCall;
use ilium_plugin_host::telemetry::{
    check_unredacted, format_binary, redact_detail, render_inspector, render_memory_view,
    InspectNode, MemoryView, Profiler, TraceKind, TraceRing, TRACE_RING_CAPACITY,
};
use proptest::prelude::*;
use serde_json::{json, Value};
use std::time::Duration;

use crate::common::{
    env_with_session, error_code, record_types, records_of, run, scratch, FakeBackend,
};

fn ms_f(millis: f64) -> Duration {
    Duration::from_secs_f64(millis / 1000.0)
}

// ---------------------------------------------------------------- DEV-670

/// REQ: DEV-670
#[test]
fn dev_670_ring_capacity_is_1024() {
    assert_eq!(TRACE_RING_CAPACITY, 1_024);
}

/// REQ: DEV-670
#[test]
fn dev_670_ring_holds_1024_records() {
    let mut ring = TraceRing::new();
    ring.begin_instance();
    for n in 0..1_024u64 {
        ring.push(n, TraceKind::Event, json!({"count": 1}));
    }
    assert_eq!(ring.records().len(), 1_024);
}

/// REQ: DEV-670
#[test]
fn dev_670_the_1025th_record_drops_the_oldest() {
    let mut ring = TraceRing::new();
    ring.begin_instance();
    let first_seq = ring.push(0, TraceKind::Event, json!({}));
    for n in 1..=1_024u64 {
        ring.push(n, TraceKind::Event, json!({}));
    }
    let records = ring.records();
    assert_eq!(records.len(), 1_024);
    assert_eq!(records[0].seq, first_seq + 1);
    assert_eq!(records[0].time_ms, 1);
}

proptest! {
    // REQ: DEV-670
    #[test]
    fn dev_670_ring_keeps_the_newest_1024_in_order(count in 0usize..3_000) {
        let mut ring = TraceRing::new();
        ring.begin_instance();
        for n in 0..count {
            ring.push(n as u64, TraceKind::HostCall, json!({"n": n}));
        }
        let records = ring.records();
        prop_assert_eq!(records.len(), count.min(1_024));
        for pair in records.windows(2) {
            prop_assert_eq!(pair[1].seq, pair[0].seq + 1);
        }
        if count > 0 {
            prop_assert_eq!(records.last().map(|r| r.time_ms), Some(count as u64 - 1));
        }
    }
}

// ---------------------------------------------------------------- DEV-671

/// REQ: DEV-671
#[test]
fn dev_671_tracing_is_on_for_a_fresh_instance() {
    let mut ring = TraceRing::new();
    ring.begin_instance();
    ring.push(0, TraceKind::Lifecycle, json!({"what": "activate"}));
    assert_eq!(ring.records().len(), 1);
}

/// REQ: DEV-671
#[test]
fn dev_671_every_new_instance_traces_without_being_asked() {
    let mut ring = TraceRing::new();
    ring.begin_instance();
    ring.begin_instance();
    ring.push(0, TraceKind::Lifecycle, json!({"what": "activate"}));
    assert_eq!(ring.records().len(), 1);
}

// ---------------------------------------------------------------- DEV-672

/// REQ: DEV-672
#[test]
fn dev_672_payload_prints_as_redacted_with_its_byte_length() {
    let redacted = redact_detail(&json!({"payload": "hello"}));
    assert_eq!(redacted, json!({"payload": "<redacted 5 bytes>"}));
}

/// REQ: DEV-672
#[test]
fn dev_672_byte_length_counts_utf8_bytes() {
    let redacted = redact_detail(&json!({"payload": "€€"}));
    assert_eq!(redacted, json!({"payload": "<redacted 6 bytes>"}));
}

/// REQ: DEV-672
#[test]
fn dev_672_tags_names_counts_and_durations_are_never_redacted() {
    let detail = json!({"tag": "pane-created", "name": "storage.put", "count": 2, "duration_ms": 0.2, "intent": "set-decoration"});
    assert_eq!(redact_detail(&detail), detail);
}

/// REQ: DEV-672
#[test]
fn dev_672_nested_payloads_are_redacted_too() {
    let detail =
        json!({"events": [{"tag": "node-renamed", "payload": "abc"}, {"tag": "x", "payload": ""}]});
    let expected = json!({"events": [{"tag": "node-renamed", "payload": "<redacted 3 bytes>"}, {"tag": "x", "payload": "<redacted 0 bytes>"}]});
    assert_eq!(redact_detail(&detail), expected);
}

proptest! {
    // REQ: DEV-672
    #[test]
    fn dev_672_redaction_hides_every_payload_and_keeps_every_tag(payload in ".{0,40}", tag in "[a-z-]{1,12}") {
        let redacted = redact_detail(&json!({"tag": tag.clone(), "payload": payload.clone()}));
        prop_assert_eq!(redacted["tag"].as_str(), Some(tag.as_str()));
        let expected_payload = format!("<redacted {} bytes>", payload.len());
        prop_assert_eq!(redacted["payload"].as_str(), Some(expected_payload.as_str()));
    }
}

// ---------------------------------------------------------------- DEV-673

/// REQ: DEV-673
#[test]
fn dev_673_unredacted_works_for_a_dev_install() {
    assert!(check_unredacted(true).is_ok());
}

/// REQ: DEV-673
#[test]
fn dev_673_unredacted_is_refused_for_every_other_plugin_trace_unredacted_denied() {
    let refusal = check_unredacted(false).expect_err("not a dev install");
    assert_eq!(refusal.code.as_str(), "trace.unredacted-denied");
}

// ---------------------------------------------------------------- DEV-674

/// REQ: DEV-674
#[test]
fn dev_674_sequence_increases_by_one_per_record() {
    let mut ring = TraceRing::new();
    ring.begin_instance();
    for n in 0..10u64 {
        ring.push(100 + n, TraceKind::Event, json!({}));
    }
    let records = ring.records();
    for pair in records.windows(2) {
        assert_eq!(pair[1].seq, pair[0].seq + 1);
    }
}

/// REQ: DEV-674
#[test]
fn dev_674_record_carries_the_time_in_milliseconds() {
    let mut ring = TraceRing::new();
    ring.begin_instance();
    ring.push(120, TraceKind::Event, json!({}));
    ring.push(900, TraceKind::HostCall, json!({}));
    let times: Vec<u64> = ring.records().iter().map(|r| r.time_ms).collect();
    assert_eq!(times, vec![120, 900]);
}

/// REQ: DEV-674
#[test]
fn dev_674_kind_names_are_the_seven_documented_kinds() {
    let kinds = [
        TraceKind::Event,
        TraceKind::Submission,
        TraceKind::Outcome,
        TraceKind::HostCall,
        TraceKind::Render,
        TraceKind::Trap,
        TraceKind::Lifecycle,
    ];
    let names: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(
        names,
        [
            "event",
            "submission",
            "outcome",
            "host-call",
            "render",
            "trap",
            "lifecycle"
        ]
    );
}

// ---------------------------------------------------------------- DEV-676

/// REQ: DEV-676
#[test]
fn dev_676_restart_keeps_the_records_of_earlier_instances() {
    let mut ring = TraceRing::new();
    let first = ring.begin_instance();
    ring.push(1, TraceKind::Event, json!({}));
    ring.push(2, TraceKind::Trap, json!({"code": "trap.deadline"}));
    let second = ring.begin_instance();
    ring.push(3, TraceKind::Lifecycle, json!({"what": "activate"}));
    assert_ne!(first, second);
    let records = ring.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records.iter().filter(|r| r.instance == first).count(), 2);
    assert_eq!(records.iter().filter(|r| r.instance == second).count(), 1);
}

/// REQ: DEV-676
#[test]
fn dev_676_instance_numbers_count_up_from_one() {
    let mut ring = TraceRing::new();
    assert_eq!(ring.begin_instance(), 1);
    assert_eq!(ring.begin_instance(), 2);
    assert_eq!(ring.begin_instance(), 3);
}

// ---------------------------------------------------------------- inspector fixtures

fn node(
    kind: &str,
    key: Option<&str>,
    text: Option<&str>,
    style: Option<&str>,
    selected: bool,
    children: Vec<InspectNode>,
) -> InspectNode {
    InspectNode {
        kind: kind.to_owned(),
        key: key.map(str::to_owned),
        text: text.map(str::to_owned),
        style: style.map(str::to_owned),
        selected,
        children,
    }
}

/// The tree of the manual's inspector figure: 6 nodes, depth 3.
fn figure_tree() -> InspectNode {
    node(
        "stack",
        None,
        None,
        None,
        false,
        vec![
            node(
                "text",
                None,
                Some("Open notes: 3"),
                Some("title"),
                false,
                vec![],
            ),
            node(
                "list",
                None,
                None,
                None,
                false,
                vec![
                    node("row", Some("n1"), Some("Inbox"), None, false, vec![]),
                    node("row", Some("n2"), Some("Plans"), None, true, vec![]),
                ],
            ),
            node("button", Some("add"), Some("Add note"), None, false, vec![]),
        ],
    )
}

fn line_with<'a>(rendered: &'a str, needle: &str) -> &'a str {
    rendered
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in\n{rendered}"))
}

// ---------------------------------------------------------------- DEV-681

/// REQ: DEV-681
#[test]
fn dev_681_node_shows_kind_key_text_and_style() {
    let rendered = render_inspector("panel main", &figure_tree(), 4_096, 16);
    let text_line = line_with(&rendered, "Open notes: 3");
    assert!(
        text_line.contains("text")
            && text_line.contains("\"Open notes: 3\"")
            && text_line.contains("style=title")
    );
    let button_line = line_with(&rendered, "Add note");
    assert!(button_line.contains("button") && button_line.contains("key=add"));
}

/// REQ: DEV-681
#[test]
fn dev_681_node_without_a_key_shows_no_key() {
    let rendered = render_inspector("panel main", &figure_tree(), 4_096, 16);
    assert!(!line_with(&rendered, "Open notes: 3").contains("key="));
}

/// REQ: DEV-681
#[test]
fn dev_681_selected_rows_are_marked() {
    let rendered = render_inspector("panel main", &figure_tree(), 4_096, 16);
    assert!(line_with(&rendered, "Plans").contains("<- selected"));
    assert!(!line_with(&rendered, "Inbox").contains("selected"));
}

/// REQ: DEV-681
#[test]
fn dev_681_children_are_indented_below_their_parent() {
    let rendered = render_inspector("panel main", &figure_tree(), 4_096, 16);
    let indent = |needle: &str| {
        line_with(&rendered, needle)
            .chars()
            .skip(1)
            .take_while(|c| *c == ' ')
            .count()
    };
    assert!(indent("Inbox") > indent("list"), "{rendered}");
    assert!(indent("list") > indent("stack"), "{rendered}");
}

/// REQ: DEV-681
#[test]
fn dev_681_the_box_title_names_the_target() {
    let rendered = render_inspector("panel main", &figure_tree(), 4_096, 16);
    let first = rendered.lines().next().expect("first line");
    assert!(first.starts_with("╭ Widget tree: panel main "), "{first}");
}

// ---------------------------------------------------------------- DEV-682

/// REQ: DEV-682
#[test]
fn dev_682_footer_shows_node_count_and_depth_against_the_limits() {
    let rendered = render_inspector("panel main", &figure_tree(), 4_096, 16);
    let footer = line_with(&rendered, "nodes ");
    assert!(footer.contains("nodes 6 of 4096"), "{footer}");
    assert!(footer.contains("depth 3 of 16"), "{footer}");
}

/// REQ: DEV-682
#[test]
fn dev_682_footer_follows_the_given_limits() {
    let rendered = render_inspector("panel main", &figure_tree(), 2_000, 8);
    let footer = line_with(&rendered, "nodes ");
    assert!(
        footer.contains("nodes 6 of 2000") && footer.contains("depth 3 of 8"),
        "{footer}"
    );
}

/// REQ: DEV-682
#[test]
fn dev_682_single_node_tree_has_depth_1() {
    let single = node("text", None, Some("x"), None, false, vec![]);
    let rendered = render_inspector("panel main", &single, 4_096, 16);
    let footer = line_with(&rendered, "nodes ");
    assert!(
        footer.contains("nodes 1 of 4096") && footer.contains("depth 1 of 16"),
        "{footer}"
    );
}

// ---------------------------------------------------------------- DEV-684

/// REQ: DEV-684
#[test]
fn dev_684_rendering_the_inspector_leaves_the_tree_untouched() {
    let tree = figure_tree();
    let before = tree.clone();
    let first = render_inspector("panel main", &tree, 4_096, 16);
    let second = render_inspector("panel main", &tree, 4_096, 16);
    assert_eq!(tree, before);
    assert_eq!(first, second);
}

// ---------------------------------------------------------------- profiler

/// REQ: DEV-690
#[test]
fn dev_690_every_call_is_counted() {
    let mut profiler = Profiler::new();
    for _ in 0..10_000 {
        profiler.record("on-events", ms_f(0.5));
    }
    assert_eq!(profiler.timing("on-events").calls, 10_000);
}

/// REQ: DEV-690
#[test]
fn dev_690_two_runs_over_the_same_calls_give_the_same_result() {
    let mut first = Profiler::new();
    let mut second = Profiler::new();
    for n in 0..500 {
        first.record("render", ms_f(1.0 + (n % 9) as f64));
        second.record("render", ms_f(1.0 + (n % 9) as f64));
    }
    assert_eq!(first.timing("render"), second.timing("render"));
}

/// REQ: DEV-690
#[test]
fn dev_690_total_is_the_sum_of_all_calls_and_max_is_the_longest() {
    let mut profiler = Profiler::new();
    for _ in 0..99 {
        profiler.record("on-events", ms_f(5.0));
    }
    profiler.record("on-events", ms_f(31.2));
    let timing = profiler.timing("on-events");
    assert_eq!(timing.calls, 100);
    assert!(
        (timing.total_ms - 526.2).abs() < 0.001,
        "total {}",
        timing.total_ms
    );
    assert_eq!(timing.max_ms, Some(31.2));
    assert_eq!(timing.p50_ms, Some(5.0));
    assert_eq!(timing.p95_ms, Some(5.0));
}

/// REQ: DEV-691
#[test]
fn dev_691_on_events_over_budget_counts_calls_over_20_ms() {
    let mut profiler = Profiler::new();
    for millis in [19.999, 20.0, 20.001, 31.2] {
        profiler.record("on-events", ms_f(millis));
    }
    assert_eq!(profiler.timing("on-events").over_budget, 2);
}

/// REQ: DEV-691
#[test]
fn dev_691_render_over_budget_counts_calls_over_10_ms() {
    let mut profiler = Profiler::new();
    for millis in [9.999, 10.0, 10.001, 12.0] {
        profiler.record("render", ms_f(millis));
    }
    assert_eq!(profiler.timing("render").over_budget, 2);
}

/// REQ: DEV-691
#[test]
fn dev_691_a_call_of_15_ms_is_over_budget_for_render_but_not_for_on_events() {
    let mut profiler = Profiler::new();
    profiler.record("render", ms_f(15.0));
    profiler.record("on-events", ms_f(15.0));
    assert_eq!(profiler.timing("render").over_budget, 1);
    assert_eq!(profiler.timing("on-events").over_budget, 0);
}

/// REQ: DEV-692
#[test]
fn dev_692_export_without_calls_has_zero_calls_and_no_percentiles() {
    let profiler = Profiler::new();
    let timing = profiler.timing("on-events");
    assert_eq!(timing.calls, 0);
    assert_eq!(timing.p50_ms, None);
    assert_eq!(timing.p95_ms, None);
    assert_eq!(timing.max_ms, None);
    assert_eq!(timing.over_budget, 0);
}

// ---------------------------------------------------------------- memory view

fn mib(n: f64) -> u64 {
    (n * 1_048_576.0).round() as u64
}

fn sample_memory() -> MemoryView {
    MemoryView {
        side: Side::Server,
        linear_bytes: mib(6.2),
        peak_bytes: mib(9.8),
        limit_bytes: mib(32.0),
        storage_bytes: 14 * 1024,
        instances: 1,
        restarts: 0,
    }
}

/// REQ: DEV-700
#[test]
fn dev_700_view_shows_the_six_values() {
    let rendered = render_memory_view("example.notes", &sample_memory());
    assert!(
        line_with(&rendered, "linear memory").contains("6.2 MiB of 32 MiB"),
        "{rendered}"
    );
    assert!(
        line_with(&rendered, "peak").contains("9.8 MiB"),
        "{rendered}"
    );
    assert!(
        line_with(&rendered, "storage").contains("14 KiB of 1 MiB"),
        "{rendered}"
    );
    let instances = line_with(&rendered, "instances");
    assert!(
        instances.contains('1') && instances.contains("restarts 0"),
        "{rendered}"
    );
}

/// REQ: DEV-700
#[test]
fn dev_700_title_names_the_plugin_and_side() {
    let rendered = render_memory_view("example.notes", &sample_memory());
    assert!(rendered
        .lines()
        .next()
        .expect("title")
        .starts_with("╭ Memory: example.notes (server) "));
}

/// REQ: DEV-700
#[test]
fn dev_700_restart_and_instance_counts_follow_the_data() {
    let view = MemoryView {
        instances: 3,
        restarts: 2,
        ..sample_memory()
    };
    let rendered = render_memory_view("example.notes", &view);
    let instances = line_with(&rendered, "instances");
    assert!(
        instances.contains('3') && instances.contains("restarts 2"),
        "{instances}"
    );
}

/// REQ: DEV-701
#[test]
fn dev_701_peak_above_80_percent_adds_the_warning_line() {
    let view = MemoryView {
        peak_bytes: 8_388_609,
        limit_bytes: 10 * 1_048_576,
        ..sample_memory()
    };
    let rendered = render_memory_view("example.notes", &view);
    assert!(
        rendered
            .lines()
            .any(|line| line.trim() == "warning: peak above 80 percent of memory_mib"),
        "{rendered}"
    );
}

/// REQ: DEV-701
#[test]
fn dev_701_peak_at_exactly_80_percent_adds_no_warning() {
    let view = MemoryView {
        peak_bytes: 8_388_608,
        limit_bytes: 10 * 1_048_576,
        ..sample_memory()
    };
    assert!(!render_memory_view("example.notes", &view).contains("warning:"));
}

/// REQ: DEV-701
#[test]
fn dev_701_peak_well_below_the_limit_adds_no_warning() {
    assert!(!render_memory_view("example.notes", &sample_memory()).contains("warning:"));
}

/// REQ: DEV-702
#[test]
fn dev_702_values_use_binary_units() {
    assert_eq!(format_binary(1_024), "1 KiB");
    assert_eq!(format_binary(1_536), "1.5 KiB");
    assert_eq!(format_binary(1_048_576), "1 MiB");
    assert_eq!(format_binary(32 * 1_048_576), "32 MiB");
}

/// REQ: DEV-702
#[test]
fn dev_702_rendered_view_never_uses_decimal_units() {
    let rendered = render_memory_view("example.notes", &sample_memory());
    for unit in ["KB", "MB", "GB"] {
        assert!(!rendered.contains(unit), "{rendered}");
    }
}

// ---------------------------------------------------------------- json catalogue via CLI

const PLUGIN: &str = "example.notes";

fn backend_with_data() -> FakeBackend {
    let mut backend = FakeBackend::new(PLUGIN);
    let mut ring = TraceRing::new();
    ring.begin_instance();
    ring.push(120, TraceKind::Event, json!({"count": 2, "payload": "abc"}));
    ring.push(
        121,
        TraceKind::Submission,
        json!({"intent": "set-decoration", "corr": 17}),
    );
    ring.push(
        900,
        TraceKind::HostCall,
        json!({"name": "storage.put", "duration_ms": 0.2}),
    );
    backend.trace = ring.records();
    backend.tree = Some(figure_tree());
    backend.calls = vec![
        ProfileCall {
            export: "on-events".to_owned(),
            duration_ms: 4.5678,
        },
        ProfileCall {
            export: "on-events".to_owned(),
            duration_ms: 31.2,
        },
        ProfileCall {
            export: "render".to_owned(),
            duration_ms: 3.1,
        },
    ];
    backend.memory = vec![sample_memory()];
    backend
}

fn snake_case(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn collect_keys(value: &Value, keys: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                keys.push(key.clone());
                // Trace `detail` is plugin data (tags and names), not part of the record schema.
                if key != "detail" {
                    collect_keys(inner, keys);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_keys(item, keys)),
        _ => {}
    }
}

fn has_keys(record: &Value, wanted: &[&str]) -> bool {
    wanted.iter().all(|key| record.get(*key).is_some())
}

fn all_outputs() -> Vec<ilium_plugin_host::cli::CliOutput> {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    vec![
        run(&env, &["trace", PLUGIN, "--json"]),
        run(&env, &["profile", PLUGIN, "--json"]),
        run(&env, &["inspect", PLUGIN, "--view", "main", "--json"]),
        run(&env, &["inspect", PLUGIN, "--memory", "--json"]),
    ]
}

/// REQ: DEV-720
#[test]
fn dev_720_records_carry_the_documented_fields() {
    let outputs = all_outputs();
    let trace = records_of(&outputs[0], "trace");
    assert_eq!(trace.len(), 3);
    assert!(trace
        .iter()
        .all(|r| has_keys(r, &["seq", "time_ms", "kind", "instance", "detail"])));
    let timing = records_of(&outputs[1], "timing");
    assert!(!timing.is_empty(), "profile printed no timing record");
    assert!(timing.iter().all(|r| has_keys(
        r,
        &[
            "export",
            "calls",
            "total_ms",
            "p50_ms",
            "p95_ms",
            "max_ms",
            "over_budget"
        ]
    )));
    let tree = records_of(&outputs[2], "tree");
    assert_eq!(tree.len(), 1);
    assert!(has_keys(tree[0], &["target", "nodes", "depth", "root"]));
    let memory = records_of(&outputs[3], "memory");
    assert!(
        !memory.is_empty(),
        "inspect --memory printed no memory record"
    );
    assert!(memory.iter().all(|r| has_keys(
        r,
        &[
            "side",
            "linear_bytes",
            "peak_bytes",
            "limit_bytes",
            "storage_bytes",
            "instances",
            "restarts"
        ]
    )));
}

/// REQ: DEV-720
#[test]
fn dev_720_every_field_name_is_snake_case() {
    for output in all_outputs() {
        let mut keys = Vec::new();
        for record in &output.stdout_jsonl {
            collect_keys(record, &mut keys);
        }
        assert!(!keys.is_empty());
        for key in keys {
            assert!(snake_case(&key), "field {key:?} is not snake_case");
        }
    }
}

/// REQ: DEV-720
#[test]
fn dev_720_type_names_are_the_documented_ones() {
    let outputs = all_outputs();
    assert_eq!(record_types(&outputs[0])[0], "trace");
    assert_eq!(record_types(&outputs[1])[0], "timing");
    assert_eq!(record_types(&outputs[2])[0], "tree");
    assert_eq!(record_types(&outputs[3])[0], "memory");
}

/// REQ: DEV-721
#[test]
fn dev_721_durations_are_numbers_with_at_most_three_decimals() {
    let outputs = all_outputs();
    let timings = records_of(&outputs[1], "timing");
    assert!(!timings.is_empty());
    for record in timings {
        for field in ["total_ms", "p50_ms", "p95_ms", "max_ms"] {
            let value = record[field]
                .as_f64()
                .unwrap_or_else(|| panic!("{field} is not a number: {record}"));
            let scaled = value * 1000.0;
            assert!(
                (scaled - scaled.round()).abs() < 1e-6,
                "{field} = {value} has more than 3 decimals"
            );
        }
    }
}

/// REQ: DEV-721
#[test]
fn dev_721_sizes_are_integers_in_bytes() {
    let outputs = all_outputs();
    let memory = records_of(&outputs[3], "memory");
    assert_eq!(memory.len(), 1);
    for field in ["linear_bytes", "peak_bytes", "limit_bytes", "storage_bytes"] {
        assert!(
            memory[0][field].as_u64().is_some(),
            "{field} must be an integer: {}",
            memory[0]
        );
    }
    assert_eq!(memory[0]["limit_bytes"].as_u64(), Some(32 * 1_048_576));
    assert_eq!(memory[0]["storage_bytes"].as_u64(), Some(14 * 1024));
}

/// REQ: DEV-722
#[test]
fn dev_722_summary_and_result_come_last() {
    for output in all_outputs() {
        let types = record_types(&output);
        assert!(types.len() >= 3);
        assert_eq!(types[types.len() - 2..], ["summary", "result"], "{types:?}");
        assert_eq!(types.iter().filter(|t| *t == "summary").count(), 1);
        assert_eq!(types.iter().filter(|t| *t == "result").count(), 1);
    }
}

/// REQ: DEV-722
#[test]
fn dev_722_trace_records_are_in_time_order() {
    let outputs = all_outputs();
    let times: Vec<u64> = records_of(&outputs[0], "trace")
        .iter()
        .map(|r| r["time_ms"].as_u64().unwrap_or(u64::MAX))
        .collect();
    assert_eq!(times, vec![120, 121, 900]);
    let seqs: Vec<u64> = records_of(&outputs[0], "trace")
        .iter()
        .map(|r| r["seq"].as_u64().unwrap_or(0))
        .collect();
    assert!(seqs.windows(2).all(|pair| pair[1] == pair[0] + 1));
}

/// REQ: DEV-722
#[test]
fn dev_722_result_exit_code_equals_the_process_exit_code() {
    for output in all_outputs() {
        let result = records_of(&output, "result");
        assert_eq!(
            result[0]["exit_code"].as_i64(),
            Some(i64::from(output.exit_code))
        );
        assert_eq!(result[0]["ok"].as_bool(), Some(output.exit_code == 0));
    }
}

/// REQ: DEV-720
#[test]
fn dev_720_unknown_plugin_error_record_keeps_the_error_fields() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let output = run(&env, &["trace", "example.unknown", "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("plugin.not-found"));
    let error = records_of(&output, "error");
    assert!(has_keys(error[0], &["code", "message"]));
}

// ---------------------------------------------------------------- remaining ids

/// REQ: DEV-675
#[test]
fn dev_675_record_writes_the_replay_file_and_prints_an_artifact_record() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let file = scratch.path().join("trace.replay");
    let output = run(
        &env,
        &[
            "trace",
            PLUGIN,
            "--record",
            file.to_str().expect("utf8"),
            "--json",
        ],
    );
    assert!(file.exists());
    let artifacts = records_of(&output, "artifact");
    assert_eq!(artifacts[0]["kind"], json!("replay"));
}

/// REQ: DEV-675
#[test]
fn dev_675_replay_file_is_not_empty_for_a_trace_with_events() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let file = scratch.path().join("trace.replay");
    run(
        &env,
        &[
            "trace",
            PLUGIN,
            "--record",
            file.to_str().expect("utf8"),
            "--json",
        ],
    );
    assert!(std::fs::metadata(&file)
        .map(|m| m.len() > 0)
        .unwrap_or(false));
}

/// REQ: DEV-680
#[test]
fn dev_680_inspector_prints_the_last_tree_of_the_target() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let output = run(&env, &["inspect", PLUGIN, "--view", "main"]);
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout_text.contains("Open notes: 3"));
}

/// REQ: DEV-680
#[test]
fn dev_680_never_rendered_target_is_inspect_no_render() {
    let scratch = scratch();
    let mut backend = backend_with_data();
    backend.tree = None;
    let env = env_with_session(scratch.path(), backend);
    let output = run(&env, &["inspect", PLUGIN, "--view", "main", "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("inspect.no-render"));
}

/// REQ: DEV-683
#[test]
fn dev_683_json_prints_one_tree_record_with_nested_children() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let output = run(&env, &["inspect", PLUGIN, "--view", "main", "--json"]);
    let trees = records_of(&output, "tree");
    assert_eq!(trees.len(), 1);
    assert_eq!(
        trees[0]["root"]["children"][1]["children"][1]["key"],
        json!("n2")
    );
}

/// REQ: DEV-693
#[test]
fn dev_693_profiling_adds_less_than_5_percent_to_a_call() {
    let mut profiler = Profiler::new();
    let calls = 100;
    for _ in 0..calls {
        profiler.record("render", ms_f(10.0));
    }
    // the profiler must have recorded every call, otherwise a zero overhead proves nothing
    let timing = profiler.timing("render");
    assert_eq!(timing.calls, calls);
    assert!(
        (timing.total_ms - 1000.0).abs() < 1e-6,
        "{}",
        timing.total_ms
    );
    let overhead = profiler.overhead_fraction();
    assert!(overhead >= 0.0, "overhead {overhead}");
    assert!(overhead < 0.05, "overhead {overhead}");
}

/// REQ: DEV-725
#[test]
fn dev_725_replay_reproduces_the_recorded_deadline_trap() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let file = scratch.path().join("slow.replay");
    std::fs::write(
        &file,
        r#"{"call":"on-events","host_call_ms":[600],"deadline_ms":500}"#,
    )
    .expect("replay");
    let output = run(
        &env,
        &["test", "--replay", file.to_str().expect("utf8"), "--json"],
    );
    let trap = records_of(&output, "error");
    assert!(
        trap.iter().any(|r| r["code"] == "trap.deadline"),
        "{:?}",
        output.stdout_jsonl
    );
    // manual does not pin the exit code of a replayed deadline trap (DEV-544 pins 0 and 3 only)
    assert_ne!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
}

/// REQ: DEV-683
#[test]
fn dev_683_the_tree_record_carries_a_root_object() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let output = run(&env, &["inspect", PLUGIN, "--view", "main", "--json"]);
    let trees = records_of(&output, "tree");
    assert_eq!(trees.len(), 1);
    assert!(
        trees[0]["root"].is_object(),
        "the single tree record must hold its root node as an object: {}",
        trees[0]
    );
}

/// REQ: DEV-684
#[test]
fn dev_684_rendering_at_another_size_still_leaves_the_tree_untouched() {
    let tree = figure_tree();
    let before = tree.clone();
    let _ = render_inspector("panel main", &tree, 800, 4);
    assert_eq!(tree, before, "a different viewport must not change the tree");
}

/// REQ: DEV-725
#[test]
fn dev_725_a_replayed_call_inside_its_deadline_raises_no_deadline_trap() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_data());
    let file = scratch.path().join("fast.replay");
    std::fs::write(
        &file,
        r#"{"call":"on-events","host_call_ms":[100],"deadline_ms":500}"#,
    )
    .expect("replay");
    let output = run(
        &env,
        &["test", "--replay", file.to_str().expect("utf8"), "--json"],
    );
    let errors = records_of(&output, "error");
    assert!(
        !errors.iter().any(|record| record["code"] == "trap.deadline"),
        "a call that met its deadline must not replay as a deadline trap: {:?}",
        output.stdout_jsonl
    );
}
