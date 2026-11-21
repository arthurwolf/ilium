//! Native authenticated private-feed transport; never paid-dispatch authority.
//!
//! One connection is one subscription to one serving session. The leader proves
//! the serving peer before subscribing and again before every evidence batch;
//! a lost proof permanently closes the connection.
use std::sync::Arc;

use ilium_execution::{JobCost, Lane, Retained, Retention};
use ilium_ipc::polling::{PollingEvidenceSubscription, PollingFeedEvent, SubscriptionObservation};
use ilium_ipc::{ClientRequest, FrameWriter, IpcError};
use ilium_transport::{SessionEndpoint, SessionReadHalf, SessionWriteHalf};

use crate::polling_auth::AuthenticatedSessionServer;
use crate::polling_subscription_reader::SubscriptionReader;

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn refused(message: &str) -> IpcError {
    std::io::Error::other(message.to_owned()).into()
}

pub(crate) struct PrivateConnection {
    reader: SubscriptionReader<SessionReadHalf>,
    /// Held so the serving session sees an open stream; closing it would end
    /// the feed.
    _writer: FrameWriter<SessionWriteHalf>,
    proof: Arc<AuthenticatedSessionServer>,
    retention: Retention,
    closed: bool,
}

impl PrivateConnection {
    pub(crate) async fn connect(
        registry: &std::path::Path,
        hint: crate::polling_discovery::SessionEndpointHint,
        leader_epoch: String,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<Self, IpcError> {
        let stream = tokio::time::timeout(
            CONNECT_TIMEOUT,
            SessionEndpoint::from_path(hint.socket_path.clone()).connect(),
        )
        .await
        .map_err(|_| refused("private polling endpoint timed out"))?
        .map_err(|error| refused(&format!("private polling endpoint refused: {error}")))?;
        let peer = stream.peer_process_id();
        let admitted = {
            let registry = registry.to_path_buf();
            let hint = hint.clone();
            execution
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 32 * 1024 * 1024,
                        result_bytes: 64 * 1024,
                    },
                    move |_context: ilium_execution::JobContext| {
                        AuthenticatedSessionServer::admit(&registry, &hint, peer)
                    },
                )
                .await
                .map_err(|_| refused("private polling admission was refused"))?
        };
        let (proof, retention) = admitted.into_parts();
        let proof = proof
            .map(Arc::new)
            .ok_or_else(|| refused("private polling peer is not the serving session"))?;
        let (read_half, write_half) = stream.into_split();
        let request_id = uuid::Uuid::new_v4().as_u128() as u64;
        let mut writer = FrameWriter::new(write_half);
        writer
            .write(&ClientRequest::SubscribePollingEvidence {
                request_id,
                subscription: PollingEvidenceSubscription {
                    expected_server_incarnation: hint.incarnation.to_string(),
                    leader_epoch,
                },
            })
            .await?;
        let reader = SubscriptionReader::new(
            read_half,
            request_id,
            hint.incarnation.to_string(),
            execution,
        )?;
        Ok(Self {
            reader,
            _writer: writer,
            proof,
            retention,
            closed: false,
        })
    }

    pub(crate) async fn next(
        &mut self,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<Retained<SubscriptionObservation>, IpcError> {
        if self.closed {
            return Err(refused("private polling connection is closed"));
        }
        // Handshake notices carry no evidence and are never surfaced.
        let observation = loop {
            match self.reader.next(execution).await {
                Ok(observation)
                    if matches!(
                        observation.view(),
                        SubscriptionObservation::Pending | SubscriptionObservation::Subscribed
                    ) => {}
                Ok(observation) => break observation,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            }
        };
        if matches!(
            observation.view(),
            SubscriptionObservation::Activity(PollingFeedEvent::Batch(_))
        ) {
            let proof = self.retention.clone().retain(Arc::clone(&self.proof));
            let authorized = execution
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 32 * 1024 * 1024,
                        result_bytes: 4096,
                    },
                    move |_context: ilium_execution::JobContext| proof.view().still_authorized(),
                )
                .await;
            if !matches!(authorized.map(|result| *result.view()), Ok(true)) {
                self.closed = true;
                return Err(refused("private polling publisher authority was lost"));
            }
        }
        Ok(observation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{polling_detection::AgentBinding, NodeId};
    use ilium_ipc::{
        polling::{PollingActivityBatch, PollingFeedEvent, PollingReply},
        ClientRequest, FrameReader, FrameWriter, ServerEvent,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn polling_private_connection_authenticates_evidence_and_rechecks_publisher_lifetime() {
        let fixture = tempfile::tempdir().unwrap();
        let endpoint =
            ilium_transport::SessionEndpoint::from_path(fixture.path().join("feed.sock"));
        let mut listener = endpoint.bind().await.unwrap();
        let incarnation = uuid::Uuid::new_v4();
        let registry = fixture.path().join("registry");
        let publication =
            crate::polling_discovery::publish(&registry, incarnation, endpoint.as_path()).unwrap();
        let serving_incarnation = incarnation.to_string();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let stream = listener.accept().await.unwrap();
            let (read, write) = stream.into_split();
            let request: ClientRequest = FrameReader::new(read).read().await.unwrap();
            let ClientRequest::SubscribePollingEvidence {
                request_id,
                subscription,
            } = request
            else {
                panic!("expected a private subscription")
            };
            assert_eq!(
                subscription.expected_server_incarnation,
                serving_incarnation
            );
            assert_eq!(subscription.leader_epoch, "synthetic-epoch");
            let mut writer = FrameWriter::new(write);
            writer
                .write(&ServerEvent::Polling {
                    request_id,
                    reply: PollingReply::EvidenceSubscribed {
                        server_incarnation: serving_incarnation.clone(),
                    },
                })
                .await
                .unwrap();
            writer
                .write(&ServerEvent::PollingEvidence {
                    subscription_id: request_id,
                    event: PollingFeedEvent::Batch(PollingActivityBatch {
                        binding: AgentBinding {
                            session: "synthetic-session".into(),
                            incarnation: serving_incarnation,
                            pane: NodeId(7),
                            generation: 2,
                        },
                        source_start: 9,
                        source_end: 9,
                        contains_gap: false,
                        events: Vec::new(),
                    }),
                })
                .await
                .unwrap();
        });
        let execution = crate::execution::ServerExecution::start().unwrap();
        let hint = crate::polling_discovery::SessionEndpointHint {
            incarnation,
            socket_path: endpoint.as_path().to_owned(),
        };
        let mut connection = tokio::time::timeout(
            Duration::from_secs(10),
            PrivateConnection::connect(
                &registry,
                hint,
                "synthetic-epoch".into(),
                &execution.decoder,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let baseline =
            tokio::time::timeout(Duration::from_secs(10), connection.next(&execution.decoder))
                .await
                .unwrap()
                .unwrap();
        assert!(
            matches!(baseline.view(), SubscriptionObservation::Activity(PollingFeedEvent::Batch(batch)) if batch.source_end == 9 && batch.binding.generation == 2)
        );
        drop(baseline);
        tasks.join_next().await.unwrap().unwrap();
        drop(publication);
        assert!(
            connection.next(&execution.decoder).await.is_err(),
            "a held observation handle cannot extend publisher authority"
        );
        assert!(
            connection.next(&execution.decoder).await.is_err(),
            "lost native authority permanently closes this connection"
        );
    }
}
