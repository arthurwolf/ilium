//! Typed native-math custody between admitted ServiceValue input and native completion copies. // No dispatcher or grant is invented here.
//! The owner must authorize baseline CPU through its actual private activation before submit. // Empty OperationNeed is not a baseline permit.
//! The owner must hold/recheck current native authority at the actual Engine/helper publication. // ServiceAuthority and epoch values only fence data.
//! Keep this owner independent of V8 helper retirement; revoke separately on native activation retirement. // Helper EOF/cancel is not native teardown.
use crate::{
    engine::{
        ArraySpec, CompletionState, EngineLimits, HostRequest, ServiceAuthority, ServiceValue,
        TypedArrayKind,
    },
    error::{AnimationError, Result},
    native_math::{
        parameters_from_wire, ComputeStatus, MathHandle, MathInput, MathOutput, MathRequest,
        NativeMath,
    },
}; // Reuse the exact supplied native APIs.
use ilium_ambient::resources::AmbientResources; // Use the original finite client.
use ilium_execution::{QuotaGroup, StorageAdmission}; // Use the original root for all copies.
use serde_json::{json, Value}; // JSON carries scalar metadata only.
use std::{collections::BTreeMap, sync::Arc}; // Keep native handles private.
const MAX_HANDLES: usize = 32; // Bound pending and terminal handles alike.
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native math bridge: {message}"))
} // Keep boundary failures explicit.
fn charge(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes)
        .map_err(|error| AnimationError::Budget(format!("native math bridge admission: {error:?}")))
} // Charge the original root.
#[derive(Clone)] // Share original issuer identity.
pub struct MathBridgeHandle {
    issuer: Arc<()>,
    id: u64,
} // No public ID reconstruction.
struct Entry {
    native: MathHandle,
    request: Option<HostRequest>,
    state: ComputeStatus,
    revision: u64,
} // Retain queued request/stop/deadline until actual submission ACK transfers ownership to the native job.
pub struct NativeMathBridge {
    native: NativeMath,
    entries: BTreeMap<u64, Entry>,
    issuer: Arc<()>,
    quota: QuotaGroup,
    package_digest: String,
    authority: ServiceAuthority,
    closed: bool,
    _metadata: StorageAdmission,
} // Registry admission releases last.
impl NativeMathBridge {
    // Authorization stays with the actual native owner.
    pub fn new(
        resources: AmbientResources,
        quota: QuotaGroup,
        package_digest: &str,
        authority: ServiceAuthority,
    ) -> Result<Self> {
        // Use the accepted native owner's existing resources.
        authority.validate()?; // Coordinates are not credentials.
        if package_digest.len() != 64
            || !package_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid("package digest"));
        } // Package identity is separate from epoch.
        let metadata = charge(&quota, MAX_HANDLES * 2048 + 1024)?; // Admit registry metadata before allocation.
        let native = NativeMath::new(resources, quota.clone(), authority.authorization_epoch)?; // NativeMath verifies original-bank root identity.
        Ok(Self {
            native,
            entries: BTreeMap::new(),
            issuer: Arc::new(()),
            quota,
            package_digest: package_digest.into(),
            authority,
            closed: false,
            _metadata: metadata,
        }) // Retain native custody only.
    } // End native owner construction.
    fn owner(&self, current: ServiceAuthority) -> Result<()> {
        if self.closed || current != self.authority {
            return Err(invalid("stale native owner"));
        }
        Ok(())
    } // Native coordinates are a fence; the actual private activation still authorizes publication.
    fn active(&self, request: &HostRequest, current: ServiceAuthority) -> Result<()> {
        // Validate native data fences.
        if self.closed
            || current != self.authority
            || request.authority != self.authority
            || request.package_digest != self.package_digest
            || !request.payload.shares_root(&self.quota)
            || request.is_cancelled()
        {
            return Err(invalid("stale, foreign, cancelled, or expired request"));
        } // Recheck at every actual bridge handoff.
        Ok(()) // The actual owner still gates effects and publication.
    } // End native data-fence validation.
    fn entry(&self, handle: &MathBridgeHandle) -> Result<&Entry> {
        // Require the original opaque handle.
        if !Arc::ptr_eq(&self.issuer, &handle.issuer) {
            return Err(invalid("foreign handle issuer"));
        } // Equal IDs are not ownership.
        self.entries
            .get(&handle.id)
            .ok_or_else(|| invalid("retired handle")) // Native IDs are never reused.
    } // End immutable registry access.
      // Test-only immutable observation of the genuine retained native result.
      // It neither constructs handles nor bypasses issuer/current-owner fences.
    #[cfg(test)]
    pub(crate) fn observe_retained_output(
        &self,
        handle: &MathBridgeHandle,
        current: ServiceAuthority,
    ) -> Result<Option<Arc<MathOutput>>> {
        self.owner(current)?;
        Ok(match &self.entry(handle)?.state {
            ComputeStatus::Ready(output) => Some(Arc::clone(output)),
            _ => None,
        })
    }
    pub fn submit(
        &mut self,
        request: &HostRequest,
        current: ServiceAuthority,
    ) -> Result<MathBridgeHandle> {
        // Caller first checks its actual native CPU baseline owner, outside any worker wait.
        self.active(request, current)?; // Validate request before decoding.
        if request.method != "compute.submit" || self.entries.len() >= MAX_HANDLES {
            return Err(invalid("submit method or retained handle limit"));
        } // Terminal outputs retain bounded slots.
        let fields = request
            .payload
            .metadata()
            .as_object()
            .ok_or_else(|| invalid("submit record"))?; // Borrow immutable native metadata.
        if fields.len() != 5
            || !["kernel", "input", "parameters", "max_bytes", "timeout_ms"]
                .iter()
                .all(|name| fields.contains_key(*name))
        {
            return Err(invalid("submit fields"));
        } // Require the exact SDK schema.
        let kernel = fields["kernel"]
            .as_str()
            .ok_or_else(|| invalid("kernel string"))?; // Native code validates the kernel.
        let parameters = fields["parameters"]
            .as_object()
            .ok_or_else(|| invalid("parameter record"))?; // Parameters remain scalar metadata.
        if parameters.len() > 24 || parameters.keys().any(|key| key.len() > 16) {
            return Err(invalid("parameter limits"));
        } // Bound metadata staging.
        let max_bytes = fields["max_bytes"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0 && *value <= 4 * 1024 * 1024)
            .ok_or_else(|| invalid("max_bytes"))?; // Preserve the native output ceiling.
        let timeout_ms = fields["timeout_ms"]
            .as_u64()
            .filter(|value| (1..=60_000).contains(value))
            .ok_or_else(|| invalid("timeout_ms"))?; // Preserve native timeout bounds.
        if request.payload.arrays().len() != 1
            || !fields["input"].as_object().is_some_and(|marker| {
                marker.len() == 1
                    && marker.get("$ilium_binary").and_then(Value::as_str) == Some("b0")
            })
        {
            return Err(invalid("single typed input"));
        } // Require exactly one typed input.
        let spec = &request.payload.arrays()[0]; // Borrow validated plane shape.
        if spec.kind != TypedArrayKind::F32 || spec.elements == 0 || spec.elements > 262_144 {
            return Err(invalid("float32 input shape"));
        } // SDK math input is F32 only.
        let source = request
            .payload
            .planes()
            .get("b0")
            .ok_or_else(|| invalid("missing input plane"))?; // Retain original guarded input.
        let _scratch = charge(
            &self.quota,
            source
                .len()
                .checked_add(8192)
                .ok_or_else(|| invalid("scratch overflow"))?,
        )?; // Admit all decoded scratch before allocation.
        let mut values = Vec::new();
        values
            .try_reserve_exact(spec.elements)
            .map_err(|_| AnimationError::Budget("native math decoded input".into()))?; // Refusal preserves original custody.
        for bytes in source.chunks_exact(4) {
            values.push(f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        } // Decode same-host bytes without alignment assumptions.
        let mut numeric = BTreeMap::new();
        for (key, value) in parameters {
            numeric.insert(
                key.clone(),
                value.as_f64().ok_or_else(|| invalid("numeric parameter"))?,
            );
        } // Stage bounded parameters under admission.
        let parameters = parameters_from_wire(kernel, &numeric)?; // Reuse native schema and coefficient checks.
        let input = MathInput::from_host(&values, self.quota.clone())?; // Native MathInput admits its own immutable copy.
        self.active(request, current)?; // Recheck after decoding.
        let native_request = MathRequest::new(input, parameters, max_bytes, timeout_ms)?
            .cap_deadline(request.native_deadline())?;
        self.active(request, current)?; // Recheck immediately before bank admission.
        let native = self
            .native
            .submit(native_request, current.authorization_epoch)?; // Use the real finite CPU bank and Receipt.
        let id = native.id();
        self.entries.insert(
            id,
            Entry {
                native,
                request: Some(request.clone()),
                state: ComputeStatus::Pending,
                revision: 1,
            },
        ); // Acknowledgement never invents ready state; request aliases stay charged.
        Ok(MathBridgeHandle {
            issuer: Arc::clone(&self.issuer),
            id,
        }) // Return the original opaque registry identity.
    } // End exact typed submission.
    pub fn acknowledge_submission(
        &mut self,
        handle: &MathBridgeHandle,
        request_id: u64,
        outcome: CompletionState,
        current: ServiceAuthority,
    ) -> Result<()> {
        // Call only with the actual correlated Engine/helper ACK observed under the native owner's publication gate.
        self.owner(current)?;
        let entry = self.entry(handle)?;
        if entry.request.as_ref().map(|request| request.id) != Some(request_id) {
            return Err(invalid("submission ACK correlation"));
        } // Require exact queued-request correlation.
        let entry = self
            .entries
            .get_mut(&handle.id)
            .ok_or_else(|| invalid("retired handle"))?; // The native job already owns input/deadline custody.
        if outcome == CompletionState::Delivered {
            entry.request = None;
        } else {
            match &entry.state {
                ComputeStatus::Pending => {
                    self.native.cancel(&entry.native)?;
                    entry.state = ComputeStatus::Cancelling;
                    entry.revision += 1;
                }
                ComputeStatus::Ready(_) | ComputeStatus::Failed(_) => {
                    entry.state = ComputeStatus::Cancelled;
                    entry.revision += 1;
                }
                _ => {}
            }
        } // A real Delivered ACK transfers queued-request custody; failed publication retains cancellation/terminal cleanup ownership.
        Ok(()) // ACK is not physical-retirement proof.
    } // End explicit submission-to-job lifetime transfer.
    pub fn poll(&mut self, handle: &MathBridgeHandle, current: ServiceAuthority) -> Result<bool> {
        // Poll only after an existing completion wake; never wait.
        self.entry(handle)?; // Check original handle identity first.
        if current != self.authority {
            self.revoke();
        } // Stale coordinates revoke; they cannot reauthorize.
        let entry = self
            .entries
            .get_mut(&handle.id)
            .ok_or_else(|| invalid("retired handle"))?; // Borrow the original job entry.
        if entry
            .request
            .as_ref()
            .is_some_and(HostRequest::is_cancelled)
            || self.closed
        {
            // Only an unacknowledged submit retains its queued-request stop/deadline; published jobs own the original native MathRequest deadline independently.
            match &entry.state {
                ComputeStatus::Pending => {
                    self.native.cancel(&entry.native)?;
                    entry.state = ComputeStatus::Cancelling;
                    entry.revision += 1;
                }
                ComputeStatus::Ready(_) | ComputeStatus::Failed(_) => {
                    entry.state = ComputeStatus::Cancelled;
                    entry.revision += 1;
                }
                _ => {}
            } // Withhold cached payloads and request cooperative cancellation without claiming physical exit.
        } // Cancellation retains original custody.
        if matches!(
            &entry.state,
            ComputeStatus::Pending | ComputeStatus::Cancelling
        ) {
            // Poll only the original pending receipt.
            let state = self
                .native
                .poll(&entry.native, current.authorization_epoch)?; // NativeMath rechecks handle and epoch.
            if !matches!(&state, ComputeStatus::Pending | ComputeStatus::Cancelling) {
                entry.revision += 1;
            } // Revise only a native lifecycle observation.
            entry.state = state; // Cache the actual guarded output; never repoll a removed slot.
        } // Observation runs no checkpoint.
        Ok(!matches!(
            &entry.state,
            ComputeStatus::Pending | ComputeStatus::Cancelling
        )) // True reports a terminal native API observation; Failed/lost is not physical worker or helper retirement proof.
    } // End wake-driven native observation.
    pub fn close_handle(
        &mut self,
        handle: &MathBridgeHandle,
        current: ServiceAuthority,
    ) -> Result<()> {
        // Owner authorizes the real compute.close operation before this call.
        self.entry(handle)?;
        self.owner(current)?; // Possessing a wire ID alone never reaches this operation; cleanup does not depend on an obsolete submission deadline.
        let entry = self
            .entries
            .get_mut(&handle.id)
            .ok_or_else(|| invalid("retired handle"))?; // Retain the original job until terminal observation.
        match &entry.state {
            ComputeStatus::Pending => {
                self.native.cancel(&entry.native)?;
                entry.state = ComputeStatus::Cancelling;
                entry.revision += 1;
            }
            ComputeStatus::Ready(_) | ComputeStatus::Failed(_) => {
                entry.state = ComputeStatus::Cancelled;
                entry.revision += 1;
            }
            _ => {}
        } // Signal pending work once, or drop a genuinely completed cached output; cancellation is not physical retirement.
        Ok(()) // The owner must poll and publish actual status.
    } // End native opaque handle close request.
    pub fn forget_terminal(&mut self, handle: &MathBridgeHandle) -> Result<()> {
        // Allow nonacquiring native cleanup after revocation.
        if matches!(
            &self.entry(handle)?.state,
            ComputeStatus::Pending | ComputeStatus::Cancelling
        ) {
            return Err(invalid("native job still has a pending receipt"));
        } // No early release based on notification, closed pipe, or local cancellation alone.
        self.entries.remove(&handle.id);
        Ok(()) // Release registry custody after a terminal native API record; actual bank/client owners independently retain physical admissions.
    } // End native handle retirement.
    pub fn descriptor(
        &self,
        handle: &MathBridgeHandle,
        current: ServiceAuthority,
        limits: &EngineLimits,
    ) -> Result<ServiceValue> {
        // Admit native handle/status metadata.
        let entry = self.entry(handle)?;
        self.owner(current)?;
        if let Some(request) = &entry.request {
            self.active(request, current)?;
        } // An unpublished descriptor remains tied to the original live submit request.
        let _scratch = charge(&self.quota, 8192)?; // Admit temporary JSON before copying.
        let status = match &entry.state {
            ComputeStatus::Pending | ComputeStatus::Cancelling => json!({"state":"preparing"}),
            ComputeStatus::Ready(_) => json!({"state":"ready"}),
            ComputeStatus::Cancelled => json!({"state":"closed"}),
            ComputeStatus::Failed(message) => {
                json!({"state":"error","error":{"code":"native_math_failed","message":message}})
            }
        }; // Report only actual native status.
        let value = json!({"ok":true,"value":{"id":handle.id.to_string(),"kind":"compute","revision":entry.revision,"status":status}}); // IDs are projections, not credentials.
        let copied = ServiceValue::copy_from_host(
            &value,
            &[],
            &BTreeMap::new(),
            limits,
            self.quota.clone(),
        )?;
        if let Some(request) = &entry.request {
            self.active(request, current)?;
        }
        Ok(copied) // Recheck unacknowledged input after copying.
    } // End admitted handle/status descriptor projection.
    pub fn copy_result(
        &self,
        handle: &MathBridgeHandle,
        request: &HostRequest,
        current: ServiceAuthority,
        kind: TypedArrayKind,
        limits: &EngineLimits,
    ) -> Result<ServiceValue> {
        // The native owner chooses F32 samples or their exact U8 byte representation; no new guest option is added.
        let entry = self.entry(handle)?;
        self.active(request, current)?;
        if entry.request.is_some() {
            return Err(invalid("submission has no delivery ACK"));
        } // Published native job lifetime is independent; the new retrieval request supplies its own original deadline/stop alias.
        let _metadata_scratch = charge(&self.quota, 8192)?;
        let expected_id = handle.id.to_string(); // Admit temporary ID metadata first.
        if request.method != "compute.result"
            || !request
                .payload
                .metadata()
                .as_object()
                .is_some_and(|fields| {
                    fields.len() == 2
                        && fields.get("id").and_then(Value::as_str) == Some(expected_id.as_str())
                        && fields.get("kind").and_then(Value::as_str) == Some("compute")
                })
            || !request.payload.arrays().is_empty()
            || !matches!(kind, TypedArrayKind::F32 | TypedArrayKind::U8)
        {
            return Err(invalid("result request or representation"));
        } // Require the exact retrieval schema and genuine handle.
        let output: Option<&Arc<MathOutput>> = match &entry.state {
            ComputeStatus::Ready(output) => Some(output),
            ComputeStatus::Pending | ComputeStatus::Cancelling => {
                return Err(invalid(
                    "result is pending; retain request until a completion wake",
                ))
            }
            _ => None,
        }; // Never return a permanent pending error to the facade's one admitted cached result request.
        let bytes = output.map_or(0, |output| output.values().len() * 4);
        let _scratch = charge(&self.quota, bytes + 8192)?; // Admit byte encoding before allocation.
        let mut planes = BTreeMap::new();
        let mut arrays = Vec::new(); // Keep scratch charged through the escaping copy.
        let metadata = match &entry.state {
            // Return structured asynchronous Results.
            ComputeStatus::Ready(output) => {
                let mut plane = Vec::new();
                plane
                    .try_reserve_exact(bytes)
                    .map_err(|_| AnimationError::Budget("native math output encoding".into()))?;
                for value in output.values() {
                    plane.extend_from_slice(&value.to_ne_bytes());
                }
                planes.insert("b0".into(), plane);
                arrays.push(ArraySpec {
                    name: "b0".into(),
                    kind,
                    elements: if kind == TypedArrayKind::F32 {
                        output.values().len()
                    } else {
                        bytes
                    },
                });
                json!({"ok":true,"value":{"$ilium_binary":"b0"}})
            } // Copy actual guarded output bits.
            ComputeStatus::Cancelled => {
                json!({"ok":false,"error":{"code":"cancelled","message":"Native math handle was closed before result publication."}})
            } // Logical closure never asserts physical cancellation.
            ComputeStatus::Failed(message) => {
                json!({"ok":false,"error":{"code":"native_math_failed","message":message}})
            } // Preserve the actual native error.
            _ => return Err(invalid("result state changed")), // Defensive exhaustiveness only.
        }; // Keep all source guards live through copying.
        let copied =
            ServiceValue::copy_from_host(&metadata, &arrays, &planes, limits, self.quota.clone())?; // Escaping bytes get their own original-root admission.
        self.active(request, current)?;
        Ok(copied) // Recheck the retrieval deadline/stop after copying; final current-authorization publication remains the real owner's closure.
    } // End typed retained-output projection.
    /// Only the owning actor's genuine finite completion wake may call this.
    /// Revocation withholds payloads; this observes retained ORIGINAL receipts
    /// without authorizing an effect, running JS, seeding or waiting.
    pub fn collect_retirement_on_wake(&mut self) -> Result<()> {
        if !self.closed {
            return Err(invalid("retirement collection before revocation"));
        }
        let handles: Vec<_> = self
            .entries
            .keys()
            .map(|id| MathBridgeHandle {
                issuer: Arc::clone(&self.issuer),
                id: *id,
            })
            .collect();
        for handle in handles {
            self.poll(&handle, self.authority)?;
        }
        if self.native.is_drained() {
            self.entries.clear();
        }
        Ok(())
    }
    pub fn is_drained(&self) -> bool {
        self.closed && self.native.is_drained() && self.entries.is_empty()
    }

    pub fn revoke(&mut self) {
        // Native activation revocation is independent of helper retirement.
        if self.closed {
            return;
        }
        self.closed = true;
        self.native.close(); // Signal original native receipts once.
        for entry in self.entries.values_mut() {
            match &entry.state {
                ComputeStatus::Pending => {
                    entry.state = ComputeStatus::Cancelling;
                    entry.revision += 1;
                }
                ComputeStatus::Ready(_) | ComputeStatus::Failed(_) => {
                    entry.state = ComputeStatus::Cancelled;
                    entry.revision += 1;
                }
                _ => {}
            }
        } // Withhold cached output immediately while retaining unsettled physical work for poll/cleanup.
    } // End independent native activation revocation.
} // End bridge API; actual host registry routing and baseline CPU/current-publication authorization remain external integration contracts.
impl Drop for NativeMathBridge {
    fn drop(&mut self) {
        self.revoke();
    }
} // The original finite bank still owns actual queued/running jobs and admissions after receipt abandonment.

#[cfg(test)]
#[path = "native_math_boundary_contract.rs"]
mod boundary_contracts;
