//! Twins of the original ilium-client compaction tests (test_port_map.tsv, class adapt-to-plugin-api,
//! target plugins/core/compaction). Each twin asserts the observable behaviour of its original through
//! FakeHost, in native mode, and is tagged `REQ: PORT:<file>::<test>`.

mod common;
mod fixtures;
mod host_ext;
#[path = "../support/host_ext_shared.rs"]
mod host_ext_shared;

mod analysis_golden;
mod analysis_parse_fixtures;
mod analysis_pick_rules;
mod analysis_pipeline;
mod analysis_rework;
mod analysis_stats_fixture;
mod app_flow;
mod misc_client;
mod remote_app;
mod remote_common;
mod remote_dialog;
mod remote_flow;
mod remote_live_resume;
mod remote_native;
mod remote_pipeline;
mod remote_settings;
mod remote_settings_ui;
mod remote_worker;
mod remote_writers;
mod report_engine;
mod scan_engine;
mod ui_pages;
