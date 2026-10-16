//! Feature tests of the core plugin `ilium.core.compaction` (manual page core-plugin-compaction.md).
//!
//! Features F-080 (remote compaction), F-082 (compaction optimiser), F-083 (auto-compact trigger reader) and
//! F-115 (Optimization tab), the page samples, the manifest, the kernel tabs, the extension points, the
//! service, the topic and the renders.

#[path = "../host_ext.rs"]
mod host_ext;
mod common;
mod doc_page;
mod extension_points;
mod kernel_tabs;
mod manifest;
mod optimizer;
mod remote_job;
mod renders;
mod settings;
mod techniques;
mod trigger_reader;
