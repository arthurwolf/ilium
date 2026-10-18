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
