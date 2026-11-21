//! Polling projections over a real isolated session transport.
use ilium_core::polling_detection::DetectorStatus;
use ilium_ipc::polling::{PollingOperation, PollingRejection, PollingReply};
use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
use std::time::Duration;
mod common;

#[tokio::test]
async fn polling_snapshot_reports_disabled_without_a_client_owned_detector() {
    let server = common::TestServer::start("polling-rpc-off").await;
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 17,
            operation: PollingOperation::Snapshot,
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::Polling {
        request_id: 17,
        reply: PollingReply::Snapshot(snapshot),
    } = event
    else {
        panic!("expected the server-owned disabled polling snapshot, got {event:?}");
    };
    assert_eq!(snapshot.status, DetectorStatus::Off);
    assert!(!snapshot.settings.enabled);
    assert!(!snapshot.key_configured);
    assert!(snapshot.findings.is_empty());
    assert!(
        snapshot.all_time.is_none(),
        "unopened accounting must not invent a zero-dollar history"
    );
}

#[tokio::test]
async fn invalid_polling_operations_are_rejected_over_the_transport() {
    let server = common::TestServer::start("polling-rpc-invalid").await;
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 18,
            operation: PollingOperation::History {
                cursor: None,
                limit: 0,
            },
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        ServerEvent::Polling {
            request_id: 18,
            reply: PollingReply::Rejected(PollingRejection::InvalidRequest)
        }
    ));
}

async fn configured_snapshot(enabled: bool) -> ilium_ipc::polling::PollingSnapshot {
    configured_snapshot_with_history(enabled, false).await
}

async fn configured_snapshot_with_history(
    enabled: bool,
    seed_unknown: bool,
) -> ilium_ipc::polling::PollingSnapshot {
    let fixture = tempfile::tempdir().unwrap();
    let config = fixture.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        format!("[polling_detection]\nenabled={enabled}\nevent_interval=7\n"),
    )
    .unwrap();
    if seed_unknown {
        use ilium_server::polling_ledger::{Ledger, NewAttempt};
        let mut ledger = Ledger::open(&fixture.path().join("data")).unwrap();
        ledger
            .prepare(
                &NewAttempt {
                    request_id: "synthetic-lost-response".into(),
                    window_id: "synthetic-window".into(),
                    month: "2026-11".into(),
                    day: "2026-11-16".into(),
                    created_utc: "2026-11-16T12:00:00Z".into(),
                    project_id: "synthetic-project".into(),
                    model: "gpt-6-luna".into(),
                    reserved_nanodollars: 10,
                },
                None,
            )
            .unwrap();
        ledger.mark_dispatched("synthetic-lost-response").unwrap();
    }
    let server = common::TestServer::start_with_polling_paths(
        "polling-config",
        ilium_server::PollingPaths {
            config_directory: config,
            data_directory: fixture.path().join("data"),
            registry_directory: fixture.path().join("registry"),
        },
    )
    .await;
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 19,
            operation: PollingOperation::Snapshot,
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::Polling {
        request_id: 19,
        reply: PollingReply::Snapshot(snapshot),
    } = event
    else {
        panic!("expected configured polling snapshot, got {event:?}");
    };
    *snapshot
}

#[tokio::test]
async fn polling_server_loads_global_opt_in_without_an_attached_client() {
    let snapshot = configured_snapshot(true).await;
    assert!(snapshot.settings.enabled);
    assert_eq!(snapshot.settings.event_interval, 7);
    assert_eq!(snapshot.status, DetectorStatus::NoApiKey);
    assert!(!snapshot.key_configured);
}

#[tokio::test]
async fn polling_server_retains_disabled_configuration_without_capture() {
    let snapshot = configured_snapshot(false).await;
    assert_eq!(snapshot.settings.event_interval, 7);
    assert!(!snapshot.settings.enabled);
    assert_eq!(snapshot.status, DetectorStatus::Off);
}

#[tokio::test]
async fn polling_disabled_detection_keeps_empty_accounting_readable() {
    let snapshot = configured_snapshot(false).await;
    let all_time = snapshot
        .all_time
        .expect("an opened ledger must expose accounting even while disabled");
    assert_eq!(all_time.attempts, 0);
    assert_eq!(all_time.unknown_attempts, 0);
    assert_eq!(all_time.reserved_nanodollars, 0);
}

#[tokio::test]
async fn polling_recovered_uncertain_charge_reaches_the_client_projection() {
    let snapshot = configured_snapshot_with_history(false, true).await;
    let all_time = snapshot
        .all_time
        .expect("recovered accounting must reach the client");
    assert_eq!(all_time.attempts, 1);
    assert_eq!(all_time.unknown_attempts, 1);
    assert_eq!(all_time.reserved_nanodollars, 10);
    assert_eq!(all_time.known_cost_nanodollars, 0);
}

// A startup-only loader misses settings changed while no client is attached.
#[tokio::test]
async fn polling_server_reloads_configuration_without_an_attached_client() {
    let fixture = tempfile::tempdir().unwrap();
    let config = fixture.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "[polling_detection]\nenabled=false\n",
    )
    .unwrap();
    let server = common::TestServer::start_with_polling_paths(
        "polling-reload",
        ilium_server::PollingPaths {
            config_directory: config.clone(),
            data_directory: fixture.path().join("data"),
            registry_directory: fixture.path().join("registry"),
        },
    )
    .await;
    std::fs::write(
        config.join("config.toml"),
        "[polling_detection]\nenabled=true\nevent_interval=9\n",
    )
    .unwrap();
    // No connection exists during the reload interval. The real server owns it,
    // even when the independent sound-config watcher is disabled in this fixture.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 20,
            operation: PollingOperation::Snapshot,
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::Polling {
        request_id: 20,
        reply: PollingReply::Snapshot(snapshot),
    } = event
    else {
        panic!("expected reloaded polling snapshot, got {event:?}");
    };
    assert!(
        snapshot.settings.enabled,
        "the server must apply saved opt-in without a client"
    );
    assert_eq!(snapshot.settings.event_interval, 9);
    assert_eq!(snapshot.status, DetectorStatus::NoApiKey);
    assert!(matches!(
        ilium_server::polling_ledger::Ledger::open(&fixture.path().join("data")),
        Err(ilium_server::polling_ledger::LedgerError::AlreadyOwned)
    ));
}

// The transport must expose durable history without turning unknown billing into zero.
#[tokio::test]
async fn polling_history_pages_keep_recovered_unknown_billing_and_reject_stale_cursors() {
    use ilium_ipc::polling::{HistoryCursor, PollingAttemptState};
    use ilium_server::polling_ledger::{Ledger, NewAttempt};
    let fixture = tempfile::tempdir().unwrap();
    let data = fixture.path().join("data");
    {
        let mut ledger = Ledger::open(&data).unwrap();
        for index in 1..=2 {
            ledger
                .prepare(
                    &NewAttempt {
                        request_id: format!("synthetic-history-{index}"),
                        window_id: format!("synthetic-window-{index}"),
                        month: "2026-11".into(),
                        day: "2026-11-16".into(),
                        created_utc: "2026-11-16T12:00:00Z".into(),
                        project_id: "synthetic-project".into(),
                        model: "synthetic-model".into(),
                        reserved_nanodollars: 10,
                    },
                    None,
                )
                .unwrap();
        }
        ledger.mark_dispatched("synthetic-history-1").unwrap();
    }
    let server = common::TestServer::start_with_polling_paths(
        "polling-history",
        ilium_server::PollingPaths {
            config_directory: fixture.path().join("config"),
            data_directory: data,
            registry_directory: fixture.path().join("registry"),
        },
    )
    .await;
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 21,
            operation: PollingOperation::History {
                cursor: None,
                limit: 1,
            },
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::Polling {
        request_id: 21,
        reply: PollingReply::History(first),
    } = event
    else {
        panic!("expected polling history, got {event:?}")
    };
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].state, PollingAttemptState::Unknown);
    assert!(first.records[0].billing_unknown);
    assert_eq!(first.records[0].cost_nanodollars, None);
    assert_eq!(first.records[0].reserved_nanodollars, 10);
    let cursor = first
        .next
        .expect("the second attempt must remain reachable");
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 22,
            operation: PollingOperation::History {
                cursor: Some(cursor.clone()),
                limit: 1,
            },
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::Polling {
        request_id: 22,
        reply: PollingReply::History(second),
    } = event
    else {
        panic!("expected second polling page, got {event:?}")
    };
    assert_eq!(second.revision, first.revision);
    assert_eq!(second.records.len(), 1);
    assert_eq!(second.records[0].state, PollingAttemptState::Cancelled);
    assert_eq!(second.records[0].reserved_nanodollars, 0);
    assert!(!second.records[0].billing_unknown);
    assert!(second.next.is_none());
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 23,
            operation: PollingOperation::History {
                cursor: Some(HistoryCursor {
                    revision: 0,
                    after_sequence: cursor.after_sequence,
                }),
                limit: 1,
            },
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        ServerEvent::Polling {
            request_id: 23,
            reply: PollingReply::Rejected(PollingRejection::StaleCursor)
        }
    ));
}

// Registry publication cannot depend on resource-queue initialization.
#[tokio::test]
async fn polling_session_publishes_its_endpoint_without_the_resource_service() {
    let fixture = tempfile::tempdir().unwrap();
    let registry = fixture.path().join("registry");
    let mut server = common::TestServer::start_with_polling_paths(
        "polling-registry",
        ilium_server::PollingPaths {
            config_directory: fixture.path().join("config"),
            data_directory: fixture.path().join("data"),
            registry_directory: registry.clone(),
        },
    )
    .await;
    // Socket binding precedes bootstrap; a completed request establishes that
    // startup has finished without a timing-dependent filesystem assertion.
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 24,
            operation: PollingOperation::Snapshot,
        },
    )
    .await
    .unwrap();
    let event: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        ServerEvent::Polling {
            request_id: 24,
            reply: PollingReply::Snapshot(_)
        }
    ));
    let found = ilium_server::polling_discovery::discover(&registry)
        .expect("polling startup must publish its endpoint before serving requests");
    assert!(!found.incomplete);
    assert_eq!(found.endpoints.len(), 1);
    assert_eq!(found.endpoints[0].socket_path, server.socket_path);
    assert!(!found.endpoints[0].incarnation.is_nil());
    write_frame(&mut stream, &ClientRequest::KillSession)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), &mut server.server_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(ilium_server::polling_discovery::discover(&registry)
        .unwrap()
        .endpoints
        .is_empty());
}

// Losing transport identity before stream splitting must not silently prevent
// the actual exclusive detector owner from subscribing to this exact server.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn polling_private_feed_admits_the_kernel_peer_lease_owner_only() {
    use ilium_ipc::polling::PollingEvidenceSubscription;
    let fixture = tempfile::tempdir().unwrap();
    let config = fixture.path().join("config");
    let data = fixture.path().join("data");
    let registry = fixture.path().join("registry");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "[polling_detection]\nenabled=true\n",
    )
    .unwrap();
    let leader = ilium_server::polling_ledger::Ledger::open(&data).unwrap();
    let server = common::TestServer::start_with_polling_paths(
        "polling-private-peer",
        ilium_server::PollingPaths {
            config_directory: config,
            data_directory: data,
            registry_directory: registry.clone(),
        },
    )
    .await;
    let mut stream = server.connect().await;
    write_frame(
        &mut stream,
        &ClientRequest::Polling {
            request_id: 30,
            operation: PollingOperation::Snapshot,
        },
    )
    .await
    .unwrap();
    let _: ServerEvent = read_frame(&mut stream).await.unwrap();
    let discovered = ilium_server::polling_discovery::discover(&registry).unwrap();
    let incarnation = discovered.endpoints[0].incarnation.to_string();
    for (request_id, expected_incarnation, epoch, expected) in [
        (31, incarnation.clone(), "wrong-epoch".to_string(), false),
        (
            32,
            uuid::Uuid::new_v4().to_string(),
            leader.leader_epoch().to_string(),
            false,
        ),
        (33, incarnation, leader.leader_epoch().to_string(), true),
    ] {
        write_frame(
            &mut stream,
            &ClientRequest::SubscribePollingEvidence {
                request_id,
                subscription: PollingEvidenceSubscription {
                    expected_server_incarnation: expected_incarnation.clone(),
                    leader_epoch: epoch,
                },
            },
        )
        .await
        .unwrap();
        let event: ServerEvent =
            tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
                .await
                .unwrap()
                .unwrap();
        let ServerEvent::Polling {
            request_id: actual,
            reply,
        } = event
        else {
            panic!("expected private handshake outcome")
        };
        assert_eq!(actual, request_id);
        assert_eq!(
            reply,
            if expected {
                PollingReply::EvidenceSubscribed {
                    server_incarnation: expected_incarnation,
                }
            } else {
                PollingReply::Rejected(PollingRejection::Unauthorized)
            }
        );
    }
    drop(leader);
    let stopped: ServerEvent =
        tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream))
            .await
            .expect("private subscription must terminate when its leader releases the lease")
            .unwrap();
    assert!(matches!(
        stopped,
        ServerEvent::PollingEvidence {
            subscription_id: 33,
            event: ilium_ipc::polling::PollingFeedEvent::Stopped(
                ilium_ipc::polling::PollingFeedStopReason::AuthorityLost
            ),
        }
    ));
}
