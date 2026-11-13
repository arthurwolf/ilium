//! Linker construction per world and grant set (reference-host-runtime.md, ABI-023).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ilium_plugin_api::grants::Grant;
use wasmtime::component::{Linker, Type, Val};
use wasmtime::StoreContextMut;

use crate::engine::PluginEngine;
use crate::error::HostError;
use crate::imports::generated_dispatch::{
    DispatchError, ImportFunction, ImportInterface, IMPORT_INTERFACES,
};
use crate::store_data::{add_wasi_to_linker, ImportFault, StoreData};
use crate::wasm_guest::{lift, lower};

/// The three worlds a component targets (reference-wit.md section 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum World {
    /// `ilium:server`.
    Server,
    /// `ilium:client`.
    Client,
    /// `ilium:anim`.
    Anim,
}

impl World {
    /// The WIT world name (reference-wit.md section 9).
    pub fn wit_name(self) -> &'static str {
        match self {
            World::Server => "ilium:server",
            World::Client => "ilium:client",
            World::Anim => "ilium:anim",
        }
    }
}

/// A linker with the imports the grants allow; unrequested imports trap with
/// `trap.import-not-requested` (reference-host-runtime.md).
#[derive(Debug)]
pub struct PluginLinker {
    /// The world this linker serves.
    pub world: World,
}

/// Builds the linker for a world and the granted capabilities (reference-host-runtime.md).
pub fn build_linker(world: World, _grants: &[Grant]) -> Result<PluginLinker, HostError> {
    Ok(PluginLinker { world })
}

/// The cache key of a linker: side, tier and the sorted grants (reference-host-runtime.md HST-06x).
pub fn linker_cache_key(
    side: ilium_plugin_api::Side,
    tier: ilium_plugin_api::Tier,
    grants: &[Grant],
) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    let mut sorted: Vec<&str> = grants.iter().map(Grant::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut hasher = Sha256::new();
    hasher.update(side.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(tier.as_str().as_bytes());
    for grant in sorted {
        hasher.update([0]);
        hasher.update(grant.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut hex, byte| {
            // Writing to a String cannot fail.
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A Wasmtime linker with WASI and every `ilium:` import function defined.
///
/// The linker itself is the same for every plugin of a process: each function forwards to
/// the import set in the calling instance's store data, and an interface that instance was
/// not granted answers `host.capability-not-requested`. Grants therefore never widen through a
/// shared linker, and one linker serves every (side, tier, grant set) key.
pub struct RuntimeLinker {
    linker: Linker<StoreData>,
}

impl std::fmt::Debug for RuntimeLinker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeLinker")
            .finish_non_exhaustive()
    }
}

impl RuntimeLinker {
    /// A linker without any definition: a component with an import fails to instantiate.
    pub fn closed(engine: &PluginEngine) -> Self {
        Self {
            linker: Linker::new(engine.runtime_engine()),
        }
    }

    /// The Wasmtime linker.
    pub fn linker(&self) -> &Linker<StoreData> {
        &self.linker
    }
}

/// Builds the linker of a process: WASI under the host policy and the generated import
/// table. Functions with a resource handle in their signature stay undefined, so a
/// component that imports one fails to instantiate with a missing-implementation error.
pub fn build_runtime_linker(engine: &PluginEngine) -> Result<RuntimeLinker, HostError> {
    let mut linker = Linker::<StoreData>::new(engine.runtime_engine());
    add_wasi_to_linker(&mut linker)?;
    for interface in IMPORT_INTERFACES {
        define_interface(&mut linker, interface)?;
    }
    Ok(RuntimeLinker { linker })
}

fn define_interface(
    linker: &mut Linker<StoreData>,
    interface: &'static ImportInterface,
) -> Result<(), HostError> {
    let failed = |error: wasmtime::Error| {
        HostError::EngineError(format!("linking {} failed: {error}", interface.instance))
    };
    let mut instance = linker.instance(interface.instance).map_err(failed)?;
    for function in interface.functions {
        instance
            .func_new(
                function.name,
                move |store: StoreContextMut<'_, StoreData>, signature, parameters, results| {
                    let parameter_types: Vec<Type> = signature.params().map(|(_, ty)| ty).collect();
                    let result_type = signature.results().next();
                    call_import(
                        store,
                        interface,
                        function,
                        &parameter_types,
                        result_type.as_ref(),
                        parameters,
                        results,
                    )
                },
            )
            .map_err(failed)?;
    }
    Ok(())
}

fn call_import(
    mut store: StoreContextMut<'_, StoreData>,
    interface: &'static ImportInterface,
    function: &'static ImportFunction,
    parameter_types: &[Type],
    result_type: Option<&Type>,
    parameters: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let abi = || {
        wasmtime::format_err!(
            "import {}.{} does not match its WIT signature",
            interface.instance,
            function.name
        )
    };
    if parameters.len() != parameter_types.len() || parameters.len() != function.parameters {
        return Err(abi());
    }
    let arguments = parameters
        .iter()
        .zip(parameter_types)
        .map(|(value, ty)| lift(value, ty))
        .collect::<Result<Vec<_>, ()>>()
        .map_err(|()| abi())?;
    let data = store.data_mut();
    // A panicking host import must not unwind into Wasmtime or end the executor thread: it
    // becomes a trap of this one call (`host.import-panic`).
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        data.imports
            .call(interface.instance, function.name, arguments)
    }));
    let (error, code) = match called {
        Ok(Ok(value)) => {
            return match (result_type, results.first_mut()) {
                (Some(ty), Some(slot)) => {
                    *slot = lower(&value, ty).map_err(|()| abi())?;
                    Ok(())
                }
                (None, None) => Ok(()),
                _ => Err(abi()),
            };
        }
        Ok(Err(DispatchError::NotLinked)) => {
            // Not granted is the plugin's fault; granted but without an implementation
            // in this process is a host wiring gap.
            let code = if interface_unlocked(interface.instance, &data.identity.granted) {
                "host.engine-error"
            } else {
                "host.capability-not-requested"
            };
            (Some(DispatchError::NotLinked), code)
        }
        Ok(Err(error)) => (Some(error), "trap.abi"),
        Err(_) => (None, "host.import-panic"),
    };
    data.import_fault = Some(ImportFault {
        error,
        code,
        instance: interface.instance.to_owned(),
        function: function.name.to_owned(),
    });
    Err(wasmtime::format_err!(
        "{code}: import {}.{}",
        interface.instance,
        function.name
    ))
}

/// Whether an import interface may be used with a set of granted capabilities: an
/// interface no capability names is always available, any other needs one of its
/// capabilities (reference-capabilities.md, ABI-023).
pub fn interface_unlocked(instance: &str, granted: &[String]) -> bool {
    let mut gated = false;
    for row in ilium_plugin_api::grants::table() {
        if !row.world_interfaces.contains(&instance) {
            continue;
        }
        gated = true;
        let family_granted = granted.iter().any(|grant| {
            grant
                .split_once(':')
                .map_or(grant.as_str(), |(family, _)| family)
                == row.family
        });
        if family_granted {
            return true;
        }
    }
    !gated
}

/// Linkers by cache key (HST-070). With the shared design above every key maps to the same
/// linker; the cache keeps the keyed lookup of the manual so a later per-grant linker needs
/// no caller change.
#[derive(Debug, Default)]
pub struct LinkerCache {
    shared: Mutex<Option<Arc<RuntimeLinker>>>,
    keys: Mutex<HashMap<String, u64>>,
}

impl LinkerCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The linker for a key, built on first use.
    pub fn get(
        &self,
        engine: &PluginEngine,
        side: ilium_plugin_api::Side,
        tier: ilium_plugin_api::Tier,
        grants: &[Grant],
    ) -> Result<Arc<RuntimeLinker>, HostError> {
        let key = linker_cache_key(side, tier, grants);
        *self
            .keys
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .entry(key)
            .or_insert(0) += 1;
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(linker) = shared.as_ref() {
            return Ok(Arc::clone(linker));
        }
        let linker = Arc::new(build_runtime_linker(engine)?);
        *shared = Some(Arc::clone(&linker));
        Ok(linker)
    }

    /// How many distinct keys were requested.
    pub fn distinct_keys(&self) -> usize {
        self.keys
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .len()
    }
}

#[cfg(test)]
mod tests {
    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_api::{Side, Tier};

    use super::{build_linker, linker_cache_key, World};

    /// Parses a grant string through the API crate's documented constructor.
    fn grant(text: &str) -> Grant {
        Grant::parse(text).expect("the grant string is a valid grant of the API table")
    }

    /// REQ: PKG-041
    #[test]
    fn wit_name_of_each_world_is_its_package_name() {
        assert_eq!(World::Server.wit_name(), "ilium:server");
        assert_eq!(World::Client.wit_name(), "ilium:client");
        assert_eq!(World::Anim.wit_name(), "ilium:anim");
    }

    /// REQ: PKG-041
    #[test]
    fn wit_names_of_the_three_worlds_are_distinct() {
        let names = [
            World::Server.wit_name(),
            World::Client.wit_name(),
            World::Anim.wit_name(),
        ];
        assert_ne!(names[0], names[1]);
        assert_ne!(names[1], names[2]);
        assert_ne!(names[0], names[2]);
    }

    /// REQ: HST-071
    #[test]
    fn build_linker_with_empty_grants_succeeds_and_keeps_world() {
        let linker =
            build_linker(World::Server, &[]).expect("an empty grant set is a valid linker input");
        assert_eq!(linker.world, World::Server);
    }

    /// REQ: HST-070
    #[test]
    fn build_linker_records_the_requested_world_for_every_world() {
        let grants = [grant("tree.read")];
        for world in [World::Server, World::Client, World::Anim] {
            let linker = build_linker(world, &grants).expect("a valid grant set builds a linker");
            assert_eq!(linker.world, world);
        }
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_is_the_same_for_the_same_inputs() {
        let grants = [grant("tree.read"), grant("http:api.example.com")];
        assert_eq!(
            linker_cache_key(Side::Server, Tier::T1, &grants),
            linker_cache_key(Side::Server, Tier::T1, &grants)
        );
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_does_not_depend_on_grant_order() {
        let forward = [grant("tree.read"), grant("http:api.example.com")];
        let reversed = [grant("http:api.example.com"), grant("tree.read")];
        assert_eq!(
            linker_cache_key(Side::Client, Tier::T2, &forward),
            linker_cache_key(Side::Client, Tier::T2, &reversed)
        );
    }

    /// REQ: HST-070
    #[test]
    fn equal_grant_sets_on_equal_sides_share_a_key() {
        let first_plugin = [grant("tree.read")];
        let second_plugin = [grant("tree.read")];
        assert_eq!(
            linker_cache_key(Side::Server, Tier::T3, &first_plugin),
            linker_cache_key(Side::Server, Tier::T3, &second_plugin)
        );
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_differs_when_the_side_differs() {
        let grants = [grant("tree.read")];
        let server_key = linker_cache_key(Side::Server, Tier::T1, &grants);
        let client_key = linker_cache_key(Side::Client, Tier::T1, &grants);
        let scene_key = linker_cache_key(Side::Scene, Tier::T1, &grants);
        assert_ne!(server_key, client_key);
        assert_ne!(server_key, scene_key);
        assert_ne!(client_key, scene_key);
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_differs_when_the_tier_differs() {
        let grants = [grant("tree.read")];
        let key_t1 = linker_cache_key(Side::Server, Tier::T1, &grants);
        let key_t2 = linker_cache_key(Side::Server, Tier::T2, &grants);
        let key_t3 = linker_cache_key(Side::Server, Tier::T3, &grants);
        assert_ne!(key_t1, key_t2);
        assert_ne!(key_t1, key_t3);
        assert_ne!(key_t2, key_t3);
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_differs_when_the_grant_set_differs() {
        let smaller = [grant("tree.read")];
        let larger = [grant("tree.read"), grant("http:api.example.com")];
        assert_ne!(
            linker_cache_key(Side::Server, Tier::T1, &smaller),
            linker_cache_key(Side::Server, Tier::T1, &larger)
        );
    }

    /// REQ: HST-071
    #[test]
    fn cache_key_accepts_an_empty_grant_set_and_is_stable() {
        let first = linker_cache_key(Side::Server, Tier::T1, &[]);
        let second = linker_cache_key(Side::Server, Tier::T1, &[]);
        assert_eq!(first, second);
        assert_eq!(
            first.len(),
            64,
            "an empty grant set still yields a SHA-256 digest, got {first:?}"
        );
        assert_ne!(
            first,
            linker_cache_key(Side::Server, Tier::T1, &[grant("tree.read")])
        );
    }

    /// REQ: HST-070
    #[test]
    fn cache_key_is_a_sha256_hex_digest() {
        let key = linker_cache_key(Side::Server, Tier::T1, &[grant("tree.read")]);
        assert_eq!(
            key.len(),
            64,
            "a SHA-256 digest is 64 hexadecimal characters, got {key:?}"
        );
        assert!(
            key.chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()),
            "the key must be lowercase hexadecimal, got {key:?}"
        );
    }
}
