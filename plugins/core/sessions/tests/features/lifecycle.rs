//! Activation of both sides (page "Manifest": server and client worlds).

use ilium_plugin_core_sessions::{SessionsClient, SessionsServer};
use ilium_plugin_sdk::host::InstanceState;
use ilium_plugin_sdk::types::DeactivationReason;

use crate::common::*;

/// REQ: F-076
#[test]
fn lifecycle_076_server_activates_and_is_active() {
    let host = activated_server();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: F-076
#[test]
fn lifecycle_076_client_activates_and_is_active() {
    let host = activated_client();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: F-076
#[test]
fn lifecycle_076_server_try_activate_succeeds() {
    let mut host = server_host();
    assert!(host.try_activate::<SessionsServer>().is_ok());
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: F-076
#[test]
fn lifecycle_076_client_try_activate_succeeds() {
    let mut host = client_host();
    assert!(host.try_activate::<SessionsClient>().is_ok());
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: F-076
#[test]
fn lifecycle_076_server_activation_records_no_failure() {
    let host = activated_server();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
    assert!(host.failures().is_empty());
}

/// REQ: F-076
#[test]
fn lifecycle_076_server_activation_submits_nothing_without_panes() {
    let host = activated_server();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
    assert!(host.submitted().is_empty());
}

/// REQ: F-076
#[test]
fn lifecycle_076_server_deactivates_on_shutdown_without_failure() {
    let mut host = activated_server();
    host.deactivate(DeactivationReason::Shutdown);
    // manual: deactivating ends in inactive or stopped; it does not pin which for shutdown.
    assert!(
        matches!(
            host.lifecycle_state(),
            InstanceState::Inactive | InstanceState::Stopped
        ),
        "{:?}",
        host.lifecycle_state()
    );
    assert!(host.failures().is_empty());
}

/// REQ: F-076
#[test]
fn lifecycle_076_server_survives_an_empty_batch() {
    let mut host = activated_server();
    assert!(host.deliver(Vec::new()).is_empty());
    assert!(host.failures().is_empty());
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}
