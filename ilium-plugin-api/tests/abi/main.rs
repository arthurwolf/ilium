//! ABI tests of the plugin contract: ABI-001..ABI-051 of reference-requirements-wit-abi.md.
//!
//! Every test carries a `/// REQ: ABI-0NN` tag. Tests named `doc_*` lint the manual and wit/;
//! they and the generated serde round trips are pure data and pass against the skeleton.
//! Every other test calls an `unimplemented!` stub and fails until the implementation lands.

mod batch;
mod cases;
mod codes;
mod compat;
mod conformance;
mod dedup;
mod deprecation_notice;
mod generated_roundtrip;
mod manifest;
mod naming;
mod queue;
mod runtime;
mod support;
mod wit_source;
