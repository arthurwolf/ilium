//! Manual page `guide-enterprise-deployment`: no network egress of logs, traps or metrics (HST-990),
//! stdout-only metrics (HST-991), owner-only copies of logs (HST-992) and no log or metric
//! forwarding key in the admin policy (HST-993).

use ilium_plugin_host::testing::ops::OpsRig;
use ilium_plugin_host::policy::{PolicyEngine, PolicyInput, PolicyOwnership};

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "guide-enterprise-deployment.md";

/// REQ: HST-990
#[test]
fn hst_990_plugin_logs_traps_and_metrics_are_not_sent_to_any_network_address() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The host MUST NOT send plugin operational logs (`<state>/plugins/logs/`), host trap or crash lines, or `ilium plugin metrics` output to any network address, socket or remote service.",
    );
}

/// REQ: HST-990
#[test]
fn hst_990_the_only_remote_route_is_the_audit_sink_which_carries_audit_records() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The only remote route is the audit sink of [SEC-228], which carries audit records and nothing else.",
    );
}

/// REQ: HST-991
#[test]
fn hst_991_plugin_metrics_writes_prometheus_text_to_standard_output_only() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`ilium plugin metrics` MUST write its Prometheus text to standard output only",
    );
}

/// REQ: HST-991
#[test]
fn hst_991_plugin_metrics_never_opens_a_listening_socket() {
    let ops = OpsRig::new();
    let output = ops.cli(&["plugin", "metrics"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stderr);
    let text = format!("{}{}", output.stdout_raw, output.stdout_jsonl.iter().map(|line| line.to_string()).collect::<String>());
    assert!(text.contains("# TYPE"), "Prometheus text goes to standard output: {text}");
    // No socket, listener address or port file appears in the state folder.
    let mut stack = vec![ops.state_dir()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).expect("read state folder").flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            assert!(!name.ends_with(".sock") && !name.contains("listen"), "socket artefact: {}", path.display());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
}

/// REQ: HST-992
#[test]
fn hst_992_a_copied_plugin_log_is_kept_under_owner_only_modes() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST keep the copy under owner-only modes (0600 for files, 0700 for folders)",
    );
}

/// REQ: HST-992
#[test]
fn hst_992_a_log_copy_never_includes_the_trust_store_grants_consents_or_data() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST NOT copy the trust store, grants, consents or plugin data folders",
    );
}

/// REQ: HST-993
#[test]
fn hst_993_the_admin_policy_defines_no_key_that_forwards_logs_or_metrics() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The admin policy MUST NOT define a key that forwards plugin logs or metrics in this version.",
    );
}

/// REQ: HST-993
#[test]
fn hst_993_a_policy_file_with_a_forwarding_key_is_rejected() {
    let admin_owned = Some(PolicyOwnership {
        administrator_owned: true,
        non_administrator_writable: false,
    });
    for key in ["forward_logs", "forward_metrics", "log_sink"] {
        let engine = PolicyEngine::from_inputs(
            PolicyInput::Document {
                text: format!("[logs]\n{key} = \"https://collector.example/ingest\"\n"),
                ownership: admin_owned,
            },
            PolicyInput::Missing,
        );
        assert!(!engine.is_valid(), "{key} must be rejected");
    }
    let plain = PolicyEngine::from_inputs(
        PolicyInput::Document { text: String::new(), ownership: admin_owned },
        PolicyInput::Missing,
    );
    assert!(plain.is_valid(), "an empty policy stays valid");
}
