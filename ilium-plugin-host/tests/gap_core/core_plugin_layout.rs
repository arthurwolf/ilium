//! Core-plugin gap tests for `core-plugin-layout`: the kernel rejects any layout with a pane under
//! 4 cells wide, a pane under 2 rows high, or a missing or duplicated node, and keeps the previous
//! layout.

use ilium_plugin_host::testing::ops::OpsRig;

const LAYOUT: &str = "ilium.core.layout";

/// REQ: COR-217
#[test]
fn cor_217_a_layout_with_a_pane_narrower_than_four_cells_is_rejected_and_the_previous_layout_kept()
{
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let previous = server.current_layout();
    let proposal = serde_json::json!({
        "panes": [
            {"node": 1, "width": 3, "height": 10},
            {"node": 2, "width": 60, "height": 10}
        ]
    });
    let error = server
        .propose_layout(LAYOUT, &proposal)
        .expect_err("a 3-cell pane must be rejected");
    assert!(format!("{error:?}").contains("layout.invalid"));
    assert_eq!(
        server.current_layout(),
        previous,
        "the previous layout must be kept"
    );
}

/// REQ: COR-217
#[test]
fn cor_217_a_layout_with_a_duplicated_node_is_rejected() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let proposal = serde_json::json!({
        "panes": [
            {"node": 1, "width": 40, "height": 10},
            {"node": 1, "width": 40, "height": 10}
        ]
    });
    assert!(server.propose_layout(LAYOUT, &proposal).is_err());
}
