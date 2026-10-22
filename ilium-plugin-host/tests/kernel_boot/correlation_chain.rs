//! reference-health-and-watchdog.md row HST-526: one correlation id per originating input,
//! carried through every dispatcher call, plugin log entry and the diagnostics bundle.

use ilium_plugin_host::correlation_chain::run_input_chain;

/// REQ: HST-526
#[test]
fn hst_526_every_log_entry_of_the_chain_carries_the_chain_correlation_id() {
    let trace = run_input_chain("key-press", &["acme.notes", "acme.sync"]);
    assert!(!trace.correlation_id.is_empty());
    assert!(trace.log_entries.iter().any(|entry| entry.writer == "acme.notes"));
    assert!(trace.log_entries.iter().any(|entry| entry.writer == "acme.sync"));
    for entry in &trace.log_entries {
        assert_eq!(entry.correlation_id, trace.correlation_id);
    }
}

/// REQ: HST-526
#[test]
fn hst_526_every_dispatcher_call_passes_the_correlation_id_unchanged() {
    let trace = run_input_chain("key-press", &["acme.notes", "acme.sync"]);
    assert!(!trace.dispatcher_correlation_ids.is_empty());
    for dispatcher_id in &trace.dispatcher_correlation_ids {
        assert_eq!(dispatcher_id, &trace.correlation_id);
    }
}

/// REQ: HST-526
#[test]
fn hst_526_the_diagnostics_bundle_carries_the_same_correlation_id() {
    let trace = run_input_chain("key-press", &["acme.notes", "acme.sync"]);
    assert!(!trace.diagnostics_bundle_correlation_ids.is_empty());
    for bundle_id in &trace.diagnostics_bundle_correlation_ids {
        assert_eq!(bundle_id, &trace.correlation_id);
    }
}

/// REQ: HST-526
#[test]
fn hst_526_two_originating_inputs_get_different_correlation_ids() {
    let first = run_input_chain("key-press", &["acme.notes", "acme.sync"]);
    let second = run_input_chain("key-press", &["acme.notes", "acme.sync"]);
    assert_ne!(first.correlation_id, second.correlation_id);
}
