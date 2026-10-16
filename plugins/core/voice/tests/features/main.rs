//! Feature tests of the core plugin `ilium.core.voice` (manual page core-plugin-voice.md).
//!
//! Features F-050 (voice session), F-051 (voice confirmations), F-052 (`ilium voice say`), F-053 (voice
//! onboarding demo) and F-054 (voice device selection), the manifest, the kernel tab `voice-control`, the
//! extension points, the service and the topic, and the renders of the page (strip and confirmation).

#[path = "../host_ext.rs"]
mod host_ext;
mod cli_say;
mod common;
mod confirmations;
mod devices;
mod doc_page;
mod extension_points;
mod manifest;
mod onboarding;
mod render;
mod session_keys;
mod settings;
