//! Activation of both sides.

use ilium_plugin_core_cost::{CostClient, CostServer};
use ilium_plugin_sdk::host::InstanceState;

use crate::common::*;

/// REQ: F-038
#[test]
fn lifecycle_038_server_activates_and_is_active() {
    let host = activated_server();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: F-031
#[test]
fn lifecycle_031_client_activates_and_is_active() {
    let host = activated_client();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: F-038
#[test]
fn lifecycle_038_server_activation_records_no_failure() {
    let host = activated_server();
    assert!(host.failures().is_empty());
}

/// REQ: F-031
#[test]
fn lifecycle_031_client_activation_records_no_failure() {
    let host = activated_client();
    assert!(host.failures().is_empty());
}

/// REQ: F-038
#[test]
fn lifecycle_038_server_try_activate_succeeds() {
    let mut host = server_host();
    assert!(host.try_activate::<CostServer>().is_ok());
}

/// REQ: F-031
#[test]
fn lifecycle_031_client_try_activate_succeeds() {
    let mut host = client_host();
    assert!(host.try_activate::<CostClient>().is_ok());
}

/// REQ: F-038
#[test]
fn lifecycle_038_server_activation_logs_no_error() {
    let host = activated_server();
    assert!(host
        .logs()
        .iter()
        .all(|line| line.message.is_empty() || !line.message.contains("error")));
}
