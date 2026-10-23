//! Lane `gap-security`: requirement tests for the security rows of the plugin security policy
//! (`reference-requirements-security-policy`). Covered IDs: SEC-700, SEC-702, SEC-703, SEC-704,
//! SEC-705, SEC-706, SEC-707, SEC-708, SEC-709, SEC-710, SEC-711, SEC-712, SEC-714, SEC-715,
//! SEC-716, SEC-717, SEC-719, SEC-720, SEC-721, SEC-722, SEC-723, SEC-725, SEC-726, SEC-727,
//! SEC-728, SEC-729, SEC-730, SEC-732, SEC-733.
//!
//! One module per manual page. `support` holds the shared rig shortcuts and the manual page
//! reader. The behavioural tests drive the `ilium_plugin_host::testing::ops` rig named in
//! API_GAPS.md; the documentation tests read the manual pages under `docs/plugins-manual/pages`.
//! Run with `--features testing`.

mod api_plugin_to_plugin;
mod reference_audit_log;
mod reference_grant_review_and_consent;
mod reference_registry;
mod reference_release_key_compromise;
mod reference_sandbox;
mod reference_security_response;
mod reference_security_testing;
mod reference_signing_and_trust;
mod reference_threat_model;
mod reference_trust_and_supply_chain;
mod support;
