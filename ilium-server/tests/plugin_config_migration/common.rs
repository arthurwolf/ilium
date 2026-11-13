//! Shared helpers of the config-migration lane for the server crate. The rig is an API gap
//! (see API_GAPS.md): `ilium_plugin_host::testing::config`.

pub use ilium_plugin_host::testing::config::{ConfigRig, PluginSpec};

pub const KIB: usize = 1024;

/// A plugin of a tier that asks for the listed grants.
pub fn plugin(id: &str, tier: &str, grants: &[&str]) -> PluginSpec {
    PluginSpec::new(id, tier).grants(grants)
}

/// A section body of the given size.
pub fn body(size: usize, fill: u8) -> Vec<u8> {
    vec![fill; size]
}
