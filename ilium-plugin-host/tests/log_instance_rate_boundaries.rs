use ilium_plugin_api::Side;
use ilium_plugin_host::telemetry::{LogConfig, LogLevel, PluginLog, PluginLogInstanceBudget};

const AT: &str = "2026-10-23 12:00:00";

fn writer(path: &std::path::Path) -> PluginLog {
    PluginLog::open(
        path,
        "example.notes",
        LogConfig {
            threshold: LogLevel::Info,
            rate_per_second: 1,
        },
    )
    .unwrap()
}

#[test]
fn shared_instance_budget_spans_writer_handles_and_summarizes_once() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = writer(directory.path());
    let mut second = writer(directory.path());
    let mut budget = PluginLogInstanceBudget::new(7, Side::Server).unwrap();
    assert_eq!(budget.instance(), 7);
    assert_eq!(budget.side(), Side::Server);
    assert!(first.write_plugin_for_instance(AT, 0, &mut budget, LogLevel::Info, "first"));
    assert!(!second.write_plugin_for_instance(AT, 1, &mut budget, LogLevel::Info, "blocked"));
    second.tick_instance(AT, 1000, &mut budget);
    first.tick_instance(AT, 1000, &mut budget);
    assert!(second.write_plugin_for_instance(AT, 1000, &mut budget, LogLevel::Info, "next"));
    let text = std::fs::read_to_string(directory.path().join("example.notes.log")).unwrap();
    assert_eq!(
        text.matches("1 lines dropped in the last second").count(),
        1
    );
    assert!(!text.contains("blocked"));
    assert!(!directory
        .path()
        .join(".example.notes.log-rate.json")
        .exists());
}

#[test]
fn distinct_instances_and_sides_have_independent_admission() {
    let directory = tempfile::tempdir().unwrap();
    let mut log = writer(directory.path());
    let mut server = PluginLogInstanceBudget::new(1, Side::Server).unwrap();
    let mut client = PluginLogInstanceBudget::new(2, Side::Client).unwrap();
    assert!(log.write_plugin_for_instance(AT, 0, &mut server, LogLevel::Info, "server"));
    assert!(log.write_plugin_for_instance(AT, 0, &mut client, LogLevel::Info, "client"));
    assert!(!log.write_plugin_for_instance(AT, 1, &mut server, LogLevel::Info, "denied"));
    assert!(!log.write_plugin_for_instance(AT, 1, &mut client, LogLevel::Info, "denied"));
}

#[test]
fn invalid_context_and_filtered_lines_do_not_consume_admission() {
    assert!(PluginLogInstanceBudget::new(0, Side::Scene).is_err());
    let directory = tempfile::tempdir().unwrap();
    let mut log = writer(directory.path());
    let mut budget = PluginLogInstanceBudget::new(3, Side::Scene).unwrap();
    assert!(!log.write_plugin_for_instance("invalid", 0, &mut budget, LogLevel::Info, "bad"));
    assert!(log.take_error().is_some());
    assert!(!log.write_plugin_for_instance(AT, 0, &mut budget, LogLevel::Debug, "filtered"));
    assert!(log.write_plugin_for_instance(AT, 10, &mut budget, LogLevel::Info, "valid"));
    log.tick_instance(AT, 9, &mut budget);
    assert!(log.take_error().is_some());
    assert!(!log.write_plugin_for_instance(AT, 10, &mut budget, LogLevel::Info, "still full"));
}
