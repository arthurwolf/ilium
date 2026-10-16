//! Lane `security`, part 1: threat model, network egress, admin policy, audit log, secrets,
//! privacy and security response (SEC-001 .. SEC-324 of the lane). Run with `--features testing`.
//!
//! One module per manual page; `support` holds the shared rig and helpers. Part 2 adds its
//! modules below the part 1 block. Every item outside the skeleton is named in API_GAPS.md.

mod support;

// Part 1.
mod guide_enterprise_deployment;
mod reference_admin_policy;
mod reference_audit_log;
mod reference_network_egress;
mod reference_secrets;
mod reference_security_response;
mod reference_threat_model;
mod security;
mod user_plugin_security_and_privacy;

// Part 2.
mod reference_extension_security;
mod reference_security_response_p2;
mod reference_security_testing;
mod reference_threat_model_p2;
