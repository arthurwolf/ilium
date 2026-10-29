#![cfg(feature = "testing")]

use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::engine::{
    validate_component, EngineConfig, PluginEngine, ValidationContext,
};

fn engine(cache_dir: Option<std::path::PathBuf>) -> PluginEngine {
    PluginEngine::new(EngineConfig {
        epoch_tick_ms: 10,
        cache_dir,
        process: ProcessKind::Server,
    })
    .expect("pinned Wasmtime engine initializes")
}

fn component(text: &str) -> Vec<u8> {
    ilium_plugin_host::testing::wat_component(text)
}

fn with_api_section(mut bytes: Vec<u8>) -> Vec<u8> {
    // A section with the name `ilium:api-version` and six big-endian bytes
    // for version 0.1.0. The payload fits a single-byte LEB length.
    let name = b"ilium:api-version";
    let body_len = 1 + name.len() + 6;
    bytes.extend_from_slice(&[0, body_len as u8, name.len() as u8]);
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&[0, 0, 0, 1, 0, 0]);
    bytes
}

#[test]
fn compiles_real_component_and_counts_only_successful_component_new() {
    let engine = engine(None);
    let bytes = component("(component)");
    let compiled = engine.compile(&bytes).expect("real component compilation");
    assert_eq!(compiled.source_len, bytes.len());
    assert_eq!(engine.stats().component_new_calls, 1);
    assert_eq!(engine.stats().deserialize_calls, 0);
    assert!(engine.compile(b"not wasm").is_err());
    assert_eq!(engine.stats().component_new_calls, 1);
}

#[test]
fn parses_and_validates_real_component_bytes_before_admission() {
    let context = ValidationContext::bare((0, 1, 0));
    assert!(validate_component(&with_api_section(component("(component)")), &context).is_ok());
    let forbidden = with_api_section(component(
        "(component (import \"wasi:sockets/tcp@0.2.0\" (instance)))",
    ));
    assert_eq!(
        validate_component(&forbidden, &context)
            .unwrap_err()
            .code
            .as_str(),
        "component.wasi-forbidden",
    );
}

#[test]
fn unavailable_disk_cache_still_compiles_without_unsafe_deserialization() {
    let directory = tempfile::tempdir().expect("temporary cache parent");
    let file = directory.path().join("cache-file");
    std::fs::write(&file, b"not a directory").expect("make unusable cache path");
    let engine = engine(Some(file));
    let bytes = component("(component)");
    engine.compile(&bytes).expect("first compile");
    engine.compile(&bytes).expect("second compile");
    assert_eq!(engine.stats().component_new_calls, 2);
    assert_eq!(engine.stats().deserialize_calls, 0);
    assert_eq!(
        engine
            .log_lines()
            .iter()
            .filter(|line| line.contains("host.cache-unavailable"))
            .count(),
        1
    );
}

#[test]
fn shared_execution_quota_refuses_ticker_before_starting_a_worker() {
    use ilium_execution::{QuotaGroup, QuotaLimits};
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: 0,
    });
    let result = PluginEngine::with_quota(
        EngineConfig {
            epoch_tick_ms: 10,
            cache_dir: None,
            process: ProcessKind::Server,
        },
        &quota,
    );
    assert!(
        result.is_err(),
        "an exhausted process quota must refuse the ticker"
    );
}
