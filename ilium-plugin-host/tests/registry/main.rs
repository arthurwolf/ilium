//! Lane `registry-publish`, part 1: the registry protocol and the distribution commands
//! (reference-registry.md PKG-361..422, reference-plugin-distribution-cli.md PKG-423..445).
//! Spec: docs/plugins-manual/pages. Gaps in the skeleton are listed in API_GAPS.md.
//! Later parts of the lane add modules here and reuse `common`.

mod common;
mod guide_plugin_administration;
mod guide_publishing;
mod pages_p2;
mod reference_plugin_distribution_cli;
mod reference_plugin_distribution_cli_p2;
mod reference_registry;
mod reference_registry_dormant_namespace;
mod reference_registry_p2;
mod reference_registry_search;
mod registry_strictness;
mod user_plugin_store;

mod publ_mirror;
mod publ_publish_flow;
mod publ_registry_metadata;
mod publ_registry_ops;
mod publ_yank_takedown;
mod req_gap_registry_publish;
