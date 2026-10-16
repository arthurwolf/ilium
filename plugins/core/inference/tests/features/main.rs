//! Feature tests of the core plugin `ilium.core.inference` (manual page core-plugin-inference.md).
//!
//! Features F-084 (AI providers) and F-085 (AI admission gate and privacy, the plugin's share: the
//! Inference and LLM Instructions tabs), the page samples, the manifest, the extension points, the
//! service and the topic, plus the renders of the page.

mod common;
mod catalog;
mod client_panel;
mod doc_page;
mod hooks_topic;
mod manifest;
mod provider;
mod service;
mod streaming;
mod test_provider;
