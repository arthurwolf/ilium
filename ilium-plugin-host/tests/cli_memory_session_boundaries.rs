//! Deterministic session adapter effects, without package admission or Wasm execution.
use ilium_plugin_host::cli::{InstallRecord, MemorySession, ProfileCall, SessionBackend, SessionRequest, SessionResponse};

fn seeded() -> std::sync::Arc<MemorySession> {
    let session = MemorySession::new();
    session.add_plugin("example.notes", "1.0.0", "T2", "disabled", false,
        &[("tree.read", "low", "granted"), ("process.spawn", "high", "granted")]);
    session
}
#[test]
fn connection_and_injected_failures_are_observed_without_starting_sessions() {
    let session = MemorySession::unreachable();
    for _ in 0..2 { assert_eq!(session.request(SessionRequest::Connect).unwrap_err(), "session.unreachable"); }
    assert_eq!(session.connect_attempts(), 2);
    assert_eq!(session.sessions_started(), 0);
    let reachable = MemorySession::new();
    reachable.inject_error("connect", "injected transport error");
    for _ in 0..2 { assert_eq!(reachable.connect().unwrap_err(), "injected transport error"); }
    reachable.clear_error("connect");
    reachable.connect().unwrap();
    assert_eq!(reachable.connect_attempts(), 3);
    assert_eq!(reachable.sessions_started(), 0);
}
#[test]
fn lifecycle_needs_an_explicit_runtime_result_and_records_real_fixture_effects() {
    let session = seeded();
    let request = SessionRequest::Activate { id: "example.notes".into() };
    assert_eq!(session.request(request.clone()).unwrap_err(), "session.runtime-required");
    assert_eq!(session.activations("example.notes"), 0);
    session.set_restore_bytes("example.notes", 987);
    session.set_runtime_outcome("example.notes", "activate", Ok(()));
    assert_eq!(session.request(request).unwrap(), SessionResponse::Done);
    assert_eq!(session.plugin_state("example.notes").as_deref(), Some("running"));
    assert_eq!(session.activations("example.notes"), 1);
    assert_eq!(session.activation_restore_bytes("example.notes"), vec![987]);
    assert_eq!(session.state_history("example.notes", true), vec!["disabled", "running"]);
    assert_eq!(session.state_history("example.notes", false), vec!["running"]);
    assert_eq!(session.started_plugin_count(), 1);
    assert!(session.calls().iter().any(|call| call.starts_with("request:Activate")));
}
#[test]
fn grant_revocation_is_scoped_and_failed_mutations_do_not_change_state() {
    let session = seeded();
    session.add_plugin("example.other", "2.0.0", "T3", "running", true, &[("tree.read", "low", "granted")]);
    session.add_contribution("example.notes", "view", "tree.read");
    session.add_contribution("example.notes", "command", "process.spawn");
    session.inject_error("revoke", "write failed");
    assert_eq!(session.revoke_grant("example.notes", "tree.read").unwrap_err(), "write failed");
    assert_eq!(session.contribution_count("example.notes"), 2);
    assert_eq!(session.granted("example.notes"), vec!["tree.read", "process.spawn"]);
    session.clear_error("revoke");
    session.revoke_grant("example.notes", "tree.read").unwrap();
    assert_eq!(session.grant_state("example.notes", "tree.read").as_deref(), Some("revoked"));
    assert_eq!(session.contribution_count("example.notes"), 1);
    assert_eq!(session.granted("example.other"), vec!["tree.read"]);
    assert!(session.is_high_risk("process.spawn"));
}
#[test]
fn uninstall_preserves_data_and_logs_until_explicit_purge() {
    let session = seeded();
    session.seed_plugin_data("example.notes");
    session.add_log_line("example.notes", "server", "warn", "retained diagnostic");
    session.record_uninstall("example.notes", false, false).unwrap();
    assert!(!session.package_installed("example.notes"));
    assert!(session.plugin("example.notes").is_none());
    assert!(session.data_folder_exists("example.notes"));
    assert!(session.cache_folder_exists("example.notes"));
    assert_eq!(session.storage_bucket_count("example.notes"), 1);
    assert_eq!(session.settings_value_count("example.notes"), 1);
    assert!(session.log_kept("example.notes"));
    session.record_install(InstallRecord { id:"example.notes".into(), version:"2.0.0".into(), tier:"T2".into(), dev:false, data_dir:"fixture/data".into(), grants:vec![] }, "disabled").unwrap();
    session.record_uninstall("example.notes", true, true).unwrap();
    assert!(!session.data_folder_exists("example.notes"));
    assert_eq!(session.storage_bucket_count("example.notes"), 0);
    assert!(!session.log_kept("example.notes"));
    assert_eq!(session.install_history().len(), 1);
}
#[test]
fn attached_review_uses_injected_decisions_and_exact_grants() {
    let session = seeded();
    let grants = vec!["process.spawn".to_owned(), "tree.read".to_owned()];
    assert!(session.review(&grants).is_err());
    assert_eq!(session.review_dialog_count(), 0);
    session.attach_clients(3);
    session.decline_review(true);
    assert!(!session.review(&grants).unwrap());
    assert_eq!(session.review_dialog_count(), 1);
    assert_eq!(session.review_dialog_grants(), vec![grants.clone()]);
    assert_eq!(session.granted("example.notes"), vec!["tree.read", "process.spawn"]);
    session.decline_review(false);
    assert!(session.review(&grants).unwrap());
    session.decline_trust(true);
    assert!(!session.trust_prompt().unwrap());
}
#[test]
fn debug_reads_return_injected_data_without_inventing_execution() {
    let session = seeded();
    let profile = vec![ProfileCall { export:"on-events".into(), duration_ms:3.75 }];
    session.set_profile("example.notes", vec!["on-events".into()], profile.clone());
    assert_eq!(session.profile_exports("example.notes"), vec!["on-events"]);
    assert_eq!(session.profile_calls("example.notes", 17), profile);
    assert!(session.calls().contains(&"profile-calls:example.notes:17".into()));
    assert!(session.trace_records("missing").is_empty());
    assert!(session.memory_views("missing").is_empty());
    assert!(session.counters("missing").is_empty());
    assert_eq!(session.activations("example.notes"), 0);
    assert_eq!(session.log_level("example.notes"), "info");
    session.set_default_log_level("debug");
    session.set_log_level("example.notes", "error").unwrap();
    assert_eq!(session.log_level("example.notes"), "error");
    assert_eq!(session.log_level("missing"), "debug");
}
#[test]
fn trace_tree_memory_and_counter_side_data_are_returned_as_independent_snapshots() {
    use ilium_plugin_api::Side;
    use ilium_plugin_host::telemetry::{InspectNode, MemoryView, TraceKind, TraceRecord};
    let session = seeded();
    let trace = TraceRecord { seq:9, time_ms:456, kind:TraceKind::Lifecycle, instance:3, detail:serde_json::json!({"state":"disabled"}) };
    session.set_trace_records("example.notes", vec![trace.clone()]);
    let tree = InspectNode { kind:"text".into(), key:Some("body".into()), text:Some("actual fixture content".into()), style:None, selected:false, children:vec![] };
    session.set_last_tree("example.notes", "main", tree.clone());
    let memory = MemoryView { side:Side::Server, linear_bytes:65536, peak_bytes:131072, limit_bytes:262144, storage_bytes:71, instances:3, restarts:2 };
    session.set_memory_views("example.notes", vec![memory.clone()]);
    session.set_counters("example.notes", vec![(Side::Server, vec![]), (Side::Client, vec![])]);
    let mut observed = session.trace_records("example.notes");
    assert_eq!(observed, vec![trace.clone()]);
    observed.clear();
    assert_eq!(session.trace_records("example.notes"), vec![trace]);
    assert_eq!(session.last_tree("example.notes", "main"), Some(tree));
    assert!(session.last_tree("example.notes", "missing").is_none());
    assert_eq!(session.memory_views("example.notes"), vec![memory]);
    assert_eq!(session.counters("example.notes"), vec![(Side::Server, vec![]), (Side::Client, vec![])]);
}
