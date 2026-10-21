//! Fixture files and expectation files (reference-test-harness.md "Fixtures and expectation files").

use std::path::Path;

use crate::errors::HarnessError;

/// A parsed fixture with `"schema": 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    /// The whole document; keys are validated against the schema with `fixture.unknown-key` and a JSON path.
    pub document: serde_json::Value,
}

/// Outcome of one fixture run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureStatus {
    /// Matched the expectation file, or has none (then `test.no-expectation` is a warning).
    Pass,
    /// Differs from the expectation.
    Fail(String),
    /// Trapped with the given trap code.
    Trap(String),
}

impl Fixture {
    /// Loads and validates a fixture file.
    pub fn load(_path: &Path) -> Result<Fixture, HarnessError> {
        unimplemented!("DEV-423")
    }
}

/// Runs one fixture against the plugin type and compares the expectation file next to it.
pub fn run<P: ilium_plugin_sdk_core::NativeEntry + Default>(
    _path: &Path,
) -> Result<FixtureStatus, HarnessError> {
    unimplemented!("DEV-423")
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::errors::codes;
    use ilium_plugin_sdk_core::types::{
        Activation, DeactivationReason, Event, PluginError, Submission,
    };

    /// Server plugin used only to give `run` a type. Its bodies are neutral because the fixture is under test.
    #[derive(Default)]
    struct ServerPlugin;

    impl ilium_plugin_sdk_core::NativeEntry for ServerPlugin {
        const SIDE: &'static str = "server";
        const EXPORTS: &'static [&'static str] = &[];
        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }
        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }
        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    /// Writes a temporary fixture file whose name is unique to this test binary process.
    fn write_fixture(file_name: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ilium-fixture-spec-{}-{file_name}",
            std::process::id()
        ));
        std::fs::write(&path, body).expect("the test can write a temporary fixture file");
        path
    }

    /// REQ: Fixtures and expectation files (a fixture is JSON with "schema": 1; empty input is a schema-only document)
    #[test]
    fn fixture_schema_1_loads_and_keeps_its_document() {
        let path = write_fixture("schema-only.json", r#"{"schema": 1}"#);
        let fixture = Fixture::load(&path).expect("a schema 1 fixture loads");
        assert_eq!(fixture.document["schema"], 1);
        assert_eq!(fixture.document.as_object().map(|object| object.len()), Some(1));
    }

    /// REQ: DEV-511 (the error names the JSON path of the first unknown key, for example $.events[2].foo)
    #[test]
    fn dev_511_unknown_key_error_names_its_json_path() {
        let body = r#"{"schema": 1, "events": [
            {"tag": "a", "val": 1}, {"tag": "b", "val": 2}, {"tag": "c", "val": 3, "foo": 4}]}"#;
        let path = write_fixture("unknown-key.json", body);
        let error = Fixture::load(&path).expect_err("an unknown key is refused");
        assert_eq!(error.code, codes::FIXTURE_UNKNOWN_KEY);
        assert!(error.message.contains("$.events[2].foo"), "message was: {}", error.message);
    }

    /// REQ: DEV-511 (a wrong value reports the JSON path of the bad value, for example $.tree[2].parent)
    #[test]
    fn dev_511_bad_value_error_names_its_json_path() {
        let body = r#"{"schema": 1, "tree": [
            {"id": 1, "parent": null, "name": "root", "class": "container", "content_kind": "ilium.terminal"},
            {"id": 2, "parent": 1, "name": "child", "class": "pane", "content_kind": "ilium.terminal"},
            {"id": 3, "parent": "x", "name": "bad", "class": "pane", "content_kind": "ilium.terminal"}]}"#;
        let path = write_fixture("bad-value.json", body);
        let error = Fixture::load(&path).expect_err("a non-integer parent is refused");
        assert_eq!(error.code, codes::FIXTURE_BAD_VALUE);
        assert!(error.message.contains("$.tree[2].parent"), "message was: {}", error.message);
    }

    /// REQ: DEV-512 (a repeated node id in the tree fails with fixture.duplicate-id)
    #[test]
    fn dev_512_repeated_node_id_is_refused() {
        let body = r#"{"schema": 1, "tree": [
            {"id": 1, "parent": null, "name": "one", "class": "container", "content_kind": "ilium.terminal"},
            {"id": 1, "parent": null, "name": "again", "class": "container", "content_kind": "ilium.terminal"}]}"#;
        let path = write_fixture("duplicate-id.json", body);
        let error = Fixture::load(&path).expect_err("a repeated id is refused");
        assert_eq!(error.code, codes::FIXTURE_DUPLICATE_ID);
    }

    /// REQ: DEV-513 (integers above 2^53 are written as strings and load without loss)
    #[test]
    fn dev_513_large_integer_written_as_string_loads() {
        let body = r#"{"schema": 1, "tree": [
            {"id": "9007199254740993", "parent": null, "name": "big", "class": "container", "content_kind": "ilium.terminal"}]}"#;
        let path = write_fixture("big-id.json", body);
        let fixture = Fixture::load(&path).expect("a string id above 2^53 loads");
        assert_eq!(fixture.document["tree"][0]["id"], "9007199254740993");
    }

    /// REQ: DEV-514 (a scene fixture accepts only its own keys; tree is refused with fixture.unknown-key)
    #[test]
    fn dev_514_scene_fixture_refuses_a_server_only_key() {
        let path = write_fixture("scene-tree.json", r#"{"schema": 1, "side": "scene", "tree": []}"#);
        let error = Fixture::load(&path).expect_err("tree is not a scene key");
        assert_eq!(error.code, codes::FIXTURE_UNKNOWN_KEY);
    }

    /// REQ: DEV-516, DEV-551 (run with no expectation file passes when the component does not trap)
    #[test]
    fn dev_516_run_without_expectation_file_passes() {
        let path = write_fixture("no-expect.json", r#"{"schema": 1, "events": []}"#);
        let status = run::<ServerPlugin>(&path).expect("the fixture runs");
        assert_eq!(status, FixtureStatus::Pass);
    }

    /// REQ: DEV-516 (run compares the expectation file next to the fixture and reports Fail on a mismatch)
    #[test]
    fn dev_516_run_reports_fail_when_expectation_differs() {
        let path = write_fixture("mismatch.json", r#"{"schema": 1, "events": []}"#);
        std::fs::write(
            path.with_extension("expect.json"),
            r#"{"schema": 1, "submissions": {"match": "exact", "items": [{"intent": "set-decoration"}]}}"#,
        )
        .expect("the test can write the expectation file");
        let status = run::<ServerPlugin>(&path).expect("the fixture runs");
        assert!(matches!(status, FixtureStatus::Fail(_)), "status was: {status:?}");
    }

    /// REQ: DEV-516 (a fixture that fails to load is an error, never a Pass)
    #[test]
    fn dev_516_run_propagates_fixture_load_error() {
        let path = write_fixture("run-unknown-key.json", r#"{"schema": 1, "bogus": true}"#);
        let error = run::<ServerPlugin>(&path).expect_err("an unknown key stops the run");
        assert_eq!(error.code, codes::FIXTURE_UNKNOWN_KEY);
    }
}
