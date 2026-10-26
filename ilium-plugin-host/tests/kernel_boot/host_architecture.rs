//! reference-host-architecture.md: crate layout and dependency edges, ownership, start and
//! failure containment rows that are unit tests of the host crate (HST-001..006, 015, 018,
//! 021..023, 027, 028, 036..038, 043..045).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ilium_plugin_api::kernel::KernelFacade;
use ilium_plugin_api::{ProcessKind, Side, Tier};
use ilium_plugin_host::lifecycle::StartMode;
use ilium_plugin_host::registry::GrantStore;
use ilium_plugin_host::testing::boot::{BootHarness, PluginSpec};
use ilium_plugin_host::testing::{FakeKernel, HostHarness};
use proptest::prelude::*;

use crate::common::*;

fn startup_plugin(id: &str) -> PluginSpec {
    PluginSpec::new(id, Tier::T1)
        .core()
        .activation(&["startup"])
}

fn lazy_plugin(id: &str) -> PluginSpec {
    PluginSpec::new(id, Tier::T1)
        .core()
        .activation(&["command:open"])
}

fn crate_named(name: &str) -> CrateManifest {
    workspace_crates()
        .into_iter()
        .find(|krate| krate.name == name)
        .unwrap_or_else(|| panic!("workspace crate {name} exists"))
}

// ---------------------------------------------------------------- HST-001

/// REQ: HST-001
#[test]
fn hst_001_workspace_contains_the_three_plugin_crates() {
    let names: BTreeSet<String> = workspace_crates()
        .into_iter()
        .map(|krate| krate.name)
        .collect();
    for required in [
        "ilium-plugin-api",
        "ilium-plugin-host",
        "ilium-plugin-helper",
    ] {
        assert!(names.contains(required), "workspace lacks {required}");
    }
}

/// REQ: HST-001
#[test]
fn hst_001_api_crate_depends_on_no_runtime_or_io_crate() {
    let api = crate_named("ilium-plugin-api");
    for forbidden in API_FORBIDDEN_CRATES {
        assert!(
            !api.dependencies.contains(forbidden),
            "ilium-plugin-api depends on {forbidden}"
        );
    }
    let ilium_edges: Vec<&String> = api
        .dependencies
        .iter()
        .filter(|name| name.starts_with("ilium"))
        .collect();
    assert!(
        ilium_edges.is_empty(),
        "ilium-plugin-api has ilium edges: {ilium_edges:?}"
    );
}

/// REQ: HST-001
#[test]
fn hst_001_api_sources_name_no_runtime_or_io_paths() {
    let sources = rust_sources(&workspace_root().join("ilium-plugin-api/src"));
    assert!(!sources.is_empty());
    for (path, text) in sources {
        for pattern in [
            "wasmtime::",
            "tokio::",
            "std::fs::",
            "std::net::",
            "std::process::",
        ] {
            assert!(!text.contains(pattern), "{path} contains {pattern}");
        }
    }
}

/// REQ: HST-001
#[test]
fn hst_001_checker_flags_wasmtime_and_tokio_in_the_api_crate() {
    let crates = [synthetic_crate(
        "ilium-plugin-api",
        "ilium-plugin-api",
        &["serde", "wasmtime", "tokio"],
    )];
    let edges = forbidden_edges(&crates);
    assert_eq!(
        edges,
        vec![
            "ilium-plugin-api -> tokio".to_owned(),
            "ilium-plugin-api -> wasmtime".to_owned()
        ]
    );
}

// ---------------------------------------------------------------- HST-002

/// REQ: HST-002
#[test]
fn hst_002_real_workspace_has_no_forbidden_edge() {
    let edges = forbidden_edges(&workspace_crates());
    assert!(edges.is_empty(), "forbidden edges: {edges:?}");
}

/// REQ: HST-002
#[test]
fn hst_002_host_to_core_transport_and_ipc_are_forbidden() {
    for target in ["ilium-core", "ilium-transport", "ilium-ipc"] {
        let crates = [synthetic_crate(
            "ilium-plugin-host",
            "ilium-plugin-host",
            &["ilium-plugin-api", target],
        )];
        assert_eq!(
            forbidden_edges(&crates),
            vec![format!("ilium-plugin-host -> {target}")],
            "edge to {target}"
        );
    }
}

/// REQ: HST-002
#[test]
fn hst_002_host_to_api_execution_platform_logging_are_allowed() {
    let crates = [synthetic_crate(
        "ilium-plugin-host",
        "ilium-plugin-host",
        &[
            "ilium-plugin-api",
            "ilium-execution",
            "ilium-platform",
            "ilium-logging",
        ],
    )];
    assert!(forbidden_edges(&crates).is_empty());
}

/// REQ: HST-002
#[test]
fn hst_002_ipc_to_host_is_forbidden_and_ipc_to_api_is_allowed() {
    let bad = [synthetic_crate(
        "ilium-ipc",
        "ilium-ipc",
        &["ilium-plugin-host"],
    )];
    assert_eq!(
        forbidden_edges(&bad),
        vec!["ilium-ipc -> ilium-plugin-host".to_owned()]
    );
    let good = [synthetic_crate(
        "ilium-ipc",
        "ilium-ipc",
        &["ilium-plugin-api"],
    )];
    assert!(forbidden_edges(&good).is_empty());
}

/// REQ: HST-002
#[test]
fn hst_002_server_client_and_cli_may_depend_on_the_host() {
    for name in ["ilium-server", "ilium-client", "ilium"] {
        let crates = [synthetic_crate(
            name,
            name,
            &["ilium-plugin-host", "ilium-plugin-api"],
        )];
        assert!(
            forbidden_edges(&crates).is_empty(),
            "{name} -> host must be allowed"
        );
    }
}

/// REQ: HST-002
#[test]
fn hst_002_plugin_crates_may_depend_only_on_the_sdk() {
    let bad = [synthetic_crate(
        "plugins/core/tree",
        "ilium-core-tree",
        &["ilium-plugin-sdk", "ilium-core"],
    )];
    assert_eq!(
        forbidden_edges(&bad),
        vec!["ilium-core-tree -> ilium-core".to_owned()]
    );
    let good = [synthetic_crate(
        "plugins/core/tree",
        "ilium-core-tree",
        &["ilium-plugin-sdk", "serde"],
    )];
    assert!(forbidden_edges(&good).is_empty());
}

/// REQ: HST-002
#[test]
fn hst_002_sdk_family_may_not_reach_outside_the_family() {
    let bad = [synthetic_crate(
        "ilium-plugin-sdk-core",
        "ilium-plugin-sdk-core",
        &["ilium-plugin-api"],
    )];
    assert_eq!(
        forbidden_edges(&bad),
        vec!["ilium-plugin-sdk-core -> ilium-plugin-api".to_owned()]
    );
    let good = [synthetic_crate(
        "ilium-plugin-sdk",
        "ilium-plugin-sdk",
        &["ilium-plugin-sdk-core", "ilium-plugin-sdk-macros"],
    )];
    assert!(forbidden_edges(&good).is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    // REQ: HST-002
    #[test]
    fn hst_002_crate_names_are_matched_exactly_not_by_prefix(suffix in "[a-z]{1,8}") {
        let name = format!("ilium-plugin-host-{suffix}");
        let crates = [synthetic_crate("x", &name, &["ilium-core", "ilium-ipc"])];
        prop_assert!(forbidden_edges(&crates).is_empty());
    }
}

// ---------------------------------------------------------------- HST-003

/// REQ: HST-003
#[test]
fn hst_003_core_depends_on_no_plugin_crate() {
    let core = crate_named("ilium-core");
    let plugin_edges: Vec<&String> = core
        .dependencies
        .iter()
        .filter(|name| name.starts_with("ilium-plugin"))
        .collect();
    assert!(
        plugin_edges.is_empty(),
        "ilium-core edges: {plugin_edges:?}"
    );
}

/// REQ: HST-003
#[test]
fn hst_003_core_sources_name_no_plugin_crate() {
    for (path, text) in rust_sources(&workspace_root().join("ilium-core/src")) {
        assert!(
            !text.contains("ilium_plugin"),
            "{path} names a plugin crate"
        );
    }
}

/// REQ: HST-003
#[test]
fn hst_003_detect_and_pty_depend_on_no_plugin_crate() {
    for name in ["ilium-detect", "ilium-pty"] {
        let krate = crate_named(name);
        assert!(
            !krate
                .dependencies
                .iter()
                .any(|dependency| dependency.starts_with("ilium-plugin")),
            "{name} depends on a plugin crate"
        );
    }
}

// ---------------------------------------------------------------- HST-004

/// REQ: HST-004
#[test]
fn hst_004_ipc_depends_on_the_api_crate_and_not_on_the_host() {
    let ipc = crate_named("ilium-ipc");
    assert!(
        ipc.dependencies.contains("ilium-plugin-api"),
        "ilium-ipc must carry api records"
    );
    assert!(!ipc.dependencies.contains("ilium-plugin-host"));
    assert!(!ipc.dependencies.contains("wasmtime"));
}

/// REQ: HST-004
#[test]
fn hst_004_ipc_sources_name_no_wasmtime_value() {
    for (path, text) in rust_sources(&workspace_root().join("ilium-ipc/src")) {
        assert!(!text.contains("wasmtime"), "{path} names wasmtime");
        assert!(
            !text.contains("ilium_plugin_host"),
            "{path} names the host crate"
        );
    }
}

/// REQ: HST-004
#[test]
fn hst_004_ipc_plugin_module_uses_api_record_types() {
    let text = read_text(&workspace_root().join("ilium-ipc/src/plugin.rs"));
    assert!(
        text.contains("ilium_plugin_api"),
        "plugin traffic must be typed by the api crate"
    );
}

// ---------------------------------------------------------------- HST-005

fn is_plugin_host_module(path: &str) -> bool {
    path == "plugin_host.rs" || path.starts_with("plugin_host/")
}

/// REQ: HST-005
#[test]
fn hst_005_client_modules_other_than_plugin_host_name_no_wasmtime() {
    let sources = rust_sources(&workspace_root().join("ilium-client/src"));
    assert!(!sources.is_empty());
    for (path, text) in sources {
        if is_plugin_host_module(&path) {
            continue;
        }
        assert!(!text.contains("wasmtime"), "{path} names a wasmtime type");
    }
}

/// REQ: HST-005
#[test]
fn hst_005_only_the_plugin_host_module_names_the_host_crate() {
    for (path, text) in rust_sources(&workspace_root().join("ilium-client/src")) {
        if is_plugin_host_module(&path) {
            continue;
        }
        assert!(
            !text.contains("ilium_plugin_host"),
            "{path} depends on ilium-plugin-host but only the plugin_host module may"
        );
    }
}

/// REQ: HST-005
#[test]
fn hst_005_client_manifest_has_no_wasmtime_dependency() {
    let client = crate_named("ilium-client");
    assert!(!client.dependencies.contains("wasmtime"));
}

// ---------------------------------------------------------------- HST-006

/// REQ: HST-006
#[test]
fn hst_006_host_sources_name_no_kernel_crate() {
    for (path, text) in rust_sources(&workspace_root().join("ilium-plugin-host/src")) {
        for forbidden in ["ilium_core", "ilium_transport", "ilium_ipc", "ilium_pty"] {
            assert!(!text.contains(forbidden), "{path} names {forbidden}");
        }
    }
}

/// REQ: HST-006
#[test]
fn hst_006_fake_kernel_is_usable_as_an_object_safe_facade() {
    let kernel: Arc<dyn KernelFacade> = Arc::new(FakeKernel::new(ProcessKind::Server));
    assert_eq!(kernel.process_kind(), ProcessKind::Server);
    assert_eq!(kernel.tree().nodes().len(), 0);
}

/// REQ: HST-006
#[test]
fn hst_006_host_runs_against_a_fake_kernel_with_no_pty() {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Server));
    let handle = HostHarness::new()
        .kernel(Arc::clone(&kernel))
        .build()
        .expect("host builds on a fake kernel");
    let report = handle.pump(8);
    assert_eq!(report.traps, 0);
    assert!(handle.failures().is_empty());
}

// ---------------------------------------------------------------- HST-015

/// REQ: HST-015
#[test]
fn hst_015_scene_stores_are_separate_from_client_host_stores() {
    let mut run = BootHarness::new(ProcessKind::Client)
        .plugin(
            PluginSpec::new("ilium.core.scenes", Tier::T1)
                .core()
                .sides(&[Side::Client, Side::Scene])
                .client_activation(&["startup"]),
        )
        .build();
    run.start();
    let client_stores: BTreeSet<u64> = run
        .store_ids("ilium.core.scenes", Side::Client)
        .into_iter()
        .collect();
    let scene_stores: BTreeSet<u64> = run
        .store_ids("ilium.core.scenes", Side::Scene)
        .into_iter()
        .collect();
    assert!(!client_stores.is_empty());
    assert!(!scene_stores.is_empty());
    assert!(client_stores.is_disjoint(&scene_stores));
}

/// REQ: HST-015
#[test]
fn hst_015_scene_frame_runs_on_neither_the_ui_thread_nor_an_executor_thread() {
    let mut run = BootHarness::new(ProcessKind::Client)
        .plugin(
            PluginSpec::new("ilium.core.scenes", Tier::T1)
                .core()
                .sides(&[Side::Scene])
                .client_activation(&["startup"]),
        )
        .build();
    run.start();
    let frame_thread = run.scene_frame_thread("ilium.core.scenes");
    assert_ne!(frame_thread, run.client_ui_thread());
    assert!(!run.client_executor_threads().contains(&frame_thread));
}

/// REQ: HST-015
#[test]
fn hst_015_every_scene_instance_has_its_own_store() {
    let mut run = BootHarness::new(ProcessKind::Client)
        .plugin(
            PluginSpec::new("ilium.core.scenes", Tier::T1)
                .core()
                .sides(&[Side::Scene])
                .client_activation(&["startup"]),
        )
        .build();
    run.start();
    run.start_scene_instance("ilium.core.scenes");
    run.start_scene_instance("ilium.core.scenes");
    let stores = run.store_ids("ilium.core.scenes", Side::Scene);
    let distinct: BTreeSet<u64> = stores.iter().copied().collect();
    assert!(stores.len() >= 2);
    assert_eq!(distinct.len(), stores.len());
}

// ---------------------------------------------------------------- HST-018

fn jobs_run(limit: u64) -> ilium_plugin_host::testing::boot::BootRun {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.search").limit("concurrent_jobs", limit))
        .build();
    run.start();
    run
}

/// REQ: HST-018
#[test]
fn hst_018_jobs_below_the_limit_all_run() {
    let mut run = jobs_run(2);
    run.start_jobs("ilium.core.search", 1);
    let stats = run.job_stats("ilium.core.search");
    assert_eq!((stats.live, stats.queued), (1, 0));
}

/// REQ: HST-018
#[test]
fn hst_018_jobs_at_the_limit_all_run() {
    let mut run = jobs_run(2);
    run.start_jobs("ilium.core.search", 2);
    let stats = run.job_stats("ilium.core.search");
    assert_eq!((stats.live, stats.queued), (2, 0));
}

/// REQ: HST-018
#[test]
fn hst_018_the_job_past_the_limit_waits_in_the_queue() {
    let mut run = jobs_run(2);
    run.start_jobs("ilium.core.search", 3);
    let stats = run.job_stats("ilium.core.search");
    assert_eq!((stats.live, stats.queued), (2, 1));
    assert_eq!(stats.peak, 2);
}

/// REQ: HST-018
#[test]
fn hst_018_a_finished_job_lets_the_queued_job_start() {
    let mut run = jobs_run(2);
    run.start_jobs("ilium.core.search", 3);
    run.complete_one_job("ilium.core.search");
    let stats = run.job_stats("ilium.core.search");
    assert_eq!((stats.live, stats.queued), (2, 0));
    assert_eq!(stats.peak, 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    // REQ: HST-018
    #[test]
    fn hst_018_live_jobs_never_exceed_the_limit(limit in 1u64..=8, jobs in 0usize..40) {
        let mut run = jobs_run(limit);
        run.start_jobs("ilium.core.search", jobs);
        let stats = run.job_stats("ilium.core.search");
        prop_assert!(stats.live as u64 <= limit);
        prop_assert!(stats.peak as u64 <= limit);
        prop_assert_eq!(stats.live + stats.queued, jobs);
    }
}

// ---------------------------------------------------------------- HST-021

/// REQ: HST-021
#[test]
fn hst_021_one_engine_per_server_process() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .plugin(startup_plugin("ilium.core.cost"))
        .plugin(lazy_plugin("ilium.core.recap"))
        .build();
    run.start();
    assert_eq!(run.engine_count(), 1);
}

/// REQ: HST-021
#[test]
fn hst_021_every_store_comes_from_the_one_engine() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .plugin(startup_plugin("ilium.core.cost"))
        .build();
    run.start();
    let engine = run.engine_id();
    for plugin in ["ilium.core.titles", "ilium.core.cost"] {
        for store in run.store_ids(plugin, Side::Server) {
            assert_eq!(
                run.engine_of_store(store),
                engine,
                "store {store} of {plugin}"
            );
        }
    }
}

/// REQ: HST-021
#[test]
fn hst_021_a_restart_of_a_plugin_does_not_create_a_second_engine() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    run.start();
    run.trap_in("ilium.core.titles", Side::Server);
    run.advance(Duration::from_secs(2));
    assert_eq!(run.engine_count(), 1);
}

/// REQ: HST-021
#[test]
fn hst_021_the_cli_process_also_creates_exactly_one_engine() {
    let mut run = BootHarness::new(ProcessKind::Cli)
        .plugin(
            PluginSpec::new("ilium.core.recap", Tier::T1)
                .core()
                .activation(&["cli:recap"]),
        )
        .build();
    let cli = run.run_cli("ilium.core.recap", "recap");
    assert_eq!(run.engine_count(), 1);
    assert!(cli.activated.contains(&"ilium.core.recap".to_owned()));
}

// ---------------------------------------------------------------- HST-022

/// REQ: HST-022
#[test]
fn hst_022_each_plugin_instance_has_a_distinct_store() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .plugin(startup_plugin("ilium.core.cost"))
        .build();
    run.start();
    let mut all: Vec<u64> = run.store_ids("ilium.core.titles", Side::Server);
    all.extend(run.store_ids("ilium.core.cost", Side::Server));
    let distinct: BTreeSet<u64> = all.iter().copied().collect();
    assert_eq!(all.len(), 2);
    assert_eq!(distinct.len(), all.len());
}

/// REQ: HST-022
#[test]
fn hst_022_job_instances_have_stores_distinct_from_the_main_instance() {
    let mut run = jobs_run(2);
    let main_stores = run.store_ids("ilium.core.search", Side::Server);
    run.start_jobs("ilium.core.search", 2);
    let job_stores = run.job_store_ids("ilium.core.search");
    assert_eq!(job_stores.len(), 2);
    let job_set: BTreeSet<u64> = job_stores.iter().copied().collect();
    assert_eq!(job_set.len(), 2);
    for store in main_stores {
        assert!(!job_set.contains(&store));
    }
}

/// REQ: HST-022
#[test]
fn hst_022_a_restarted_instance_gets_a_fresh_store() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    run.start();
    let before = run.store_ids("ilium.core.titles", Side::Server);
    run.trap_in("ilium.core.titles", Side::Server);
    run.advance(Duration::from_secs(2));
    let after = run.store_ids("ilium.core.titles", Side::Server);
    assert!(after.iter().any(|store| !before.contains(store)));
}

// ---------------------------------------------------------------- HST-023

fn temp_grant_path() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("grants.json");
    (dir, path)
}

fn some_grants(count: usize) -> Vec<ilium_plugin_api::grants::Grant> {
    ["tree.read", "agent.read", "sound.play", "notify.toast"]
        .iter()
        .take(count)
        .map(|text| ilium_plugin_api::grants::Grant::parse(text).expect("known grant"))
        .collect()
}

/// REQ: HST-023
#[test]
fn hst_023_a_second_writer_cannot_open_the_store_while_the_first_holds_it() {
    let (_dir, path) = temp_grant_path();
    {
        let _first = GrantStore::open(&path).expect("first writer opens");
        let second = GrantStore::open(&path);
        // manual does not pin the lock-refusal error code, so only the refusal is asserted.
        assert!(second.is_err(), "a second writer must be refused");
    }
    assert!(
        GrantStore::open(&path).is_ok(),
        "the lock is released on drop"
    );
}

/// REQ: HST-023
#[test]
fn hst_023_the_store_file_is_replaced_atomically_and_owner_only() {
    let (_dir, path) = temp_grant_path();
    let mut store = GrantStore::open(&path).expect("open");
    let plugin = ilium_plugin_api::ids::PluginId("ilium.core.tree".to_owned());
    store.set_granted(&plugin, &some_grants(2)).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("file exists")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let parsed: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).expect("read"))
        .expect("valid json after a write");
    assert!(parsed.is_object() || parsed.is_array());
}

/// REQ: HST-023
#[test]
fn hst_023_a_reader_never_sees_a_partial_file_while_the_writer_replaces_it() {
    let (_dir, path) = temp_grant_path();
    let mut store = GrantStore::open(&path).expect("open");
    let plugin = ilium_plugin_api::ids::PluginId("ilium.core.tree".to_owned());
    store.set_granted(&plugin, &some_grants(1)).expect("seed");
    let reader_path = path.clone();
    let reader = std::thread::spawn(move || {
        let mut partial = 0usize;
        for _ in 0..400 {
            let bytes = std::fs::read(&reader_path).unwrap_or_default();
            if !bytes.is_empty() && serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
                partial += 1;
            }
        }
        partial
    });
    for round in 0..60 {
        store
            .set_granted(&plugin, &some_grants(1 + round % 4))
            .expect("write");
    }
    assert_eq!(reader.join().expect("reader thread"), 0, "partial reads");
}

/// REQ: HST-023
#[test]
fn hst_023_concurrent_openers_never_hold_the_writer_lock_together() {
    let (_dir, path) = temp_grant_path();
    let holders = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max_seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut threads = Vec::new();
    for _ in 0..4 {
        let path = path.clone();
        let holders = Arc::clone(&holders);
        let max_seen = Arc::clone(&max_seen);
        threads.push(std::thread::spawn(move || {
            for _ in 0..30 {
                if let Ok(_store) = GrantStore::open(&path) {
                    let now = holders.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                    holders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }));
    }
    for thread in threads {
        thread.join().expect("opener thread");
    }
    assert!(max_seen.load(std::sync::atomic::Ordering::SeqCst) <= 1);
}

// ---------------------------------------------------------------- HST-027

/// REQ: HST-027
#[test]
fn hst_027_non_startup_plugins_are_verified_but_not_instantiated() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(lazy_plugin("ilium.core.recap"))
        .plugin(lazy_plugin("ilium.core.search"))
        .build();
    run.start();
    assert_eq!(run.instantiations("ilium.core.recap"), 0);
    assert_eq!(run.instantiations("ilium.core.search"), 0);
    let trace = run.trace();
    assert!(position_of(&trace, "verify:ilium.core.recap").is_some());
    assert!(position_of(&trace, "register:ilium.core.recap").is_some());
}

/// REQ: HST-027
#[test]
fn hst_027_startup_plugins_are_instantiated_exactly_once() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    run.start();
    assert_eq!(run.instantiations("ilium.core.titles"), 1);
}

/// REQ: HST-027
#[test]
fn hst_027_all_eager_steps_precede_the_first_activation() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .plugin(startup_plugin("ilium.core.cost"))
        .plugin(lazy_plugin("ilium.core.recap"))
        .build();
    run.start();
    let trace = run.trace();
    let first_activation = trace
        .iter()
        .position(|entry| entry.starts_with("activate:"))
        .expect("an activation");
    for plugin in ["ilium.core.titles", "ilium.core.cost", "ilium.core.recap"] {
        let verified = position_of(&trace, &format!("verify:{plugin}")).expect("verified");
        let registered = position_of(&trace, &format!("register:{plugin}")).expect("registered");
        assert!(
            verified < first_activation,
            "{plugin} verified after the first activation"
        );
        assert!(
            registered < first_activation,
            "{plugin} registered after the first activation"
        );
    }
}

/// REQ: HST-027
#[test]
fn hst_027_a_lazy_plugin_is_instantiated_when_its_trigger_fires() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(lazy_plugin("ilium.core.recap"))
        .build();
    run.start();
    run.fire_trigger("command:open");
    assert_eq!(run.instantiations("ilium.core.recap"), 1);
}

// ---------------------------------------------------------------- HST-028

/// REQ: HST-028
#[test]
fn hst_028_a_provider_activates_before_its_consumer() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.consumer").requires(&["ilium.core.provider"]))
        .plugin(startup_plugin("ilium.core.provider"))
        .build();
    run.start();
    let trace = run.trace();
    let provider = position_of(&trace, "activate:ilium.core.provider").expect("provider activated");
    let consumer = position_of(&trace, "activate:ilium.core.consumer").expect("consumer activated");
    assert!(provider < consumer);
}

/// REQ: HST-028
#[test]
fn hst_028_a_two_plugin_cycle_leaves_both_disabled_with_requires_cycle() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.alpha").requires(&["ilium.core.beta"]))
        .plugin(startup_plugin("ilium.core.beta").requires(&["ilium.core.alpha"]))
        .build();
    let report = run.start();
    for plugin in ["ilium.core.alpha", "ilium.core.beta"] {
        assert_eq!(
            run.disabled_reason(plugin).as_deref(),
            Some("manifest.requires-cycle"),
            "{plugin}"
        );
        assert_eq!(run.instantiations(plugin), 0);
        assert!(report
            .failed
            .iter()
            .any(|(id, code)| id == plugin && code == "manifest.requires-cycle"));
    }
}

/// REQ: HST-028
#[test]
fn hst_028_a_cycle_does_not_disable_unrelated_plugins() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.alpha").requires(&["ilium.core.beta"]))
        .plugin(startup_plugin("ilium.core.beta").requires(&["ilium.core.alpha"]))
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    let report = run.start();
    assert!(report.started.contains(&"ilium.core.titles".to_owned()));
    assert_eq!(run.disabled_reason("ilium.core.titles"), None);
}

/// REQ: HST-028
#[test]
fn hst_028_a_three_plugin_cycle_disables_all_three() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.a1").requires(&["ilium.core.a2"]))
        .plugin(startup_plugin("ilium.core.a2").requires(&["ilium.core.a3"]))
        .plugin(startup_plugin("ilium.core.a3").requires(&["ilium.core.a1"]))
        .build();
    run.start();
    for plugin in ["ilium.core.a1", "ilium.core.a2", "ilium.core.a3"] {
        assert_eq!(
            run.disabled_reason(plugin).as_deref(),
            Some("manifest.requires-cycle")
        );
    }
}

// ---------------------------------------------------------------- HST-036

/// REQ: HST-036
#[test]
fn hst_036_a_panic_in_an_import_traps_with_host_import_panic() {
    let handle = HostHarness::new()
        .panic_in_import("tree.get")
        .build()
        .expect("build");
    handle.deliver(Vec::new());
    handle.pump(8);
    let failures = handle.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code.as_str(), "host.import-panic");
}

/// REQ: HST-036
#[test]
fn hst_036_the_dispatcher_stays_alive_after_an_import_panic() {
    let handle = HostHarness::new()
        .panic_in_import("tree.get")
        .build()
        .expect("build");
    handle.deliver(Vec::new());
    handle.pump(8);
    let second = handle.pump(4);
    assert!(second.rounds <= 4);
}

/// REQ: HST-036
#[test]
fn hst_036_the_panic_is_logged_with_the_import_name() {
    let handle = HostHarness::new()
        .panic_in_import("tree.get")
        .build()
        .expect("build");
    handle.deliver(Vec::new());
    handle.pump(8);
    assert!(handle
        .logs()
        .iter()
        .any(|line| line.message.contains("tree.get")));
}

// ---------------------------------------------------------------- HST-037

/// REQ: HST-037
#[test]
fn hst_037_engine_failure_starts_no_plugin_host() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .fail_engine_creation()
        .build();
    let report = run.start();
    assert!(!run.host_started());
    assert!(report.started.is_empty());
}

/// REQ: HST-037
#[test]
fn hst_037_engine_failure_logs_host_engine_failed() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .fail_engine_creation()
        .build();
    run.start();
    assert!(run
        .logs()
        .iter()
        .any(|line| line.message.contains("host.engine-failed")));
}

/// REQ: HST-037
#[test]
fn hst_037_engine_failure_keeps_the_session_running_like_no_plugins() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .fail_engine_creation()
        .build();
    run.start();
    assert!(run.kernel_alive());
    let pane = run.kernel_new_pane();
    run.kernel_type(pane, "echo ok");
    assert!(run.kernel_screen(pane).contains("echo ok"));
}

// ---------------------------------------------------------------- HST-038

/// REQ: HST-038
#[test]
fn hst_038_no_kernel_lock_is_held_while_a_guest_call_runs() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .restored_tree(5)
        .build();
    run.start();
    run.deliver_tree_traffic("ilium.core.titles", 20);
    assert_eq!(run.max_kernel_locks_held_during_guest_calls(), 0);
}

/// REQ: HST-038
#[test]
fn hst_038_an_import_releases_its_kernel_locks_when_its_work_ends() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .restored_tree(5)
        .build();
    run.start();
    run.deliver_tree_traffic("ilium.core.titles", 5);
    assert_eq!(run.kernel_locks_held(), 0);
}

// ---------------------------------------------------------------- HST-043

/// REQ: HST-043
#[test]
fn hst_043_no_plugins_mode_starts_no_host_and_no_engine() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .mode(StartMode::NoPlugins)
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    let report = run.start();
    assert!(!run.host_started());
    assert_eq!(run.engine_count(), 0);
    assert!(report.started.is_empty());
}

/// REQ: HST-043
#[test]
fn hst_043_safe_mode_does_start_the_host_for_core_plugins() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .mode(StartMode::SafePlugins)
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    let report = run.start();
    assert!(run.host_started());
    assert!(report.started.contains(&"ilium.core.titles".to_owned()));
}

// ---------------------------------------------------------------- HST-044

/// REQ: HST-044
#[test]
fn hst_044_an_unreadable_grant_store_refuses_t2_and_t3_plugins() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(PluginSpec::new("acme.t2plugin", Tier::T2).activation(&["startup"]))
        .plugin(PluginSpec::new("acme.t3plugin", Tier::T3).activation(&["startup"]))
        .corrupt_grant_store()
        .build();
    run.start();
    for plugin in ["acme.t2plugin", "acme.t3plugin"] {
        assert_eq!(run.instantiations(plugin), 0, "{plugin}");
        assert_eq!(
            run.disabled_reason(plugin).as_deref(),
            Some("host.grants-unreadable"),
            "{plugin}"
        );
    }
}

/// REQ: HST-044
#[test]
fn hst_044_an_unreadable_grant_store_grants_a_t1_plugin_only_its_defaults() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(
            startup_plugin("ilium.core.titles")
                .default_grants(&["tree.read"])
                .stored_grants(&["tree.read", "tree.write"]),
        )
        .corrupt_grant_store()
        .build();
    run.start();
    assert_eq!(run.instantiations("ilium.core.titles"), 1);
    assert_eq!(
        run.granted("ilium.core.titles"),
        vec!["tree.read".to_owned()]
    );
}

/// REQ: HST-044
#[test]
fn hst_044_a_readable_grant_store_applies_the_stored_grants() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(
            startup_plugin("ilium.core.titles")
                .default_grants(&["tree.read"])
                .stored_grants(&["tree.read", "tree.write"]),
        )
        .build();
    run.start();
    let granted: BTreeSet<String> = run.granted("ilium.core.titles").into_iter().collect();
    assert!(granted.contains("tree.write"));
}

// ---------------------------------------------------------------- HST-045

/// REQ: HST-045
#[test]
fn hst_045_a_package_with_a_changed_hash_is_not_loaded() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(PluginSpec::new("acme.hashed", Tier::T3).activation(&["startup"]))
        .tamper_package("acme.hashed")
        .build();
    let report = run.start();
    assert_eq!(run.instantiations("acme.hashed"), 0);
    assert!(report.failed.iter().any(|(id, _)| id == "acme.hashed"));
}

/// REQ: HST-045
#[test]
fn hst_045_the_load_refusal_is_package_corrupt_and_the_plugin_stays_disabled() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(PluginSpec::new("acme.hashed", Tier::T3).activation(&["startup"]))
        .tamper_package("acme.hashed")
        .build();
    let report = run.start();
    assert!(report
        .failed
        .iter()
        .any(|(id, code)| id == "acme.hashed" && code == "package.corrupt"));
    assert_eq!(
        run.disabled_reason("acme.hashed").as_deref(),
        Some("package.corrupt")
    );
}

/// REQ: HST-045
#[test]
fn hst_045_an_unchanged_package_loads() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup_plugin("ilium.core.titles"))
        .build();
    let report = run.start();
    assert!(report.failed.is_empty());
    assert_eq!(run.instantiations("ilium.core.titles"), 1);
}

/// REQ: HST-045
#[test]
fn hst_045_the_install_time_hash_check_uses_manifest_file_hash() {
    // The requirements table names `manifest.file-hash` (a `files` entry whose SHA-256 differs);
    // the architecture page names `package.corrupt` for the load-time store check.
    assert_eq!(
        ilium_plugin_api::codes::manifest::FILE_HASH,
        "manifest.file-hash"
    );
    assert_eq!(ilium_plugin_api::codes::package::CORRUPT, "package.corrupt");
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(PluginSpec::new("acme.hashed", Tier::T3).activation(&["startup"]))
        .tamper_package("acme.hashed")
        .build();
    let diagnostics = run.verify_install_hashes("acme.hashed");
    assert!(diagnostics.iter().any(|code| code == "manifest.file-hash"));
}
