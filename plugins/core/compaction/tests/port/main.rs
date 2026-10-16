//! Twins of the original ilium-client compaction tests (test_port_map.tsv, class adapt-to-plugin-api,
//! target plugins/core/compaction). Each twin asserts the observable behaviour of its original through
//! FakeHost, in native mode, and is tagged `REQ: PORT:<file>::<test>`.

mod common;
mod fixtures;

mod app_flow;
mod report_engine;
mod scan_engine;
mod ui_pages;
mod remote_app;
mod remote_dialog;
mod remote_flow;
mod remote_native;
mod remote_settings;
mod remote_settings_ui;
mod remote_worker;
mod misc_client;
