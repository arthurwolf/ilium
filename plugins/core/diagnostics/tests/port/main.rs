//! Port tests of the core plugin `ilium.core.diagnostics`: twins of the monolith tests that touch the
//! debug switch and the terminal engine budget, written against the plugin API only.

#![allow(dead_code)]

#[path = "../features/common.rs"]
mod common;

mod debug_switch;
mod engine_budget;
