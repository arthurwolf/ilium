//! A declared but missing component must not advertise a working feature.
#![cfg(feature = "testing")]

use std::sync::Arc;

use ilium_plugin_api::{ProcessKind, Side};
use ilium_plugin_host::{
    engine::{EngineConfig, PluginEngine},
    imports::backed::NullLogSink,
    runtime::{CataloguePlugin, HostRuntime, PluginState, RuntimeConfig},
    testing::FakeKernel,
};

fn server_runtime() -> HostRuntime {
    let engine = Arc::new(
        PluginEngine::new(EngineConfig {
            epoch_tick_ms: 10,
            cache_dir: None,
            process: ProcessKind::Server,
        })
        .expect("engine"),
    );
    HostRuntime::new(
        engine,
        RuntimeConfig {
            side: Side::Server,
            kernel: Arc::new(FakeKernel::new(ProcessKind::Server)),
            log: Arc::new(NullLogSink),
            provider: None,
            disabled: Default::default(),
        },
    )
}

#[test]
fn a_missing_declared_component_does_not_register_its_commands() {
    let mut runtime = server_runtime();
    let errors = runtime.load(vec![CataloguePlugin {
        id: "test.missing".to_owned(),
        manifest: r#"{"id":"test.missing","server":{"entry":"server.wasm"},"contributes":{"commands":[{"id":"run"}]}}"#.to_owned(),
        component: None,
    }]);
    assert!(
        !errors.is_empty(),
        "missing declared component must be reported"
    );
    assert_ne!(
        runtime.state("test.missing"),
        Some(PluginState::ManifestOnly)
    );
    assert!(runtime
        .contributions()
        .lock()
        .expect("registry")
        .entries("commands")
        .is_empty());
}

#[test]
fn a_plugin_with_no_server_side_can_still_contribute_its_manifest() {
    let mut runtime = server_runtime();
    let errors = runtime.load(vec![CataloguePlugin {
        id: "test.client-only".to_owned(),
        manifest: r#"{"id":"test.client-only","client":{"entry":"client.wasm"},"contributes":{"commands":[{"id":"run"}]}}"#.to_owned(),
        component: None,
    }]);
    assert!(errors.is_empty());
    assert_eq!(
        runtime.state("test.client-only"),
        Some(PluginState::ManifestOnly)
    );
    assert_eq!(
        runtime
            .contributions()
            .lock()
            .expect("registry")
            .entries("commands")
            .len(),
        1
    );
}
