//! Admin policy, enterprise rules, network egress allow-list, secrets access and privacy toggles
//! (reference-security.md).

use ilium_plugin_api::grants::Grant;

/// The verdict of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// Allowed.
    Allow,
    /// Denied, with the `policy.*` code.
    Deny(String),
}

/// The policy in force.
#[derive(Debug)]
pub struct PolicyEngine;

impl PolicyEngine {
    /// Checks whether a grant may be given to a plugin.
    pub fn check_grant(&self, _plugin: &str, _grant: &Grant) -> PolicyVerdict {
        unimplemented!("host::policy::PolicyEngine::check_grant")
    }

    /// Checks a network destination against the egress allow-list.
    pub fn check_egress(&self, _plugin: &str, _host: &str) -> PolicyVerdict {
        unimplemented!("host::policy::PolicyEngine::check_egress")
    }
}
