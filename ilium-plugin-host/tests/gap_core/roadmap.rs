//! Core-plugin gap tests for `roadmap`: every wave ships a kill switch, and a failing gate on a
//! released wave blocks or rolls back the release and is recorded.

use ilium_plugin_host::testing::ops::OpsRig;

const WAVE_ONE: &str = "wave-1";

fn manual_page(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// REQ: COR-103
#[test]
fn cor_103_the_wave_kill_switch_serves_the_native_path_without_changing_stored_data() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let data_before = rig.storage_fingerprint();
    rig.set_kill_switch(WAVE_ONE, true);
    assert_eq!(
        server.features_served_by(WAVE_ONE),
        "native",
        "the native path must serve"
    );
    assert_eq!(
        rig.storage_fingerprint(),
        data_before,
        "stored data must be unchanged"
    );
}

/// REQ: COR-103
#[test]
fn cor_103_the_wave_kill_switch_is_documented_in_the_roadmap_release_notes() {
    let page = manual_page("roadmap");
    assert!(
        page.to_lowercase().contains("kill switch"),
        "the release notes must document the kill switch"
    );
}

/// REQ: COR-104
#[test]
fn cor_104_a_failing_gate_on_a_candidate_stops_the_release_and_records_gate_wave_and_commit() {
    let rig = OpsRig::new();
    let output = rig.run_release_gate(WAVE_ONE, "gate-failing-fixture");
    assert_ne!(
        output.exit_code, 0,
        "the release job must stop on a failing gate"
    );
    assert_eq!(rig.release_job_status(WAVE_ONE), "stopped");
    let record = rig
        .gate_records()
        .into_iter()
        .find(|record| record.wave == WAVE_ONE)
        .expect("a regression record must name the wave");
    assert!(
        !record.gate.is_empty(),
        "the record must name the failing gate"
    );
    assert!(!record.commit.is_empty(), "the record must name the commit");
}

/// REQ: COR-104
#[test]
fn cor_104_a_passing_gate_on_a_candidate_does_not_stop_the_release() {
    let rig = OpsRig::new();
    let output = rig.run_release_gate(WAVE_ONE, "gate-passing-fixture");
    assert_eq!(
        output.exit_code, 0,
        "the release job must continue when every gate passes"
    );
    assert_ne!(rig.release_job_status(WAVE_ONE), "stopped");
}
