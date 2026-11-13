//! wasm32 component glue of `#[ilium_plugin]`.
//!
//! For a server or client plugin the macro emits one module, compiled only on wasm32 (and, in
//! a crate that declares the `component-<side>` features, only with that feature). The module holds the wit-bindgen bindings of the export
//! interfaces the plugin declares (import interfaces are shared with the SDK through `with`),
//! the single plugin instance, the host type behind `host()` and the `Guest` implementations.
//! The bodies come from the SDK's generated `__ilium_wasm_*` macros; this file only selects them.
//!
//! Host imports follow the manifest: an interface the host links only on a grant is wired to
//! the imports when `manifest.json` (next to `Cargo.toml`) requests that grant for the side,
//! and to the detached handle otherwise, so the component never imports what the host refuses.

use std::path::{Path, PathBuf};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

use crate::generated_glue::{
    ExportGlue, EXPORTS, EXPORTS_WITHOUT_GLUE, HOST_CALLS, WIT_DIRECTORIES,
};

/// Exports every server and client component has, declared or not (the host requires both).
const REQUIRED_EXPORTS: &[&str] = &["lifecycle", "handler"];

/// The glue module of one plugin struct; empty for a scene plugin.
pub(crate) fn glue(side: &str, exports: &[String], krate: &syn::Path, name: &Ident) -> TokenStream {
    if side == "scene" {
        return TokenStream::new();
    }
    let feature = format!("component-{side}");
    let module = format_ident!("__ilium_component_{}", side);
    let manifest_directory = std::env::var("CARGO_MANIFEST_DIR").map(PathBuf::from).ok();
    let body = match &manifest_directory {
        Some(directory) => match body(side, exports, krate, name, directory) {
            Ok(tokens) => tokens,
            Err(message) => quote!(::core::compile_error!(#message);),
        },
        None => quote!(::core::compile_error!("ilium_plugin: CARGO_MANIFEST_DIR is not set");),
    };
    // A crate that holds both sides of a plugin declares the `component-<side>` features and
    // builds one side per component. A crate with one side declares none and is always that
    // component on wasm32.
    let gate = if manifest_directory
        .as_deref()
        .is_some_and(|directory| declares_feature(directory, &feature))
    {
        quote!(all(target_arch = "wasm32", feature = #feature))
    } else {
        quote!(target_arch = "wasm32")
    };
    quote! {
        #[cfg(#gate)]
        #[doc(hidden)]
        mod #module {
            #body
        }
    }
}

/// True when the crate's `Cargo.toml` declares the feature (a line `<feature> = ...`).
fn declares_feature(manifest_directory: &Path, feature: &str) -> bool {
    std::fs::read_to_string(manifest_directory.join("Cargo.toml"))
        .map(|text| manifest_declares_feature(&text, feature))
        .unwrap_or(false)
}

fn manifest_declares_feature(cargo_toml: &str, feature: &str) -> bool {
    cargo_toml.lines().any(|line| {
        line.trim_start()
            .strip_prefix(feature)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

fn body(
    side: &str,
    exports: &[String],
    krate: &syn::Path,
    name: &Ident,
    manifest_directory: &Path,
) -> Result<TokenStream, String> {
    let selected = selected_exports(side, exports)?;
    let wit_root = wit_root(manifest_directory)?;
    let grants = side_grants(manifest_directory, side)?;
    let krate_text = quote!(#krate).to_string().replace(' ', "");

    let mut world = String::from("package ilium:plugin-glue@0.1.0;\n\nworld component {\n");
    for export in &selected {
        world.push_str(&format!("    export {};\n", export.wit));
    }
    world.push_str("}\n");

    let paths: Vec<String> = WIT_DIRECTORIES
        .iter()
        .map(|directory| wit_root.join(directory).to_string_lossy().into_owned())
        .collect();

    let mut with: Vec<(&str, &str)> = Vec::new();
    for export in &selected {
        for entry in export.with {
            if !with.contains(entry) {
                with.push(*entry);
            }
        }
    }
    with.sort();
    let with_keys = with.iter().map(|(key, _)| *key);
    let with_paths = with
        .iter()
        .map(|(_, suffix)| {
            syn::parse_str::<syn::Path>(&format!("{krate_text}::wasm::bindings::{suffix}"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("ilium_plugin: bindings path does not parse: {error}"))?;
    let runtime_path = format!("{krate_text}::wasm::wit_bindgen::rt");

    let host_side = format_ident!("{}", side);
    let host_macros = HOST_CALLS.iter().map(|host| {
        let glue_macro = format_ident!("{}", host.glue_macro);
        if host.always || granted(&grants, host.grant) {
            quote!(#krate::#glue_macro!(#host_side __IliumHost);)
        } else {
            quote!(#krate::#glue_macro!(off __IliumHost);)
        }
    });

    let export_macros = selected.iter().map(|export| {
        let glue_macro = format_ident!("{}", export.glue_macro);
        quote!(#krate::#glue_macro!();)
    });

    // Rebuild when the manifest changes: its grants select the imports.
    let manifest_path = manifest_directory.join("manifest.json");
    let track_manifest = if manifest_path.is_file() {
        let text = manifest_path.to_string_lossy().into_owned();
        quote!(
            const _: &str = ::core::include_str!(#text);
        )
    } else {
        TokenStream::new()
    };

    Ok(quote! {
        use super::#name as __IliumPlugin;

        #track_manifest

        /// Implements the `Guest` trait of every exported interface.
        struct __IliumGlue;

        /// The backend of `host()`: forwards to the component's imports.
        struct __IliumHost;

        // wit-bindgen output: generated code, not held to the workspace lints.
        #[allow(clippy::all, dead_code, unused, missing_docs)]
        mod bindings {
            #krate::wasm::wit_bindgen::generate!({
                inline: #world,
                path: [#(#paths),*],
                with: { #(#with_keys: #with_paths),* },
                runtime_path: #runtime_path,
            });
        }

        ::std::thread_local! {
            // A component instance is single-threaded and holds exactly one plugin value.
            static __ILIUM_INSTANCE: ::core::cell::RefCell<::core::option::Option<__IliumPlugin>> =
                const { ::core::cell::RefCell::new(::core::option::Option::None) };
        }

        fn __ilium_with<R>(call: impl ::core::ops::FnOnce(&mut __IliumPlugin) -> R) -> R {
            __ILIUM_INSTANCE.with(|slot| {
                let mut slot = slot.borrow_mut();
                call(slot.get_or_insert_with(<__IliumPlugin as ::core::default::Default>::default))
            })
        }

        fn __ilium_install_host() {
            let backend: ::std::boxed::Box<dyn #krate::HostApi> = ::std::boxed::Box::new(__IliumHost);
            #krate::set_host(::std::boxed::Box::new(backend));
        }

        #(#host_macros)*

        #(#export_macros)*

        bindings::export!(__IliumGlue with_types_in bindings);
    })
}

/// The export interfaces of the component, required ones first, each once.
fn selected_exports(side: &str, exports: &[String]) -> Result<Vec<&'static ExportGlue>, String> {
    let mut names: Vec<&str> = REQUIRED_EXPORTS.to_vec();
    for export in exports {
        if !names.contains(&export.as_str()) {
            names.push(export);
        }
    }
    let mut selected = Vec::new();
    for name in names {
        if EXPORTS_WITHOUT_GLUE.contains(&name) {
            return Err(format!(
                "ilium_plugin: the `{name}` export has no component glue yet"
            ));
        }
        let Some(export) = EXPORTS.iter().find(|export| export.name == name) else {
            return Err(format!("ilium_plugin: unknown export `{name}`"));
        };
        if !export.sides.contains(&side) {
            return Err(format!(
                "ilium_plugin: the `{name}` export does not exist in the {side} world"
            ));
        }
        selected.push(export);
    }
    Ok(selected)
}

/// `wit/` of the workspace: the nearest ancestor of the plugin crate that has `wit/types`.
fn wit_root(manifest_directory: &Path) -> Result<PathBuf, String> {
    manifest_directory
        .ancestors()
        .map(|ancestor| ancestor.join("wit"))
        .find(|candidate| candidate.join("types").is_dir())
        .ok_or_else(|| {
            format!(
                "ilium_plugin: no wit/types directory above {}",
                manifest_directory.display()
            )
        })
}

/// The grants `manifest.json` requests for the side; none when the crate has no manifest.
fn side_grants(manifest_directory: &Path, side: &str) -> Result<Vec<String>, String> {
    let path = manifest_directory.join("manifest.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("ilium_plugin: cannot read {}: {error}", path.display()))?;
    grants_of(&text, side).map_err(|error| format!("ilium_plugin: {}: {error}", path.display()))
}

fn grants_of(manifest: &str, side: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(manifest).map_err(|error| format!("not JSON: {error}"))?;
    let Some(list) = value.get("capabilities").and_then(|all| all.get(side)) else {
        return Ok(Vec::new());
    };
    let Some(entries) = list.as_array() else {
        return Err(format!("capabilities.{side} is not an array"));
    };
    entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("capabilities.{side} holds a non-string entry"))
        })
        .collect()
}

/// The host links `ilium:host/<interface>` when a grant is `<interface>` or `<interface>.<rest>`
/// (same rule as the host's component validation).
fn granted(grants: &[String], interface: &str) -> bool {
    grants.iter().any(|grant| {
        grant == interface
            || grant
                .strip_prefix(interface)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::{granted, grants_of, manifest_declares_feature, selected_exports};

    #[test]
    fn a_feature_is_declared_by_its_own_line_only() {
        let manifest = "[features]\ncomponent-server = []\n  component-client=[]\n";

        assert!(manifest_declares_feature(manifest, "component-server"));
        assert!(manifest_declares_feature(manifest, "component-client"));
        assert!(!manifest_declares_feature(manifest, "component-scene"));
        assert!(!manifest_declares_feature(
            "# component-server = []",
            "component-server"
        ));
    }

    #[test]
    fn grants_are_read_for_the_requested_side_only() {
        let manifest =
            r#"{"capabilities": {"server": ["fs.read:project", "jobs.run"], "client": ["keys"]}}"#;

        assert_eq!(
            grants_of(manifest, "server").unwrap(),
            vec!["fs.read:project", "jobs.run"]
        );
        assert_eq!(grants_of(manifest, "client").unwrap(), vec!["keys"]);
        assert!(grants_of(r#"{"capabilities": {}}"#, "server")
            .unwrap()
            .is_empty());
        assert!(grants_of("{}", "server").unwrap().is_empty());
    }

    #[test]
    fn malformed_capabilities_are_reported() {
        assert!(grants_of("not json", "server").is_err());
        assert!(grants_of(r#"{"capabilities": {"server": "fs"}}"#, "server").is_err());
        assert!(grants_of(r#"{"capabilities": {"server": [3]}}"#, "server").is_err());
    }

    #[test]
    fn a_grant_permits_its_interface_and_no_lookalike() {
        let grants = vec!["fs.read:project".to_owned(), "http".to_owned()];

        assert!(granted(&grants, "fs"));
        assert!(granted(&grants, "http"));
        assert!(!granted(&grants, "http-api"));
        assert!(!granted(&grants, "f"));
        assert!(!granted(&grants, "jobs"));
    }

    #[test]
    fn lifecycle_and_handler_are_always_exported_once_and_first() {
        let selected =
            selected_exports("server", &["handler".to_owned(), "job-runner".to_owned()]).unwrap();
        let names: Vec<&str> = selected.iter().map(|export| export.name).collect();

        assert_eq!(names, vec!["lifecycle", "handler", "job-runner"]);
    }

    #[test]
    fn unknown_wrong_side_and_glueless_exports_are_rejected() {
        assert!(selected_exports("server", &["no-such".to_owned()])
            .unwrap_err()
            .contains("unknown export"));
        assert!(selected_exports("server", &["views".to_owned()])
            .unwrap_err()
            .contains("does not exist in the server world"));
        assert!(selected_exports("client", &["renderer".to_owned()])
            .unwrap_err()
            .contains("no component glue"));
    }
}
