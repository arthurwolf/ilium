//! Port tests of the core plugin `ilium.core.voice`: twins of the voice tests that lived in the
//! monolith (ilium, ilium-ipc, ilium-server, ilium-client), written against the plugin API only.

#[path = "../features/common.rs"]
mod common;
#[path = "../host_ext.rs"]
mod host_ext;

mod cli_args;
mod client_start;
mod control;
mod control_edges;
mod control_settings;
mod device_startup;
mod onboarding_ports;
mod prep;
mod relay;
mod settings_ports;
mod studio;
mod submit;
mod ui_ports;
mod voice_state;
