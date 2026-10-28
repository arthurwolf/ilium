//! Operator upgrade doc checks: one test per requirement ID for recording the plugin
//! list before a binary replacement and for the shutdown timeout that drops plugins
//! without `deactivate`. Each test reads the manual page that states the rule and checks
//! the rule's key phrases.

use crate::support::manual_page;

/// REQ: LIF-299
#[test]
fn lif_299_plugin_list_recorded_before_binary_replace() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "Before replacing the Ilium binary",
        "the operator MUST run `ilium plugin list --json`",
        "MUST keep its output as the record",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-300
#[test]
fn lif_300_shutdown_drops_plugin_without_deactivate() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "A clean server stop MUST give the whole shutdown",
        "MUST be dropped without `deactivate`",
        "with `host.shutdown-timeout` logged",
        "MUST NOT treat a quiet plugin log",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-299
#[test]
fn lif_299_before_and_after_listings_are_compared_after_restart() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "ilium plugin list --json > before.json",
        "A plugin whose package state differs between `before.json` and `after.json`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-300
#[test]
fn lif_300_whole_shutdown_row_names_five_seconds_and_timeout_record() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "| The whole shutdown exceeds 5 s |",
        "`host.shutdown-timeout`: the server shutdown took longer than the host allows",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
