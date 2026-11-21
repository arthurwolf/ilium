//! Connection-owned private polling authority and capture lifecycle.
use std::path::PathBuf;
use std::sync::Arc;

use ilium_execution::{JobCost, Lane, Retained, Retention};
use ilium_ipc::polling::{PollingFeedEvent, PollingFeedStopReason};
use ilium_ipc::ServerEvent;

use crate::ipc::DirectEventSender;
use crate::polling_auth::AuthenticatedDetector;
use crate::state::ServerState;

pub(crate) struct AuthorizedFeed {
    pub(crate) proof: Arc<AuthenticatedDetector>,
    pub(crate) configuration_directory: PathBuf,
}

struct ActiveSubscription {
    capability: AuthorizedFeed,
    // Payload before its original storage guard, including cancellation paths.
    retention: Retention,
}

impl ActiveSubscription {
    async fn stopped(&self, state: &Arc<ServerState>) -> Option<PollingFeedStopReason> {
        let Some(execution) = state.execution.get() else {
            return Some(PollingFeedStopReason::Unavailable);
        };
        // The physical native read keeps a clone of the original charge and
        // birth proof even when its async connection owner is cancelled.
        let proof = self
            .retention
            .clone()
            .retain(Arc::clone(&self.capability.proof));
        let configuration_directory = self.capability.configuration_directory.clone();
        let checked = execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 4096,
                },
                move |context: ilium_execution::JobContext| {
                    if context.stop_requested() {
                        return Err("polling authority check cancelled".to_string());
                    }
                    let settings = crate::polling_config::load_settings(&configuration_directory)
                        .map_err(|error| error.to_string())?;
                    if !settings.enabled {
                        return Ok(Some(PollingFeedStopReason::Disabled));
                    }
                    let authorized = proof
                        .view()
                        .still_authorized(&settings)
                        .map_err(|error| error.to_string())?;
                    Ok((!authorized).then_some(PollingFeedStopReason::AuthorityLost))
                },
            )
            .await;
        match checked {
            Ok(result) => *result.view(),
            Err(error) => {
                state.record_blocking_error(None, "polling_feed_authority", &error.to_string());
                Some(PollingFeedStopReason::Unavailable)
            }
        }
    }
}

/// The caller's JoinSet owns capture and cancellation; losing the exact
/// authority terminates the connection-local subscription.
pub(crate) async fn run(
    state: Arc<ServerState>,
    subscription_id: u64,
    admitted: Retained<Option<AuthorizedFeed>>,
    replies: DirectEventSender,
) {
    let (capability, retention) = admitted.into_parts();
    let Some(capability) = capability else { return };
    let active = ActiveSubscription {
        capability,
        retention,
    };
    let Some(execution) = state.execution.get() else {
        return;
    };
    let Ok(_registry_storage) = execution.client.try_reserve_storage(1024 * 1024) else {
        let _ = replies
            .send(ServerEvent::PollingEvidence {
                subscription_id,
                event: PollingFeedEvent::Stopped(PollingFeedStopReason::Unavailable),
            })
            .await;
        return;
    };
    let mut capture = Capture::default();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let stopped = match active.stopped(&state).await {
            Some(reason) => Some(reason),
            None => capture
                .tick(&state, &active, subscription_id, &replies)
                .await
                .err(),
        };
        if let Some(reason) = stopped {
            let _ = replies
                .send(ServerEvent::PollingEvidence {
                    subscription_id,
                    event: PollingFeedEvent::Stopped(reason),
                })
                .await;
            return;
        }
    }
}

const MAX_FEED_AGENTS: usize = 1024;

#[derive(Default)]
struct Capture {
    cursors: std::collections::HashMap<
        ilium_core::NodeId,
        Retained<crate::polling_evidence::VerifiedCursor>,
    >,
}

impl Capture {
    async fn tick(
        &mut self,
        state: &Arc<ServerState>,
        authority: &ActiveSubscription,
        subscription_id: u64,
        replies: &DirectEventSender,
    ) -> Result<(), PollingFeedStopReason> {
        use ilium_core::polling_detection::AgentBinding;
        use ilium_ipc::polling::PollingActivityBatch;
        // The fixed registry admission covers both bounded metadata collections.
        // Overflow is explicit unavailability, never silent live-binding eviction.
        let mut panes = Vec::with_capacity(MAX_FEED_AGENTS);
        {
            let registry = state.panes.read().await;
            for (pane, resource) in registry.iter() {
                if matches!(resource, crate::pane::PaneResource::Terminal(runtime) if runtime.cache_wait_source().is_some())
                {
                    if panes.len() == MAX_FEED_AGENTS {
                        return Err(PollingFeedStopReason::Unavailable);
                    }
                    panes.push(*pane);
                }
            }
        }
        let retired: Vec<_> = self
            .cursors
            .keys()
            .filter(|pane| !panes.contains(pane))
            .copied()
            .collect();
        for pane in retired {
            if let Some(cursor) = self.cursors.remove(&pane) {
                let binding = AgentBinding {
                    session: state.session_name.clone(),
                    incarnation: state.polling.incarnation().to_string(),
                    pane,
                    generation: cursor.view().generation(),
                };
                send_private(
                    state,
                    authority,
                    subscription_id,
                    PollingFeedEvent::Closed(binding),
                    replies,
                )
                .await?;
            }
        }
        for pane in panes {
            let previous = self.cursors.remove(&pane);
            let old = previous
                .as_ref()
                .map(|cursor| (cursor.view().generation(), cursor.view().position()));
            let observed = crate::polling_evidence::read_verified(state, pane, previous).await;
            let observed = match observed {
                Ok(observed) => observed,
                Err(_) => {
                    if let Some((generation, _)) = old {
                        let binding = AgentBinding {
                            session: state.session_name.clone(),
                            incarnation: state.polling.incarnation().to_string(),
                            pane,
                            generation,
                        };
                        send_private(
                            state,
                            authority,
                            subscription_id,
                            PollingFeedEvent::Closed(binding),
                            replies,
                        )
                        .await?;
                    }
                    continue;
                }
            };
            let ((cursor, batch), retention) = observed.into_parts();
            let end = cursor.position();
            let start = old.map_or(end, |(_, position)| position);
            let binding = AgentBinding {
                session: state.session_name.clone(),
                incarnation: state.polling.incarnation().to_string(),
                pane,
                generation: cursor.generation(),
            };
            self.cursors.insert(pane, retention.clone().retain(cursor));
            if old.is_none() || start != end || batch.contains_gap || !batch.events.is_empty() {
                let event = PollingFeedEvent::Batch(PollingActivityBatch {
                    binding,
                    source_start: start,
                    source_end: end,
                    contains_gap: batch.contains_gap,
                    events: batch.events,
                });
                // Preserve the original acquisition charge while the direct queue
                // separately admits ownership; neither payload is cloned.
                let result = send_private(state, authority, subscription_id, event, replies).await;
                drop(retention);
                result?;
            }
        }
        Ok(())
    }
}

async fn send_private(
    state: &Arc<ServerState>,
    authority: &ActiveSubscription,
    subscription_id: u64,
    event: PollingFeedEvent,
    replies: &DirectEventSender,
) -> Result<(), PollingFeedStopReason> {
    if let Some(reason) = authority.stopped(state).await {
        return Err(reason);
    }
    let panes = state.panes.read().await;
    if let PollingFeedEvent::Batch(batch) = &event {
        if batch.validate().is_err() {
            return Err(PollingFeedStopReason::Unavailable);
        }
        let current = panes
            .get(&batch.binding.pane)
            .and_then(|resource| match resource {
                crate::pane::PaneResource::Terminal(runtime) => runtime.cache_wait_source(),
                _ => None,
            });
        if current.is_none_or(|source| source.generation != batch.binding.generation) {
            return Err(PollingFeedStopReason::Unavailable);
        }
    }
    // Keep the generation stable through nonblocking queue acceptance.
    // Do not await capacity after the authority fence: a stalled consumer must
    // not release a newly queued private batch after losing its lease.
    if replies.try_send(ServerEvent::PollingEvidence {
        subscription_id,
        event,
    }) {
        Ok(())
    } else {
        Err(PollingFeedStopReason::Unavailable)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io::Write;

    async fn subscribed_fixture() -> (
        crate::polling_test_support::EvidenceFixture,
        crate::polling_ledger::Ledger,
        Retained<Option<AuthorizedFeed>>,
    ) {
        let mut fixture = crate::polling_test_support::evidence_fixture().await;
        writeln!(fixture.transcript, "{}", serde_json::json!({
            "type":"response_item", "payload":{
                "type":"message", "role":"assistant", "id":"synthetic-historical-message",
                "content":[{"type":"output_text", "text":"Historical activity must not be replayed."}]
            }
        })).unwrap();
        let config = fixture.directory.path().join("config");
        let data = fixture.directory.path().join("polling-ledger");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(
            config.join("config.toml"),
            "[polling_detection]\nenabled=true\n",
        )
        .unwrap();
        let leader = crate::polling_ledger::Ledger::open(&data).unwrap();
        let execution = &fixture.state.execution.get().unwrap().client;
        fixture
            .state
            .polling
            .initialize(
                crate::PollingPaths {
                    config_directory: config,
                    data_directory: data,
                    registry_directory: fixture.directory.path().join("registry"),
                },
                execution,
            )
            .await
            .unwrap();
        let admitted = fixture
            .state
            .polling
            .admit_feed(
                Some(std::process::id()),
                ilium_ipc::polling::PollingEvidenceSubscription {
                    expected_server_incarnation: fixture.state.polling.incarnation().to_string(),
                    leader_epoch: leader.leader_epoch().to_string(),
                },
                execution,
            )
            .await
            .unwrap();
        assert!(admitted.view().is_some());
        (fixture, leader, admitted)
    }

    // Removing capture or replaying old data must fail at the first public
    // observation, with all source/process/native-I/O checks still real.
    #[tokio::test]
    async fn polling_feed_baselines_then_delivers_only_appended_normalized_activity() {
        let (mut fixture, leader, admitted) = subscribed_fixture().await;
        let execution = &fixture.state.execution.get().unwrap().client;
        let (sender, mut receiver) = DirectEventSender::admitted_channel(1, execution.clone());
        let mut task = crate::task_guard::AbortOnDropHandle::new(tokio::spawn(run(
            Arc::clone(&fixture.state),
            77,
            admitted,
            sender,
        )));
        let baseline = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .expect("an authenticated feed must establish a prospective baseline")
            .unwrap();
        let ServerEvent::PollingEvidence {
            subscription_id: 77,
            event: PollingFeedEvent::Batch(baseline),
        } = baseline
        else {
            panic!("expected a private baseline batch")
        };
        assert_eq!(baseline.binding.pane, fixture.pane);
        assert_eq!(baseline.binding.generation, 7);
        assert!(baseline.events.is_empty());
        assert_eq!(baseline.source_start, baseline.source_end);
        writeln!(fixture.transcript, "{}", serde_json::json!({
            "type":"response_item", "payload":{
                "type":"message", "role":"assistant", "phase":"commentary", "id":"synthetic-message-1",
                "content":[{"type":"output_text", "text":"The synthetic job is pending."}]
            }
        })).unwrap();
        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        let ServerEvent::PollingEvidence {
            subscription_id: 77,
            event: PollingFeedEvent::Batch(batch),
        } = observed
        else {
            panic!("expected appended normalized activity")
        };
        batch.validate().unwrap();
        assert_eq!(batch.binding, baseline.binding);
        assert_eq!(batch.source_start, baseline.source_end);
        assert!(batch.source_end > batch.source_start);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].text, "The synthetic job is pending.");
        assert_eq!(batch.events[0].timestamp, None);
        assert!(baseline.source_start > 0, "historical bytes were baselined");
        {
            let mut panes = fixture.state.panes.write().await;
            let crate::pane::PaneResource::Terminal(runtime) =
                panes.get_mut(&fixture.pane).unwrap()
            else {
                panic!("fixture terminal missing")
            };
            runtime.agent_generation = 8;
        }
        let closed = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(closed, ServerEvent::PollingEvidence { subscription_id: 77,
            event: PollingFeedEvent::Closed(ref binding) } if binding.generation == 7)
        );
        let replacement = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        let ServerEvent::PollingEvidence {
            subscription_id: 77,
            event: PollingFeedEvent::Batch(replacement),
        } = replacement
        else {
            panic!("replacement must establish its own baseline")
        };
        assert_eq!(replacement.binding.generation, 8);
        assert!(replacement.events.is_empty());
        assert_eq!(replacement.source_start, replacement.source_end);
        drop(leader);
        let stopped = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            stopped,
            ServerEvent::PollingEvidence {
                subscription_id: 77,
                event: PollingFeedEvent::Stopped(PollingFeedStopReason::AuthorityLost)
            }
        ));
        task.settle().await.unwrap();
    }
    #[tokio::test]
    async fn polling_private_send_refuses_slow_consumers_opt_out_and_lost_leases() {
        let (fixture, leader, admitted) = subscribed_fixture().await;
        let (capability, retention) = admitted.into_parts();
        let authority = ActiveSubscription {
            capability: capability.unwrap(),
            retention,
        };
        let execution = &fixture.state.execution.get().unwrap().client;
        let (sender, mut receiver) = DirectEventSender::admitted_channel(1, execution.clone());
        assert!(sender.try_send(ServerEvent::Polling {
            request_id: 1,
            reply: ilium_ipc::polling::PollingReply::Accepted
        }));
        let event = || {
            PollingFeedEvent::Closed(ilium_core::polling_detection::AgentBinding {
                session: fixture.state.session_name.clone(),
                incarnation: fixture.state.polling.incarnation().to_string(),
                pane: fixture.pane,
                generation: 7,
            })
        };
        let slow = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            send_private(&fixture.state, &authority, 88, event(), &sender),
        )
        .await
        .expect("a full queue must not wait to publish after its authority check");
        assert_eq!(slow, Err(PollingFeedStopReason::Unavailable));
        assert!(matches!(
            receiver.recv().await,
            Some(ServerEvent::Polling { request_id: 1, .. })
        ));
        assert!(receiver.try_recv().is_err());
        let config = fixture.directory.path().join("config/config.toml");
        std::fs::write(&config, "[polling_detection]\nenabled=false\n").unwrap();
        assert_eq!(
            send_private(&fixture.state, &authority, 88, event(), &sender).await,
            Err(PollingFeedStopReason::Disabled)
        );
        assert!(receiver.try_recv().is_err());
        std::fs::write(&config, "[polling_detection]\nenabled=true\n").unwrap();
        drop(leader);
        assert_eq!(
            send_private(&fixture.state, &authority, 88, event(), &sender).await,
            Err(PollingFeedStopReason::AuthorityLost)
        );
        assert!(receiver.try_recv().is_err());
    }
}
