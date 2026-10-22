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
mod reference_licence_notice;
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

// Part 3: release-security requirements SEC-439 .. SEC-459 (one module per requirement area).
mod crash_reports_and_audit_export;
mod emergency_disable;
mod per_user_isolation;
mod sandbox_prerequisites;
mod secret_and_token_rotation;
mod security_response_disclosure;
mod signature_algorithm_allow_list;
