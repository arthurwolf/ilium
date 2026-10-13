//! Service registry, versions, `call_service`, topics and peer messages (reference-services.md).

use ilium_plugin_api::PluginError;

/// A provided service (reference-services.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEntry {
    /// Service name.
    pub name: String,
    /// Service version.
    pub version: String,
    /// The providing plugin.
    pub provider: String,
}

/// The registry of services.
#[derive(Debug)]
pub struct ServiceRegistry {
    entries: Vec<ServiceEntry>,
}

impl ServiceRegistry {
    /// Finds a provider satisfying a version requirement.
    pub fn find(&self, name: &str, requirement: &str) -> Option<&ServiceEntry> {
        unimplemented!("host::services::ServiceRegistry::find")
    }

    /// Calls a service; the reply is at most 1 MiB.
    pub fn call(&self, caller: &str, name: &str, request: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::services::ServiceRegistry::call")
    }
}
