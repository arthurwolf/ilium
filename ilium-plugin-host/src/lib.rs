//! The plugin host of Ilium (contract `ilium:plugin@0.1.0`).
//!
//! The host embeds the WebAssembly engine, owns the per-plugin queues and the dispatcher, the
//! package store and installer, the lifecycle manager, the host imports of every WIT interface
//! and the `ilium plugin ...` commands. It reaches the application only through the
//! object-safe `ilium_plugin_api::kernel::KernelFacade`; it has no edge to `ilium-core`,
//! `ilium-transport` or `ilium-ipc` (test plan section 1.2, reference-host-architecture.md).
//!
//! Skeleton state: data types, constants and closed enums are complete. Every function with
//! behaviour is `unimplemented!("host::<path>")` until the implementation phase.
//!
//! Manual: reference-host-architecture.md, reference-host-runtime.md, reference-runtime-model.md,
//! reference-packaging.md, reference-cli.md.

// The skeleton functions name their parameters for the implementation that replaces the
// `unimplemented!` bodies, and private fields wait for the code that reads them; until then both
// are unused by design (test plan 1.3).

pub mod approval;
pub mod audit;
pub mod autoupdate;
pub mod calls;
pub mod cli;
pub mod crash_records;
pub mod conformance_hooks;
pub mod consent;
pub mod devtools;
pub mod dispatch;
pub mod engine;
pub mod error;
pub mod extensions;
pub mod helper_client;
pub mod imports;
pub mod install;
pub mod instance;
pub mod ipc;
pub mod lifecycle;
pub mod limits_rules;
pub mod linker;
pub mod package;
pub mod platform_rules;
pub mod policy;
pub mod registry;
pub mod registry_client;
pub mod render;
pub mod resolve;
pub mod review;
pub mod scheduler;
pub mod secrets;
pub mod services;
pub mod session;
pub mod settings;
pub mod signing;
pub mod storage;
pub mod stores;
pub mod sync;
pub mod telemetry;
#[cfg(feature = "testing")]
pub mod testing;

pub use error::HostError;

#[cfg(test)]
mod tests {
    use ilium_plugin_api::kernel::KernelFacade;

    /// Parses the `[dependencies]` table of a manifest and returns its dependency names.
    /// Dev-dependencies are excluded because they do not shape the shipped crate graph.
    fn normal_dependency_names(manifest_text: &str) -> Vec<String> {
        let manifest: toml::Table =
            toml::from_str(manifest_text).expect("manifest must parse as TOML");
        let dependencies = manifest
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .expect("manifest must have a [dependencies] table");
        dependencies.keys().cloned().collect()
    }

    /// Dependency names of this crate's manifest that belong to the Ilium workspace.
    fn ilium_dependency_names() -> Vec<String> {
        let mut names: Vec<String> = normal_dependency_names(include_str!("../Cargo.toml"))
            .into_iter()
            .filter(|name| name.starts_with("ilium-"))
            .collect();
        names.sort();
        names
    }

    /// REQ: HST-001
    #[test]
    fn workspace_contains_the_three_plugin_crates() {
        let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for crate_name in ["ilium-plugin-api", "ilium-plugin-host", "ilium-plugin-helper"] {
            let manifest_path = crate_root.join(crate_name).join("Cargo.toml");
            assert!(
                manifest_path.is_file(),
                "workspace crate {crate_name} must have a Cargo.toml at {}",
                manifest_path.display()
            );
        }
    }

    /// REQ: HST-001
    #[test]
    fn api_crate_has_no_wasmtime_v8_tokio_or_io_dependency() {
        let api_dependencies = normal_dependency_names(include_str!(
            "../../ilium-plugin-api/Cargo.toml"
        ));
        for forbidden in ["wasmtime", "v8", "tokio", "mio", "reqwest", "hyper"] {
            assert!(
                !api_dependencies.iter().any(|name| name == forbidden),
                "ilium-plugin-api must not depend on {forbidden}"
            );
        }
    }

    /// REQ: HST-002
    #[test]
    fn host_ilium_dependencies_are_exactly_the_allowed_edges() {
        let mut expected = vec![
            "ilium-execution".to_string(),
            "ilium-logging".to_string(),
            "ilium-platform".to_string(),
            "ilium-plugin-api".to_string(),
        ];
        expected.sort();
        assert_eq!(ilium_dependency_names(), expected);
    }

    /// REQ: HST-006
    #[test]
    fn host_has_no_edge_to_core_transport_ipc_server_or_client() {
        let names = ilium_dependency_names();
        for forbidden in [
            "ilium-core",
            "ilium-transport",
            "ilium-ipc",
            "ilium-server",
            "ilium-client",
            "ilium-detect",
            "ilium-pty",
        ] {
            assert!(
                !names.iter().any(|name| name == forbidden),
                "ilium-plugin-host must not depend on {forbidden}; it reaches the application only through the kernel facade"
            );
        }
    }

    /// REQ: HST-006
    #[test]
    fn kernel_facade_is_object_safe_for_fake_kernel_testing() {
        fn accepts_dyn_facade(_facade: &dyn KernelFacade) {}
        // Compile-time check: the host takes the facade as a trait object, so a fake kernel can
        // stand in for the real one without a PTY. The function is never called.
        let _: fn(&dyn KernelFacade) = accepts_dyn_facade;
    }
}
