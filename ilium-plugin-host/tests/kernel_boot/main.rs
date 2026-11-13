//! Kernel-boot lane, host crate part: crate layout, ownership, platform support, limits and
//! rendering rows. One module per manual page; later parts extend `common`.

mod common;
mod errors_and_limits;
mod host_architecture;
mod host_rendering;
mod kernel_and_boot;
mod platform_support;
