//! reference-test-harness.md "The fakes": DEV-450 to DEV-473.
//!
//! Rows that need a plugin to call a host import (timer firing, fs scope checks, http and process
//! lookups, storage puts, jobs) cannot be written until `HostApi` exposes the imports; see
//! API_GAPS.md. DEV-452 is the TypeScript `advanceClock`
//! argument check and is skipped in the lane report.

use ilium_plugin_sdk_core::host::{Bucket, ProcessExit};
use ilium_plugin_sdk_core::types::{AgentActivity, ErrorCode, Event, FsPath, NodeClass, NodeId};
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::fakes::fs::parse_shorthand;
use ilium_plugin_testkit::{FakeHost, TreeNode};

use crate::common::{
    assert_fails_with, manifest_text, panic_text, spy, spy_reset, temp_dir, with_clean_env,
    write_file, Spy, DEFAULT_CLOCK_MS,
};

fn active_host_with(grants: &[&str]) -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new().grants(grants);
    host.activate::<Spy>();
    host
}

fn all_events() -> Vec<Event> {
    spy().batches.into_iter().flatten().collect()
}

fn node(name: &str, parent: Option<u64>) -> TreeNode {
    TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    }
}

// ---- DEV-450, DEV-451, clock ----------------------------------------------------------------

/// REQ: DEV-450
#[test]
fn dev_450_advance_clock_with_no_timer_set_delivers_nothing_and_moves_time() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.advance_clock(60_000);
        assert!(spy().batches.is_empty());
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 60_000);
    });
}

/// REQ: DEV-450
#[test]
fn dev_450_advance_clock_by_zero_changes_nothing() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.advance_clock(0);
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS);
        assert!(spy().batches.is_empty());
    });
}

/// REQ: DEV-451
#[test]
fn dev_451_the_clock_has_no_daylight_saving_over_200_days() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        let day = 86_400_000u64;
        for _ in 0..200 {
            host.advance_clock(day);
        }
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 200 * day);
        assert_eq!(
            host.clock().now() % day,
            0,
            "every day starts at 00:00:00 UTC"
        );
    });
}

/// REQ: DEV-451
#[test]
fn dev_451_the_default_instant_is_midnight_utc() {
    with_clean_env(|| {
        let host = active_host_with(&[]);
        assert_eq!(host.clock().now() % 86_400_000, 0);
        assert_eq!(
            host.clock().now() / 86_400_000,
            21_915,
            "2030-01-01 is day 21915 after 1970-01-01"
        );
    });
}

/// REQ: DEV-451
#[test]
fn dev_451_set_clock_forward_moves_time_and_backward_fails_with_clock_backwards() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.set_clock(DEFAULT_CLOCK_MS + 5_000);
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 5_000);
        assert_fails_with(codes::CLOCK_BACKWARDS, || {
            host.set_clock(DEFAULT_CLOCK_MS + 4_999)
        });
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 5_000);
    });
}

// ---- DEV-456, DEV-457, DEV-458 --------------------------------------------------------------

/// REQ: DEV-456
#[test]
fn dev_456_a_file_written_by_the_test_reads_back() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.fs().write("project:notes.md", b"hello");
        assert_eq!(host.fs().read("project:notes.md"), Some(b"hello".to_vec()));
    });
}

/// REQ: DEV-456
#[test]
fn dev_456_a_missing_file_reads_as_none() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        assert_eq!(host.fs().read("project:absent.md"), None);
    });
}

/// REQ: DEV-456
#[test]
fn dev_456_a_second_write_replaces_the_first() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.fs().write("project:a.txt", b"one");
        host.fs().write("project:a.txt", b"two");
        assert_eq!(host.fs().read("project:a.txt"), Some(b"two".to_vec()));
    });
}

/// REQ: DEV-456
#[test]
fn dev_456_the_same_relative_path_in_two_scopes_holds_two_files() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.fs().write("project:a.txt", b"project");
        host.fs().write("session:a.txt", b"session");
        assert_eq!(host.fs().read("project:a.txt"), Some(b"project".to_vec()));
        assert_eq!(host.fs().read("session:a.txt"), Some(b"session".to_vec()));
    });
}

/// REQ: DEV-456
#[test]
fn dev_456_the_shorthand_and_the_record_form_name_the_same_file() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.fs().write("project:dir/b.txt", b"x");
        let record = FsPath {
            scope: "project".to_string(),
            path: "dir/b.txt".to_string(),
        };
        assert_eq!(host.fs().read(record), Some(b"x".to_vec()));
    });
}

/// REQ: DEV-457
#[test]
fn dev_457_the_fake_file_system_creates_no_file_on_the_real_disk() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        let name = "sdk_fakehost_457_marker.txt";
        host.fs()
            .write(format!("project:{name}").as_str(), b"memory only");
        assert_eq!(
            host.fs().read(format!("project:{name}").as_str()),
            Some(b"memory only".to_vec())
        );
        let candidates = [
            std::path::PathBuf::from(name),
            std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name),
            std::env::temp_dir().join(name),
        ];
        for path in candidates {
            assert!(!path.exists(), "{} must not exist", path.display());
        }
    });
}

/// REQ: DEV-457
#[test]
fn dev_457_a_file_written_by_one_host_is_not_seen_by_a_new_host() {
    with_clean_env(|| {
        let mut first_host = active_host_with(&[]);
        first_host.fs().write("project:in_memory_457.txt", b"memory only");
        let mut second_host = active_host_with(&[]);
        assert_eq!(second_host.fs().read("project:in_memory_457.txt"), None);
    });
}

/// REQ: DEV-458
#[test]
fn dev_458_an_absolute_path_fails_with_fs_path_invalid() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        assert_fails_with(codes::FS_PATH_INVALID, || {
            host.fs().write("project:/etc/passwd", b"x")
        });
    });
}

/// REQ: DEV-458
#[test]
fn dev_458_a_path_with_a_parent_segment_fails_with_fs_path_invalid() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        assert_fails_with(codes::FS_PATH_INVALID, || {
            host.fs().write("project:../escape.txt", b"x")
        });
        assert_fails_with(codes::FS_PATH_INVALID, || {
            host.fs().write("project:a/../b.txt", b"x")
        });
    });
}

/// REQ: DEV-458
#[test]
fn dev_458_read_of_an_invalid_path_also_fails_with_fs_path_invalid() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        assert_fails_with(codes::FS_PATH_INVALID, || host.fs().read("project:../x"));
    });
}

/// REQ: DEV-458
#[test]
fn dev_458_parse_shorthand_splits_scope_and_path() {
    let parsed = parse_shorthand("project:dir/file.txt").expect("valid shorthand");
    assert_eq!(parsed.scope, "project");
    assert_eq!(parsed.path, "dir/file.txt");
    let error = parse_shorthand("project:../x").expect_err("parent segment");
    assert_eq!(error.code, codes::FS_PATH_INVALID);
    let error = parse_shorthand("project:/abs").expect_err("absolute path");
    assert_eq!(error.code, codes::FS_PATH_INVALID);
}

// ---- DEV-460, DEV-462, DEV-463, DEV-464 (registration side) ----------------------------------

/// REQ: DEV-460
#[test]
fn dev_460_a_registered_process_expectation_starts_no_run() {
    with_clean_env(|| {
        let mut host = active_host_with(&["process.run:git"]);
        host.process()
            .expect("git", &["status"])
            .returns(ProcessExit {
                status: Some(0),
                signal: None,
                stdout: b"clean".to_vec(),
                stderr: Vec::new(),
            });
        assert!(host.process().calls().is_empty());
    });
}

/// REQ: DEV-460
#[test]
fn dev_460_process_and_processes_are_the_same_fake() {
    with_clean_env(|| {
        let mut host = active_host_with(&["process.run:git"]);
        host.process().allow_unexpected();
        assert!(host.processes().calls().is_empty());
    });
}

/// REQ: DEV-462
#[test]
fn dev_462_a_registered_http_entry_makes_no_network_call() {
    with_clean_env(|| {
        let mut host = active_host_with(&["http:api.example.com"]);
        host.http().expect(
            "GET",
            "https://api.example.com/v1/items",
            ilium_plugin_sdk_core::host::HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: b"[]".to_vec(),
            },
        );
        assert!(host.http().calls().is_empty());
    });
}

/// REQ: DEV-464
#[test]
fn dev_464_a_process_expectation_that_returns_a_failure_registers_without_a_call() {
    with_clean_env(|| {
        let mut host = active_host_with(&["process.run:git"]);
        host.process().expect("git", &["push"]).fails(
            ilium_plugin_sdk_core::types::PluginError::new(ErrorCode::Io, "disk full"),
        );
        assert!(host.process().calls().is_empty());
    });
}

// ---- DEV-465, DEV-466 (snapshot side) --------------------------------------------------------

fn snapshot_file(label: &str, body: &str) -> std::path::PathBuf {
    write_file(&temp_dir(label), "storage.json", body)
}

/// REQ: DEV-465
#[test]
fn dev_465_a_storage_snapshot_value_is_readable_after_activate() {
    with_clean_env(|| {
        spy_reset();
        let path = snapshot_file("s465", r#"{ "global": { "k": "AQID" } }"#);
        let mut host = FakeHost::new().with_storage_snapshot(&path);
        host.activate::<Spy>();
        assert_eq!(host.storage().get(Bucket::Global, "k"), Some(vec![1, 2, 3]));
    });
}

/// REQ: DEV-465
#[test]
fn dev_465_values_survive_deactivate_then_activate_on_the_same_host() {
    with_clean_env(|| {
        spy_reset();
        let path = snapshot_file("s465b", r#"{ "project": { "kept": "AQ==" } }"#);
        let mut host = FakeHost::new().with_storage_snapshot(&path);
        host.activate::<Spy>();
        host.deactivate(ilium_plugin_sdk_core::types::DeactivationReason::Shutdown);
        host.activate::<Spy>();
        assert_eq!(host.storage().get(Bucket::Project, "kept"), Some(vec![1]));
        assert_eq!(host.snapshot().storage["project"]["kept"], vec![1]);
    });
}

/// REQ: DEV-466
#[test]
fn dev_466_dump_returns_one_bucket_as_a_map() {
    with_clean_env(|| {
        spy_reset();
        let path = snapshot_file(
            "s466",
            r#"{ "global": { "a": "AQ==", "b": "Ag==" }, "session": { "c": "Aw==" } }"#,
        );
        let mut host = FakeHost::new().with_storage_snapshot(&path);
        host.activate::<Spy>();
        let global = host.storage().dump(Bucket::Global);
        assert_eq!(global.len(), 2);
        assert_eq!(global["a"], vec![1]);
        assert_eq!(global["b"], vec![2]);
        assert_eq!(host.storage().dump(Bucket::Session).len(), 1);
        assert!(host.storage().dump(Bucket::Shared).is_empty());
    });
}

/// REQ: DEV-466
#[test]
fn dev_466_an_empty_bucket_dumps_as_an_empty_map() {
    with_clean_env(|| {
        let host = active_host_with(&[]);
        assert!(host.storage().dump(Bucket::Global).is_empty());
        assert_eq!(host.storage().get(Bucket::Global, "missing"), None);
    });
}

// ---- DEV-467 ---------------------------------------------------------------------------------

fn settings_manifest(label: &str) -> std::path::PathBuf {
    let dir = temp_dir(label);
    let base = manifest_text("example.demo", &[], &[]);
    let with_settings = base.replacen(
        "\n}\n",
        r#",
  "contributes": {
    "settings": [
      { "kind": "tab", "id": "demo", "title": "Demo", "sections": [
        { "id": "badges", "title": "Badges", "scope": "client", "rows": [
          { "type": "toggle", "key": "show_badges", "title": "Show badges", "default": true }
        ] }
      ] }
    ]
  }
}
"#,
        1,
    );
    write_file(&dir, "ilium-plugin.json", &with_settings)
}

/// REQ: DEV-467
#[test]
fn dev_467_a_valid_value_is_stored_and_settings_changed_reaches_the_plugin() {
    with_clean_env(|| {
        spy_reset();
        let path = settings_manifest("set467a");
        let mut host = FakeHost::for_manifest(&path)
            .expect("manifest loads")
            .side("client");
        host.activate::<Spy>();
        host.settings()
            .set("show_badges", serde_json::json!(false))
            .expect("valid toggle value");
        assert_eq!(
            host.settings().get("show_badges"),
            Some(serde_json::json!(false))
        );
        host.settle();
        let changed: Vec<_> = all_events()
            .into_iter()
            .filter_map(|event| match event {
                Event::SettingsChanged(change) => Some(change.keys),
                _ => None,
            })
            .collect();
        assert_eq!(changed, vec![vec!["show_badges".to_string()]]);
    });
}

/// REQ: DEV-467
#[test]
fn dev_467_a_value_of_the_wrong_type_returns_invalid_argument_and_changes_nothing() {
    with_clean_env(|| {
        spy_reset();
        let path = settings_manifest("set467b");
        let mut host = FakeHost::for_manifest(&path)
            .expect("manifest loads")
            .side("client");
        host.activate::<Spy>();
        let error = host
            .settings()
            .set("show_badges", serde_json::json!("not a boolean"))
            .expect_err("a toggle takes a boolean");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        host.settle();
        assert!(all_events()
            .iter()
            .all(|event| !matches!(event, Event::SettingsChanged(_))));
    });
}

/// REQ: DEV-467
#[test]
fn dev_467_the_default_of_a_row_reads_back_before_any_set() {
    with_clean_env(|| {
        spy_reset();
        let path = settings_manifest("set467c");
        let mut host = FakeHost::for_manifest(&path)
            .expect("manifest loads")
            .side("client");
        host.activate::<Spy>();
        assert_eq!(
            host.settings().get("show_badges"),
            Some(serde_json::json!(true))
        );
    });
}

// ---- DEV-470, DEV-471 ------------------------------------------------------------------------

/// REQ: DEV-470, DEV-471
#[test]
fn dev_470_add_returns_ids_in_increasing_order_from_one() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read"]);
        let first = host.tree().add(node("a", None));
        let second = host.tree().add(node("b", Some(first)));
        let third = host.tree().add(node("c", Some(first)));
        assert_eq!((first, second, third), (1, 2, 3));
    });
}

/// REQ: DEV-470
#[test]
fn dev_470_get_returns_the_stored_node_with_its_assigned_id() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read"]);
        let id = host.tree().add(node("shell", None));
        let stored = host.tree().get(id).expect("node exists");
        assert_eq!(stored.id, id);
        assert_eq!(stored.name, "shell");
        assert_eq!(stored.class, NodeClass::Pane);
        assert_eq!(host.tree().get(99), None);
    });
}

/// REQ: DEV-470
#[test]
fn dev_470_add_queues_tree_changed_for_the_next_settle() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read"]);
        let id = host.tree().add(node("shell", None));
        host.settle();
        let seen = all_events().iter().any(|event| match event {
            Event::TreeChanged(ids) => ids.contains(&NodeId(id)),
            _ => false,
        });
        assert!(seen, "tree-changed with node {id} must reach the plugin");
    });
}

/// REQ: DEV-470
#[test]
fn dev_470_rename_changes_the_name_and_queues_tree_changed() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read"]);
        let id = host.tree().add(node("old", None));
        host.settle();
        host.tree().rename(id, "new");
        assert_eq!(host.tree().get(id).expect("node").name, "new");
        host.settle();
        let batches = spy().batches;
        assert!(
            batches.len() >= 2,
            "one batch per settle with queued events"
        );
        assert!(matches!(
            batches.last().expect("batch")[0],
            Event::TreeChanged(_)
        ));
    });
}

/// REQ: DEV-470
#[test]
fn dev_470_set_activity_delivers_agent_activity_changed_with_the_next_revision() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read", "agent.read"]);
        let id = host.tree().add(node("agent", None));
        host.tree().set_activity(id, AgentActivity::Working);
        host.tree().set_activity(id, AgentActivity::Idle);
        host.settle();
        let revisions: Vec<(AgentActivity, u64)> = all_events()
            .into_iter()
            .filter_map(|event| match event {
                Event::AgentActivityChanged(payload) => Some((payload.current, payload.revision)),
                _ => None,
            })
            .collect();
        assert_eq!(
            revisions,
            vec![(AgentActivity::Working, 1), (AgentActivity::Idle, 2)]
        );
    });
}

/// REQ: DEV-470
#[test]
fn dev_470_remove_deletes_the_node_and_its_children_and_delivers_node_removed() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read"]);
        let parent = host.tree().add(node("parent", None));
        let child = host.tree().add(node("child", Some(parent)));
        host.tree().remove(parent);
        assert_eq!(host.tree().get(parent), None);
        assert_eq!(host.tree().get(child), None);
        host.settle();
        let removed = all_events()
            .iter()
            .any(|event| matches!(event, Event::NodeRemoved(id) if *id == NodeId(parent)));
        assert!(removed);
    });
}

/// REQ: DEV-471
#[test]
fn dev_471_ids_continue_after_a_removal_and_are_never_reused() {
    with_clean_env(|| {
        let mut host = active_host_with(&["tree.read"]);
        let first = host.tree().add(node("a", None));
        host.tree().remove(first);
        let second = host.tree().add(node("b", None));
        assert_eq!(first, 1);
        assert_eq!(second, 2);
    });
}

// ---- DEV-472, DEV-473 ------------------------------------------------------------------------

/// REQ: DEV-472
#[test]
fn dev_472_a_plugin_that_started_no_job_leaves_the_job_list_empty() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        assert!(host.jobs().list().is_empty());
    });
}

/// REQ: DEV-473
#[test]
fn dev_473_invoke_splits_the_command_name_from_its_arguments() {
    with_clean_env(|| {
        let mut host = active_host_with(&["cli.contribute"]);
        let invocation = host.cli().invoke(&["notes", "list", "--json"], b"");
        assert_eq!(invocation.name, "notes");
        assert_eq!(
            invocation.arguments,
            vec!["list".to_string(), "--json".to_string()]
        );
    });
}

/// REQ: DEV-473
#[test]
fn dev_473_invoke_delivers_cli_invoked_with_the_name_and_argv() {
    with_clean_env(|| {
        let mut host = active_host_with(&["cli.contribute"]);
        host.cli().invoke(&["notes", "list"], b"");
        let seen: Vec<_> = all_events()
            .into_iter()
            .filter_map(|event| match event {
                Event::CliInvoked(payload) => Some((payload.name, payload.argv)),
                _ => None,
            })
            .collect();
        assert_eq!(seen, vec![("notes".to_string(), vec!["list".to_string()])]);
    });
}

/// REQ: DEV-473
#[test]
fn dev_473_without_the_cli_contribute_grant_no_event_reaches_the_plugin() {
    with_clean_env(|| {
        let mut host = active_host_with(&[]);
        host.cli().invoke(&["notes"], b"");
        assert!(all_events()
            .iter()
            .all(|event| !matches!(event, Event::CliInvoked(_))));
        assert_eq!(panic_text(|| host.snapshot().instance), None);
    });
}
