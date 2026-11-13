# API gaps for the events-intents lane

Tests are written against the manual; these items are missing from the skeleton.

- ilium-plugin-host: `HostHarness::with_component(self, name: &str) -> Self`; load a fixture component (EVT-*, WIT exports); crate-wide, reason: no component loader in the testing module.
- ilium-plugin-host: `HarnessHandle::advance_clock(&self, by: std::time::Duration)`; RQO-004, RQO-008, RQO-021, RQO-022, RQO-030; reason: dedup window and result hold need a manual clock.
- ilium-plugin-host: `HarnessHandle::submit_directly(&self, s: Submission) -> Result<RequestId, ErrorCode>` and `take_result(&self, RequestId) -> Result<RequestResult, ErrorCode>`; RQO-002, RQO-008, WIT:ilium:host/intents-host.submit.
- ilium-plugin-host: `HarnessHandle::{restart_server, restart_instance(DeactivationReason), deactivate_plugin, shutdown, add_second_plugin(&[&str]), failures() -> Vec<Failure>}`; RQO-006, RQO-030, RQO-031, EVT-047, EVT-003.
- ilium-plugin-host: `FakeKernel::{emit_event, set_tree, set_pane_restored, step_log, interrupt_after_journal, hold_next_request, fail_next_write, veto_next_hook, sides_that_applied}`; EVT-005..050, RQO-*; reason: kernel scripting hooks.
- ilium-plugin-host: forwarding probes `forwarded_submissions, attach_second_client, forward_latency_of, mirror_update_index, request_completed_index, client_tree_read, testing::applying_side, testing::a_client_applied_intent`; HST-490..494.
- ilium-plugin-api: `Intent::CASE_NAMES`, `Outcome::CASE_NAMES`, `Event::case_name()`; EVT-010, EVT-014.
- Plugin id is taken from the Activation the guest receives (`Shared::plugin()`).
- Fixture contracts: none required beyond `fixture_component`, which fails red with `fixture.missing`.
- ilium-plugin-host: `HostHarness`/`HarnessHandle::now_monotonic() -> std::time::Duration` and `FakeKernel::freeze_server_tree_reads()`; EVT/HST tests in api_intents.rs; reason: clock and tree-read probes.
