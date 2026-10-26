//! Tests of the sandbox helper lane (reference-host-helper.md, reference-sandbox.md).
//!
//! Layout: `support` holds the wire codec, the helper process driver, fixture and manual
//! readers; `probe_child` is the re-exec entry used to observe the OS sandbox from inside;
//! one module per manual page (or part of a page).

// SEC-057 (reference-sandbox.md) supports this OS sandbox only on Linux.
// Keep the complete behavioural lane enabled there; other platforms have no helper sandbox.
#![cfg(target_os = "linux")]

mod probe_child;
mod reference_host_helper;
mod reference_sandbox_confinement;
mod reference_sandbox_files;
mod reference_sandbox_hatches;
mod reference_sandbox_trust;
mod support;
