//! reference-test-harness.md "Fixtures and expectation files": DEV-510 to DEV-516.

use std::path::PathBuf;

use ilium_plugin_sdk_core::host::InstanceState;
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::fixture::{run, Fixture, FixtureStatus};
use ilium_plugin_testkit::FakeHost;

use crate::common::{
    assert_fails_with, client_batch, outcome_code, sound, spy, spy_reset, spy_script, temp_dir,
    with_clean_env, write_file, Spy, DEFAULT_CLOCK_MS,
};

fn fixture(label: &str, body: &str) -> PathBuf {
    write_file(&temp_dir(label), "case.json", body)
}

fn load_error_text(body: &str, label: &str) -> String {
    let path = fixture(label, body);
    let error = Fixture::load(&path).expect_err("fixture must be refused");
    format!("{}: {}", error.code, error.message)
}

// ---- DEV-510 ---------------------------------------------------------------------------------

/// REQ: DEV-510
#[test]
fn dev_510_a_fixture_clock_sets_the_start_time_when_the_builder_did_not() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture(
            "f510a",
            r#"{ "schema": 1, "clock": "2031-02-03 04:05:06" }"#,
        );
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.run_fixture(&path).expect("fixture runs");
        assert_eq!(host.clock().now(), 1_927_947_906_000);
    });
}

/// REQ: DEV-510
#[test]
fn dev_510_a_fixture_grant_list_applies_when_the_builder_set_none() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture("f510b", r#"{ "schema": 1, "grants": ["sound.play"] }"#);
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.run_fixture(&path).expect("fixture runs");
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        assert_eq!(
            outcome_code(&host.outcomes()[0]),
            None,
            "sound.play was granted by the fixture"
        );
    });
}

/// REQ: DEV-510
#[test]
fn dev_510_the_builder_grant_list_wins_over_the_fixture_grant_list() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture("f510c", r#"{ "schema": 1, "grants": [] }"#);
        let mut host = FakeHost::new().grants(&["sound.play"]);
        host.activate::<Spy>();
        host.run_fixture(&path).expect("fixture runs");
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        assert_eq!(
            outcome_code(&host.outcomes()[0]),
            None,
            "the builder grant stays"
        );
    });
}

// ---- DEV-511 ---------------------------------------------------------------------------------

/// REQ: DEV-511
#[test]
fn dev_511_a_bad_value_reports_the_json_path_of_the_first_bad_key() {
    with_clean_env(|| {
        let text = load_error_text(
            r#"{ "schema": 1, "tree": [
                { "id": 1, "parent": null, "name": "a", "class": "pane", "content_kind": "x" },
                { "id": 2, "parent": 1, "name": "b", "class": "pane", "content_kind": "x" },
                { "id": 3, "parent": "not a number", "name": "c", "class": "pane", "content_kind": "x" }
            ] }"#,
            "f511a",
        );
        assert!(
            text.starts_with("fixture.bad-value: $.tree[2].parent"),
            "{text}"
        );
    });
}

/// REQ: DEV-511
#[test]
fn dev_511_of_two_bad_keys_the_first_is_reported() {
    with_clean_env(|| {
        let text = load_error_text(r#"{ "schema": "one", "tier": 5 }"#, "f511b");
        assert!(text.contains("$.schema"), "{text}");
        assert!(!text.contains("$.tier"), "{text}");
    });
}

/// REQ: DEV-511
#[test]
fn dev_511_an_unknown_key_at_depth_reports_its_path() {
    with_clean_env(|| {
        let text = load_error_text(
            r#"{ "schema": 1, "events": [ {"tag":"theme-changed"}, {"tag":"theme-changed"}, {"tag":"theme-changed","foo":1} ] }"#,
            "f511c",
        );
        assert!(
            text.starts_with("fixture.unknown-key: $.events[2].foo"),
            "{text}"
        );
    });
}

// ---- DEV-512 ---------------------------------------------------------------------------------

/// REQ: DEV-512
#[test]
fn dev_512_a_repeated_node_id_fails_with_duplicate_id() {
    with_clean_env(|| {
        let text = load_error_text(
            r#"{ "schema": 1, "tree": [
                { "id": 7, "parent": null, "name": "a", "class": "pane", "content_kind": "x" },
                { "id": 7, "parent": null, "name": "b", "class": "pane", "content_kind": "x" }
            ] }"#,
            "f512a",
        );
        assert!(text.starts_with(codes::FIXTURE_DUPLICATE_ID), "{text}");
    });
}

/// REQ: DEV-512
#[test]
fn dev_512_distinct_node_ids_load() {
    with_clean_env(|| {
        let path = fixture(
            "f512b",
            r#"{ "schema": 1, "tree": [
                { "id": 1, "parent": null, "name": "a", "class": "pane", "content_kind": "x" },
                { "id": 2, "parent": 1, "name": "b", "class": "pane", "content_kind": "x" }
            ] }"#,
        );
        let loaded = Fixture::load(&path).expect("valid fixture");
        assert_eq!(loaded.document["tree"].as_array().map(Vec::len), Some(2));
        assert_eq!(loaded.document["schema"], 1);
    });
}

// ---- DEV-513 ---------------------------------------------------------------------------------

/// REQ: DEV-513
#[test]
fn dev_513_an_integer_above_two_to_the_53_written_as_a_string_is_read_as_a_u64() {
    with_clean_env(|| {
        spy_reset();
        let big = (1u64 << 53) + 1;
        let path = fixture(
            "f513a",
            &format!(
                r#"{{ "schema": 1, "grants": ["tree.read"],
                    "tree": [ {{ "id": "{big}", "parent": null, "name": "a", "class": "pane", "content_kind": "x" }} ] }}"#
            ),
        );
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.run_fixture(&path).expect("fixture runs");
        assert!(
            host.tree().get(big).is_some(),
            "node id {big} must survive exactly"
        );
    });
}

/// REQ: DEV-513
#[test]
fn dev_513_the_largest_u64_as_a_string_loads_and_a_larger_one_is_a_bad_value() {
    with_clean_env(|| {
        let max = fixture(
            "f513b",
            r#"{ "schema": 1, "tree": [ { "id": "18446744073709551615", "parent": null, "name": "a", "class": "pane", "content_kind": "x" } ] }"#,
        );
        let loaded = Fixture::load(&max).expect("u64::MAX loads");
        assert_eq!(loaded.document["tree"][0]["id"], "18446744073709551615");
        let text = load_error_text(
            r#"{ "schema": 1, "tree": [ { "id": "18446744073709551616", "parent": null, "name": "a", "class": "pane", "content_kind": "x" } ] }"#,
            "f513c",
        );
        assert!(
            text.starts_with("fixture.bad-value: $.tree[0].id"),
            "{text}"
        );
    });
}

// ---- DEV-514 ---------------------------------------------------------------------------------

/// REQ: DEV-514
#[test]
fn dev_514_a_scene_fixture_accepts_exactly_the_five_documented_keys() {
    with_clean_env(|| {
        let path = fixture(
            "f514a",
            r#"{ "schema": 1, "side": "scene", "settings": {}, "clock": "2030-01-01 00:00:00", "frames": [] }"#,
        );
        let loaded = Fixture::load(&path).expect("the five documented keys load");
        for key in ["schema", "side", "settings", "clock", "frames"] {
            assert!(loaded.document.get(key).is_some(), "{key} is kept");
        }
        assert_eq!(loaded.document.as_object().expect("object").len(), 5);
        let sixth = load_error_text(
            r#"{ "schema": 1, "side": "scene", "settings": {}, "clock": "2030-01-01 00:00:00", "frames": [], "events": [] }"#,
            "f514a2",
        );
        assert!(
            sixth.starts_with("fixture.unknown-key: $.events"),
            "{sixth}"
        );
    });
}

/// REQ: DEV-514
#[test]
fn dev_514_every_other_key_in_a_scene_fixture_fails_with_unknown_key() {
    with_clean_env(|| {
        for key in [
            "grants",
            "tier",
            "tree",
            "panes",
            "agents",
            "storage",
            "files",
            "http",
            "processes",
            "events",
            "completions",
            "lifecycle",
            "expect_state",
        ] {
            let body = format!(r#"{{ "schema": 1, "side": "scene", "{key}": null }}"#);
            let text = load_error_text(&body, "f514b");
            assert!(
                text.starts_with(&format!("fixture.unknown-key: $.{key}")),
                "{key}: {text}"
            );
        }
    });
}

/// REQ: DEV-514
#[test]
fn dev_514_a_server_fixture_accepts_the_non_scene_keys() {
    with_clean_env(|| {
        let path = fixture(
            "f514c",
            r#"{ "schema": 1, "side": "server", "grants": [], "tier": "T1", "events": [] }"#,
        );
        let loaded = Fixture::load(&path).expect("loads");
        assert_eq!(loaded.document["tier"], "T1");
        assert_eq!(loaded.document.as_object().expect("object").len(), 4);
    });
}

/// REQ: DEV-514
#[test]
fn dev_514_lifecycle_steps_run_after_the_events_in_order() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture(
            "f514d",
            r#"{ "schema": 1, "events": [ { "tag": "theme-changed" } ],
                "lifecycle": [ { "step": "advance", "value": 5000 }, { "step": "restart", "value": "AQI=" } ],
                "expect_state": "active" }"#,
        );
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.run_fixture(&path).expect("states match");
        let log = spy();
        assert!(matches!(
            log.batches[0][0],
            ilium_plugin_sdk_core::types::Event::ThemeChanged
        ));
        assert_eq!(
            log.activations.last().expect("restart activation").restored,
            Some(vec![1, 2])
        );
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 5_000);
    });
}

/// REQ: DEV-514
#[test]
fn dev_514_expect_state_that_differs_fails_with_state_mismatch() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture(
            "f514e",
            r#"{ "schema": 1, "expect_state": "stopped-by-failure" }"#,
        );
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        let error = host.run_fixture(&path).expect_err("state is active");
        assert_eq!(error.code, codes::FIXTURE_STATE_MISMATCH);
        assert_eq!(host.lifecycle_state(), InstanceState::Active);
    });
}

/// REQ: DEV-514
#[test]
fn dev_514_a_trap_step_followed_by_expect_state_restart_scheduled_passes() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture(
            "f514f",
            r#"{ "schema": 1, "events": [ { "tag": "theme-changed" } ],
                "lifecycle": [ { "step": "trap", "value": "trap.panic" } ],
                "expect_state": "restart-scheduled" }"#,
        );
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.run_fixture(&path).expect("state matches");
    });
}

/// REQ: DEV-514
#[test]
fn dev_514_an_unknown_expect_state_name_is_a_bad_value() {
    with_clean_env(|| {
        let text = load_error_text(r#"{ "schema": 1, "expect_state": "sleeping" }"#, "f514g");
        assert!(
            text.starts_with("fixture.bad-value: $.expect_state"),
            "{text}"
        );
    });
}

// ---- DEV-515, DEV-516 ------------------------------------------------------------------------

/// REQ: DEV-516
#[test]
fn dev_516_a_fixture_without_an_expectation_file_passes_when_nothing_traps() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture(
            "f516a",
            r#"{ "schema": 1, "events": [ { "tag": "theme-changed" } ] }"#,
        );
        assert_eq!(run::<Spy>(&path).expect("runs"), FixtureStatus::Pass);
        assert_eq!(
            spy().batches.len(),
            1,
            "the event was delivered and settled"
        );
    });
}

/// REQ: DEV-516
#[test]
fn dev_516_a_trap_during_the_run_is_reported_with_its_code() {
    with_clean_env(|| {
        spy_reset();
        let path = fixture(
            "f516b",
            r#"{ "schema": 1, "events": [ { "tag": "theme-changed" } ],
                "lifecycle": [ { "step": "trap", "value": "trap.deadline" } ] }"#,
        );
        assert_eq!(
            run::<Spy>(&path).expect("runs"),
            FixtureStatus::Trap("trap.deadline".to_string())
        );
    });
}

/// REQ: DEV-515, DEV-516
#[test]
fn dev_515_only_the_fields_the_item_gives_are_compared() {
    with_clean_env(|| {
        spy_reset();
        let dir = temp_dir("f515a");
        let path = write_file(
            &dir,
            "case.json",
            r#"{ "schema": 1, "events": [ { "tag": "theme-changed" } ] }"#,
        );
        write_file(
            &dir,
            "case.expect.json",
            r#"{ "schema": 1, "logs": { "match": "subset", "items": [ { "level": "info" } ] } }"#,
        );
        assert_eq!(run::<Spy>(&path).expect("runs"), FixtureStatus::Pass);
    });
}

/// REQ: DEV-515, DEV-516
#[test]
fn dev_515_a_given_field_that_differs_fails_the_comparison() {
    with_clean_env(|| {
        spy_reset();
        let dir = temp_dir("f515b");
        let path = write_file(
            &dir,
            "case.json",
            r#"{ "schema": 1, "events": [ { "tag": "theme-changed" } ] }"#,
        );
        write_file(
            &dir,
            "case.expect.json",
            r#"{ "schema": 1, "submissions": { "match": "exact", "items": [ { "intent": "set-decoration", "slot": "work" } ] } }"#,
        );
        assert!(matches!(
            run::<Spy>(&path).expect("runs"),
            FixtureStatus::Fail(_)
        ));
    });
}

/// REQ: DEV-515
#[test]
fn dev_515_exact_requires_the_same_count_and_ordered_allows_gaps() {
    with_clean_env(|| {
        spy_reset();
        let dir = temp_dir("f515c");
        let path = write_file(&dir, "case.json", r#"{ "schema": 1 }"#);
        write_file(
            &dir,
            "case.expect.json",
            r#"{ "schema": 1, "outcomes": { "match": "exact", "items": [ "ok" ] } }"#,
        );
        assert!(
            matches!(run::<Spy>(&path).expect("runs"), FixtureStatus::Fail(_)),
            "no outcomes exist, one expected"
        );
        write_file(
            &dir,
            "case.expect.json",
            r#"{ "schema": 1, "outcomes": { "match": "ordered", "items": [] } }"#,
        );
        assert_eq!(run::<Spy>(&path).expect("runs"), FixtureStatus::Pass);
    });
}

/// REQ: DEV-516
#[test]
fn dev_516_run_fixture_with_a_missing_file_is_an_error() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        let missing = temp_dir("f516c").join("absent.json");
        assert_fails_with(codes::FIXTURE_BAD_VALUE, || {
            host.run_fixture(&missing).map_err(|e| panic!("{}", e.code))
        });
    });
}

/// REQ: DEV-511
#[test]
fn dev_511_a_fixture_with_an_unsupported_schema_number_is_a_bad_value_at_schema() {
    with_clean_env(|| {
        let text = load_error_text(r#"{ "schema": 2 }"#, "f511d");
        assert!(text.starts_with("fixture.bad-value: $.schema"), "{text}");
    });
}
