//! Port twins of the existing ilium-client / ilium tests that belong to the agent-toolbar feature set
//! (tools/test_port_map.tsv, class adapt-to-plugin-api). Each twin asserts, through the plugin's
//! FakeHost message channels, the observable values the original test asserted. The channel protocol
//! is not in the manual yet; see API_GAPS_ports.md.

mod agent_freeze;
mod agent_history_path;
mod agent_prompt_transcript;
mod agent_toolbar;
mod antigravity_model_statusline;
mod app;
mod claude_model_statusline;
mod common;
mod completed_agent_action;
mod goal_resume_link;
mod icon_cycle;
mod icon_settings;
mod keys_rs;
mod last_prompt_banner;
mod mouse;
mod pty_tui_smoke;
mod status_icons;
mod transcript_context;
mod tree_ui;
mod value_effort;
mod value_icon;
