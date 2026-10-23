//! Gap lane for the ABI requirements ABI-063 to ABI-120 of reference-requirements-wit-abi.md:
//! one module per manual page. Groups: errors-and-limits (ABI-063, ABI-082), abi-evolution
//! (ABI-064, ABI-065, ABI-066, ABI-119), wit (ABI-068, ABI-069, ABI-106, ABI-107, ABI-108,
//! ABI-109, ABI-110, ABI-111, ABI-112, ABI-117), host-runtime (ABI-070, ABI-071, ABI-072),
//! host-conformance (ABI-073, ABI-074, ABI-075, ABI-120), abi-major-versions (ABI-077, ABI-078),
//! health-and-watchdog (ABI-079, ABI-080), runtime-model (ABI-081, ABI-100, ABI-101, ABI-102,
//! ABI-105, ABI-114). Red by design: the calls target hooks that do not exist yet.

mod reference_abi_evolution;
mod reference_abi_major_versions;
mod reference_errors_and_limits;
mod reference_health_and_watchdog;
mod reference_host_conformance;
mod reference_host_runtime;
mod reference_runtime_model;
mod reference_wit;
