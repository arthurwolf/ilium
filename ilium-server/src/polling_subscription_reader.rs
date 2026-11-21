//! Charged wire-sequence consumption after native serving-peer admission.
//! This reader cannot authenticate a transport or authorize paid dispatch.
use ilium_execution::Retained;
use ilium_ipc::{polling::SubscriptionObservation, IpcError};
use tokio::io::AsyncRead;

pub(crate) struct SubscriptionReader<R> {
    reader: ilium_ipc::FrameReader<R>,
    gate: Option<ilium_ipc::polling::SubscriptionGate>,
    storage: std::sync::Arc<ilium_execution::StorageAdmission>,
}

impl<R: AsyncRead + Unpin> SubscriptionReader<R> {
    pub(crate) fn new(
        reader: R,
        request_id: u64,
        incarnation: String,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<Self, IpcError> {
        let storage = execution
            .try_reserve_storage(16 * 1024)
            .map_err(|error| reader_error(format!("polling reader storage refused: {error:?}")))?;
        let gate = ilium_ipc::polling::SubscriptionGate::new(request_id, incarnation)
            .ok_or_else(|| reader_error("invalid polling subscription identity"))?;
        Ok(Self {
            reader: ilium_ipc::FrameReader::new(reader),
            gate: Some(gate),
            storage,
        })
    }

    pub(crate) async fn next(
        &mut self,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<Retained<SubscriptionObservation>, IpcError> {
        use ilium_execution::{JobCost, Lane};
        use std::time::Duration;

        // Taking the gate before any await makes cancellation permanently close
        // this stream, including cancellation after a partially consumed header.
        let mut gate = self
            .gate
            .take()
            .ok_or_else(|| reader_error("polling subscription is closed"))?;
        let decoded =
            crate::polling_feed_transport::read_event(&mut self.reader, execution).await?;
        let reservation = tokio::time::timeout(
            Duration::from_secs(15),
            execution.reserve(
                Lane::Cpu,
                JobCost {
                    input_bytes: 16 * 1024,
                    result_bytes: 16 * 1024,
                },
            ),
        )
        .await
        .map_err(|_| reader_error("polling validation admission timed out"))?
        .map_err(|error| {
            reader_error(format!("polling validation admission refused: {error:?}"))
        })?;
        let storage = std::sync::Arc::clone(&self.storage);
        let checked = execution
            .run_reserved(reservation, move |_| {
                let (event, retention) = decoded.into_parts();
                let observation = retention.retain(gate.observe(event));
                // The bounded gate remains covered even if its async owner drops.
                Ok::<_, IpcError>((gate, observation, storage))
            })
            .await
            .map_err(|error| reader_error(error.to_string()))?;
        let ((gate, observation, storage), outer_retention) = checked.into_parts();
        if !matches!(
            observation.view(),
            SubscriptionObservation::Rejected
                | SubscriptionObservation::Activity(ilium_ipc::polling::PollingFeedEvent::Stopped(
                    _
                ))
        ) {
            self.gate = Some(gate);
        }
        drop(storage);
        drop(outer_retention);
        Ok(observation)
    }
}

fn reader_error(message: impl Into<String>) -> IpcError {
    IpcError::Io(std::io::Error::other(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{polling_detection::AgentBinding, NodeId};
    use ilium_ipc::{
        polling::{PollingActivityBatch, PollingFeedEvent, PollingFeedStopReason, PollingReply},
        FrameWriter, ServerEvent,
    };
    use std::time::Duration;

    fn subscribed(incarnation: &str) -> ServerEvent {
        ServerEvent::Polling {
            request_id: 81,
            reply: PollingReply::EvidenceSubscribed {
                server_incarnation: incarnation.into(),
            },
        }
    }

    #[tokio::test]
    async fn polling_subscription_reader_transfers_original_evidence_custody_and_stops() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let (write, read) = tokio::io::duplex(4096);
        let mut reader =
            SubscriptionReader::new(read, 81, "synthetic-server".into(), &execution.decoder)
                .unwrap();
        let sent = tokio::spawn(async move {
            let mut writer = FrameWriter::new(write);
            writer.write(&subscribed("synthetic-server")).await.unwrap();
            writer
                .write(&ServerEvent::PollingEvidence {
                    subscription_id: 81,
                    event: PollingFeedEvent::Batch(PollingActivityBatch {
                        binding: AgentBinding {
                            session: "synthetic-session".into(),
                            incarnation: "synthetic-server".into(),
                            pane: NodeId(7),
                            generation: 2,
                        },
                        source_start: 17,
                        source_end: 17,
                        contains_gap: false,
                        events: Vec::new(),
                    }),
                })
                .await
                .unwrap();
            writer
                .write(&ServerEvent::PollingEvidence {
                    subscription_id: 81,
                    event: PollingFeedEvent::Stopped(PollingFeedStopReason::AuthorityLost),
                })
                .await
                .unwrap();
        });
        let accepted = reader.next(&execution.decoder).await.unwrap();
        assert!(matches!(
            accepted.view(),
            SubscriptionObservation::Subscribed
        ));
        drop(accepted);
        let baseline = reader.next(&execution.decoder).await.unwrap();
        assert!(
            matches!(baseline.view(), SubscriptionObservation::Activity(PollingFeedEvent::Batch(batch)) if batch.source_start == 17 && batch.source_end == 17 && batch.binding.pane == NodeId(7))
        );
        let held = execution.decoder.quota_group().snapshot();
        assert!(
            held.result_bytes > 0,
            "evidence must stay charged until the consumer finishes"
        );
        assert_eq!(
            held.jobs, 0,
            "completed evidence must release finite worker credits"
        );
        drop(baseline);
        assert_eq!(execution.decoder.quota_group().snapshot().result_bytes, 0);
        let stopped = reader.next(&execution.decoder).await.unwrap();
        assert!(matches!(
            stopped.view(),
            SubscriptionObservation::Activity(PollingFeedEvent::Stopped(
                PollingFeedStopReason::AuthorityLost
            ))
        ));
        drop(stopped);
        assert!(
            reader.next(&execution.decoder).await.is_err(),
            "a stopped stream cannot resume"
        );
        sent.await.unwrap();
    }

    #[tokio::test]
    async fn polling_subscription_reader_rejects_wrong_identity_without_reopening() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let (write, read) = tokio::io::duplex(4096);
        let mut reader =
            SubscriptionReader::new(read, 81, "synthetic-server".into(), &execution.decoder)
                .unwrap();
        let sent = tokio::spawn(async move {
            let mut writer = FrameWriter::new(write);
            writer.write(&subscribed("foreign-server")).await.unwrap();
            writer.write(&subscribed("synthetic-server")).await.unwrap();
        });
        let rejected = reader.next(&execution.decoder).await.unwrap();
        assert!(matches!(rejected.view(), SubscriptionObservation::Rejected));
        drop(rejected);
        assert!(reader.next(&execution.decoder).await.is_err());
        sent.await.unwrap();
    }

    #[tokio::test]
    async fn polling_subscription_reader_cancellation_poisons_the_consumed_stream() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let (_write, read) = tokio::io::duplex(64);
        let mut reader =
            SubscriptionReader::new(read, 81, "synthetic-server".into(), &execution.decoder)
                .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), reader.next(&execution.decoder))
                .await
                .is_err()
        );
        assert!(
            reader.next(&execution.decoder).await.is_err(),
            "cancellation must not reuse an uncertain byte prefix"
        );
        let usage = execution.decoder.quota_group().snapshot();
        assert_eq!(usage.jobs, 0);
        assert_eq!(usage.result_bytes, 0);
    }
}
