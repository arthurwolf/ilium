//! HST-361: `ilium plugin stats <id> --json`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use ilium_plugin_api::Side;
use ilium_plugin_host::cli::CliEnv;
use ilium_plugin_host::telemetry::{SideTelemetry, COUNTER_NAMES};

use crate::common::{env_with_session, error_code, fixed_now, records_of, run, scratch, CountingFetcher, FakeBackend};

fn backend_with_counters() -> FakeBackend {
    let side = SideTelemetry::new();
    let now_ms = fixed_now().duration_since(std::time::UNIX_EPOCH).expect("epoch").as_millis() as u64;
    side.record_restart("trap", now_ms);
    let mut backend = FakeBackend::new("example.notes");
    backend.counters = vec![(Side::Server, side.snapshot(now_ms))];
    backend
}

/// REQ: HST-361
#[test]
fn hst_361_stats_exits_zero_for_an_existing_plugin() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_counters());
    assert_eq!(run(&env, &["stats", "example.notes", "--json"]).exit_code, 0);
}

/// REQ: HST-361
#[test]
fn hst_361_every_line_is_json_with_a_type_field() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_counters());
    let output = run(&env, &["stats", "example.notes", "--json"]);
    assert!(!output.stdout_jsonl.is_empty());
    assert!(output.stdout_jsonl.iter().all(|record| record["type"].is_string()));
}

/// REQ: HST-361
#[test]
fn hst_361_every_counter_name_appears_in_a_bucket_record() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_counters());
    let output = run(&env, &["stats", "example.notes", "--json"]);
    let buckets = records_of(&output, "bucket");
    assert!(!buckets.is_empty());
    for name in COUNTER_NAMES {
        assert!(buckets.iter().any(|b| b.get(name).is_some()), "counter {name} missing");
    }
}

/// REQ: HST-361
#[test]
fn hst_361_unknown_plugin_is_plugin_not_found_exit_5() {
    let scratch = scratch();
    let env = env_with_session(scratch.path(), backend_with_counters());
    let output = run(&env, &["stats", "example.unknown", "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("plugin.not-found"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: HST-361
#[test]
fn hst_361_stats_never_touches_the_network() {
    let scratch = scratch();
    let fetcher = Arc::new(CountingFetcher::default());
    let root = scratch.path();
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("project");
    let env = CliEnv::new(root.join("data"), root.join("state"), root.join("config"), project, fixed_now(), true, fetcher.clone())
        .with_session(Arc::new(backend_with_counters()));
    run(&env, &["stats", "example.notes", "--json"]);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}
