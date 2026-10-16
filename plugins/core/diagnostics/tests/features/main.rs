//! Feature tests of the core plugin `ilium.core.diagnostics` (manual page core-plugin-diagnostics.md).
//!
//! The manifest, the page, the report service, the log tail and the warnings list, the log-sinks hook,
//! the report-sections collect point, the attach dialog, the diary, and the agent debug history (F-032)
//! with its ported tests. The agent debug surface is not on the page: see API_GAPS.md.

mod agent_debug_client;
mod agent_debug_export;
mod agent_debug_history;
mod agent_debug_server;
mod attach_dialog;
mod common;
mod diary;
mod doc_page;
mod extension_points;
mod kernel_setting;
mod log_sinks;
mod log_tail;
mod manifest;
mod renders;
mod report;
