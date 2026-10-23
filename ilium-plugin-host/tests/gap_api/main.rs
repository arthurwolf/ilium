//! Lane `gap-api` (API gap requirements, host half): API-200, API-201, API-203, API-204, API-209,
//! API-213, API-214 and API-216, from the reference-requirements-api-host register and the
//! manual pages errors-and-limits, api-events, api-fs, api-http-streams, api-services,
//! api-settings, api-commands and api-pane-lifecycle. Run with `--features testing`.
//!
//! One module per manual page. The tests drive the `ilium_plugin_host::testing::harness` rig.

mod api_commands;
mod api_events;
mod api_fs;
mod api_http_streams;
mod api_pane_lifecycle;
mod api_services;
mod api_settings;
mod reference_errors_and_limits;
