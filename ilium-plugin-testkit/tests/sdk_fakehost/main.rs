//! sdk-fakehost lane, testkit crate: unit tests of the harness against `reference-test-harness.md`
//! (requirement ids DEV-400 to DEV-514). One module per manual section.
//!
//! The skeleton bodies are `unimplemented!()`, so every test here is red until the harness is
//! written. Tests that need an API the skeleton lacks are in `API_GAPS.md` and, where they cannot
//! compile, behind the cfg flag named in that file.

mod cli_support;
mod common;
mod kernel_regressions;

mod reference_test_harness_activation;
mod reference_test_harness_coverage;
mod reference_test_harness_events;
mod reference_test_harness_fakes;
mod reference_test_harness_fixtures;
mod reference_test_harness_fuzz;
mod reference_test_harness_gated;
mod reference_test_harness_golden;
mod reference_test_harness_modes;
mod reference_test_harness_replay;
mod reference_test_harness_report;
mod reference_test_harness_scenes;
mod reference_test_harness_session;
mod reference_test_harness_setup;
mod reference_test_harness_setup_p2;
mod reference_test_harness_traps;
mod reference_test_harness_ts_contract;
mod reference_test_harness_views;
