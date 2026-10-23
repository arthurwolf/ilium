//! Core-plugin gap tests for `core-plugin-agent-detect`: with the plugin off every pane reports
//! agent class `none` and activity `unknown`, and re-enabling reclassifies them within 2 seconds.

use std::time::Duration;

use ilium_plugin_host::testing::ops::OpsRig;

const AGENT_DETECT: &str = "ilium.core.agent-detect";
const DETECTION_INTERVAL: Duration = Duration::from_secs(1);

/// REQ: COR-219
#[test]
fn cor_219_with_agent_detect_off_every_pane_reports_class_none_and_activity_unknown() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let first = server.open_pane("main");
    let second = server.open_pane("side");
    server.disable(AGENT_DETECT);
    rig.advance(DETECTION_INTERVAL);
    for pane in [first, second] {
        assert_eq!(server.pane_class(pane), "none");
        assert_eq!(server.pane_activity(pane), "unknown");
    }
}

/// REQ: COR-219
#[test]
fn cor_219_re_enabling_agent_detect_reclassifies_every_pane_within_two_seconds() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let pane = server.open_pane("main");
    server.disable(AGENT_DETECT);
    server.enable(AGENT_DETECT);
    let started = rig.virtual_elapsed();
    while server.pane_class(pane) == "none" {
        rig.advance(Duration::from_millis(100));
        assert!(
            rig.virtual_elapsed() - started <= Duration::from_secs(2),
            "reclassification took too long"
        );
    }
}
