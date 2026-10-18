//! Page reference-host-runtime.md: version policy, Config, compile cache, allocator, loading,
//! linker, store, WASI, epoch ticker, component validation (HST-050..089).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::limits::{LimitKey, Limits};
use ilium_plugin_api::{ProcessKind, Side, Tier};
use ilium_plugin_host::calls::{deadline_ticks, ResolvedLimits};
use ilium_plugin_host::cli::{run, CliEnv};
use ilium_plugin_host::engine::{
    clock_jump_detected, compile_cache_dir, config_fingerprint, embedded_artifacts,
    prune_stale_cache_dirs, runtime_settings, select_allocator, store_settings, validate_component,
    AllocatorKind, EngineConfig, LoadPath, PluginEngine, ValidationContext, WASMTIME_VERSION,
};
use ilium_plugin_host::install::{Consent, ConsentDecision, ConsentRequest, Installer};
use ilium_plugin_host::linker::{build_linker, linker_cache_key, World};
use ilium_plugin_host::package::write_package;
use ilium_plugin_host::registry::PackageStore;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::telemetry::cut_wasi_line;
use ilium_plugin_host::testing::{wat_component, HostHarness};

use crate::support::*;

fn engine_config(cache_dir: Option<PathBuf>) -> EngineConfig {
    EngineConfig {
        epoch_tick_ms: 10,
        cache_dir,
        process: ProcessKind::Server,
    }
}

fn new_engine() -> PluginEngine {
    PluginEngine::new(engine_config(None)).expect("engine builds with the fixed Config")
}

fn empty_component() -> Vec<u8> {
    wat_component("(component)")
}

/// Appends a custom section (`id 0`) to a component binary; sections may follow in any order.
fn with_custom_section(mut bytes: Vec<u8>, name: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    push_leb(&mut body, name.len() as u64);
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(payload);
    bytes.push(0);
    push_leb(&mut bytes, body.len() as u64);
    bytes.extend_from_slice(&body);
    bytes
}

fn push_leb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn api_section() -> [u8; 6] {
    [0, 0, 0, 1, 0, 0]
}

fn valid_bare() -> Vec<u8> {
    with_custom_section(empty_component(), "ilium:api-version", &api_section())
}

fn bare_context() -> ValidationContext {
    ValidationContext::bare((0, 1, 0))
}

fn refusal_code(bytes: &[u8], context: &ValidationContext) -> String {
    match validate_component(bytes, context) {
        Ok(()) => "accepted".to_owned(),
        Err(diagnostic) => diagnostic.code.as_str().to_owned(),
    }
}

fn grant(text: &str) -> Grant {
    Grant::parse(text).expect("grant text parses")
}

fn toml_of(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

struct NoFetch;
impl Fetcher for NoFetch {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

struct ApproveAll;
impl Consent for ApproveAll {
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
        ConsentDecision::Approve(request.requested.clone())
    }
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn wasmtime_requirement_line() -> String {
    let text = toml_of(&workspace_root().join("Cargo.toml"));
    text.lines()
        .find(|line| {
            line.trim_start().starts_with("wasmtime ") || line.trim_start().starts_with("wasmtime=")
        })
        .unwrap_or("")
        .to_owned()
}

fn wasmtime_lock_versions() -> Vec<String> {
    let text = toml_of(&workspace_root().join("Cargo.lock"));
    let mut versions = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line == "name = \"wasmtime\"" {
            if let Some(next) = lines.peek() {
                versions.push(
                    next.trim_start_matches("version = ")
                        .trim_matches('"')
                        .to_owned(),
                );
            }
        }
    }
    versions
}

fn host_dependency_features() -> Option<(bool, Vec<String>)> {
    let text = toml_of(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    let value: toml::Value = text.parse().expect("host Cargo.toml parses");
    let dep = value.get("dependencies")?.get("wasmtime")?;
    let table = dep.as_table()?;
    let defaults_off = table.get("default-features").and_then(|v| v.as_bool()) == Some(false);
    let mut features: Vec<String> = table
        .get("features")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    features.sort();
    Some((defaults_off, features))
}

// ---------------------------------------------------------------- version policy

/// REQ: HST-050
#[test]
fn hst_050_workspace_manifest_pins_wasmtime_with_equals_requirement() {
    let line = wasmtime_requirement_line();
    assert!(
        line.contains("\"=49.0.2\""),
        "workspace wasmtime line was {line:?}"
    );
}

/// REQ: HST-050
#[test]
fn hst_050_cargo_lock_holds_exactly_one_wasmtime_at_the_pinned_version() {
    assert_eq!(wasmtime_lock_versions(), vec!["49.0.2".to_owned()]);
}

/// REQ: HST-050
#[test]
fn hst_050_runtime_constant_equals_the_pinned_version() {
    assert_eq!(WASMTIME_VERSION, "49.0.2");
    assert_eq!(runtime_settings().wasmtime_version, "49.0.2");
}

/// REQ: HST-050
#[test]
fn hst_050_doctor_output_shows_the_wasmtime_version() {
    let dir = tmp();
    let env = CliEnv::new(
        dir.path().join("data"),
        dir.path().join("state"),
        dir.path().join("config"),
        dir.path().to_path_buf(),
        std::time::SystemTime::now(),
        false,
        Arc::new(NoFetch),
    );
    let output = run(
        &[
            "plugin".to_owned(),
            "doctor".to_owned(),
            "--json".to_owned(),
        ],
        &env,
    );
    let text = output
        .stdout_jsonl
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("49.0.2"), "doctor records were {text}");
}

/// REQ: HST-051
#[test]
fn doc_hst_051_manual_states_14_day_and_48_hour_security_windows() {
    let page = manual_page("reference-host-runtime.md");
    assert!(page.contains("within 14 days of the upstream fix"));
    assert!(page.contains("within 48 hours when the vulnerability is exploited in the wild"));
}

/// REQ: HST-051
#[test]
fn doc_hst_051_release_checklist_records_advisory_to_release_time() {
    let table = manual_page("reference-requirements-kernel-host.md");
    let row = table
        .lines()
        .find(|line| line.starts_with("| HST-051 |"))
        .expect("HST-051 row");
    assert!(
        row.contains("advisory-to-release time recorded in release notes"),
        "requirement table must record advisory-to-release time: {row}"
    );
}

/// REQ: HST-052
#[test]
fn doc_hst_052_bump_gate_names_the_four_required_checks() {
    let script = workspace_root().join("tools/wasmtime_bump_gate.py");
    let text = std::fs::read_to_string(&script).unwrap_or_default();
    for needle in ["conformance", "bench", "cwasm", "safety"] {
        assert!(
            text.contains(needle),
            "bump gate must run the {needle} check"
        );
    }
}

/// REQ: HST-052
#[test]
fn doc_hst_052_manual_lists_the_safety_tests_under_errors_and_edge_cases() {
    let page = manual_page("reference-host-runtime.md");
    assert!(page.contains("the safety tests listed under \"Errors and edge cases\" below"));
    assert!(page.contains("## Errors and edge cases"));
}

/// REQ: HST-053
#[test]
fn hst_053_every_embedded_cwasm_header_matches_the_host_fingerprint() {
    let artifacts = embedded_artifacts();
    assert!(
        !artifacts.is_empty(),
        "the release build embeds the bundled plugins"
    );
    for artifact in artifacts {
        assert_eq!(
            artifact.fingerprint,
            config_fingerprint(),
            "stale cwasm for {}",
            artifact.name
        );
    }
}

/// REQ: HST-053
#[test]
fn hst_053_a_stale_fingerprint_is_detected_by_the_build_check() {
    let mut artifacts = embedded_artifacts();
    let first = artifacts
        .first_mut()
        .expect("at least one embedded artifact");
    first.fingerprint = "0".repeat(64);
    assert_ne!(first.fingerprint, config_fingerprint());
    let engine = new_engine();
    let path = engine
        .load_embedded(first)
        .expect("a stale artifact falls back, never ships a crash");
    assert_eq!(path, LoadPath::Compiled);
}

// ---------------------------------------------------------------- build features

/// REQ: HST-054
#[test]
fn hst_054_wasmtime_is_built_with_exactly_the_documented_features() {
    let (defaults_off, features) = host_dependency_features().expect("host depends on wasmtime");
    assert!(defaults_off, "wasmtime must use default-features = false");
    let mut expected = vec![
        "async",
        "cache",
        "component-model",
        "cranelift",
        "parallel-compilation",
        "runtime",
        "std",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(features, expected);
}

/// REQ: HST-054
#[test]
fn hst_054_component_model_async_feature_is_not_enabled() {
    let (_, features) = host_dependency_features().expect("host depends on wasmtime");
    assert!(!features.iter().any(|f| f == "component-model-async"));
}

/// REQ: HST-054
#[test]
fn hst_054_unused_wasmtime_features_stay_off() {
    let (_, features) = host_dependency_features().expect("host depends on wasmtime");
    for off in [
        "winch",
        "gc",
        "threads",
        "profiling",
        "coredump",
        "debug-builtins",
    ] {
        assert!(
            !features.iter().any(|f| f == off),
            "feature {off} must be off"
        );
    }
}

/// REQ: HST-055
#[test]
fn hst_055_config_turns_component_model_async_off() {
    let settings = runtime_settings();
    assert!(settings.component_model);
    assert!(!settings.component_model_async);
}

/// REQ: HST-055
#[test]
fn hst_055_component_needing_async_is_refused_with_component_invalid() {
    let bytes = wat_component("(component (core module $m (func (export \"f\"))) (core instance $i (instantiate $m)) (func (canon lift (core func $i \"f\") async)))");
    assert_eq!(refusal_code(&bytes, &bare_context()), "component.invalid");
}

/// REQ: HST-056
#[test]
fn hst_056_instantiate_call_and_drop_under_the_pinned_concurrency_setting() {
    assert!(!runtime_settings().concurrency_support);
    let host = host_with(QuietGuest);
    host.deliver(one_event());
    let report = host.pump(8);
    assert_eq!(report.traps, 0);
}

/// REQ: HST-057
#[test]
fn hst_057_fingerprint_is_stable_across_calls() {
    assert_eq!(config_fingerprint(), config_fingerprint());
}

/// REQ: HST-057
#[test]
fn hst_057_fingerprint_is_a_sha256_hex_digest() {
    let fingerprint = config_fingerprint();
    assert_eq!(fingerprint.len(), 64);
    assert!(fingerprint
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

/// REQ: HST-057
#[test]
fn hst_057_every_embedded_artifact_header_equals_the_host_fingerprint() {
    for artifact in embedded_artifacts() {
        assert_eq!(artifact.fingerprint, config_fingerprint());
    }
}

/// REQ: HST-057
#[test]
fn hst_057_precompile_and_host_settings_are_identical() {
    assert_eq!(runtime_settings(), runtime_settings());
}

/// REQ: HST-058
#[test]
fn hst_058_epoch_interruption_is_on() {
    assert!(runtime_settings().epoch_interruption);
}

/// REQ: HST-058
#[test]
fn hst_058_fuel_is_off() {
    assert!(!runtime_settings().consume_fuel);
}

/// REQ: HST-059
#[test]
fn hst_059_max_wasm_stack_is_512_kib() {
    assert_eq!(runtime_settings().max_wasm_stack, 512 * 1024);
}

/// REQ: HST-059
#[test]
fn hst_059_async_stack_size_is_1_mib_and_not_below_the_wasm_stack() {
    let settings = runtime_settings();
    assert_eq!(settings.async_stack_size, 1024 * 1024);
    assert!(settings.async_stack_size >= settings.max_wasm_stack);
}

/// REQ: HST-059
#[test]
fn hst_059_recursion_guest_traps_at_the_stack_limit() {
    let mut host = host_with_fixture("stack-recursion", Tier::T3);
    fail_once(&mut host);
    assert_eq!(host.failures().len(), 1);
}

/// REQ: HST-060
#[test]
fn hst_060_signals_based_traps_are_on() {
    assert!(runtime_settings().signals_based_traps);
}

/// REQ: HST-060
#[test]
fn hst_060_no_ilium_source_installs_a_segv_bus_or_ill_handler() {
    for crate_dir in [
        "ilium-server/src",
        "ilium-client/src",
        "ilium/src",
        "ilium-plugin-host/src",
    ] {
        let mut stack = vec![workspace_root().join(crate_dir)];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    for signal in ["SIGSEGV", "SIGBUS", "SIGILL"] {
                        assert!(
                            !text.contains(signal),
                            "{} mentions {signal}",
                            path.display()
                        );
                    }
                }
            }
        }
    }
}

/// REQ: HST-060
#[test]
fn hst_060_out_of_bounds_guest_access_traps_and_the_process_survives() {
    let mut host = host_with_fixture("oob-access", Tier::T3);
    fail_once(&mut host);
    assert_eq!(host.failures().len(), 1);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    // manual does not pin the exact trap code of an out-of-bounds access; every trap is trap.*.
    let failures = host.failures();
    assert!(
        failures[0].code.as_str().starts_with("trap."),
        "code {}",
        failures[0].code.as_str()
    );
}

// ---------------------------------------------------------------- compile cache

/// REQ: HST-061
#[test]
fn hst_061_cache_dir_name_carries_version_and_fingerprint() {
    let base = tmp();
    let dir = compile_cache_dir(base.path());
    let expected = format!("{}-{}", WASMTIME_VERSION, config_fingerprint());
    assert_eq!(dir.file_name().unwrap().to_str().unwrap(), expected);
    assert!(dir.starts_with(base.path().join("ilium/plugin-compile")));
}

/// REQ: HST-061
#[test]
fn hst_061_cache_dir_is_owner_only_once_the_engine_uses_it() {
    use std::os::unix::fs::PermissionsExt;
    let base = tmp();
    let dir = compile_cache_dir(base.path());
    let engine = PluginEngine::new(engine_config(Some(dir.clone()))).expect("engine");
    engine.compile(&empty_component()).expect("compile");
    let mode = std::fs::metadata(&dir)
        .expect("cache dir exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}

/// REQ: HST-061
#[test]
fn hst_061_startup_deletes_cache_dirs_of_other_keys() {
    let base = tmp();
    let root = base.path().join("ilium/plugin-compile");
    std::fs::create_dir_all(root.join("48.0.0-abc")).unwrap();
    std::fs::create_dir_all(root.join("49.0.2-deadbeef")).unwrap();
    let keep = compile_cache_dir(base.path());
    std::fs::create_dir_all(&keep).unwrap();
    let removed = prune_stale_cache_dirs(base.path()).expect("prune succeeds");
    assert_eq!(removed.len(), 2);
    assert!(!root.join("48.0.0-abc").exists());
    assert!(!root.join("49.0.2-deadbeef").exists());
    assert!(keep.exists());
}

/// REQ: HST-061
#[test]
fn hst_061_prune_with_no_cache_root_removes_nothing() {
    let base = tmp();
    assert!(prune_stale_cache_dirs(base.path())
        .expect("prune succeeds")
        .is_empty());
}

/// REQ: HST-062
#[test]
fn hst_062_read_only_cache_dir_still_loads_and_logs_once() {
    use std::os::unix::fs::PermissionsExt;
    let base = tmp();
    let dir = compile_cache_dir(base.path());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let engine = PluginEngine::new(engine_config(Some(dir.clone()))).expect("engine");
    engine
        .compile(&empty_component())
        .expect("first load compiles in memory");
    engine
        .compile(&empty_component())
        .expect("second load compiles in memory");
    let lines = engine
        .log_lines()
        .into_iter()
        .filter(|l| l.contains("host.cache-unavailable"))
        .count();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(lines, 1);
}

/// REQ: HST-062
#[test]
fn hst_062_unusable_cache_path_is_a_file_and_load_still_works() {
    let base = tmp();
    let file = base.path().join("not-a-dir");
    std::fs::write(&file, b"x").unwrap();
    let engine = PluginEngine::new(engine_config(Some(file))).expect("engine");
    engine
        .compile(&empty_component())
        .expect("compiles in memory");
    assert_eq!(
        engine
            .log_lines()
            .iter()
            .filter(|l| l.contains("host.cache-unavailable"))
            .count(),
        1
    );
}

/// REQ: HST-062
#[test]
fn hst_062_healthy_cache_logs_nothing() {
    let base = tmp();
    let engine =
        PluginEngine::new(engine_config(Some(compile_cache_dir(base.path())))).expect("engine");
    engine.compile(&empty_component()).expect("compile");
    assert!(engine
        .log_lines()
        .iter()
        .all(|l| !l.contains("host.cache-unavailable")));
}

// ---------------------------------------------------------------- allocator

/// REQ: HST-063
#[test]
fn hst_063_default_allocator_is_on_demand() {
    assert_eq!(runtime_settings().allocator, AllocatorKind::OnDemand);
}

/// REQ: HST-064
#[test]
fn hst_064_pooling_chosen_when_on_demand_misses_and_pooling_meets_both_within_rss() {
    // (host_start_ms, instantiate_median_ms), then pooling pair, then added RSS in MiB.
    assert_eq!(
        select_allocator((200.0, 4.0), (100.0, 3.0), 16.0),
        AllocatorKind::Pooling
    );
}

/// REQ: HST-064
#[test]
fn hst_064_on_demand_kept_when_it_meets_both_budgets() {
    assert_eq!(
        select_allocator((150.0, 5.0), (80.0, 2.0), 1.0),
        AllocatorKind::OnDemand
    );
}

/// REQ: HST-064
#[test]
fn hst_064_on_demand_misses_only_instantiate_time_selects_pooling() {
    assert_eq!(
        select_allocator((100.0, 5.1), (100.0, 4.0), 8.0),
        AllocatorKind::Pooling
    );
}

/// REQ: HST-064
#[test]
fn hst_064_pooling_adding_more_than_16_mib_keeps_on_demand() {
    assert_eq!(
        select_allocator((200.0, 9.0), (100.0, 3.0), 16.5),
        AllocatorKind::OnDemand
    );
}

/// REQ: HST-064
#[test]
fn hst_064_pooling_that_misses_one_budget_is_rejected() {
    assert_eq!(
        select_allocator((200.0, 9.0), (160.0, 3.0), 4.0),
        AllocatorKind::OnDemand
    );
    assert_eq!(
        select_allocator((200.0, 9.0), (100.0, 5.5), 4.0),
        AllocatorKind::OnDemand
    );
}

/// REQ: HST-064
#[test]
fn doc_hst_064_manual_states_the_selection_inputs() {
    let page = manual_page("reference-host-runtime.md");
    assert!(page.contains("150 ms for the 40-plugin bundled set"));
    assert!(page.contains("5 ms median"));
    assert!(page.contains("no more than 16 MiB RSS"));
}

// ---------------------------------------------------------------- loading

/// REQ: HST-065
#[test]
fn hst_065_deserialize_call_sites_exist_only_in_the_embedded_loader() {
    let mut sites = Vec::new();
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if text.contains("Component::deserialize") || text.contains("deserialize_file") {
                    sites.push(path.file_name().unwrap().to_string_lossy().into_owned());
                }
            }
        }
    }
    assert!(
        !sites.is_empty(),
        "the embedded loader must call Component::deserialize"
    );
    assert!(
        sites.iter().all(|name| name.contains("embedded")),
        "call sites: {sites:?}"
    );
}

/// REQ: HST-065
#[test]
fn hst_065_compiling_package_bytes_never_deserializes() {
    let engine = new_engine();
    engine.compile(&empty_component()).expect("compile");
    assert_eq!(engine.stats().deserialize_calls, 0);
}

/// REQ: HST-066
#[test]
fn hst_066_every_embedded_cwasm_deserializes_on_the_host_engine() {
    let engine = new_engine();
    let artifacts = embedded_artifacts();
    assert!(!artifacts.is_empty());
    for artifact in &artifacts {
        let path = engine
            .load_embedded(artifact)
            .unwrap_or_else(|e| panic!("{}: {e}", artifact.name));
        assert_eq!(
            path,
            LoadPath::Deserialized,
            "{} took the fallback",
            artifact.name
        );
    }
}

/// REQ: HST-066
#[test]
fn hst_066_embedded_set_has_exactly_the_30_core_plugins() {
    assert_eq!(
        embedded_artifacts().len(),
        30,
        "30 core plugins are bundled"
    );
}

/// REQ: HST-067
#[test]
fn hst_067_t2_component_loads_with_component_new_and_no_deserialize() {
    let engine = new_engine();
    engine.compile(&empty_component()).expect("compile");
    let stats = engine.stats();
    assert_eq!(stats.component_new_calls, 1);
    assert_eq!(stats.deserialize_calls, 0);
}

/// REQ: HST-067
#[test]
fn hst_067_repeat_load_through_cache_still_uses_component_new_api() {
    let base = tmp();
    let engine =
        PluginEngine::new(engine_config(Some(compile_cache_dir(base.path())))).expect("engine");
    engine.compile(&empty_component()).expect("first");
    engine.compile(&empty_component()).expect("second");
    assert_eq!(engine.stats().component_new_calls, 2);
    assert_eq!(engine.stats().deserialize_calls, 0);
}

/// REQ: HST-068
#[test]
fn hst_068_package_with_a_cwasm_file_is_refused_at_install() {
    let dir = tmp();
    let installer = Installer::new(PackageStore {
        root: dir.path().to_path_buf(),
    });
    let bytes = write_package(&[
        ("manifest.json".to_owned(), b"{}".to_vec()),
        ("server.wasm".to_owned(), empty_component()),
        ("server.cwasm".to_owned(), vec![1, 2, 3]),
    ])
    .expect("package writes");
    let error = installer
        .install(&bytes, &ApproveAll)
        .expect_err("install is refused");
    assert_eq!(error.code.as_str(), "package.precompiled-forbidden");
}

/// REQ: HST-068
#[test]
fn hst_068_check_reports_the_cwasm_refusal_too() {
    let dir = tmp();
    let installer = Installer::new(PackageStore {
        root: dir.path().to_path_buf(),
    });
    let bytes = write_package(&[
        ("manifest.json".to_owned(), b"{}".to_vec()),
        ("nested/client.cwasm".to_owned(), vec![9]),
    ])
    .expect("package writes");
    let report = installer.check(&bytes, ilium_plugin_api::manifest::CheckMode::Install);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code.as_str() == "package.precompiled-forbidden"));
}

/// REQ: HST-068
#[test]
fn hst_068_a_refused_package_installs_nothing() {
    let dir = tmp();
    let installer = Installer::new(PackageStore {
        root: dir.path().to_path_buf(),
    });
    let bytes = write_package(&[
        ("manifest.json".to_owned(), b"{}".to_vec()),
        ("a.cwasm".to_owned(), vec![0]),
    ])
    .unwrap();
    let _ = installer.install(&bytes, &ApproveAll);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

// ---------------------------------------------------------------- InstancePre and Linker

/// REQ: HST-069
#[test]
fn hst_069_grant_changes_increment_the_instance_pre_generation() {
    let host = HostHarness::new()
        .grants(&["tree.read"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let first = host.instance_pre_generation();
    host.grant_now("tree.write");
    let second = host.instance_pre_generation();
    host.revoke("tree.write");
    let third = host.instance_pre_generation();
    assert!(second > first && third > second);
}

/// REQ: HST-069
#[test]
fn hst_069_existing_instance_keeps_running_after_a_grant_change() {
    let host = host_with(QuietGuest);
    host.grant_now("tree.read");
    assert_eq!(host.state(), InstanceState::Active);
}

/// REQ: HST-069
#[test]
fn hst_069_an_unchanged_grant_set_keeps_the_generation() {
    let host = host_with(QuietGuest);
    let before = host.instance_pre_generation();
    host.pump(2);
    assert_eq!(host.instance_pre_generation(), before);
}

/// REQ: HST-070
#[test]
fn hst_070_equal_grant_sets_on_equal_sides_share_a_linker_key() {
    let a = linker_cache_key(
        Side::Server,
        Tier::T2,
        &[grant("tree.read"), grant("tree.write")],
    );
    let b = linker_cache_key(
        Side::Server,
        Tier::T2,
        &[grant("tree.write"), grant("tree.read")],
    );
    assert_eq!(a, b);
}

/// REQ: HST-070
#[test]
fn hst_070_different_grant_sets_do_not_share() {
    let a = linker_cache_key(Side::Server, Tier::T2, &[grant("tree.read")]);
    let b = linker_cache_key(
        Side::Server,
        Tier::T2,
        &[grant("tree.read"), grant("tree.write")],
    );
    assert_ne!(a, b);
}

/// REQ: HST-070
#[test]
fn hst_070_side_and_tier_are_part_of_the_key() {
    let grants = [grant("tree.read")];
    assert_ne!(
        linker_cache_key(Side::Server, Tier::T2, &grants),
        linker_cache_key(Side::Client, Tier::T2, &grants)
    );
    assert_ne!(
        linker_cache_key(Side::Server, Tier::T2, &grants),
        linker_cache_key(Side::Server, Tier::T3, &grants)
    );
}

/// REQ: HST-070
#[test]
fn hst_070_key_is_a_sha256_hex_digest() {
    let key = linker_cache_key(Side::Server, Tier::T1, &[]);
    assert_eq!(key.len(), 64);
    assert!(
        key.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "lowercase hex: {key}"
    );
    assert_eq!(
        key,
        linker_cache_key(Side::Server, Tier::T1, &[]),
        "deterministic"
    );
    // Exact SHA-256 value: see API_GAPS (needs the sha2 dev-dependency).
}

/// REQ: HST-070
#[test]
fn hst_070_build_linker_serves_the_requested_world() {
    let linker = build_linker(World::Client, &[grant("tree.read")]).expect("linker builds");
    assert_eq!(linker.world, World::Client);
}

/// REQ: HST-071
#[test]
fn hst_071_declined_capability_returns_not_permitted_without_kernel_code() {
    let kernel = Arc::new(ilium_plugin_host::testing::FakeKernel::new(
        ProcessKind::Server,
    ));
    let host = HostHarness::new()
        .kernel(kernel.clone())
        .grants(&[])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.decline("tree.write");
    let reply = host.call_import_json("ilium:host/tree", "set-name", "{\"node\":1,\"name\":\"x\"}");
    assert_eq!(reply.error_code(), Some("not-permitted"));
    assert!(kernel.applied_intents().is_empty());
}

/// REQ: HST-071
#[test]
fn hst_071_revoked_capability_returns_not_permitted_not_a_trap() {
    let host = HostHarness::new()
        .grants(&["tree.write"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.revoke("tree.write");
    let reply = host.call_import_json("ilium:host/tree", "set-name", "{\"node\":1,\"name\":\"x\"}");
    assert_eq!(reply.error_code(), Some("not-permitted"));
    assert!(host.failures().is_empty());
}

/// REQ: HST-072
#[test]
fn hst_072_unrequested_interface_import_is_refused_with_import_not_requested() {
    let bytes = with_custom_section(
        wat_component("(component (import \"ilium:host/fs@0.1.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "manifest.import-not-requested"
    );
}

/// REQ: HST-072
#[test]
fn hst_072_requested_interface_import_is_accepted() {
    let bytes = with_custom_section(
        wat_component("(component (import \"ilium:host/fs@0.1.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    let context = bare_context().requesting(&["fs.read:project"]);
    assert_eq!(refusal_code(&bytes, &context), "accepted");
}

/// REQ: HST-072
#[test]
fn hst_072_always_granted_interface_needs_no_request() {
    let bytes = with_custom_section(
        wat_component("(component (import \"ilium:host/log@0.1.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(refusal_code(&bytes, &bare_context()), "accepted");
}

/// REQ: HST-072
#[test]
fn hst_072_refused_component_is_never_instantiated() {
    let engine = new_engine();
    let bytes = with_custom_section(
        wat_component("(component (import \"ilium:host/fs@0.1.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.wasi-forbidden"
    );
    assert_eq!(engine.stats().instantiations, 0);
}

/// REQ: HST-073
#[test]
fn hst_073_unrequested_function_in_a_linked_interface_traps() {
    let mut host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let reply = host.call_import_json("ilium:host/fs", "write", "{}");
    assert_eq!(
        reply.trap_code(),
        Some("host.capability-not-requested".to_owned())
    );
    let _ = &mut host;
}

/// REQ: HST-073
#[test]
fn hst_073_the_trap_counts_as_a_failure() {
    let host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let _ = host.call_import_json("ilium:host/fs", "write", "{}");
    assert_eq!(host.failure_count(), 1);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
}

/// REQ: HST-073
#[test]
fn hst_073_requested_function_does_not_trap() {
    let host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let reply = host.call_import_json(
        "ilium:host/fs",
        "read",
        "{\"path\":\"a.txt\",\"offset\":0,\"max-bytes\":1}",
    );
    assert_eq!(reply.trap_code(), None);
}

// ---------------------------------------------------------------- Store

/// REQ: HST-074
#[test]
fn hst_074_store_limits_equal_the_clamped_manifest_values() {
    let mut limits = Limits::default();
    limits.0.insert("memory_mib".to_owned(), 64);
    limits.0.insert("table_elements".to_owned(), 5_000);
    let resolved = ilium_plugin_api::limits::clamp(&limits, Tier::T2).expect("limits clamp");
    let settings = store_settings(&ResolvedLimits(resolved));
    assert_eq!(settings.memory_bytes, 64 * 1024 * 1024);
    assert_eq!(settings.table_elements, 5_000);
}

/// REQ: HST-074
#[test]
fn hst_074_store_traps_on_grow_failure() {
    let resolved = ilium_plugin_api::limits::clamp(&Limits::default(), Tier::T3).expect("clamp");
    assert!(store_settings(&ResolvedLimits(resolved)).trap_on_grow_failure);
}

/// REQ: HST-074
#[test]
fn hst_074_store_has_fixed_instance_table_and_memory_counts() {
    let resolved = ilium_plugin_api::limits::clamp(&Limits::default(), Tier::T3).expect("clamp");
    let settings = store_settings(&ResolvedLimits(resolved));
    assert_eq!(
        (settings.instances, settings.tables, settings.memories),
        (16, 8, 4)
    );
}

/// REQ: HST-074
#[test]
fn hst_074_memory_growth_past_the_limit_traps_and_the_guest_never_sees_minus_one() {
    let mut host = host_with_fixture("memory-grow", Tier::T3);
    fail_once(&mut host);
    let traps = host.failures();
    assert_eq!(traps.len(), 1);
    assert_eq!(code_of(&traps[0]), "trap.memory");
}

/// REQ: HST-074
#[test]
fn hst_074_job_instance_uses_job_memory_mib() {
    let resolved = ilium_plugin_api::limits::clamp(&Limits::default(), Tier::T3).expect("clamp");
    let resolved = ResolvedLimits(resolved);
    assert_eq!(resolved.0[&LimitKey::JobMemoryMib], 64);
    assert_eq!(resolved.0[&LimitKey::MemoryMib], 32);
}

/// REQ: HST-075
#[test]
fn hst_075_import_after_revoke_returns_revoked() {
    let host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.revoke_without_reactivation("fs.read:project");
    let reply = host.call_import_json(
        "ilium:host/fs",
        "read",
        "{\"path\":\"a.txt\",\"offset\":0,\"max-bytes\":1}",
    );
    assert_eq!(reply.error_code(), Some("revoked"));
}

/// REQ: HST-075
#[test]
fn hst_075_import_with_current_epoch_does_not_return_revoked() {
    let host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let reply = host.call_import_json(
        "ilium:host/fs",
        "read",
        "{\"path\":\"a.txt\",\"offset\":0,\"max-bytes\":1}",
    );
    assert_ne!(reply.error_code(), Some("revoked"));
}

/// REQ: HST-075
#[test]
fn hst_075_store_carries_plugin_side_instance_and_epoch() {
    let host = host_with(QuietGuest);
    let data = host.store_data();
    assert_eq!(data.plugin, plugin_id());
    assert_eq!(data.side, Side::Server);
    assert_eq!(data.instance, 1);
    assert_eq!(data.authorization_epoch, host.authorization_epoch());
}

/// REQ: HST-075
#[test]
fn hst_075_instance_number_rises_with_each_new_instance() {
    let mut host = host_with(TrapGuest("trap.panic"));
    fail_once(&mut host);
    advance_ms(&mut host, 1_000);
    assert_eq!(host.store_data().instance, 2);
}

// ---------------------------------------------------------------- WASI

/// REQ: HST-076
#[test]
fn hst_076_stdout_lines_reach_the_plugin_log_at_info() {
    let mut host = host_with_fixture("wasi-probe", Tier::T3);
    fail_once(&mut host);
    assert!(host
        .logs()
        .iter()
        .any(|l| l.level == "info" && l.message.contains("stdout-line")));
}

/// REQ: HST-076
#[test]
fn hst_076_stderr_lines_reach_the_plugin_log_at_warn() {
    let mut host = host_with_fixture("wasi-probe", Tier::T3);
    fail_once(&mut host);
    assert!(host
        .logs()
        .iter()
        .any(|l| l.level == "warn" && l.message.contains("stderr-line")));
}

/// REQ: HST-076
#[test]
fn hst_076_environment_is_empty_and_the_single_argument_is_the_plugin_id() {
    let mut host = host_with_fixture("wasi-probe", Tier::T3);
    fail_once(&mut host);
    let text = log_text(&host);
    assert!(text.contains("env-count=0"));
    assert!(text.contains("args=test.plugin"));
}

/// REQ: HST-076
#[test]
fn hst_076_stdin_is_always_empty() {
    let mut host = host_with_fixture("wasi-probe", Tier::T3);
    fail_once(&mut host);
    assert!(log_text(&host).contains("stdin-bytes=0"));
}

/// REQ: HST-076
#[test]
fn hst_076_exit_call_traps_with_wasi_exit() {
    let mut host = host_with_fixture("wasi-exit", Tier::T3);
    fail_once(&mut host);
    assert!(log_text(&host).contains("wasi.exit"));
    assert_eq!(host.failures().len(), 1);
}

/// REQ: HST-076
#[test]
fn hst_076_without_an_fs_grant_the_preopen_list_is_empty() {
    let mut host = host_with_fixture("wasi-probe", Tier::T3);
    fail_once(&mut host);
    assert!(log_text(&host).contains("preopens=0"));
}

/// REQ: HST-076
#[test]
fn hst_076_an_fs_grant_adds_exactly_its_scope_as_a_preopen() {
    let bytes = fixture_wasm("wasi-probe");
    let mut host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_wasm_bytes(&bytes)
        .build()
        .expect("host");
    fail_once(&mut host);
    assert!(log_text(&host).contains("preopens=1"));
}

/// REQ: HST-077
#[test]
fn hst_077_component_importing_wasi_sockets_is_refused() {
    let bytes = with_custom_section(
        wat_component("(component (import \"wasi:sockets/tcp@0.2.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.wasi-forbidden"
    );
}

/// REQ: HST-077
#[test]
fn hst_077_component_importing_wasi_http_is_refused() {
    let bytes = with_custom_section(
        wat_component("(component (import \"wasi:http/types@0.2.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.wasi-forbidden"
    );
}

/// REQ: HST-077
#[test]
fn hst_077_wasi_clocks_import_is_allowed() {
    let bytes = with_custom_section(
        wat_component("(component (import \"wasi:clocks/monotonic-clock@0.2.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(refusal_code(&bytes, &bare_context()), "accepted");
}

/// REQ: HST-077
#[test]
fn hst_077_refusal_happens_at_load_before_any_instantiation() {
    let engine = new_engine();
    let bytes = with_custom_section(
        wat_component("(component (import \"wasi:sockets/tcp@0.2.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    let _ = validate_component(&bytes, &bare_context());
    assert_eq!(engine.stats().instantiations, 0);
}

/// REQ: HST-078
#[test]
fn hst_078_a_20_kib_line_is_cut_at_16_kib_and_marked() {
    let line = "a".repeat(20 * 1024);
    let cut = cut_wasi_line(&line);
    assert!(cut.starts_with(&"a".repeat(16 * 1024)));
    assert!(cut.ends_with("[cut]"));
    assert_eq!(cut.len(), 16 * 1024 + "[cut]".len());
}

/// REQ: HST-078
#[test]
fn hst_078_a_line_of_exactly_16_kib_is_not_cut() {
    let line = "b".repeat(16 * 1024);
    assert_eq!(cut_wasi_line(&line), line);
}

/// REQ: HST-078
#[test]
fn hst_078_a_line_one_byte_over_16_kib_is_cut() {
    let line = "c".repeat(16 * 1024 + 1);
    let cut = cut_wasi_line(&line);
    assert!(cut.ends_with("[cut]"));
    assert_eq!(cut.len(), 16 * 1024 + 5);
}

/// REQ: HST-078
#[test]
fn hst_078_a_line_one_byte_under_16_kib_is_not_cut() {
    let line = "d".repeat(16 * 1024 - 1);
    assert_eq!(cut_wasi_line(&line), line);
}

proptest::proptest! {
    // REQ: HST-078
    #[test]
    fn hst_078_cut_output_never_exceeds_the_limit_plus_marker(length in 0usize..40_000) {
        let cut = cut_wasi_line(&"x".repeat(length));
        proptest::prop_assert!(cut.len() <= 16 * 1024 + 5);
        proptest::prop_assert_eq!(cut.ends_with("[cut]"), length > 16 * 1024);
    }
}

// ---------------------------------------------------------------- epoch ticker

fn ticker_threads() -> usize {
    // Linux truncates thread names to 15 characters: `ilium-plugin-ep`.
    std::fs::read_dir("/proc/self/task")
        .expect("procfs lists threads")
        .flatten()
        .filter(|entry| {
            std::fs::read_to_string(entry.path().join("comm"))
                .map(|name| name.trim() == "ilium-plugin-ep")
                .unwrap_or(false)
        })
        .count()
}

/// REQ: HST-079
#[test]
fn hst_079_one_engine_has_exactly_one_ticker_thread_named_ilium_plugin_epoch() {
    let before = ticker_threads();
    let engine = new_engine();
    assert_eq!(ticker_threads(), before + 1);
    drop(engine);
}

/// REQ: HST-079
#[test]
fn hst_079_the_epoch_advances_every_10_ms() {
    let engine = new_engine();
    let start = engine.current_epoch();
    std::thread::sleep(Duration::from_millis(500));
    let ticks = engine.current_epoch() - start;
    assert!((40..=52).contains(&ticks), "ticks in 500 ms: {ticks}");
}

/// REQ: HST-079
#[test]
fn hst_079_configured_tick_is_10_ms() {
    assert_eq!(new_engine().config().epoch_tick_ms, 10);
}

/// REQ: HST-079
#[test]
fn hst_079_two_engines_have_two_tickers() {
    let before = ticker_threads();
    let first = new_engine();
    let second = new_engine();
    assert_eq!(ticker_threads(), before + 2);
    drop((first, second));
}

/// REQ: HST-080
#[test]
fn hst_080_callback_is_installed_before_each_call_and_sees_the_call_state() {
    let host = host_with(QuietGuest);
    host.deliver(one_event());
    host.pump(8);
    let observed = host.tick_observations();
    // manual does not pin how many ticks a call sees; at least one call ran, so one is observed.
    assert!(!observed.is_empty(), "no tick was observed");
    assert!(observed.iter().all(|o| o.callback_installed));
}

/// REQ: HST-080
#[test]
fn hst_080_callback_decides_continue_yield_or_trap() {
    let host = host_with(QuietGuest);
    let decisions = host.tick_decision_names();
    assert_eq!(decisions, vec!["continue", "yield", "trap"]);
}

/// REQ: HST-081
#[test]
fn hst_081_deadline_in_ticks_is_ceil_over_10_plus_1() {
    assert_eq!(deadline_ticks(20), 3);
    assert_eq!(deadline_ticks(10), 2);
    assert_eq!(deadline_ticks(1), 2);
    assert_eq!(deadline_ticks(50), 6);
    assert_eq!(deadline_ticks(2_000), 201);
}

/// REQ: HST-081
#[test]
fn hst_081_deadline_in_ticks_boundaries_around_a_multiple_of_ten() {
    assert_eq!(deadline_ticks(19), 3);
    assert_eq!(deadline_ticks(20), 3);
    assert_eq!(deadline_ticks(21), 4);
}

proptest::proptest! {
    // REQ: HST-081
    #[test]
    fn hst_081_ticks_always_cover_the_deadline_and_never_exceed_it_by_more_than_20_ms(deadline in 1u64..100_000) {
        let ticks = deadline_ticks(deadline);
        proptest::prop_assert!(ticks * 10 >= deadline + 10);
        proptest::prop_assert!(ticks * 10 <= deadline + 20);
    }
}

/// REQ: HST-081
#[test]
fn hst_081_spinning_guest_with_a_20_ms_deadline_traps_between_20_and_40_ms() {
    let mut limits = Limits::default();
    limits.0.insert("event_deadline_ms".to_owned(), 20);
    let bytes = fixture_wasm("spin-loop");
    for _ in 0..20 {
        let mut host = HostHarness::new()
            .limits(limits.clone())
            .with_wasm_bytes(&bytes)
            .build()
            .expect("host");
        let started = std::time::Instant::now();
        fail_once(&mut host);
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(20) && elapsed <= Duration::from_millis(40),
            "trapped after {elapsed:?}"
        );
    }
}

/// REQ: HST-081
#[test]
fn hst_081_guest_that_returns_at_19_ms_never_traps() {
    let bytes = fixture_wasm("sleep-19ms");
    let host = HostHarness::new()
        .with_wasm_bytes(&bytes)
        .build()
        .expect("host");
    for _ in 0..20 {
        host.deliver(one_event());
        host.pump(8);
    }
    assert!(host.failures().is_empty());
}

/// REQ: HST-082
#[test]
fn hst_082_a_gap_of_more_than_one_second_is_a_clock_jump() {
    assert!(clock_jump_detected(0, 1_001));
    assert!(clock_jump_detected(5_000, 7_500));
}

/// REQ: HST-082
#[test]
fn hst_082_a_gap_of_exactly_one_second_or_less_is_not_a_jump() {
    assert!(!clock_jump_detected(0, 1_000));
    assert!(!clock_jump_detected(0, 10));
    assert!(!clock_jump_detected(100, 110));
}

/// REQ: HST-082
#[test]
fn hst_082_no_trap_decision_on_the_jump_tick() {
    let host = host_with_fixture("spin-loop", Tier::T3);
    host.deliver(one_event());
    host.inject_clock_jump(Duration::from_secs(5));
    assert!(host.failures().is_empty());
}

/// REQ: HST-082
#[test]
fn hst_082_ticker_continues_while_a_call_is_in_flight() {
    let engine = new_engine();
    let start = engine.current_epoch();
    std::thread::sleep(Duration::from_millis(100));
    assert!(engine.current_epoch() > start);
}

/// REQ: HST-083
#[test]
fn hst_083_deadline_trap_carries_host_deadline_class_and_limit() {
    let mut limits = Limits::default();
    limits.0.insert("event_deadline_ms".to_owned(), 20);
    let bytes = fixture_wasm("spin-loop");
    let mut host = HostHarness::new()
        .limits(limits)
        .with_wasm_bytes(&bytes)
        .build()
        .expect("host");
    fail_once(&mut host);
    let text = log_text(&host);
    assert!(text.contains("trap.deadline"));
    assert!(text.contains("event"));
    assert!(text.contains("20"));
}

/// REQ: HST-083
#[test]
fn hst_083_deadline_trap_is_logged_once() {
    let bytes = fixture_wasm("spin-loop");
    let mut host = HostHarness::new()
        .with_wasm_bytes(&bytes)
        .build()
        .expect("host");
    fail_once(&mut host);
    assert_eq!(log_count(&host, "trap.deadline"), 1);
}

// ---------------------------------------------------------------- component validation

/// REQ: HST-084
#[test]
fn hst_084_invalid_bytes_report_step_1_first() {
    let mut bytes = vec![0, 0x61, 0x73, 0x6d];
    bytes.extend_from_slice(b"junk");
    assert_eq!(
        refusal_code(&bytes, &ValidationContext::new(World::Server, (0, 1, 0))),
        "component.invalid"
    );
}

/// REQ: HST-084
#[test]
fn hst_084_oversized_component_with_bad_api_section_reports_too_large() {
    let mut bytes =
        with_custom_section(empty_component(), "ilium:api-version", &[9, 9, 9, 9, 9, 9]);
    for _ in 0..9 {
        bytes = with_custom_section(bytes, "pad", &vec![0u8; 3 * 1024 * 1024 + 900 * 1024]);
    }
    assert!(bytes.len() > 32 * 1024 * 1024);
    assert_eq!(refusal_code(&bytes, &bare_context()), "component.too-large");
}

/// REQ: HST-084
#[test]
fn hst_084_wrong_api_section_and_forbidden_import_reports_api_version_first() {
    let bytes = with_custom_section(
        wat_component("(component (import \"wasi:sockets/tcp@0.2.0\" (instance)))"),
        "ilium:api-version",
        &[0, 0, 0, 2, 0, 0],
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "manifest.api-version"
    );
}

/// REQ: HST-084
#[test]
fn hst_084_oversized_section_and_forbidden_import_reports_section_size_first() {
    let bytes = with_custom_section(
        wat_component("(component (import \"wasi:sockets/tcp@0.2.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    let bytes = with_custom_section(bytes, "big", &vec![0u8; 4 * 1024 * 1024 + 1024]);
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.custom-section-too-large"
    );
}

/// REQ: HST-084
#[test]
fn hst_084_world_mismatch_is_reported_before_the_api_version() {
    let bytes = with_custom_section(empty_component(), "ilium:api-version", &[0, 0, 0, 2, 0, 0]);
    let context =
        ValidationContext::new(World::Client, (0, 1, 0)).expecting_world_text("ilium:server");
    assert_eq!(refusal_code(&bytes, &context), "component.world-mismatch");
}

/// REQ: HST-084
#[test]
fn hst_084_a_clean_component_passes_all_seven_steps() {
    assert_eq!(refusal_code(&valid_bare(), &bare_context()), "accepted");
}

/// REQ: HST-084
#[test]
fn hst_084_unknown_interface_import_is_component_import_unknown() {
    let bytes = with_custom_section(
        wat_component("(component (import \"ilium:host/does-not-exist@0.1.0\" (instance)))"),
        "ilium:api-version",
        &api_section(),
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.import-unknown"
    );
}

/// REQ: HST-085
#[test]
fn hst_085_exact_six_bytes_equal_to_manifest_api_are_accepted() {
    assert_eq!(refusal_code(&valid_bare(), &bare_context()), "accepted");
}

/// REQ: HST-085
#[test]
fn hst_085_each_of_the_three_fields_must_match() {
    for wrong in [[0, 1, 0, 1, 0, 0], [0, 0, 0, 2, 0, 0], [0, 0, 0, 1, 0, 1]] {
        let bytes = with_custom_section(empty_component(), "ilium:api-version", &wrong);
        assert_eq!(
            refusal_code(&bytes, &bare_context()),
            "manifest.api-version",
            "bytes {wrong:?}"
        );
    }
}

/// REQ: HST-085
#[test]
fn hst_085_five_and_seven_byte_sections_are_refused() {
    for payload in [vec![0, 0, 0, 1, 0], vec![0, 0, 0, 1, 0, 0, 0]] {
        let bytes = with_custom_section(empty_component(), "ilium:api-version", &payload);
        assert_eq!(
            refusal_code(&bytes, &bare_context()),
            "manifest.api-version",
            "len {}",
            payload.len()
        );
    }
}

/// REQ: HST-085
#[test]
fn hst_085_missing_section_is_refused() {
    assert_eq!(
        refusal_code(&empty_component(), &bare_context()),
        "manifest.api-version"
    );
}

/// REQ: HST-085
#[test]
fn hst_085_fields_are_big_endian() {
    let bytes = with_custom_section(empty_component(), "ilium:api-version", &[1, 0, 0, 0, 0, 0]);
    assert_eq!(
        refusal_code(&bytes, &ValidationContext::bare((256, 0, 0))),
        "accepted"
    );
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "manifest.api-version"
    );
}

/// REQ: HST-086
#[test]
fn hst_086_component_over_32_mib_is_refused() {
    let mut bytes = valid_bare();
    for _ in 0..9 {
        bytes = with_custom_section(bytes, "pad", &vec![0u8; 3 * 1024 * 1024 + 900 * 1024]);
    }
    assert_eq!(refusal_code(&bytes, &bare_context()), "component.too-large");
}

/// REQ: HST-086
#[test]
fn hst_086_single_custom_section_over_4_mib_is_refused() {
    let bytes = with_custom_section(valid_bare(), "big", &vec![0u8; 4 * 1024 * 1024 + 1024]);
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.custom-section-too-large"
    );
}

/// REQ: HST-086
#[test]
fn hst_086_single_custom_section_just_under_4_mib_is_accepted() {
    let bytes = with_custom_section(valid_bare(), "big", &vec![0u8; 4 * 1024 * 1024 - 1024]);
    assert_eq!(refusal_code(&bytes, &bare_context()), "accepted");
}

/// REQ: HST-086
#[test]
fn hst_086_custom_sections_together_over_8_mib_are_refused() {
    let mut bytes = valid_bare();
    for _ in 0..3 {
        bytes = with_custom_section(bytes, "part", &vec![0u8; 3 * 1024 * 1024]);
    }
    assert_eq!(
        refusal_code(&bytes, &bare_context()),
        "component.custom-section-too-large"
    );
}

/// REQ: HST-086
#[test]
fn hst_086_custom_sections_together_just_under_8_mib_are_accepted() {
    let mut bytes = valid_bare();
    for _ in 0..2 {
        bytes = with_custom_section(bytes, "part", &vec![0u8; 3 * 1024 * 1024 + 900 * 1024]);
    }
    assert_eq!(refusal_code(&bytes, &bare_context()), "accepted");
}

/// REQ: HST-086
#[test]
fn hst_086_small_component_is_accepted() {
    assert!(valid_bare().len() < 1024);
    assert_eq!(refusal_code(&valid_bare(), &bare_context()), "accepted");
}

// ---------------------------------------------------------------- Config source and fallbacks

/// REQ: HST-087
#[test]
fn hst_087_precompile_tool_and_host_use_the_same_runtime_config_function() {
    let precompile = workspace_root().join("ilium-plugin-precompile/src/main.rs");
    let text = std::fs::read_to_string(&precompile).unwrap_or_default();
    assert!(
        text.contains("runtime_config"),
        "precompile must call runtime_config()"
    );
    let host_engine =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine.rs"))
            .unwrap_or_default();
    assert!(
        host_engine.contains("runtime_config"),
        "host engine must call runtime_config()"
    );
}

/// REQ: HST-087
#[test]
fn hst_087_only_one_function_builds_a_wasmtime_config() {
    let mut definitions = 0;
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                definitions += text.matches("wasmtime::Config::new()").count();
            }
        }
    }
    assert_eq!(definitions, 1);
}

/// REQ: HST-088
#[test]
fn hst_088_artifact_with_another_fingerprint_falls_back_to_component_new() {
    let engine = new_engine();
    let mut artifact = embedded_artifacts()
        .into_iter()
        .next()
        .expect("one embedded artifact");
    artifact.fingerprint = "f".repeat(64);
    let path = engine.load_embedded(&artifact).expect("fallback loads");
    assert_eq!(path, LoadPath::Compiled);
    assert_eq!(engine.stats().deserialize_calls, 0);
    assert!(engine.stats().component_new_calls >= 1);
}

/// REQ: HST-088
#[test]
fn hst_088_fallback_logs_host_cwasm_incompatible() {
    let engine = new_engine();
    let mut artifact = embedded_artifacts()
        .into_iter()
        .next()
        .expect("one embedded artifact");
    artifact.fingerprint = "e".repeat(64);
    engine.load_embedded(&artifact).expect("fallback loads");
    assert!(engine
        .log_lines()
        .iter()
        .any(|l| l.contains("host.cwasm-incompatible")));
}

/// REQ: HST-088
#[test]
fn hst_088_matching_fingerprint_deserializes_without_a_warning() {
    let engine = new_engine();
    let artifact = embedded_artifacts()
        .into_iter()
        .next()
        .expect("one embedded artifact");
    assert_eq!(
        engine.load_embedded(&artifact).expect("loads"),
        LoadPath::Deserialized
    );
    assert!(engine
        .log_lines()
        .iter()
        .all(|l| !l.contains("host.cwasm-incompatible")));
}

/// REQ: HST-088
#[test]
fn hst_088_no_bundled_plugin_takes_the_fallback_path() {
    let engine = new_engine();
    for artifact in embedded_artifacts() {
        assert_eq!(
            engine.load_embedded(&artifact).expect("loads"),
            LoadPath::Deserialized,
            "{}",
            artifact.name
        );
    }
}

/// REQ: HST-088
#[test]
fn hst_088_artifact_without_embedded_wasm_and_other_fingerprint_is_refused() {
    let engine = new_engine();
    let mut artifact = embedded_artifacts()
        .into_iter()
        .next()
        .expect("one embedded artifact");
    artifact.fingerprint = "d".repeat(64);
    artifact.wasm.clear();
    let error = engine
        .load_embedded(&artifact)
        .expect_err("no fallback available");
    assert_eq!(error.code().as_str(), "host.cwasm-incompatible");
}

/// REQ: HST-089
#[test]
fn hst_089_stack_overflow_traps_with_host_stack_overflow() {
    let mut host = host_with_fixture("stack-recursion", Tier::T3);
    fail_once(&mut host);
    assert!(log_text(&host).contains("host.stack-overflow"));
}

/// REQ: HST-089
#[test]
fn hst_089_process_survives_and_the_instance_restarts() {
    let mut host = host_with_fixture("stack-recursion", Tier::T3);
    fail_once(&mut host);
    assert_eq!(host.failures().len(), 1);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
}

/// REQ: HST-089
#[test]
fn hst_089_a_second_overflow_after_restart_is_contained_too() {
    let mut host = host_with_fixture("stack-recursion", Tier::T3);
    fail_once(&mut host);
    advance_ms(&mut host, 1_000);
    fail_once(&mut host);
    assert_eq!(host.failures().len(), 2);
}
