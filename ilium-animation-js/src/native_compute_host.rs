//! Worker-owned routing for the actual finite native math bridge.
//! Wire IDs only select retained original handles; private runtime gates authorize effects.
use crate::{
    engine::{CompletionState, EngineLimits, HostRequest, ServiceValue, TypedArrayKind},
    error::{AnimationError, Result},
    native_math_bridge::{MathBridgeHandle, NativeMathBridge},
    runtime::PackageInstance,
};
use ilium_ambient::resources::AmbientResources;
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_HANDLES: usize = 32;
const MAX_RESULTS: usize = 32;
struct OwnedHandle {
    original: MathBridgeHandle,
    terminal: bool,
}
/// Construct with the original finite client already configured to wake the
/// existing worker event channel. This type never creates an execution bank.
pub struct NativeComputeHost {
    bridge: Option<NativeMathBridge>,
    closed: bool,
    handles: BTreeMap<String, OwnedHandle>,
    results: BTreeMap<String, HostRequest>,
    closes: BTreeMap<String, HostRequest>,
    resources: AmbientResources,
    quota: QuotaGroup,
    limits: EngineLimits,
    _metadata: StorageAdmission,
}
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("native compute host: {message}"))
}
impl NativeComputeHost {
    pub fn new(
        resources: AmbientResources,
        quota: QuotaGroup,
        limits: EngineLimits,
    ) -> Result<Self> {
        // NativeMath additionally verifies original-root identity at first issue.
        let metadata = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|error| AnimationError::Budget(format!("compute registry: {error:?}")))?;
        Ok(Self {
            bridge: None,
            closed: false,
            handles: BTreeMap::new(),
            results: BTreeMap::new(),
            closes: BTreeMap::new(),
            resources,
            quota,
            limits,
            _metadata: metadata,
        })
    }
    fn handle_id(request: &HostRequest) -> Result<&str> {
        let fields = request
            .payload
            .metadata()
            .as_object()
            .ok_or_else(|| invalid("handle record"))?;
        if fields.len() != 2
            || fields.get("kind").and_then(Value::as_str) != Some("compute")
            || !request.payload.arrays().is_empty()
        {
            return Err(invalid("handle schema"));
        }
        fields
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| {
                !id.is_empty() && id.len() <= 20 && id.bytes().all(|byte| byte.is_ascii_digit())
            })
            .ok_or_else(|| invalid("handle ID"))
    }
    /// Return unrelated requests intact for their actual service adapters.
    /// Recognized failures propagate; they never manufacture a successful RPC.
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if self.closed
            && matches!(
                request.method.as_str(),
                "compute.submit" | "compute.result" | "compute.close"
            )
        {
            return Err(invalid("host admission closed"));
        }
        match request.method.as_str() {
            "compute.submit" => {
                if self.handles.len() >= MAX_HANDLES {
                    return Err(invalid("handle limit"));
                }
                if self.bridge.is_none() {
                    let resources = self.resources.clone();
                    let quota = self.quota.clone();
                    let digest = instance.package().digest().to_owned();
                    self.bridge = Some(instance.with_baseline_math(&request, |authority| {
                        NativeMathBridge::new(resources, quota, &digest, authority)
                    })?);
                }
                let bridge = self
                    .bridge
                    .as_mut()
                    .ok_or_else(|| invalid("bridge unavailable"))?;
                let handle = instance
                    .with_baseline_math(&request, |authority| bridge.submit(&request, authority))?;
                // Retain the genuine handle BEFORE any fallible descriptor/copy/ACK.
                // An unpublished job stays owned and is cancelled on failure.
                let descriptor = instance.with_baseline_math(&request, |authority| {
                    bridge.descriptor(&handle, authority, &self.limits)
                });
                let descriptor = match descriptor {
                    Ok(value) => value,
                    Err(error) => {
                        bridge.revoke();
                        return Err(error);
                    }
                };
                let id = descriptor
                    .metadata()
                    .get("value")
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("native descriptor ID"))?
                    .to_owned();
                if self.handles.contains_key(&id) {
                    bridge.revoke();
                    return Err(invalid("duplicate native ID"));
                }
                self.handles.insert(
                    id,
                    OwnedHandle {
                        original: handle.clone(),
                        terminal: false,
                    },
                );
                let delivered = instance.complete_baseline_math(&request, descriptor);
                let outcome = delivered
                    .as_ref()
                    .copied()
                    .unwrap_or(CompletionState::Cancelled);
                let ack = instance.with_native_math_authority(|authority| {
                    bridge.acknowledge_submission(&handle, request.id, outcome, authority)
                });
                // Even if channel revocation prevents ACK processing, native jobs
                // remain retained; fail closed and signal independent cancellation.
                if ack.is_err() {
                    bridge.revoke();
                }
                delivered?;
                ack?;
                Ok(None)
            }
            "compute.result" => {
                let id = Self::handle_id(&request)?.to_owned();
                if !self.handles.contains_key(&id) {
                    return Err(invalid("unknown original handle"));
                }
                if self.results.contains_key(&id) || self.results.len() >= MAX_RESULTS {
                    return Err(invalid("result request limit"));
                }
                instance.with_baseline_math(&request, |_| Ok(()))?;
                self.results.insert(id.clone(), request);
                self.publish_ready(instance, &id)?;
                Ok(None)
            }
            "compute.close" => {
                let id = Self::handle_id(&request)?.to_owned();
                if self.closes.contains_key(&id) || self.closes.len() >= MAX_HANDLES {
                    return Err(invalid("close request limit"));
                }
                let owned = self
                    .handles
                    .get_mut(&id)
                    .ok_or_else(|| invalid("unknown original handle"))?;
                let bridge = self
                    .bridge
                    .as_mut()
                    .ok_or_else(|| invalid("bridge unavailable"))?;
                let descriptor = instance.with_baseline_math(&request, |authority| {
                    bridge.close_handle(&owned.original, authority)?;
                    bridge.descriptor(&owned.original, authority, &self.limits)
                })?;
                owned.terminal = descriptor
                    .metadata()
                    .get("value")
                    .and_then(|value| value.get("status"))
                    .and_then(|status| status.get("state"))
                    .and_then(Value::as_str)
                    .is_some_and(|state| state == "closed");
                // Retain the actual close request through cancellation and wake.
                // A preparing acknowledgement would release the SDK close guard
                // before the native receipt has actually become terminal.
                self.closes.insert(id.clone(), request);
                self.publish_ready(instance, &id)?;
                self.publish_closed(instance, &id)?;
                Ok(None)
            }
            _ => Ok(Some(request)),
        }
    }
    fn publish_ready(&mut self, instance: &mut PackageInstance, id: &str) -> Result<()> {
        let owned = self
            .handles
            .get(id)
            .ok_or_else(|| invalid("unknown original handle"))?;
        let Some(request) = self.results.get(id) else {
            return Ok(());
        };
        if request.is_cancelled() {
            self.results.remove(id);
            return Ok(());
        }
        if !owned.terminal {
            return Ok(());
        }
        let bridge = self
            .bridge
            .as_ref()
            .ok_or_else(|| invalid("bridge unavailable"))?;
        let value = instance.with_baseline_math(request, |authority| {
            bridge.copy_result(
                &owned.original,
                request,
                authority,
                TypedArrayKind::F32,
                &self.limits,
            )
        })?;
        // Take exactly once BEFORE publication. A lost copy/ACK cannot become
        // a duplicate resolve or recreate the facade's admitted result request.
        let request = self
            .results
            .remove(id)
            .ok_or_else(|| invalid("result custody lost"))?;
        instance.complete_baseline_math(&request, value)?;
        Ok(())
    }
    fn publish_closed(&mut self, instance: &mut PackageInstance, id: &str) -> Result<()> {
        let owned = self
            .handles
            .get(id)
            .ok_or_else(|| invalid("unknown original handle"))?;
        if !owned.terminal {
            return Ok(());
        }
        let Some(request) = self.closes.get(id) else {
            return Ok(());
        };
        let bridge = self
            .bridge
            .as_mut()
            .ok_or_else(|| invalid("bridge unavailable"))?;
        if !request.is_cancelled() {
            let descriptor = instance.with_baseline_math(request, |authority| {
                bridge.descriptor(&owned.original, authority, &self.limits)
            })?;
            let request = self
                .closes
                .remove(id)
                .ok_or_else(|| invalid("close custody lost"))?;
            // Failures retain terminal handle custody until owner revocation.
            instance.complete_baseline_math(&request, descriptor)?;
        } else {
            self.closes.remove(id);
        }
        // Actual native terminal state, not close intent, permits forgetting.
        bridge.forget_terminal(&owned.original)?;
        self.handles.remove(id);
        Ok(())
    }
    /// Call on the original finite completion wake, never from a checking loop.
    /// New result requests may use already cached terminal output without polling.
    pub fn on_completion_wake(&mut self, instance: &mut PackageInstance) -> Result<()> {
        let Some(bridge) = self.bridge.as_mut() else {
            return Ok(());
        };
        let observation = instance.with_native_math_authority(|authority| {
            for owned in self.handles.values_mut().filter(|owned| !owned.terminal) {
                owned.terminal = bridge.poll(&owned.original, authority)?;
            }
            Ok(())
        });
        if let Err(error) = observation {
            bridge.revoke();
            return Err(error);
        }
        // Bounded key staging is covered by this registry's original admission.
        let ids: Vec<String> = self.results.keys().cloned().collect();
        for id in ids {
            self.publish_ready(instance, &id)?;
        }
        let closes: Vec<String> = self.closes.keys().cloned().collect();
        for id in closes {
            self.publish_closed(instance, &id)?;
        }
        Ok(())
    }
    /// Authenticated synchronous status projection, with its own escaping charge.
    /// The caller must use the inert protected seed-copy boundary, not a JS hook.
    pub fn snapshots(&self, instance: &mut PackageInstance) -> Result<ServiceValue> {
        instance.with_native_math_authority(|authority| {
            let mut values = Vec::with_capacity(self.handles.len());
            if let Some(bridge) = &self.bridge {
                for owned in self.handles.values() {
                    let descriptor = bridge.descriptor(&owned.original, authority, &self.limits)?;
                    let value = descriptor
                        .metadata()
                        .get("value")
                        .ok_or_else(|| invalid("descriptor value"))?;
                    values.push(value.clone());
                }
            }
            ServiceValue::copy_from_host(
                &Value::Array(values),
                &[],
                &BTreeMap::new(),
                &self.limits,
                self.quota.clone(),
            )
        })
    }
    /// Observe retirement only from the original completion wake. No current
    /// helper/channel is required to dispose of cancelled original receipts.
    pub fn collect_retirement_on_wake(&mut self) -> Result<()> {
        if !self.closed {
            return Err(invalid("retirement before revocation"));
        }
        if let Some(bridge) = &mut self.bridge {
            bridge.collect_retirement_on_wake()?;
            if bridge.is_drained() {
                self.handles.clear();
            }
        }
        Ok(())
    }
    pub fn is_drained(&self) -> bool {
        self.closed
            && self.results.is_empty()
            && self.closes.is_empty()
            && self
                .bridge
                .as_ref()
                .is_none_or(NativeMathBridge::is_drained)
    }
    pub fn revoke(&mut self) {
        self.closed = true;
        if let Some(bridge) = &mut self.bridge {
            bridge.revoke();
        }
        for request in self.results.values() {
            request.stop_token().stop();
        }
        self.results.clear();
        for request in self.closes.values() {
            request.stop_token().stop();
        }
        self.closes.clear();
        // Original receipt custody remains in the bridge and finite bank.
    }
}
impl Drop for NativeComputeHost {
    fn drop(&mut self) {
        self.revoke();
    }
}
