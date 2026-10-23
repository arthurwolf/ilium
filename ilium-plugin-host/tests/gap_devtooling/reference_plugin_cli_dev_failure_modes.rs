//! Failure modes of `ilium plugin dev`: failed rebuilds, degraded file watching and crash loops
//! (DEV-879, DEV-881). Page: reference-plugin-cli-dev-failure-modes.

use serde_json::Value;

use crate::{
    dev_session, dev_session_with_crashing_plugin, dev_session_with_watcher_fault, new_rust_project,
};

/// REQ: DEV-879
#[test]
fn dev_keeps_last_good_instance_when_rebuild_fails() {
    let (_root, env, _dir) = new_rust_project();
    let mut session = dev_session(&env);
    assert!(session.instance_answers(), "the first instance answers");
    session.edit("src/lib.rs", "this is not rust {");
    let after_failure = session.records_since_edit();
    assert!(
        session.instance_answers(),
        "the last good instance still answers after a failed build"
    );
    assert!(
        session.is_alive(),
        "a failed build must not end the session"
    );
    let diagnostics: Vec<&Value> = after_failure
        .iter()
        .filter(|record| record["type"] == "diagnostic")
        .collect();
    assert!(
        !diagnostics.is_empty(),
        "build diagnostics are printed as diagnostic records"
    );
    let reloads: Vec<&Value> = after_failure
        .iter()
        .filter(|record| record["type"] == "reload")
        .collect();
    assert_eq!(
        reloads.len(),
        1,
        "exactly one reload record: {after_failure:?}"
    );
    assert_eq!(reloads[0]["outcome"], "build-failed");
}

/// REQ: DEV-879
#[test]
fn dev_reloads_after_a_later_successful_build() {
    let (_root, env, _dir) = new_rust_project();
    let mut session = dev_session(&env);
    session.edit("src/lib.rs", "this is not rust {");
    let _ = session.records_since_edit();
    session.edit("src/lib.rs", "pub fn fixed() {}\n");
    let after_fix = session.records_since_edit();
    let reload = after_fix
        .iter()
        .find(|record| record["type"] == "reload")
        .expect("a successful build reloads");
    assert_ne!(reload["outcome"], "build-failed", "{reload}");
    assert!(session.instance_answers(), "the reloaded instance answers");
}

/// REQ: DEV-881
#[test]
fn dev_warns_watcher_degraded_and_polls_every_1000_ms_when_inotify_limit_is_reached() {
    let (_root, env, _dir) = new_rust_project();
    let session = dev_session_with_watcher_fault(&env, "inotify-limit");
    let warnings = with_code_in(&session.records(), "dev.watcher-degraded");
    assert_eq!(warnings.len(), 1, "one warning with the watcher code");
    assert_eq!(warnings[0]["severity"], "warn");
    let message = warnings[0]["message"].as_str().expect("warning message");
    assert!(
        message.contains("inotify"),
        "the message names the cause: {message}"
    );
    assert!(
        message.contains("max_user_watches") || message.contains("fix"),
        "names the fix: {message}"
    );
    assert_eq!(
        session.poll_interval_ms(),
        1000,
        "fallback polling interval"
    );
}

/// REQ: DEV-881
#[test]
fn dev_stops_reloading_plugin_after_three_activation_crashes_in_60_seconds() {
    let (_root, env, _dir) = new_rust_project();
    let mut session = dev_session_with_crashing_plugin(&env);
    session.trigger_activation_crashes(3);
    let loops = with_code_in(&session.records(), "dev.crash-loop");
    assert!(
        !loops.is_empty(),
        "dev.crash-loop is emitted after the third crash"
    );
    let attempts_before = session.reload_attempts();
    session.trigger_activation_crashes(2);
    assert_eq!(
        session.reload_attempts(),
        attempts_before,
        "no further reloads until the next source change"
    );
    session.edit("src/lib.rs", "pub fn changed() {}\n");
    assert!(
        session.reload_attempts() > attempts_before,
        "a source change resumes reloading"
    );
}

/// Records of a session whose `code` or `rule` equals `code`.
fn with_code_in(records: &[Value], code: &str) -> Vec<Value> {
    records
        .iter()
        .filter(|record| record["code"] == code || record["rule"] == code)
        .cloned()
        .collect()
}
