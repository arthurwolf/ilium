//! Kernel feature parity (port-compat lane, COR-001 and COR-011, the kernel-owned F ids).
//! Every baseline scenario runs twice: with plugins (`StartMode::Normal`) and kernel only
//! (`StartMode::NoPlugins`); both runs must give the baseline result of the user documentation.
//! One module per manual page or user-documentation page covered.

mod automation;
mod common;
#[path = "../support/plugin_model.rs"]
mod plugin_model;
mod editors_and_boards;
mod fleet_kernel_ports;
mod how_it_works;
mod inference_and_privacy;
mod panes_and_layout;
mod port_handlers;
mod reference_feature_traceability;
mod req_gap_port_compat;
mod session_recovery;
mod settings;
