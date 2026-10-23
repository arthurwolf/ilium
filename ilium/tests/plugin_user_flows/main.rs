//! User and administrator lane, part 1 (ilium binary side): install and manage, core plugins,
//! troubleshooting, extensions, privacy, administration, files on disk and policy, driven through
//! the `ilium plugin` command functions and a user-profile rig. One module per manual page.
//! Red tests derived from docs/plugins-manual; every test carries a requirement tag line.

mod admin_lockfile_replay;
mod audit_chain;
mod blocked_state_guidance;
mod gap_abi_major_versions;
mod gap_cli_completion;
mod gap_cli_output_stability;
mod gap_host_aggregate_limits;
mod gap_host_release_verification;
mod gap_operator_upgrades;
mod gap_registry_key_compromise;
mod gap_release_key_compromise;
mod gap_release_rollout;
mod gap_sdk_deprecation;
mod guide_admin_failure_runbook;
mod guide_administration;
mod lifecycle_and_security;
mod package_size_limits;
mod production_gaps;
mod reference_core_replacement_matrix;
mod reference_plugin_files;
mod reference_plugin_policy;
mod reference_plugin_policy_p2;
mod support;
mod user_core_plugins;
mod user_data_portability;
mod user_extensions_tab;
mod user_faq;
mod user_install_and_manage;
mod user_permissions_explained;
mod user_plugins_overview;
mod user_privacy;
mod user_troubleshooting;
mod user_trust_policy;
