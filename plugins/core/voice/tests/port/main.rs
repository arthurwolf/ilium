//! Port tests of the core plugin `ilium.core.voice`: twins of the voice tests that lived in the
//! monolith (ilium, ilium-ipc, ilium-server, ilium-client), written against the plugin API only.

#![allow(dead_code)]

#[path = "../host_ext.rs"]
mod host_ext;
#[path = "../features/common.rs"]
mod common;

mod cli_args;
mod client_start;
mod control;
mod onboarding_ports;
mod relay;
mod settings_ports;
