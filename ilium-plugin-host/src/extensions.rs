//! Extension points, contributions, hook chains and core-plugin replacement
//! (reference-extension-points.md).

use ilium_plugin_api::PluginError;

/// How a hook chain combines its handlers (reference-extension-points.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChainMode {
    /// Any handler may refuse.
    Veto,
    /// Each handler transforms the payload in turn.
    Transform,
    /// Every answer is collected.
    Collect,
}

/// One registered contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    /// The contributing plugin.
    pub plugin: String,
    /// The extension point.
    pub point: String,
    /// The contribution as JSON.
    pub payload_json: String,
}

/// The registry of extension points and contributions.
#[derive(Debug)]
pub struct ExtensionRegistry {
    contributions: Vec<Contribution>,
}

impl ExtensionRegistry {
    /// Adds a contribution.
    pub fn contribute(&mut self, contribution: Contribution) -> Result<(), PluginError> {
        unimplemented!("host::extensions::ExtensionRegistry::contribute")
    }

    /// Runs the chain of an extension point over a payload.
    pub fn run_chain(
        &self,
        point: &str,
        mode: ChainMode,
        payload_json: String,
    ) -> Result<Vec<String>, PluginError> {
        unimplemented!("host::extensions::ExtensionRegistry::run_chain")
    }
}
