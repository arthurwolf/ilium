//! Red-baseline conformance tests of the Ilium plugin platform (test-plan.md 1.3 lane
//! `conformance`). One module per manual page covered; `common` holds the shared helpers.
//!
//! Every test cites its requirement id(s) in a `REQ:` comment so `tools/req_coverage.py` can
//! trace it. Missing API is listed in `API_GAPS.md` beside this file.

mod common;
mod gap;
mod reference_conformance_suite;
mod reference_conformance_suite_cases;
mod reference_conformance_suite_restart;
mod testing;
