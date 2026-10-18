//! Plugin twins of the existing agent-detection tests (class adapt-to-plugin-api in tools/test_port_map.tsv).
//!
//! Each twin asserts the observable behaviour of the original test through the plugin API (FakeHost, the
//! `detector` export `identify`/`classify`, decorations, settings). Originals whose subject is the kernel-native
//! detection loop, IPC, persistence or client-core rendering are listed in API_GAPS.md as having no plugin surface.

mod common;
mod detect_activity;
mod detect_chronology_more;
mod detect_goal;
mod detect_identity;
mod detect_interstitial;
mod kernel_ports;
mod live_agent;
mod status_chronology;
mod tree_ui;
