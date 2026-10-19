//! The observe hook `log-sinks` (page sample "Owner side: run the log-sinks hook") and the batching
//! rule EXT-054: one chain per 250 ms with up to 64 records.

use serde_json::Value;

use crate::common::{
    append_log, log_line, observe_sinks, session, sink_chains, write_log, PLUGIN_ID,
};

const LOG: &str = "log-2026-10-12 09-14-03.txt";

fn settle_window(session: &mut ilium_plugin_sdk::testing::FakeSession) {
    session.pump().expect("session settles");
    session.server(PLUGIN_ID).advance_clock(250);
    session.pump().expect("session settles");
}

fn records(chains: &[Vec<Value>]) -> Vec<Value> {
    chains.iter().flatten().cloned().collect()
}

/// REQ: COR-050
#[test]
fn cor_050_a_warn_line_is_published_to_a_log_sink_with_level_warn() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    let all = records(&sink_chains(&mut session, "example.chat"));
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["level"], "warn");
    assert!(all[0]["message"]
        .as_str()
        .unwrap()
        .contains("disk almost full"));
}

/// REQ: COR-050
#[test]
fn cor_050_an_error_line_is_published_with_level_error() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(
        &mut session,
        LOG,
        log_line("ERROR", "model call failed").as_bytes(),
    );
    settle_window(&mut session);
    let all = records(&sink_chains(&mut session, "example.chat"));
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["level"], "error");
}

/// REQ: COR-050
#[test]
fn cor_050_info_lines_are_never_published() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(&mut session, LOG, log_line("INFO", "started").as_bytes());
    settle_window(&mut session);
    assert!(records(&sink_chains(&mut session, "example.chat")).is_empty());
}

/// REQ: COR-050
#[test]
fn cor_050_a_payload_holds_exactly_the_five_documented_keys() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    let all = records(&sink_chains(&mut session, "example.chat"));
    let mut keys: Vec<&String> = all[0].as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["level", "message", "plugin", "target", "time"]);
}

/// REQ: COR-050
#[test]
fn cor_050_the_target_of_a_published_record_is_plugin() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    assert_eq!(
        records(&sink_chains(&mut session, "example.chat"))[0]["target"],
        "plugin"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_without_a_contributor_no_hook_chain_is_started() {
    let mut session = session();
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    assert!(session.server(PLUGIN_ID).submitted().is_empty());
    assert!(sink_chains(&mut session, "example.chat").is_empty());
}

/// REQ: EXT-054
#[test]
fn ext_054_64_records_inside_one_window_arrive_in_a_single_chain() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    let text: String = (0..64)
        .map(|index| log_line("WARN", &format!("problem {index:02}")))
        .collect();
    write_log(&mut session, LOG, text.as_bytes());
    settle_window(&mut session);
    let chains = sink_chains(&mut session, "example.chat");
    assert_eq!(
        chains.iter().map(Vec::len).collect::<Vec<_>>(),
        [64],
        "{chains:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_65_records_inside_one_window_split_into_64_and_1() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    let text: String = (0..65)
        .map(|index| log_line("WARN", &format!("problem {index:02}")))
        .collect();
    write_log(&mut session, LOG, text.as_bytes());
    settle_window(&mut session);
    settle_window(&mut session);
    let chains = sink_chains(&mut session, "example.chat");
    assert_eq!(
        chains.iter().map(Vec::len).collect::<Vec<_>>(),
        [64, 1],
        "{chains:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_63_records_inside_one_window_arrive_in_a_single_chain() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    let text: String = (0..63)
        .map(|index| log_line("WARN", &format!("problem {index:02}")))
        .collect();
    write_log(&mut session, LOG, text.as_bytes());
    settle_window(&mut session);
    let chains = sink_chains(&mut session, "example.chat");
    assert_eq!(
        chains.iter().map(Vec::len).collect::<Vec<_>>(),
        [63],
        "{chains:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_records_keep_their_log_order_across_chains() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    let text: String = (0..70)
        .map(|index| log_line("WARN", &format!("problem {index:02}")))
        .collect();
    write_log(&mut session, LOG, text.as_bytes());
    settle_window(&mut session);
    settle_window(&mut session);
    let messages: Vec<String> = records(&sink_chains(&mut session, "example.chat"))
        .iter()
        .map(|record| record["message"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(messages.len(), 70);
    assert!(
        messages[0].contains("problem 00") && messages[69].contains("problem 69"),
        "{messages:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_two_writes_inside_250_ms_make_one_chain() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(&mut session, LOG, log_line("WARN", "first").as_bytes());
    session.pump().expect("session settles");
    session.server(PLUGIN_ID).advance_clock(100);
    append_log(&mut session, LOG, &log_line("WARN", "second"));
    settle_window(&mut session);
    let chains = sink_chains(&mut session, "example.chat");
    assert_eq!(
        chains.iter().map(Vec::len).collect::<Vec<_>>(),
        [2],
        "{chains:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_a_write_after_the_window_starts_a_new_chain() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    write_log(&mut session, LOG, log_line("WARN", "first").as_bytes());
    settle_window(&mut session);
    append_log(&mut session, LOG, &log_line("WARN", "second"));
    settle_window(&mut session);
    let chains = sink_chains(&mut session, "example.chat");
    assert_eq!(
        chains.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 1],
        "{chains:?}"
    );
}

/// REQ: EXT-054
#[test]
fn ext_054_a_record_written_by_a_sink_handler_during_its_call_is_not_delivered_again() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    session.observer_logs(
        "example.chat",
        "ilium.core.diagnostics/log-sinks",
        "warn",
        "handler complains",
    );
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    settle_window(&mut session);
    let all = records(&sink_chains(&mut session, "example.chat"));
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(!all[0]["message"]
        .as_str()
        .unwrap()
        .contains("handler complains"));
}

/// REQ: EXT-054
#[test]
fn ext_054_host_extension_records_about_log_sinks_handlers_are_not_delivered() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    session.observer_fails("example.chat", "ilium.core.diagnostics/log-sinks");
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    settle_window(&mut session);
    let all = records(&sink_chains(&mut session, "example.chat"));
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(
        all.iter().all(|record| record["plugin"] != "extension"),
        "{all:?}"
    );
}

/// REQ: COR-050
#[test]
fn cor_050_a_failing_sink_does_not_matter_to_the_other_sinks() {
    let mut session = session();
    observe_sinks(&mut session, "example.chat");
    observe_sinks(&mut session, "example.metrics");
    session.observer_fails("example.chat", "ilium.core.diagnostics/log-sinks");
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    assert_eq!(
        records(&sink_chains(&mut session, "example.metrics")).len(),
        1
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_ninth_sink_is_refused_because_max_handlers_is_8() {
    let mut session = session();
    for index in 0..9 {
        observe_sinks(&mut session, &format!("example.sink{index}"));
    }
    write_log(
        &mut session,
        LOG,
        log_line("WARN", "disk almost full").as_bytes(),
    );
    settle_window(&mut session);
    let delivered = (0..9)
        .filter(|index| !sink_chains(&mut session, &format!("example.sink{index}")).is_empty())
        .count();
    assert_eq!(delivered, 8);
}
