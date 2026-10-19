//! Port tests of the core plugin `ilium.core.diagnostics`: twins of the monolith tests that touch the
//! debug switch and the terminal engine budget, written against the plugin API only.

#[path = "../support/session.rs"]
mod session_support;

mod common;

mod debug_log_keys;
mod debug_switch;
mod engine_budget;
