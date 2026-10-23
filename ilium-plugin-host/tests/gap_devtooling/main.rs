//! Devtooling requirement tests for the plugin developer tools, group devtooling-gap.
//!
//! Each `/// REQ: DEV-<n>` tag marks the test that covers that inventory requirement. Requirements
//! covered here: 878, 879, 881, 882, 884, 886, 889, 890, 891, 896, 897, 900, 901, 902, 903, 904,
//! 906, 907, 908, 911, 912, 913, 914, 915, 916, 919, 920, 922, 923, 924, 925, 927, 928, 931, 932,
//! 933, 934, 935, 937, 938, 939, 940, 941, 964, 965, 966, 967, 968, 969, 970, 971, 972, 975, 977,
//! 978, 982, 985, 986, 987, 988, 989, 990, 992, 993, 995, 996, 997, 998.

#[path = "../devtools/common.rs"]
mod common;

mod guide_backwards_compatibility_test_matrix;
mod guide_ci_recipes;
mod guide_compatibility_and_upgrades;
mod guide_data_migration;
mod guide_deprecating_plugin_features;
mod guide_operator_upgrades;
mod guide_plugin_documentation;
mod host_upgrade_while_plugins_run;
mod reference_abi_evolution;
mod reference_abi_major_versions;
mod reference_cli_completion;
mod reference_cli_output_stability;
mod reference_config_and_data_migration;
mod reference_conformance_suite;
mod reference_plugin_cli;
mod reference_plugin_cli_dev_failure_modes;
mod reference_plugin_debugging;
mod reference_plugin_test_harness;
mod reference_project_templates;
mod reference_sdk_deprecation;
mod reference_test_harness;
mod reference_toolchain_support_matrix;
mod typescript_sdk;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_host::cli::{CliEnv, CliOutput};
use serde_json::Value;

use crate::common::*;

/// The text of a published plugin manual page, read from `docs/plugins-manual/pages/<name>.md`.
pub(crate) fn manual_page(name: &str) -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/{name}.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read manual page {path}: {error}"))
}

/// A UTF-8 view of a path; test paths are always valid UTF-8.
pub(crate) fn utf8(path: &Path) -> &str {
    path.to_str().expect("test path is valid UTF-8")
}

/// A Rust server project made by `ilium plugin new`, with its environment and project folder.
pub(crate) fn new_rust_project() -> (tempfile::TempDir, CliEnv, PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(
        &env,
        &[
            "new",
            dir.to_str().expect("utf8 path"),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    (root, env, dir)
}

/// Every record of `output` that carries the field `key`, in printed order.
pub(crate) fn rows_with<'a>(output: &'a CliOutput, key: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record.get(key).is_some())
        .collect()
}

/// Every record of `output` whose `code` or `rule` field equals `code`, in printed order.
pub(crate) fn with_code<'a>(output: &'a CliOutput, code: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| {
            record.get("code").and_then(Value::as_str) == Some(code)
                || record.get("rule").and_then(Value::as_str) == Some(code)
        })
        .collect()
}

/// Every file under `dir` mapped to its bytes, keyed by path relative to `dir`. A missing folder
/// yields an empty map, so a snapshot taken before a folder exists still compares.
pub(crate) fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    if !dir.exists() {
        return BTreeMap::new();
    }
    tree_of(dir)
        .into_iter()
        .map(|relative| {
            let bytes = std::fs::read(dir.join(&relative)).expect("read snapshot file");
            (relative, bytes)
        })
        .collect()
}

/// The SHA-256 of one file, as lowercase hex.
pub(crate) fn file_sha256(path: &Path) -> String {
    sha256_hex(&std::fs::read(path).expect("read file for digest"))
}

// Test-support helpers for the devtooling-gap tests. Each is a stub until the plugin developer
// tools expose the hook it needs, so the tests that call it fail with `unimplemented!` (red).
/// A running `ilium plugin dev` session.
pub(crate) struct DevSession;

impl DevSession {
    pub(crate) fn edit(&mut self, _rel: &str, _text: &str) {
        unimplemented!("gap_devtooling::DevSession::edit")
    }

    pub(crate) fn records_since_edit(&self) -> Vec<Value> {
        unimplemented!("gap_devtooling::DevSession::records_since_edit")
    }

    pub(crate) fn instance_answers(&self) -> bool {
        unimplemented!("gap_devtooling::DevSession::instance_answers")
    }

    pub(crate) fn is_alive(&self) -> bool {
        unimplemented!("gap_devtooling::DevSession::is_alive")
    }

    pub(crate) fn records(&self) -> Vec<Value> {
        unimplemented!("gap_devtooling::DevSession::records")
    }

    pub(crate) fn poll_interval_ms(&self) -> u64 {
        unimplemented!("gap_devtooling::DevSession::poll_interval_ms")
    }

    pub(crate) fn trigger_activation_crashes(&mut self, _count: usize) {
        unimplemented!("gap_devtooling::DevSession::trigger_activation_crashes")
    }

    pub(crate) fn reload_attempts(&self) -> usize {
        unimplemented!("gap_devtooling::DevSession::reload_attempts")
    }
}

/// A session that calls a deprecated plugin item.
pub(crate) struct DeprecatedItemSession;

impl DeprecatedItemSession {
    pub(crate) fn call_deprecated_item(&self) -> CliOutput {
        unimplemented!("gap_devtooling::DeprecatedItemSession::call_deprecated_item")
    }

    pub(crate) fn warn_log_lines(&self) -> Vec<Value> {
        unimplemented!("gap_devtooling::DeprecatedItemSession::warn_log_lines")
    }

    pub(crate) fn advance_clock_ms(&self, _milliseconds: u64) {
        unimplemented!("gap_devtooling::DeprecatedItemSession::advance_clock_ms")
    }
}

/// The fake host a plugin test runs against.
pub(crate) struct FakeHostHarness;

impl FakeHostHarness {
    pub(crate) fn call_host(&mut self, _method: &str, _argument: &str) -> Result<Value, Value> {
        unimplemented!("gap_devtooling::FakeHostHarness::call_host")
    }

    pub(crate) fn connect_real_socket(&mut self, _address: &str) -> Result<Value, Value> {
        unimplemented!("gap_devtooling::FakeHostHarness::connect_real_socket")
    }

    pub(crate) fn read_real_clock(&mut self) -> Result<Value, Value> {
        unimplemented!("gap_devtooling::FakeHostHarness::read_real_clock")
    }

    pub(crate) fn read_file_outside_test_directory(&mut self, _path: &str) -> Result<Value, Value> {
        unimplemented!("gap_devtooling::FakeHostHarness::read_file_outside_test_directory")
    }
}

/// The result of asking the shell completion for plugin ids.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Completion {
    pub(crate) candidates: Vec<String>,
    pub(crate) stderr: String,
    pub(crate) list_json_calls: usize,
}

/// A plugin built against one ABI minor.
pub(crate) struct PluginBuild;

impl PluginBuild {
    pub(crate) fn loads_on_host(&self, _host_minor: &str) -> bool {
        unimplemented!("gap_devtooling::PluginBuild::loads_on_host")
    }
}

/// When an ABI item was deprecated and removed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AbiRemoval {
    pub(crate) deprecated_in_minor: u32,
    pub(crate) removed_in_minor: u32,
}

/// What happened when the host tried to load a plugin directory.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoadAttempt {
    pub(crate) started: bool,
    pub(crate) error_code: Option<String>,
}

/// The report of the ABI compatibility gate between two releases.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AbiGateReport {
    pub(crate) passes: bool,
    pub(crate) failing_rule: Option<String>,
}

impl AbiGateReport {
    pub(crate) fn named_items(&self) -> Vec<String> {
        unimplemented!("gap_devtooling::AbiGateReport::named_items")
    }

    pub(crate) fn changed_items(&self) -> Vec<String> {
        unimplemented!("gap_devtooling::AbiGateReport::changed_items")
    }
}

/// The announcement of a major release.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MajorAnnouncement {
    pub(crate) days_before_release: u32,
    pub(crate) removals_at_release: usize,
    pub(crate) removals_announced: usize,
}

/// A released plugin kept as a backwards-compatibility fixture.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginFixture {
    pub(crate) version: String,
}

/// Whether a fixture loaded on the current host.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoadOutcome {
    pub(crate) loaded: bool,
    pub(crate) error_code: Option<String>,
}

pub(crate) fn dev_session(_env: &CliEnv) -> DevSession {
    unimplemented!("gap_devtooling::dev_session")
}

pub(crate) fn dev_session_with_watcher_fault(_env: &CliEnv, _fault: &str) -> DevSession {
    unimplemented!("gap_devtooling::dev_session_with_watcher_fault")
}

pub(crate) fn dev_session_with_crashing_plugin(_env: &CliEnv) -> DevSession {
    unimplemented!("gap_devtooling::dev_session_with_crashing_plugin")
}

pub(crate) fn matrix_component_hashes(_env: &CliEnv) -> BTreeMap<String, String> {
    unimplemented!("gap_devtooling::matrix_component_hashes")
}

pub(crate) fn unsupported_host_env(_root: &Path, _os: &str, _arch: &str) -> CliEnv {
    unimplemented!("gap_devtooling::unsupported_host_env")
}

pub(crate) fn end_of_life_host_env(_root: &Path, _date: &str) -> CliEnv {
    unimplemented!("gap_devtooling::end_of_life_host_env")
}

pub(crate) fn server_start_log_lines(_env: &CliEnv) -> Vec<Value> {
    unimplemented!("gap_devtooling::server_start_log_lines")
}

pub(crate) fn installed_plugins_still_load(_env: &CliEnv) -> bool {
    unimplemented!("gap_devtooling::installed_plugins_still_load")
}

pub(crate) fn announced_minimum_raises() -> Vec<(u32, u32)> {
    unimplemented!("gap_devtooling::announced_minimum_raises")
}

pub(crate) fn toolchain_minimums(_release: &str) -> BTreeMap<String, String> {
    unimplemented!("gap_devtooling::toolchain_minimums")
}

pub(crate) fn install_toolchain_one_minor_newer_than_tested(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::install_toolchain_one_minor_newer_than_tested")
}

pub(crate) fn installed_rust_version(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::installed_rust_version")
}

pub(crate) fn build_pinned_component_on_second_host(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::build_pinned_component_on_second_host")
}

pub(crate) fn deprecated_command_env(_root: &Path, _command: &str, _message: &str) -> CliEnv {
    unimplemented!("gap_devtooling::deprecated_command_env")
}

pub(crate) fn deprecated_fixture_env(_root: &Path) -> CliEnv {
    unimplemented!("gap_devtooling::deprecated_fixture_env")
}

pub(crate) fn deprecated_item_env(_root: &Path) -> CliEnv {
    unimplemented!("gap_devtooling::deprecated_item_env")
}

pub(crate) fn deprecated_item_session(_env: &CliEnv) -> DeprecatedItemSession {
    unimplemented!("gap_devtooling::deprecated_item_session")
}

pub(crate) fn package_declaring_expired_item(
    _env: &CliEnv,
    _item: &str,
    _remove_in: &str,
) -> String {
    unimplemented!("gap_devtooling::package_declaring_expired_item")
}

pub(crate) fn fake_host_harness(_allowed: &[&str]) -> FakeHostHarness {
    unimplemented!("gap_devtooling::fake_host_harness")
}

pub(crate) fn failing_case_run(_root: &Path, _home: &str) -> CliOutput {
    unimplemented!("gap_devtooling::failing_case_run")
}

pub(crate) fn project_with_fixtures(_names: &[&str]) -> (tempfile::TempDir, CliEnv, PathBuf) {
    unimplemented!("gap_devtooling::project_with_fixtures")
}

pub(crate) fn cli_build_with_changed_record_field(
    _root: &Path,
    _record: &str,
    _field: &str,
) -> CliEnv {
    unimplemented!("gap_devtooling::cli_build_with_changed_record_field")
}

pub(crate) fn cli_with_deprecation_entry(_env: &CliEnv, _id: &str) -> CliEnv {
    unimplemented!("gap_devtooling::cli_with_deprecation_entry")
}

pub(crate) fn deprecated_flag_invocation(_root: &Path) -> (Vec<String>, CliEnv) {
    unimplemented!("gap_devtooling::deprecated_flag_invocation")
}

pub(crate) fn env_at_contract_minor_offset(_env: &CliEnv, _minor_offset: u32) -> CliEnv {
    unimplemented!("gap_devtooling::env_at_contract_minor_offset")
}

pub(crate) fn plant_locked_dependency_with_advisory(
    _env: &CliEnv,
    _name: &str,
    _version: &str,
) -> String {
    unimplemented!("gap_devtooling::plant_locked_dependency_with_advisory")
}

pub(crate) fn pack_with_warm_cache(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::pack_with_warm_cache")
}

pub(crate) fn manifest_requires_range_bounds(_env: &CliEnv) -> (String, String) {
    unimplemented!("gap_devtooling::manifest_requires_range_bounds")
}

pub(crate) fn sdk_package_metadata() -> Value {
    unimplemented!("gap_devtooling::sdk_package_metadata")
}

pub(crate) fn sdk_builds_with_toolchain(_toolchain: &str) -> bool {
    unimplemented!("gap_devtooling::sdk_builds_with_toolchain")
}

pub(crate) fn cli_with_changed_template_files(_env: &CliEnv, _files: &[&str]) -> CliEnv {
    unimplemented!("gap_devtooling::cli_with_changed_template_files")
}

pub(crate) fn add_slow_test_fixture(_dir: &Path) {
    unimplemented!("gap_devtooling::add_slow_test_fixture")
}

pub(crate) fn complete_plugin_ids(_env: &CliEnv, _partial: &str) -> Completion {
    unimplemented!("gap_devtooling::complete_plugin_ids")
}

pub(crate) fn package_component_limit_bytes() -> usize {
    unimplemented!("gap_devtooling::package_component_limit_bytes")
}

pub(crate) fn sdk_current_version(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::sdk_current_version")
}

pub(crate) fn next_sdk_minor(_current: &str) -> String {
    unimplemented!("gap_devtooling::next_sdk_minor")
}

pub(crate) fn project_pinned_sdk_behind_cli(_env: &CliEnv, _minors_behind: u32) -> CliEnv {
    unimplemented!("gap_devtooling::project_pinned_sdk_behind_cli")
}

pub(crate) fn dev_watcher_poll_interval_when_native_unavailable(_env: &CliEnv) -> u64 {
    unimplemented!("gap_devtooling::dev_watcher_poll_interval_when_native_unavailable")
}

pub(crate) fn pack_interrupted_by(_env: &CliEnv, _command: &str, _signal: &str) -> CliOutput {
    unimplemented!("gap_devtooling::pack_interrupted_by")
}

pub(crate) fn bench_with_deadline_exceeded(_env: &CliEnv) -> CliOutput {
    unimplemented!("gap_devtooling::bench_with_deadline_exceeded")
}

pub(crate) fn seed_dev_data(_env: &CliEnv) {
    unimplemented!("gap_devtooling::seed_dev_data")
}

pub(crate) fn dev_data_folders(_env: &CliEnv) -> Vec<PathBuf> {
    unimplemented!("gap_devtooling::dev_data_folders")
}

pub(crate) fn dev_once(_env: &CliEnv, _extra: &[&str]) -> CliOutput {
    unimplemented!("gap_devtooling::dev_once")
}

pub(crate) fn zip_entries(_bundle: &Path) -> Vec<(String, Vec<u8>)> {
    unimplemented!("gap_devtooling::zip_entries")
}

pub(crate) fn add_oversize_bundle_payload(_project: &Path, _mib: u64) {
    unimplemented!("gap_devtooling::add_oversize_bundle_payload")
}

pub(crate) fn cold_instantiate_ms(_bundle: &Path) -> Duration {
    unimplemented!("gap_devtooling::cold_instantiate_ms")
}

pub(crate) fn embedded_suite_version(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::embedded_suite_version")
}

pub(crate) fn add_deprecated_sdk_call(_project: &Path, _old_item: &str, _replacement: &str) {
    unimplemented!("gap_devtooling::add_deprecated_sdk_call")
}

pub(crate) fn current_sdk_version(_env: &CliEnv) -> String {
    unimplemented!("gap_devtooling::current_sdk_version")
}

pub(crate) fn sdk_removal_release(_item: &str) -> String {
    unimplemented!("gap_devtooling::sdk_removal_release")
}

pub(crate) fn run_plugin_calling_deprecated_item(_env: &CliEnv) -> CliOutput {
    unimplemented!("gap_devtooling::run_plugin_calling_deprecated_item")
}

pub(crate) fn abi_surface(_minor: &str) -> Vec<String> {
    unimplemented!("gap_devtooling::abi_surface")
}

pub(crate) fn plugin_built_against(_minor: &str) -> PluginBuild {
    unimplemented!("gap_devtooling::plugin_built_against")
}

pub(crate) fn abi_removal_of(_item: &str) -> AbiRemoval {
    unimplemented!("gap_devtooling::abi_removal_of")
}

pub(crate) fn deprecated_item_build(_env: &CliEnv, _item: &str) -> CliOutput {
    unimplemented!("gap_devtooling::deprecated_item_build")
}

pub(crate) fn set_manifest_abi(_dir: &Path, _abi: &str) {
    unimplemented!("gap_devtooling::set_manifest_abi")
}

pub(crate) fn host_load_attempt(_dir: &Path, _abi: &str) -> LoadAttempt {
    unimplemented!("gap_devtooling::host_load_attempt")
}

pub(crate) fn abi_compat_gate(_from: &str, _to: &str) -> AbiGateReport {
    unimplemented!("gap_devtooling::abi_compat_gate")
}

pub(crate) fn support_dates_of_minor(_minor: &str) -> (String, String) {
    unimplemented!("gap_devtooling::support_dates_of_minor")
}

pub(crate) fn plus_twelve_months(_date: &str) -> String {
    unimplemented!("gap_devtooling::plus_twelve_months")
}

pub(crate) fn release_surface(_version: &str) -> Vec<String> {
    unimplemented!("gap_devtooling::release_surface")
}

pub(crate) fn supported_minors_at(_version: &str) -> Vec<String> {
    unimplemented!("gap_devtooling::supported_minors_at")
}

pub(crate) fn patch_release_exists_for(_minor: &str) -> bool {
    unimplemented!("gap_devtooling::patch_release_exists_for")
}

pub(crate) fn major_announcement(_version: &str) -> MajorAnnouncement {
    unimplemented!("gap_devtooling::major_announcement")
}

pub(crate) fn released_plugin_fixtures() -> Vec<PluginFixture> {
    unimplemented!("gap_devtooling::released_plugin_fixtures")
}

pub(crate) fn load_fixture_on_current_host(_fixture: &PluginFixture) -> LoadOutcome {
    unimplemented!("gap_devtooling::load_fixture_on_current_host")
}
