//! Shims for API the skeleton does not have yet. Each function panics with `api.gap` and the
//! signature the test needs, so the test binary compiles and the test fails red. The same
//! signatures are listed in `API_GAPS.md`. When the real item lands, replace the body with the
//! real call and delete the bullet.

use std::path::Path;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::DeactivationReason;
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::{HarnessHandle, HostHarness};
use ilium_plugin_testkit::{FakeHost, HarnessError};

fn missing<T>(signature: &str) -> T {
    panic!("api.gap: {signature}");
}

/// ilium-plugin-testkit `conformance::DEFAULT_SEED: u64` (DEV-624).
pub const DEFAULT_SEED: u64 = 0x1_1D5E_ED00;

/// ilium-plugin-testkit `conformance::is_project_case_id(id: &str) -> bool` (DEV-620).
pub fn is_project_case_id(_id: &str) -> bool {
    missing("ilium_plugin_testkit::conformance::is_project_case_id(id: &str) -> bool")
}

/// ilium-plugin-testkit `conformance::sample_batch(n: usize) -> Vec<Event>` (DEV-621).
pub fn sample_batch(_n: usize) -> Vec<Event> {
    missing("ilium_plugin_testkit::conformance::sample_batch(n: usize) -> Vec<Event>")
}

/// ilium-plugin-testkit `conformance::case(id, title, body)` (DEV-624).
pub fn case(_id: &str, _title: &str, _body: impl FnOnce(&mut FakeHost)) {
    missing("ilium_plugin_testkit::conformance::case(id: &str, title: &str, body: impl FnOnce(&mut FakeHost))")
}

/// ilium-plugin-testkit `conformance::case_in(dir, id, title, body)`: `case` for a project folder (DEV-624).
pub fn case_in(_dir: &Path, _id: &str, _title: &str, _body: impl FnOnce(&mut FakeHost)) {
    missing("ilium_plugin_testkit::conformance::case_in(dir: &Path, id: &str, title: &str, body: impl FnOnce(&mut FakeHost))")
}

/// ilium-plugin-testkit `conformance::try_case_in(..) -> Result<(), HarnessError>` (DEV-624).
pub fn try_case_in(
    _dir: &Path,
    _id: &str,
    _title: &str,
    _body: impl FnOnce(&mut FakeHost),
) -> Result<(), HarnessError> {
    missing("ilium_plugin_testkit::conformance::try_case_in(dir: &Path, id: &str, title: &str, body: impl FnOnce(&mut FakeHost)) -> Result<(), HarnessError>")
}

/// ilium-plugin-testkit `conformance::check_within_budget(host, kind, millis)`: the non-panicking
/// form of `assert_within_budget`. `Ok(Some("needs-component"))` is the native-mode skip (DEV-622).
pub fn check_within_budget(
    _host: &FakeHost,
    _kind: &str,
    _millis: u32,
) -> Result<Option<String>, HarnessError> {
    missing("ilium_plugin_testkit::conformance::check_within_budget(host: &FakeHost, kind: &str, millis: u32) -> Result<Option<String>, HarnessError>")
}

/// ilium-plugin-testkit `conformance::assert_within_budget(host, kind, millis)` (DEV-622).
pub fn assert_within_budget(_host: &FakeHost, _kind: &str, _millis: u32) {
    missing("ilium_plugin_testkit::conformance::assert_within_budget(host: &FakeHost, kind: &str, millis: u32)")
}

/// ilium-plugin-host `HostHarness::with_component(bytes: Vec<u8>) -> HostHarness` (CONF probe cases).
pub fn with_component(_harness: HostHarness, _bytes: Vec<u8>) -> HostHarness {
    missing("ilium_plugin_host::testing::HostHarness::with_component(self, bytes: Vec<u8>) -> HostHarness")
}

/// ilium-plugin-host `HostHarness::settings_json(json: String) -> HostHarness` (CONF probe cases).
pub fn settings_json(_harness: HostHarness, _json: String) -> HostHarness {
    missing(
        "ilium_plugin_host::testing::HostHarness::settings_json(self, json: String) -> HostHarness",
    )
}

/// ilium-plugin-host `HarnessHandle::deactivate(reason) -> Option<Vec<u8>>` (CONF-LIFE-03, -05).
pub fn deactivate(_handle: &mut HarnessHandle, _reason: DeactivationReason) -> Option<Vec<u8>> {
    missing("ilium_plugin_host::testing::HarnessHandle::deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>>")
}

/// ilium-plugin-host `HarnessHandle::render(view, area) -> Result<UiTree, Trap>` (CONF-VIEW-*).
pub fn render(_handle: &mut HarnessHandle, _view: ViewTarget, _area: Area) -> Result<UiTree, Trap> {
    missing("ilium_plugin_host::testing::HarnessHandle::render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap>")
}

/// ilium-plugin-host `HarnessHandle::restore_bytes() -> Option<Vec<u8>>` (CONF-LIFE-03).
pub fn restore_bytes(_handle: &HarnessHandle) -> Option<Vec<u8>> {
    missing("ilium_plugin_host::testing::HarnessHandle::restore_bytes(&self) -> Option<Vec<u8>>")
}

/// ilium-plugin-host `HarnessHandle::revoke(grant: &str)` (CONF-GRANT-02).
pub fn revoke(_handle: &mut HarnessHandle, _grant: &str) {
    missing("ilium_plugin_host::testing::HarnessHandle::revoke(&mut self, grant: &str)")
}

/// ilium-plugin-host `HarnessHandle::click_action(key: &str)` (CONF-VIEW-04).
pub fn click_action(_handle: &mut HarnessHandle, _target: ViewTarget, _key: &str) {
    missing("ilium_plugin_host::testing::HarnessHandle::click_action(&mut self, target: ViewTarget, key: &str)")
}

/// ilium-plugin-testkit `fixture::ui_tree_with_nodes(count: usize) -> UiTree`: a valid tree of exactly `count` nodes (CONF-VIEW-03).
pub fn ui_tree_with_nodes(_count: usize) -> UiTree {
    missing("ilium_plugin_testkit::fixture::ui_tree_with_nodes(count: usize) -> UiTree")
}

/// ilium-plugin-host `HarnessHandle::state_is(InstanceState)` is not needed; kept as a note that
/// `HarnessHandle::state()` already exists.
pub fn state_name(state: InstanceState) -> &'static str {
    match state {
        InstanceState::Inactive => "inactive",
        InstanceState::Activating => "activating",
        InstanceState::WaitingDependency => "waiting-dependency",
        InstanceState::Active => "active",
        InstanceState::Deactivating => "deactivating",
        InstanceState::RestartScheduled => "restart-scheduled",
        InstanceState::Failed => "failed",
        InstanceState::StoppedByFailure => "stopped-by-failure",
        InstanceState::Stopped => "stopped",
    }
}
