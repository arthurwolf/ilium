//! Twins of the existing text-trigger tests of the monolith (stage-0 port map, class adapt-to-plugin-api).
//!
//! Every original test is re-expressed on the plugin channels of the manual page core-plugin-text-triggers.md:
//! output filters installed per rule, `output-matched` answered by one `send-input` with the dedup key
//! `trigger-{filter}-{node}-{row}`, the 10 minute host dedup window, the rule-list settings of the kernel tab,
//! the `test-rule` service and the rendered tab. Each module groups the twins of one original file.
#![allow(dead_code)]

#[path = "../features/common.rs"]
mod common;
mod support;

mod client_dialog;
mod client_misc;
mod client_preview;
mod client_save_failures;
mod client_trigger_settings;
mod ipc_text_trigger;
mod server_config;
mod server_connection;
mod server_integration;
mod server_matching;
mod server_text_triggers_instances;
