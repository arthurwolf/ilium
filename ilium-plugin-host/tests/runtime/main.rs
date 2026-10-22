//! Lane `host-runtime`: Wasmtime embedding, Config, traps, limits, restart rule (HST-050..089,
//! HST-400..412, API-001..081). Run with `--features testing`.
//!
//! One module per manual page; `support` holds the shared helpers. Later parts of the lane add
//! modules here.

mod api_pending_requests;
mod api_restart_and_limits;
mod api_retry_and_codes;
mod reference_errors_and_limits;
mod reference_host_runtime;
mod reference_requirements_api_host;
mod support;
