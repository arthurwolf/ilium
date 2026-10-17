//! Ports of the existing worktree tests that adapt to the plugin API (FakeHost twins).
//! Shared helpers come from the features suite (read only, included by path).
mod app;
#[path = "../features/common.rs"]
mod common;
mod git_status;
mod git_text_setting;
mod support;
mod ui_twins;
mod worktree_cli;
mod worktree_dialog;
mod worktree_manager;
