//! Tests of the sandbox helper lane (reference-host-helper.md, reference-sandbox.md).
//!
//! Layout: `support` holds the wire codec, the helper process driver, fixture and manual
//! readers; `probe_child` is the re-exec entry used to observe the OS sandbox from inside;
//! one module per manual page (or part of a page).

#[cfg(unix)]
mod probe_child;
#[cfg(unix)]
mod reference_host_helper;
#[cfg(unix)]
mod reference_sandbox_confinement;
#[cfg(unix)]
mod reference_sandbox_files;
#[cfg(unix)]
mod reference_sandbox_hatches;
#[cfg(unix)]
mod reference_sandbox_trust;
#[cfg(unix)]
mod support;
