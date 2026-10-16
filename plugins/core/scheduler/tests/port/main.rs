//! FakeHost/FakeSession twins of the baseline tests that move with the scheduler feature
//! (F-055 scheduled input, F-057 prompt queue). Each twin asserts the observable behaviour of
//! its original on the manual's channels. Tags: `PORT:<file>::<test>`.
#![allow(dead_code)]

#[path = "../features/common.rs"]
mod common;
mod support;

mod client_app;
mod client_mouse;
mod client_prompt_queue;
mod client_scheduled_input;
mod client_tree_ui;
mod client_ui;
mod pty_tui_smoke;
mod server_ordered_owner_regressions;
mod server_scheduled_input;
mod server_smoke;
