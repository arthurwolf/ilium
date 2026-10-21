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

#[cfg(test)]
mod tests {
    use super::{PolicyEngine, PolicyVerdict};
    use ilium_plugin_api::grants::Grant;

    fn grant(text: &str) -> Grant {
        Grant::parse(text).expect("test grant must parse")
    }

    /// REQ: SEC-184
    #[test]
    fn check_grant_without_restricting_policy_allows_normal_grant() {
        let engine = PolicyEngine;
        assert_eq!(engine.check_grant("com.example.plugin", &grant("tree.read")), PolicyVerdict::Allow);
    }

    /// REQ: SEC-184
    #[test]
    fn check_grant_without_restricting_policy_allows_exact_host_grant() {
        let engine = PolicyEngine;
        assert_eq!(
            engine.check_grant("com.example.plugin", &grant("http:api.example.com")),
            PolicyVerdict::Allow
        );
    }

    /// REQ: SEC-184
    #[test]
    fn check_grant_without_restricting_policy_allows_wildcard_host_grant() {
        let engine = PolicyEngine;
        assert_eq!(
            engine.check_grant("com.example.plugin", &grant("http:*.openstreetmap.org")),
            PolicyVerdict::Allow
        );
    }

    /// REQ: SEC-085
    #[test]
    fn check_egress_without_restricting_policy_allows_public_host() {
        let engine = PolicyEngine;
        assert_eq!(engine.check_egress("com.example.plugin", "api.example.com"), PolicyVerdict::Allow);
    }

    /// REQ: SEC-085
    #[test]
    fn check_egress_without_restricting_policy_allows_subdomain_of_public_host() {
        let engine = PolicyEngine;
        assert_eq!(
            engine.check_egress("com.example.plugin", "tiles.openstreetmap.org"),
            PolicyVerdict::Allow
        );
    }

    /// REQ: SEC-184
    #[test]
    fn policy_verdict_deny_carries_policy_code_and_differs_from_allow() {
        let verdict = PolicyVerdict::Deny(String::from("policy.grant-forbidden"));
        assert_ne!(verdict, PolicyVerdict::Allow);
        assert_eq!(verdict, PolicyVerdict::Deny(String::from("policy.grant-forbidden")));
    }

    /// REQ: SEC-184
    #[test]
    fn policy_verdict_deny_codes_differ_by_detail_code() {
        assert_ne!(
            PolicyVerdict::Deny(String::from("policy.id-denied")),
            PolicyVerdict::Deny(String::from("policy.tier-denied"))
        );
    }
}
