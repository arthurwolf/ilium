//! Lane `grants` (api half): grant parsing, the grant table and its tier x side x risk matrix,
//! install-time grant refusals (CAP-001..018, SEC-124). Part 1 wrote these modules; later parts
//! of the lane add modules here.
//!
//! Tests tagged `doc_` read the manual. Every other test calls an `unimplemented!` stub of
//! `ilium_plugin_api::grants` or `ilium_plugin_api::manifest` and fails until the implementation lands.

mod reference_capabilities;
mod reference_grant_review_and_consent;
mod support;
