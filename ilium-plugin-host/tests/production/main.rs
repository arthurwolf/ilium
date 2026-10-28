//! Lane `host-production`: requirement tests for the plugin host rows HST-126..135, 389..392,
//! 950..963, 970..981 and 990..993 of the kernel-host register. Each module follows one manual page.
//!
//! Documentation rows read the manual page under `docs/plugins-manual/pages` and assert its
//! concrete values, codes and steps. Behaviour rows call the existing host API where one exists
//! and otherwise name the missing item in the failing assertion; those tests stay red until the
//! feature exists. Gaps are listed in API_GAPS.md.

mod common;
mod guide_backup_and_restore;
mod guide_enterprise_deployment;
mod reference_errors_and_limits;
mod reference_host_aggregate_limits;
mod reference_host_telemetry;
mod reference_minimum_system_requirements;
mod reference_offline_and_low_disk;
mod reference_plugin_files;
mod reference_platform_support;
