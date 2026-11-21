//! Bounded, charged intake for a private polling subscription.
use ilium_execution::Retained;
use ilium_ipc::{FrameReader, IpcError, ServerEvent};
use tokio::io::AsyncRead;

/// The owner must abandon the stream after any error or cancellation; a
/// partially consumed frame is never resumed as a fresh header.
pub(crate) async fn read_event<R: AsyncRead + Unpin>(
    reader: &mut FrameReader<R>,
    execution: &crate::execution::ExecutionClient,
) -> Result<Retained<ServerEvent>, IpcError> {
    use ilium_execution::{JobCost, Lane};
    use std::time::Duration;

    // An idle feed holds neither a decoder job nor its maximum result budget.
    let length = reader.read_encoded_length().await?;
    if length > 2 * 1024 * 1024 {
        return Err(IpcError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "private polling frame exceeds 2 MiB",
        )));
    }
    // A short wire representation can expand substantially during decoding.
    // Reuse the audited closed IPC graph's complete declaration, then reduce
    // to actual completed storage; the wire limit is not an allocation bound.
    let reservation = tokio::time::timeout(
        Duration::from_secs(15),
        execution.reserve(
            Lane::Cpu,
            JobCost {
                input_bytes: reader.encoded_payload_input_bytes(length)?,
                result_bytes: crate::execution::SERVER_DECODER_RESULT_BYTES,
            },
        ),
    )
    .await
    .map_err(|_| intake_error("private polling decoder admission timed out"))?
    .map_err(|error| {
        intake_error(format!(
            "private polling decoder admission refused: {error:?}"
        ))
    })?;
    let frame = tokio::time::timeout(Duration::from_secs(15), reader.read_encoded_payload(length))
        .await
        .map_err(|_| intake_error("private polling frame body timed out"))??;
    // Physical decoding keeps the original frame charge even if its async
    // owner is cancelled before the worker starts or while it is running.
    let charged_frame = (frame, reservation.retention());
    let decoded = execution
        .run_reserved(reservation, move |_| {
            let result =
                ilium_ipc::decode_bounded_frame_with_storage::<ServerEvent>(&charged_frame.0);
            drop(charged_frame);
            result
        })
        .await
        .map_err(|error| intake_error(error.to_string()))?;
    let bytes = ilium_execution::retained_result_storage_bytes::<ServerEvent>(decoded.view().1)
        .map_err(|error| {
            intake_error(format!("private polling storage sizing refused: {error:?}"))
        })?;
    let mut decoded = decoded.map(|(event, _)| event);
    decoded
        .try_reduce_to_result_storage(bytes)
        .map_err(|error| {
            intake_error(format!(
                "private polling storage transfer refused: {error:?}"
            ))
        })?;
    Ok(decoded)
}

fn intake_error(message: impl Into<String>) -> IpcError {
    IpcError::Io(std::io::Error::other(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ipc::{polling::PollingReply, FrameWriter};
    use std::time::Duration;

    // Dropping decoded custody before the consumer finishes must lose the
    // measurable charge; retaining a CPU job instead must fail the jobs check.
    #[tokio::test]
    async fn polling_feed_transport_keeps_decoded_storage_without_worker_credit() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let before = execution.decoder.quota_group().snapshot();
        let (writer, reader) = tokio::io::duplex(4096);
        let sent = tokio::spawn(async move {
            FrameWriter::new(writer)
                .write(&ServerEvent::Polling {
                    request_id: 81,
                    reply: PollingReply::EvidenceSubscribed {
                        server_incarnation: "synthetic-server".into(),
                    },
                })
                .await
        });
        let value = read_event(&mut FrameReader::new(reader), &execution.decoder)
            .await
            .unwrap();
        sent.await.unwrap().unwrap();
        assert!(
            matches!(value.view(), ServerEvent::Polling { request_id: 81, reply: PollingReply::EvidenceSubscribed { server_incarnation } } if server_incarnation == "synthetic-server")
        );
        let held = execution.decoder.quota_group().snapshot();
        assert!(held.result_bytes > before.result_bytes);
        assert_eq!(held.jobs, before.jobs);
        drop(value);
        assert_eq!(
            execution.decoder.quota_group().snapshot().result_bytes,
            before.result_bytes
        );
    }

    // Header intake must remain idle without occupying a finite decoder slot.
    #[tokio::test]
    async fn polling_feed_transport_idle_header_holds_no_decoder_admission() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let before = execution.decoder.quota_group().snapshot();
        let (_writer, reader) = tokio::io::duplex(64);
        let mut reader = FrameReader::new(reader);
        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            read_event(&mut reader, &execution.decoder)
        )
        .await
        .is_err());
        let after = execution.decoder.quota_group().snapshot();
        assert_eq!(after.jobs, before.jobs);
        assert_eq!(after.result_bytes, before.result_bytes);
        assert_eq!(after.input_bytes, before.input_bytes);
    }

    // An oversized declaration must be refused from its header, even if its
    // producer cannot finish sending a body through this tiny transport.
    #[tokio::test]
    async fn polling_feed_transport_refuses_oversized_header_before_body() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let (writer, reader) = tokio::io::duplex(64);
        let sent = tokio::spawn(async move {
            FrameWriter::new(writer)
                .write(&ServerEvent::Polling {
                    request_id: 81,
                    reply: PollingReply::EvidenceSubscribed {
                        server_incarnation: "x".repeat(2 * 1024 * 1024 + 1),
                    },
                })
                .await
        });
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            read_event(&mut FrameReader::new(reader), &execution.decoder),
        )
        .await;
        sent.abort();
        let _ = sent.await;
        let error = match outcome.unwrap() {
            Ok(_) => panic!("oversized private feed accepted"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("private polling frame exceeds"),
            "{error}"
        );
    }
}
