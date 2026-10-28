//! Port twins of the tests of `naming_title_tmux.rs`.

use ilium_plugin_sdk::types::*;

use crate::common::*;

/// REQ: PORT:ilium/tests/naming_title_tmux.rs::unasked_agent_keeps_title_across_all_restructure_apply_lanes
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

/// REQ: PORT:ilium/tests/naming_title_tmux.rs::unasked_agent_keeps_title_across_all_restructure_apply_lanes
#[test]
fn port_043_manual_title_is_not_renamed_by_organise() {
    let mut host = host_with_two_panes();
    host.deliver_one(Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(1),
        title: "My claude".to_string(),
        source: TitleOrigin::Manual,
    }));
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion(&plan_reply()));
    host.settle();
    let renamed_nodes: Vec<u64> = restructures(&host.submitted())
        .iter()
        .flat_map(|spec| spec.operations.iter())
        .filter_map(|operation| match operation {
            RestructureOp::Rename(rename) => Some(rename.node.0),
            _ => None,
        })
        .collect();
    assert!(!renamed_nodes.contains(&1));
}
