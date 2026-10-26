//! reference-plugin-cli.md: `dev` (DEV-180..189, DEV-196). Time is virtual: a `DevEvent` script
//! carries the changes, the plugin calls and the interrupt (see API_GAPS_p2.md).

use std::sync::atomic::Ordering;

use ilium_plugin_host::cli::{DevEvent, MemorySession};

use crate::common::*;
use crate::common_p2::*;

fn change(at_ms: u64) -> DevEvent {
    DevEvent::Change {
        at_ms,
        path: "src/lib.rs".to_owned(),
    }
}

fn interrupt(at_ms: u64) -> DevEvent {
    DevEvent::Interrupt { at_ms }
}

/// REQ: DEV-180
#[test]
fn dev_180_without_a_session_exits_six_and_starts_none() {
    let root = scratch();
    let env = env(root.path());
    let dir = both_project(&env, root.path());
    let output = run_in(&env.with_dev_script(vec![interrupt(1000)]), &dir, &["dev"]);
    assert_eq!(output.exit_code, 6);
    assert_eq!(code(&output).as_deref(), Some("session.unreachable"));
    assert_eq!(recs(&output, "error").len(), 1);
    assert_eq!(types(&output), vec!["error", "summary", "result"]);
}

/// REQ: DEV-180
#[test]
fn dev_180_with_a_session_it_talks_to_it_and_starts_no_second_session() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(&env.with_dev_script(vec![interrupt(1000)]), &dir, &["dev"]);
    assert!(session.connect_attempts() >= 1);
    assert_eq!(session.sessions_started(), 0);
}

/// REQ: DEV-180
#[test]
fn dev_180_flags_do_not_change_the_no_session_exit() {
    let root = scratch();
    let env = env(root.path());
    let dir = both_project(&env, root.path());
    let env = env.with_dev_script(vec![interrupt(1000)]);
    for flags in [
        vec!["--timings"],
        vec!["--release"],
        vec!["--grant-all"],
        vec!["--replace-installed"],
    ] {
        let mut args = vec!["dev"];
        args.extend(flags);
        assert_eq!(run_in(&env, &dir, &args).exit_code, 6);
    }
}

/// REQ: DEV-181
#[test]
fn dev_181_installs_as_t3_with_the_dev_badge() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    let history = session.install_history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, "acme.both");
    assert_eq!(history[0].tier, "T3");
    assert!(history[0].dev);
}

/// REQ: DEV-181
#[test]
fn dev_181_a_later_real_install_starts_with_its_own_data_folder() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.clone().with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    write_dist(&dir, &["server", "client"], "release");
    let packed = run_in(&env, &dir, &["pack"]);
    assert_eq!(packed.exit_code, 0, "{:?}", packed.stdout_jsonl);
    let package = dir.join("dist/acme.both-0.1.0.iliumpkg");
    let installed = run(&env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(installed.exit_code, 0, "{:?}", installed.stdout_jsonl);
    let history = session.install_history();
    assert_eq!(history.len(), 2);
    assert!(history[0].dev && !history[1].dev);
    assert_ne!(history[0].data_dir, history[1].data_dir);
    assert!(!history[1].data_dir.starts_with(&history[0].data_dir));
    assert!(!history[0].data_dir.starts_with(&history[1].data_dir));
}

/// REQ: DEV-182
#[test]
fn dev_182_builds_with_debug_information_by_default() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(read_json(&dir.join("dist/build.json"))["profile"], "debug");
}

/// REQ: DEV-182
#[test]
fn dev_182_release_builds_without_debug_information() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all", "--release"],
    );
    assert_eq!(
        read_json(&dir.join("dist/build.json"))["profile"],
        "release"
    );
}

/// REQ: DEV-183
#[test]
fn dev_183_a_real_copy_of_the_same_id_fails_id_installed() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.both", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(code(&output).as_deref(), Some("dev.id-installed"));
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        session.plugin_state("acme.both").as_deref(),
        Some("running")
    );
    assert!(session.install_history().is_empty());
}

/// REQ: DEV-183
#[test]
fn dev_183_an_earlier_dev_copy_is_not_a_real_copy() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.both", "0.1.0", "T3", "running", true, &[]);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_ne!(code(&output).as_deref(), Some("dev.id-installed"));
}

/// REQ: DEV-183
#[test]
fn dev_183_replace_installed_disables_the_copy_while_dev_runs_and_enables_it_on_exit() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.both", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all", "--replace-installed"],
    );
    assert_eq!(output.exit_code, 130);
    assert_eq!(
        session.state_history("acme.both", false),
        vec!["running", "disabled", "running"]
    );
    assert_eq!(
        session.plugin_state("acme.both").as_deref(),
        Some("running")
    );
}

/// REQ: DEV-184
#[test]
fn dev_184_the_first_install_shows_the_review_dialog() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(&env.with_dev_script(vec![interrupt(1000)]), &dir, &["dev"]);
    assert_eq!(session.review_dialog_count(), 1);
}

/// REQ: DEV-184
#[test]
fn dev_184_a_rebuilt_manifest_with_new_grants_asks_again_for_the_new_grants_only() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let mut manifest = read_json(&dir.join("manifest.json"));
    let old: Vec<String> = manifest["grants"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|g| g["id"].as_str().map(str::to_owned))
        .collect();
    assert!(!old.iter().any(|id| id == EXTRA_LOW_RISK_GRANT));
    let mut grants = manifest["grants"].as_array().cloned().unwrap_or_default();
    grants.push(serde_json::json!({"id": EXTRA_LOW_RISK_GRANT, "reason": "test"}));
    manifest["grants"] = serde_json::Value::Array(grants);
    let script = vec![
        DevEvent::WriteFile {
            at_ms: 1000,
            path: "manifest.json".to_owned(),
            bytes: serde_json::to_vec(&manifest).expect("encode"),
        },
        interrupt(3000),
    ];
    run_in(&env.with_dev_script(script), &dir, &["dev"]);
    let dialogs = session.review_dialog_grants();
    assert_eq!(dialogs.len(), 2);
    assert_eq!(dialogs[1], vec![EXTRA_LOW_RISK_GRANT.to_owned()]);
}

/// REQ: DEV-184
#[test]
fn dev_184_grant_all_grants_every_non_high_risk_grant_without_a_dialog() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(session.review_dialog_count(), 0);
    let history = session.install_history();
    assert!(!history[0].grants.is_empty());
    assert!(history[0]
        .grants
        .iter()
        .all(|grant| !session.is_high_risk(grant)));
}

/// REQ: DEV-184
#[test]
fn dev_184_a_high_risk_grant_always_asks_even_with_grant_all() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let mut manifest = read_json(&dir.join("manifest.json"));
    let mut grants = manifest["grants"].as_array().cloned().unwrap_or_default();
    grants.push(serde_json::json!({"id": HIGH_RISK_GRANT, "reason": "test"}));
    manifest["grants"] = serde_json::Value::Array(grants);
    write_json(&dir.join("manifest.json"), &manifest);
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(session.review_dialog_count(), 1);
    assert_eq!(
        session.review_dialog_grants()[0],
        vec![HIGH_RISK_GRANT.to_owned()]
    );
}

/// REQ: DEV-185
#[test]
fn dev_185_ignored_directories_start_no_rebuild() {
    for path in [
        "dist/both.wasm",
        "target/debug/x",
        "node_modules/pkg/index.js",
        ".ilium/lock",
        ".git/HEAD",
    ] {
        let root = scratch();
        let session = MemorySession::new();
        let builder = FakeBuilder::ok();
        let env = env_session(root.path(), &session).with_builder(builder.clone());
        let dir = both_project(&env, root.path());
        let script = vec![
            DevEvent::Change {
                at_ms: 1000,
                path: path.to_owned(),
            },
            interrupt(5000),
        ];
        run_in(&env.with_dev_script(script), &dir, &["dev", "--grant-all"]);
        assert_eq!(
            builder.builds.load(Ordering::SeqCst),
            builder_builds_of_one_dev_build(),
            "{path} must be ignored"
        );
    }
}

/// The compile-stage runs of the first build alone (sides of the `both` project).
fn builder_builds_of_one_dev_build() -> usize {
    2
}

/// REQ: DEV-185
#[test]
fn dev_185_a_gitignored_path_starts_no_rebuild() {
    let root = scratch();
    let session = MemorySession::new();
    let builder = FakeBuilder::ok();
    let env = env_session(root.path(), &session).with_builder(builder.clone());
    let dir = both_project(&env, root.path());
    std::fs::write(dir.join(".gitignore"), "scratch/\n*.tmp\n").expect("write gitignore");
    let script = vec![
        DevEvent::Change {
            at_ms: 1000,
            path: "scratch/notes.txt".to_owned(),
        },
        DevEvent::Change {
            at_ms: 1100,
            path: "build.tmp".to_owned(),
        },
        interrupt(5000),
    ];
    run_in(&env.with_dev_script(script), &dir, &["dev", "--grant-all"]);
    assert_eq!(
        builder.builds.load(Ordering::SeqCst),
        builder_builds_of_one_dev_build()
    );
}

/// REQ: DEV-185
#[test]
fn dev_185_a_source_change_starts_a_rebuild_300_ms_after_the_last_change() {
    let root = scratch();
    let session = MemorySession::new();
    let early = FakeBuilder::ok();
    let env = env_session(root.path(), &session).with_builder(early.clone());
    let dir = both_project(&env, root.path());
    run_in(
        &env.clone()
            .with_dev_script(vec![change(1000), change(1200), interrupt(1499)]),
        &dir,
        &["dev", "--grant-all"],
    );
    let before = early.builds.load(Ordering::SeqCst);
    assert_eq!(
        before,
        builder_builds_of_one_dev_build(),
        "no rebuild before 1500"
    );
    let late = FakeBuilder::ok();
    run_in(
        &env.with_builder(late.clone()).with_dev_script(vec![
            change(1000),
            change(1200),
            interrupt(1501),
        ]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert!(
        late.builds.load(Ordering::SeqCst) > before,
        "one rebuild at 1500 for both changes"
    );
}

/// REQ: DEV-186
#[test]
fn dev_186_changes_during_a_build_run_exactly_one_more_build() {
    let root = scratch();
    let session = MemorySession::new();
    let builder = FakeBuilder::ok();
    let env = env_session(root.path(), &session)
        .with_builder(builder.clone())
        .with_build_duration_ms(1000);
    let dir = both_project(&env, root.path());
    // Initial build 0..1000. Change at 1200 rebuilds at 1500..2500. Three changes queue meanwhile.
    let script = vec![
        change(1200),
        change(1600),
        change(1800),
        change(2400),
        interrupt(9000),
    ];
    run_in(&env.with_dev_script(script), &dir, &["dev", "--grant-all"]);
    let per_build = builder_builds_of_one_dev_build();
    assert_eq!(
        builder.builds.load(Ordering::SeqCst),
        3 * per_build,
        "initial, the rebuild, exactly one queued build"
    );
}

/// REQ: DEV-186
#[test]
fn dev_186_no_queued_change_means_no_extra_build() {
    let root = scratch();
    let session = MemorySession::new();
    let builder = FakeBuilder::ok();
    let env = env_session(root.path(), &session)
        .with_builder(builder.clone())
        .with_build_duration_ms(1000);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![change(1200), interrupt(9000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(
        builder.builds.load(Ordering::SeqCst),
        2 * builder_builds_of_one_dev_build()
    );
}

/// REQ: DEV-187
#[test]
fn dev_187_reloads_the_server_side_first_then_every_attached_client_side() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    session.attach_clients(2);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all", "--json"],
    );
    let sides: Vec<_> = recs(&output, "reload")
        .iter()
        .map(|record| record["side"].as_str().expect("side").to_owned())
        .collect();
    assert_eq!(sides, vec!["server", "client", "client"]);
}

/// REQ: DEV-187
#[test]
fn dev_187_each_reload_deactivates_the_old_instance_with_reason_upgrade_then_activates_with_restore_bytes(
) {
    let root = scratch();
    let session = MemorySession::new();
    session.set_restore_bytes("acme.both", 321);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    let deactivations = session.deactivations("acme.both");
    assert_eq!(deactivations.first().map(String::as_str), Some("upgrade"));
    assert_eq!(deactivations.last().map(String::as_str), Some("disabled"));
    assert_eq!(
        session
            .activation_restore_bytes("acme.both")
            .last()
            .copied(),
        Some(321)
    );
    let reloads = recs(&output, "reload");
    assert_eq!(reloads[0]["restored_bytes"], 321);
    assert_eq!(reloads[0]["plugin"], "acme.both");
    assert_eq!(reloads[0]["version"], "0.1.0");
}

/// REQ: DEV-187
#[test]
fn dev_187_the_reload_record_has_the_catalogue_fields_in_order() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    let lines = raw_lines(&output, "reload");
    assert!(!lines.is_empty());
    assert!(lines[0].starts_with("{\"type\":\"reload\""));
    assert!(
        keys_in_order(
            &lines[0],
            &[
                "type",
                "plugin",
                "side",
                "version",
                "restored_bytes",
                "restore_dropped",
                "build_ms"
            ]
        ),
        "{}",
        lines[0]
    );
}

/// REQ: DEV-187
#[test]
fn dev_187_a_failed_rebuild_keeps_the_instance_prints_errors_and_sends_no_deactivate() {
    let root = scratch();
    let session = MemorySession::new();
    let builder = FakeBuilder::failing_build(3);
    let env = env_session(root.path(), &session).with_builder(builder);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert!(
        error_messages(&output)
            .iter()
            .any(|message| message.contains("E0308")),
        "compiler errors printed"
    );
    assert!(recs(&output, "reload").is_empty());
    assert_eq!(
        session.deactivations("acme.both"),
        vec!["disabled".to_owned()],
        "only the final deactivation"
    );
    assert_eq!(output.exit_code, 130);
}

/// REQ: DEV-187
#[test]
fn dev_187_a_good_rebuild_after_a_failed_one_reloads_again() {
    let root = scratch();
    let session = MemorySession::new();
    let builder = FakeBuilder::failing_build(3);
    let env = env_session(root.path(), &session).with_builder(builder);
    let dir = both_project(&env, root.path());
    let script = vec![change(1000), change(5000), interrupt(9000)];
    let output = run_in(&env.with_dev_script(script), &dir, &["dev", "--grant-all"]);
    assert!(!recs(&output, "reload").is_empty());
}

/// REQ: DEV-188
#[test]
fn dev_188_restore_bytes_over_one_mib_are_dropped() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_restore_bytes("acme.both", 1_048_577);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    let reloads = recs(&output, "reload");
    assert!(!reloads.is_empty());
    for record in reloads {
        assert_eq!(record["restore_dropped"], true);
        assert_eq!(record["restored_bytes"], 0);
    }
}

/// REQ: DEV-188
#[test]
fn dev_188_exactly_one_mib_is_kept() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_restore_bytes("acme.both", 1_048_576);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    let reloads = recs(&output, "reload");
    assert_eq!(reloads[0]["restore_dropped"], false);
    assert_eq!(reloads[0]["restored_bytes"], 1_048_576);
}

/// REQ: DEV-188
#[test]
fn dev_188_one_byte_under_one_mib_is_kept() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_restore_bytes("acme.both", 1_048_575);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(recs(&output, "reload")[0]["restored_bytes"], 1_048_575);
    assert_eq!(
        session
            .activation_restore_bytes("acme.both")
            .last()
            .copied(),
        Some(1_048_575)
    );
}

/// REQ: DEV-188
#[test]
fn dev_188_dropped_bytes_are_not_handed_to_the_new_instance() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_restore_bytes("acme.both", 2_000_000);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![change(1000), interrupt(3000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(
        session
            .activation_restore_bytes("acme.both")
            .last()
            .copied(),
        Some(0)
    );
}

/// REQ: DEV-189
#[test]
fn dev_189_ctrl_c_deactivates_with_disabled_uninstalls_prints_summary_and_result_and_exits_130() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert_eq!(output.exit_code, 130);
    let kinds = types(&output);
    assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"]);
    assert_eq!(result(&output)["ok"], false);
    assert_eq!(result(&output)["exit_code"], 130);
    assert_eq!(
        session
            .deactivations("acme.both")
            .last()
            .map(String::as_str),
        Some("disabled")
    );
    assert!(!session.package_installed("acme.both"));
}

/// REQ: DEV-189
#[test]
fn dev_189_the_data_folder_is_kept_by_default() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    session.seed_plugin_data("acme.both");
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all"],
    );
    assert!(session.data_folder_exists("acme.both"));
}

/// REQ: DEV-189
#[test]
fn dev_189_purge_on_exit_deletes_the_data_folder() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    session.seed_plugin_data("acme.both");
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all", "--purge-on-exit"],
    );
    assert!(!session.data_folder_exists("acme.both"));
}

/// REQ: DEV-189
#[test]
fn dev_189_replaced_installed_plugin_is_restored_on_exit() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.both", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all", "--replace-installed"],
    );
    assert_eq!(
        session.plugin_state("acme.both").as_deref(),
        Some("running")
    );
}

/// REQ: DEV-189
#[test]
fn dev_189_a_plugin_that_was_already_disabled_stays_disabled_after_replace_installed() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.both", "0.1.0", "T3", "disabled", false, &[]);
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    run_in(
        &env.with_dev_script(vec![interrupt(1000)]),
        &dir,
        &["dev", "--grant-all", "--replace-installed"],
    );
    assert_eq!(
        session.plugin_state("acme.both").as_deref(),
        Some("disabled")
    );
}

fn timing_script(interrupt_at: u64) -> Vec<DevEvent> {
    vec![
        DevEvent::Calls {
            at_ms: 1000,
            side: "server".to_owned(),
            export: "on-events".to_owned(),
            duration_ms: vec![2, 4, 9],
        },
        interrupt(interrupt_at),
    ]
}

/// REQ: DEV-196
#[test]
fn dev_196_timings_prints_a_timing_record_every_ten_seconds() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(timing_script(10_500)),
        &dir,
        &["dev", "--grant-all", "--timings"],
    );
    let timings = recs(&output, "timing");
    let record = timings
        .iter()
        .find(|record| record["export"] == "on-events")
        .expect("timing record");
    assert_eq!(record["plugin"], "acme.both");
    assert_eq!(record["side"], "server");
    assert_eq!(record["calls"], 3);
    assert_eq!(record["max_ms"], 9);
    assert_eq!(record["deadline_ms"], 20);
}

/// REQ: DEV-196
#[test]
fn dev_196_no_record_before_the_first_ten_seconds_have_passed() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(timing_script(9_999)),
        &dir,
        &["dev", "--grant-all", "--timings"],
    );
    assert!(recs(&output, "timing")
        .iter()
        .all(|record| record["calls"] == 0));
}

/// REQ: DEV-196
#[test]
fn dev_196_without_the_flag_no_timing_record_is_printed() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(timing_script(25_000)),
        &dir,
        &["dev", "--grant-all"],
    );
    assert!(recs(&output, "timing").is_empty());
}

/// REQ: DEV-196
#[test]
fn dev_196_the_timing_record_has_the_catalogue_fields_in_order() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = both_project(&env, root.path());
    let output = run_in(
        &env.with_dev_script(timing_script(10_500)),
        &dir,
        &["dev", "--grant-all", "--timings"],
    );
    let lines = raw_lines(&output, "timing");
    assert!(!lines.is_empty());
    assert!(
        keys_in_order(
            &lines[0],
            &[
                "type",
                "plugin",
                "side",
                "export",
                "calls",
                "p50_ms",
                "p99_ms",
                "max_ms",
                "deadline_ms"
            ]
        ),
        "{}",
        lines[0]
    );
}
