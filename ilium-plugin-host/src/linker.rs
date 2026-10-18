//! Linker construction per world and grant set (reference-host-runtime.md, ABI-023).

use ilium_plugin_api::grants::Grant;

use crate::error::HostError;

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
pub fn build_linker(_world: World, _grants: &[Grant]) -> Result<PluginLinker, HostError> {
    unimplemented!("host::linker::build_linker")
}

/// The cache key of a linker: side, tier and the sorted grants (reference-host-runtime.md HST-06x).
pub fn linker_cache_key(
    _side: ilium_plugin_api::Side,
    _tier: ilium_plugin_api::Tier,
    _grants: &[Grant],
) -> String {
    unimplemented!("host::linker::linker_cache_key")
}
