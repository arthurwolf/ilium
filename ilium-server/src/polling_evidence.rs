//! Process- and generation-fenced incremental polling evidence acquisition.
use std::io;
use std::sync::Arc;

use ilium_agent_session::polling_activity::{IncrementalBatch, IncrementalReader};
use ilium_core::NodeId;
use ilium_execution::{JobCost, Lane, Retained};
use ilium_platform::{file_identity, process_control, process_info, secure_fs};
use thiserror::Error;

use crate::pane::{CacheWaitSource, PaneResource};
use crate::state::ServerState;

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("the pane has no verified supported agent transcript")]
    Unavailable,
    #[error("the agent or transcript identity changed during observation")]
    Fenced,
    #[error("the transcript could not be read safely")]
    Read,
}

/// A cursor may only be reused for the precise accepted agent generation.
pub struct VerifiedCursor {
    generation: u64,
    session_id: String,
    process_id: u32,
    started_at: u64,
    process_identity: Arc<ilium_platform::process_control::PtyProcessIdentity>,
    reader: IncrementalReader,
}

impl VerifiedCursor {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn position(&self) -> u64 {
        self.reader.position()
    }
}

/// Baseline when `cursor` is absent; subsequent calls read only appended
/// bytes. Never retain this cursor or publish events after a failed fence.
/// Move the returned original cursor and batch with their retention guard;
/// cloning an evidence payload requires a separately admitted allocation.
pub async fn read_verified(
    state: &Arc<ServerState>,
    pane: NodeId,
    cursor: Option<Retained<VerifiedCursor>>,
) -> Result<Retained<(VerifiedCursor, IncrementalBatch)>, EvidenceError> {
    let source = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane) else {
            return Err(EvidenceError::Unavailable);
        };
        runtime
            .cache_wait_source()
            .ok_or(EvidenceError::Unavailable)?
    };
    let execution = state
        .execution
        .get()
        .ok_or(EvidenceError::Unavailable)?
        .client
        .clone();
    let result = execution
        .run(
            Lane::Io,
            JobCost {
                // A 256 KiB JSON record can expand into many small Value nodes;
                // use the same conservative native parsing envelope as monitoring.
                input_bytes: 64 * 1024 * 1024,
                // Covers the bounded framer, 4096 hashed dedup identities and
                // up to 512 normalized events including UTF-8 field storage.
                result_bytes: 4 * 1024 * 1024,
            },
            move |context: ilium_execution::JobContext| {
                let observed = if context.stop_requested() {
                    Err(EvidenceError::Unavailable)
                } else {
                    match cursor {
                        Some(cursor) => {
                            let (cursor, retention) = cursor.into_parts();
                            let observed = read_current(&source, Some(cursor));
                            drop(retention);
                            observed
                        }
                        None => read_current(&source, None),
                    }
                };
                Ok::<_, io::Error>((source, observed))
            },
        )
        .await
        .map_err(|_| EvidenceError::Unavailable)?;
    let (source, _) = result.view();
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane) else {
        return Err(EvidenceError::Fenced);
    };
    if !runtime.cache_wait_source_is_current(source) {
        return Err(EvidenceError::Fenced);
    }
    drop(panes);
    let ((source, observed), retention) = result.into_parts();
    drop(source);
    let observed = observed?;
    let bytes = retained_storage_bytes(&observed)?;
    let mut observed = retention.retain(observed);
    observed
        .try_reduce_to_result_storage(bytes)
        .map_err(|_| EvidenceError::Unavailable)?;
    Ok(observed)
}

/// Charge retained backing capacities, not every cursor's maximum parse input.
fn retained_storage_bytes(
    observed: &(VerifiedCursor, IncrementalBatch),
) -> Result<usize, EvidenceError> {
    let (cursor, batch) = observed;
    let mut bytes = std::mem::size_of_val(observed)
        .saturating_add(cursor.session_id.capacity())
        .saturating_add(cursor.reader.retained_storage_bytes())
        .saturating_add(std::mem::size_of::<process_control::PtyProcessIdentity>())
        .saturating_add(2 * std::mem::size_of::<usize>())
        .saturating_add(batch.events.capacity().saturating_mul(std::mem::size_of::<
            ilium_core::polling_detection::ActivityEvent,
        >()));
    for event in &batch.events {
        bytes = bytes.saturating_add(event.text.capacity());
        for field in [&event.timestamp, &event.name, &event.call_id]
            .into_iter()
            .flatten()
        {
            bytes = bytes.saturating_add(field.capacity());
        }
    }
    ilium_execution::retained_result_storage_bytes::<(VerifiedCursor, IncrementalBatch)>(bytes)
        .map_err(|_| EvidenceError::Unavailable)
}

fn read_current(
    source: &CacheWaitSource,
    cursor: Option<VerifiedCursor>,
) -> Result<(VerifiedCursor, IncrementalBatch), EvidenceError> {
    if cursor.as_ref().is_some_and(|cursor| {
        cursor.generation != source.generation
            || cursor.session_id != source.session_id
            || cursor.process_id != source.process.process_id
            || cursor.started_at != source.process.started_at_unix_seconds
            || !Arc::ptr_eq(&cursor.process_identity, &source.process_identity)
    }) {
        return Err(EvidenceError::Fenced);
    }
    if !process_control::captured_process_is_current(&source.process_identity)
        .map_err(|_| EvidenceError::Read)?
    {
        return Err(EvidenceError::Fenced);
    }
    let file =
        secure_fs::open_regular_file(&source.transcript_path).map_err(|_| EvidenceError::Read)?;
    if !process_info::process_has_open_file(source.process.process_id, &file, 4096)
        .map_err(|_| EvidenceError::Read)?
    {
        return Err(EvidenceError::Fenced);
    }
    let identity = file_identity::identity(&file).map_err(|_| EvidenceError::Read)?;
    let (next, batch) = match cursor {
        Some(cursor) => cursor
            .reader
            .read(
                &source.transcript_path,
                &source.process.class,
                &source.session_id,
            )
            .map_err(|_| EvidenceError::Read)?,
        None => (
            IncrementalReader::baseline(&source.transcript_path)
                .map_err(|_| EvidenceError::Read)?,
            IncrementalBatch {
                events: Vec::new(),
                contains_gap: false,
            },
        ),
    };
    if next.identity() != &identity {
        return Err(EvidenceError::Fenced);
    }
    let reopened =
        secure_fs::open_regular_file(&source.transcript_path).map_err(|_| EvidenceError::Read)?;
    if file_identity::identity(&reopened).map_err(|_| EvidenceError::Read)? != identity
        || !process_control::captured_process_is_current(&source.process_identity)
            .map_err(|_| EvidenceError::Read)?
        || !process_info::process_has_open_file(source.process.process_id, &reopened, 4096)
            .map_err(|_| EvidenceError::Read)?
    {
        return Err(EvidenceError::Fenced);
    }
    Ok((
        VerifiedCursor {
            generation: source.generation,
            session_id: source.session_id.clone(),
            process_id: source.process.process_id,
            started_at: source.process.started_at_unix_seconds,
            process_identity: source.process_identity.clone(),
            reader: next,
        },
        batch,
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    // The real acquisition result must keep its execution admission after the
    // native read finishes; cloning the cursor/batch out of it drops that charge.
    #[tokio::test]
    async fn polling_verified_read_keeps_storage_custody_without_a_worker_credit() {
        let fixture = crate::polling_test_support::evidence_fixture().await;
        let state = &fixture.state;
        let pane = fixture.pane;
        let quota = state.execution.get().unwrap().client.quota_group();
        let before = quota.snapshot();
        let observed = read_verified(state, pane, None).await.unwrap();
        let after = quota.snapshot();
        assert!(
            after.result_bytes > before.result_bytes,
            "verified evidence must retain its original storage charge"
        );
        assert_eq!(
            after.jobs, before.jobs,
            "completed reads must release worker credits"
        );
        drop(observed);
        assert_eq!(quota.snapshot().result_bytes, before.result_bytes);
        // The window registry admits 1024 agents. Retaining one small baseline
        // per supported slot must not reserve the maximum parse envelope each.
        let mut baselines = Vec::new();
        for index in 0..1024 {
            let observed = read_verified(state, pane, None).await;
            assert!(
                observed.is_ok(),
                "baseline storage refused at slot {index}: {:?}",
                quota.snapshot()
            );
            baselines.push(observed.unwrap());
        }
        assert_eq!(quota.snapshot().jobs, before.jobs);
        drop(baselines);
        assert_eq!(quota.snapshot().result_bytes, before.result_bytes);
    }
}
