//! Lane `imports-core`, part 1: the settings schema rules (SET-001..SET-064) and the host
//! imports clock, codec, fs, http, jobs, journal, log, panes, process and the first settings
//! calls, plus the job-runner and settings-validate exports. Run with `--features testing`.
//!
//! One module per manual page; `support` holds the shared helpers. Part 2 of the lane adds the
//! modules for settings.set-info onward, storage, timers and tree.

mod api_clock_and_timers;
mod api_codec;
mod api_durable_timers;
mod api_exports;
mod api_fs;
mod api_fs_atomic_and_tail;
mod api_http;
mod api_http_streams;
mod api_jobs;
mod api_log;
mod api_panes;
mod api_process_control;
mod api_settings;
mod api_settings_p2;
mod api_storage;
mod api_timers;
mod api_tree;
mod capability_agent_config_propose;
mod reference_requirements_settings;
mod reference_settings_schema;
mod support;
