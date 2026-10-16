//! Ports of the original Smart Copy tests of the monolith (ilium-client) to the plugin API.
//!
//! Each test is the FakeHost twin of one or more original tests (tagged per test with the original file and test name) and asserts
//! the same observable behaviour on the plugin channels: candidate lines, picker keys, clipboard
//! text, status lines, menu entries and pointer claims.

mod common;
mod clipboard_ports;
mod menu_ports;
mod mouse_ports;
mod selection_ports;
mod smart_copy_ports;
mod light_ports;
mod layout_ports;
mod tokens_ports;
mod host_internal_ports;
