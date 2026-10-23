//! Lane `lifecycle-ops`, part 1: install, update, rollback and uninstall operations, persistence,
//! scope and sessions, offline and low disk, managing plugins, dev reload and the remaining LIF
//! rows (LIF-065..094, 150..166, 170..176, 190..193, 220..229, 270..285).
//!
//! One module per manual page. `common` holds the shared helpers; later parts extend it. The
//! tests drive the `ilium_plugin_host::testing::ops` rig named in API_GAPS.md. Run with
//! `--features testing`.

mod api_extension_points;
mod common;
mod core_plugins;
mod extending_core_plugins;
mod lif_activation_and_restart_storms;
mod lif_crash_loop_guard;
mod lif_deprecation_and_settings_reset;
mod lif_documentation_versions;
mod lif_kill_policy_and_formats;
mod lif_operator_upgrade_and_alerts;
mod lif_trial_rollback_and_retention;
mod lifecycle;
mod plugin_data_export;
mod reference_install_update_uninstall;
mod reference_manifest;
mod reference_offline_and_low_disk;
mod reference_persistence_and_restart;
mod reference_plugin_lifecycle_states;
mod reference_plugin_scope_and_sessions;
mod security;
mod services;
mod settings_screens;
mod user_managing_plugins;
