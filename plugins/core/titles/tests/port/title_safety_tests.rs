//! Port twins of the tests of `title_safety_tests.rs`.

use ilium_plugin_sdk::types::*;

use crate::common::*;

/// REQ: PORT:ilium-server/src/ipc/title_safety_tests.rs::an_unknown_submission_revokes_the_prior_receipt_and_title_observation, PORT:ilium-server/src/ipc/title_safety_tests.rs::empty_history_repair_requires_proof_and_preserves_delivered_tasks_and_manual_names
#[test]
fn port_041_refused_rename_is_not_retried() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 5, "work on something", "Fix login");
    let correlation = host
        .submitted()
        .iter()
        .find(|submission| matches!(submission.intent, Intent::RenameNode(_)))
        .expect("rename submission")
        .correlation;
    host.deliver_one(outcome_event(
        correlation,
        Outcome::Refused(plugin_error(ErrorCode::Conflict)),
    ));
    host.settle();
    assert_eq!(renames(&host.submitted()).len(), 1);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-server/src/ipc/title_safety_tests.rs::all_inference_apply_lanes_preserve_an_unasked_agent_bundle
#[test]
fn port_043_restructure_renames_stay_inferred() {
    let host = organised_host(&plan_reply());
    let spec = restructures(&host.submitted()).remove(0);
    for operation in &spec.operations {
        if let RestructureOp::Rename(rename) = operation {
            assert_eq!(rename.source, TitleSource::Inferred);
        }
    }
}
