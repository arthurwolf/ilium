//! Port twins of the original progress tests (manual page core-plugin-progress.md), lane
//! core-automation, part ports. Each twin asserts the observable behaviour of the original through
//! the plugin API; helpers shared with the features tests are read from ../features/common.rs.

#[path = "../features/common.rs"]
mod common;
mod support;

mod app;
mod gap_ports;
mod handlers;
mod lib_rs;
mod progress_bar;
mod progress_display;
mod progress_guard;
mod progress_monitor;
mod progress_wait;
mod progress_watchdog;
mod restart_recovery;
