//! Explicit allocation custody across worker, observer and transport ownership.
use super::*;
use ilium_core::AllocationSize;
use ilium_execution::StorageAdmission;
use std::{
    mem::{size_of, size_of_val},
    ops::{Deref, DerefMut},
};
pub(super) const RESULT_BYTES: usize = crate::execution::GENERAL_RESULT_BYTES;
const ENVELOPE_BYTES: usize = 4096;
// Scratch is separate from submitted capture handles. Charging every operation's
// full workspace to the general input bank can starve snapshots behind another
// admitted capture even though the process has sufficient storage capacity.
const WORKSPACE_BYTES: usize =
    store::MAX_CATALOG_RETAINED_BYTES * 2 + store::MAX_CATALOG_BYTES as usize;
/// Payload is destroyed before the lease. Shared catalogs clone only this Arc.
pub(crate) struct ResourceMemory<T> {
    pub(super) value: T,
    storage: Arc<StorageAdmission>,
}
impl<T: std::fmt::Debug> std::fmt::Debug for ResourceMemory<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResourceMemory")
            .field("value", &self.value)
            .field("retained_bytes", &self.storage.resident_bytes())
            .finish()
    }
}
impl<T> Deref for ResourceMemory<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T> DerefMut for ResourceMemory<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}
impl<T> ResourceMemory<T> {
    pub(super) fn from_parts(value: T, storage: Arc<StorageAdmission>) -> Self {
        Self { value, storage }
    }
    pub(super) fn map<U>(self, transform: impl FnOnce(T) -> U) -> ResourceMemory<U> {
        let Self { value, storage } = self;
        ResourceMemory {
            value: transform(value),
            storage,
        }
    }
    pub(super) fn into_parts(self) -> (T, Arc<StorageAdmission>) {
        (self.value, self.storage)
    }
}
pub(super) type Eligibility = Arc<ResourceMemory<BTreeMap<InvocationId, InvocationEligibility>>>;
pub(super) type Catalog = Arc<ResourceMemory<ResourceCatalog>>;
pub(super) trait ResourceBytes {
    fn resource_bytes(&self) -> usize;
}
macro_rules! domain{($($kind:ty),* $(,)?)=>{$(impl ResourceBytes for $kind{fn resource_bytes(&self)->usize{self.retained_bytes()}})*};}
domain!(ResourceProjectId,ResourceCatalog,(),PathBuf,BTreeMap<InvocationId,InvocationEligibility>,Vec<PathBuf>,Vec<AgentIdentity>,Vec<ProjectResourceLink>);
impl ResourceBytes for ResourceProcessIdentity {
    fn resource_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.boot_id.capacity())
    }
}
impl ResourceBytes for store::CommittedResourceChange {
    fn resource_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.catalog.heap_bytes())
            .saturating_add(self.result.heap_bytes())
            .saturating_add(self.events.heap_bytes())
    }
}
impl ResourceBytes for ServerEvent {
    fn resource_bytes(&self) -> usize {
        self.retained_bytes()
    }
}
impl ResourceBytes for ResourceReply {
    fn resource_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.catalog.heap_bytes())
            .saturating_add(self.clock.heap_bytes())
            .saturating_add(self.result.heap_bytes())
            .saturating_add(self.reservation.heap_bytes())
            .saturating_add(self.recovery_token.heap_bytes())
            .saturating_add(self.message.heap_bytes())
            .saturating_add(self.local_invocations.heap_bytes())
            .saturating_add(self.active_waits.heap_bytes())
    }
}
/// A busy ledger is momentary mutex contention, not exhausted capacity.
/// Retry only that condition: waiting for quota while retaining a catalog or
/// worker payload could prevent the very release needed to satisfy the wait.
pub(super) async fn reserve_after_contention(
    state: &ServerState,
    bytes: usize,
    context: &str,
) -> Result<Arc<StorageAdmission>, ResourceError> {
    let client = &state
        .execution
        .get()
        .ok_or_else(|| err("unavailable", "Execution unavailable"))?
        .client;
    loop {
        match client.try_reserve_storage(bytes) {
            Ok(storage) => return Ok(storage),
            Err(ilium_execution::RejectReason::Busy) => {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            Err(error) => return Err(err("admission", format!("{context}: {error:?}"))),
        }
    }
}
pub(super) async fn io<T: ResourceBytes + Send + 'static>(
    state: &ServerState,
    action: impl FnOnce() -> Result<T, ResourceError> + Send + 'static,
) -> Result<ResourceMemory<T>, ResourceError> {
    let client = &state
        .execution
        .get()
        .ok_or_else(|| err("unavailable", "Execution service unavailable"))?
        .client;
    // The catalog payload has its own allocation custody before the worker
    // constructs it. Reserving the whole 128 MiB as a job result instead would
    // wait forever while logging or an attaching request retains even 64 KiB
    // of the general client's result allowance.
    let payload_storage =
        reserve_after_contention(state, RESULT_BYTES, "Resource worker allocation refused").await?;
    let workspace_storage = reserve_after_contention(
        state,
        WORKSPACE_BYTES,
        "Resource worker workspace allocation refused",
    )
    .await?;
    let capture_bytes = size_of_val(&action).saturating_add(ENVELOPE_BYTES);
    let admission = client
        .reserve(
            ilium_execution::Lane::Io,
            ilium_execution::JobCost {
                // Captured heap and peak workspace retain workspace_storage;
                // submitted inline closure/lease handles still charge input.
                input_bytes: capture_bytes,
                // Heap payload is covered by payload_storage; the execution
                // result still admits its wrapper and bounded error envelope.
                result_bytes: size_of::<ResourceMemory<Result<T, ResourceError>>>()
                    .saturating_add(ENVELOPE_BYTES),
            },
        )
        .await
        .map_err(|e| err("admission", format!("{e:?}")))?;
    let loaded = client
        .run_reserved(admission, move |_| {
            let mut value = action();
            let retained_bytes = match &value {
                Ok(value) => value.resource_bytes(),
                Err(error) => size_of::<ResourceError>()
                    .saturating_add(error.code.capacity())
                    .saturating_add(error.message.capacity()),
            };
            if retained_bytes.saturating_add(ENVELOPE_BYTES) > RESULT_BYTES {
                value = Err(err(
                    "capacity",
                    "Resource result exceeds its admitted 128 MiB allocation envelope",
                ));
            }
            // Errors retain the same lease too, including after cancellation
            // when only the worker/result slot still owns the operation.
            let retained = ResourceMemory {
                value,
                storage: payload_storage,
            };
            // The callback owns this lease through physical completion, even
            // when its requester has disconnected or cancelled the receipt.
            drop(workspace_storage);
            Ok::<_, std::convert::Infallible>(retained)
        })
        .await
        .map_err(|error| err("worker", error))?;
    let bytes = loaded
        .view()
        .value
        .as_ref()
        .map_err(Clone::clone)?
        .resource_bytes()
        .checked_add(ENVELOPE_BYTES)
        .filter(|n| *n <= RESULT_BYTES)
        .ok_or_else(|| {
            err(
                "capacity",
                "Resource result exceeds its admitted 128 MiB allocation envelope",
            )
        })?;
    // No payload crosses a charge gap. Refuse transient pressure rather than
    // awaiting another lease while retaining a complete worker reservation.
    let storage =
        reserve_after_contention(state, bytes, "Resource retained allocation refused").await?;
    let (payload, hold) = loaded.into_parts();
    let (value, worker_storage) = payload.into_parts();
    let value = value?;
    let retained = ResourceMemory { value, storage };
    drop(hold);
    drop(worker_storage);
    Ok(retained)
}
pub(super) fn committed_catalog(
    committed: ResourceMemory<store::CommittedResourceChange>,
) -> (Catalog, ResourceCommandResult) {
    let (value, storage) = committed.into_parts();
    let store::CommittedResourceChange {
        catalog,
        result,
        events,
        ..
    } = value;
    drop(events);
    (
        Arc::new(ResourceMemory {
            value: catalog,
            storage,
        }),
        result,
    )
}
pub(super) fn publish(
    state: &ServerState,
    event: ResourceMemory<ServerEvent>,
) -> Result<(), ResourceError> {
    let reservation = state
        .events
        .reserve_ordered(event.value.retained_bytes())
        .map_err(|e| {
            err(
                "admission",
                format!("Resource event journal refused publication: {e:?}"),
            )
        })?;
    let (value, storage) = event.into_parts();
    let outcome = reservation
        .commit(value)
        .map(|_| ())
        .map_err(|e| err("publication", format!("{:?}", e.refusal)));
    drop(storage);
    outcome
}
/// The direct writer additionally charges its immutable queued/active copies.
/// Acquire that distinct envelope before relinquishing the producer's lease.
pub(super) async fn send(
    state: &ServerState,
    replies: &DirectEventSender,
    event: ResourceMemory<ServerEvent>,
) -> Result<(), ResourceError> {
    let bytes = DirectEventSender::required_storage_bytes(&event.value)
        .ok_or_else(|| err("capacity", "Resource reply envelope overflow"))?;
    let storage = reserve_after_contention(state, bytes, "Resource reply custody refused").await?;
    let (event, producer) = event.into_parts();
    let result = replies
        .send_with_storage(event, Some(storage))
        .await
        .map_err(|_| {
            err(
                "interrupted",
                "Resource reply receiver closed or refused custody",
            )
        });
    drop(producer);
    result
}
pub(super) type LinkRevision = (ResourceProjectId, ResourceId, u64);
pub(super) type NoticeBindings = BTreeMap<(NodeId, u64), Vec<(ResourceId, u64)>>;
pub(super) fn observer_bytes(
    projected: &Option<Vec<LinkRevision>>,
    delivered: &std::collections::BTreeSet<(ReservationId, u64)>,
    notified: &NoticeBindings,
) -> usize {
    let projected = projected.as_ref().map_or(0, |items| {
        items
            .capacity()
            .saturating_mul(size_of::<LinkRevision>())
            .saturating_add(items.iter().fold(0usize, |bytes, (project, resource, _)| {
                bytes
                    .saturating_add(project.heap_bytes())
                    .saturating_add(resource.heap_bytes())
            }))
    });
    let delivered = delivered.iter().fold(
        delivered.len().saturating_mul(
            size_of::<(ReservationId, u64)>()
                .saturating_mul(16)
                .saturating_add(256),
        ),
        |bytes, (id, _)| bytes.saturating_add(id.heap_bytes()),
    );
    let notified = notified.iter().fold(
        notified.len().saturating_mul(
            size_of::<((NodeId, u64), Vec<(ResourceId, u64)>)>()
                .saturating_mul(16)
                .saturating_add(256),
        ),
        |bytes, (_, bindings)| bytes.saturating_add(bindings.retained_bytes()),
    );
    projected
        .saturating_add(delivered)
        .saturating_add(notified)
        .saturating_add(4096)
}
pub(super) fn reserve_storage(
    state: &ServerState,
    bytes: usize,
) -> Result<Arc<StorageAdmission>, ResourceError> {
    state
        .execution
        .get()
        .ok_or_else(|| err("unavailable", "Execution unavailable"))?
        .client
        .try_reserve_storage(bytes)
        .map_err(|e| {
            err(
                "admission",
                format!("Resource metadata custody refused: {e:?}"),
            )
        })
}
