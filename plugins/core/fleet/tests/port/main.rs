//! Port twins of the original ilium tests that the fleet plugin takes over (the `PORT:` family).
//! Each module mirrors one original file. Originals that test kernel or client internals with no
//! plugin-visible channel are not twinned; the coverage tool lists them as skipped.

#[path = "../features/common.rs"]
mod common;

mod agent_message_dialog;
mod app;
mod broadcast;
mod gaps;
mod http_api;
mod initial_prompt;
mod integration;
mod pane_filter;
mod scoped_ask;
