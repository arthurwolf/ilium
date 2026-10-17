//! Ports of the tests of the notification and sound code that moved into the core plugin
//! `ilium.core.notifications` (class adapt-to-plugin-api of tools/test_port_map.tsv). Each twin asserts
//! the observable behaviour of the original through the plugin API on `FakeHost`.
#![allow(dead_code)]

mod attention_tooltips;
mod common;
mod finished;
mod sound_preview;
mod sounds;
mod tasks;
mod texts;
mod svc_server;
mod svc_sounds;
