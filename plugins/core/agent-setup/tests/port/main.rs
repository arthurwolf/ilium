//! Port twins of the original ilium-client tests whose behaviour moved into the core plugin
//! `ilium.core.agent-setup` (class adapt-to-plugin-api in tools/test_port_map.tsv). Each twin asserts the
//! same observable behaviour as the original through FakeHost, the install service, the config-writer
//! facade and the Setup tab. Items the skeleton lacks are listed in tests/API_GAPS_port.md.

mod app;
mod common;
mod config_fs;
mod config_text;
mod feature_setup;
#[path = "../host_ext.rs"]
mod host_ext;
mod host_ext_port;
mod path_paste;
mod setup_prompt;
