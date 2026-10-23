//! Lane `host-gap`: requirement tests for the plugin host rows HST-512..699 that the existing
//! lanes do not cover, one module per manual page. Covered IDs (81): HST-514, 600, 601, 602, 604,
//! 605, 607, 608, 609, 610, 612, 613, 614, 615, 617, 618, 622, 623, 625, 626, 627, 628, 629, 630,
//! 631, 633, 634, 636, 638, 639, 640, 641, 642, 643, 644, 646, 647, 648, 651, 652, 653, 654, 655,
//! 656, 657, 659, 660, 661, 662, 663, 664, 665, 666, 667, 668, 669, 670, 671, 672, 673, 674, 675,
//! 677, 679, 680, 681, 682, 683, 686, 687, 688, 689, 690, 691, 692, 693, 694, 695, 696, 697, 699.
//! HST-512, 520 and 521 are manual process checks and are not written here (see the report).
//!
//! The tests drive the `ilium_plugin_host::testing::ops` rig named in API_GAPS.md and read the
//! manual pages under `docs/plugins-manual/pages`. Hooks that the rig does not yet provide are
//! called anyway, so these tests stay red until the feature exists. Run with `--features testing`.

mod api_log;
mod api_storage;
mod common;
mod guide_admin_failure_runbook;
mod guide_enterprise_deployment;
mod reference_debugging_tools;
mod reference_errors_and_limits;
mod reference_health_and_watchdog;
mod reference_host_budgets;
mod reference_host_soak_and_load_testing;
mod reference_host_telemetry;
mod reference_install_update_uninstall;
mod reference_offline_and_low_disk;
mod reference_persistence_and_restart;
mod reference_plugin_files;
mod reference_resource_accounting;
mod user_plugin_health;
