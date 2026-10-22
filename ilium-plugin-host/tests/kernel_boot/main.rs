//! Kernel-boot lane, host crate part: crate layout, ownership, platform support, limits and
//! rendering rows. One module per manual page; later parts extend `common`.

mod common;
mod correlation_chain;
mod engine_recycle_and_cache;
mod errors_and_limits;
mod helper_crash_forensics;
mod host_architecture;
mod host_capacity_ceilings;
mod host_rendering;
mod ipc_traffic_visibility;
mod kernel_and_boot;
mod kill_switch_plugin;
mod platform_support;
mod telemetry_daily_history;
