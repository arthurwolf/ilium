//! One explicit, bounded live receipt subscription on the existing scene actor.
//! Receipts enter only after the terminal writer's original broker proof and
//! saved-history settlement. No timer, worker thread or service bank is opened.
use crate::{
    engine::{
        ArraySpec, CompletionState, EngineLimits, HostRequest, ServiceAuthority, ServiceValue,
        TypedArrayKind,
    },
    error::{AnimationError, Result},
    native_worlds::WorldPresentationReceipt,
    runtime::{HelperRetirementEvidence, PackageInstance},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

const MAX_RECEIPTS: usize = 16;
const MAX_HANDLES: usize = 64;
fn invalid(reason: &str) -> AnimationError {
    AnimationError::Runtime(format!("native presentation: {reason}"))
}
struct QueuedReceipt {
    receipt: WorldPresentationReceipt,
    _admission: StorageAdmission,
}
struct UncertainPresentationCompletion {
    request: HostRequest,
    _result: ServiceValue,
}
struct Subscription {
    authority: ServiceAuthority,
    revision: u64,
    pending_next: Option<HostRequest>,
    pending_close: Option<HostRequest>,
    receipts: VecDeque<QueuedReceipt>,
    closed: bool,
}
impl Subscription {
    fn descriptor(&self, id: &str) -> Value {
        json!({"id":id,"kind":"presentation","revision":self.revision,
            "status":{"state":if self.closed {"closed"} else {"ready"}}})
    }
}
pub struct NativePresentationHost {
    current: BTreeMap<String, Subscription>,
    closed_snapshots: BTreeMap<String, Value>,
    uncertain_completion: Option<UncertainPresentationCompletion>,
    helper_retirement: HelperRetirementEvidence,
    next_id: u64,
    closed: bool,
    quota: QuotaGroup,
    limits: EngineLimits,
    _metadata: StorageAdmission,
}
impl NativePresentationHost {
    /// Construct only on an actual `presentation.subscribe` request. The
    /// existing scene actor and original quota root pay for the entire queue.
    pub fn new(
        instance: &PackageInstance,
        quota: QuotaGroup,
        limits: EngineLimits,
    ) -> Result<Self> {
        if !instance.shares_root(&quota) {
            return Err(AnimationError::PermissionDenied(
                "presentation helper owner uses a foreign quota root".into(),
            ));
        }
        let metadata = quota
            .reserve_external_storage(128 * 1024)
            .map_err(|error| AnimationError::Budget(format!("presentation registry: {error:?}")))?;
        Ok(Self {
            current: BTreeMap::new(),
            closed_snapshots: BTreeMap::new(),
            uncertain_completion: None,
            helper_retirement: instance.helper_retirement_evidence(),
            next_id: 1,
            closed: false,
            quota,
            limits,
            _metadata: metadata,
        })
    }
    fn check_helper_owner(&self, instance: &PackageInstance) -> Result<()> {
        let candidate = instance.helper_retirement_evidence();
        if !self.helper_retirement.same_owner(&candidate) {
            return Err(AnimationError::PermissionDenied(
                "foreign presentation helper owner".into(),
            ));
        }
        Ok(())
    }
    fn payload(request: &HostRequest) -> Result<&Value> {
        if !request.payload.arrays().is_empty() || !request.payload.planes().is_empty() {
            return Err(invalid("receipt request must be JSON only"));
        }
        Ok(request.payload.metadata())
    }
    fn handle_id(request: &HostRequest) -> Result<&str> {
        let value = Self::payload(request)?
            .as_object()
            .ok_or_else(|| invalid("handle object"))?;
        if value.len() != 2 || value.get("kind").and_then(Value::as_str) != Some("presentation") {
            return Err(invalid("handle projection"));
        }
        value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| {
                id.len() <= 128
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
            .ok_or_else(|| invalid("handle ID"))
    }
    fn published_id(&self, id: &str, authority: ServiceAuthority) -> bool {
        let prefix = format!(
            "presentation-{}-{}-{}-",
            authority.instance_id, authority.plan_generation, authority.authorization_epoch
        );
        id.strip_prefix(&prefix)
            .and_then(|part| part.parse::<u64>().ok())
            .is_some_and(|sequence| {
                sequence > 0 && sequence < self.next_id && id == format!("{prefix}{sequence}")
            })
    }
    fn complete(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        value: Value,
    ) -> Result<CompletionState> {
        if self.uncertain_completion.is_some() {
            return Err(invalid("uncertain presentation completion still retained"));
        }
        self.check_helper_owner(instance)?;
        instance.check_native_presentation_request(request)?;
        let output = ServiceValue::copy_from_host(
            &value,
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )?;
        self.complete_output(instance, request, output)
    }
    fn complete_output(
        &mut self,
        instance: &mut PackageInstance,
        request: &HostRequest,
        output: ServiceValue,
    ) -> Result<CompletionState> {
        if self.uncertain_completion.is_some() {
            return Err(invalid("uncertain presentation completion still retained"));
        }
        self.check_helper_owner(instance)?;
        instance.check_native_presentation_request(request)?;
        let retained = output.clone();
        match instance.complete_native_presentation(request, output) {
            Ok(state) => Ok(state),
            Err(error) => {
                request.stop_token().stop();
                self.uncertain_completion = Some(UncertainPresentationCompletion {
                    request: request.clone(),
                    _result: retained,
                });
                self.closed = true;
                Err(error)
            }
        }
    }
    fn receipt_output(&self, receipt: &WorldPresentationReceipt) -> Result<ServiceValue> {
        let count = receipt.owners.len();
        if !receipt.admission.shares_root(&self.quota)
            || receipt.owner_namespace == 0
            || count > 8192
            || receipt.frame_id.len() > 128
            || receipt.source_identity.len() > 256
        {
            return Err(invalid("receipt owner namespace or original admission"));
        }
        // The queued receipt retains its own source allocation. These temporary
        // planes and constant-sized metadata need a separate debit before copy.
        let bytes = count
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(32 * 1024))
            .ok_or_else(|| invalid("packed receipt scratch overflow"))?;
        let _scratch = self
            .quota
            .reserve_external_storage(bytes)
            .map_err(|failure| {
                AnimationError::Budget(format!("packed receipt scratch: {failure:?}"))
            })?;
        let mut ids = Vec::with_capacity(count * 4);
        let mut dots = Vec::with_capacity(count * 4);
        for (index, owner) in receipt.owners.iter().enumerate() {
            ids.extend_from_slice(&((index + 1) as u32).to_ne_bytes());
            dots.extend_from_slice(&owner.dots.to_ne_bytes());
        }
        let arrays = [
            ArraySpec {
                name: "b0".into(),
                kind: TypedArrayKind::U32,
                elements: count,
            },
            ArraySpec {
                name: "b1".into(),
                kind: TypedArrayKind::U32,
                elements: count,
            },
        ];
        let planes = BTreeMap::from([("b0".into(), ids), ("b1".into(), dots)]);
        let metadata = json!({"ok":true,"value":{
            "frame_id":receipt.frame_id,"source_identity":receipt.source_identity,
            "composition_revision":receipt.composition_revision,"emitted_dots":receipt.emitted_dots,
            "owners":{"token_prefix":format!("source-owner-{}-",receipt.owner_namespace),
                "ids":{"$ilium_binary":"b0"},"dots":{"$ilium_binary":"b1"}}
        }});
        ServiceValue::copy_from_host(
            &metadata,
            &arrays,
            &planes,
            &self.limits,
            self.quota.clone(),
        )
    }
    pub fn dispatch(
        &mut self,
        instance: &mut PackageInstance,
        request: HostRequest,
    ) -> Result<Option<HostRequest>> {
        if !matches!(
            request.method.as_str(),
            "presentation.subscribe" | "presentation.next" | "presentation.close"
        ) {
            return Ok(Some(request));
        }
        if self.closed {
            return Err(invalid("receipt owner retiring"));
        }
        self.check_helper_owner(instance)?;
        let authority = instance.check_native_presentation_request(&request)?;
        match request.method.as_str() {
            "presentation.subscribe" => {
                if Self::payload(&request)? != &json!({}) {
                    return Err(invalid("subscribe payload"));
                }
                if !self.current.is_empty()
                    || self.current.len() + self.closed_snapshots.len() >= MAX_HANDLES
                    || self.next_id == u64::MAX
                {
                    return Err(AnimationError::Budget(
                        "presentation subscription bound".into(),
                    ));
                }
                let id = format!(
                    "presentation-{}-{}-{}-{}",
                    authority.instance_id,
                    authority.plan_generation,
                    authority.authorization_epoch,
                    self.next_id
                );
                let entry = Subscription {
                    authority,
                    revision: 1,
                    pending_next: None,
                    pending_close: None,
                    receipts: VecDeque::new(),
                    closed: false,
                };
                let descriptor = entry.descriptor(&id);
                instance.with_native_presentation_registry(&request, |_| {
                    self.current.insert(id.clone(), entry);
                    Ok(())
                })?;
                match self.complete(instance, &request, json!({"ok":true,"value":descriptor})) {
                    Ok(CompletionState::Delivered) => self.next_id += 1,
                    Ok(_) => {
                        self.current.remove(&id);
                    }
                    Err(error) => {
                        self.closed = true;
                        return Err(error);
                    }
                }
            }
            "presentation.next" => {
                let id = Self::handle_id(&request)?.to_owned();
                if !self.current.contains_key(&id) {
                    if !self.published_id(&id, authority) {
                        return Err(invalid("unknown subscription"));
                    }
                    let _ = self.complete(instance, &request, json!({"ok":true,"value":null}))?;
                    return Ok(None);
                }
                instance.with_native_presentation_registry(&request, |_| {
                    let entry = self
                        .current
                        .get_mut(&id)
                        .ok_or_else(|| invalid("subscription lost"))?;
                    if entry.authority != authority
                        || entry.pending_next.is_some()
                        || entry.pending_close.is_some()
                    {
                        return Err(invalid("stale or overlapping receipt next"));
                    }
                    entry.pending_next = Some(request.clone());
                    Ok(())
                })?;
                self.deliver(instance, &id)?;
            }
            "presentation.close" => {
                let id = Self::handle_id(&request)?.to_owned();
                if !self.current.contains_key(&id) {
                    if !self.published_id(&id, authority) {
                        return Err(invalid("unknown subscription"));
                    }
                    let descriptor=self.closed_snapshots.get(&id).cloned()
                        .unwrap_or_else(|| json!({"id":id,"kind":"presentation","revision":2,"status":{"state":"closed"}}));
                    let state =
                        self.complete(instance, &request, json!({"ok":true,"value":descriptor}))?;
                    if state == CompletionState::Delivered {
                        self.closed_snapshots.remove(&id);
                    }
                    return Ok(None);
                }
                instance.with_native_presentation_registry(&request, |_| {
                    let entry = self
                        .current
                        .get_mut(&id)
                        .ok_or_else(|| invalid("subscription lost"))?;
                    if entry.authority != authority || entry.pending_close.is_some() {
                        return Err(invalid("overlapping receipt close"));
                    }
                    entry.closed = true;
                    entry.revision = entry
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| invalid("receipt revision"))?;
                    entry.pending_close = Some(request.clone());
                    Ok(())
                })?;
                self.deliver(instance, &id)?;
            }
            _ => return Err(invalid("method inventory")),
        }
        Ok(None)
    }
    fn deliver(&mut self, instance: &mut PackageInstance, id: &str) -> Result<()> {
        let next = self
            .current
            .get(id)
            .and_then(|entry| entry.pending_next.clone());
        if let Some(request) = next {
            let output = {
                let entry = self
                    .current
                    .get(id)
                    .ok_or_else(|| invalid("subscription lost"))?;
                if entry.closed {
                    Some(ServiceValue::copy_from_host(
                        &json!({"ok":true,"value":null}),
                        &[],
                        &BTreeMap::new(),
                        &self.limits,
                        self.quota.clone(),
                    )?)
                } else {
                    entry
                        .receipts
                        .front()
                        .map(|queued| self.receipt_output(&queued.receipt))
                        .transpose()?
                }
            };
            if let Some(output) = output {
                let state = self.complete_output(instance, &request, output)?;
                if state != CompletionState::Delivered {
                    self.closed = true;
                    return Err(invalid("receipt next ACK uncertain"));
                }
                let entry = self
                    .current
                    .get_mut(id)
                    .ok_or_else(|| invalid("subscription lost"))?;
                if !entry.closed {
                    entry.receipts.pop_front();
                }
                entry.pending_next = None;
            }
        }
        let close = self
            .current
            .get(id)
            .and_then(|entry| entry.pending_close.clone());
        if let Some(request) = close {
            let entry = self
                .current
                .get(id)
                .ok_or_else(|| invalid("subscription lost"))?;
            if entry.pending_next.is_some() {
                return Ok(());
            }
            let descriptor = entry.descriptor(id);
            let state = self.complete(
                instance,
                &request,
                json!({"ok":true,"value":descriptor.clone()}),
            )?;
            if state != CompletionState::Delivered {
                self.closed = true;
                return Err(invalid("receipt close ACK uncertain"));
            }
            self.closed_snapshots.insert(id.to_owned(), descriptor);
            self.current.remove(id);
        }
        Ok(())
    }
    pub fn publish(
        &mut self,
        instance: &mut PackageInstance,
        receipt: WorldPresentationReceipt,
    ) -> Result<()> {
        if self.closed || self.current.is_empty() {
            return Ok(());
        }
        self.check_helper_owner(instance)?;
        if !receipt.admission.shares_root(&self.quota) {
            return Err(invalid("receipt uses foreign allocation admission"));
        }
        let id = self
            .current
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| invalid("subscription lost"))?;
        let entry = self
            .current
            .get_mut(&id)
            .ok_or_else(|| invalid("subscription lost"))?;
        if entry.closed {
            return Ok(());
        }
        if entry.receipts.len() >= MAX_RECEIPTS {
            self.closed = true;
            return Err(AnimationError::Budget(
                "presentation receipt queue full".into(),
            ));
        }
        // The settled source evidence is already released. Charge the queued
        // informational copy before it can outlive that original proof.
        let bytes = receipt
            .owners
            .len()
            .checked_mul(128)
            .and_then(|owners| owners.checked_add(receipt.frame_id.len()))
            .and_then(|size| size.checked_add(receipt.source_identity.len()))
            .and_then(|size| size.checked_add(1024))
            .ok_or_else(|| invalid("receipt queue size overflow"))?;
        let admission = self
            .quota
            .reserve_external_storage(bytes)
            .map_err(|error| {
                AnimationError::Budget(format!("presentation receipt queue: {error:?}"))
            })?;
        entry.receipts.push_back(QueuedReceipt {
            receipt,
            _admission: admission,
        });
        self.deliver(instance, &id)
    }
    pub fn snapshots(&self, instance: &PackageInstance) -> Result<ServiceValue> {
        self.check_helper_owner(instance)?;
        instance.native_presentation_authority()?;
        let values: Vec<Value> = self
            .current
            .iter()
            .map(|(id, entry)| entry.descriptor(id))
            .chain(self.closed_snapshots.values().cloned())
            .collect();
        ServiceValue::copy_from_host(
            &Value::Array(values),
            &[],
            &BTreeMap::new(),
            &self.limits,
            self.quota.clone(),
        )
    }
    pub fn acknowledge_seeded_snapshots(&mut self) {
        self.closed_snapshots.clear();
    }
    pub fn revoke(&mut self) {
        self.closed = true;
        if let Some(pending) = &self.uncertain_completion {
            pending.request.stop_token().stop();
        }
        for entry in self.current.values() {
            if let Some(request) = &entry.pending_next {
                request.stop_token().stop();
            }
            if let Some(request) = &entry.pending_close {
                request.stop_token().stop();
            }
        }
    }
    pub fn finite_work_drained(&self) -> bool {
        self.closed
    }
    pub fn on_helper_retired(&mut self) -> Result<()> {
        if !self.closed {
            return Err(invalid("presentation owner is not revoked"));
        }
        if !self.helper_retirement.is_physically_retired() {
            return Err(invalid(
                "original presentation helper physical retirement is unproven",
            ));
        }
        self.current.clear();
        self.closed_snapshots.clear();
        self.uncertain_completion.take();
        Ok(())
    }
    pub fn is_drained(&self) -> bool {
        self.closed
            && self.current.is_empty()
            && self.closed_snapshots.is_empty()
            && self.uncertain_completion.is_none()
    }
}

#[cfg(test)]
mod actual_presentation_custody_tests {
    use super::*;
    use crate::{
        engine::CreateState,
        helper::{isolation_qualification as fixture, HelperLimits},
        manifest::AnimationMode,
        native_worlds::{WorldPresentationOwner, WorldPresentationReceiptData},
        permissions::Ceiling,
        runtime::InstancePreparation,
        trust::TrustVerifier,
    };
    use ilium_execution::QuotaLimits;
    use std::path::Path;
    fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 16 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
            worker_threads: 40,
            worker_bytes: 2 * 1024 * 1024 * 1024,
        })
    }

    fn package_instance_for_source(quota: &QuotaGroup, source: &str) -> PackageInstance {
        let bytes = fixture::archive(source);
        let executable =
            std::env::var("ILIUM_ANIMATION_HELPER").expect("actual helper absolute path required");
        let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
        let settings = json!({});
        let environment = json!({});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &bytes,
            verifier: &verifier,
            helper_executable: Path::new(&executable),
            trusted_bootstrap: crate::TRUSTED_BOOTSTRAP,
            settings: &settings,
            mode: AnimationMode::Live,
            environment: &environment,
            host_policy: Ceiling {
                permissions: vec![],
            },
            instance_id: 73,
            limits: HelperLimits::default(),
            quota: quota.clone(),
        })
        .unwrap();
        let (mut instance, review) = verified.prepare_without_rights().unwrap();
        let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
        let resolution = instance.finish_resolution(pending).unwrap();
        assert_eq!(resolution.accepted_creation(), Some(CreateState::Pending));
        instance
    }

    fn subscription_source() -> &'static str {
        r#"
export function plan(){return {format:'gray32',fps:30,inputs:{}};}
export async function create(){
    const opened=await __ilium_dispatch('presentation.subscribe',{});
    if(!opened.ok)throw Error('fixture_subscription_open_failed');
    const handle={id:opened.value.id,kind:'presentation'};
    const received=await __ilium_dispatch('presentation.next',handle);
    if(!received.ok)throw Error('fixture_subscription_next_failed');
    return {render(){},dispose(){}};
}
"#
    }

    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_presentation_ack_loss_does_not_drain_original_custody_on_revoke() {
        let quota = quota();
        let mut instance = package_instance_for_source(&quota, subscription_source());
        let mut host =
            NativePresentationHost::new(&instance, quota.clone(), EngineLimits::default()).unwrap();
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let subscribe = requests.remove(0);
        assert_eq!(subscribe.method, "presentation.subscribe");
        assert!(host.dispatch(&mut instance, subscribe).unwrap().is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        let next = requests.remove(0);
        assert_eq!(next.method, "presentation.next");
        assert!(host.dispatch(&mut instance, next).unwrap().is_none());

        // Synthetic informational receipt tests transport custody only. Actual
        // source proof/history settlement is qualified at the compositor seam.
        let receipt_admission = quota.reserve_external_storage(4096).unwrap();
        instance
            .arm_complete_service_ack_failure_for_test()
            .unwrap();
        let error = host
            .publish(
                &mut instance,
                WorldPresentationReceipt(std::sync::Arc::new(WorldPresentationReceiptData {
                    frame_id: "synthetic_transport_frame".into(),
                    source_identity: "f".repeat(64),
                    composition_revision: 9,
                    emitted_dots: 1,
                    owners: vec![WorldPresentationOwner {
                        token: "source-owner-1-1".into(),
                        dots: 1,
                    }],
                    owner_namespace: 1,
                    admission: receipt_admission,
                })),
            )
            .expect_err("actual post-write acknowledgement loss is required");
        let message = error.to_string();
        assert!(
            message
                .contains("test interrupted CompleteService acknowledgement after packet release"),
            "{message}"
        );
        assert!(
            message.contains("test interrupted physical retirement before child shutdown"),
            "{message}"
        );
        let transport = instance.helper_transport_test_snapshot();
        let sequence = transport.target_sequence.unwrap();
        assert_eq!(transport.packet_released_sequence, Some(sequence));
        assert_eq!(transport.acknowledgement_failure_sequence, Some(sequence));
        assert_eq!(transport.write_succeeded, Some(true));
        assert_eq!(transport.retirement_interruptions, 1);
        assert!(!instance.is_physically_retired());
        assert!(host.uncertain_completion.is_some());
        assert!(!host.is_drained());
        host.revoke();
        assert!(host.finite_work_drained());
        assert!(!host.is_drained(), "logical revoke must retain uncertain original completion until actual helper retirement");
        let retained_before_retirement = quota.snapshot().worker_bytes;
        assert!(
            host.on_helper_retired().is_err(),
            "logical helper retirement state must not release original completion custody"
        );
        assert!(host.uncertain_completion.is_some());
        assert_eq!(quota.snapshot().worker_bytes, retained_before_retirement);
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        assert!(!host.is_drained());
        let before_release = quota.snapshot().worker_bytes;
        host.on_helper_retired().unwrap();
        assert!(host.is_drained());
        assert!(quota.snapshot().worker_bytes < before_release);
        drop(host);
        drop(instance);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_presentation_retirement_uses_exact_original_helper_evidence() {
        let quota = quota();
        let mut original = package_instance_for_source(&quota, subscription_source());
        let mut host =
            NativePresentationHost::new(&original, quota.clone(), EngineLimits::default()).unwrap();
        let mut requests = original.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "presentation.subscribe");
        assert!(host
            .dispatch(&mut original, requests.remove(0))
            .unwrap()
            .is_none());
        assert_eq!(original.pump().unwrap(), CreateState::Pending);
        let mut requests = original.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "presentation.next");
        assert!(host
            .dispatch(&mut original, requests.remove(0))
            .unwrap()
            .is_none());

        host.revoke();
        assert!(host.finite_work_drained());
        assert!(!host.is_drained());

        // This second helper deliberately has the same source, settings,
        // visible instance ID and quota root. Only its private retirement Arc
        // differs, so its physical exit must not settle the original host.
        let mut foreign = package_instance_for_source(&quota, subscription_source());
        foreign.retire_helper().unwrap();
        assert!(foreign.is_physically_retired());
        assert!(!original.is_physically_retired());
        let retained_after_foreign_exit = quota.snapshot().worker_bytes;
        assert!(host.on_helper_retired().is_err());
        assert!(!host.is_drained());
        assert_eq!(quota.snapshot().worker_bytes, retained_after_foreign_exit);

        original.retire_helper().unwrap();
        assert!(original.is_physically_retired());
        assert!(!host.is_drained());
        host.on_helper_retired().unwrap();
        assert!(host.is_drained());

        drop(host);
        drop(foreign);
        drop(original);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    // Prepared only: append inside actual_presentation_custody_tests after the
    // currently frozen validation job terminates. This tests synthetic transport,
    // never grants native pixel or saved-world history authority.
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_receives_all_8192_owner_groups_as_bounded_u32_planes() {
        let quota = quota();
        let source = r#"
export function plan(){return {format:'gray32',fps:30,inputs:{}};}
export async function create(){
    const opened=await __ilium_dispatch('presentation.subscribe',{});
    if(!opened.ok)throw Error('fixture_subscription_open_failed');
    const received=await __ilium_dispatch('presentation.next',{id:opened.value.id,kind:'presentation'});
    if(!received.ok)throw Error('fixture_subscription_next_failed');
    const receipt=received.value;
    const owners=receipt.owners;
    if(owners.token_prefix!=='source-owner-1-')throw Error('fixture_owner_namespace');
    if(!(owners.ids instanceof Uint32Array)||!(owners.dots instanceof Uint32Array))throw Error('fixture_owner_planes');
    if(owners.ids.length!==8192||owners.dots.length!==8192||receipt.emitted_dots!==8192)throw Error('fixture_owner_count');
    for(let i=0;i<8192;i++)if(owners.ids[i]!==i+1||owners.dots[i]!==1)throw Error('fixture_owner_group');
    return {render(){},dispose(){}};
}
"#;
        let mut instance = package_instance_for_source(&quota, source);
        let mut host =
            NativePresentationHost::new(&instance, quota.clone(), EngineLimits::default()).unwrap();
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "presentation.subscribe");
        assert!(host
            .dispatch(&mut instance, requests.remove(0))
            .unwrap()
            .is_none());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "presentation.next");
        assert!(host
            .dispatch(&mut instance, requests.remove(0))
            .unwrap()
            .is_none());
        // Reserve the synthetic fixture's strings and vector before allocating.
        let fixture_admission = quota.reserve_external_storage(2 * 1024 * 1024).unwrap();
        let owners = (1..=8192)
            .map(|id| WorldPresentationOwner {
                token: format!("source-owner-1-{id}"),
                dots: 1,
            })
            .collect();
        host.publish(
            &mut instance,
            WorldPresentationReceipt(std::sync::Arc::new(WorldPresentationReceiptData {
                frame_id: "synthetic_packed_transport_frame".into(),
                source_identity: "f".repeat(64),
                composition_revision: 9,
                emitted_dots: 8192,
                owners,
                owner_namespace: 1,
                admission: fixture_admission,
            })),
        )
        .expect("all admitted owner groups must fit the real metadata limit");
        assert_eq!(instance.pump().unwrap(), CreateState::Ready);
        host.revoke();
        assert!(host.finite_work_drained());
        instance.retire_helper().unwrap();
        host.on_helper_retired().unwrap();
        assert!(host.is_drained());
        drop(host);
        drop(instance);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
