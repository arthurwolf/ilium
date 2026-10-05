//! Native finite packed-clip ownership. Playback has no V8, fetcher or worker.
//! Trusted adapters certify determinism/frozen inputs, retain native source
//! evidence, retire preparation custody, and settle actual terminal emissions.
//! This module does not implement disk compression or a saved-world adapter.
use crate::{
    clip_chunk_store::{ClipChunkReader, ClipChunkStore, ClipChunkWriter, ProceduralFrame},
    clock::{AnimationClock, ClockSample},
    error::{AnimationError, Result},
    manifest::AnimationMode,
    package::Package,
    permissions::{Grant, PermissionBroker},
    plan::{AnimationPlan, PlanBudget},
    surface::{Format, NativeText, PackedSurface, Shape, Snapshot, SourceToken},
    trust::{PackageIdentity, TrustVerifier},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};
const META_BYTES: usize = 65536;
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static NEXT_PRESENTATION: AtomicU64 = AtomicU64::new(1);
fn failure(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("replay: {message}"))
}
fn reserve(quota: &QuotaGroup, bytes: usize) -> Result<StorageAdmission> {
    quota
        .reserve_external_storage(bytes.max(1))
        .map_err(|e| AnimationError::Budget(format!("replay admission: {e:?}")))
}
fn check_stop(stop: &StopToken) -> Result<()> {
    if stop.is_stopped() {
        Err(failure("cancelled"))
    } else {
        Ok(())
    }
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| failure("size overflow"))
}
fn multiply(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(|| failure("size overflow"))
}
fn native_id(counter: &AtomicU64) -> Result<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| failure("native identity exhausted"))
}
fn name(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        Err(failure("invalid native metadata name"))
    } else {
        Ok(())
    }
}
fn bounded_json(value: &Value) -> Result<Vec<u8>> {
    fn check(value: &Value, depth: usize, nodes: &mut usize, bytes: &mut usize) -> Result<()> {
        *nodes = add(*nodes, 1)?;
        if depth > 16 || *nodes > 4096 {
            return Err(failure("metadata structure limit"));
        }
        match value {
            Value::String(text) => *bytes = add(*bytes, text.len())?,
            Value::Array(items) => {
                if items.len() > 2048 {
                    return Err(failure("metadata array limit"));
                }
                for item in items {
                    check(item, depth + 1, nodes, bytes)?;
                }
            }
            Value::Object(items) => {
                if items.len() > 64 {
                    return Err(failure("metadata object limit"));
                }
                for (key, item) in items {
                    *bytes = add(*bytes, key.len())?;
                    check(item, depth + 1, nodes, bytes)?;
                }
            }
            _ => {}
        }
        if *bytes > META_BYTES {
            return Err(failure("metadata byte limit"));
        }
        Ok(())
    }
    check(value, 0, &mut 0, &mut 0)?;
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            Value::Object(items) => {
                let ordered: BTreeMap<_, _> = items.iter().collect();
                Value::Object(
                    ordered
                        .into_iter()
                        .map(|(key, item)| (key.clone(), canonical(item)))
                        .collect(),
                )
            }
            _ => value.clone(),
        }
    }
    let bytes = serde_json::to_vec(&canonical(value))?;
    if bytes.len() > META_BYTES {
        return Err(failure("escaped metadata byte limit"));
    }
    Ok(bytes)
}
/// Producing grant scope, not a transferable grant or helper-authored identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantLineage {
    pub request_id: String,
    pub capability: String,
    pub scope_digest: [u8; 32],
}
impl GrantLineage {
    /// Only the accepted native Grant returned by the original broker can
    /// produce this capture descriptor. It remains data; playback rechecks
    /// the same channel and grant before using the recorded clip.
    pub(crate) fn from_grant(grant: &Grant) -> Result<Self> {
        let capability = serde_json::to_value(grant.right.id)?
            .as_str()
            .ok_or_else(|| failure("grant capability representation"))?
            .to_owned();
        let mut digest = Sha256::new();
        digest.update(b"ilium-replay-grant-scope-v1\0");
        digest.update(serde_json::to_vec(&grant.right.scope)?);
        Ok(Self {
            request_id: grant.request_id.clone(),
            capability,
            scope_digest: digest.finalize().into(),
        })
    }
    pub(crate) fn matches_grant(&self, grant: &Grant) -> Result<bool> {
        Ok(self == &Self::from_grant(grant)?)
    }
}
fn validate_lineage(lineage: &[GrantLineage]) -> Result<()> {
    if lineage.len() > 32 {
        return Err(failure("grant lineage limit"));
    }
    let mut ids = BTreeSet::new();
    for item in lineage {
        name(&item.request_id)?;
        name(&item.capability)?;
        if !ids.insert(&item.request_id) {
            return Err(failure("duplicate producing grant"));
        }
    }
    Ok(())
}
/// Always supplied by the native owning channel, never read from script RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayAuthority {
    pub package_digest: String,
    pub instance_id: u64,
    pub revision: u64,
    pub authorization_epoch: u64,
}
impl ReplayAuthority {
    fn validate(&self, package: &PackageIdentity) -> Result<()> {
        if self.package_digest != package.digest() || self.instance_id == 0 || self.revision == 0 {
            return Err(AnimationError::PermissionDenied(
                "replay owner lineage mismatch".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayAccess {
    Prepare,
    CachedDelivery,
    Playback,
    Emission,
}
/// Nonblocking native grant/epoch check. Informational capture metadata cannot
/// authorize a read. The compositor calls Emission under its revocation fence.
pub trait ReplayAuthorization: Send + Sync {
    fn check(
        &self,
        authority: &ReplayAuthority,
        lineage: &[GrantLineage],
        access: ReplayAccess,
    ) -> Result<()>;
    /// Verify original broker commitment after a real backend flush without
    /// asking whether a now-revoked channel can authorize a NEW operation.
    fn validate_flushed(
        &self,
        authority: &ReplayAuthority,
        proof: &ReplayFlushedProof,
    ) -> Result<()>;
}
/// The terminal owner can construct this only after its original broker
/// settles a complete physical flush. It carries no guessed source dots.
pub struct ReplayFlushedProof {
    broker: Arc<Mutex<PermissionBroker>>,
    authority: ReplayAuthority,
    _storage: StorageAdmission,
}
impl ReplayFlushedProof {
    pub(crate) fn from_native(
        broker: Arc<Mutex<PermissionBroker>>,
        authority: ReplayAuthority,
        storage: StorageAdmission,
    ) -> Self {
        Self {
            broker,
            authority,
            _storage: storage,
        }
    }
    pub(crate) fn belongs_to(
        &self,
        broker: &Arc<Mutex<PermissionBroker>>,
        authority: &ReplayAuthority,
    ) -> bool {
        Arc::ptr_eq(&self.broker, broker) && &self.authority == authority
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputFamily {
    Pointer,
    Location,
    Audio,
    Series,
    Earthquakes,
    Aircraft,
    Boats,
    Chess,
    Astronomy,
    Weather,
}
/// Native verified frozen recording identity; construction never records a live
/// device/feed. Its producing root and capture/grant metadata remain retained.
pub struct FrozenInputs {
    recording: Option<String>,
    families: BTreeSet<InputFamily>,
    digest: [u8; 32],
    civil_anchor: Option<i64>,
    capture_label: String,
    lineage: Vec<GrantLineage>,
    quota: QuotaGroup,
    _storage: StorageAdmission,
}
impl FrozenInputs {
    pub fn from_host(
        quota: QuotaGroup,
        recording: Option<&str>,
        families: &[InputFamily],
        digest: [u8; 32],
        civil_anchor: Option<i64>,
        capture_label: &str,
        lineage: &[GrantLineage],
    ) -> Result<Arc<Self>> {
        if families.len() > 10
            || capture_label.len() > 1024
            || capture_label.chars().any(char::is_control)
        {
            return Err(failure("frozen input metadata limit"));
        }
        if let Some(recording) = recording {
            name(recording)?;
        }
        validate_lineage(lineage)?;
        let set: BTreeSet<_> = families.iter().copied().collect();
        if set.len() != families.len() {
            return Err(failure("duplicate frozen input"));
        }
        let storage = reserve(&quota, 32768)?;
        Ok(Arc::new(Self {
            recording: recording.map(str::to_owned),
            families: set,
            digest,
            civil_anchor,
            capture_label: capture_label.into(),
            lineage: lineage.to_vec(),
            quota,
            _storage: storage,
        }))
    }
    pub fn capture_label(&self) -> &str {
        &self.capture_label
    }
}
/// Native execution-profile witness. It binds an actual sealed helper launch,
/// immutable package/settings/environment and a no-external-dependency plan.
/// It certifies the finite captured clip's replay conditions, not a theorem
/// about arbitrary JavaScript or a source-owned recording.
#[derive(Debug)]
pub struct ReplayCertification {
    reset_rules_digest: [u8; 32],
    clock_random_binding_digest: [u8; 32],
    prepared_assets_digest: [u8; 32],
    async_order_digest: [u8; 32],
    full_unoccluded: bool,
    loop_state_continuity_verified: bool,
    binding: [u8; 32],
    ambient_seed: u32,
    bootstrap_digest: [u8; 32],
    environment_digest: [u8; 32],
    helper_build_digest: [u8; 32],
    recorded_video_opens: u64,
}
/// Immutable execution inputs used by the certificate's ordered digest binding.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplayExecutionIdentity {
    pub ambient_seed: u32,
    pub bootstrap_digest: [u8; 32],
    pub environment_digest: [u8; 32],
    pub helper_build_digest: [u8; 32],
}

impl ReplayCertification {
    /// A source-free package may be certified without editing Rust for each
    /// archive. The trusted helper configured time/random before guest load;
    /// the native producer additionally rejects every external host request.
    pub(crate) fn sealed_procedural(
        package: &Package,
        plan: &AnimationPlan,
        settings: &Value,
        execution_identity: ReplayExecutionIdentity,
        preparation_yields: u64,
    ) -> Result<Self> {
        Self::sealed(
            package,
            plan,
            settings,
            execution_identity,
            preparation_yields,
            0,
        )
    }
    pub(crate) fn sealed_recorded_video(
        package: &Package,
        plan: &AnimationPlan,
        settings: &Value,
        execution_identity: ReplayExecutionIdentity,
        preparation_yields: u64,
        recorded_video_opens: u64,
    ) -> Result<Self> {
        if !(1..=8).contains(&recorded_video_opens) {
            return Err(failure("recorded Video count"));
        }
        Self::sealed(
            package,
            plan,
            settings,
            execution_identity,
            preparation_yields,
            recorded_video_opens,
        )
    }
    fn sealed(
        package: &Package,
        plan: &AnimationPlan,
        settings: &Value,
        execution_identity: ReplayExecutionIdentity,
        preparation_yields: u64,
        recorded_video_opens: u64,
    ) -> Result<Self> {
        let ReplayExecutionIdentity {
            ambient_seed,
            bootstrap_digest,
            environment_digest,
            helper_build_digest,
        } = execution_identity;
        let replay = plan
            .replay
            .as_ref()
            .ok_or_else(|| failure("replay declaration absent"))?;
        if !plan.inputs.is_empty()
            || (recorded_video_opens == 0 && !plan.permissions.is_empty())
            || (recorded_video_opens != 0
                && plan.permissions.iter().any(|permission| {
                    !matches!(permission.id.as_str(), "disk.read" | "network.http")
                }))
            || plan.preparation.is_some()
            || replay.seamless
            || replay.input_recording.is_some()
            || replay.civil_anchor_ms.is_some()
            || plan.mode_unavailable_reason.is_some()
        {
            return Err(failure("finite recorded Video replay profile unavailable"));
        }
        if helper_build_digest == [0; 32] {
            return Err(failure("running helper build identity absent"));
        }
        let binding = replay_binding(
            package,
            plan,
            settings,
            execution_identity,
            recorded_video_opens != 0,
        )?;
        let mut assets = Sha256::new();
        assets.update(b"ilium-audited-assets-v1");
        for (path, bytes) in package.files() {
            if path.starts_with("assets/") {
                assets.update(path.as_bytes());
                assets.update((bytes.len() as u64).to_le_bytes());
                assets.update(Sha256::digest(bytes));
            }
        }
        Ok(Self {
            reset_rules_digest: Sha256::digest(
                [b"finite-sequential-samples-v1".as_slice(), &binding].concat(),
            )
            .into(),
            clock_random_binding_digest: Sha256::digest(
                [b"sealed-ambient-v1".as_slice(), &binding].concat(),
            )
            .into(),
            prepared_assets_digest: assets.finalize().into(),
            async_order_digest: if recorded_video_opens != 0 {
                Sha256::digest(
                    [
                        b"sequential-recorded-video-v1".as_slice(),
                        &preparation_yields.to_le_bytes(),
                        &recorded_video_opens.to_le_bytes(),
                        &binding,
                    ]
                    .concat(),
                )
                .into()
            } else if preparation_yields == 0 {
                Sha256::digest([b"no-native-async-requests-v1".as_slice(), &binding].concat())
                    .into()
            } else {
                Sha256::digest(
                    [
                        b"sequential-create-yields-v1".as_slice(),
                        &preparation_yields.to_le_bytes(),
                        &binding,
                    ]
                    .concat(),
                )
                .into()
            },
            full_unoccluded: true,
            loop_state_continuity_verified: false,
            binding,
            ambient_seed,
            bootstrap_digest,
            environment_digest,
            helper_build_digest,
            recorded_video_opens,
        })
    }
}
fn replay_binding(
    package: &Package,
    plan: &AnimationPlan,
    settings: &Value,
    execution_identity: ReplayExecutionIdentity,
    recorded_video: bool,
) -> Result<[u8; 32]> {
    let ReplayExecutionIdentity {
        ambient_seed,
        bootstrap_digest,
        environment_digest,
        helper_build_digest,
    } = execution_identity;
    let mut hash = Sha256::new();
    hash.update(if recorded_video {
        b"ilium-audited-recorded-video-binding-v1".as_slice()
    } else {
        b"ilium-audited-procedural-binding-v1".as_slice()
    });
    hash.update(package.digest().as_bytes());
    hash.update(bounded_json(&serde_json::to_value(plan)?)?);
    hash.update(bounded_json(settings)?);
    hash.update(ambient_seed.to_le_bytes());
    hash.update(bootstrap_digest);
    hash.update(environment_digest);
    hash.update(helper_build_digest);
    // A cached text raster cannot survive a bundled font revision, even if a
    // distributor accidentally reuses an otherwise matching helper digest.
    hash.update(Sha256::digest(include_bytes!(
        "../../ilium-ambient/assets/fonts/CascadiaCode-Regular.otf"
    )));
    hash.update(env!("CARGO_PKG_VERSION").as_bytes());
    hash.update(v8::V8::get_version().as_bytes());
    hash.update(std::env::consts::ARCH.as_bytes());
    hash.update(std::env::consts::OS.as_bytes());
    Ok(hash.finalize().into())
}
/// Actual history commit adapter. Its payload is native frozen ownership
/// evidence, never arbitrary script owner IDs. Preparation never calls it.
pub trait ReplayHistory: Send + Sync {
    fn emitted(
        &self,
        source_digest: [u8; 32],
        frozen_evidence: &[u8],
        receipt: &ReplayReceipt,
        surviving_dots: &[usize],
    ) -> Result<()>;
}
pub struct NativeEvidenceInput {
    pub source_digest: [u8; 32],
    pub lineage: Vec<GrantLineage>,
    pub payload: Vec<u8>,
    pub history: Arc<dyn ReplayHistory>,
}
struct EvidenceEntry {
    token: SourceToken,
    source: [u8; 32],
    lineage: Vec<GrantLineage>,
    payload: Vec<u8>,
    history: Arc<dyn ReplayHistory>,
}
/// Persistent Rust evidence independent of V8 and ephemeral source receipt slots.
/// Native source adapters use tokens issued HERE in their Surface owner patches;
/// all participating adapters must share this evidence registry authority.
pub struct FrozenEvidence {
    entries: BTreeMap<u64, EvidenceEntry>,
    digest: [u8; 32],
    package_digest: String,
    quota: QuotaGroup,
    _storage: StorageAdmission,
}
impl FrozenEvidence {
    pub fn from_native(
        quota: QuotaGroup,
        package: &PackageIdentity,
        inputs: &[NativeEvidenceInput],
    ) -> Result<Arc<Self>> {
        if inputs.len() > 256 {
            return Err(failure("frozen source count"));
        }
        let mut total = 65536usize;
        for input in inputs {
            validate_lineage(&input.lineage)?;
            if input.payload.len() > 4 * 1024 * 1024 {
                return Err(failure("source evidence size"));
            }
            total = add(total, add(input.payload.len(), 8192)?)?;
        }
        if total > 32 * 1024 * 1024 {
            return Err(AnimationError::Budget("source evidence total".into()));
        }
        let storage = reserve(&quota, total)?;
        let mut entries = BTreeMap::new();
        let mut stable = BTreeSet::new();
        for input in inputs {
            let mut hash = Sha256::new();
            hash.update(input.source_digest);
            hash.update(&input.payload);
            hash.update(bounded_json(&serde_json::to_value(&input.lineage)?)?);
            let digest: [u8; 32] = hash.finalize().into();
            if !stable.insert(digest) {
                return Err(failure("duplicate native source evidence"));
            }
            let token = SourceToken::from_native(native_id(&NEXT_TOKEN)?)
                .map_err(|e| failure(&e.to_string()))?;
            entries.insert(
                token.evidence_key(),
                EvidenceEntry {
                    token,
                    source: input.source_digest,
                    lineage: input.lineage.clone(),
                    payload: input.payload.clone(),
                    history: input.history.clone(),
                },
            );
        }
        let mut hash = Sha256::new();
        for digest in stable {
            hash.update(digest);
        }
        let digest = hash.finalize().into();
        Ok(Arc::new(Self {
            entries,
            digest,
            package_digest: package.digest().into(),
            quota,
            _storage: storage,
        }))
    }
    pub fn token(&self, index: usize) -> Result<SourceToken> {
        self.entries
            .values()
            .nth(index)
            .map(|entry| entry.token)
            .ok_or_else(|| failure("source evidence missing"))
    }
    /// Resolve the token issued for this exact native source. Token-key order
    /// is unrelated to the input order supplied to `from_native`.
    pub fn token_for_source(&self, source_digest: [u8; 32], payload: &[u8]) -> Result<SourceToken> {
        let mut matches = self
            .entries
            .values()
            .filter(|entry| entry.source == source_digest && entry.payload.as_slice() == payload);
        let token = matches
            .next()
            .map(|entry| entry.token)
            .ok_or_else(|| failure("recorded source evidence missing"))?;
        if matches.next().is_some() {
            return Err(failure("recorded source evidence ambiguous"));
        }
        Ok(token)
    }
    fn resolve(&self, token: SourceToken) -> Result<&EvidenceEntry> {
        self.entries
            .get(&token.evidence_key())
            .ok_or_else(|| failure("unretained native source token"))
    }
}

#[cfg(test)]
mod recorded_video_evidence_tests {
    use super::*;
    use ilium_execution::QuotaLimits;

    struct SyntheticHistory;
    impl ReplayHistory for SyntheticHistory {
        fn emitted(&self, _: [u8; 32], _: &[u8], _: &ReplayReceipt, _: &[usize]) -> Result<()> {
            Err(failure("synthetic history is never an emission authority"))
        }
    }

    #[test]
    fn token_resolution_uses_original_source_identity_not_token_order() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 0,
            worker_bytes: 4096,
        });
        let storage = quota.reserve_external_storage(1024).unwrap();
        let mut entries = BTreeMap::new();
        let history: Arc<dyn ReplayHistory> = Arc::new(SyntheticHistory);
        for (token_number, source, payload) in [
            (20, [1u8; 32], b"first".to_vec()),
            (10, [2u8; 32], b"second".to_vec()),
        ] {
            let token = SourceToken::from_native(token_number).unwrap();
            entries.insert(
                token.evidence_key(),
                EvidenceEntry {
                    token,
                    source,
                    payload,
                    lineage: Vec::new(),
                    history: Arc::clone(&history),
                },
            );
        }
        let evidence = FrozenEvidence {
            entries,
            digest: [0; 32],
            package_digest: "synthetic-test".into(),
            quota,
            _storage: storage,
        };
        assert_eq!(
            evidence
                .token_for_source([1; 32], b"first")
                .unwrap()
                .evidence_key(),
            20
        );
        assert_eq!(
            evidence
                .token_for_source([2; 32], b"second")
                .unwrap()
                .evidence_key(),
            10
        );
        assert!(evidence.token_for_source([1; 32], b"second").is_err());
        assert!(evidence.token_for_source([3; 32], b"first").is_err());
    }
}
pub struct ClipSpecification<'a> {
    pub package: &'a Package,
    pub verifier: &'a TrustVerifier,
    pub plan: &'a AnimationPlan,
    pub settings: &'a Value,
    pub shape: Shape,
    pub backend: &'a str,
    pub api_version: u32,
    pub appearance_digest: [u8; 32],
    pub certification: ReplayCertification,
    pub frozen: Arc<FrozenInputs>,
    pub evidence: Arc<FrozenEvidence>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClipKey([u8; 32]);
impl ClipKey {
    pub fn hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
pub struct ClipSpec {
    key: ClipKey,
    package: PackageIdentity,
    shape: Shape,
    fps: f64,
    duration: f64,
    frames: usize,
    seamless: bool,
    frozen: Arc<FrozenInputs>,
    evidence: Arc<FrozenEvidence>,
    lineage: Vec<GrantLineage>,
    recorded_video: bool,
    _metadata: StorageAdmission,
}
impl ClipSpec {
    pub fn from_accepted(quota: &QuotaGroup, input: ClipSpecification<'_>) -> Result<Arc<Self>> {
        if !quota.shares_root(&input.frozen.quota) || !quota.shares_root(&input.evidence.quota) {
            return Err(AnimationError::PermissionDenied(
                "replay original root mismatch".into(),
            ));
        }
        let metadata = reserve(quota, 1024 * 1024)?;
        name(input.backend)?;
        if !input
            .package
            .manifest()
            .modes
            .contains(&AnimationMode::PreRendered)
        {
            return Err(failure("package supports live mode only"));
        }
        let plan_value = serde_json::to_value(input.plan)?;
        bounded_json(&plan_value)?;
        bounded_json(input.settings)?;
        let plan = AnimationPlan::parse(
            &plan_value,
            AnimationMode::PreRendered,
            PlanBudget::default(),
        )?;
        let replay = plan
            .replay
            .as_ref()
            .ok_or_else(|| failure("replay declaration absent"))?;
        let package = input.verifier.verify(input.package);
        if input.evidence.package_digest != package.digest() {
            return Err(AnimationError::PermissionDenied(
                "frozen evidence producing package mismatch".into(),
            ));
        }
        if (input.certification.recorded_video_opens == 0 && !input.evidence.entries.is_empty())
            || (input.certification.recorded_video_opens != 0
                && input.evidence.entries.len()
                    != input.certification.recorded_video_opens as usize)
        {
            return Err(failure(
                "recorded Video evidence count differs from certified opens",
            ));
        }
        let format = match input.shape.format {
            Format::Mask8 => "mask8",
            Format::Mono1 => "mono1",
            Format::Mono8 => "mono8",
            Format::Gray8 => "gray8",
            Format::Gray32 => "gray32",
            Format::Rgb8 => "rgb8",
            Format::Rgba8 => "rgba8",
        };
        if plan
            .output
            .as_ref()
            .map(|o| o.format.as_str())
            .or(plan.format.as_deref())
            != Some(format)
        {
            return Err(failure("accepted format mismatch"));
        }
        if input.api_version != input.package.manifest().api_version {
            return Err(failure("API version mismatch"));
        }
        if let Some(output) = &plan.output {
            let mode = match input.shape.mode {
                crate::surface::Mode::Cells => "cells",
                crate::surface::Mode::Pixels => "pixels",
            };
            let update = match input.shape.update {
                crate::surface::Update::Replace => "replace",
                crate::surface::Update::Retain => "retain",
            };
            let colour = match input.shape.colour_space {
                crate::surface::ColourSpace::Srgb => "srgb",
                crate::surface::ColourSpace::Linear => "linear",
            };
            if output.mode != mode
                || output.update != update
                || output.cell_rgb != input.shape.cell_rgb
                || output.colour_space.as_deref().unwrap_or("srgb") != colour
            {
                return Err(failure("accepted output shape mismatch"));
            }
        }
        input.shape.layout().map_err(|e| failure(&e.to_string()))?;
        if input.certification.binding
            != replay_binding(
                input.package,
                &plan,
                input.settings,
                ReplayExecutionIdentity {
                    ambient_seed: input.certification.ambient_seed,
                    bootstrap_digest: input.certification.bootstrap_digest,
                    environment_digest: input.certification.environment_digest,
                    helper_build_digest: input.certification.helper_build_digest,
                },
                input.certification.recorded_video_opens != 0,
            )?
        {
            return Err(failure("native replay certificate binding mismatch"));
        }
        if !input.certification.full_unoccluded
            || [
                input.certification.reset_rules_digest,
                input.certification.clock_random_binding_digest,
                input.certification.prepared_assets_digest,
                input.certification.async_order_digest,
            ]
            .contains(&[0; 32])
        {
            return Err(failure("deterministic full-viewport certification absent"));
        }
        if replay.seamless && !input.certification.loop_state_continuity_verified {
            return Err(failure("seamless loop state continuity not certified"));
        }
        let demands = &plan.inputs;
        let mut required = BTreeSet::new();
        for (present, family) in [
            (demands.pointer.is_some(), InputFamily::Pointer),
            (demands.location.is_some(), InputFamily::Location),
            (demands.audio.is_some(), InputFamily::Audio),
            (demands.series.is_some(), InputFamily::Series),
            (demands.earthquakes.is_some(), InputFamily::Earthquakes),
            (demands.aircraft.is_some(), InputFamily::Aircraft),
            (demands.boats.is_some(), InputFamily::Boats),
            (demands.chess.is_some(), InputFamily::Chess),
            (demands.astronomy.is_some(), InputFamily::Astronomy),
            (demands.weather.is_some(), InputFamily::Weather),
        ] {
            if present {
                required.insert(family);
            }
        }
        if !required.is_subset(&input.frozen.families)
            || (!required.is_empty()
                && replay.input_recording.as_deref() != input.frozen.recording.as_deref())
        {
            return Err(failure(
                "requested live input lacks authenticated frozen recording",
            ));
        }
        if demands.clock.as_ref().is_some_and(|clock| clock.civil)
            && (replay.civil_anchor_ms.is_none()
                || replay.civil_anchor_ms != input.frozen.civil_anchor)
        {
            return Err(failure("civil clock lacks matching frozen anchor"));
        }
        let fps = replay.fps.unwrap_or(plan.fps);
        let duration = replay.duration_seconds;
        let count = (fps * duration).ceil();
        if !count.is_finite() || !(1.0..=14400.0).contains(&count) {
            return Err(failure("finite sample count"));
        }
        let mut lineage = input.frozen.lineage.clone();
        for entry in input.evidence.entries.values() {
            for item in &entry.lineage {
                if !lineage.contains(item) {
                    lineage.push(item.clone());
                }
            }
        }
        validate_lineage(&lineage)?;
        let material = json!({"package":package.digest(),"verified_ilium":package.is_ilium(),"api":input.api_version,
            "backend":input.backend,"plan":plan_value,"settings":input.settings,"shape":input.shape,
            "appearance":input.appearance_digest,"frozen":input.frozen.digest,"recording":input.frozen.recording,
            "families":input.frozen.families,"civil_anchor":input.frozen.civil_anchor,"frozen_lineage":input.frozen.lineage,
            "evidence":input.evidence.digest,"reset":input.certification.reset_rules_digest,
            "clock_random":input.certification.clock_random_binding_digest,"assets":input.certification.prepared_assets_digest,
            "async_order":input.certification.async_order_digest});
        let key = ClipKey(Sha256::digest(bounded_json(&material)?).into());
        Ok(Arc::new(Self {
            key,
            package,
            shape: input.shape,
            fps,
            duration,
            frames: count as usize,
            seamless: replay.seamless,
            frozen: input.frozen,
            evidence: input.evidence,
            lineage,
            recorded_video: input.certification.recorded_video_opens != 0,
            _metadata: metadata,
        }))
    }
    pub fn key(&self) -> ClipKey {
        self.key
    }
    pub fn frame_count(&self) -> usize {
        self.frames
    }
    pub fn capture_label(&self) -> &str {
        self.frozen.capture_label()
    }
    pub fn is_recorded_video(&self) -> bool {
        self.recorded_video
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ReplayLimits {
    pub max_clips: usize,
    pub max_preparations: usize,
    pub max_frames: usize,
    pub max_clip_bytes: usize,
    pub max_cache_bytes: usize,
    pub max_text_bytes: usize,
    pub max_text_spans: usize,
    pub max_leases: usize,
}
impl Default for ReplayLimits {
    fn default() -> Self {
        Self {
            max_clips: 8,
            max_preparations: 1,
            max_frames: 3600,
            max_clip_bytes: 128 * 1024 * 1024,
            max_cache_bytes: 256 * 1024 * 1024,
            max_text_bytes: 16384,
            max_text_spans: 64,
            max_leases: 3,
        }
    }
}
impl ReplayLimits {
    fn validate(self) -> Result<()> {
        if !(1..=32).contains(&self.max_clips)
            || !(1..=4).contains(&self.max_preparations)
            || !(1..=14400).contains(&self.max_frames)
            || self.max_clip_bytes == 0
            || self.max_clip_bytes > 1024 * 1024 * 1024
            || self.max_cache_bytes < self.max_clip_bytes
            || self.max_cache_bytes > 2 * 1024 * 1024 * 1024
            || self.max_text_bytes > 16384
            || self.max_text_spans > 64
            || !(1..=16).contains(&self.max_leases)
        {
            return Err(failure("finite clip limits"));
        }
        Ok(())
    }
}
/// Owner adapter must hold only this preparation's source/V8/native work.
/// bind_retirement_custody attaches the supplied original-root lease to actual
/// exit ownership; it MUST survive cancelled/stuck work after callers drop.
/// retire returns success only after V8/process/threads and acquisition handles
/// are actually retired. is_retired reads that proof, never a cancellation flag.
pub trait ReplayPreparationOwner: Send + Sync {
    fn quota_group(&self) -> QuotaGroup;
    fn bind_retirement_custody(&self, storage: Arc<StorageAdmission>);
    fn cancel(&self);
    fn retire(&self) -> Result<()>;
    fn is_retired(&self) -> bool;
}
#[derive(Clone)]
struct ClipFrame {
    packed: PackedSurface,
    text: Vec<NativeText>,
}
pub struct ReplayClip {
    spec: Arc<ClipSpec>,
    frames: Vec<ClipFrame>,
    disk: Option<Arc<ClipChunkReader>>,
    charged_bytes: usize,
    _storage: Arc<StorageAdmission>,
}
impl ReplayClip {
    pub fn key(&self) -> ClipKey {
        self.spec.key
    }
    pub fn frame_count(&self) -> usize {
        self.spec.frames
    }
    pub fn capture_label(&self) -> &str {
        self.spec.capture_label()
    }
    /// Archive only a complete source-free clip. The native cache directory
    /// and quota are supplied by the host; no SourceToken is serialized.
    /// This transition occurs after finish proved physical producer retirement.
    pub fn archive_procedural(
        &self,
        store: &ClipChunkStore,
        stop: &StopToken,
    ) -> Result<Arc<ReplayClip>> {
        check_stop(stop)?;
        if !store.shares_root(&self.spec.frozen.quota)
            || !self.spec.evidence.entries.is_empty()
            || !self.spec.lineage.is_empty()
            || self.frames.len() != self.spec.frames
            || self.disk.is_some()
        {
            return Err(AnimationError::PermissionDenied(
                "protected or incomplete replay cannot use procedural archive".into(),
            ));
        }
        let mut writer = store.begin_procedural(&self.key().hex(), self.spec.frames, false)?;
        for frame in &self.frames {
            check_stop(stop)?;
            writer.push_packed(self.spec.shape, &frame.packed, &frame.text)?;
        }
        check_stop(stop)?;
        writer.finish()?;
        let disk = Arc::new(store.open_procedural(&self.key().hex())?);
        if disk.frame_count() != self.spec.frames {
            return Err(failure("procedural archive frame count"));
        }
        let storage = Arc::new(reserve(&self.spec.frozen.quota, 8192)?);
        Ok(Arc::new(ReplayClip {
            spec: self.spec.clone(),
            frames: Vec::new(),
            disk: Some(disk),
            charged_bytes: 8192,
            _storage: storage,
        }))
    }
}
struct CacheEntry {
    clip: Arc<ReplayClip>,
    used: u64,
}
struct PreparationEntry {
    job: u64,
    authority: ReplayAuthority,
    stop: StopToken,
    owner: Option<Arc<dyn ReplayPreparationOwner>>,
}
struct CacheState {
    clips: BTreeMap<ClipKey, CacheEntry>,
    preparing: BTreeMap<ClipKey, PreparationEntry>,
    next: u64,
    clock: u64,
    _storage: StorageAdmission,
}
pub struct ReplayCache {
    quota: QuotaGroup,
    limits: ReplayLimits,
    state: Arc<Mutex<CacheState>>,
}
pub enum Preparation {
    Cached(Arc<ReplayClip>),
    InProgress,
    Started(Box<ClipPreparation>),
}
impl ReplayCache {
    pub fn new(quota: QuotaGroup, limits: ReplayLimits) -> Result<Self> {
        limits.validate()?;
        let metadata = reserve(&quota, 65536)?;
        Ok(Self {
            quota,
            limits,
            state: Arc::new(Mutex::new(CacheState {
                clips: BTreeMap::new(),
                preparing: BTreeMap::new(),
                next: 1,
                clock: 0,
                _storage: metadata,
            })),
        })
    }
    /// Lazy owner factory is never invoked on a cache hit or single-flight join.
    /// Native caller executes this on its existing admitted preparation worker.
    pub fn begin(
        &self,
        spec: Arc<ClipSpec>,
        authority: ReplayAuthority,
        authorization: Arc<dyn ReplayAuthorization>,
        stop: StopToken,
        owner_factory: impl FnOnce() -> Result<Arc<dyn ReplayPreparationOwner>>,
    ) -> Result<Preparation> {
        self.begin_with_store(spec, authority, authorization, stop, None, owner_factory)
    }
    /// Stream a source-free procedural preparation to the admitted native
    /// cache directory. A source-owned producer needs retained cold evidence
    /// and must use begin() until that adapter exists.
    pub fn begin_streaming(
        &self,
        spec: Arc<ClipSpec>,
        authority: ReplayAuthority,
        authorization: Arc<dyn ReplayAuthorization>,
        stop: StopToken,
        store: Arc<ClipChunkStore>,
        owner_factory: impl FnOnce() -> Result<Arc<dyn ReplayPreparationOwner>>,
    ) -> Result<Preparation> {
        self.begin_with_store(
            spec,
            authority,
            authorization,
            stop,
            Some(store),
            owner_factory,
        )
    }
    fn begin_with_store(
        &self,
        spec: Arc<ClipSpec>,
        authority: ReplayAuthority,
        authorization: Arc<dyn ReplayAuthorization>,
        stop: StopToken,
        store: Option<Arc<ClipChunkStore>>,
        owner_factory: impl FnOnce() -> Result<Arc<dyn ReplayPreparationOwner>>,
    ) -> Result<Preparation> {
        check_stop(&stop)?;
        authority.validate(&spec.package)?;
        authorization.check(&authority, &spec.lineage, ReplayAccess::Prepare)?;
        if !self.quota.shares_root(&spec.frozen.quota)
            || !self.quota.shares_root(&spec.evidence.quota)
        {
            return Err(failure("clip cache original root mismatch"));
        }
        if let Some(store) = &store {
            if !store.shares_root(&self.quota)
                || !spec.evidence.entries.is_empty()
                || !spec.lineage.is_empty()
            {
                return Err(AnimationError::PermissionDenied(
                    "streaming clip lacks original procedural ownership".into(),
                ));
            }
        }
        let (bytes, scratch) = estimate(&spec, self.limits, store.is_some())?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        state.clock = state
            .clock
            .checked_add(1)
            .ok_or_else(|| failure("LRU clock exhausted"))?;
        let clock = state.clock;
        if let Some(entry) = state.clips.get_mut(&spec.key) {
            authorization.check(&authority, &spec.lineage, ReplayAccess::CachedDelivery)?;
            entry.used = clock;
            return Ok(Preparation::Cached(entry.clip.clone()));
        }
        if state.preparing.contains_key(&spec.key) {
            return Ok(Preparation::InProgress);
        }
        if state.preparing.len() >= self.limits.max_preparations {
            return Err(AnimationError::Budget("replay preparation slots".into()));
        }
        let job = state.next;
        state.next = state
            .next
            .checked_add(1)
            .ok_or_else(|| failure("preparation identity exhausted"))?;
        let storage = Arc::new(reserve(&self.quota, bytes)?);
        let scratch_storage = reserve(&self.quota, scratch)?;
        let child_stop = stop.child();
        state.preparing.insert(
            spec.key,
            PreparationEntry {
                job,
                authority: authority.clone(),
                stop: child_stop.clone(),
                owner: None,
            },
        );
        drop(state);
        let mut created_owner: Option<Arc<dyn ReplayPreparationOwner>> = None;
        let setup = (|| {
            let owner = owner_factory()?;
            created_owner = Some(owner.clone());
            owner.bind_retirement_custody(storage.clone());
            let mut state = self
                .state
                .lock()
                .map_err(|_| failure("clip cache poisoned"))?;
            let entry = state
                .preparing
                .get_mut(&spec.key)
                .ok_or_else(|| failure("preparation retired during setup"))?;
            if entry.job != job {
                return Err(failure("stale preparation setup"));
            }
            // Publish physical custody before fallible validation, so cancelled
            // setup remains single-flight until its native owner truly exits.
            entry.owner = Some(owner.clone());
            if !self.quota.shares_root(&owner.quota_group()) {
                return Err(failure("preparation owner original root mismatch"));
            }
            check_stop(&child_stop)?;
            Ok(owner)
        })();
        let owner = match setup {
            Ok(owner) => owner,
            Err(error) => {
                child_stop.stop();
                if let Some(owner) = &created_owner {
                    owner.cancel();
                }
                if let Ok(mut state) = self.state.lock() {
                    if let Some(entry) = state.preparing.get_mut(&spec.key) {
                        entry.owner = created_owner.clone();
                    }
                    if created_owner
                        .as_ref()
                        .is_none_or(|owner| owner.is_retired())
                    {
                        state.preparing.remove(&spec.key);
                    }
                }
                return Err(error);
            }
        };
        let mut frames = Vec::new();
        let writer = if let Some(store) = &store {
            match store.begin_procedural(&spec.key.hex(), spec.frames, false) {
                Ok(writer) => Some(writer),
                Err(error) => {
                    child_stop.stop();
                    owner.cancel();
                    return Err(error);
                }
            }
        } else {
            if frames.try_reserve_exact(spec.frames).is_err() {
                child_stop.stop();
                owner.cancel();
                return Err(failure("clip allocation"));
            }
            None
        };
        // The existing 8192-byte retained metadata envelope includes this boxed owner.
        Ok(Preparation::Started(Box::new(ClipPreparation {
            state: self.state.clone(),
            ticket_identity: Arc::new(()),
            limits: self.limits,
            spec,
            authority,
            authorization,
            stop: child_stop,
            job,
            owner,
            frames,
            writer,
            store,
            first_frame: None,
            written: 0,
            pending: None,
            boundary_verified: false,
            failed: false,
            published: false,
            storage,
            _scratch: scratch_storage,
            charged_bytes: bytes,
        })))
    }
    pub fn invalidate_preparations(&self, current: &ReplayAuthority) -> Result<()> {
        let state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        for entry in state.preparing.values() {
            if entry.authority.instance_id == current.instance_id && entry.authority != *current {
                entry.stop.stop();
                if let Some(owner) = &entry.owner {
                    owner.cancel();
                }
            }
        }
        Ok(())
    }
    pub fn collect_retired(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        state.preparing.retain(|_, entry| {
            !entry
                .owner
                .as_ref()
                .is_some_and(|owner| entry.stop.is_stopped() && owner.is_retired())
        });
        Ok(())
    }
    pub fn evict(&self, key: ClipKey) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?
            .clips
            .remove(&key)
            .is_some())
    }
    pub fn contains(&self, key: ClipKey) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?
            .clips
            .contains_key(&key))
    }
    /// Swap a physically retired in-memory clip for its completed native disk
    /// stream. Existing presentation leases retain their original frame bytes.
    pub fn replace_with_disk(&self, old: &Arc<ReplayClip>, disk: Arc<ReplayClip>) -> Result<()> {
        if old.key() != disk.key()
            || disk.disk.is_none()
            || !self.quota.shares_root(&disk.spec.frozen.quota)
        {
            return Err(failure("foreign procedural archive"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        let entry = state
            .clips
            .get_mut(&old.key())
            .ok_or_else(|| failure("clip evicted"))?;
        if !Arc::ptr_eq(&entry.clip, old) {
            return Err(failure("clip changed during archive"));
        }
        entry.clip = disk;
        Ok(())
    }
    /// A cold index is useful only after the current native activation has
    /// physically retired its helper. Source-owned cold evidence is unavailable.
    pub fn open_procedural_from_disk(
        &self,
        spec: Arc<ClipSpec>,
        authority: ReplayAuthority,
        authorization: Arc<dyn ReplayAuthorization>,
        store: &ClipChunkStore,
    ) -> Result<Arc<ReplayClip>> {
        authority.validate(&spec.package)?;
        if !self.quota.shares_root(&spec.frozen.quota)
            || !store.shares_root(&self.quota)
            || !spec.evidence.entries.is_empty()
            || !spec.lineage.is_empty()
        {
            return Err(AnimationError::PermissionDenied(
                "cold replay lacks original procedural ownership".into(),
            ));
        }
        authorization.check(&authority, &spec.lineage, ReplayAccess::CachedDelivery)?;
        let disk = Arc::new(store.open_procedural(&spec.key.hex())?);
        if disk.frame_count() != spec.frames {
            return Err(failure("cold procedural frame count"));
        }
        let storage = Arc::new(reserve(&self.quota, 8192)?);
        let clip = Arc::new(ReplayClip {
            spec: spec.clone(),
            frames: Vec::new(),
            disk: Some(disk),
            charged_bytes: 8192,
            _storage: storage,
        });
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        if state.preparing.contains_key(&spec.key) {
            return Err(failure("cold clip conflicts with live preparation"));
        }
        state.clock = state
            .clock
            .checked_add(1)
            .ok_or_else(|| failure("LRU clock exhausted"))?;
        let used = state.clock;
        if let Some(entry) = state.clips.get_mut(&spec.key) {
            entry.used = used;
            return Ok(entry.clip.clone());
        }
        while state.clips.len() >= self.limits.max_clips
            || state
                .clips
                .values()
                .try_fold(clip.charged_bytes, |n, entry| {
                    add(n, entry.clip.charged_bytes)
                })?
                > self.limits.max_cache_bytes
        {
            let oldest = state
                .clips
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
                .ok_or_else(|| failure("cold cache admission"))?;
            state.clips.remove(&oldest);
        }
        state.clips.insert(
            spec.key,
            CacheEntry {
                clip: clip.clone(),
                used,
            },
        );
        Ok(clip)
    }
}
fn estimate(spec: &ClipSpec, limits: ReplayLimits, streaming: bool) -> Result<(usize, usize)> {
    if spec.frames > limits.max_frames {
        return Err(AnimationError::Budget("replay frame count".into()));
    }
    let layout = spec.shape.layout().map_err(|e| failure(&e.to_string()))?;
    let packed = add(
        multiply(layout.cells, size_of::<u8>() + size_of::<Option<[u8; 3]>>())?,
        multiply(layout.dots, size_of::<Option<SourceToken>>())?,
    )?;
    let text = add(
        limits.max_text_bytes,
        multiply(limits.max_text_spans, size_of::<NativeText>())?,
    )?;
    let each = add(add(packed, text)?, size_of::<ClipFrame>() + 512)?;
    let scratch = add(multiply(packed, 2)?, add(text, 8192)?)?;
    if streaming {
        // ChunkWriter accounts its own bounded raw/compressed scratch and
        // retained frame bytes; only the first frame stays here for boundary.
        return Ok((8192, scratch));
    }
    let bytes = add(multiply(spec.frames, each)?, 8192)?;
    if bytes > limits.max_clip_bytes {
        return Err(AnimationError::Budget(
            "full finite replay clip estimate".into(),
        ));
    }
    Ok((bytes, scratch))
}
/// Native ticket issued from the exact finite schedule; no script can choose a
/// completed sample ID or use the newest same-sized asynchronous result.
pub struct ReplaySample {
    identity: Arc<()>,
    job: u64,
    index: usize,
    pub time: f64,
    pub wall: f64,
    pub delta: f64,
}
impl ReplaySample {
    pub fn index(&self) -> usize {
        self.index
    }
}
/// Packed under the original native drawing admission; only the finite
/// preparation ticket can publish it after acceptance and request checks.
pub struct CapturedSample {
    identity: Arc<()>,
    job: u64,
    index: usize,
    frame: ClipFrame,
}
pub struct ClipPreparation {
    state: Arc<Mutex<CacheState>>,
    ticket_identity: Arc<()>,
    limits: ReplayLimits,
    spec: Arc<ClipSpec>,
    authority: ReplayAuthority,
    authorization: Arc<dyn ReplayAuthorization>,
    stop: StopToken,
    job: u64,
    owner: Arc<dyn ReplayPreparationOwner>,
    frames: Vec<ClipFrame>,
    writer: Option<ClipChunkWriter>,
    store: Option<Arc<ClipChunkStore>>,
    first_frame: Option<ClipFrame>,
    written: usize,
    pending: Option<usize>,
    boundary_verified: bool,
    failed: bool,
    published: bool,
    storage: Arc<StorageAdmission>,
    _scratch: StorageAdmission,
    charged_bytes: usize,
}
impl ClipPreparation {
    fn check(&self) -> Result<()> {
        check_stop(&self.stop)?;
        if self.failed || self.published {
            return Err(failure("preparation closed"));
        }
        self.authorization
            .check(&self.authority, &self.spec.lineage, ReplayAccess::Prepare)?;
        let state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        if !state
            .preparing
            .get(&self.spec.key)
            .is_some_and(|entry| entry.job == self.job && entry.authority == self.authority)
        {
            return Err(failure("stale preparation"));
        }
        Ok(())
    }
    pub fn next_sample(&mut self) -> Result<ReplaySample> {
        self.check()?;
        if self.pending.is_some() || self.written >= self.spec.frames {
            return Err(failure("sample already open or schedule complete"));
        }
        let index = self.written;
        self.pending = Some(index);
        Ok(ReplaySample {
            identity: self.ticket_identity.clone(),
            job: self.job,
            index,
            time: index as f64 / self.spec.fps,
            wall: index as f64 / self.spec.fps,
            delta: if index == 0 { 0.0 } else { 1.0 / self.spec.fps },
        })
    }
    pub fn capture_snapshot(
        &self,
        sample: &ReplaySample,
        snapshot: &Snapshot,
        tone: impl FnMut(f32, usize, usize) -> f32,
        threshold: impl FnMut(f32, usize, usize) -> bool,
    ) -> Result<CapturedSample> {
        self.check()?;
        if !Arc::ptr_eq(&sample.identity, &self.ticket_identity)
            || sample.job != self.job
            || self.pending != Some(sample.index)
            || sample.index != self.written
        {
            return Err(failure("wrong replay sample capture"));
        }
        let frame = pack(
            snapshot,
            &self.spec,
            self.limits,
            tone,
            threshold,
            &self.stop,
        )?;
        Ok(CapturedSample {
            identity: Arc::clone(&sample.identity),
            job: sample.job,
            index: sample.index,
            frame,
        })
    }
    /// Called only after native accept_frame and the no-request invariant. The
    /// file adapter writes outside the native drawing-finish callback.
    pub fn push_captured(&mut self, sample: ReplaySample, captured: CapturedSample) -> Result<()> {
        let result = (|| {
            self.check()?;
            if !Arc::ptr_eq(&sample.identity, &self.ticket_identity)
                || !Arc::ptr_eq(&captured.identity, &self.ticket_identity)
                || sample.job != self.job
                || captured.job != self.job
                || self.pending != Some(sample.index)
                || sample.index != self.written
                || captured.index != self.written
            {
                return Err(failure("wrong replay sample completion"));
            }
            let frame = captured.frame;
            if let Some(writer) = &mut self.writer {
                if self.written == 0 && self.spec.seamless {
                    self.first_frame = Some(frame.clone());
                }
                writer.push_packed(self.spec.shape, &frame.packed, &frame.text)?;
            } else {
                self.frames.push(frame);
            }
            self.written += 1;
            self.pending = None;
            Ok(())
        })();
        if result.is_err() {
            self.fail();
        }
        result
    }
    pub fn push_snapshot(
        &mut self,
        sample: ReplaySample,
        snapshot: &Snapshot,
        tone: impl FnMut(f32, usize, usize) -> f32,
        threshold: impl FnMut(f32, usize, usize) -> bool,
    ) -> Result<()> {
        let captured = self.capture_snapshot(&sample, snapshot, tone, threshold)?;
        self.push_captured(sample, captured)
    }
    /// Render the boundary at exactly duration after all ordinary samples. Native
    /// state-continuity certification AND exact packed/evidence equality required.
    pub fn boundary_sample(&mut self) -> Result<ReplaySample> {
        self.check()?;
        if !self.spec.seamless || self.written != self.spec.frames || self.pending.is_some() {
            return Err(failure("loop boundary not ready"));
        }
        self.pending = Some(self.spec.frames);
        Ok(ReplaySample {
            identity: self.ticket_identity.clone(),
            job: self.job,
            index: self.spec.frames,
            time: self.spec.duration,
            wall: self.spec.duration,
            delta: self.spec.duration - (self.spec.frames - 1) as f64 / self.spec.fps,
        })
    }
    pub fn verify_boundary(
        &mut self,
        sample: ReplaySample,
        snapshot: &Snapshot,
        tone: impl FnMut(f32, usize, usize) -> f32,
        threshold: impl FnMut(f32, usize, usize) -> bool,
    ) -> Result<()> {
        let result = (|| {
            self.check()?;
            if !self.spec.seamless
                || self.written != self.spec.frames
                || self.pending != Some(self.spec.frames)
                || sample.index != self.spec.frames
                || sample.job != self.job
                || !Arc::ptr_eq(&sample.identity, &self.ticket_identity)
            {
                return Err(failure("loop boundary not ready"));
            }
            let frame = pack(
                snapshot,
                &self.spec,
                self.limits,
                tone,
                threshold,
                &self.stop,
            )?;
            let first = self
                .first_frame
                .as_ref()
                .or_else(|| self.frames.first())
                .ok_or_else(|| failure("clip empty"))?;
            if frame.packed.masks != first.packed.masks
                || frame.packed.rgb != first.packed.rgb
                || frame.packed.owners != first.packed.owners
                || frame.text != first.text
            {
                return Err(failure("seamless packed/evidence boundary differs"));
            }
            self.boundary_verified = true;
            self.pending = None;
            Ok(())
        })();
        if result.is_err() {
            self.fail();
        }
        result
    }
    pub fn finish(mut self) -> Result<Arc<ReplayClip>> {
        self.check()?;
        if self.written != self.spec.frames
            || self.pending.is_some()
            || (self.spec.seamless && !self.boundary_verified)
        {
            self.fail();
            return Err(failure("partial or uncertified clip discarded"));
        }
        // This occurs on the existing background owner, never playback or UI.
        self.owner.retire()?;
        if !self.owner.is_retired() {
            self.fail();
            return Err(failure("preparation did not actually retire"));
        }
        self.check()?;
        let disk = if let Some(writer) = self.writer.take() {
            check_stop(&self.stop)?;
            writer.finish()?; // Only a complete atomic index becomes visible.
            let store = self
                .store
                .as_ref()
                .ok_or_else(|| failure("stream store missing"))?;
            let reader = Arc::new(store.open_procedural(&self.spec.key.hex())?);
            if reader.frame_count() != self.spec.frames {
                self.fail();
                return Err(failure("completed stream frame count changed"));
            }
            Some(reader)
        } else {
            None
        };
        self.check()?;
        let clip = Arc::new(ReplayClip {
            spec: self.spec.clone(),
            frames: std::mem::take(&mut self.frames),
            disk,
            charged_bytes: self.charged_bytes,
            _storage: self.storage.clone(),
        });
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("clip cache poisoned"))?;
        while state.clips.len() >= self.limits.max_clips
            || state
                .clips
                .values()
                .try_fold(clip.charged_bytes, |n, entry| {
                    add(n, entry.clip.charged_bytes)
                })?
                > self.limits.max_cache_bytes
        {
            let oldest = state
                .clips
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
                .ok_or_else(|| failure("cache cannot admit completed clip"))?;
            state.clips.remove(&oldest); // External leases keep both frame/evidence charges.
        }
        let used = state.clock;
        state.preparing.remove(&self.spec.key);
        state.clips.insert(
            self.spec.key,
            CacheEntry {
                clip: clip.clone(),
                used,
            },
        );
        self.published = true;
        Ok(clip)
    }
    pub fn cancel(&mut self) {
        self.stop.stop();
        self.fail();
    }
    fn fail(&mut self) {
        self.failed = true;
        self.frames.clear();
        self.first_frame = None;
        self.writer = None; // A partial index is never published.
        self.pending = None;
        self.stop.stop();
        self.owner.cancel();
    }
}
impl Drop for ClipPreparation {
    fn drop(&mut self) {
        if !self.published {
            self.fail();
            if self.owner.is_retired() {
                if let Ok(mut state) = self.state.lock() {
                    if state
                        .preparing
                        .get(&self.spec.key)
                        .is_some_and(|entry| entry.job == self.job)
                    {
                        state.preparing.remove(&self.spec.key);
                    }
                }
            }
        }
    }
}
fn pack(
    snapshot: &Snapshot,
    spec: &ClipSpec,
    limits: ReplayLimits,
    tone: impl FnMut(f32, usize, usize) -> f32,
    threshold: impl FnMut(f32, usize, usize) -> bool,
    stop: &StopToken,
) -> Result<ClipFrame> {
    check_stop(stop)?;
    if snapshot.shape() != spec.shape || snapshot.states().contains(&0) {
        return Err(failure("incomplete or mismatched full viewport"));
    }
    let mut text_bytes = 0usize;
    let mut spans = 0usize;
    for text in snapshot.text() {
        text_bytes = add(text_bytes, text.text.len())?;
        spans = add(spans, 1)?;
    }
    if text_bytes > limits.max_text_bytes || spans > limits.max_text_spans {
        return Err(failure("packed text limits"));
    }
    let packed = snapshot
        .pack(tone, threshold)
        .map_err(|e| failure(&e.to_string()))?;
    for token in packed.owners.iter().flatten() {
        spec.evidence.resolve(*token)?;
    }
    check_stop(stop)?;
    Ok(ClipFrame {
        packed,
        text: snapshot.text().cloned().collect(),
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    Once,
    Repeat,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReceipt {
    pub clip: ClipKey,
    pub frame: usize,
    pub lease_sequence: u64,
    pub authority: ReplayAuthority,
}
struct LeaseState {
    clip: Arc<ReplayClip>,
    index: usize,
    disk_frame: Option<Arc<ProceduralFrame>>,
    receipt: ReplayReceipt,
    authorization: Arc<dyn ReplayAuthorization>,
    lifecycle: Arc<AtomicU64>,
    generation: u64,
    stop: StopToken,
    _storage: StorageAdmission,
}
pub struct PlaybackLease {
    state: Arc<LeaseState>,
}
impl std::fmt::Debug for PlaybackLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaybackLease")
            .field("receipt", &self.state.receipt)
            .finish()
    }
}
impl PlaybackLease {
    fn check(&self, access: ReplayAccess) -> Result<()> {
        check_stop(&self.state.stop)?;
        if self.state.lifecycle.load(Ordering::Acquire) != self.state.generation {
            return Err(failure("stale playback lease"));
        }
        self.state.authorization.check(
            &self.state.receipt.authority,
            &self.state.clip.spec.lineage,
            access,
        )
    }
    pub fn packed(&self) -> Result<&PackedSurface> {
        self.check(ReplayAccess::Playback)?;
        match &self.state.disk_frame {
            Some(frame) => Ok(&frame.packed),
            None => Ok(&self.state.clip.frames[self.state.index].packed),
        }
    }
    pub fn text(&self) -> Result<&[NativeText]> {
        self.check(ReplayAccess::Playback)?;
        match &self.state.disk_frame {
            Some(frame) => Ok(&frame.text),
            None => Ok(&self.state.clip.frames[self.state.index].text),
        }
    }
    pub fn receipt(&self) -> &ReplayReceipt {
        &self.state.receipt
    }
    /// A retained worker snapshot may be painted again. Mint a fresh original
    /// receipt while sharing its admitted decoded frame; no disk I/O occurs on
    /// the terminal owner and each physical flush has a unique history key.
    pub fn fork_for_presentation(&self) -> Result<Self> {
        self.check(ReplayAccess::Playback)?;
        let storage = reserve(&self.state.clip.spec.frozen.quota, 4096)?;
        let mut receipt = self.state.receipt.clone();
        receipt.lease_sequence = native_id(&NEXT_PRESENTATION)?;
        let state = Arc::new(LeaseState {
            clip: Arc::clone(&self.state.clip),
            index: self.state.index,
            disk_frame: self.state.disk_frame.clone(),
            receipt,
            authorization: Arc::clone(&self.state.authorization),
            lifecycle: Arc::clone(&self.state.lifecycle),
            generation: self.state.generation,
            stop: self.state.stop.clone(),
            _storage: storage,
        });
        Ok(Self { state })
    }
    /// Pre-admit and project final compositor masks before any terminal effect.
    /// This produces no history credit. The host commits under its grant fence.
    pub fn prepare_emission(self, kept_masks: &[u8]) -> Result<PendingEmission> {
        self.check(ReplayAccess::Emission)?;
        let packed = self.packed()?;
        if kept_masks.len() != packed.masks.len()
            || kept_masks
                .iter()
                .zip(&packed.masks)
                .any(|(kept, mask)| kept & !mask != 0)
        {
            return Err(failure("emission claims absent clip dots"));
        }
        let quota = &self.state.clip.spec.frozen.quota;
        let storage = reserve(quota, add(multiply(kept_masks.len(), 128)?, 65536)?)?;
        let mut projected: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let width = self.state.clip.spec.shape.cell_width as usize;
        for (cell, kept) in kept_masks.iter().enumerate() {
            for (dy, bits) in BITS.iter().enumerate() {
                for (dx, bit) in bits.iter().enumerate() {
                    if kept & bit == 0 {
                        continue;
                    }
                    let dot = (cell / width * 4 + dy) * (width * 2) + cell % width * 2 + dx;
                    if let Some(token) = packed.owners[dot] {
                        projected.entry(token.evidence_key()).or_default().push(dot);
                    }
                }
            }
        }
        Ok(PendingEmission {
            lease: self.state,
            projected,
            _storage: storage,
            proof: None,
            settled_sources: 0,
            state: EmissionSettlementState::AwaitingProof,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmissionSettlementState {
    AwaitingProof,
    Flushed,
    ValidationFailed,
    HistoryFailed,
    Settled,
}
/// Only the trusted native compositor calls this AFTER actual terminal emission
/// under the ordered authorization fence. The same admitted owner retains the
/// flush proof and settlement cursor across every uncertain terminal outcome.
pub struct PendingEmission {
    lease: Arc<LeaseState>,
    projected: BTreeMap<u64, Vec<usize>>,
    _storage: StorageAdmission,
    proof: Option<ReplayFlushedProof>,
    settled_sources: usize,
    state: EmissionSettlementState,
}
impl PendingEmission {
    pub fn after_host_emission(&mut self, proof: ReplayFlushedProof) -> Result<()> {
        if self.state != EmissionSettlementState::AwaitingProof || self.proof.is_some() {
            return Err(failure("replay flush proof already attached"));
        }
        self.proof = Some(proof);
        let proof = self
            .proof
            .as_ref()
            .ok_or_else(|| failure("replay flush proof custody missing"))?;
        if let Err(error) = self
            .lease
            .authorization
            .validate_flushed(&self.lease.receipt.authority, proof)
        {
            self.state = EmissionSettlementState::ValidationFailed;
            return Err(error);
        }
        self.state = EmissionSettlementState::Flushed;
        Ok(())
    }
    /// On a failed history call this owner keeps the original flush proof and
    /// exact remaining source cursor. Output stays blocked and another settle
    /// call is rejected, because the failed adapter may have partially committed.
    pub fn settle(&mut self) -> Result<()> {
        if self.state != EmissionSettlementState::Flushed || self.proof.is_none() {
            return Err(failure("replay emission is not a validated physical flush"));
        }
        while self.settled_sources < self.projected.len() {
            let Some((&token, dots)) = self.projected.iter().nth(self.settled_sources) else {
                self.state = EmissionSettlementState::HistoryFailed;
                return Err(failure("emitted source cursor missing"));
            };
            let Some(entry) = self.lease.clip.spec.evidence.entries.get(&token) else {
                self.state = EmissionSettlementState::HistoryFailed;
                return Err(failure("emitted evidence missing"));
            };
            if let Err(error) =
                entry
                    .history
                    .emitted(entry.source, &entry.payload, &self.lease.receipt, dots)
            {
                self.state = EmissionSettlementState::HistoryFailed;
                return Err(error);
            }
            self.settled_sources += 1;
        }
        self.state = EmissionSettlementState::Settled;
        Ok(())
    }
}
pub enum Playback {
    Frame(PlaybackLease),
    Ended,
}
pub struct ReplayPlayer {
    clip: Arc<ReplayClip>,
    authority: ReplayAuthority,
    authorization: Arc<dyn ReplayAuthorization>,
    clock: AnimationClock,
    speed: f64,
    paused: bool,
    _storage: StorageAdmission,
    offset: f64,
    mode: PlaybackMode,
    lifecycle: Arc<AtomicU64>,
    leases: Vec<Weak<LeaseState>>,
    max_leases: usize,
    stop: StopToken,
}
/// Native playback configuration; it carries no permission or source authority.
pub struct PlaybackSettings {
    pub now: Duration,
    pub speed: f64,
    pub mode: PlaybackMode,
    pub max_leases: usize,
    pub stop: StopToken,
}
impl ReplayPlayer {
    pub fn new(
        clip: Arc<ReplayClip>,
        authority: ReplayAuthority,
        authorization: Arc<dyn ReplayAuthorization>,
        settings: PlaybackSettings,
    ) -> Result<Self> {
        let PlaybackSettings {
            now,
            speed,
            mode,
            max_leases,
            stop,
        } = settings;
        authority.validate(&clip.spec.package)?;
        authorization.check(&authority, &clip.spec.lineage, ReplayAccess::Playback)?;
        if !(1..=16).contains(&max_leases) {
            return Err(failure("presentation lease slots"));
        }
        check_stop(&stop)?;
        let storage = reserve(&clip.spec.frozen.quota, 8192)?;
        let stop = stop.child();
        let clock = AnimationClock::new(now, speed).map_err(|e| failure(&e.to_string()))?;
        Ok(Self {
            clip,
            authority,
            authorization,
            clock,
            speed,
            paused: false,
            _storage: storage,
            offset: 0.0,
            mode,
            lifecycle: Arc::new(AtomicU64::new(1)),
            leases: Vec::with_capacity(max_leases),
            max_leases,
            stop,
        })
    }
    pub fn sample(&mut self, now: Duration) -> Result<(Playback, ClockSample)> {
        check_stop(&self.stop)?;
        self.authorization.check(
            &self.authority,
            &self.clip.spec.lineage,
            ReplayAccess::Playback,
        )?;
        let mut candidate_clock = self.clock.clone();
        let sample = candidate_clock
            .sample(now)
            .map_err(|e| failure(&e.to_string()))?;
        let position = self.offset + sample.time;
        if !position.is_finite() {
            return Err(failure("playback position overflow"));
        }
        self.clock = candidate_clock;
        if self.mode == PlaybackMode::Once && position >= self.clip.spec.duration {
            return Ok((Playback::Ended, sample));
        }
        let position = if self.mode == PlaybackMode::Repeat {
            position % self.clip.spec.duration
        } else {
            position
        };
        let index =
            ((position * self.clip.spec.fps).floor() as usize).min(self.clip.spec.frames - 1);
        self.leases.retain(|lease| lease.strong_count() > 0);
        if self.leases.len() >= self.max_leases {
            return Err(AnimationError::Budget(
                "active replay presentation leases".into(),
            ));
        }
        let storage = reserve(&self.clip.spec.frozen.quota, 4096)?;
        let disk_frame = match &self.clip.disk {
            Some(disk) => Some(Arc::new(
                disk.procedural_frame(index, self.clip.spec.shape)?,
            )),
            None => None,
        };
        check_stop(&self.stop)?;
        self.authorization.check(
            &self.authority,
            &self.clip.spec.lineage,
            ReplayAccess::Playback,
        )?;
        let sequence = native_id(&NEXT_PRESENTATION)?;
        let state = Arc::new(LeaseState {
            clip: self.clip.clone(),
            index,
            disk_frame,
            receipt: ReplayReceipt {
                clip: self.clip.key(),
                frame: index,
                lease_sequence: sequence,
                authority: self.authority.clone(),
            },
            authorization: self.authorization.clone(),
            lifecycle: self.lifecycle.clone(),
            generation: self.lifecycle.load(Ordering::Acquire),
            stop: self.stop.clone(),
            _storage: storage,
        });
        self.leases.push(Arc::downgrade(&state));
        Ok((Playback::Frame(PlaybackLease { state }), sample))
    }
    pub fn pause(&mut self, now: Duration) -> Result<()> {
        check_stop(&self.stop)?;
        self.clock
            .suspend(now)
            .map_err(|e| failure(&e.to_string()))?;
        self.paused = true;
        Ok(())
    }
    pub fn resume(&mut self, now: Duration) -> Result<()> {
        check_stop(&self.stop)?;
        self.clock
            .resume(now)
            .map_err(|e| failure(&e.to_string()))?;
        self.paused = false;
        Ok(())
    }
    pub fn set_speed(&mut self, now: Duration, speed: f64) -> Result<()> {
        check_stop(&self.stop)?;
        self.clock
            .set_speed(now, speed)
            .map_err(|e| failure(&e.to_string()))?;
        self.speed = speed;
        Ok(())
    }
    pub fn seek(&mut self, now: Duration, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 || seconds > self.clip.spec.duration {
            return Err(failure("seek outside finite clip"));
        }
        check_stop(&self.stop)?;
        let generation = self
            .lifecycle
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or_else(|| failure("playback generation exhausted"))?;
        let mut clock = self.clock.clone();
        clock
            .reset(now, self.speed)
            .map_err(|e| failure(&e.to_string()))?;
        if self.paused {
            clock.suspend(now).map_err(|e| failure(&e.to_string()))?;
        }
        self.clock = clock;
        self.offset = seconds;
        self.lifecycle.store(generation, Ordering::Release);
        Ok(())
    }
    pub fn stop(&mut self) {
        self.stop.stop();
        let _ = self
            .lifecycle
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1));
    }
}
impl Drop for ReplayPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

// These synthetic cache/player checks need access to private, prevalidated
// replay state. They do not mint a production replay certificate. Their
// broker-issued proof follows a test host sink flush, not terminal acceptance;
// protected-source integration remains a separate native adapter gate.
#[cfg(test)]
#[path = "replay_contract_tests.rs"]
mod replay_contract_tests;
