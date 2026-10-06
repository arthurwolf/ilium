//! Owner-thread V8 engine. No isolate, local handle, promise or writable buffer
//! crosses threads; only the watchdog's documented IsolateHandle does.
use crate::{
    error::{AnimationError, Result},
    manifest::AnimationMode,
    package::Package,
};
use ilium_execution::{QuotaGroup, StorageAdmission, WorkerAdmission};
use ilium_platform::owned_worker::{self, OwnedWorker, StopToken, WorkerKind};
use serde_json::Value;
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    ffi::c_void,
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const HEAP_EMERGENCY: usize = 8 * 1024 * 1024;
const PLATFORM_BYTES: usize = 8 * 1024 * 1024;
const WATCHDOG_BYTES: usize = 2 * 1024 * 1024;
struct PlatformState {
    quota: QuotaGroup,
    threads: usize,
    testing: bool,
    _admission: WorkerAdmission,
}
static PLATFORM: OnceLock<Mutex<Option<PlatformState>>> = OnceLock::new();
/// Production bootstrap: call before starting isolate/other application workers.
/// The native platform pool and its declared resident baseline remain charged
/// to this original quota for the process lifetime; no private fallback bank.
pub fn initialize_engine(quota: QuotaGroup, platform_threads: usize) -> Result<()> {
    initialize(quota, platform_threads, false)
}
/// Explicit Rust-harness initialization; never selected by runtime failure.
/// The harness is already threaded, so V8 requires its unprotected test platform.
pub fn initialize_engine_for_tests(quota: QuotaGroup, platform_threads: usize) -> Result<()> {
    initialize(quota, platform_threads, true)
}
fn initialize(quota: QuotaGroup, threads: usize, testing: bool) -> Result<()> {
    if !(1..=4).contains(&threads) {
        return Err(AnimationError::Budget("V8 platform threads".into()));
    }
    let mut state = PLATFORM
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| AnimationError::Runtime("V8 bootstrap poisoned".into()))?;
    if let Some(state) = state.as_ref() {
        if state.threads != threads || state.testing != testing || !state.quota.shares_root(&quota)
        {
            return Err(AnimationError::Runtime(
                "V8 platform already belongs to another quota/configuration".into(),
            ));
        }
        return Ok(());
    }
    let admission = quota
        .reserve_external_worker(
            threads + 1, // include the platform owned-worker join supervisor
            PLATFORM_BYTES
                .checked_mul(threads + 1)
                .ok_or_else(|| AnimationError::Budget("platform bytes overflow".into()))?,
        )
        .map_err(admission_error)?;
    let platform = if testing {
        v8::new_unprotected_default_platform(threads as u32, false)
    } else {
        v8::new_default_platform(threads as u32, false)
    }
    .make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    *state = Some(PlatformState {
        quota,
        threads,
        testing,
        _admission: admission,
    });
    Ok(())
}
fn admission_error(error: ilium_execution::RejectReason) -> AnimationError {
    AnimationError::Budget(format!("V8 admission: {error:?}"))
}
#[derive(Debug, Clone)]
pub struct EngineLimits {
    pub heap_bytes: usize,
    pub backing_bytes: usize,
    pub frame_bytes: usize,
    pub json_bytes: usize,
    pub pending_requests: usize,
    pub pending_bytes: usize,
    pub evaluation_ms: u64,
    pub preparation_ms: u64,
    pub render_ms: u64,
    pub dispose_ms: u64,
}
impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            heap_bytes: 64 * 1024 * 1024,
            backing_bytes: 16 * 1024 * 1024,
            frame_bytes: 8 * 1024 * 1024,
            json_bytes: 256 * 1024,
            pending_requests: 32,
            pending_bytes: 4 * 1024 * 1024,
            evaluation_ms: 500,
            preparation_ms: 10_000,
            render_ms: 100,
            dispose_ms: 100,
        }
    }
}
impl EngineLimits {
    fn validate(&self) -> Result<()> {
        if !(8 * 1024 * 1024..=256 * 1024 * 1024).contains(&self.heap_bytes)
            || self.backing_bytes == 0
            || self.backing_bytes > 256 * 1024 * 1024
            || self.frame_bytes == 0
            || self.frame_bytes > 16 * 1024 * 1024
            || self.json_bytes == 0
            || self.json_bytes > 256 * 1024
            || self.pending_requests == 0
            || self.pending_requests > 64
            || self.pending_bytes == 0
            || self.pending_bytes > 8 * 1024 * 1024
            || [
                self.evaluation_ms,
                self.preparation_ms,
                self.render_ms,
                self.dispose_ms,
            ]
            .iter()
            .any(|ms| *ms == 0 || *ms > 60_000)
        {
            return Err(AnimationError::Budget("invalid V8 engine limits".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateState {
    Pending,
    Ready,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedArrayKind {
    U8,
    F32,
    U16,
    U32,
}
impl TypedArrayKind {
    fn width(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::F32 | Self::U32 => 4,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ArraySpec {
    pub name: String,
    pub kind: TypedArrayKind,
    pub elements: usize,
}
#[derive(Debug)]
pub struct RenderOutput {
    pub metadata: Value,
    pub planes: BTreeMap<String, Vec<u8>>,
    _admission: StorageAdmission,
}
impl RenderOutput {
    pub(crate) fn from_retained_parts(
        metadata: Value,
        planes: BTreeMap<String, Vec<u8>>,
        admission: StorageAdmission,
    ) -> Self {
        Self {
            metadata,
            planes,
            _admission: admission,
        }
    }
    pub(crate) fn into_parts(self) -> (Value, BTreeMap<String, Vec<u8>>, StorageAdmission) {
        (self.metadata, self.planes, self._admission)
    }
}
pub const SERVICE_WIRE_VERSION: u16 = 1; // Version the service tree independently of frame metadata.
pub const SERVICE_BYTE_ORDER: &str = if cfg!(target_endian = "little") {
    "little"
} else {
    "big"
}; // Parent and helper must negotiate this same-host binary ABI.
const SERVICE_PLANES: usize = 48; // Preserve the existing binary-plane ceiling.
const SERVICE_REFERENCES: usize = 64; // Bound transient SDK wrapper projections without creating native handle authority.
const SERVICE_NODES: usize = 4096; // Bound native metadata traversal and reconstruction.
const SERVICE_DEPTH: usize = 32; // Bound recursion before allocating an escaping value.
const PURE_SOURCE_SCRATCH_BYTES: usize = 1024 * 1024; // Match sources::operation_peak for both pure requests before native JSON computation.
const PURE_SOURCE_INPUT_BYTES: usize = 512; // Exactly three primitive options; reject larger trees before math.
const PURE_SOURCE_RESULT_BYTES: usize = 16 * 1024; // Existing fixed-body observer output has a finite result ceiling.
const TEXT_MEASURE_INPUT_BYTES: usize = 17 * 1024; // Bounded primitive text plus font/size keys.
const TEXT_MEASURE_RESULT_BYTES: usize = 512; // Two bounded integer pixel dimensions.
const SERVICE_TAG: &str = "$ilium_binary"; // Reserve a structural marker that never denotes authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)] // Compare only native-issued activation coordinates.
pub struct ServiceAuthority {
    // The private helper session separately binds the immutable package digest.
    pub instance_id: u64,         // Identify the accepted native instance.
    pub plan_generation: u64,     // Identify the accepted native plan revision.
    pub authorization_epoch: u64, // Fence results from an earlier broker epoch.
} // End the activation stamp.
impl ServiceAuthority {
    // Validate stamps without consulting script-readable JSON.
    pub(crate) fn validate(self) -> Result<()> {
        // Reuse this validation in the helper adapter.
        if self.instance_id == 0 || self.plan_generation == 0 || self.authorization_epoch == 0 {
            // Refuse uninitialized coordinates.
            return Err(runtime("invalid native service authority")); // Fail before package execution.
        } // All coordinates are present.
        Ok(()) // This validates shape, not a PermissionBroker grant.
    } // End stamp validation.
} // End native stamp methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)] // Preserve the caller-derived lifecycle phase.
pub enum ServicePhase {
    // These variants do not come from a request payload.
    Create, // Acquisition occurred during native preparation.
    Async,  // Acquisition occurred during a separately authorized pump.
} // End dispatch phase inventory.
pub(crate) struct ServiceBudget {
    // Additional per-instance bounds debit no independent quota bank.
    count: AtomicUsize, // Retain a slot while any native request payload owner survives.
    bytes: AtomicUsize, // Retain wire-byte occupancy through escaped immutable clones.
    max_count: usize,   // Use the original configured pending request limit.
    max_bytes: usize,   // Use the original configured pending byte limit.
    closed: AtomicBool, // Refuse admissions after native retirement.
} // End the local aggregate fence.
impl ServiceBudget {
    // Storage itself is always admitted by the original QuotaGroup.
    pub(crate) fn new(limits: &EngineLimits) -> Arc<Self> {
        // Share these counters with escaped payloads.
        Arc::new(Self {
            count: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            max_count: limits.pending_requests,
            max_bytes: limits.pending_bytes,
            closed: AtomicBool::new(false),
        }) // Start with no occupied slots.
    } // End fence construction.
    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<ServiceLease> {
        // Acquire both dimensions before copying binary data.
        if self.closed.load(Ordering::Acquire) {
            // Native retirement is terminal for this fence.
            return Err(runtime("service admission is closed")); // Never create a replacement budget.
        } // Continue under the original limits.
        self.count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count
                    .checked_add(1)
                    .filter(|total| *total <= self.max_count)
            })
            .map_err(|_| AnimationError::Budget("retained service request count".into()))?; // Include escaped and cancelled holders.
        if self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= self.max_bytes)
            })
            .is_err()
        {
            // Fail before any escaping allocation.
            self.count.fetch_sub(1, Ordering::AcqRel); // Roll back the count reservation exactly once.
            return Err(AnimationError::Budget(
                "retained service request bytes".into(),
            )); // Preserve existing pending payloads.
        } // Both dimensions are now owned.
        let lease = ServiceLease {
            budget: Arc::clone(self),
            bytes,
        }; // Release only through the final payload owner.
        if self.closed.load(Ordering::Acquire) {
            // Cover retirement racing a parent-side admission.
            return Err(runtime("service admission closed during reservation")); // The local lease rolls back on this return.
        } // Admission remains live.
        Ok(lease) // Transfer the reservation into the immutable payload.
    } // End aggregate admission.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    } // Logical retirement does not release existing holders.
} // End aggregate fence methods.
struct ServiceLease {
    budget: Arc<ServiceBudget>,
    bytes: usize,
} // Share the fence, not separately allocated payload copies.
impl Drop for ServiceLease {
    // Counters survive engine/session destruction through the Arc.
    fn drop(&mut self) {
        // Run after the immutable payload's allocation fields are destroyed.
        self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel); // Return the exact admitted wire size.
        self.budget.count.fetch_sub(1, Ordering::AcqRel); // Return its single physical request slot.
    } // End lease release.
} // End retained request accounting.
#[derive(Clone)] // Cloning shares the exact allocation and its original-root admission.
pub struct ServiceValue {
    inner: Arc<ServiceValueData>,
} // Keep all owned buffers behind immutable access.
struct ServiceValueData {
    // Field order releases payloads before their accounting guards.
    metadata: Value, // Binary leaves contain only canonical structural markers.
    arrays: Vec<ArraySpec>, // Record each binary kind and element count once.
    planes: BTreeMap<String, Vec<u8>>, // Own fixed snapshots of the exact logical view ranges.
    json_bytes: usize, // Cache the checked encoded metadata size.
    binary_bytes: usize, // Cache the checked sum of copied plane bytes.
    wire_bytes: usize, // Include bounded descriptor and request-envelope overhead.
    quota: QuotaGroup, // Prove root identity before copying into an isolate.
    _request: Option<ServiceLease>, // Retain pending bounds through the last request payload alias.
    _admission: StorageAdmission, // Release original-root storage after every owned allocation.
} // End immutable payload storage.
impl std::fmt::Debug for ServiceValue {
    // Avoid formatting bulk binary contents in diagnostics.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Expose bounded inventory only.
        formatter
            .debug_struct("ServiceValue")
            .field("arrays", &self.inner.arrays)
            .field("json_bytes", &self.inner.json_bytes)
            .field("binary_bytes", &self.inner.binary_bytes)
            .finish() // Keep payload bytes out of logs.
    } // End payload formatting.
} // End payload diagnostics.
impl std::ops::Deref for ServiceValue {
    // Preserve read-only JSON indexing for native consumers.
    type Target = Value; // The structural tree carries no native handle authority.
    fn deref(&self) -> &Value {
        &self.inner.metadata
    } // Never expose mutable or move-out access.
} // End borrowed metadata access.
impl ServiceValue {
    // Native producers must supply already-custodied input slices.
    pub fn metadata(&self) -> &Value {
        &self.inner.metadata
    } // Borrow the immutable structural tree.
    pub fn arrays(&self) -> &[ArraySpec] {
        &self.inner.arrays
    } // Borrow the complete type inventory.
    pub fn planes(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.inner.planes
    } // Borrow planes without detaching their guard.
    pub fn wire_bytes(&self) -> usize {
        self.inner.wire_bytes
    } // Let helper accounting use the same checked cost.
    pub fn binary_bytes(&self) -> usize {
        self.inner.binary_bytes
    } // Expose measured payload size for validation.
    pub(crate) fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.inner.quota.shares_root(quota)
    } // Verify original-parent custody before borrowing completion planes into IPC.
    pub(crate) fn validate_limits(&self, limits: &EngineLimits) -> Result<()> {
        // Recheck immutable producer data against the receiving session's actual configured bounds.
        limits.validate()?; // Reject enlarged or malformed engine ceilings before transport publication.
        service_sizes(
            self.inner.json_bytes,
            self.inner.binary_bytes,
            self.inner.arrays.len(),
            limits,
        )?; // Construction already validated the immutable structure and exact plane shapes.
        Ok(()) // Revalidation copies no data and preserves the existing source admission.
    } // End receiving-session payload validation.
    pub fn copy_from_host(
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
        limits: &EngineLimits,
        quota: QuotaGroup,
    ) -> Result<Self> {
        // Admit a distinct immutable native completion copy.
        Self::copy_parts(metadata, arrays, planes, limits, quota, None) // Do not borrow the producer's allocation lifetime.
    } // End public native-copy construction.
    /// Copy borrowed native planes into an independently admitted immutable result.
    /// The caller retains source custody for this call; clones share only the
    /// resulting allocation and its original-root debit.
    pub fn copy_from_borrowed_host(
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, &[u8]>,
        limits: &EngineLimits,
        quota: QuotaGroup,
    ) -> Result<Self> {
        Self::copy_parts(metadata, arrays, planes, limits, quota, None)
    }

    #[cfg(test)]
    pub(crate) fn copy_from_borrowed_host_with_admission_hook(
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, &[u8]>,
        limits: &EngineLimits,
        quota: QuotaGroup,
        after_admission: impl FnOnce(&StorageAdmission) -> Result<()>,
    ) -> Result<Self> {
        Self::copy_parts_inner(
            metadata,
            arrays,
            planes,
            limits,
            quota,
            None,
            after_admission,
        )
    }

    pub(crate) fn copy_request_from_host(
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
        limits: &EngineLimits,
        quota: QuotaGroup,
        budget: &Arc<ServiceBudget>,
    ) -> Result<Self> {
        // Revalidate helper ingress under the original parent root.
        Self::copy_parts(metadata, arrays, planes, limits, quota, Some(budget)) // Charge pending bounds before cloning data.
    } // End parent-side request construction.
    fn copy_parts<B: AsRef<[u8]>>(
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, B>,
        limits: &EngineLimits,
        quota: QuotaGroup,
        budget: Option<&Arc<ServiceBudget>>,
    ) -> Result<Self> {
        Self::copy_parts_inner(metadata, arrays, planes, limits, quota, budget, |_| Ok(()))
    }

    fn copy_parts_inner<B, F>(
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, B>,
        limits: &EngineLimits,
        quota: QuotaGroup,
        budget: Option<&Arc<ServiceBudget>>,
        after_admission: F,
    ) -> Result<Self>
    where
        B: AsRef<[u8]>,
        F: FnOnce(&StorageAdmission) -> Result<()>,
    {
        // Share one bounded construction path.
        limits.validate()?; // Reject caller attempts to enlarge configured hard ceilings.
        let (json_bytes, binary_bytes, wire_bytes) =
            validate_service_parts(metadata, arrays, planes, limits)?; // Validate all structure and arithmetic before allocation.
        let request = budget
            .map(|budget| budget.reserve(wire_bytes))
            .transpose()?; // Reserve aggregate request occupancy when applicable.
        let admission = quota
            .reserve_external_storage(service_resident_bytes(
                json_bytes,
                binary_bytes,
                arrays.len(),
            )?)
            .map_err(admission_error)?; // Use the producer's original root before making a copy.
        after_admission(&admission)?;
        let mut copied = BTreeMap::new(); // The new map is covered by the live storage admission.
        for (name, bytes) in planes {
            let bytes = bytes.as_ref();
            // Copy each plane while the source remains borrowed.
            let mut output = Vec::new(); // Allocate only after complete shape and quota validation.
            output
                .try_reserve_exact(bytes.len())
                .map_err(|_| AnimationError::Budget("service copy allocation".into()))?; // Report allocation refusal without publishing a partial payload.
            output.extend_from_slice(bytes); // Copy bytes without numeric coercion or reinterpretation.
            copied.insert(name.clone(), output); // Bind the copied plane to its canonical name.
        } // Every plane has independent destination custody.
        Ok(Self {
            inner: Arc::new(ServiceValueData {
                metadata: metadata.clone(),
                arrays: arrays.to_vec(),
                planes: copied,
                json_bytes,
                binary_bytes,
                wire_bytes,
                quota,
                _request: request,
                _admission: admission,
            }),
        }) // Clones now share this exact admitted allocation.
    } // End native payload construction.
} // End immutable payload API.
#[derive(Clone)] // Share request metadata and binary custody without deep copies.
pub struct HostRequest {
    inner: Arc<HostRequestData>,
} // Only native construction can create a request record.
pub struct HostRequestData {
    // Read-only field access preserves existing consumer spelling.
    pub id: u64,         // Correlation is scoped to this engine and active authority.
    pub method: String,  // The native callback validated this method spelling.
    pub timeout_ms: u64, // This is a bound, never a permission.
    pub package_digest: String, // The immutable Engine package supplies this identity.
    pub authority: ServiceAuthority, // Capture the native activation rather than payload JSON.
    pub phase: ServicePhase, // Record the native acquisition phase for parent verification.
    deadline: Instant,   // Preserve time spent waiting in local queues.
    stop: StopToken,     // Cancellation is a request, not proof that a body has exited.
    pub payload: ServiceValue, // Keep the guard after all owned request-header allocations.
} // End retained request data.
impl std::ops::Deref for HostRequest {
    // Expose immutable fields without a DerefMut escape.
    type Target = HostRequestData; // Do not implement Clone for the underlying owned record.
    fn deref(&self) -> &HostRequestData {
        &self.inner
    } // Shared references cannot move out charged buffers.
} // End immutable request access.
impl std::fmt::Debug for HostRequest {
    // Keep bounded request diagnostics useful.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Omit bulk content and native tokens.
        formatter
            .debug_struct("HostRequest")
            .field("id", &self.id)
            .field("method", &self.method)
            .field("authority", &self.authority)
            .field("payload", &self.payload)
            .finish() // Report identity and size information.
    } // End request formatting.
} // End request diagnostics.
impl HostRequest {
    // Native same-process monotonic deadline only; never serialized as guest authority.
    pub(crate) fn native_deadline(&self) -> Instant {
        self.inner.deadline
    }

    // Native adapters retain this value through actual native settlement.
    pub fn is_cancelled(&self) -> bool {
        self.stop.is_stopped()
            || self.deadline <= Instant::now()
            || self
                .payload
                .inner
                ._request
                .as_ref()
                .is_some_and(|lease| lease.budget.closed.load(Ordering::Acquire))
    } // Include timeout and whole-instance retirement.
    pub fn stop_token(&self) -> StopToken {
        self.stop.clone()
    } // Pass cooperative cancellation to the actual work owner.
    pub fn remaining_ms(&self) -> u64 {
        self.deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64
    } // The helper must additionally subtract exchange elapsed time before reconstructing a deadline.
    fn stop(&self) {
        self.stop.stop();
    } // Preserve allocations until their real last holder releases them.
    pub(crate) fn from_transport(
        id: u64,
        method: String,
        timeout_ms: u64,
        package_digest: String,
        authority: ServiceAuthority,
        phase: ServicePhase,
        payload: ServiceValue,
    ) -> Result<Self> {
        // Reconstruct only after parent-side authenticated admission.
        authority.validate()?; // Refuse uninitialized activation stamps.
        validate_service_method(&method)?; // Preserve the same method grammar on both sides.
        if id == 0
            || timeout_ms == 0
            || timeout_ms > 60_000
            || package_digest.len() != 64
            || !package_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || payload.inner._request.is_none()
        {
            // Require valid identity, deadline, and retained request accounting.
            return Err(runtime("invalid retained service request")); // Never manufacture an uncharged transport request.
        } // The helper must also compare the digest and active stamp to its native session.
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(timeout_ms))
            .ok_or_else(|| runtime("host deadline overflow"))?; // Bound transport-relative time without accepting script timing authority.
        Ok(Self {
            inner: Arc::new(HostRequestData {
                id,
                method,
                timeout_ms,
                package_digest,
                authority,
                phase,
                deadline,
                stop: StopToken::default(),
                payload,
            }),
        }) // Transfer all retained custody together.
    } // End native request reconstruction.
} // End request lifecycle helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)] // Report ordinary terminal delivery outcomes without retiring unrelated requests.
pub enum CompletionState {
    Delivered,
    Unknown,
    TimedOut,
    Cancelled,
} // Distinguish successful publication from stale/terminal correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Bootstrap,
    Module,
    Plan,
    Preparation,
    Async,
    Completion, // Native result settlement cannot execute package reactions or acquire services.
    Seed,       // Native snapshot installation has a distinct observable delivery gate.
    Diagnostics, // Separate checkpoint-free diagnostic hooks from frame acknowledgement and native protected delivery.
    FrameInventory, // Only the native pre-render inventory window may expose private frame buffers to the embedding engine.
    FrameFinish,    // Only native frame finishing may return private sealed planes.
    Render,
    Acknowledge,
    Dispose,
}
struct Pending {
    resolver: v8::Global<v8::PromiseResolver>,
    request: HostRequest, // Keep immutable input custody through Promise settlement and cancellation.
}
struct Bridge {
    phase: Phase,
    native_deadline: Option<Instant>, // Mirror the current watchdog deadline for synchronous native work.
    pure_source_running: bool,        // Deny nested pure entry while a bridge call owns scratch.
    callback_stop: StopToken, // The engine watchdog and retirement signal synchronous native font work.
    next_id: u64,
    requests: VecDeque<HostRequest>,
    pending: BTreeMap<u64, Pending>,
    cancelled: VecDeque<HostRequest>, // Explicit bounded terminal inventory for the helper parent.
    authority: Option<ServiceAuthority>, // Installed only by a native accepted activation.
    service_budget: Arc<ServiceBudget>, // Remains alive through escaped immutable request payloads.
    quota: QuotaGroup,                // All escaping copies debit the engine's original root.
    package_digest: String, // Native request identity comes from the verified immutable package.
    prototypes: Option<Rc<WirePrototypes>>, // Preserve pristine structural identities before guest execution.
    service_reactions_pending: bool, // Keep completion reactions out of render and acknowledgement checkpoints.
    violation: Option<String>,
    limits: EngineLimits,
    modules: BTreeMap<String, v8::Global<v8::Module>>,
    module_ids: BTreeMap<i32, String>,
}
struct AllocationBudget {
    used: AtomicUsize,
    limit: usize,
    failed: AtomicBool,
}
unsafe extern "C" fn allocate_buffer(budget: &AllocationBudget, length: usize) -> *mut c_void {
    if budget
        .used
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(length)
                .filter(|total| *total <= budget.limit)
        })
        .is_err()
    {
        budget.failed.store(true, Ordering::Release);
        return std::ptr::null_mut();
    }
    let Ok(layout) = Layout::from_size_align(length.max(1), 8) else {
        budget.used.fetch_sub(length, Ordering::AcqRel);
        budget.failed.store(true, Ordering::Release);
        return std::ptr::null_mut();
    };
    // SAFETY: Layout is checked; the matching V8 allocator free callback receives
    // the same pointer/length. Always initialize, including the uninitialized API.
    let pointer = unsafe { alloc_zeroed(layout) };
    if pointer.is_null() {
        budget.used.fetch_sub(length, Ordering::AcqRel);
        budget.failed.store(true, Ordering::Release);
    }
    pointer.cast()
}
unsafe extern "C" fn free_buffer(budget: &AllocationBudget, pointer: *mut c_void, length: usize) {
    if !pointer.is_null() {
        if let Ok(layout) = Layout::from_size_align(length.max(1), 8) {
            unsafe { dealloc(pointer.cast(), layout) };
            budget.used.fetch_sub(length, Ordering::AcqRel);
        }
    }
}
unsafe extern "C" fn drop_allocator(pointer: *const AllocationBudget) {
    unsafe {
        drop(Arc::from_raw(pointer));
    }
}
static ALLOCATOR: v8::RustAllocatorVtable<AllocationBudget> = v8::RustAllocatorVtable {
    allocate: allocate_buffer,
    allocate_uninitialized: allocate_buffer,
    free: free_buffer,
    drop: drop_allocator,
};
struct WatchState {
    deadline: Option<Instant>,
    stopped: bool,
}
struct WatchControl {
    state: Mutex<WatchState>,
    changed: Condvar,
    terminated: AtomicBool,
    heap_failed: AtomicBool,
    callback_stop: StopToken,
    handle: v8::IsolateHandle,
}
unsafe extern "C" fn near_heap(pointer: *mut c_void, current: usize, _initial: usize) -> usize {
    // SAFETY: Engine retains this Arc until the callback is removed and the
    // isolate has been destroyed. The fixed emergency headroom was admitted.
    let control = unsafe { &*pointer.cast::<WatchControl>() };
    let already_failed = control.heap_failed.swap(true, Ordering::AcqRel);
    control.callback_stop.stop();
    control.handle.terminate_execution();
    if already_failed {
        current
    } else {
        current.saturating_add(HEAP_EMERGENCY)
    }
}
fn supervise_watchdog(control: Arc<WatchControl>, stop: StopToken) {
    let mut state = control
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    loop {
        if state.stopped || stop.is_stopped() {
            return;
        }
        match state.deadline {
            None => {
                state = control
                    .changed
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
            }
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    control.terminated.store(true, Ordering::Release);
                    control.callback_stop.stop();
                    control.handle.terminate_execution();
                    state.deadline = None;
                } else {
                    state = control
                        .changed
                        .wait_timeout(state, remaining)
                        .unwrap_or_else(|poison| poison.into_inner())
                        .0;
                }
            }
        }
    }
}
struct StagedSeed {
    id: u64,
    metadata: v8::Global<v8::Value>,
    planes: v8::Global<v8::Object>,
} // Engine-private: no guest reachable reference before explicit activation.
/// Explicitly !Send/!Sync even if a future V8 version makes OwnedIsolate movable.
pub struct Engine {
    isolate: Option<v8::OwnedIsolate>,
    context: Option<v8::Global<v8::Context>>,
    module: Option<v8::Global<v8::Module>>,
    instance: Option<v8::Global<v8::Object>>,
    creation: Option<v8::Global<v8::Promise>>,
    creation_deadline: Option<Instant>,
    package: Arc<Package>,
    limits: EngineLimits,
    bridge: Rc<RefCell<Bridge>>,
    allocation: Arc<AllocationBudget>,
    control: Arc<WatchControl>,
    watchdog: Option<OwnedWorker>,
    _storage: StorageAdmission,
    _heap_adjustment: Option<StorageAdmission>,
    quota: QuotaGroup,
    invalid: bool,
    awaiting_accept: bool,
    seeded_buffers: Vec<v8::Global<v8::ArrayBuffer>>,
    staged_seed: Option<StagedSeed>,
    next_seed_id: u64,
    _owner: PhantomData<Rc<()>>,
}
impl Engine {
    /// Constructs native machinery only: no package/bootstrap JavaScript executes.
    pub fn new(package: Arc<Package>, limits: EngineLimits, quota: QuotaGroup) -> Result<Self> {
        limits.validate()?;
        {
            let state = PLATFORM
                .get()
                .ok_or_else(|| {
                    AnimationError::Runtime(
                        "initialize_engine must precede worker construction".into(),
                    )
                })?
                .lock()
                .map_err(|_| AnimationError::Runtime("V8 bootstrap poisoned".into()))?;
            if !state
                .as_ref()
                .is_some_and(|state| state.quota.shares_root(&quota))
            {
                return Err(AnimationError::Runtime(
                    "engine quota does not own V8 platform".into(),
                ));
            }
        }
        let package_bytes = package
            .files()
            .values()
            .try_fold(0usize, |total, data| total.checked_add(data.len()))
            .ok_or_else(|| AnimationError::Budget("package size overflow".into()))?;
        let storage_bytes = [
            limits.heap_bytes,
            HEAP_EMERGENCY,
            limits.backing_bytes,
            limits.frame_bytes,
            // Escaping service data now owns independent original-root leases instead of this engine-lifetime pool.
            limits.json_bytes.saturating_mul(32),
            package_bytes,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| AnimationError::Budget("engine storage overflow".into()))?;
        let storage = quota
            .reserve_external_storage(storage_bytes)
            .map_err(admission_error)?;
        let watchdog_admission = Arc::new(
            quota
                .reserve_external_worker(1, WATCHDOG_BYTES)
                .map_err(admission_error)?,
        );
        let allocation = Arc::new(AllocationBudget {
            used: AtomicUsize::new(0),
            limit: limits.backing_bytes,
            failed: AtomicBool::new(false),
        });
        // SAFETY: Arc::into_raw owns one allocator reference, reclaimed exactly once
        // by drop_allocator. The vtable exclusively allocates/frees checked layouts.
        let allocator =
            unsafe { v8::new_rust_allocator(Arc::into_raw(Arc::clone(&allocation)), &ALLOCATOR) };
        let mut isolate = v8::Isolate::new(
            v8::CreateParams::default()
                .heap_limits(0, limits.heap_bytes)
                .array_buffer_allocator(allocator.make_shared()),
        );
        let extra_heap = isolate
            .get_heap_statistics()
            .heap_size_limit()
            .saturating_sub(limits.heap_bytes);
        let heap_adjustment = if extra_heap > 0 {
            Some(
                quota
                    .reserve_external_storage(extra_heap)
                    .map_err(admission_error)?,
            )
        } else {
            None
        };
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        isolate.set_allow_atomics_wait(false);
        let callback_stop = StopToken::default();
        let control = Arc::new(WatchControl {
            state: Mutex::new(WatchState {
                deadline: None,
                stopped: false,
            }),
            changed: Condvar::new(),
            terminated: AtomicBool::new(false),
            heap_failed: AtomicBool::new(false),
            callback_stop: callback_stop.clone(),
            handle: isolate.thread_safe_handle(),
        });
        let bridge = Rc::new(RefCell::new(Bridge {
            phase: Phase::Idle,
            native_deadline: None,
            pure_source_running: false,
            callback_stop,
            next_id: 1,
            requests: VecDeque::new(),
            pending: BTreeMap::new(),
            cancelled: VecDeque::new(), // Start with no undelivered terminal records.
            authority: None, // Consent/activation must be bound explicitly before create or pump.
            service_budget: ServiceBudget::new(&limits), // Preserve configured count and byte ceilings.
            quota: quota.clone(), // Share the original ledger rather than constructing another group.
            package_digest: package.digest().to_owned(), // Capture the immutable package identity natively.
            prototypes: None, // Populate from native-created builtin objects in the fresh context.
            service_reactions_pending: false, // No native completion has queued package continuations yet.
            violation: None,
            limits: limits.clone(),
            modules: BTreeMap::new(),
            module_ids: BTreeMap::new(),
        }));
        isolate.set_slot(Rc::clone(&bridge));
        let (context, prototypes) = {
            // Keep captured V8 globals in locals until every fallible construction step succeeds.
            v8::scope!(let scope,&mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            context.set_allow_generation_from_strings(false);
            let prototypes = WirePrototypes::capture(scope)?; // Constructor errors drop these globals before destroying the still-local isolate.
            let dispatch = v8::Function::new(scope, dispatch_host)
                .ok_or_else(|| AnimationError::Runtime("dispatch binding".into()))?;
            let name = v8::String::new(scope, "__ilium_dispatch")
                .ok_or_else(|| AnimationError::Budget("dispatch name".into()))?;
            if context.global(scope).define_own_property(
                scope,
                name.into(),
                dispatch.into(),
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
            ) != Some(true)
            {
                return Err(AnimationError::Runtime("dispatch registration".into()));
            }
            let project = v8::Function::new(scope, pure_geography_project)
                .ok_or_else(|| runtime("geography project binding"))?;
            let observe = v8::Function::new(scope, pure_astronomy_observe)
                .ok_or_else(|| runtime("astronomy observe binding"))?;
            for (name, callback) in [
                ("__ilium_geography_project", project),
                ("__ilium_astronomy_observe", observe),
            ] {
                let key = v8::String::new(scope, name)
                    .ok_or_else(|| runtime("pure source binding name"))?;
                if context.global(scope).define_own_property(
                    scope,
                    key.into(),
                    callback.into(),
                    v8::PropertyAttribute::READ_ONLY
                        | v8::PropertyAttribute::DONT_DELETE
                        | v8::PropertyAttribute::DONT_ENUM,
                ) != Some(true)
                {
                    return Err(runtime("pure source registration"));
                }
            }
            let text_measure = v8::Function::new(scope, native_text_measure)
                .ok_or_else(|| runtime("native text measurement binding"))?;
            let text_measure_name = v8::String::new(scope, "__ilium_text_measure")
                .ok_or_else(|| runtime("native text measurement name"))?;
            if context.global(scope).define_own_property(
                scope,
                text_measure_name.into(),
                text_measure.into(),
                v8::PropertyAttribute::READ_ONLY
                    | v8::PropertyAttribute::DONT_DELETE
                    | v8::PropertyAttribute::DONT_ENUM,
            ) != Some(true)
            {
                return Err(runtime("native text measurement registration"));
            }
            let phase = v8::Function::new(scope, service_phase)
                .ok_or_else(|| runtime("service phase binding"))?; // Let trusted synchronous SDK guards inspect native phase without acquiring anything.
            let phase_name = v8::String::new(scope, "__ilium_service_phase")
                .ok_or_else(|| runtime("service phase name"))?; // Expose only a read-only lifecycle probe.
            if context.global(scope).define_own_property(
                scope,
                phase_name.into(),
                phase.into(),
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
            ) != Some(true)
            {
                return Err(runtime("service phase registration"));
            } // Package code cannot replace native phase evidence.
            let version_name = v8::String::new(scope, "__ilium_service_wire_version")
                .ok_or_else(|| runtime("service version name"))?; // Allow bootstrap to require the concrete binary ABI.
            let version = v8::Integer::new(scope, i32::from(SERVICE_WIRE_VERSION)); // Version 1 uses canonical markers and four typed kinds.
            if context.global(scope).define_own_property(
                scope,
                version_name.into(),
                version.into(),
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
            ) != Some(true)
            {
                return Err(runtime("service version registration"));
            } // Fail bootstrap integration closed on missing native protocol support.
            let global = context.global(scope);
            for name in ["SharedArrayBuffer", "Atomics", "WebAssembly", "ShadowRealm"] {
                let key = v8::String::new(scope, name)
                    .ok_or_else(|| runtime("restricted global name"))?;
                let undefined = v8::undefined(scope);
                if global.define_own_property(
                    scope,
                    key.into(),
                    undefined.into(),
                    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
                ) != Some(true)
                {
                    return Err(runtime("restricted global registration"));
                }
            }
            (v8::Global::new(scope, context), prototypes) // Both local owners are destroyed safely if later worker construction fails.
        };
        let wake = Arc::clone(&control);
        let body = Arc::clone(&control);
        let watchdog = owned_worker::spawn_owned(
            "ilium-v8-watchdog",
            WorkerKind::Cooperative,
            StopToken::default(),
            move || {
                let _retain_until_actual_join = &watchdog_admission;
                wake.changed.notify_all();
            },
            move |stop| {
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                );
                supervise_watchdog(body, stop)
            },
        )?;
        isolate.add_near_heap_limit_callback(near_heap, Arc::as_ptr(&control).cast_mut().cast());
        bridge.borrow_mut().prototypes = Some(Rc::new(prototypes)); // Transfer globals only when the complete Engine will own their teardown.
        Ok(Self {
            isolate: Some(isolate),
            context: Some(context),
            module: None,
            instance: None,
            creation: None,
            creation_deadline: None,
            package,
            limits,
            bridge,
            allocation,
            control,
            watchdog: Some(watchdog),
            _storage: storage,
            _heap_adjustment: heap_adjustment,
            quota,
            invalid: false,
            awaiting_accept: false,
            seeded_buffers: Vec::new(),
            staged_seed: None,
            next_seed_id: 0,
            _owner: PhantomData,
        })
    }
    fn begin(&mut self, phase: Phase, milliseconds: u64) -> Result<()> {
        if self.is_invalid() {
            return Err(AnimationError::Runtime("animation engine retired".into()));
        }
        if self.staged_seed.is_some() && phase != Phase::Seed {
            return Err(runtime("native seed activation or discard required"));
        } // No guest execution/checkpoint while native staged protected inputs are private.
        if self
            .creation_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.cancel();
            return Err(AnimationError::Runtime(
                "animation preparation deadline".into(),
            ));
        }
        if self.bridge.borrow().service_reactions_pending
            && !matches!(
                phase,
                Phase::Preparation
                    | Phase::Async
                    | Phase::Completion
                    | Phase::Seed
                    | Phase::Diagnostics
            )
        {
            return Err(runtime(
                "native service continuations require an authorized pump",
            ));
        } // Prevent an unrelated synchronous checkpoint from draining completion reactions.
        if matches!(phase, Phase::Preparation | Phase::Async)
            && self.bridge.borrow().authority.is_none()
        {
            return Err(runtime("native service authority is not bound"));
        } // Script-readable accepted-plan fields never activate execution.
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(milliseconds))
            .ok_or_else(|| AnimationError::Budget("deadline overflow".into()))?;
        let deadline = self
            .creation_deadline
            .map_or(deadline, |preparation| preparation.min(deadline));
        self.control
            .state
            .lock()
            .map_err(|_| AnimationError::Runtime("watchdog state poisoned".into()))?
            .deadline = Some(deadline);
        {
            let mut bridge = self.bridge.borrow_mut();
            bridge.phase = phase;
            bridge.native_deadline = Some(deadline);
        }
        self.control.changed.notify_all();
        Ok(())
    }
    fn finish<T>(&mut self, result: Result<T>) -> Result<T> {
        self.control
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .deadline = None;
        self.control.changed.notify_all();
        {
            let mut bridge = self.bridge.borrow_mut();
            bridge.phase = Phase::Idle;
            bridge.native_deadline = None;
        }
        let violation = self.bridge.borrow_mut().violation.take();
        let failure = if self.control.terminated.load(Ordering::Acquire) {
            Some("JavaScript deadline exceeded".to_owned())
        } else if self.control.heap_failed.load(Ordering::Acquire)
            || self.allocation.failed.load(Ordering::Acquire)
        {
            Some("JavaScript heap/backing-store budget exceeded".to_owned())
        } else {
            violation
        };
        if let Some(failure) = failure {
            self.cancel();
            return Err(AnimationError::Runtime(failure));
        }
        if result.is_err() {
            self.cancel();
        }
        result
    }
    pub fn is_invalid(&self) -> bool {
        self.invalid
            || self.control.terminated.load(Ordering::Acquire)
            || self.control.heap_failed.load(Ordering::Acquire)
            || self.allocation.failed.load(Ordering::Acquire)
    }
    pub fn cancel(&mut self) {
        self.staged_seed.take();
        let _ = self.detach_seed_buffers(); // No JS; failed cleanup keeps allocator custody until isolate teardown.
        self.invalid = true;
        self.control.callback_stop.stop();
        self.control.handle.terminate_execution();
        let mut bridge = self.bridge.borrow_mut();
        bridge.authority = None; // Retire the native service stamp before dropping local queues.
        bridge.native_deadline = None;
        bridge.service_reactions_pending = false; // A retired engine will never run its queued reactions.
        bridge.service_budget.close(); // Prevent replacement admissions while escaped payloads remain charged.
        for pending in bridge.pending.values() {
            pending.request.stop();
        } // Signal every outstanding native request without claiming body exit.
        bridge.requests.clear();
        bridge.pending.clear();
        bridge.cancelled.clear(); // Whole-instance retirement supersedes undelivered individual terminal records.
    }
    /// Install only trusted Rust-owned facade code, before package evaluation.
    pub fn install_bootstrap(&mut self, source: &str) -> Result<()> {
        if self.module.is_some() {
            return Err(AnimationError::Runtime(
                "bootstrap must precede package load".into(),
            ));
        }
        self.begin(Phase::Bootstrap, self.limits.evaluation_ms)?;
        let result = (|| {
            if source.len() > self.limits.json_bytes {
                return Err(AnimationError::Budget("bootstrap source".into()));
            }
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let source = v8::String::new(scope, source)
                .ok_or_else(|| runtime("bootstrap source allocation"))?;
            let script = v8::Script::compile(scope, source, None)
                .ok_or_else(|| runtime("bootstrap compilation"))?;
            script
                .run(scope)
                .ok_or_else(|| runtime("bootstrap execution"))?;
            let global = context.global(scope);
            for name in [
                "__ilium_host",
                "__ilium_make_frame",
                "__ilium_finish_frame",
                "__ilium_accept_frame",
            ] {
                let key =
                    v8::String::new(scope, name).ok_or_else(|| runtime("bootstrap hook name"))?;
                let value = global
                    .get(scope, key.into())
                    .ok_or_else(|| runtime("bootstrap hook access"))?;
                if value.is_undefined() {
                    return Err(runtime("bootstrap hook missing"));
                }
                if global.define_own_property(
                    scope,
                    key.into(),
                    value,
                    v8::PropertyAttribute::READ_ONLY
                        | v8::PropertyAttribute::DONT_DELETE
                        | v8::PropertyAttribute::DONT_ENUM, // Preserve the bootstrap's nonenumerable mandatory and optional hooks.
                ) != Some(true)
                {
                    return Err(runtime("bootstrap hook sealing"));
                }
            }
            // Legacy test facades need no binary seed. Production hooks are
            // sealed when present, before any package module is evaluated.
            for name in [
                "__ilium_seed_frame",
                "__ilium_frame_buffers",
                "__ilium_take_status",
                "__ilium_configure_ambient",
            ] {
                // Seal trusted private inventory before any package can add a replacement hook.
                let key = v8::String::new(scope, name)
                    .ok_or_else(|| runtime("optional bootstrap hook name"))?;
                let value = global
                    .get(scope, key.into())
                    .ok_or_else(|| runtime("optional bootstrap hook access"))?;
                if (!value.is_undefined() || name == "__ilium_frame_buffers") // Seal absent inventory as undefined so only the original trusted bootstrap can choose the legacy path.
                    && global.define_own_property(
                        scope,
                        key.into(),
                        value,
                        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE | v8::PropertyAttribute::DONT_ENUM, // Preserve the bootstrap's nonenumerable mandatory and optional hooks.
                    ) != Some(true)
                {
                    return Err(runtime("optional bootstrap hook sealing"));
                }
            }
            scope.perform_microtask_checkpoint();
            Ok(())
        })();
        self.finish(result)
    }
    /// This trusted hook runs after bootstrap installation and before module
    /// instantiation/evaluation, so guest top-level code cannot capture the
    /// physical Date or Math.random in pre-rendered mode.
    pub fn configure_ambient(&mut self, mode: AnimationMode, seed: u32) -> Result<()> {
        if self.module.is_some() {
            return Err(runtime("ambient configuration after module load"));
        }
        self.begin(Phase::Bootstrap, self.limits.evaluation_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let global = context.global(scope);
            let hook = function_property(scope, global, "__ilium_configure_ambient")?;
            let mode = v8::String::new(
                scope,
                match mode {
                    AnimationMode::Live => "live",
                    AnimationMode::PreRendered => "pre_rendered",
                },
            )
            .ok_or_else(|| runtime("ambient mode allocation"))?;
            let seed = v8::Number::new(scope, f64::from(seed));
            hook.call(scope, global.into(), &[mode.into(), seed.into()])
                .ok_or_else(|| runtime("ambient configuration rejected"))?;
            scope.perform_microtask_checkpoint();
            Ok(())
        })();
        self.finish(result)
    }
    /// Compile every immutable declared module, then instantiate and evaluate entry.
    /// Relative resolution never consults disk, network, Node or another package.
    pub fn load(&mut self) -> Result<()> {
        if self.module.is_some() {
            return Err(runtime("package already loaded"));
        }
        self.begin(Phase::Module, self.limits.evaluation_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            for (path, bytes) in self.package.files() {
                if path != &self.package.manifest().entry
                    && !(path.starts_with("modules/") && path.ends_with(".mjs"))
                {
                    continue;
                }
                let text = std::str::from_utf8(bytes).map_err(|_| runtime("non-UTF8 module"))?;
                let code = v8::String::new(scope, text)
                    .ok_or_else(|| runtime("module source allocation"))?;
                let name = v8::String::new(scope, path)
                    .ok_or_else(|| runtime("module name allocation"))?;
                let origin = v8::ScriptOrigin::new(
                    scope,
                    name.into(),
                    0,
                    0,
                    false,
                    -1,
                    None,
                    false,
                    false,
                    true,
                    None,
                );
                let mut source = v8::script_compiler::Source::new(code, Some(&origin));
                let module = v8::script_compiler::compile_module(scope, &mut source)
                    .ok_or_else(|| runtime("module compilation"))?;
                let id = module
                    .script_id()
                    .ok_or_else(|| runtime("module identity"))?;
                let mut bridge = self.bridge.borrow_mut();
                bridge.module_ids.insert(id, path.clone());
                bridge
                    .modules
                    .insert(path.clone(), v8::Global::new(scope, module));
            }
            let module = {
                let bridge = self.bridge.borrow();
                let module = bridge
                    .modules
                    .get(&self.package.manifest().entry)
                    .ok_or_else(|| runtime("entry module missing"))?;
                v8::Local::new(scope, module)
            };
            if module.instantiate_module(scope, resolve_module) != Some(true) {
                return Err(runtime("module import resolution"));
            }
            let evaluation = module
                .evaluate(scope)
                .ok_or_else(|| runtime("module evaluation"))?;
            scope.perform_microtask_checkpoint();
            if let Ok(promise) = v8::Local::<v8::Promise>::try_from(evaluation) {
                if promise.state() != v8::PromiseState::Fulfilled {
                    return Err(runtime("module top-level await/error is unsupported"));
                }
            }
            self.module = Some(v8::Global::new(scope, module));
            Ok(())
        })();
        self.finish(result)
    }
    /// Pure planning: native dispatch is unavailable during this phase.
    pub fn plan(
        &mut self,
        settings: &Value,
        mode: AnimationMode,
        environment: &Value,
    ) -> Result<Value> {
        self.begin(Phase::Plan, self.limits.evaluation_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let module = v8::Local::new(
                scope,
                self.module
                    .as_ref()
                    .ok_or_else(|| runtime("package not loaded"))?,
            );
            let namespace = module
                .get_module_namespace()
                .to_object(scope)
                .ok_or_else(|| runtime("module namespace"))?;
            let function = function_property(scope, namespace, "plan")?;
            let settings = json_into(scope, settings, self.limits.json_bytes)?;
            let mode = v8::String::new(
                scope,
                match mode {
                    AnimationMode::Live => "live",
                    AnimationMode::PreRendered => "pre_rendered",
                },
            )
            .ok_or_else(|| runtime("mode"))?;
            let environment = json_into(scope, environment, self.limits.json_bytes)?;
            let returned = function
                .call(
                    scope,
                    namespace.into(),
                    &[settings, mode.into(), environment],
                )
                .ok_or_else(|| runtime("planning threw"))?;
            if returned.is_promise() {
                return Err(runtime("plan must be synchronous"));
            }
            let output = json_out(scope, returned, self.limits.json_bytes)?;
            scope.perform_microtask_checkpoint();
            Ok(output)
        })();
        self.finish(result)
    }
    pub fn start_create(&mut self, settings: &Value, accepted_plan: &Value) -> Result<CreateState> {
        if self.instance.is_some() || self.creation.is_some() {
            return Err(runtime("instance creation already started"));
        }
        self.creation_deadline =
            Instant::now().checked_add(Duration::from_millis(self.limits.preparation_ms));
        self.begin(Phase::Preparation, self.limits.preparation_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let module = v8::Local::new(
                scope,
                self.module
                    .as_ref()
                    .ok_or_else(|| runtime("package not loaded"))?,
            );
            let namespace = module
                .get_module_namespace()
                .to_object(scope)
                .ok_or_else(|| runtime("module namespace"))?;
            let function = function_property(scope, namespace, "create")?;
            let global = context.global(scope);
            let host = property(scope, global, "__ilium_host")?;
            if !host.is_object() {
                return Err(runtime("trusted host facade missing"));
            }
            let settings = json_into(scope, settings, self.limits.json_bytes)?;
            let accepted = json_into(scope, accepted_plan, self.limits.json_bytes)?;
            let returned = function
                .call(scope, namespace.into(), &[host, settings, accepted])
                .ok_or_else(|| runtime("create threw"))?;
            let promise = v8::Local::<v8::Promise>::try_from(returned)
                .map_err(|_| runtime("create must return a Promise"))?;
            self.creation = Some(v8::Global::new(scope, promise));
            scope.perform_microtask_checkpoint();
            match promise.state() {
                v8::PromiseState::Pending => Ok(CreateState::Pending),
                v8::PromiseState::Rejected => Err(runtime("create rejected")),
                v8::PromiseState::Fulfilled => {
                    let instance = promise
                        .result(scope)
                        .to_object(scope)
                        .ok_or_else(|| runtime("create result must be an instance"))?;
                    function_property(scope, instance, "render")?;
                    self.instance = Some(v8::Global::new(scope, instance));
                    self.creation = None;
                    self.creation_deadline = None;
                    Ok(CreateState::Ready)
                }
            }
        })();
        self.finish(result)
    }
    /// Drain bounded completions/microtasks only on the isolate owner, never UI/I/O.
    pub fn pump(&mut self) -> Result<CreateState> {
        self.begin(
            if self.creation.is_some() {
                Phase::Preparation
            } else {
                Phase::Async
            },
            self.limits.evaluation_ms,
        )?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            expire_requests(scope, &self.bridge)?;
            scope.perform_microtask_checkpoint();
            self.bridge.borrow_mut().service_reactions_pending = false; // Only this authorized pump drains native completion continuations.
            if let Some(creation) = &self.creation {
                let promise = v8::Local::new(scope, creation);
                match promise.state() {
                    v8::PromiseState::Pending => Ok(CreateState::Pending),
                    v8::PromiseState::Rejected => Err(runtime("create rejected")),
                    v8::PromiseState::Fulfilled => {
                        let instance = promise
                            .result(scope)
                            .to_object(scope)
                            .ok_or_else(|| runtime("create result must be an instance"))?;
                        function_property(scope, instance, "render")?;
                        self.instance = Some(v8::Global::new(scope, instance));
                        self.creation = None;
                        self.creation_deadline = None;
                        Ok(CreateState::Ready)
                    }
                }
            } else if self.instance.is_some() {
                Ok(CreateState::Ready)
            } else {
                Err(runtime("creation not started"))
            }
        })();
        self.finish(result)
    }
    pub fn take_requests(&mut self) -> Result<Vec<HostRequest>> {
        if self.is_invalid() {
            return Err(runtime("engine retired"));
        }
        let mut bridge = self.bridge.borrow_mut();
        let now = Instant::now();
        let ids: std::collections::BTreeSet<_> = bridge
            .pending
            .iter()
            .filter(|(_, pending)| {
                pending.request.deadline > now && !pending.request.is_cancelled()
            }) // Do not issue an expired, cancelled, or retired retained request.
            .map(|(id, _)| *id)
            .collect();
        Ok(bridge
            .requests
            .drain(..)
            .filter(|request| ids.contains(&request.id))
            .collect())
    }
    pub fn bind_service_authority(
        &mut self,
        package_digest: &str,
        authority: ServiceAuthority,
    ) -> Result<()> {
        // Bind only from the native authenticated activation command.
        authority.validate()?; // Reject zero activation coordinates before inspecting lifecycle state.
        if self.is_invalid() || package_digest != self.package.digest() {
            return Err(runtime(
                "service authority package mismatch or retired engine",
            ));
        } // Bind the stamp to the immutable loaded package.
        let mut bridge = self.bridge.borrow_mut(); // This method executes no JavaScript.
        if bridge.phase != Phase::Idle || self.creation.is_some() || self.instance.is_some() {
            return Err(runtime("service authority must precede instance creation"));
        } // Do not upgrade a running instance from script-visible plan fields.
        if bridge.authority.is_some_and(|current| current != authority) {
            return Err(runtime(
                "service authority replacement requires a fresh engine",
            ));
        } // Retire and recreate when authorization changes.
        bridge.authority = Some(authority); // Install the parent-authenticated coordinates exactly once.
        Ok(()) // The parent broker still authorizes every actual service effect.
    } // End explicit native activation.
    pub fn service_usage(&self) -> (usize, usize) {
        // Expose retained request occupancy for concrete native regression evidence.
        let bridge = self.bridge.borrow(); // Read only the immutable shared budget reference.
        (
            bridge.service_budget.count.load(Ordering::Acquire),
            bridge.service_budget.bytes.load(Ordering::Acquire),
        ) // Independent atomic samples are bounded, not a coherent concurrent snapshot.
    } // End retained occupancy diagnostics.
    pub fn take_cancelled_requests(&mut self) -> Vec<HostRequest> {
        // Drain explicit timeout/cancellation inventory for the helper adapter.
        self.bridge.borrow_mut().cancelled.drain(..).collect() // Every returned request keeps its original storage and request lease.
    } // A whole-instance cancel instead requires the parent's existing retirement path.
    pub fn expire_service_requests(&mut self) -> Result<()> {
        // Reconcile deadlines during nonacquiring helper pagination without running package continuations.
        if self.is_invalid() {
            return Err(runtime("engine retired"));
        } // A retired isolate cannot execute any further native settlement.
        self.begin(Phase::Completion, self.limits.evaluation_ms)?; // Reuse the native-only publication phase and existing watchdog budget.
        let result = (|| {
            // Keep resolver handles on their original isolate owner.
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?; // Borrow the single native isolate.
            v8::scope!(let scope, isolate); // Bound all transient owner-thread handles.
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            ); // Reopen only the original Promise context.
            let scope = &mut v8::ContextScope::new(scope, context); // Resolve within the request's native realm.
            v8::tc_scope!(let scope, scope); // Keep native settlement failures inside the engine transaction.
            expire_requests(scope, &self.bridge) // Queue terminal results and retained cancellation records without a checkpoint.
        })(); // A later separately authorized pump owns every package reaction.
        self.finish(result) // Preserve existing failure handling for genuine V8 settlement errors.
    } // End nonacquiring deadline reconciliation.
    pub fn cancel_request(
        &mut self,
        id: u64,
        authority: ServiceAuthority,
    ) -> Result<CompletionState> {
        // Cancel one request without retiring unrelated work.
        if self.is_invalid() {
            return Err(runtime("engine retired"));
        } // Do not reenter a terminated isolate.
        if self.bridge.borrow().authority != Some(authority) {
            return Err(runtime("stale native service authority"));
        } // A stale caller cannot cancel a current request.
        if !self.bridge.borrow().pending.contains_key(&id) {
            return Ok(CompletionState::Unknown);
        } // Duplicate terminal commands are nonfatal.
        self.begin(Phase::Completion, self.limits.evaluation_ms)?; // Terminal settlement cannot acquire services or run package reactions.
        let result = (|| {
            // Restrict V8 work to the isolate owner and native context.
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?; // Borrow the owned isolate.
            v8::scope!(let scope, isolate); // Establish owner-thread local handle scope.
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            ); // Reopen the native context.
            let scope = &mut v8::ContextScope::new(scope, context); // Set the correct Promise realm.
            v8::tc_scope!(let scope, scope); // Keep V8 failure contained in the engine transaction.
            terminal_service(
                scope,
                &self.bridge,
                id,
                "cancelled",
                "host request cancelled",
            )?; // Queue one inert terminal result and retained cancellation record.
            Ok(CompletionState::Cancelled) // Native body exit remains externally owned.
        })(); // No microtask checkpoint occurs here.
        self.finish(result) // Apply existing engine failure handling to genuine V8 failures only.
    } // End individual native cancellation.
    pub fn complete_service_request(
        &mut self,
        id: u64,
        authority: ServiceAuthority,
        result: ServiceValue,
    ) -> Result<CompletionState> {
        // Deliver one already admitted immutable result without running JavaScript.
        if self.is_invalid() {
            return Err(runtime("engine retired"));
        } // Retired instances accept no result publication.
        let request = {
            // Drop the bridge borrow before validation or V8 operations.
            let bridge = self.bridge.borrow(); // Read the native activation and original request.
            if bridge.authority != Some(authority) {
                return Err(runtime("stale native service authority"));
            } // Never trust an epoch embedded in result JSON.
            let Some(pending) = bridge.pending.get(&id) else {
                return Ok(CompletionState::Unknown);
            }; // Duplicate or already cancelled completions are nonfatal.
            if pending.request.authority != authority {
                return Err(runtime("service completion authority mismatch"));
            } // Match the captured native request stamp.
            pending.request.clone() // Retain input custody through result validation and settlement.
        }; // No V8 handle crosses a thread or escapes this owner.
        if !self.quota.shares_root(&result.inner.quota) {
            return Err(runtime("foreign service result quota"));
        } // Equal configured limits do not establish the original root.
        let expired = request.deadline <= Instant::now(); // Check time before allocating destination buffers.
        let cancelled = request.is_cancelled(); // Honor native cancellation even while the resolver remains pending.
        if !expired && !cancelled {
            // A terminal result is discarded without paying a new V8 copy.
            service_sizes(
                result.inner.json_bytes,
                result.inner.binary_bytes,
                result.inner.arrays.len(),
                &self.limits,
            )?; // Apply this engine's actual result bounds independently of the producer's limits.
            if result.inner.binary_bytes
                > self
                    .limits
                    .backing_bytes
                    .saturating_sub(self.backing_bytes())
            {
                return Err(AnimationError::Budget(
                    "service result backing bytes".into(),
                ));
            } // Preflight the existing allocator before calling V8 allocation APIs.
        } // Existing source custody remains live through every failure return.
        self.begin(Phase::Completion, self.limits.evaluation_ms)?; // Native delivery has its own nonacquiring phase.
        let completed = (|| {
            // Keep all destination handles local to the isolate owner.
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?; // Borrow the single native isolate.
            v8::scope!(let scope, isolate); // Bound the lifetime of all transient V8 handles.
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            ); // Reopen only this engine's context.
            let scope = &mut v8::ContextScope::new(scope, context); // Use the original Promise realm.
            v8::tc_scope!(let scope, scope); // Catch allocation/settlement failure through existing engine handling.
            if expired || request.deadline <= Instant::now() {
                // Include time spent waiting to enter the native context.
                terminal_service(
                    scope,
                    &self.bridge,
                    id,
                    "timeout",
                    "host request deadline exceeded",
                )?; // Settle timeout and notify the native work owner.
                return Ok(CompletionState::TimedOut); // Do not retire other pending requests for ordinary lateness.
            } // Construct only current, admitted result data.
            if cancelled || request.is_cancelled() {
                // Recheck cooperative cancellation before the first destination allocation.
                terminal_service(
                    scope,
                    &self.bridge,
                    id,
                    "cancelled",
                    "host request cancelled",
                )?; // Settle cancellation without exposing the producer's result bytes.
                return Ok(CompletionState::Cancelled); // Other pending requests remain usable.
            } // Current input custody remains retained throughout copying.
            let mut planes = BTreeMap::new(); // At most 48 owner-thread views are temporarily retained.
            for spec in &result.inner.arrays {
                // Copy native planes into separate V8-owned allocations.
                let bytes = &result.inner.planes[&spec.name]; // Constructor validation established the exact shape.
                let buffer = v8::ArrayBuffer::new(scope, bytes.len()); // The original allocator charges every surviving JS alias.
                if !bytes.is_empty() {
                    // Empty attached views need no data pointer.
                    let destination = buffer
                        .data()
                        .ok_or_else(|| runtime("service result allocation"))?; // Require an actual native destination for nonempty content.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            destination.as_ptr().cast::<u8>(),
                            bytes.len(),
                        );
                    } // SAFETY: this fresh exclusive buffer has exactly the checked byte length.
                } // No native source pointer is shared with package JavaScript.
                let view: v8::Local<v8::Value> = match spec.kind {
                    // Preserve the SDK's exact four-kind inventory.
                    TypedArrayKind::U8 => v8::Uint8Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("service U8 result"))?
                        .into(), // Reconstruct byte data exactly.
                    TypedArrayKind::F32 => v8::Float32Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("service F32 result"))?
                        .into(), // Preserve float32 bits without JSON conversion.
                    TypedArrayKind::U16 => v8::Uint16Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("service U16 result"))?
                        .into(), // Preserve native-endian 16-bit elements.
                    TypedArrayKind::U32 => v8::Uint32Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("service U32 result"))?
                        .into(), // Preserve native-endian 32-bit elements.
                }; // These result buffers never enter the frame/seed detachment list.
                planes.insert(spec.name.clone(), view); // Bind the native view to its validated structural slot.
            } // The native source admission still covers overlap with all destination copies.
            let value = service_into(scope, &result.inner.metadata, &planes)?; // Hydrate a strict inert tree without conversion hooks.
            if request.deadline <= Instant::now() {
                // Recheck after bounded copying and metadata reconstruction.
                terminal_service(
                    scope,
                    &self.bridge,
                    id,
                    "timeout",
                    "host request deadline exceeded",
                )?; // Withhold a result that expired during copying.
                return Ok(CompletionState::TimedOut); // Temporary destination allocations remain allocator-accounted until collection.
            } // Publication is still current and occurs without a package checkpoint.
            if request.is_cancelled() {
                // Honor a native cancellation signalled while the bounded copy was in progress.
                terminal_service(
                    scope,
                    &self.bridge,
                    id,
                    "cancelled",
                    "host request cancelled",
                )?; // Withhold the copied result and preserve terminal custody.
                return Ok(CompletionState::Cancelled); // Do not convert cancellation into apparent success.
            } // The parent broker still serializes actual revoke against protected publication.
            let pending = self
                .bridge
                .borrow_mut()
                .pending
                .remove(&id)
                .ok_or_else(|| runtime("service completion lost pending request"))?; // Remove exactly the validated pending resolver.
            self.bridge
                .borrow_mut()
                .requests
                .retain(|queued| queued.id != id); // Suppress an unissued queue entry when native completion wins first.
            let resolver = v8::Local::new(scope, &pending.resolver); // Reopen the resolver on its owner thread.
            resolve_service(scope, resolver, value)?; // Queue reactions while defeating inherited thenable assimilation.
            self.bridge.borrow_mut().service_reactions_pending = true; // Require a separately authorized pump before any synchronous checkpoint.
            Ok(CompletionState::Delivered) // A subsequent current-authority pump runs continuation code.
        })(); // Dropping result after copying releases only this native completion holder.
        self.finish(completed) // Preserve existing watchdog and allocator failure retirement.
    } // End binary completion delivery.
    pub fn complete_request(&mut self, id: u64, result: &Value) -> Result<()> {
        // Retain the JSON-only native caller API as a checked adapter.
        if self.is_invalid() || !self.bridge.borrow().pending.contains_key(&id) {
            return Err(runtime("unknown or retired host completion"));
        } // Refuse stale legacy IDs before allocating any result copy.
        let authority = self
            .bridge
            .borrow()
            .authority
            .ok_or_else(|| runtime("native service authority is not bound"))?; // This adapter never infers authority from a payload.
        let result = ServiceValue::copy_from_host(
            result,
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )?; // Give legacy JSON results the same immutable source custody.
        match self.complete_service_request(id, authority, result)? {
            // Share the no-checkpoint completion path.
            CompletionState::Delivered => Ok(()), // Preserve success for a current JSON-only completion.
            _ => Err(runtime("unknown or terminal host completion")), // Preserve legacy duplicate/late error behavior without poisoning unrelated requests.
        } // Binary-aware helper callers use the explicit CompletionState instead.
    } // End the legacy native result adapter.
    /// Native-only copy phase. No hook/global getter/package function/checkpoint.
    /// Private globals retain exact copied views until activate/discard/cancel.
    pub fn prepare_frame_seed(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<u64> {
        if self.awaiting_accept || self.staged_seed.is_some() || !self.seeded_buffers.is_empty() {
            return Err(runtime("previous frame seed is still owned"));
        }
        validate_array_specs(arrays, planes, self.limits.frame_bytes)?;
        let id = self
            .next_seed_id
            .checked_add(1)
            .ok_or_else(|| runtime("native seed id exhausted"))?;
        self.begin(Phase::Seed, self.limits.render_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope, scope);
            // Built-in JSON parse has no reviver; supplied is native non-Proxy.
            let metadata = json_into(scope, metadata, self.limits.json_bytes)?;
            let supplied = v8::Object::new(scope);
            for spec in arrays {
                let bytes = &planes[&spec.name];
                let buffer = v8::ArrayBuffer::new(scope, bytes.len());
                retain_buffer(scope, buffer, &mut self.seeded_buffers)?;
                if !bytes.is_empty() {
                    let destination = buffer
                        .data()
                        .ok_or_else(|| runtime("seed buffer allocation"))?;
                    // The new V8-owned allocation has exactly bytes.len() bytes,
                    // no JS aliases yet, and is exclusively owned on this thread.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            destination.as_ptr().cast::<u8>(),
                            bytes.len(),
                        );
                    }
                }
                let view: v8::Local<v8::Value> = match spec.kind {
                    TypedArrayKind::U8 => v8::Uint8Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("seed U8 view"))?
                        .into(),
                    TypedArrayKind::F32 => v8::Float32Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("seed F32 view"))?
                        .into(),
                    TypedArrayKind::U16 => v8::Uint16Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("seed U16 view"))?
                        .into(),
                    TypedArrayKind::U32 => v8::Uint32Array::new(scope, buffer, 0, spec.elements)
                        .ok_or_else(|| runtime("seed U32 view"))?
                        .into(),
                };
                let key =
                    v8::String::new(scope, &spec.name).ok_or_else(|| runtime("seed plane name"))?;
                if supplied.create_data_property(scope, key.into(), view) != Some(true) {
                    return Err(runtime("seed plane binding"));
                }
            }
            Ok(StagedSeed {
                id,
                metadata: v8::Global::new(scope, metadata),
                planes: v8::Global::new(scope, supplied),
            })
        })();
        match result {
            Ok(seed) => {
                self.next_seed_id = id;
                self.staged_seed = Some(seed);
                self.finish(Ok(id))
            }
            Err(error) => {
                let _ = self.detach_seed_buffers();
                self.finish(Err(error))
            }
        }
    }
    /// JS hook phase, ONLY after external native authority guard has released.
    /// Caller-supplied integer is lifecycle correlation, never permission proof.
    pub fn activate_frame_seed(&mut self, id: u64) -> Result<()> {
        if self.staged_seed.as_ref().map(|seed| seed.id) != Some(id) {
            return Err(runtime("stale native seed activation"));
        }
        self.begin(Phase::Seed, self.limits.render_ms)?;
        let seed = self
            .staged_seed
            .take()
            .ok_or_else(|| runtime("native seed missing"))?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope, scope);
            let global = context.global(scope);
            let hook = function_property(scope, global, "__ilium_seed_frame")?;
            let metadata = v8::Local::new(scope, &seed.metadata);
            let planes = v8::Local::new(scope, &seed.planes);
            let returned = hook
                .call(scope, global.into(), &[metadata, planes.into()])
                .ok_or_else(|| runtime("binary seed hook threw"))?;
            if returned.is_promise() {
                return Err(runtime("binary seed hook must be synchronous"));
            }
            Ok(()) // NO checkpoint: pending service reactions require later pump.
        })();
        if result.is_err() {
            let _ = self.detach_seed_buffers();
        }
        self.finish(result)
    }
    pub fn discard_frame_seed(&mut self, id: u64) -> Result<()> {
        if self.staged_seed.as_ref().map(|seed| seed.id) != Some(id) {
            return Err(runtime("stale native seed discard"));
        }
        self.begin(Phase::Seed, self.limits.render_ms)?;
        self.staged_seed.take();
        let cleanup = self.detach_seed_buffers();
        self.finish(cleanup) // Native detach only; failed detach retires instance.
    }
    /// Direct engine consumers may perform both phases outside a broker lock.
    /// Protected runtime inputs use the separate authenticated copy/ACK methods.
    pub fn seed_frame(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let id = self.prepare_frame_seed(metadata, arrays, planes)?;
        self.activate_frame_seed(id)
    }
    fn detach_seed_buffers(&mut self) -> Result<()> {
        let buffers = std::mem::take(&mut self.seeded_buffers);
        if buffers.is_empty() {
            return Ok(());
        }
        let isolate = self
            .isolate
            .as_mut()
            .ok_or_else(|| runtime("isolate missing during seed cleanup"))?;
        v8::scope!(let scope, isolate);
        for buffer in buffers {
            let buffer = v8::Local::new(scope, &buffer);
            if !buffer.was_detached()
                && (buffer.detach(None) != Some(true) || !buffer.was_detached())
            {
                return Err(runtime("seed buffer could not be detached"));
            }
        }
        Ok(())
    }
    /// Drain bounded local script diagnostics separately from drawing metadata.
    /// This phase cannot dispatch privileged operations.
    pub fn take_status(&mut self) -> Result<Value> {
        self.begin(Phase::Diagnostics, self.limits.render_ms)?; // This separate nonacquiring JS hook must stay outside protected native publication.
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope, scope);
            let global = context.global(scope);
            let hook = property(scope, global, "__ilium_take_status")?;
            if hook.is_undefined() {
                return Ok(serde_json::json!({"records":[],"dropped":0}));
            }
            let hook = v8::Local::<v8::Function>::try_from(hook)
                .map_err(|_| runtime("diagnostic hook type"))?;
            let value = hook
                .call(scope, global.into(), &[])
                .ok_or_else(|| runtime("diagnostic hook threw"))?;
            json_out(scope, value, (32 * 1024).min(self.limits.json_bytes))
        })();
        self.finish(result)
    }
    /// One synchronous drawing transaction. Expected plane sizes are trusted
    /// caller input and validated before any Rust frame allocation.
    pub fn render(&mut self, context_value: &Value, arrays: &[ArraySpec]) -> Result<RenderOutput> {
        if self.awaiting_accept {
            return Err(runtime("previous render requires logical acknowledgement"));
        }
        let mut names = std::collections::BTreeSet::new();
        let mut frame_bytes = 0usize;
        if arrays.len() > 48 {
            return Err(AnimationError::Budget("frame plane count".into()));
        }
        for spec in arrays {
            if spec.name.is_empty()
                || spec.name.len() > 80
                || !spec
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !names.insert(&spec.name)
            {
                return Err(AnimationError::Budget(
                    "invalid/duplicate frame plane".into(),
                ));
            }
            frame_bytes = frame_bytes
                .checked_add(
                    spec.elements
                        .checked_mul(spec.kind.width())
                        .ok_or_else(|| AnimationError::Budget("frame shape overflow".into()))?,
                )
                .ok_or_else(|| AnimationError::Budget("frame bytes overflow".into()))?;
        }
        if frame_bytes > self.limits.frame_bytes {
            return Err(AnimationError::Budget("frame byte budget".into()));
        }
        self.begin(Phase::Render, self.limits.render_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let global = context.global(scope);
            let make = function_property(scope, global, "__ilium_make_frame")?;
            let context_value = json_into(scope, context_value, self.limits.json_bytes)?;
            let frame_value = make
                .call(scope, global.into(), &[context_value])
                .ok_or_else(|| runtime("frame creation threw"))?;
            let frame = frame_value
                .to_object(scope)
                .ok_or_else(|| runtime("frame must be an object"))?;
            let mut buffers = Vec::new();
            let rendered = (|| {
                with_frame_phase(&self.bridge, Phase::FrameInventory, || {
                    collect_native_frame_buffers(scope, global, frame, &mut buffers)
                })?; // Restore Render on success/error inside final detachment; preserve the running watchdog deadline.
                let instance = v8::Local::new(
                    scope,
                    self.instance
                        .as_ref()
                        .ok_or_else(|| runtime("instance not ready"))?,
                );
                let render = function_property(scope, instance, "render")?;
                // Production facade injects only selected binary inputs into its
                // private context. Legacy test facades use the original JSON.
                let injected_context = property(scope, frame, "context")?;
                let render_context = if injected_context.is_object() {
                    injected_context
                } else {
                    context_value
                };
                let returned = render
                    .call(scope, instance.into(), &[render_context, frame_value])
                    .ok_or_else(|| runtime("render threw"))?;
                if returned.is_promise() {
                    return Err(runtime("render must be synchronous"));
                }
                let finish = function_property(scope, global, "__ilium_finish_frame")?;
                let finished = with_frame_phase(&self.bridge, Phase::FrameFinish, || {
                    // Open the sealed-plane handoff only for the actual native finish call.
                    finish
                        .call(scope, global.into(), &[frame_value])
                        .ok_or_else(|| runtime("frame finishing threw"))?
                        .to_object(scope)
                        .ok_or_else(|| runtime("invalid frame output")) // Preserve existing output validation without a checkpoint.
                })?; // Restore Render on success/error before inspecting output or propagating failure.
                let metadata_value = property(scope, finished, "metadata")?;
                let metadata = json_out(scope, metadata_value, self.limits.json_bytes)?;
                let planes = property(scope, finished, "planes")?
                    .to_object(scope)
                    .ok_or_else(|| runtime("frame planes missing"))?;
                let keys = planes
                    .get_own_property_names(scope, Default::default())
                    .ok_or_else(|| runtime("frame plane keys"))?;
                if keys.length() as usize != arrays.len() {
                    return Err(runtime("unexpected frame plane count"));
                }
                let mut views = Vec::new();
                for spec in arrays {
                    let value = property(scope, planes, &spec.name)?;
                    if !match spec.kind {
                        TypedArrayKind::U8 => value.is_uint8_array(),
                        TypedArrayKind::F32 => value.is_float32_array(),
                        TypedArrayKind::U16 => value.is_uint16_array(),
                        TypedArrayKind::U32 => value.is_uint32_array(),
                    } {
                        return Err(runtime("frame plane type mismatch"));
                    }
                    let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
                        .map_err(|_| runtime("invalid typed plane"))?;
                    let bytes = spec
                        .elements
                        .checked_mul(spec.kind.width())
                        .ok_or_else(|| runtime("frame shape overflow"))?;
                    if view.byte_length() != bytes {
                        return Err(runtime("frame plane shape mismatch"));
                    }
                    let buffer = view
                        .buffer(scope)
                        .ok_or_else(|| runtime("frame plane buffer"))?;
                    retain_buffer(scope, buffer, &mut buffers)?;
                    views.push((spec.name.clone(), view, bytes));
                }
                // All shapes/types/aggregate bounds have been checked, then allocation.
                let admission = self
                    .quota
                    .reserve_external_storage(
                        frame_bytes
                            .saturating_add(serde_json::to_vec(&metadata)?.len().saturating_mul(32))
                            .max(1),
                    )
                    .map_err(admission_error)?;
                let mut output = BTreeMap::new();
                for (name, view, bytes) in views {
                    let mut copied = Vec::new();
                    copied
                        .try_reserve_exact(bytes)
                        .map_err(|_| AnimationError::Budget("frame allocation".into()))?;
                    copied.resize(bytes, 0);
                    if view.copy_contents(&mut copied) != bytes {
                        return Err(runtime("frame copy length"));
                    }
                    output.insert(name, copied);
                }
                scope.perform_microtask_checkpoint();
                Ok(RenderOutput {
                    metadata,
                    planes: output,
                    _admission: admission,
                })
            })();
            // Detach known native frame planes even if script throws, mutates output
            // descriptors, returns a Promise, or supplies malformed frame metadata.
            let mut detach_failed = false;
            for buffer in buffers {
                let buffer = v8::Local::new(scope, &buffer);
                if buffer.detach(None) != Some(true) {
                    detach_failed = true;
                }
            }
            if detach_failed {
                return Err(runtime("frame buffer could not be detached"));
            }
            rendered
        })();
        // Includes injected input aliases even if make_frame/render/finish throws
        // before they appear in the returned-plane object.
        let cleanup = self.detach_seed_buffers();
        let result = self.finish(result.and_then(|output| cleanup.map(|()| output)))?;
        self.awaiting_accept = result
            .metadata
            .get("submitted")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Ok(result)
    }
    /// Logical publication acknowledgement, distinct from terminal emission.
    pub fn accept_frame(&mut self, accepted: bool) -> Result<()> {
        if !self.awaiting_accept {
            return Err(runtime("no submitted frame awaiting acknowledgement"));
        }
        self.begin(Phase::Acknowledge, self.limits.render_ms)?;
        let result = (|| {
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let global = context.global(scope);
            let accept = function_property(scope, global, "__ilium_accept_frame")?;
            let accepted = v8::Boolean::new(scope, accepted);
            let returned = accept
                .call(scope, global.into(), &[accepted.into()])
                .ok_or_else(|| runtime("frame acknowledgement threw"))?;
            if returned.is_promise() {
                return Err(runtime("frame acknowledgement must be synchronous"));
            }
            scope.perform_microtask_checkpoint();
            Ok(())
        })();
        let result = self.finish(result);
        self.awaiting_accept = false;
        result
    }
    /// Trusted diagnostics only; it shares limits and cannot acquire host effects.
    pub fn evaluate_json(&mut self, source: &str) -> Result<Value> {
        self.begin(Phase::Plan, self.limits.evaluation_ms)?;
        let result = (|| {
            if source.len() > self.limits.json_bytes {
                return Err(AnimationError::Budget("diagnostic source".into()));
            }
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let source =
                v8::String::new(scope, source).ok_or_else(|| runtime("diagnostic string"))?;
            let script = v8::Script::compile(scope, source, None)
                .ok_or_else(|| runtime("diagnostic compilation"))?;
            let value = script
                .run(scope)
                .ok_or_else(|| runtime("diagnostic execution"))?;
            let output = json_out(scope, value, self.limits.json_bytes)?;
            scope.perform_microtask_checkpoint();
            Ok(output)
        })();
        self.finish(result)
    }
    pub fn dispose(&mut self) -> Result<()> {
        if self.is_invalid() {
            self.cancel();
            return Ok(());
        }
        if let Err(error) = self.begin(Phase::Dispose, self.limits.dispose_ms) {
            // Pending service reactions may forbid running a disposal checkpoint.
            self.cancel(); // Still perform native retirement and signal outstanding work on every disposal failure.
            return Err(error); // Report skipped guest disposal without executing unauthorized continuations.
        } // Normal bounded guest disposal proceeds only in an allowed lifecycle state.
        let result = (|| {
            let Some(instance) = self.instance.as_ref() else {
                return Ok(());
            };
            let isolate = self
                .isolate
                .as_mut()
                .ok_or_else(|| runtime("isolate missing"))?;
            v8::scope!(let scope,isolate);
            let context = v8::Local::new(
                scope,
                self.context
                    .as_ref()
                    .ok_or_else(|| runtime("context missing"))?,
            );
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope,scope);
            let instance = v8::Local::new(scope, instance);
            let value = property(scope, instance, "dispose")?;
            if value.is_undefined() {
                return Ok(());
            }
            let dispose = v8::Local::<v8::Function>::try_from(value)
                .map_err(|_| runtime("dispose must be callable"))?;
            let returned = dispose
                .call(scope, instance.into(), &[])
                .ok_or_else(|| runtime("dispose threw"))?;
            scope.perform_microtask_checkpoint();
            if let Ok(promise) = v8::Local::<v8::Promise>::try_from(returned) {
                if promise.state() != v8::PromiseState::Fulfilled {
                    return Err(runtime(
                        "dispose must complete within its bounded checkpoint",
                    ));
                }
            }
            Ok(())
        })();
        let result = self.finish(result);
        self.cancel();
        result
    }
    /// Current backing-store allocation, useful for measured resource evidence.
    pub fn backing_bytes(&self) -> usize {
        self.allocation.used.load(Ordering::Acquire)
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel();
        {
            let mut state = self
                .control
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.stopped = true;
            state.deadline = None;
        }
        self.control.changed.notify_all();
        if let Some(watchdog) = self.watchdog.take() {
            let ticket = watchdog.ticket();
            drop(watchdog);
            let _observed = ticket.join_until(Instant::now() + Duration::from_secs(2));
            /* On timeout supervisor retains thread + wake admission until actual join. */
        }
        self.creation.take();
        self.staged_seed.take();
        self.seeded_buffers.clear();
        self.instance.take();
        self.module.take();
        self.context.take();
        {
            let mut bridge = self.bridge.borrow_mut();
            bridge.pending.clear();
            bridge.prototypes.take(); // Release captured V8 globals before destroying their isolate.
            bridge.modules.clear();
            bridge.module_ids.clear();
        }
        if let Some(mut isolate) = self.isolate.take() {
            isolate.remove_near_heap_limit_callback(near_heap, 0);
            isolate.remove_slot::<Rc<RefCell<Bridge>>>();
            drop(isolate);
        }
    }
}
fn runtime(message: &str) -> AnimationError {
    AnimationError::Runtime(message.to_owned())
}
fn validate_array_specs(
    arrays: &[ArraySpec],
    planes: &BTreeMap<String, Vec<u8>>,
    maximum: usize,
) -> Result<()> {
    if arrays.len() > 48 || arrays.len() != planes.len() {
        return Err(AnimationError::Budget("binary seed plane count".into()));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for spec in arrays {
        if spec.name.is_empty()
            || spec.name.len() > 80
            || !spec
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !names.insert(&spec.name)
        {
            return Err(AnimationError::Budget("binary seed plane name".into()));
        }
        let bytes = spec
            .elements
            .checked_mul(spec.kind.width())
            .ok_or_else(|| AnimationError::Budget("binary seed shape overflow".into()))?;
        if planes.get(&spec.name).map(Vec::len) != Some(bytes) {
            return Err(runtime("binary seed shape mismatch"));
        }
        total = total
            .checked_add(bytes)
            .ok_or_else(|| AnimationError::Budget("binary seed byte overflow".into()))?;
    }
    if total > maximum {
        return Err(AnimationError::Budget("binary seed byte limit".into()));
    }
    Ok(())
}
fn validate_service_method(method: &str) -> Result<()> {
    // Reject invalid methods before native service admission.
    if method.is_empty()
        || method.len() > 80
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._".contains(&byte))
    {
        // Preserve the existing closed spelling grammar.
        return Err(runtime("invalid host method")); // Method text cannot select arbitrary native symbols.
    } // The dispatcher must still resolve an explicitly supported method.
    Ok(()) // Spelling validation does not implement a service.
} // End method validation.
fn service_sizes(
    json_bytes: usize,
    binary_bytes: usize,
    planes: usize,
    limits: &EngineLimits,
) -> Result<usize> {
    // Account for wire inventory without serializing binary into JSON.
    let metadata = planes
        .checked_mul(128)
        .and_then(|bytes| bytes.checked_add(512))
        .and_then(|bytes| bytes.checked_add(json_bytes))
        .ok_or_else(|| runtime("service metadata size overflow"))?; // Include canonical specs and bounded request/result headers.
    if metadata > limits.json_bytes {
        return Err(AnimationError::Budget(
            "service metadata and descriptors".into(),
        ));
    } // Leave room inside the existing helper metadata bound.
    metadata
        .checked_add(binary_bytes)
        .filter(|bytes| *bytes <= limits.pending_bytes)
        .ok_or_else(|| AnimationError::Budget("service payload byte limit".into()))
    // Apply the configured total service limit, including metadata.
} // End checked wire-size accounting.
fn service_resident_bytes(json_bytes: usize, binary_bytes: usize, planes: usize) -> Result<usize> {
    // Declare native DOM, map, header, and binary storage conservatively.
    json_bytes
        .checked_mul(32)
        .and_then(|bytes| {
            planes
                .checked_mul(512)
                .and_then(|descriptors| bytes.checked_add(descriptors))
        })
        .and_then(|bytes| bytes.checked_add(binary_bytes))
        .and_then(|bytes| bytes.checked_add(2048))
        .ok_or_else(|| AnimationError::Budget("service resident size overflow".into()))
    // All copies retain this original-root reservation.
} // End resident-storage calculation.
fn service_json_bytes(value: &Value, maximum: usize) -> Result<usize> {
    // Measure encoded metadata without an unbounded intermediate Vec.
    struct Counter {
        bytes: usize,
        maximum: usize,
    } // The writer owns only fixed-size counting state.
    impl std::io::Write for Counter {
        // Reuse serde's exact escaping and numeric representation.
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            // Refuse excessive metadata before growing any destination.
            self.bytes = self
                .bytes
                .checked_add(bytes.len())
                .filter(|total| *total <= self.maximum)
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "service JSON limit")
                })?; // Count every serialized UTF-8 byte.
            Ok(bytes.len()) // No data is buffered or retained.
        } // End bounded counting writes.
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        } // There is no buffered output to flush.
    } // End the allocation-free metadata writer.
    let mut counter = Counter { bytes: 0, maximum }; // Preserve the caller's actual metadata limit.
    serde_json::to_writer(&mut counter, value)?; // Use exact serializer behavior without a byte-array fallback.
    Ok(counter.bytes) // Return the checked canonical encoded size.
} // End metadata measurement.
fn service_references(
    value: &Value,
    arrays: &[ArraySpec],
    seen: &mut [bool; SERVICE_PLANES],
    nodes: &mut usize,
    depth: usize,
) -> Result<()> {
    // Validate structural placeholders without constructing authority.
    if depth > SERVICE_DEPTH || *nodes >= SERVICE_NODES {
        return Err(AnimationError::Budget(
            "service metadata depth/nodes".into(),
        ));
    } // Bound recursive work before entering another node.
    *nodes += 1; // Count the current metadata value exactly once.
    match value {
        // Only JSON's closed data inventory reaches this validator.
        Value::Array(values) => {
            // Preserve every array element and its order.
            if values.len() > SERVICE_NODES.saturating_sub(*nodes) {
                return Err(AnimationError::Budget(
                    "service metadata array nodes".into(),
                ));
            } // Refuse oversized arrays before recursion.
            for value in values {
                service_references(value, arrays, seen, nodes, depth + 1)?;
            } // Check every child and binary reference.
        } // End array metadata validation.
        Value::Object(values) => {
            // A binary leaf is an exact single-field structural object.
            if let Some(reference) = values.get(SERVICE_TAG) {
                // Recognize only the reserved protocol field.
                if values.len() != 1 {
                    return Err(runtime("binary reference has extra fields"));
                } // Prevent mixed metadata and marker objects.
                let name = reference
                    .as_str()
                    .ok_or_else(|| runtime("binary reference name type"))?; // Do not coerce numeric or object references.
                let index = arrays
                    .iter()
                    .position(|spec| spec.name == name)
                    .ok_or_else(|| runtime("unknown binary reference"))?; // Require a declared binary plane.
                if seen[index] {
                    return Err(runtime("duplicate binary reference"));
                } // Every declared copy is used exactly once.
                seen[index] = true; // Mark only structural use, never handle or provenance registration.
                return Ok(()); // A binary leaf has no recursively interpreted children.
            } // Ordinary records must contain no reserved prototype keys.
            if values.len() > SERVICE_NODES.saturating_sub(*nodes) {
                return Err(AnimationError::Budget(
                    "service metadata object nodes".into(),
                ));
            } // Bound native map traversal.
            for (name, value) in values {
                // Validate each complete own metadata entry.
                if matches!(name.as_str(), "__proto__" | "prototype" | "constructor") {
                    return Err(runtime("reserved service metadata key"));
                } // Preserve existing strict data-key restrictions.
                service_references(value, arrays, seen, nodes, depth + 1)?; // Validate nested values without executing code.
            } // Every object child has passed the same bound.
        } // End record metadata validation.
        _ => {} // Primitive JSON values contain no binary references.
    } // Finish this node's closed type case.
    Ok(()) // The caller separately verifies that all declared planes were referenced.
} // End marker graph validation.
fn validate_service_parts<B: AsRef<[u8]>>(
    metadata: &Value,
    arrays: &[ArraySpec],
    planes: &BTreeMap<String, B>,
    limits: &EngineLimits,
) -> Result<(usize, usize, usize)> {
    // Preflight a complete native service payload before copying it.
    if arrays.len() > SERVICE_PLANES || arrays.len() != planes.len() {
        return Err(runtime("service binary plane inventory"));
    } // Refuse omitted or extra binary allocations.
    let mut binary_bytes = 0usize; // Aggregate exact logical plane sizes with checked arithmetic.
    for (index, spec) in arrays.iter().enumerate() {
        // Require deterministic names and one specification per plane.
        if spec.name != format!("b{index}") {
            return Err(runtime("noncanonical service plane name"));
        } // Names never select arbitrary properties or native objects.
        let bytes = spec
            .elements
            .checked_mul(spec.kind.width())
            .ok_or_else(|| AnimationError::Budget("service plane shape overflow".into()))?; // Reject shape multiplication overflow first.
        if planes.get(&spec.name).map(|plane| plane.as_ref().len()) != Some(bytes) {
            return Err(runtime("service binary shape mismatch"));
        } // Require exactly the declared bytes for each kind.
        binary_bytes = binary_bytes
            .checked_add(bytes)
            .filter(|total| *total <= limits.pending_bytes)
            .ok_or_else(|| AnimationError::Budget("service binary byte limit".into()))?;
        // Keep total input/result bytes bounded.
    } // All binary lengths are validated before copying.
    let mut seen = [false; SERVICE_PLANES]; // Use bounded stack storage for reference accounting.
    let mut nodes = 0usize; // Start a fresh metadata complexity budget.
    service_references(metadata, arrays, &mut seen, &mut nodes, 0)?; // Reject malformed markers, deep data, and prototype keys.
    if seen[..arrays.len()].iter().any(|seen| !seen) {
        return Err(runtime("unreferenced service binary plane"));
    } // No unused bulk bytes may cross the boundary.
    let json_bytes = service_json_bytes(metadata, limits.json_bytes)?; // Count exact metadata encoding without allocating its bytes.
    let wire_bytes = service_sizes(json_bytes, binary_bytes, arrays.len(), limits)?; // Include header/spec overhead in admission.
    Ok((json_bytes, binary_bytes, wire_bytes)) // Return only fully checked accounting dimensions.
} // End complete native payload validation.
fn service_into<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: &Value,
    planes: &BTreeMap<String, v8::Local<'s, v8::Value>>,
) -> Result<v8::Local<'s, v8::Value>> {
    // Hydrate only a previously validated immutable metadata tree.
    match value {
        // Rebuild data without JSON.parse or package property hooks.
        Value::Null => Ok(v8::null(scope).into()), // Preserve explicit null values.
        Value::Bool(value) => Ok(v8::Boolean::new(scope, *value).into()), // Preserve primitive booleans.
        Value::Number(value) => Ok(v8::Number::new(
            scope,
            value
                .as_f64()
                .ok_or_else(|| runtime("service number conversion"))?,
        )
        .into()), // Preserve JavaScript's native number representation.
        Value::String(value) => Ok(v8::String::new(scope, value)
            .ok_or_else(|| runtime("service string allocation"))?
            .into()), // Allocate only already bounded primitive text.
        Value::Array(values) => {
            // Allocate a bounded dense native array.
            let array = v8::Array::new(scope, values.len() as i32); // The structural validator bounded length before this cast.
            for (index, value) in values.iter().enumerate() {
                // Populate every own index without prototype setters.
                let key = v8::String::new(scope, &index.to_string())
                    .ok_or_else(|| runtime("service result index"))?; // Construct the exact numeric own-property name.
                let value = service_into(scope, value, planes)?; // Hydrate nested JSON or binary data.
                if array.create_data_property(scope, key.into(), value) != Some(true) {
                    return Err(runtime("service result array_binding"));
                } // Define data directly instead of invoking inherited setters.
            } // Every dense element is now independently populated.
            Ok(array.into()) // Nested arrays retain their ordinary array behavior.
        } // End array reconstruction.
        Value::Object(values) => {
            // Recognize structural binary leaves before building records.
            if let Some(name) = values.get(SERVICE_TAG) {
                // Validation already required an exact single-field marker.
                return planes
                    .get(
                        name.as_str()
                            .ok_or_else(|| runtime("service result reference type"))?,
                    )
                    .copied()
                    .ok_or_else(|| runtime("service result reference missing"));
                // Return only a native-allocated declared view.
            } // Ordinary records receive no inherited accessors or thenable behavior.
            let null = v8::null(scope); // Use a native null prototype for returned records.
            let object = v8::Object::with_prototype_and_properties(scope, null.into(), &[], &[]); // Construct inert data without package code.
            for (name, value) in values {
                // Populate exact metadata keys as own data properties.
                let key =
                    v8::String::new(scope, name).ok_or_else(|| runtime("service result key"))?; // Allocate an already bounded property name.
                let value = service_into(scope, value, planes)?; // Hydrate the complete child value.
                if object.create_data_property(scope, key.into(), value) != Some(true) {
                    return Err(runtime("service result object binding"));
                } // Do not run prototype setters during publication.
            } // Every native record field is now inert data.
            Ok(object.into()) // This object carries no native registry capability by itself.
        } // End object reconstruction.
    } // End JSON and binary type reconstruction.
} // No package callback or microtask runs while native data is installed.
fn resolve_service<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    resolver: v8::Local<'s, v8::PromiseResolver>,
    value: v8::Local<'s, v8::Value>,
) -> Result<()> {
    // Make Promise settlement inert even for a root array or typed view.
    let mut shield = None; // Any temporary thenable shield must disappear before the value reaches package code.
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        // Shield the promise result's root from inherited then getters.
        let prototype = object
            .get_prototype(scope)
            .ok_or_else(|| runtime("service result prototype"))?; // All result objects were freshly constructed by native code.
        let key =
            v8::String::new(scope, "then").ok_or_else(|| runtime("service thenable shield key"))?; // Inspect only the root's own data property.
        let own = object
            .has_own_property(scope, key.into())
            .ok_or_else(|| runtime("service result own then"))?; // A native JSON data field can never contain a callable function.
        if !prototype.is_null() && !own {
            // Null-prototype records and existing own data need no mutation.
            let undefined = v8::undefined(scope); // Temporarily block inherited Object/Array/TypedArray then accessors.
            if object.define_own_property(
                scope,
                key.into(),
                undefined.into(),
                v8::PropertyAttribute::DONT_ENUM,
            ) != Some(true)
            {
                return Err(runtime("service thenable shield"));
            } // Define a configurable native data property without invoking getters.
            shield = Some((object, key)); // Retain only owner-thread handles until synchronous resolve returns.
        } // Preserve legitimate root metadata named then exactly.
    } // Primitive values cannot supply a thenable callback.
    let resolved = resolver.resolve(scope, value); // Read then synchronously and queue reactions under explicit microtask policy.
    if let Some((object, key)) = shield {
        // Restore the exact public array/view shape before any package reaction.
        if object.delete(scope, key.into()) != Some(true) {
            return Err(runtime("service thenable shield removal"));
        } // The configurable own shield is removed without prototype hooks.
    } // Root typed results remain valid strict inputs for a later dispatch.
    if resolved != Some(true) {
        return Err(runtime("host Promise resolution"));
    } // Keep native settlement failure distinct from an ordinary service error.
    Ok(()) // A later native-authorized pump owns package continuation execution.
} // End protected result settlement.
fn terminal_service(
    scope: &mut v8::PinScope,
    bridge: &Rc<RefCell<Bridge>>,
    id: u64,
    code: &str,
    message: &str,
) -> Result<bool> {
    // Settle a single cancelled or expired request without executing its reactions.
    let Some(pending) = bridge.borrow_mut().pending.remove(&id) else {
        return Ok(false);
    }; // Unknown or already terminal requests are harmless duplicates.
    pending.request.stop(); // Signal cancellation while all payload owners retain their allocations.
    {
        // Limit the RefCell borrow to native queue mutation only.
        let mut state = bridge.borrow_mut(); // No V8 operation runs while this borrow is held.
        state.requests.retain(|request| request.id != id); // Suppress queued work that was never issued.
        state.cancelled.push_back(pending.request); // Keep its request lease until the native terminal inventory is drained.
    } // The outbox shares the original immutable request allocation.
    let metadata = serde_json::json!({"ok":false,"error":{"code":code,"message":message}}); // Produce a complete SDK error envelope.
    let value = service_into(scope, &metadata, &BTreeMap::new())?; // Construct inert native error data.
    let resolver = v8::Local::new(scope, &pending.resolver); // Reopen only the owner-thread Promise resolver.
    resolve_service(scope, resolver, value)?; // Resolve once without package execution.
    bridge.borrow_mut().service_reactions_pending = true; // Only a subsequent authorized pump may execute terminal continuations.
    Ok(true) // Actual native bodies remain the external operation owner's responsibility.
} // End individual terminal settlement.
fn service_phase(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
) {
    // Expose a read-only native lifecycle probe for synchronous SDK guards.
    let phase = scope
        .get_slot::<Rc<RefCell<Bridge>>>()
        .map(|bridge| {
            let state = bridge.borrow();
            match state.phase {
                Phase::Seed => 3,
                Phase::FrameInventory => 4,
                Phase::FrameFinish => 5,
                Phase::Preparation if state.authority.is_some() => 1,
                Phase::Async if state.authority.is_some() => 2,
                _ => 0,
            }
        })
        .unwrap_or(0); // Only native entrypoints open seed, inventory, or finish gates; none permits acquisition.
    returned.set(v8::Integer::new(scope, phase).into()); // A script may observe this state but cannot change it.
} // Seed 3, private inventory 4, and sealed-plane finish 5 all remain nonacquiring.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PureSourceKind {
    Project,
    Observe,
}
#[derive(Clone, Copy)]
enum PureSourceFailure {
    Invalid,
    Budget,
    Deadline,
    Phase,
    Unavailable,
    UnsupportedFont,
    UnsupportedGlyph,
}
impl PureSourceFailure {
    fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid_request",
            Self::Budget => "budget_exceeded",
            Self::Deadline => "timeout",
            Self::Phase => "service_phase",
            Self::Unavailable => "native_source_unavailable",
            Self::UnsupportedFont => "unsupported_font",
            Self::UnsupportedGlyph => "unsupported_glyph",
        }
    }
    fn message(self) -> &'static str {
        match self {
            Self::Invalid => "Pure source options are invalid or outside the supported domain.",
            Self::Budget => "Pure source computation exceeded the original engine quota.",
            Self::Deadline => "The current engine deadline expired during pure source computation.",
            Self::Phase => "Pure source computation is unavailable in this native lifecycle phase.",
            Self::Unavailable => "Pure native sources are unavailable in this build.",
            Self::UnsupportedFont => "Only the bundled CascadiaCode-Regular font is supported.",
            Self::UnsupportedGlyph => "The bundled font lacks a requested glyph.",
        }
    }
}
struct PureSourceGuard(Rc<RefCell<Bridge>>);
impl Drop for PureSourceGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().pure_source_running = false;
    }
}
fn native_text_measure(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
) {
    let Some(bridge) = scope.get_slot::<Rc<RefCell<Bridge>>>().cloned() else {
        return;
    };
    let answer = native_text_measure_value(scope, args, &bridge);
    if matches!(answer, Err(PureSourceFailure::Deadline)) {
        bridge.borrow_mut().violation = Some("JavaScript deadline exceeded".into());
    }
    let payload = match answer {
        Ok((width, height)) => {
            serde_json::json!({"ok":true,"value":{"width":width,"height":height}})
        }
        Err(failure) => serde_json::json!({"ok":false,"error":{
            "code": failure.code(), "message": failure.message()
        }}),
    };
    match service_into(scope, &payload, &BTreeMap::new()) {
        Ok(value) => returned.set(value),
        Err(_) => {
            bridge.borrow_mut().violation = Some("native text result publication failed".into())
        }
    }
}
fn native_text_measure_value(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    bridge: &Rc<RefCell<Bridge>>,
) -> std::result::Result<(u32, u32), PureSourceFailure> {
    let (quota, prototypes, deadline, stop) = {
        let mut state = bridge.borrow_mut();
        if state.pure_source_running {
            return Err(PureSourceFailure::Phase);
        }
        if !matches!(
            state.phase,
            Phase::Module
                | Phase::Plan
                | Phase::Preparation
                | Phase::Async
                | Phase::Diagnostics
                | Phase::Render
                | Phase::Acknowledge
                | Phase::Dispose
        ) {
            return Err(PureSourceFailure::Phase);
        }
        let deadline = state.native_deadline.ok_or(PureSourceFailure::Phase)?;
        if Instant::now() >= deadline {
            return Err(PureSourceFailure::Deadline);
        }
        state.pure_source_running = true;
        (
            state.quota.clone(),
            state.prototypes.clone(),
            deadline,
            state.callback_stop.clone(),
        )
    };
    let _guard = PureSourceGuard(Rc::clone(bridge));
    if args.length() != 1 {
        return Err(PureSourceFailure::Invalid);
    }
    let prototypes = prototypes.ok_or(PureSourceFailure::Phase)?;
    let value = v8::Local::new(scope, args.get(0));
    let (input, views, _) =
        service_out(scope, value, None, &prototypes, TEXT_MEASURE_INPUT_BYTES, 0)
            .map_err(|_| PureSourceFailure::Invalid)?;
    if !views.is_empty() {
        return Err(PureSourceFailure::Invalid);
    }
    let Value::Object(options) = input else {
        return Err(PureSourceFailure::Invalid);
    };
    if options.len() != 3 {
        return Err(PureSourceFailure::Invalid);
    }
    if options.get("font").and_then(Value::as_str) != Some("CascadiaCode-Regular") {
        return Err(PureSourceFailure::UnsupportedFont);
    }
    let text = options
        .get("text")
        .and_then(Value::as_str)
        .ok_or(PureSourceFailure::Invalid)?;
    let size = options
        .get("size_px")
        .and_then(Value::as_f64)
        .ok_or(PureSourceFailure::Invalid)?;
    if !size.is_finite() || !(8.0..=128.0).contains(&size) || text.len() > 16_384 {
        return Err(PureSourceFailure::Invalid);
    }
    if stop.is_stopped() || Instant::now() >= deadline {
        return Err(PureSourceFailure::Deadline);
    }
    #[cfg(feature = "native-host")]
    let measured = {
        let media = crate::native_media::NativeMedia::new(
            quota,
            crate::native_media::MediaLimits::default(),
        )
        .map_err(|_| PureSourceFailure::Budget)?;
        let measured = media.measure_text(text, size as f32, &stop);
        if stop.is_stopped() || Instant::now() >= deadline {
            return Err(PureSourceFailure::Deadline);
        }
        measured.map_err(|error| match error {
            AnimationError::Budget(_) => PureSourceFailure::Budget,
            AnimationError::Runtime(ref message)
                if message == "native media: unsupported_glyph" =>
            {
                PureSourceFailure::UnsupportedGlyph
            }
            _ => PureSourceFailure::Invalid,
        })?
    };
    #[cfg(not(feature = "native-host"))]
    let measured = {
        let _ = quota;
        return Err(PureSourceFailure::Unavailable);
    };
    if stop.is_stopped() || Instant::now() >= deadline {
        return Err(PureSourceFailure::Deadline);
    }
    let payload = serde_json::json!({"ok":true,"value":{"width":measured.0,"height":measured.1}});
    service_json_bytes(&payload, TEXT_MEASURE_RESULT_BYTES)
        .map_err(|_| PureSourceFailure::Budget)?;
    Ok(measured)
}
fn pure_geography_project(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    returned: v8::ReturnValue<v8::Value>,
) {
    pure_source_callback(scope, args, returned, PureSourceKind::Project);
}
fn pure_astronomy_observe(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    returned: v8::ReturnValue<v8::Value>,
) {
    pure_source_callback(scope, args, returned, PureSourceKind::Observe);
}
fn pure_source_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
    kind: PureSourceKind,
) {
    let Some(bridge) = scope.get_slot::<Rc<RefCell<Bridge>>>().cloned() else {
        return;
    };
    let answer = pure_source_value(scope, args, &bridge, kind);
    match answer {
        Ok(value) => returned.set(value),
        Err(failure) => {
            if matches!(failure, PureSourceFailure::Deadline) {
                // A guest catch must not turn an expired native call into a successful frame.
                bridge.borrow_mut().violation = Some("JavaScript deadline exceeded".into());
            }
            if kind == PureSourceKind::Observe {
                let payload = serde_json::json!({"ok":false,"error":{
                    "code":failure.code(),"message":failure.message()
                }});
                if let Ok(value) = service_into(scope, &payload, &BTreeMap::new()) {
                    returned.set(value);
                    return;
                }
                bridge.borrow_mut().violation = Some("pure source error publication failed".into());
            }
            if let Some(message) = v8::String::new(scope, failure.message()) {
                let exception = v8::Exception::type_error(scope, message);
                scope.throw_exception(exception);
            } else {
                bridge.borrow_mut().violation =
                    Some("pure source exception allocation failed".into());
            }
        }
    }
}
fn pure_source_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments,
    bridge: &Rc<RefCell<Bridge>>,
    kind: PureSourceKind,
) -> std::result::Result<v8::Local<'s, v8::Value>, PureSourceFailure> {
    let (quota, prototypes, deadline) = {
        let mut state = bridge.borrow_mut();
        if state.pure_source_running {
            return Err(PureSourceFailure::Phase);
        }
        if !matches!(
            state.phase,
            Phase::Module
                | Phase::Plan
                | Phase::Preparation
                | Phase::Async
                | Phase::Diagnostics
                | Phase::Render
                | Phase::Acknowledge
                | Phase::Dispose
        ) {
            return Err(PureSourceFailure::Phase);
        }
        let deadline = state.native_deadline.ok_or(PureSourceFailure::Phase)?;
        if Instant::now() >= deadline {
            return Err(PureSourceFailure::Deadline);
        }
        state.pure_source_running = true;
        (state.quota.clone(), state.prototypes.clone(), deadline)
    }; // Release RefCell before any V8 reflection or native calculation.
    let _guard = PureSourceGuard(Rc::clone(bridge));
    if !cfg!(all(feature = "native-host", feature = "native-network")) {
        return Err(PureSourceFailure::Unavailable);
    }
    let _scratch = quota
        .reserve_external_storage(PURE_SOURCE_SCRATCH_BYTES)
        .map_err(|_| PureSourceFailure::Budget)?;
    if args.length() != 1 {
        return Err(PureSourceFailure::Invalid);
    }
    let prototypes = prototypes.ok_or(PureSourceFailure::Phase)?;
    let input = v8::Local::new(scope, args.get(0));
    let (input, views, _) =
        service_out(scope, input, None, &prototypes, PURE_SOURCE_INPUT_BYTES, 0)
            .map_err(|_| PureSourceFailure::Invalid)?;
    if !views.is_empty() {
        return Err(PureSourceFailure::Invalid);
    }
    let Value::Object(options) = input else {
        return Err(PureSourceFailure::Invalid);
    };
    if options.len() != 3 {
        return Err(PureSourceFailure::Invalid);
    }
    let latitude = options
        .get("latitude")
        .and_then(Value::as_f64)
        .ok_or(PureSourceFailure::Invalid)?;
    let longitude = options
        .get("longitude")
        .and_then(Value::as_f64)
        .ok_or(PureSourceFailure::Invalid)?;
    let calculated = match kind {
        PureSourceKind::Project => {
            let projection = options
                .get("projection")
                .and_then(Value::as_str)
                .ok_or(PureSourceFailure::Invalid)?;
            if !matches!(projection, "equirectangular" | "mercator" | "orthographic") {
                return Err(PureSourceFailure::Invalid);
            }
            pure_source_calculation(kind, 0, latitude, longitude, projection)
        }
        PureSourceKind::Observe => {
            let epoch_ms = options
                .get("epoch_ms")
                .and_then(Value::as_i64)
                .filter(|epoch| epoch.unsigned_abs() <= 9_007_199_254_740_991)
                .ok_or(PureSourceFailure::Invalid)?;
            pure_source_calculation(kind, epoch_ms, latitude, longitude, "")
        }
    };
    if Instant::now() >= deadline {
        return Err(PureSourceFailure::Deadline);
    }
    let calculated = calculated.map_err(|error| match error {
        AnimationError::Budget(_) => PureSourceFailure::Budget,
        _ => PureSourceFailure::Invalid,
    })?;
    if Instant::now() >= deadline {
        return Err(PureSourceFailure::Deadline);
    }
    let payload = if kind == PureSourceKind::Observe {
        serde_json::json!({"ok":true,"value":calculated})
    } else {
        calculated
    };
    service_json_bytes(&payload, PURE_SOURCE_RESULT_BYTES)
        .map_err(|_| PureSourceFailure::Budget)?;
    let value =
        service_into(scope, &payload, &BTreeMap::new()).map_err(|_| PureSourceFailure::Budget)?;
    if Instant::now() >= deadline {
        return Err(PureSourceFailure::Deadline);
    }
    Ok(value) // V8 heap retains the output under Engine::_storage after scratch is released.
}
#[cfg(all(feature = "native-host", feature = "native-network"))]
fn pure_source_calculation(
    kind: PureSourceKind,
    epoch_ms: i64,
    latitude: f64,
    longitude: f64,
    projection: &str,
) -> Result<Value> {
    match kind {
        PureSourceKind::Project => {
            crate::sources::geography::project(latitude, longitude, projection)
        }
        PureSourceKind::Observe => {
            crate::sources::astronomy::observe(epoch_ms, latitude, longitude)
        }
    }
}
#[cfg(not(all(feature = "native-host", feature = "native-network")))]
fn pure_source_calculation(
    _kind: PureSourceKind,
    _epoch_ms: i64,
    _latitude: f64,
    _longitude: f64,
    _projection: &str,
) -> Result<Value> {
    Err(runtime("pure native source build feature unavailable"))
}
struct WirePrototypes {
    // Retain identities before any package or bootstrap JavaScript runs.
    object: v8::Global<v8::Object>, // Permit ordinary records with the original object prototype.
    array: v8::Global<v8::Object>,  // Permit dense arrays with the original array prototype.
    u8: v8::Global<v8::Object>,     // Admit only the exact Uint8Array prototype.
    f32: v8::Global<v8::Object>,    // Admit only the exact Float32Array prototype.
    u16: v8::Global<v8::Object>,    // Admit only the exact Uint16Array prototype.
    u32: v8::Global<v8::Object>,    // Admit only the exact Uint32Array prototype.
} // All globals must be cleared before the owning isolate is destroyed.
impl WirePrototypes {
    // Capture native builtins without reading mutable global constructor names.
    fn capture(scope: &mut v8::PinScope<'_, '_>) -> Result<Self> {
        // Run inside the fresh native context.
        let object = v8::Object::new(scope); // Create an ordinary native object.
        let object = wire_prototype(scope, object)?; // Retain its original prototype identity.
        let array = v8::Array::new(scope, 0); // Create a native array without guest callbacks.
        let array = wire_prototype(scope, array.into())?; // Retain its original prototype identity.
        let buffer = v8::ArrayBuffer::new(scope, 0); // Use one zero-length native buffer for typed prototypes.
        let u8 =
            v8::Uint8Array::new(scope, buffer, 0, 0).ok_or_else(|| runtime("wire U8 prototype"))?; // Construct the exact builtin view.
        let u8 = wire_prototype(scope, u8.into())?; // Retain the original U8 prototype.
        let f32 = v8::Float32Array::new(scope, buffer, 0, 0)
            .ok_or_else(|| runtime("wire F32 prototype"))?; // Construct the exact builtin view.
        let f32 = wire_prototype(scope, f32.into())?; // Retain the original F32 prototype.
        let u16 = v8::Uint16Array::new(scope, buffer, 0, 0)
            .ok_or_else(|| runtime("wire U16 prototype"))?; // Construct the exact builtin view.
        let u16 = wire_prototype(scope, u16.into())?; // Retain the original U16 prototype.
        let u32 = v8::Uint32Array::new(scope, buffer, 0, 0)
            .ok_or_else(|| runtime("wire U32 prototype"))?; // Construct the exact builtin view.
        let u32 = wire_prototype(scope, u32.into())?; // Retain the original U32 prototype.
        Ok(Self {
            object,
            array,
            u8,
            f32,
            u16,
            u32,
        }) // Return only isolate-owner global handles.
    } // Construction executes no package code.
} // Prototype identities are structural restrictions, never authority.
fn wire_prototype(
    scope: &mut v8::PinScope<'_, '_>,
    object: v8::Local<v8::Object>,
) -> Result<v8::Global<v8::Object>> {
    // Capture a known native object's prototype.
    let prototype = object
        .get_prototype(scope)
        .ok_or_else(|| runtime("wire prototype missing"))?; // Read the internal prototype without user property access.
    let prototype =
        v8::Local::<v8::Object>::try_from(prototype).map_err(|_| runtime("wire prototype type"))?; // All captured builtin prototypes are objects.
    Ok(v8::Global::new(scope, prototype)) // Keep the identity alive on this isolate's owner thread.
} // Call only with objects freshly created by native code.
struct WireReference<'s> {
    // A temporary identity substitution carries only untrusted registry lookup data.
    object: v8::Local<'s, v8::Object>, // Keep the original nonproxy SDK wrapper identity on the owner thread.
    metadata: Value,                   // Retain only the validated id and kind string projection.
    json_bytes: usize, // Charge each actual substitution's complete encoded metadata size.
} // Native service registries must independently authenticate the projected identity.
struct WireWalk<'s> {
    // Bound both traversal work and the exact serialized metadata size.
    nodes: usize,                              // Count every recursively visited value.
    json_bytes: usize, // Count canonical serde_json encoding bytes as values are constructed.
    binary_bytes: usize, // Count selected view bytes without copying the backing buffers.
    max_json: usize,   // Preserve the caller's original metadata ceiling.
    max_binary: usize, // Preserve the caller's original aggregate binary ceiling.
    reference_bytes: usize, // Share the original metadata scratch bound between transient projections and the outgoing tree.
    references: Vec<WireReference<'s>>, // Permit only exact original wrapper identities, never proxy traversal or authority reconstruction.
    ancestors: Vec<v8::Local<'s, v8::Object>>, // Detect only active-path cycles; repeated immutable copies remain allowed.
    views: Vec<(ArraySpec, v8::Local<'s, v8::ArrayBufferView>)>, // Keep at most 48 owner-thread views until native admission succeeds.
} // The engine's fixed original-root scratch admission covers these bounded descriptors.
type ServiceProjection<'s> = (
    Value,
    Vec<(ArraySpec, v8::Local<'s, v8::ArrayBufferView>)>,
    usize,
);

fn service_out<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    references: Option<v8::Local<'s, v8::Value>>,
    prototypes: &WirePrototypes,
    max_json: usize,
    max_binary: usize,
) -> Result<ServiceProjection<'s>> {
    // Traverse raw options directly while projecting only explicitly registered wrapper identities.
    let (references, reference_bytes) = wire_references(scope, references, prototypes, max_json)?; // Validate the whole bounded projection table without reading package payload fields.
    let mut walk = WireWalk {
        nodes: 0,
        json_bytes: 0,
        binary_bytes: 0,
        max_json,
        max_binary,
        reference_bytes,
        references,
        ancestors: Vec::new(),
        views: Vec::new(),
    }; // Keep projections and outgoing metadata within the original admitted scratch bound.
    let payload = wire_walk(scope, value, prototypes, &mut walk, 0)?; // Reject the complete tree before any native binary copy.
    Ok((payload, walk.views, walk.json_bytes)) // The caller admits the flight and copies each selected view synchronously.
} // No V8 handle may leave the caller's isolate-owner stack.
fn wire_add_json(walk: &mut WireWalk<'_>, bytes: usize) -> Result<()> {
    // Count serialized bytes before adding more metadata.
    walk.json_bytes = walk
        .json_bytes
        .checked_add(bytes)
        .filter(|total| *total <= walk.max_json.saturating_sub(walk.reference_bytes))
        .ok_or_else(|| AnimationError::Budget("service JSON bytes".into()))?; // Include transient projection custody without enlarging the original metadata scratch limit.
    Ok(()) // No output allocation occurs in this counter.
} // Metadata and binary limits remain independent.
fn wire_json_string(walk: &mut WireWalk<'_>, text: &str) -> Result<()> {
    // Match serde_json string escaping exactly for valid UTF-8.
    wire_add_json(walk, 2)?; // Account for both quotation marks before content.
    for byte in text.bytes() {
        // Count bytewise; UTF-8 multibyte text is emitted unchanged.
        let bytes = match byte {
            b'"' | b'\\' | 8 | 9 | 10 | 12 | 13 => 2,
            0..=31 => 6,
            _ => 1,
        }; // Handle the exact short escapes and remaining control escapes.
        wire_add_json(walk, bytes)?; // Stop as soon as the metadata ceiling is exceeded.
    } // All encoded string bytes have now been counted.
    Ok(()) // The caller may retain the already bounded string.
} // Strings never execute guest conversion hooks.
fn wire_string(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::String>,
    walk: &WireWalk<'_>,
) -> Result<String> {
    // Convert a genuine primitive string with strict Unicode.
    let remaining = walk
        .max_json
        .saturating_sub(walk.reference_bytes)
        .saturating_sub(walk.json_bytes)
        .saturating_sub(2); // Reserve projection custody and eventual quotation marks before allocating UTF-16 scratch.
    if value.length() > remaining || value.utf8_length(scope) > remaining {
        return Err(AnimationError::Budget("service string bytes".into()));
    } // Check both lengths before native string allocation.
    let mut units = Vec::new(); // Avoid infallible allocation for temporary UTF-16 storage.
    units
        .try_reserve_exact(value.length())
        .map_err(|_| AnimationError::Budget("service string allocation".into()))?; // Reserve only the verified length.
    units.resize(value.length(), 0_u16); // Initialize every code unit before native copying.
    value.write_v2(scope, 0, &mut units, v8::WriteFlags::empty()); // Copy primitive code units without replacement or user hooks.
    String::from_utf16(&units).map_err(|_| runtime("service string contains an unpaired surrogate"))
    // Reject lossy key collisions and altered native identifiers.
} // UTF-16 scratch is released before the next tree node.
fn wire_keys<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    skip_indices: bool,
) -> Result<v8::Local<'s, v8::Array>> {
    // Enumerate every own symbol and named property without reading values.
    let args = v8::GetPropertyNamesArgs {
        mode: v8::KeyCollectionMode::OwnOnly,
        property_filter: v8::PropertyFilter::ALL_PROPERTIES,
        index_filter: if skip_indices {
            v8::IndexFilter::SkipIndices
        } else {
            v8::IndexFilter::IncludeIndices
        },
        key_conversion: v8::KeyConversionMode::ConvertToString,
    }; // Explicit options avoid default symbol and nonenumerable omissions.
    object
        .get_property_names(scope, args)
        .ok_or_else(|| runtime("service own keys")) // This overload honors index_filter, unlike get_own_property_names.
} // Call only after rejecting proxies and checking the expected prototype.
fn wire_own_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: v8::Local<'s, v8::Name>,
) -> Result<v8::Local<'s, v8::Value>> {
    // Read only own data descriptors.
    let descriptor = object
        .get_own_property_descriptor(scope, name)
        .ok_or_else(|| runtime("service descriptor lookup"))?; // Ordinary-object descriptor lookup does not invoke property getters.
    let descriptor = v8::Local::<v8::Object>::try_from(descriptor)
        .map_err(|_| runtime("service sparse or missing property"))?; // Missing descriptors cannot be inherited from a prototype.
    let value_key =
        v8::String::new(scope, "value").ok_or_else(|| runtime("service descriptor key"))?; // Use the standard descriptor's own value field.
    if descriptor.has_own_property(scope, value_key.into()) != Some(true) {
        return Err(runtime("service accessor properties are forbidden"));
    } // An accessor descriptor lacks an own value, including undefined getters.
    descriptor
        .get(scope, value_key.into())
        .ok_or_else(|| runtime("service descriptor value")) // Its verified own data field cannot fall through to a poisoned Object.prototype.
} // No caller-supplied getter or prototype field is evaluated.
fn wire_expected_prototype(
    scope: &mut v8::PinScope<'_, '_>,
    object: v8::Local<v8::Object>,
    expected: &v8::Global<v8::Object>,
    allow_null: bool,
) -> Result<()> {
    // Require the original structural identity.
    let prototype = object
        .get_prototype(scope)
        .ok_or_else(|| runtime("service prototype lookup"))?; // The proxy guard precedes this internal operation.
    if allow_null && prototype.is_null() {
        return Ok(());
    } // Null-prototype records are valid data containers.
    let expected = v8::Local::new(scope, expected); // Reconstitute the original native prototype only on its owner.
    if !prototype.strict_equals(expected.into()) {
        return Err(runtime("service prototype is unsupported"));
    } // Reject subclasses and altered prototype chains.
    Ok(()) // Prototype properties are never read.
} // This validates data shape without creating permission or provenance.
fn wire_exotic(value: v8::Local<v8::Value>) -> bool {
    // Reject native structured objects even if their prototype has been changed.
    value.is_function()
        || value.is_promise()
        || value.is_date()
        || value.is_reg_exp()
        || value.is_map()
        || value.is_set()
        || value.is_weak_map()
        || value.is_weak_set()
        || value.is_map_iterator()
        || value.is_set_iterator()
        || value.is_generator_object()
        || value.is_arguments_object()
        || value.is_native_error()
        || value.is_boolean_object()
        || value.is_number_object()
        || value.is_string_object()
        || value.is_big_int_object()
        || value.is_symbol_object()
        || value.is_module_namespace_object()
        || value.is_array_buffer()
        || value.is_shared_array_buffer()
        || value.is_array_buffer_view()
        || value.is_wasm_memory_object()
        || value.is_wasm_module_object() // Views admitted earlier bypass this remaining-exotic guard.
} // Unsupported native object bodies are never silently serialized.
fn wire_reference_array<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    prototypes: &WirePrototypes,
    maximum: usize,
) -> Result<v8::Local<'s, v8::Array>> {
    // Inspect table containers without invoking array iterators or indexed getters.
    if value.is_proxy() || !value.is_array() {
        return Err(runtime(
            "service wrapper references require nonproxy arrays",
        ));
    } // Reject proxies before native prototype or descriptor operations.
    let array = v8::Local::<v8::Array>::try_from(value)
        .map_err(|_| runtime("service wrapper reference array"))?; // Read only a genuine array's internal length.
    wire_expected_prototype(scope, array.into(), &prototypes.array, false)?; // Refuse custom table prototypes before key enumeration.
    if array.length() as usize > maximum {
        return Err(AnimationError::Budget(
            "service wrapper reference count".into(),
        ));
    } // Bound traversal and native vector allocation before inspecting entries.
    if wire_keys(scope, array.into(), false)?.length() != array.length() + 1 {
        return Err(runtime("service wrapper reference array shape"));
    } // Require only dense indices and the builtin length property.
    Ok(array) // Each caller still reads every required index through an own data descriptor.
} // End table-container validation.
fn wire_reference_index<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    array: v8::Local<'s, v8::Array>,
    index: u32,
) -> Result<v8::Local<'s, v8::Value>> {
    // Read a required table index without inherited properties.
    let key = v8::String::new(scope, &index.to_string())
        .ok_or_else(|| runtime("service wrapper reference index"))?; // Construct one bounded canonical index name.
    wire_own_value(scope, array.into(), key.into()) // Missing indices and accessors fail instead of evaluating package code.
} // End own-data table indexing.
fn wire_reference_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
    maximum: usize,
    walk: &mut WireWalk<'s>,
) -> Result<String> {
    // Decode one exact inert projection field under the shared metadata bound.
    let key =
        v8::String::new(scope, name).ok_or_else(|| runtime("service wrapper projection key"))?; // The caller supplies only the fixed id and kind keys.
    let value = wire_own_value(scope, object, key.into())?; // Reject accessors and absent own fields before inspecting their values.
    let value = v8::Local::<v8::String>::try_from(value)
        .map_err(|_| runtime("service wrapper projection strings required"))?; // Never invoke coercion or accept boxed strings.
    if value.length() == 0 || value.length() > maximum || value.utf8_length(scope) > maximum {
        return Err(AnimationError::Budget(
            "service wrapper projection string limit".into(),
        ));
    } // Bound each identifier before allocating UTF-16 or UTF-8 storage.
    wire_json_string(walk, name)?; // Account for the exact fixed field name first.
    let value = wire_string(scope, value, walk)?; // Reject invalid Unicode under the remaining shared table budget.
    wire_json_string(walk, &value)?; // Include encoded content and quotation marks before retaining this field.
    Ok(value) // This string remains untrusted native-registry lookup data.
} // End inert projection-field validation.
fn wire_references<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: Option<v8::Local<'s, v8::Value>>,
    prototypes: &WirePrototypes,
    max_json: usize,
) -> Result<(Vec<WireReference<'s>>, usize)> {
    // Parse the complete optional identity table independently of raw package options.
    let Some(value) = value else {
        return Ok((Vec::new(), 0));
    }; // Two-argument callers incur no reference-table allocation or extra restriction.
    let table = wire_reference_array(scope, value, prototypes, SERVICE_REFERENCES)?; // Bound the table to 64 dense entries with no symbol or named extras.
    if table.length() as usize > max_json / 21 {
        return Err(AnimationError::Budget(
            "service wrapper projection metadata".into(),
        ));
    } // Each nonempty id/kind record needs at least 21 encoded bytes before any retained vector allocation.
    let mut budget = WireWalk {
        nodes: 0,
        json_bytes: 0,
        binary_bytes: 0,
        max_json,
        max_binary: 0,
        reference_bytes: 0,
        references: Vec::new(),
        ancestors: Vec::new(),
        views: Vec::new(),
    }; // Count transient projection storage inside the existing original-root metadata scratch allowance.
    let mut references: Vec<WireReference<'s>> = Vec::new(); // Own only bounded local handles and inert native metadata.
    references
        .try_reserve_exact(table.length() as usize)
        .map_err(|_| AnimationError::Budget("service wrapper reference allocation".into()))?; // Reserve the verified number of records fallibly.
    for index in 0..table.length() {
        // Inspect each exact pair without running an iterator callback.
        let entry = wire_reference_index(scope, table, index)?; // Reject a sparse or accessor table entry immediately.
        let entry = wire_reference_array(scope, entry, prototypes, 2)?; // Each pair has the original Array prototype and no extra own properties.
        if entry.length() != 2 {
            return Err(runtime(
                "service wrapper reference requires identity and projection",
            ));
        } // Require exactly the wrapper and its inert projection.
        let wrapper = wire_reference_index(scope, entry, 0)?; // Read the original wrapper identity as an own data value.
        if wrapper.is_proxy() || wrapper.is_array() || wire_exotic(wrapper) {
            return Err(runtime(
                "service wrapper identity must be an ordinary nonproxy object",
            ));
        } // No proxy or unsupported exotic can be laundered through an identity substitution.
        let object = v8::Local::<v8::Object>::try_from(wrapper)
            .map_err(|_| runtime("service wrapper identity object"))?; // Primitive identifiers alone cannot participate in object substitution.
        wire_expected_prototype(scope, object, &prototypes.object, true)?; // SDK wrappers are ordinary or null-prototype objects; their methods are intentionally not traversed.
        if references
            .iter()
            .any(|reference| reference.object.strict_equals(wrapper))
        {
            return Err(runtime("duplicate service wrapper identity"));
        } // Never let later entries overwrite an earlier projection for the same object.
        let projection = wire_reference_index(scope, entry, 1)?; // Read only the pair's inert projection value.
        if projection.is_proxy() || projection.is_array() || wire_exotic(projection) {
            return Err(runtime("service wrapper projection must be plain data"));
        } // Reject proxies before prototype or own-key inspection.
        let projection = v8::Local::<v8::Object>::try_from(projection)
            .map_err(|_| runtime("service wrapper projection object"))?; // Require a record, never a primitive shorthand or boxed identity.
        wire_expected_prototype(scope, projection, &prototypes.object, true)?; // Admit only ordinary/null records with the original structural identity.
        if wire_keys(scope, projection, false)?.length() != 2 {
            return Err(runtime(
                "service wrapper projection requires only id and kind",
            ));
        } // Include symbols and nonenumerable keys when rejecting extras.
        let before = budget.json_bytes; // Measure this projection independently while retaining the aggregate table bound.
        wire_add_json(&mut budget, 5)?; // Count braces, the comma, and both field colons before retaining strings.
        let id = wire_reference_string(scope, projection, "id", 256, &mut budget)?; // Preserve the exact bounded string used for native registry lookup.
        let kind = wire_reference_string(scope, projection, "kind", 80, &mut budget)?; // Carry an explicit untrusted native registry kind without asserting authority.
        let json_bytes = budget.json_bytes - before; // Record the exact encoded size for every later occurrence in raw options.
        references.push(WireReference {
            object,
            metadata: serde_json::json!({"id": id, "kind": kind}),
            json_bytes,
        }); // Methods remain local and no handle, grant, epoch, or provenance is constructed here.
    } // Every table entry is validated before the raw payload is traversed or any binary plane is copied.
    Ok((references, budget.json_bytes)) // The outgoing metadata and these temporary projections share the original scratch ceiling.
} // A guest may propose ordinary lookup data, but only the native dispatcher can authenticate a real handle.
fn wire_walk<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    prototypes: &WirePrototypes,
    walk: &mut WireWalk<'s>,
    depth: usize,
) -> Result<Value> {
    // Recursively construct only JSON data and binary references.
    if depth > SERVICE_DEPTH || walk.nodes >= SERVICE_NODES {
        return Err(AnimationError::Budget(
            "service traversal depth/nodes".into(),
        ));
    } // Bound recursion and total work before visiting another value.
    walk.nodes += 1; // Charge the current node exactly once.
    if value.is_proxy() {
        return Err(runtime("service proxies are forbidden"));
    } // Reject proxies before any own-key, descriptor, or prototype operation.
    if value.is_null() {
        wire_add_json(walk, 4)?;
        return Ok(Value::Null);
    } // Preserve explicit null.
    if value.is_boolean() {
        let boolean = value.is_true();
        wire_add_json(walk, if boolean { 4 } else { 5 })?;
        return Ok(Value::Bool(boolean));
    } // Primitive booleans need no coercion.
    if value.is_number() {
        // Accept only finite primitive numbers without conversion hooks.
        let number = value
            .number_value(scope)
            .filter(|number| number.is_finite())
            .ok_or_else(|| runtime("service number must be finite"))?; // NaN and infinities are not JSON data.
        let number = if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0 {
            serde_json::Number::from(number as i64)
        } else {
            serde_json::Number::from_f64(number)
                .ok_or_else(|| runtime("service number encoding"))?
        }; // Preserve safe integer accessors used by native option validation.
        wire_add_json(walk, number.to_string().len())?; // Count exactly the representation serde_json will emit.
        return Ok(Value::Number(number)); // Leave all numerical domain validation to the actual native service.
    } // Non-numeric primitives continue to the strict string/object inventory.
    if value.is_string() {
        // Convert only genuine primitive strings.
        let string =
            v8::Local::<v8::String>::try_from(value).map_err(|_| runtime("service string type"))?; // Avoid user-controlled toString methods.
        let string = wire_string(scope, string, walk)?; // Reject overlong or invalid Unicode before retaining it.
        wire_json_string(walk, &string)?; // Include all JSON escaping in the cumulative budget.
        return Ok(Value::String(string)); // Preserve the exact valid Unicode text.
    } // Undefined, BigInt, and Symbol are rejected by the object guard below.
    let object = v8::Local::<v8::Object>::try_from(value)
        .map_err(|_| runtime("service value is not supported data"))?; // Admit no remaining primitive types.
    if let Some(index) = walk
        .references
        .iter()
        .position(|reference| reference.object.strict_equals(value))
    {
        // Match exact nonproxy wrapper identity only after the unconditional proxy guard.
        if depth >= SERVICE_DEPTH || walk.nodes > SERVICE_NODES - 2 {
            return Err(AnimationError::Budget(
                "service handle projection depth/nodes".into(),
            ));
        } // The projected record contributes two primitive children.
        let json_bytes = walk.references[index].json_bytes; // Read the prevalidated projection's exact encoded size.
        wire_add_json(walk, json_bytes)?; // Admit every occurrence before cloning its two small strings.
        walk.nodes += 2; // Count the id and kind leaves in the outgoing metadata tree.
        return Ok(walk.references[index].metadata.clone()); // This data cannot mint a native handle, grant, permission, or provenance.
    } // All other payload objects continue through the unchanged strict native traversal.
    let typed = if value.is_uint8_array() {
        Some((TypedArrayKind::U8, &prototypes.u8))
    } else if value.is_float32_array() {
        Some((TypedArrayKind::F32, &prototypes.f32))
    } else if value.is_uint16_array() {
        Some((TypedArrayKind::U16, &prototypes.u16))
    } else if value.is_uint32_array() {
        Some((TypedArrayKind::U32, &prototypes.u32))
    } else {
        None
    }; // Keep the exact four-type frozen inventory.
    if let Some((kind, prototype)) = typed {
        // Replace an attached fixed view with a native-created binary reference.
        if walk.views.len() >= SERVICE_PLANES {
            return Err(AnimationError::Budget("service binary plane count".into()));
        } // Bound retained V8 handles and plane metadata.
        wire_expected_prototype(scope, object, prototype, false)?; // Reject typed-array subclasses and forged prototype chains.
        let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
            .map_err(|_| runtime("service typed view"))?; // Access the actual native view rather than JS fields.
        let bytes = view.byte_length(); // Select only the view's bytes, excluding neighboring backing storage.
        let binary_bytes = walk
            .binary_bytes
            .checked_add(bytes)
            .filter(|total| *total <= walk.max_binary)
            .ok_or_else(|| AnimationError::Budget("service binary bytes".into()))?; // Reject aggregate excess before materializing even an on-heap view's native ArrayBuffer.
        let offset = view.byte_offset(); // Preserve subview selection without leaking unrelated bytes.
        let buffer = view
            .buffer(scope)
            .ok_or_else(|| runtime("service view buffer"))?; // Retain the native buffer handle on this owner thread.
        let backing = buffer.get_backing_store(); // Inspect native backing flags independently of script-visible properties.
        if buffer.was_detached()
            || backing.is_shared()
            || backing.is_resizable_by_user_javascript()
            || bytes % kind.width() != 0
            || offset % kind.width() != 0
            || offset
                .checked_add(bytes)
                .is_none_or(|end| end > buffer.byte_length())
        {
            return Err(runtime(
                "service binary view is detached, shared, resizable, or malformed",
            ));
        } // Reject unstable buffers and invalid shape before copying.
        walk.binary_bytes = binary_bytes; // Commit the checked aggregate after stable backing validation.
        if wire_keys(scope, object, true)?.length() != 0 {
            return Err(runtime("service typed view has named or symbol properties"));
        } // Ignore numeric elements while rejecting hidden expandos and accessors.
        let name = format!("b{}", walk.views.len()); // Generate a deterministic untrusted-data plane identifier.
        wire_add_json(walk, 3)?; // Count braces and the single field colon.
        wire_json_string(walk, "$ilium_binary")?; // Count the transport-only placeholder key.
        wire_json_string(walk, &name)?; // Count the generated plane reference.
        walk.views.push((
            ArraySpec {
                name: name.clone(),
                kind,
                elements: bytes / kind.width(),
            },
            view,
        )); // Retain the exact owner-thread view until admission and immediate copy.
        return Ok(serde_json::json!({"$ilium_binary": name})); // This marker carries bytes only and can never mint native identity.
    } // Other typed arrays and DataView are unsupported.
    if wire_exotic(value) {
        return Err(runtime("service object type is unsupported"));
    } // Reject known structured native objects, including buffers outside the typed inventory.
    if walk
        .ancestors
        .iter()
        .any(|ancestor| ancestor.strict_equals(value))
    {
        return Err(runtime("service payload contains a cycle"));
    } // Reject cyclic graphs before descending.
    let is_array = value.is_array(); // Native type detection ignores spoofed constructor properties.
    wire_expected_prototype(
        scope,
        object,
        if is_array {
            &prototypes.array
        } else {
            &prototypes.object
        },
        !is_array,
    )?; // Limit data containers to dense arrays and ordinary/null records.
    walk.ancestors.push(object); // Keep at most 33 current-path identities under the recursion ceiling.
    let result = wire_container(scope, object, is_array, prototypes, walk, depth); // Traverse the container through own data descriptors only.
    walk.ancestors.pop(); // Restore cycle state on both success and failure.
    result // Propagate the first malformed value or budget failure.
} // No binary content has been copied during traversal.
fn wire_container<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    is_array: bool,
    prototypes: &WirePrototypes,
    walk: &mut WireWalk<'s>,
    depth: usize,
) -> Result<Value> {
    // Keep array and record validation shallow and explicit.
    if is_array {
        // Dense arrays preserve their exact element order.
        let array =
            v8::Local::<v8::Array>::try_from(object).map_err(|_| runtime("service array type"))?; // Read the native length instead of a guest property.
        let length = array.length() as usize; // Convert the bounded native u32 length to the host index type.
        if length > SERVICE_NODES.saturating_sub(walk.nodes) {
            return Err(AnimationError::Budget("service array nodes".into()));
        } // Reject huge sparse arrays before key enumeration or Vec allocation.
        if wire_keys(scope, object, false)?.length() as usize != length + 1 {
            return Err(runtime("service array is sparse or has extra properties"));
        } // Dense indices plus the builtin length must be the complete own-key inventory.
        wire_add_json(walk, 2 + length.saturating_sub(1))?; // Count brackets and commas before creating the metadata vector.
        let mut values = Vec::new(); // Allocate only metadata covered by the original engine scratch reservation.
        values
            .try_reserve_exact(length)
            .map_err(|_| AnimationError::Budget("service array allocation".into()))?; // Reserve the verified element count fallibly.
        for index in 0..length {
            // Visit every index; missing own indices cannot use inherited values.
            let name = v8::String::new(scope, &index.to_string())
                .ok_or_else(|| runtime("service array index"))?; // Construct a canonical numeric property name.
            let child = wire_own_value(scope, object, name.into())?; // Reject indexed getters and holes.
            values.push(wire_walk(scope, child, prototypes, walk, depth + 1)?); // Recursively copy only bounded data metadata.
        } // All indices have independent own data descriptors.
        return Ok(Value::Array(values)); // Return the complete dense metadata array.
    } // Non-array values use exact own string keys.
    let keys = wire_keys(scope, object, false)?; // Include nonenumerable properties and symbols so neither can hide.
    let length = keys.length() as usize; // Key enumeration is bounded by the isolate heap and checked before native growth.
    if length > SERVICE_NODES.saturating_sub(walk.nodes) {
        return Err(AnimationError::Budget("service object nodes".into()));
    } // Bound native map entries before collecting them.
    wire_add_json(walk, 2 + length.saturating_sub(1) + length)?; // Count braces, commas, and all field colons.
    let mut output = serde_json::Map::new(); // Retain only validated metadata under the original scratch budget.
    for index in 0..keys.length() {
        // Each native-generated key array element is an own data element.
        let key = keys
            .get_index(scope, index)
            .ok_or_else(|| runtime("service object key"))?; // Reading the engine-created dense key list cannot invoke a guest getter.
        let key = v8::Local::<v8::String>::try_from(key)
            .map_err(|_| runtime("service symbol keys are forbidden"))?; // Symbols are rejected, never dropped or coerced.
        let name = wire_string(scope, key, walk)?; // Bound and strictly decode the property name before retaining it.
        if matches!(
            name.as_str(),
            "$ilium_binary" | "__proto__" | "prototype" | "constructor"
        ) {
            return Err(runtime("service object key is reserved"));
        } // Preserve existing prototype-key refusals and prevent forged binary references.
        wire_json_string(walk, &name)?; // Count encoded property-name bytes before the value.
        let child = wire_own_value(scope, object, key.into())?; // Reject both enumerable and hidden accessor properties.
        let child = wire_walk(scope, child, prototypes, walk, depth + 1)?; // Validate the entire nested value.
        if output.insert(name, child).is_some() {
            return Err(runtime("service duplicate object key"));
        } // Reject any unexpected key collision instead of overwriting.
    } // The complete own-key inventory has been checked.
    Ok(Value::Object(output)) // Return an inert metadata record.
} // Host copies, authority tickets, and pending retention belong to the enclosing boundary implementation.
fn property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Result<v8::Local<'s, v8::Value>> {
    let name = v8::String::new(scope, name).ok_or_else(|| runtime("property name allocation"))?;
    object
        .get(scope, name.into())
        .ok_or_else(|| runtime("property access threw"))
}
fn function_property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Result<v8::Local<'s, v8::Function>> {
    v8::Local::<v8::Function>::try_from(property(scope, object, name)?)
        .map_err(|_| runtime("required function missing"))
}
fn json_into<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: &Value,
    max_bytes: usize,
) -> Result<v8::Local<'s, v8::Value>> {
    let json = serde_json::to_string(value)?;
    if json.len() > max_bytes {
        return Err(AnimationError::Budget("JSON input bytes".into()));
    }
    let json = v8::String::new(scope, &json).ok_or_else(|| runtime("JSON input allocation"))?;
    v8::json::parse(scope, json).ok_or_else(|| runtime("JSON input parse"))
}
fn json_out(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    max_bytes: usize,
) -> Result<Value> {
    let json = v8::json::stringify(scope, value)
        .ok_or_else(|| runtime("value is not JSON serializable"))?;
    if json.length() > max_bytes || json.utf8_length(scope) > max_bytes {
        return Err(AnimationError::Budget("JSON output bytes".into()));
    }
    serde_json::from_str(&json.to_rust_string_lossy(scope)).map_err(Into::into)
}
fn retain_buffer(
    scope: &mut v8::PinScope,
    buffer: v8::Local<v8::ArrayBuffer>,
    buffers: &mut Vec<v8::Global<v8::ArrayBuffer>>,
) -> Result<()> {
    if buffers
        .iter()
        .any(|existing| v8::Local::new(scope, existing) == buffer)
    {
        return Ok(());
    }
    if buffers.len() >= 48 {
        return Err(AnimationError::Budget("transaction buffer count".into()));
    }
    #[cfg(test)]
    boundary_tests::observe_buffer(scope, buffer);
    buffers.push(v8::Global::new(scope, buffer));
    Ok(())
}
fn with_frame_phase<T>(
    bridge: &Rc<RefCell<Bridge>>,
    phase: Phase,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    // Scope only native private-frame hooks.
    let previous = bridge.borrow().phase; // Save the caller's phase without retaining a RefCell borrow.
    bridge.borrow_mut().phase = phase; // Open the requested nonacquiring frame handoff.
    let result = operation(); // Execute under the existing watchdog deadline with no checkpoint.
    bridge.borrow_mut().phase = previous; // Restore the exact previous phase on both Ok and Err.
    result // Preserve the original success or failure for normal engine teardown.
} // No public API, authorization mutation, or deadline reset is introduced.
fn collect_native_frame_buffers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    global: v8::Local<'s, v8::Object>,
    frame: v8::Local<'s, v8::Object>,
    buffers: &mut Vec<v8::Global<v8::ArrayBuffer>>,
) -> Result<()> {
    // Collect private working, sealed, and input buffers before package code can throw or mutate drawing state.
    let hook = property(scope, global, "__ilium_frame_buffers")?; // Bootstrap sealed either the actual trusted function or its absence before package evaluation.
    if hook.is_undefined() {
        return collect_frame_buffers(scope, frame, buffers);
    } // Preserve legacy discovery only for facades that supplied no private inventory hook.
    let hook = v8::Local::<v8::Function>::try_from(hook)
        .map_err(|_| runtime("frame inventory hook type"))?; // A present malformed hook is a failure, never permission to invoke drawing getters.
    let inventory = hook
        .call(scope, global.into(), &[frame.into()])
        .ok_or_else(|| runtime("frame inventory hook threw"))?; // The trusted hook reads its private WeakMap and never invokes a drawing property getter.
    if inventory.is_proxy()
        || !inventory.is_object()
        || inventory.is_array()
        || wire_exotic(inventory)
    {
        return Err(runtime("frame inventory must be a nonproxy record"));
    } // Reject observable proxy traps and incompatible native containers before descriptor inspection.
    let inventory = v8::Local::<v8::Object>::try_from(inventory)
        .map_err(|_| runtime("frame inventory object"))?; // Retain the original private object without coercion.
    let keys = wire_keys(scope, inventory, false)?; // Enumerate every own key, including symbols and nonenumerable entries, without reading drawing facades.
    if keys.length() > 48 {
        return Err(AnimationError::Budget("frame inventory plane count".into()));
    } // Preserve the original 48-plane ceiling before retaining native handles.
    for index in 0..keys.length() {
        // Every bounded entry must be an own data property holding an actual ArrayBufferView.
        let key = keys
            .get_index(scope, index)
            .ok_or_else(|| runtime("frame inventory key"))?; // Read only the engine-created dense key list.
        let key = v8::Local::<v8::String>::try_from(key)
            .map_err(|_| runtime("frame inventory symbol key"))?; // Reject symbols instead of silently omitting a buffer from detachment.
        let value = wire_own_value(scope, inventory, key.into())?; // Accessor descriptors are rejected without executing a getter or reading a prototype value.
        if value.is_proxy() || !value.is_array_buffer_view() {
            return Err(runtime("frame inventory view type"));
        } // Only actual native views contribute buffer custody.
        let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
            .map_err(|_| runtime("frame inventory view"))?; // Preserve each view's genuine backing identity without property access.
        let buffer = view
            .buffer(scope)
            .ok_or_else(|| runtime("frame inventory buffer"))?; // Borrow the native backing buffer even when several returned views share it.
        retain_buffer(scope, buffer, buffers)?; // Deduplicate original buffers and retain every known owner through the finally detachment path.
    } // No surface, drawing getter, metadata serializer, or package render method was invoked during discovery.
    Ok(()) // Native render may now execute with all private frame allocations retained for exception cleanup.
} // Trusted inventory failure never falls back to public drawing property discovery.
fn collect_frame_buffers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    frame: v8::Local<'s, v8::Object>,
    buffers: &mut Vec<v8::Global<v8::ArrayBuffer>>,
) -> Result<()> {
    for name in ["gray", "cell_rgb"] {
        let value = property(scope, frame, name)?;
        if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
            if let Some(buffer) = view.buffer(scope) {
                retain_buffer(scope, buffer, buffers)?;
            }
        }
    }
    for (name, fields) in [("cells", &["masks", "rgb"][..]), ("pixels", &["data"][..])] {
        let value = property(scope, frame, name)?;
        if value.is_object() {
            let object = value
                .to_object(scope)
                .ok_or_else(|| runtime("frame surface"))?;
            for field in fields {
                let value = property(scope, object, field)?;
                if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
                    if let Some(buffer) = view.buffer(scope) {
                        retain_buffer(scope, buffer, buffers)?;
                    }
                }
            }
        }
    }
    Ok(())
}
fn dispatch_host(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut returned: v8::ReturnValue<v8::Value>,
) {
    // Convert a bounded raw SDK request into an immutable native snapshot.
    let Some(bridge) = scope.get_slot::<Rc<RefCell<Bridge>>>().cloned() else {
        return;
    }; // Require the engine's private owner-thread bridge.
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        return;
    }; // Create only a local Promise resolver.
    let promise = resolver.get_promise(scope); // Keep the native Promise available for an exact queue-admission receipt.
    returned.set(promise.into()); // The callback remains asynchronous to the SDK.
    let attempted = (|| {
        // Keep validation failure separate from native work publication.
        let (phase, authority, limits, budget, quota, package_digest, prototypes) = {
            // Snapshot native controls before traversing package data.
            let state = bridge.borrow(); // No package-facing V8 calls occur under this borrow.
            (
                state.phase,
                state.authority,
                state.limits.clone(),
                Arc::clone(&state.service_budget),
                state.quota.clone(),
                state.package_digest.clone(),
                state.prototypes.clone(),
            ) // Clones share immutable owner state only.
        }; // Release the bridge before any property inspection.
        let phase = match phase {
            // Derive phase from the native entrypoint, never JSON.
            Phase::Preparation => ServicePhase::Create, // Preparation may acquire admitted native work.
            Phase::Async => ServicePhase::Async, // A current-authority pump may acquire subsequent work.
            phase => {
                // All remaining lifecycle phases are synchronous or inert publication.
                bridge.borrow_mut().violation =
                    Some(format!("host acquisition unavailable during {phase:?}")); // Poison a caught render/module/acknowledgement phase escape.
                return Err(runtime("host acquisition outside accepted lifecycle phase"));
                // Preserve synchronous render constraints even when script catches errors.
            } // End disallowed native phases.
        }; // Continue only with a native acquiring phase.
        let authority =
            authority.ok_or_else(|| runtime("native service authority is not bound"))?; // Fail closed before any payload inspection.
        if budget.closed.load(Ordering::Acquire)
            || budget.count.load(Ordering::Acquire) >= limits.pending_requests
        {
            return Err(AnimationError::Budget(
                "retained service request count".into(),
            ));
        } // Avoid traversing when no retained slot can be admitted.
        if !matches!(args.length(), 2 | 3) || !args.get(0).is_string() {
            return Err(runtime(
                "dispatch requires method, strict binary data, and optional wrapper references",
            ));
        } // Existing two-argument callers retain the same raw-data boundary.
        let method = v8::Local::<v8::String>::try_from(args.get(0))
            .map_err(|_| runtime("dispatch method"))?; // Primitive strings cannot invoke conversion hooks.
        if method.length() > 80 || method.utf8_length(scope) > 80 {
            return Err(AnimationError::Budget("host method bytes".into()));
        } // Check native string lengths before allocating method text.
        let method = method.to_rust_string_lossy(scope); // The subsequent ASCII grammar rejects any replacement character.
        validate_service_method(&method)?; // Refuse unsupported identifier spellings without native effects.
        let prototypes = prototypes.ok_or_else(|| runtime("native wire prototypes missing"))?; // Require the original builtin identities captured before package execution.
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(limits.preparation_ms))
            .ok_or_else(|| runtime("host deadline overflow"))?; // Include traversal, copies, and queue residence in this request deadline.
        let payload_value = v8::Local::new(scope, args.get(1)); // Re-root callback arguments into the current owner scope before retaining local views.
        let references = if args.length() == 3 {
            Some(v8::Local::new(scope, args.get(2)))
        } else {
            None
        }; // Re-root the optional bounded wrapper table without touching payload properties.
        let (metadata, views, json_bytes) = service_out(
            scope,
            payload_value,
            references,
            &prototypes,
            limits.json_bytes,
            limits.pending_bytes,
        )?; // Traverse unchanged raw options while projecting explicitly listed SDK wrappers as data.
        let binary_bytes = views
            .iter()
            .try_fold(0usize, |bytes, (_, view)| {
                bytes.checked_add(view.byte_length())
            })
            .ok_or_else(|| runtime("service binary size overflow"))?; // Count every logical view occurrence, including aliases.
        let wire_bytes = service_sizes(json_bytes, binary_bytes, views.len(), &limits)?; // Include all descriptor and request-header overhead.
        let lease = budget.reserve(wire_bytes)?; // Reserve physical pending count/bytes before allocating the native plane copies.
        let admission = quota
            .reserve_external_storage(service_resident_bytes(
                json_bytes,
                binary_bytes,
                views.len(),
            )?)
            .map_err(admission_error)?; // Admit escaping storage from the original root before the first binary copy.
        let mut arrays = Vec::new(); // Type descriptors now belong to the independently admitted payload.
        arrays
            .try_reserve_exact(views.len())
            .map_err(|_| AnimationError::Budget("service type inventory allocation".into()))?; // Reserve the complete bounded inventory fallibly.
        let mut planes = BTreeMap::new(); // Every plane map node is covered by the new payload admission.
        for (spec, view) in views {
            // Copy on the owner thread before returning control to the package.
            let bytes = spec
                .elements
                .checked_mul(spec.kind.width())
                .ok_or_else(|| runtime("service copy shape overflow"))?; // Retain checked element-size arithmetic at the copy boundary.
            let mut copied = Vec::new(); // No destination bytes existed before root and pending admission.
            copied
                .try_reserve_exact(bytes)
                .map_err(|_| AnimationError::Budget("service plane allocation".into()))?; // Refuse allocation without partially publishing a request.
            copied.resize(bytes, 0); // Initialize all destination bytes before V8 copying.
            if view.copy_contents(&mut copied) != bytes {
                return Err(runtime("service view copy length changed"));
            } // Copy only the native view's logical range.
            planes.insert(spec.name.clone(), copied); // Never retain the package's writable backing buffer.
            arrays.push(spec); // Keep exact kind/count identity alongside immutable native bytes.
        } // Mutation after this callback cannot alter any native request plane.
        let payload = ServiceValue {
            inner: Arc::new(ServiceValueData {
                metadata,
                arrays,
                planes,
                json_bytes,
                binary_bytes,
                wire_bytes,
                quota,
                _request: Some(lease),
                _admission: admission,
            }),
        }; // Transfer every allocation and guard together.
        let mut state = bridge.borrow_mut(); // Publication below mutates only native bounded queues.
        if state.authority != Some(authority)
            || !matches!(state.phase, Phase::Preparation | Phase::Async)
        {
            return Err(runtime("service authority changed before publication"));
        } // Recheck the captured native boundary immediately before enqueueing.
        let id = state.next_id; // Allocate identities only after all validation, admission, and copying succeeded.
        state.next_id = id
            .checked_add(1)
            .ok_or_else(|| runtime("host request identity exhausted"))?; // Never recycle an old native correlation ID.
        let request = HostRequest {
            inner: Arc::new(HostRequestData {
                id,
                method,
                timeout_ms: limits.preparation_ms,
                package_digest,
                authority,
                phase,
                deadline,
                stop: StopToken::default(),
                payload,
            }),
        }; // Capture the immutable principal and physical request custody.
        state.pending.insert(
            id,
            Pending {
                resolver: v8::Global::new(scope, resolver),
                request: request.clone(),
            },
        ); // Keep the lease until both Promise and escaped native owners retire.
        state.requests.push_back(request); // Publish the complete snapshot as one bounded queue entry.
        Ok(()) // No native service has been claimed or executed by the transport itself.
    })(); // Validation and allocation failures drop every acquired guard.
    let admission_key = v8::String::new(scope, "__ilium_admitted"); // Trusted void controls can inspect native queue admission synchronously.
    let admitted = v8::Boolean::new(scope, attempted.is_ok()); // This says nothing about broker permission, native effect completion, or handle closure.
    if admission_key.is_none_or(|key| {
        promise.define_own_property(
            scope,
            key.into(),
            admitted.into(),
            v8::PropertyAttribute::READ_ONLY
                | v8::PropertyAttribute::DONT_DELETE
                | v8::PropertyAttribute::DONT_ENUM,
        ) != Some(true)
    }) {
        // Seal the receipt before package code can observe the Promise.
        bridge.borrow_mut().violation = Some("service queue admission receipt failed".into()); // Retire instead of exposing ambiguous admission after enqueueing.
        return; // The native owner will clear pending work through the existing failure path.
    } // Bootstrap must use a captured own-data descriptor and require Boolean true exactly.
    if let Err(error) = attempted {
        // Return a complete SDK error without a successful fake service.
        let metadata = serde_json::json!({"ok":false,"error":{"code":"host_request_rejected","message":error.to_string()}}); // Preserve a bounded explanatory error message.
        let result = service_into(scope, &metadata, &BTreeMap::new())
            .and_then(|value| resolve_service(scope, resolver, value)); // Resolve inert native data without inherited thenable hooks.
        if result.is_err() {
            bridge.borrow_mut().violation = Some("host rejection could not be settled".into());
        } // Retire rather than leave an unexplained hanging Promise.
    } // Successful requests remain pending until a separate native completion or terminal event.
} // No service dispatch callback, quota bank, or credential lookup is implemented here.
fn expire_requests(scope: &mut v8::PinScope, bridge: &Rc<RefCell<Bridge>>) -> Result<()> {
    // Expire resolvers while retaining native work custody.
    let now = Instant::now(); // Use one monotonic instant for this bounded sweep.
    let expired: Vec<_> = bridge
        .borrow()
        .pending
        .iter()
        .filter(|(_, pending)| pending.request.deadline <= now || pending.request.stop.is_stopped())
        .map(|(id, pending)| (*id, pending.request.deadline <= now))
        .collect(); // Use one deadline observation so a just-expiring request cannot be mislabeled as cancelled.
    for (id, timed_out) in expired {
        // Retire only the affected pending request.
        let (code, message) = if timed_out {
            ("timeout", "host request deadline exceeded")
        } else {
            ("cancelled", "host request cancelled")
        }; // Preserve the actual terminal reason.
        terminal_service(scope, bridge, id, code, message)?; // Publish cancellation inventory without prematurely freeing escaped payloads.
    } // The authorized pump owns the later reaction checkpoint.
    Ok(()) // The pump's later checkpoint remains the only continuation boundary.
} // Pending-map removal never proves an escaped native body has exited.
fn resolve_module<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    attributes: v8::Local<'s, v8::FixedArray>,
    referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    // SAFETY: V8 invokes this callback with its current owner-thread context.
    v8::callback_scope!(unsafe scope,context);
    let resolved = (|| {
        if specifier.length() > 240 || attributes.length() != 0 {
            return None;
        }
        let specifier = specifier.to_rust_string_lossy(scope);
        let bridge = scope.get_slot::<Rc<RefCell<Bridge>>>()?.clone();
        let state = bridge.borrow();
        let base = state.module_ids.get(&referrer.script_id()?)?;
        let path = resolve_path(base, &specifier)?;
        state
            .modules
            .get(&path)
            .map(|module| v8::Local::new(scope, module))
    })();
    if resolved.is_none() {
        if let Some(message) = v8::String::new(
            scope,
            "Only declared package-local relative modules may be imported",
        ) {
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        }
    }
    resolved
}
fn resolve_path(base: &str, specifier: &str) -> Option<String> {
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None;
    }
    let mut components: Vec<_> = base
        .rsplit_once('/')
        .map_or(Vec::new(), |(directory, _)| directory.split('/').collect());
    for component in specifier.split('/') {
        match component {
            "." => {}
            ".." => {
                components.pop()?;
            }
            "" => return None,
            component => components.push(component),
        }
    }
    let path = components.join("/");
    if crate::package::valid_path(&path)
        && (path == "entry.mjs" || (path.starts_with("modules/") && path.ends_with(".mjs")))
    {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)] // Keep native inspectors test-only.
pub(crate) mod inventory_contracts {
    // Share the unit-test initializer.
    use super::*; // Exercise private production paths.
    use serde_json::json; // Build bounded native fixtures.
    use sha2::{Digest, Sha256}; // Hash immutable fixture source.
    use std::io::{Cursor, Write}; // Build the archive in memory.
    pub(crate) fn fixture_lock() -> (std::sync::MutexGuard<'static, ()>, QuotaGroup) {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
        (guard, quota())
    }
    pub(crate) type ReleaseSignal = (
        std::sync::mpsc::SyncSender<()>,
        std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    );
    pub(crate) fn release_signal() -> &'static ReleaseSignal {
        static SIGNAL: OnceLock<ReleaseSignal> = OnceLock::new();
        SIGNAL.get_or_init(|| {
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            (sender, std::sync::Mutex::new(receiver))
        })
    }
    pub(crate) fn quota() -> QuotaGroup {
        // Use one original test root.
        static QUOTA: OnceLock<QuotaGroup> = OnceLock::new(); // Create no fallback root.
        let quota = QUOTA
            .get_or_init(|| {
                QuotaGroup::new_with_admission_wake(
                    ilium_execution::QuotaLimits {
                        clients: 32,
                        jobs: 32,
                        service_jobs: 32,
                        input_bytes: 32 * 1024 * 1024,
                        result_bytes: 32 * 1024 * 1024,
                        worker_threads: 32,
                        worker_bytes: 2048 * 1024 * 1024,
                    },
                    || {
                        let _ = release_signal().0.try_send(());
                    },
                )
            })
            .clone(); // Match frozen test quota limits.
        initialize_engine_for_tests(quota.clone(), 1).unwrap(); // Initialize the shared native platform.
        quota // Return the existing root.
    } // End shared initialization.
    pub(crate) fn package(source: &str) -> Arc<Package> {
        // Use real package validation.
        let manifest = json!({"api_version":1,"id":"inventory-contract","name":"Inventory","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]}); // Preserve actual file inventory.
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new())); // Own temporary archive bytes.
        for (path, bytes) in [
            ("entry.mjs", source.as_bytes().to_vec()),
            ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ] {
            // Include entry and manifest.
            zip.start_file(
                path,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap(); // Match frozen ZIP construction.
            zip.write_all(&bytes).unwrap(); // Write exact source bytes.
        } // End archive inventory.
        Arc::new(
            Package::from_bytes(
                &zip.finish().unwrap().into_inner(),
                crate::package::PackageLimits::default(),
            )
            .unwrap(),
        ) // Validate the completed package.
    } // End package fixture.
    const BOOTSTRAP: &str = r#" // Isolate native buffer custody.
globalThis.traps=0;globalThis.render_called=false;globalThis.fail_finish=false; // Record explicit observations.
globalThis.working=new Float32Array(4);globalThis.sealed=new Float32Array(4);globalThis.input=new Uint8Array(2);globalThis.held=[working.buffer,sealed.buffer,input.buffer]; // Retain exact buffers before failure.
globalThis.__ilium_host={}; // Expose no service authority.
globalThis.frame={gray:working,present(){sealed.set(working);}};for(const name of ['cells','pixels'])Object.defineProperty(frame,name,{get(){traps++;throw Error('drawing getter');}}); // Make getter fallback observable.
globalThis.__ilium_make_frame=()=>frame; // Return known buffer owners.
globalThis.__ilium_frame_buffers=()=>{globalThis.inventory_phase=__ilium_service_phase();if(inventory_phase!==4)throw Error('inventory phase');return INVENTORY;}; // Require native inventory phase4.
globalThis.__ilium_finish_frame=()=>{globalThis.finish_phase=__ilium_service_phase();if(finish_phase!==5)throw Error('finish phase');if(fail_finish)throw Error('finish failure');return {metadata:{submitted:true},planes:{gray:sealed}};}; // Require native finish phase5.
globalThis.__ilium_accept_frame=()=>{}; // Preserve logical acknowledgement.
globalThis.__ilium_seed_frame=(metadata,planes)=>{input=planes.source;held=[working.buffer,sealed.buffer,input.buffer];}; // Capture separately seeded input.
"#; // Keep inspectors outside script.
    fn engine(source: &str, inventory: &str) -> Engine {
        // Construct the actual native engine.
        let package = package(source);
        let digest = package.digest().to_owned(); // Bind the immutable package digest.
        let mut engine = Engine::new(package, EngineLimits::default(), quota()).unwrap(); // Preserve default engine limits.
        engine
            .install_bootstrap(&BOOTSTRAP.replace("INVENTORY", inventory))
            .unwrap(); // Install the trusted test expression.
        engine
            .bind_service_authority(
                &digest,
                ServiceAuthority {
                    instance_id: 1,
                    plan_generation: 1,
                    authorization_epoch: 1,
                },
            )
            .unwrap(); // Bind native fixture coordinates.
        engine.load().unwrap();
        assert_eq!(
            engine.start_create(&json!({}), &json!({})).unwrap(),
            CreateState::Ready
        ); // Reach synchronous render readiness.
        engine // Retain owner-thread custody.
    } // End native constructor.
    fn held_buffers(engine: &mut Engine) -> Vec<v8::Global<v8::ArrayBuffer>> {
        // Capture buffers before invalidation.
        v8::scope!(let scope,engine.isolate.as_mut().unwrap()); // Use the original isolate owner.
        let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
        let scope = &mut v8::ContextScope::new(scope, context); // Read known valid fixture data.
        let global = context.global(scope);
        let held =
            v8::Local::<v8::Array>::try_from(property(scope, global, "held").unwrap()).unwrap(); // Read the trusted own data property.
        (0..held.length())
            .map(|index| {
                let buffer =
                    v8::Local::<v8::ArrayBuffer>::try_from(held.get_index(scope, index).unwrap())
                        .unwrap();
                v8::Global::new(scope, buffer)
            })
            .collect() // Retain native backing identities.
    } // Run no script after failure.
    fn detached(engine: &mut Engine, buffers: &[v8::Global<v8::ArrayBuffer>]) -> Vec<bool> {
        // Inspect actual detached flags.
        v8::scope!(let scope,engine.isolate.as_mut().unwrap()); // Create only native local handles.
        buffers
            .iter()
            .map(|buffer| v8::Local::new(scope, buffer).was_detached())
            .collect() // Read V8's internal flag.
    } // Execute no JavaScript or checkpoint.
    #[test] // Check restoration before finish resets.
    fn private_inventory_restores_phase_on_all_results_and_deduplicates_native_buffers() {
        let (_serial, _root) = fixture_lock();
        // Assert actual native custody.
        let cases = [ // Count only discovered buffers.
            ("Object.fromEntries(Array.from({length:48},(_,i)=>[String(i),working]))",true,1), // Deduplicate forty-eight aliases.
            ("Object.defineProperty({a:working},'b',{get(){traps++;throw Error('inventory getter');}})",false,1), // Reject a later accessor.
            ("Object.assign({a:working},{[Symbol('hidden')]:sealed})",false,1), // Reject hidden symbols.
            ("Object.fromEntries(Array.from({length:49},(_,i)=>[String(i),working]))",false,0), // Enforce forty-eight own keys.
            ("new Proxy({a:working},{ownKeys(){traps++;throw Error('proxy');}})",false,0), // Reject proxies before traps.
            ("[working]",false,0), // Reject array inventories.
            ("null",false,0), // Reject a present null inventory.
            ("{a:1}",false,0), // Require native view leaves.
        ]; // Pin every native oracle.
        for (inventory, valid, count) in cases {
            // Isolate terminal cases.
            let mut engine = engine(
                "export async function create(){return {render(){}};}",
                inventory,
            ); // Exercise private collection directly.
            engine
                .begin(Phase::Render, engine.limits.render_ms)
                .unwrap(); // Arm the existing watchdog once.
            let deadline = engine.control.state.lock().unwrap().deadline;
            let bridge = Rc::clone(&engine.bridge); // Capture phase and deadline.
            {
                // Bound all temporary native handles.
                v8::scope!(let scope,engine.isolate.as_mut().unwrap());
                let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
                let scope = &mut v8::ContextScope::new(scope, context); // Enter the original context.
                v8::tc_scope!(let scope,scope);
                let global = context.global(scope);
                let frame =
                    v8::Local::<v8::Object>::try_from(property(scope, global, "frame").unwrap())
                        .unwrap();
                let mut buffers = Vec::new(); // Borrow trusted fixture data.
                let result = with_frame_phase(&bridge, Phase::FrameInventory, || {
                    assert_eq!(bridge.borrow().phase, Phase::FrameInventory);
                    collect_native_frame_buffers(scope, global, frame, &mut buffers)
                }); // Call the real guarded collector.
                assert_eq!(result.is_ok(), valid);
                assert_eq!(buffers.len(), count); // Check exact native retention.
                assert_eq!(bridge.borrow().phase, Phase::Render); // Check immediate Ok/Err restoration.
                assert_eq!(
                    property(scope, global, "inventory_phase")
                        .unwrap()
                        .number_value(scope),
                    Some(4.0)
                ); // Verify observed native phase4.
                assert_eq!(
                    property(scope, global, "traps")
                        .unwrap()
                        .number_value(scope),
                    Some(0.0)
                ); // Execute no accessors or getters.
                for buffer in buffers {
                    let buffer = v8::Local::new(scope, &buffer);
                    assert!(!buffer.was_detached());
                    assert_eq!(buffer.detach(None), Some(true));
                    assert!(buffer.was_detached());
                } // Detach each distinct buffer once.
            } // Drop handles before Engine.
            assert_eq!(engine.control.state.lock().unwrap().deadline, deadline);
            // Preserve the original deadline.
        } // Use normal Engine retirement.
    } // End collector contracts.
    #[test] // Check bounded native finishing.
    fn private_finish_restores_render_phase_and_original_deadline() {
        let (_serial, _root) = fixture_lock();
        // Call the real finish hook.
        for fail in [false, true] {
            // Cover both hook outcomes.
            let mut engine = engine(
                "export async function create(){return {render(){}};}",
                "{working,sealed,input}",
            ); // Use actual fixture buffers.
            engine
                .evaluate_json(if fail {
                    "fail_finish=true"
                } else {
                    "fail_finish=false"
                })
                .unwrap(); // Select the trusted hook outcome.
            engine
                .begin(Phase::Render, engine.limits.render_ms)
                .unwrap();
            let deadline = engine.control.state.lock().unwrap().deadline;
            let bridge = Rc::clone(&engine.bridge); // Preserve the running watchdog.
            {
                // Bound native hook handles.
                v8::scope!(let scope,engine.isolate.as_mut().unwrap());
                let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
                let scope = &mut v8::ContextScope::new(scope, context);
                v8::tc_scope!(let scope,scope); // Catch the intentional exception.
                let global = context.global(scope);
                let hook = function_property(scope, global, "__ilium_finish_frame").unwrap(); // Read the sealed native hook.
                let result = with_frame_phase(&bridge, Phase::FrameFinish, || {
                    assert_eq!(bridge.borrow().phase, Phase::FrameFinish);
                    hook.call(scope, global.into(), &[])
                        .ok_or_else(|| runtime("fixture finish error"))
                }); // Execute under native phase5.
                assert_eq!(result.is_err(), fail);
                assert_eq!(bridge.borrow().phase, Phase::Render); // Restore both outcomes immediately.
            } // Drop the caught exception scope.
            assert_eq!(engine.control.state.lock().unwrap().deadline, deadline);
            // Preserve original execution time.
        } // Restore the caller's phase.
    } // End finish restoration test.
    #[test] // Forbid private-phase acquisition.
    fn private_frame_phases_refuse_native_dispatch_without_a_checkpoint() {
        let (_serial, _root) = fixture_lock();
        // Probe the real dispatch callback.
        for phase in [Phase::FrameInventory, Phase::FrameFinish] {
            // Cover both nonacquiring gates.
            let mut engine = engine(
                "export async function create(){return {render(){}};}",
                "{working,sealed,input}",
            );
            let bridge = Rc::clone(&engine.bridge); // Use current native coordinates.
            engine
                .begin(Phase::Render, engine.limits.render_ms)
                .unwrap(); // Preserve the render deadline.
            {
                // Confine the intentional violation.
                v8::scope!(let scope,engine.isolate.as_mut().unwrap());
                let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
                let scope = &mut v8::ContextScope::new(scope, context); // Enter the native callback realm.
                let source = v8::String::new(scope,"__ilium_dispatch('http.request',{}).then(()=>{globalThis.phase_reaction=true;})").unwrap();
                let script = v8::Script::compile(scope, source, None).unwrap(); // Detect any forbidden checkpoint.
                with_frame_phase(&bridge, phase, || {
                    script
                        .run(scope)
                        .ok_or_else(|| runtime("phase fixture execution"))
                })
                .unwrap(); // Attempt real native acquisition.
                assert_eq!(bridge.borrow().phase, Phase::Render);
                assert!(bridge.borrow().violation.is_some());
                assert!(bridge.borrow().pending.is_empty()); // Reject and restore before teardown.
                let global = context.global(scope);
                assert!(property(scope, global, "phase_reaction")
                    .unwrap()
                    .is_undefined()); // Leave the reaction unexecuted.
            } // Keep Promise handles local.
            assert_eq!(engine.service_usage(), (0, 0)); // Admit no request resources.
        } // Retire the violating Engine.
    } // End acquisition gate test.
    #[test] // Inspect detachment after failure.
    fn thrown_render_detaches_working_sealed_and_native_seeded_input_buffers() {
        let (_serial, _root) = fixture_lock();
        // Clean inventory and seed custody.
        let mut engine = engine("export async function create(){return {render(c,f){globalThis.render_called=true;f.gray.fill(0.5);throw Error('render failure');}};}","{working,sealed,input}"); // Throw after obtaining the alias.
        engine
            .seed_frame(
                &json!({}),
                &[ArraySpec {
                    name: "source".into(),
                    kind: TypedArrayKind::U8,
                    elements: 2,
                }],
                &BTreeMap::from([("source".into(), vec![3, 7])]),
            )
            .unwrap(); // Install a real native input seed.
        let buffers = held_buffers(&mut engine);
        assert_eq!(detached(&mut engine, &buffers), vec![false, false, false]); // Capture exact pre-failure owners.
        assert!(engine
            .render(
                &json!({}),
                &[ArraySpec {
                    name: "gray".into(),
                    kind: TypedArrayKind::F32,
                    elements: 4
                }]
            )
            .is_err()); // Exercise render's final cleanup.
        assert!(engine.is_invalid());
        assert_eq!(detached(&mut engine, &buffers), vec![true, true, true]); // Physically detach all three buffers.
        drop(buffers); // Drop Globals before Engine.
    } // Assert V8 flags directly.
    #[test] // Clean partial failed inventory.
    fn malformed_inventory_detaches_known_buffer_and_native_seed_on_error() {
        let (_serial, _root) = fixture_lock();
        // Preserve discovered custody.
        let mut engine = engine("export async function create(){return {render(){globalThis.render_called=true;}};}","Object.defineProperty({a:working},'b',{get(){traps++;throw Error('inventory getter');}})"); // Fail after one known view.
        engine
            .seed_frame(
                &json!({}),
                &[ArraySpec {
                    name: "source".into(),
                    kind: TypedArrayKind::U8,
                    elements: 2,
                }],
                &BTreeMap::from([("source".into(), vec![1, 2])]),
            )
            .unwrap(); // Keep seed cleanup independent.
        let buffers = held_buffers(&mut engine); // Retain pre-failure native identities.
        assert!(engine
            .render(
                &json!({}),
                &[ArraySpec {
                    name: "gray".into(),
                    kind: TypedArrayKind::F32,
                    elements: 4
                }]
            )
            .is_err()); // Fail without getter fallback.
        assert!(engine.is_invalid());
        assert_eq!(detached(&mut engine, &buffers), vec![true, false, true]); // Detach known working/input; sealed was unlisted.
        drop(buffers); // Engine owns undiscovered storage.
    } // Collector tests assert zero getters.

    fn split_seed_engine(hook_body: &str) -> Engine {
        let package = package("export async function create(){return {render(){}};}");
        let digest = package.digest().to_owned();
        let mut engine = Engine::new(package, EngineLimits::default(), quota()).unwrap();
        let bootstrap = BOOTSTRAP.replace("INVENTORY", "{working,sealed,input}")
            .replace("globalThis.__ilium_seed_frame=(metadata,planes)=>{input=planes.source;held=[working.buffer,sealed.buffer,input.buffer];};",
                &format!("globalThis.seed_calls=0;globalThis.__ilium_seed_frame=(metadata,planes)=>{{seed_calls++;{hook_body}}};"));
        engine.install_bootstrap(&bootstrap).unwrap();
        engine
            .bind_service_authority(
                &digest,
                ServiceAuthority {
                    instance_id: 1,
                    plan_generation: 1,
                    authorization_epoch: 1,
                },
            )
            .unwrap();
        engine.load().unwrap();
        assert_eq!(
            engine.start_create(&json!({}), &json!({})).unwrap(),
            CreateState::Ready
        );
        engine
    }
    fn staged_buffers(engine: &mut Engine) -> Vec<v8::Global<v8::ArrayBuffer>> {
        v8::scope!(let scope, engine.isolate.as_mut().unwrap());
        engine
            .seeded_buffers
            .iter()
            .map(|buffer| {
                let local = v8::Local::new(scope, buffer);
                v8::Global::new(scope, local)
            })
            .collect()
    }
    fn raw_seed_observations(engine: &mut Engine) -> (i32, bool) {
        v8::scope!(let scope, engine.isolate.as_mut().unwrap());
        let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
        let scope = &mut v8::ContextScope::new(scope, context);
        let global = context.global(scope);
        let calls = property(scope, global, "seed_calls")
            .unwrap()
            .int32_value(scope)
            .unwrap();
        let ran = property(scope, global, "seed_reaction").unwrap().is_true();
        (calls, ran) // Native reads; no guest evaluation or checkpoint.
    }
    #[test]
    fn inert_seed_copy_has_no_hook_or_checkpoint_and_discard_detaches() {
        let (_serial, _root) = fixture_lock();
        let mut engine = split_seed_engine(
            "input=planes.source;Promise.resolve().then(()=>{globalThis.seed_reaction=true;});",
        );
        let id = engine
            .prepare_frame_seed(
                &json!({}),
                &[ArraySpec {
                    name: "source".into(),
                    kind: TypedArrayKind::U8,
                    elements: 2,
                }],
                &BTreeMap::from([("source".into(), vec![3, 7])]),
            )
            .unwrap();
        let buffers = staged_buffers(&mut engine);
        assert_eq!(raw_seed_observations(&mut engine), (0, false));
        assert!(engine.pump().is_err());
        assert!(engine.evaluate_json("globalThis.seed_calls").is_err());
        assert!(!engine.is_invalid());
        engine.discard_frame_seed(id).unwrap();
        assert_eq!(detached(&mut engine, &buffers), vec![true]);
        assert_eq!(raw_seed_observations(&mut engine), (0, false));
        drop(buffers);
    }
    #[test]
    fn seed_activation_is_single_use_synchronous_without_checkpoint() {
        let (_serial, _root) = fixture_lock();
        let mut engine = split_seed_engine(
            "input=planes.source;Promise.resolve().then(()=>{globalThis.seed_reaction=true;});",
        );
        let id = engine
            .prepare_frame_seed(
                &json!({}),
                &[ArraySpec {
                    name: "source".into(),
                    kind: TypedArrayKind::U8,
                    elements: 2,
                }],
                &BTreeMap::from([("source".into(), vec![3, 7])]),
            )
            .unwrap();
        assert!(engine.activate_frame_seed(id + 1).is_err());
        assert_eq!(raw_seed_observations(&mut engine), (0, false));
        engine.activate_frame_seed(id).unwrap();
        assert_eq!(raw_seed_observations(&mut engine), (1, false));
        assert!(engine.activate_frame_seed(id).is_err());
        engine.pump().unwrap();
        assert_eq!(raw_seed_observations(&mut engine), (1, true));
    }
    #[test]
    fn seed_hook_throw_and_native_cancellation_detach_original_views() {
        let (_serial, _root) = fixture_lock();
        for should_throw in [true, false] {
            let mut engine =
                split_seed_engine("input=planes.source;throw Error('synthetic hook failure');");
            let id = engine
                .prepare_frame_seed(
                    &json!({}),
                    &[ArraySpec {
                        name: "source".into(),
                        kind: TypedArrayKind::U8,
                        elements: 2,
                    }],
                    &BTreeMap::from([("source".into(), vec![3, 7])]),
                )
                .unwrap();
            let buffers = staged_buffers(&mut engine);
            if should_throw {
                assert!(engine.activate_frame_seed(id).is_err());
            } else {
                engine.cancel();
            }
            assert!(engine.is_invalid());
            assert_eq!(detached(&mut engine, &buffers), vec![true]);
            assert!(engine.staged_seed.is_none());
            drop(buffers);
        }
    }
    #[test]
    fn native_seed_shape_count_caps_remain_48_and_refuse_before_copy() {
        let mut engine = split_seed_engine("input=planes.source;");
        let specs: Vec<_> = (0..48)
            .map(|n| ArraySpec {
                name: format!("input_{n}"),
                kind: TypedArrayKind::U8,
                elements: 2,
            })
            .collect();
        let mut planes: BTreeMap<_, _> =
            specs.iter().map(|s| (s.name.clone(), vec![3, 7])).collect();
        let mut excess = specs.clone();
        excess.push(ArraySpec {
            name: "input_48".into(),
            kind: TypedArrayKind::U8,
            elements: 2,
        });
        planes.insert("input_48".into(), vec![1, 2]);
        let before = engine.backing_bytes();
        assert!(engine
            .prepare_frame_seed(&json!({}), &excess, &planes)
            .is_err());
        assert_eq!(engine.backing_bytes(), before);
        assert!(engine.seeded_buffers.is_empty());
        planes.remove("input_48");
        let id = engine
            .prepare_frame_seed(&json!({}), &specs, &planes)
            .unwrap();
        assert_eq!(engine.seeded_buffers.len(), 48);
        engine.discard_frame_seed(id).unwrap();
    }
    #[test]
    fn inert_seed_preserves_all_four_native_typed_payload_bits() {
        let (_serial, _root) = fixture_lock();
        let mut engine = split_seed_engine("input=planes.source;");
        let cases = [
            ("u8", TypedArrayKind::U8, vec![0, 255]),
            (
                "f32",
                TypedArrayKind::F32,
                [f32::NAN.to_ne_bytes(), (-0.0f32).to_ne_bytes()].concat(),
            ),
            (
                "u16",
                TypedArrayKind::U16,
                [0u16.to_ne_bytes(), 65535u16.to_ne_bytes()].concat(),
            ),
            (
                "u32",
                TypedArrayKind::U32,
                [0u32.to_ne_bytes(), u32::MAX.to_ne_bytes()].concat(),
            ),
        ];
        let specs: Vec<_> = cases
            .iter()
            .map(|(name, kind, _)| ArraySpec {
                name: (*name).into(),
                kind: *kind,
                elements: 2,
            })
            .collect();
        let planes = cases
            .iter()
            .map(|(name, _, data)| ((*name).into(), data.clone()))
            .collect();
        let id = engine
            .prepare_frame_seed(&json!({}), &specs, &planes)
            .unwrap();
        {
            v8::scope!(let scope, engine.isolate.as_mut().unwrap());
            let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
            let scope = &mut v8::ContextScope::new(scope, context);
            let supplied = v8::Local::new(scope, &engine.staged_seed.as_ref().unwrap().planes);
            for (name, _, expected) in &cases {
                let value = property(scope, supplied, name).unwrap();
                let view = v8::Local::<v8::ArrayBufferView>::try_from(value).unwrap();
                let mut copied = vec![0; expected.len()];
                assert_eq!(view.copy_contents(&mut copied), expected.len());
                assert_eq!(&copied, expected);
            }
        }
        engine.discard_frame_seed(id).unwrap();
    }
    #[test]
    fn pre_render_ambient_is_bound_before_guest_module_evaluation() {
        fn capture() -> Value {
            let source = r#"
                const savedDate = Date;
                const savedRandom = Math.random;
                const moduleNow = savedDate.now();
                const calledWithArguments = savedDate(2001, 1, 1);
                const expectedCall = new savedDate(0).toString();
                const firstRandom = savedRandom();
                const utcConstructed = new savedDate(2020, 0, 1, 12, 30);
                let localeParseRejected = false;
                try { savedDate.parse("Jan 1 2020"); } catch { localeParseRejected = true; }
                export function plan() {
                    return { moduleNow, calledWithArguments, expectedCall,
                        firstRandom, nextRandom: savedRandom(), savedNow: savedDate.now(),
                        retainedConstructor: new savedDate(0).constructor === savedDate,
                        utcConstructed: utcConstructed.toISOString(),
                        timezoneOffset: utcConstructed.getTimezoneOffset(),
                        nativePrototypeHidden: Object.getPrototypeOf(utcConstructed) === savedDate.prototype
                            && Object.getPrototypeOf(savedDate.prototype) === null,
                        ambientFacilitiesHidden: [Intl, globalThis.Temporal, globalThis.performance,
                            globalThis.crypto, WeakRef, FinalizationRegistry,
                            SharedArrayBuffer, Atomics].every(x => x === undefined),
                        localeParseRejected };
                }
            "#;
            let package = package(source);
            let mut engine = Engine::new(package, EngineLimits::default(), quota()).unwrap();
            engine.install_bootstrap(crate::TRUSTED_BOOTSTRAP).unwrap();
            engine
                .configure_ambient(AnimationMode::PreRendered, 77)
                .unwrap();
            engine.load().unwrap();
            engine
                .plan(&json!({}), AnimationMode::PreRendered, &json!({}))
                .unwrap()
        }
        let first = capture();
        let second = capture();
        assert_eq!(first, second, "a new native realm resets the same seed");
        assert_eq!(first["moduleNow"], 0);
        assert_eq!(first["savedNow"], 0);
        assert_eq!(first["calledWithArguments"], first["expectedCall"]);
        assert_eq!(first["retainedConstructor"], true);
        assert_eq!(first["utcConstructed"], "2020-01-01T12:30:00.000Z");
        assert_eq!(first["timezoneOffset"], 0);
        assert_eq!(first["nativePrototypeHidden"], true);
        assert_eq!(first["ambientFacilitiesHidden"], true);
        assert_eq!(first["localeParseRejected"], true);
        assert_ne!(first["firstRandom"], first["nextRandom"]);
    }
} // End native frame contracts.

#[cfg(test)]
#[path = "engine_boundary_tests.rs"]
pub(crate) mod boundary_tests;

#[cfg(all(test, feature = "native-host", feature = "native-network"))]
mod pure_source_contracts {
    use super::*;
    use serde_json::{json, Value};

    fn engine(quota: QuotaGroup) -> Engine {
        let package = inventory_contracts::package(
            "export function plan(){return {}} export async function create(){return {render(){},dispose(){}}}",
        );
        let mut engine = Engine::new(package, EngineLimits::default(), quota).unwrap();
        engine.install_bootstrap(crate::TRUSTED_BOOTSTRAP).unwrap();
        engine
    }
    fn number(value: &Value) -> f64 {
        value.as_f64().expect("native numeric field")
    }
    fn close(actual: &Value, expected: &Value) {
        let actual = number(actual);
        let expected = number(expected);
        assert!(
            (actual - expected).abs() < 1.0e-12,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn projection_is_synchronous_matches_native_math_and_rejects_untrusted_fields() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let mut engine = engine(quota);
        for (latitude, longitude, projection) in [
            (48.0, 2.0, "equirectangular"),
            (90.0, 180.0, "mercator"),
            (-90.0, -180.0, "mercator"),
            (0.0, 180.0, "orthographic"),
            (45.0, 45.0, "orthographic"),
        ] {
            let script = format!(
                "__ilium_host.sources.geography.project({{latitude:{latitude},longitude:{longitude},projection:'{projection}'}})"
            );
            let actual = engine.evaluate_json(&script).unwrap();
            let expected =
                crate::sources::geography::project(latitude, longitude, projection).unwrap();
            assert_eq!(actual["visible"], expected["visible"]);
            close(&actual["x"], &expected["x"]);
            close(&actual["y"], &expected["y"]);
        }
        let invalid = engine
            .evaluate_json(
                r#"(() => {
            const outcomes = [];
            for (const options of [
                {latitude:91,longitude:0,projection:'mercator'},
                {latitude:0,longitude:181,projection:'mercator'},
                {latitude:NaN,longitude:0,projection:'mercator'},
                {latitude:0,longitude:0,projection:'unknown'},
                {latitude:0,longitude:0,projection:'mercator',extra:1},
                {latitude:'0',longitude:0,projection:'mercator'}
            ]) { try { __ilium_host.sources.geography.project(options); outcomes.push(false); }
                 catch (error) { outcomes.push(error instanceof TypeError); } }
            return outcomes;
        })()"#,
            )
            .unwrap();
        assert_eq!(invalid, json!([true, true, true, true, true, true]));
        let traps = engine
            .evaluate_json(
                r#"(() => {
            let traps=0;
            const accessor={longitude:0,projection:'mercator'};
            Object.defineProperty(accessor,'latitude',{enumerable:true,get(){traps++;return 0;}});
            try { __ilium_host.sources.geography.project(accessor); } catch (_) {}
            const proxy=new Proxy({latitude:0,longitude:0,projection:'mercator'},
                {ownKeys(){traps++;return ['latitude','longitude','projection'];}});
            try { __ilium_host.sources.geography.project(proxy); } catch (_) {}
            return traps;
        })()"#,
            )
            .unwrap();
        assert_eq!(traps, json!(0));
        assert!(!engine.is_invalid()); // Caught validation failures do not poison ordinary guest work.
    }

    #[test]
    fn observation_is_synchronous_matches_native_bodies_and_preserves_units() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let mut engine = engine(quota);
        for epoch_ms in [-5_364_662_400_000_i64, 1_700_000_000_000, 2_524_521_600_000] {
            let script = format!(
                "__ilium_host.sources.astronomy.observe({{epoch_ms:{epoch_ms},latitude:48,longitude:2}})"
            );
            let result = engine.evaluate_json(&script).unwrap();
            assert_eq!(result["ok"], true);
            let actual = &result["value"];
            let expected = crate::sources::astronomy::observe(epoch_ms, 48.0, 2.0).unwrap();
            assert_eq!(actual["epoch_ms"], expected["epoch_ms"]);
            assert_eq!(actual["units"], "unit_direction_not_physical_position");
            assert_eq!(actual["heliocentric"]["frame"], "J2000_ecliptic");
            assert_eq!(actual["heliocentric"]["units"], "astronomical_units");
            assert_eq!(actual["bodies"].as_array().unwrap().len(), 7);
            assert_eq!(
                actual["heliocentric"]["bodies"].as_array().unwrap().len(),
                8
            );
            close(&actual["julian_date"], &expected["julian_date"]);
            close(
                &actual["local_sidereal_degrees"],
                &expected["local_sidereal_degrees"],
            );
            close(
                &actual["bodies"][0]["altitude_degrees"],
                &expected["bodies"][0]["altitude_degrees"],
            );
            close(
                &actual["bodies"][1]["east_north_up"][2],
                &expected["bodies"][1]["east_north_up"][2],
            );
            close(
                &actual["heliocentric"]["bodies"][4]["position_au"][0],
                &expected["heliocentric"]["bodies"][4]["position_au"][0],
            );
            assert_eq!(actual["bodies"][0]["magnitude"], Value::Null);
        }
        let invalid = engine.evaluate_json(r#"(() => [
            __ilium_host.sources.astronomy.observe({epoch_ms:-5396198400000,latitude:0,longitude:0}),
            __ilium_host.sources.astronomy.observe({epoch_ms:2524608000000,latitude:0,longitude:0}),
            __ilium_host.sources.astronomy.observe({epoch_ms:1.5,latitude:0,longitude:0}),
            __ilium_host.sources.astronomy.observe({epoch_ms:0,latitude:Infinity,longitude:0}),
            __ilium_host.sources.astronomy.observe({epoch_ms:0,latitude:0,longitude:0,extra:1})
        ].map(value => value.ok === false && value.error.code === 'invalid_request'))()"#).unwrap();
        assert_eq!(invalid, json!([true, true, true, true, true]));
        let traps = engine
            .evaluate_json(
                r#"(() => {
            let traps=0;
            const options={epoch_ms:0,longitude:0};
            Object.defineProperty(options,'latitude',{enumerable:true,get(){traps++;return 0;}});
            const result=__ilium_host.sources.astronomy.observe(options);
            return [result.ok,traps];
        })()"#,
            )
            .unwrap();
        assert_eq!(traps, json!([false, 0]));
        assert!(!engine.is_invalid());
    }

    #[test]
    fn quota_refusal_is_catchable_and_result_heap_charge_lasts_through_engine_owner() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let mut engine = engine(quota.clone());
        let before = quota.snapshot();
        let available = before.limits.worker_bytes - before.worker_bytes;
        assert!(available > PURE_SOURCE_SCRATCH_BYTES);
        let fill = quota
            .reserve_external_storage(available - PURE_SOURCE_SCRATCH_BYTES + 1)
            .unwrap();
        let refused = engine.evaluate_json(r#"(() => {
            try { __ilium_host.sources.geography.project({latitude:0,longitude:0,projection:'mercator'}); }
            catch (error) { return error instanceof TypeError && error.message.includes('quota'); }
            return false;
        })()"#).unwrap();
        assert_eq!(refused, json!(true));
        let observed = engine.evaluate_json(
            "__ilium_host.sources.astronomy.observe({epoch_ms:1700000000000,latitude:0,longitude:0})"
        ).unwrap();
        assert_eq!(observed["ok"], false);
        assert_eq!(observed["error"]["code"], "budget_exceeded");
        drop(fill);
        engine.evaluate_json("globalThis.kept=__ilium_host.sources.astronomy.observe({epoch_ms:1700000000000,latitude:0,longitude:0});kept.ok").unwrap();
        assert_eq!(quota.snapshot().worker_bytes, before.worker_bytes);
        assert_eq!(
            engine.evaluate_json("kept.value.bodies.length").unwrap(),
            json!(7)
        );
        drop(engine);
        assert!(quota.snapshot().worker_bytes < before.worker_bytes);
    }

    #[test]
    fn module_plan_and_create_call_the_pure_bridge_without_acquisition() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let script = r#"
            const at_load = __ilium_host.sources.geography.project({
                latitude:0,longitude:0,projection:'equirectangular'
            });
            export function plan(_settings, mode) {
                const observed=__ilium_host.sources.astronomy.observe({
                    epoch_ms:1700000000000,latitude:48,longitude:2
                });
                return {mode,load_x:at_load.x,observed:observed.ok,
                    body_count:observed.ok ? observed.value.bodies.length : 0};
            }
            export async function create(host) {
                const point=host.sources.geography.project({
                    latitude:45,longitude:45,projection:'orthographic'
                });
                globalThis.created_point=point;
                return {render(){},dispose(){}};
            }
        "#;
        let package = inventory_contracts::package(script);
        let digest = package.digest().to_owned();
        let mut engine = Engine::new(package, EngineLimits::default(), quota).unwrap();
        engine.install_bootstrap(crate::TRUSTED_BOOTSTRAP).unwrap();
        engine.load().unwrap();
        for (mode, label) in [
            (AnimationMode::Live, "live"),
            (AnimationMode::PreRendered, "pre_rendered"),
        ] {
            let plan = engine.plan(&json!({}), mode, &json!({})).unwrap();
            assert_eq!(plan["mode"], label);
            assert_eq!(plan["load_x"], 0.5);
            assert_eq!(plan["observed"], true);
            assert_eq!(plan["body_count"], 7);
        }
        engine
            .bind_service_authority(
                &digest,
                ServiceAuthority {
                    instance_id: 1,
                    plan_generation: 1,
                    authorization_epoch: 1,
                },
            )
            .unwrap();
        assert_eq!(
            engine.start_create(&json!({}), &json!({})).unwrap(),
            CreateState::Ready
        );
        assert_eq!(
            engine.evaluate_json("created_point.visible").unwrap(),
            json!(true)
        );
        assert!(engine.take_requests().unwrap().is_empty());
    }

    #[test]
    fn caught_deadline_failure_still_retires_the_engine() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let mut engine = engine(quota);
        engine.limits.evaluation_ms = 1;
        let result = engine.evaluate_json(r#"(() => {
            for (let index=0; index<1000000; index++) {
                try { __ilium_host.sources.geography.project({latitude:0,longitude:0,projection:'mercator'}); }
                catch (_) {}
            }
            return true;
        })()"#);
        assert!(result.is_err());
        assert!(engine.is_invalid());
    }

    #[test]
    fn text_measure_uses_the_existing_engine_quota_and_releases_font_scratch() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let before_engine = quota.snapshot().worker_bytes;
        let mut engine = engine(quota.clone());
        let baseline = quota.snapshot().worker_bytes;
        let measured = engine
            .evaluate_json("__ilium_host.text.measure({text:'A',size_px:16})")
            .unwrap();
        assert!(measured["width"].as_u64().is_some_and(|width| width > 0));
        assert_eq!(quota.snapshot().worker_bytes, baseline);

        // Leave room for V8's small result and diagnostic buffers while
        // refusing the existing 32 MiB native font setup charge. An
        // independent text quota would incorrectly make this call succeed.
        let snapshot = quota.snapshot();
        let available = snapshot.limits.worker_bytes - snapshot.worker_bytes;
        assert!(available > 32 * 1024 * 1024);
        let pressure = quota
            .reserve_external_storage(available - 16 * 1024 * 1024)
            .unwrap();
        let denied = engine
            .evaluate_json(
                r#"(() => {
                try { __ilium_host.text.measure({text:'A',size_px:16}); }
                catch (error) { return String(error.message); }
                return 'unexpected_success';
            })()"#,
            )
            .unwrap();
        assert!(denied
            .as_str()
            .is_some_and(|message| message.contains("budget_exceeded")));
        drop(pressure);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        assert!(engine
            .evaluate_json("__ilium_host.text.measure({text:'A'}).width")
            .unwrap()
            .as_u64()
            .is_some_and(|width| width > 0));
        drop(engine);
        assert_eq!(quota.snapshot().worker_bytes, before_engine);
    }

    #[test]
    fn native_text_deadline_and_cancellation_signal_the_original_callback_stop() {
        let (_serial, quota) = inventory_contracts::fixture_lock();
        let mut cancelled = engine(quota.clone());
        cancelled.cancel();
        assert!(cancelled.control.callback_stop.is_stopped());
        assert!(cancelled.bridge.borrow().callback_stop.is_stopped());
        assert!(cancelled
            .evaluate_json("__ilium_host.text.measure({text:'A'})")
            .is_err());
        drop(cancelled);

        let mut expired = engine(quota);
        expired.limits.evaluation_ms = 1;
        let result = expired.evaluate_json(
            r#"(() => {
            for (let index = 0; index < 1000000; index++) {
                try { __ilium_host.text.measure({text:'A'}); } catch (_) {}
            }
            return true;
        })()"#,
        );
        assert!(result.is_err());
        assert!(expired.is_invalid());
        assert!(expired.control.callback_stop.is_stopped());
        assert!(expired.bridge.borrow().callback_stop.is_stopped());
    }
}

#[cfg(test)]
#[path = "borrowed_service_contract_tests.rs"]
mod borrowed_service_contract_tests;

#[cfg(test)]
#[path = "world_region_guest_transport_tests.rs"]
mod guest_region_transport_tests;
