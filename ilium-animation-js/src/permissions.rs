//! Host-owned authorization; this module performs no I/O and exposes no V8 bindings.
//! Call all broker methods through one serialized owner or one externally held mutex.
//! Native adapters derive operation needs from actual arguments, never helper claims.
use serde::{Deserialize, Serialize}; // Keep wire requests separate from authority objects.
use sha2::{Digest, Sha256}; // Bind package bytes, stable requests, and selected resources.
use std::collections::{BTreeMap, BTreeSet}; // Canonical ordering makes scope keys stable.
use std::io::{self, Write}; // Count serialized bytes before retaining input graphs.
use std::net::IpAddr; // Resolved addresses come from the native transport.
use std::sync::Arc; // An unforgeable in-process issuer distinguishes identical brokers.
use url::Url; // Parse origins instead of comparing URL prefixes.
const MAX_ITEMS: usize = 64; // Bound requests, demands, and operation requirements.
const MAX_SCOPE_ITEMS: usize = 16; // Bound each origin, method, product, or kernel set.
const MAX_DECISIONS: usize = 256; // Include session decisions in this retention limit.
const MAX_INSTANCES: usize = 8; // Active instances and pending reviews share this cap.
const MAX_OPERATIONS: usize = 128; // Revoked operations occupy slots until settled.
const MAX_FRAME_EMISSIONS: usize = MAX_OPERATIONS; // Native output commitments retain bounded unsettled slots.
const MAX_WIRE_BYTES: usize = 65_536; // Bound each permission projection before cloning.
const MAX_STATE_BYTES: usize = 262_144; // Bound the complete remembered/session ledger.
#[derive(Debug, thiserror::Error)] // Errors never imply permission or completed effects.
pub enum PermissionError {
    // Keep errors independent of application-specific APIs.
    #[error("invalid permission input: {0}")] // Identify malformed projections precisely.
    Invalid(&'static str), // Only bounded host-authored diagnostic text is used here.
    #[error("permission capacity exhausted")] // Backpressure has no permissive fallback.
    Capacity, // Counts and serialized storage have explicit ceilings.
    #[error("package verification failed")] // A claimed publisher cannot repair a mismatch.
    Integrity, // The expected digest must come from the Rust trust inventory.
    #[error("permission belongs to another broker")] // Reject cross-principal token reuse.
    WrongBroker, // Issuer identity is never deserialized from a script.
    #[error("stale authorization or plan")] // Epochs and plan nonces prevent ABA reuse.
    Stale, // Late reviews and results cannot authorize a replacement plan.
    #[error("request exceeds manifest ceiling: {0}")] // Name the rejected normalized request.
    BeyondManifest(String), // The host must not silently broaden the manifest.
    #[error("a decision is needed for request: {0}")] // There is no preselected allowance.
    DecisionRequired(String), // Optional requests also need an explicit resolution.
    #[error("a current host-selected resource is needed: {0}")] // Paths are not grants.
    SelectionRequired(String), // The native picker/reopener must supply the binding.
    #[error("permission denied")] // Use for undeclared demands and denied actual scopes.
    Denied, // Script-side permission queries are advisory only.
    #[error("unknown operation ticket")] // Ticket integers alone are insufficient.
    UnknownOperation, // Native owners must retain the original ticket.
    #[error("operation already crossed its effect boundary")] // Prevent accidental reissue.
    AlreadyCommitted, // Retries and redirect hops need new operation tickets.
    #[error("operation has not crossed its effect boundary")] // No fabricated completion.
    NotCommitted, // Cache delivery can commit a bounded no-I/O action first.
    #[error("remembered decisions do not match this principal")] // Block identity inheritance.
    WrongPrincipal, // Unsigned replacements cannot inherit grants by package name.
    #[error(transparent)] // Preserve parsing failures without falling back to defaults.
    Json(#[from] serde_json::Error), // Trusted persistence is still validated input.
} // End the standalone error contract.
type Result<T> = std::result::Result<T, PermissionError>; // Use one module error type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Closed capability IDs.
pub enum Capability {
    // Adding a capability requires explicit scope validation below.
    #[serde(rename = "network.http")] // HTTPS authority is distinct from local networking.
    NetworkHttp, // Exact origins and methods only.
    #[serde(rename = "network.local")] // Local-address authority is an additional right.
    NetworkLocal, // Never replaces the corresponding HTTP right.
    #[serde(rename = "disk.read")] // Read/list user-selected resources only.
    DiskRead, // Package assets use a separate baseline API.
    #[serde(rename = "disk.write")] // Create/update authority does not grant deletion.
    DiskWrite, // Destructive disk operations are unsupported by this contract.
    #[serde(rename = "audio.loopback")] // Output capture never implies microphone access.
    AudioLoopback, // Products and the device selector are scoped.
    #[serde(rename = "audio.microphone")] // Require a separate input-capture decision.
    AudioMicrophone, // Native device handles remain broker-owned.
    #[serde(rename = "location.observer")] // Only the configured observer is supported.
    LocationObserver, // No precise-device-location authority is implied.
    #[serde(rename = "input.pointer")] // Animation-local pointer input only.
    InputPointer, // Global input and keyboard capture are unsupported.
    #[serde(rename = "screen.occlusion")] // Geometry snapshots need explicit authority.
    ScreenOcclusion, // Final compositor protection is unconditional.
    #[serde(rename = "device.gpu")] // Permit named bounded host kernels only.
    DeviceGpu, // Raw device/native code access is not a capability.
    #[serde(rename = "state.persist")] // Storage always remains inside this principal.
    StatePersist, // Namespace names never select another plugin's storage.
} // Unknown serialized IDs fail deserialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)] // Sets sort.
#[serde(rename_all = "UPPERCASE")] // Match HTTP's wire method names exactly.
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
} // No implicit wildcard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Exact selection shapes.
#[serde(rename_all = "snake_case")] // JSON uses stable snake_case names.
pub enum Selection {
    File,
    Folder,
} // A file grant cannot become a subtree grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)] // Products.
#[serde(rename_all = "snake_case")] // Keep product IDs distinct from display labels.
pub enum AudioProduct {
    Level,
    Waveform,
    Envelope,
    Bands,
    History,
} // Rates/windows remain resource limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] // Scope is a closed tagged union.
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)] // Reject extra authority.
pub enum Scope {
    // Canonical wire v1; the SDK translates illustrative spec shorthands.
    Network {
        origins: BTreeSet<String>,
        methods: BTreeSet<HttpMethod>,
    }, // Exact cross product.
    Disk {
        slot: String,
        selection: Selection,
    }, // Stable picker slot, never an absolute path.
    Audio {
        device: String,
        products: BTreeSet<AudioProduct>,
    }, // Selector plus products.
    Observer,          // User-configured observer only.
    AnimationViewport, // Pointer or occupancy geometry inside this animation's viewport.
    Gpu {
        kernels: BTreeSet<String>,
    }, // Host policy separately lists implemented kernels.
    Namespace {
        name: String,
    }, // Native storage prefixes this with the principal key.
} // Scope shapes cannot be substituted between capability families.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] // A capability and its ceiling.
#[serde(deny_unknown_fields)] // A publisher or grant field is never accepted here.
pub struct Right {
    pub id: Capability,
    pub scope: Scope,
} // Public values are requests only.
#[derive(Clone, Debug, Serialize, Deserialize)] // A projection of the external manifest.
#[serde(deny_unknown_fields)] // The package loader owns the rest of the manifest schema.
pub struct Ceiling {
    pub permissions: Vec<Right>,
} // Empty means no privileged authority.
#[derive(Clone, Debug, Serialize, Deserialize)] // Untrusted plan request input.
#[serde(deny_unknown_fields)] // No script-created grants, identities, epochs, or bindings.
pub struct PermissionRequest {
    // Required fields have no permissive defaults.
    #[serde(default)] // An omitted identity is replaced by a canonical scope digest.
    pub request_id: Option<String>, // Explicit IDs remain stable through replans.
    pub id: Capability, // Closed capability ID.
    pub scope: Scope,   // Must fit one complete manifest ceiling entry.
    pub required: bool, // Denial blocks activation when true.
    pub reason: String, // Bounded plain explanation shown separately from host authority text.
} // Explanations do not influence authorization.
#[derive(Clone, Debug, Serialize, Deserialize)] // Only privileged demands belong here.
#[serde(deny_unknown_fields)] // Resource budgets are validated by the separate resource planner.
pub struct Demand {
    pub demand_id: String,
    pub request_ids: BTreeSet<String>,
} // All dependencies.
#[derive(Clone, Debug, Serialize, Deserialize)] // The authorization projection of one plan.
#[serde(deny_unknown_fields)] // Unknown fields cannot smuggle accepted-plan state.
pub struct PermissionPlan {
    pub permissions: Vec<PermissionRequest>,
    pub demands: Vec<Demand>,
} // Input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)] // Intentionally not Deserialize.
pub struct PackageIdentity {
    package_id: String,
    content_hash: [u8; 32],
    verified_ilium: bool,
} // Rust-only.
impl PackageIdentity {
    // Verification binds the entire immutable archive and dependency closure.
    pub fn unverified(package_id: String, package_bytes: &[u8]) -> Result<Self> {
        // Safe default.
        valid_id(&package_id)?; // A display name or URL cannot choose a different principal.
        Ok(Self {
            package_id,
            content_hash: Sha256::digest(package_bytes).into(),
            verified_ilium: false,
        }) // Bind bytes.
    } // The installer must pass the same admitted bytes to validation and execution.
      // Unit fixtures construct raw archives; production verification uses only
      // the fully validated Package-bound constructors below.
    #[cfg(test)]
    pub(crate) fn from_ilium_inventory(
        package_id: String,
        package_bytes: &[u8],
        trusted_hash: [u8; 32],
    ) -> Result<Self> {
        // Host trust boundary.
        let mut identity = Self::unverified(package_id, package_bytes)?; // Never trust archive fields.
        if identity.content_hash != trusted_hash {
            return Err(PermissionError::Integrity);
        } // Exact match.
        identity.verified_ilium = true; // Only call with a digest from Rust's authenticated inventory.
        Ok(identity) // Signature verification, when used, feeds that same authenticated inventory.
    } // No equivalent JSON field or JS/native callback is exported.
    /// Only a fully validated immutable native Package can supply this digest.
    pub(crate) fn from_package(package: &crate::package::Package) -> Result<Self> {
        valid_id(&package.manifest().id)?;
        Ok(Self {
            package_id: package.manifest().id.clone(),
            content_hash: package.archive_hash(),
            verified_ilium: false,
        })
    }
    /// The caller is the native verifier; this digest must come from its compiled
    /// exact release inventory, never a script, descriptor or editable setting.
    pub(crate) fn from_package_inventory(
        package: &crate::package::Package,
        inventory_digest: &str,
    ) -> Result<Self> {
        if package.digest() != inventory_digest {
            return Err(PermissionError::Integrity);
        }
        let mut identity = Self::from_package(package)?;
        identity.verified_ilium = true;
        Ok(identity)
    }
    pub fn content_hash(&self) -> [u8; 32] {
        self.content_hash
    } // Cache identity includes these bytes.
    pub fn principal_key(&self) -> String {
        // Remember grants across verified Ilium updates only.
        if self.verified_ilium {
            return format!("ilium:{}", self.package_id);
        } // Verified package lineage.
        format!("unsigned:{}:{}", self.package_id, hex(&self.content_hash)) // Unsigned identity includes bytes.
    } // An unsigned replacement cannot reuse a trusted lineage.
} // The broker retains this immutable identity for its entire lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)] // Binding kinds are never script decoded.
pub enum BindingKind {
    File,
    Folder,
    AudioDevice,
} // Native handles carry actual resolved identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)] // Intentionally not Deserialize.
pub struct HostBinding {
    kind: BindingKind,
    opaque_identity: String,
} // No native paths or handles in JS.
impl HostBinding {
    // Only the trusted picker/device table constructs these values.
    pub(crate) fn new(kind: BindingKind, opaque_identity: String) -> Result<Self> {
        // Rust-only input.
        valid_id(&opaque_identity)?; // Use a stable opaque native-table identity, not a pathname.
        Ok(Self {
            kind,
            opaque_identity,
        }) // Reopening must verify the same file/root/device identity.
    } // A copied handle string never authenticates a new native resource.
    pub fn opaque_identity(&self) -> &str {
        &self.opaque_identity
    } // Lookup stays in the native table.
    fn fingerprint(&self) -> [u8; 32] {
        // Remember identity without resurrecting a handle on load.
        let mut digest = Sha256::new(); // Hash a versioned, typed identity.
        digest.update(b"ilium-binding-v1\0"); // Separate this digest from package/request digests.
        digest.update([match self.kind {
            BindingKind::File => 0,
            BindingKind::Folder => 1,
            BindingKind::AudioDevice => 2,
        }]); // Type fence.
        digest.update(self.opaque_identity.as_bytes()); // Native identity is already bounded.
        digest.finalize().into() // Persistence stores only this fingerprint.
    } // Actual access still needs a newly validated HostBinding.
} // HostBinding is not a filesystem capability by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)] // Decisions are host UI input, never script JSON.
pub enum UserChoice {
    AllowSession,
    AllowRemembered,
    DenySession,
    DenyRemembered,
} // No default choice.
impl UserChoice {
    // Convert explicit UI outcomes to ledger fields.
    fn parts(self) -> (bool, bool) {
        // Return allowance and persistence independently.
        (
            matches!(self, Self::AllowSession | Self::AllowRemembered),
            matches!(self, Self::AllowRemembered | Self::DenyRemembered),
        ) // Preserve denial lifetime.
    } // Installing or selecting a package never calls this implicitly.
} // Explicit revocations use the separate always-remembered revoke method.
#[derive(Clone, Debug, Serialize)] // Normalized, immutable request shown in a review.
pub struct ReviewedRequest {
    pub request_id: String,
    pub right: Right,
    pub required: bool,
    pub reason: String,
} // Host validates all fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)] // Explain why a request is unresolved/denied.
#[serde(rename_all = "snake_case")] // Stable SDK-facing status names.
pub enum Verdict {
    Allowed,
    UserDenied,
    HostDenied,
    Prompt,
    NeedsSelection,
} // No implicit allowance.
#[derive(Clone, Debug, Serialize)] // Read-only review data is safe to display.
pub struct ReviewItem {
    pub request: ReviewedRequest,
    pub verdict: Verdict,
    binding: Option<HostBinding>,
} // Binding is host supplied.
#[derive(Debug)] // Reviews themselves are unforgeable, non-deserializable tokens.
pub struct PlanReview {
    issuer: Arc<()>,
    stamp: Stamp,
    items: Vec<ReviewItem>,
    demands: Vec<Demand>,
} // One pending nonce.
impl PlanReview {
    /// Native correlation only; issuer and nonce remain private in this token.
    pub fn instance_id(&self) -> u64 {
        self.stamp.instance_id
    }
    pub fn plan_revision(&self) -> u64 {
        self.stamp.plan_revision
    }
    pub fn authorization_epoch(&self) -> u64 {
        self.stamp.epoch
    }

    // UI consumes normalized scopes and plain reasons.
    pub fn items(&self) -> &[ReviewItem] {
        &self.items
    } // Mutation cannot change the pending review.
} // Resolve consumes this exact review once.
#[derive(Clone, Debug, Serialize)] // Informational grant returned to the script.
pub struct Grant {
    pub request_id: String,
    pub right: Right,
    pub binding: Option<HostBinding>,
} // Never Deserialize as authority.
#[derive(Clone, Debug, Serialize)] // Accepted permissions and dependent demands only.
pub struct AcceptedPlan {
    pub principal: PackageIdentity,
    pub instance_id: u64,
    pub plan_revision: u64,
    pub authorization_epoch: u64,
    pub grants: BTreeMap<String, Grant>,
    pub demands: BTreeMap<String, Demand>,
} // Read-only copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)] // Nonces prevent instance/revision ABA reuse.
struct Stamp {
    instance_id: u64,
    plan_revision: u64,
    epoch: u64,
    nonce: u64,
} // Host-generated fencing.
#[derive(Clone, Debug)] // A channel is bound in native callback data or authenticated IPC state.
pub struct Channel {
    issuer: Arc<()>,
    stamp: Stamp,
} // Never reconstruct from helper-supplied IDs.
#[derive(Debug)] // The caller receives a channel and an informational plan copy.
pub struct Activation {
    pub channel: Channel,
    pub plan: AcceptedPlan,
} // Broker keeps its own plan.
#[derive(Debug)] // A ticket names one admitted operation and its original channel.
pub struct OperationTicket {
    issuer: Arc<()>,
    operation_id: u64,
    stamp: Stamp,
} // Neither Clone nor Deserialize.
impl OperationTicket {
    // Correlation IDs are observable but confer no authority alone.
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    } // Native owners retain the ticket itself.
} // Helper RPC IDs must map to these host-owned tickets.
/// Private native output commitment. It is issued under the original broker
/// lock and cannot be reconstructed from frame identity coordinates.
#[derive(Debug)]
pub(crate) struct FrameEmissionTicket {
    issuer: Arc<()>,
    emission_id: u64,
    stamp: Stamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameEmissionOutcome {
    BackendFlushed,
    BackendFailureUncertain,
}
struct PendingFrameEmission {
    stamp: Stamp,
}
#[derive(Debug)] // Invalidation does not claim that effects were undone.
pub struct InvalidatedOperation {
    pub operation_id: u64,
    pub may_have_effects: bool,
} // Commit boundary evidence.
#[derive(Debug)] // The parent must cancel/invalidate these native owners and retained data.
#[must_use] // Do not silently discard cancellation work.
pub struct Invalidation {
    pub authorization_epoch: u64,
    pub instance_ids: Vec<u64>,
    pub operations: Vec<InvalidatedOperation>,
    pub all_rights_blocked: bool,
} // Bounded inventory.
#[derive(Debug)] // Decisions may persist even when required denial prevents activation.
#[must_use] // Handle invalidation and activation failure together.
pub struct Resolution {
    pub activation: Option<Activation>,
    pub denied_required: Vec<String>,
    pub invalidation: Invalidation,
} // Atomic policy result.
#[derive(Clone, Debug, Serialize, Deserialize)] // Only trusted host persistence reads this schema.
#[serde(deny_unknown_fields)] // Malformed stored authority fails closed.
struct DecisionRecord {
    right: Right,
    allowed: bool,
    remembered: bool,
    binding_hash: Option<[u8; 32]>,
    approved_hash: [u8; 32],
} // Approval lineage.
#[derive(Serialize, Deserialize)] // This is not a script-facing API.
#[serde(deny_unknown_fields)] // Reject unsupported persistence schemas.
struct SavedDecisions {
    version: u32,
    principal_key: String,
    epoch: u64,
    block_all: bool,
    records: Vec<DecisionRecord>,
} // Host-owned file payload.
#[derive(Clone, Debug, Serialize)] // Native services derive these from actual operations.
pub struct OperationNeed {
    right: Right,
    binding: Option<HostBinding>,
} // Intentionally not Deserialize.
impl OperationNeed {
    // Generic needs cover disk/device/provider/cache adapters.
    pub fn new(right: Right, binding: Option<HostBinding>) -> Result<Self> {
        // Native adapter boundary.
        let right = right.normalized()?; // Validate the actual scope even for verified packages.
        check_binding(&right, binding.as_ref(), true)?; // Handle-bearing operations need exact identity.
        Ok(Self { right, binding }) // Adapters cannot substitute script-declared requirements.
    } // Resource budgets and target-safe I/O remain independently mandatory.
    /// Exact origin/method before DNS. This is a need, never DNS or a grant.
    pub(crate) fn http_preflight(url: &str, method: HttpMethod) -> Result<Self> {
        let parsed = checked_url(url)?;
        Self::new(
            Right {
                id: Capability::NetworkHttp,
                scope: Scope::Network {
                    origins: BTreeSet::from([parsed.origin().ascii_serialization()]),
                    methods: BTreeSet::from([method]),
                },
            },
            None,
        )
    }
    pub fn http_hop(
        url: &str,
        method: HttpMethod,
        resolved_addresses: &[IpAddr],
    ) -> Result<Vec<Self>> {
        // Each redirect is a new hop.
        if resolved_addresses.is_empty() || resolved_addresses.len() > MAX_ITEMS {
            return Err(PermissionError::Invalid("resolved addresses"));
        } // No unchecked DNS.
        let parsed = checked_url(url)?; // Reject credentials, fragments, and non-HTTPS schemes.
        let scope = Scope::Network {
            origins: BTreeSet::from([parsed.origin().ascii_serialization()]),
            methods: BTreeSet::from([method]),
        }; // Exact origin/port.
        let mut needs = vec![Self::new(
            Right {
                id: Capability::NetworkHttp,
                scope: scope.clone(),
            },
            None,
        )?]; // HTTP is always required.
        if resolved_addresses
            .iter()
            .any(|address| !public_address(*address))
        {
            // Conservatively classify special addresses.
            needs.push(Self::new(
                Right {
                    id: Capability::NetworkLocal,
                    scope,
                },
                None,
            )?); // Add rather than replace authority.
        } // The transport must connect only to this checked address set, with original TLS/SNI.
        Ok(needs) // Re-resolution, proxy routing, and redirects require another native check.
    } // The helper cannot supply a trusted resolution or a public-address boolean.
} // Network-capable native decoders/providers use this same boundary.
#[derive(Clone, Copy, Debug)] // The embedder supplies phase from native state.
pub enum CallPhase {
    Plan,
    Create,
    Async,
    Render,
    Dispose,
} // Never decode this from an RPC body.
struct ActivePlan {
    channel: Channel,
    plan: AcceptedPlan,
} // The authoritative accepted copy.
struct InstanceSlot {
    last_revision: u64,
    pending_nonce: u64,
    active: Option<ActivePlan>,
} // Bounded instances.
struct PendingOperation {
    channel: Channel,
    demand_id: String,
    needs: Vec<OperationNeed>,
    committed: bool,
} // Kept through settlement.
pub struct PermissionBroker {
    // One principal's serialized authority owner, independent of V8.
    identity: PackageIdentity,
    issuer: Arc<()>,
    manifest: Vec<Right>,
    policy: Vec<Right>, // Immutable trust; mutable host policy below.
    epoch: u64,
    next_id: u64,
    decisions: Vec<DecisionRecord>,
    block_all: bool, // Explicit denial survives automatic grant policy.
    instances: BTreeMap<u64, InstanceSlot>,
    operations: BTreeMap<u64, PendingOperation>, // No unbounded queues.
    frame_emissions: BTreeMap<u64, PendingFrameEmission>, // Held through actual backend settlement.
    _authority_storage: Option<ilium_execution::StorageAdmission>, // Last: retained through final shared native owner.
} // Native resource owners retain their own quota guards outside this module.
impl PermissionBroker {
    /// Original runtime admission follows authority retained by native sources.
    /// Install ONCE while still exclusive, after consuming remembered restore.
    pub(crate) fn retain_authority_storage(
        &mut self,
        storage: ilium_execution::StorageAdmission,
    ) -> Result<()> {
        if self._authority_storage.is_some() {
            return Err(PermissionError::Invalid(
                "authority admission already installed",
            ));
        }
        self._authority_storage = Some(storage);
        Ok(())
    }

    /// Advisory native correlation, never an authority constructor.
    pub fn authorization_epoch(&self) -> u64 {
        self.epoch
    }

    /// Genuine current native pending slot; these fields are correlation only.
    pub(crate) fn pending_review_coordinates(&self, instance_id: u64) -> Result<(u64, u64)> {
        let slot = self
            .instances
            .get(&instance_id)
            .ok_or(PermissionError::Invalid("missing native review instance"))?;
        if slot.pending_nonce == 0 || slot.active.is_some() {
            return Err(PermissionError::Invalid("no pending native review"));
        }
        Ok((slot.last_revision, self.epoch))
    }

    pub fn identity(&self) -> &PackageIdentity {
        &self.identity
    }
    // All mutations are serialized with dispatch and delivery.
    pub fn new(identity: PackageIdentity, manifest: Ceiling, host_policy: Ceiling) -> Result<Self> {
        // No implicit supported-right list.
        Ok(Self {
            identity,
            issuer: Arc::new(()),
            manifest: validate_ceiling(manifest)?,
            policy: validate_ceiling(host_policy)?,
            epoch: 1,
            next_id: 1,
            decisions: Vec::new(),
            block_all: false,
            instances: BTreeMap::new(),
            operations: BTreeMap::new(),
            frame_emissions: BTreeMap::new(),
            _authority_storage: None,
        }) // Empty policy denies everything.
    } // Initialization creates no resources, subprocesses, or subscriptions.
    pub fn parse_plan(bytes: &[u8]) -> Result<PermissionPlan> {
        // Decode a bounded authorization projection.
        if bytes.len() > MAX_WIRE_BYTES {
            return Err(PermissionError::Capacity);
        } // Bound before JSON allocation.
        Ok(serde_json::from_slice(bytes)?) // Unknown capabilities and fields are errors.
    } // Runtime bindings must not expose persistence/identity constructors alongside this parser.
    pub(crate) fn restore_remembered(mut self, bytes: &[u8]) -> Result<Self> {
        // Consuming bootstrap-only restore.
        if !self.instances.is_empty() || !self.operations.is_empty() || !self.decisions.is_empty() {
            return Err(PermissionError::Invalid("restore after use"));
        } // No live rollback.
        if bytes.len() > MAX_STATE_BYTES {
            return Err(PermissionError::Capacity);
        } // No unbounded stored ledger.
        let saved: SavedDecisions = serde_json::from_slice(bytes)?; // Parse failure consumes this broker.
        if saved.version != 1 || saved.principal_key != self.identity.principal_key() {
            return Err(PermissionError::WrongPrincipal);
        } // Exact lineage.
        if saved.epoch == 0 || saved.records.len() > MAX_DECISIONS {
            return Err(PermissionError::Invalid("stored epoch or count"));
        } // Reject malformed state.
        let mut records: Vec<DecisionRecord> = Vec::new(); // Validate fully before publication.
        for mut record in saved.records {
            // Previous scopes may exceed a narrowed current manifest.
            record.right = record.right.normalized()?; // Stored scopes use the same validation as plans.
            if !record.remembered
                || (!record.allowed && record.binding_hash.is_some())
                || (record.allowed && record.right.needs_binding() != record.binding_hash.is_some())
            {
                return Err(PermissionError::Invalid("stored decision binding"));
            } // Never restore a native handle.
            if !self.identity.verified_ilium && record.approved_hash != self.identity.content_hash {
                return Err(PermissionError::WrongPrincipal);
            } // Unsigned content fence.
            if records
                .iter()
                .any(|previous| previous.right == record.right)
            {
                return Err(PermissionError::Invalid("duplicate stored scope"));
            } // No order-dependent conflicts.
            records.push(record); // Only normalized bounded records survive.
        } // Bindings must be revalidated by native code when a plan is reviewed.
        check_size(&records, MAX_STATE_BYTES.saturating_sub(1024))?; // Leave room for schema/identity fields.
        self.epoch = saved
            .epoch
            .checked_add(1)
            .ok_or(PermissionError::Capacity)?; // Advance across restart.
        self.block_all = saved.block_all; // Capacity-failure denial also survives restart.
        self.decisions = records; // Commit the validated remembered state atomically.
        Ok(self) // The caller cannot retain a permissive broker after failed restore.
    } // The host must never replace restore failure with an empty/default ledger.
    pub fn export_remembered(&self) -> Result<Vec<u8>> {
        // The host owns durable atomic publication.
        let saved = SavedDecisions {
            version: 1,
            principal_key: self.identity.principal_key(),
            epoch: self.epoch,
            block_all: self.block_all,
            records: self
                .decisions
                .iter()
                .filter(|record| record.remembered)
                .cloned()
                .collect(),
        }; // Session choices are excluded.
        check_size(&saved, MAX_STATE_BYTES)?; // Bound the eventual durable payload.
        Ok(serde_json::to_vec(&saved)?) // A returned buffer is not a durable-save receipt.
    } // Report remembered success only after the parent's persistence acknowledgement.
    pub fn prepare(
        &mut self,
        instance_id: u64,
        plan_revision: u64,
        plan: PermissionPlan,
        bindings: BTreeMap<String, HostBinding>,
    ) -> Result<PlanReview> {
        // Acquisition-free review.
        check_size(&plan, MAX_WIRE_BYTES)?; // Also bound manually constructed Rust inputs.
        check_size(&bindings, MAX_WIRE_BYTES)?; // Selected-resource descriptors are bounded too.
        if instance_id == 0
            || plan_revision == 0
            || plan.permissions.len() > MAX_ITEMS
            || plan.demands.len() > MAX_ITEMS
            || bindings.len() > MAX_ITEMS
        {
            return Err(PermissionError::Invalid("plan identity or count"));
        } // No zero or oversized identity.
        if let Some(slot) = self.instances.get(&instance_id) {
            // Existing active work can continue during review.
            if plan_revision <= slot.last_revision {
                return Err(PermissionError::Stale);
            } // Even failed reviews consume revisions.
        } else if self.instances.len() >= MAX_INSTANCES {
            return Err(PermissionError::Capacity);
        } // Pending reviews count too.
        let mut items = Vec::new(); // No accepted authority is created while validating.
        let mut ids = BTreeSet::new(); // Dependencies resolve only against unique stable IDs.
        for input in plan.permissions {
            // Each request must independently fit a ceiling entry.
            let right = Right {
                id: input.id,
                scope: input.scope,
            }
            .normalized()?; // Canonicalize scopes before keying.
            valid_reason(&input.reason)?; // Reject control/escape/bidi spoofing in explanations.
            let request_id = match input.request_id {
                Some(id) => {
                    valid_id(&id)?;
                    id
                }
                None => canonical_id(&right)?,
            }; // Stable omission fallback.
            if !ids.insert(request_id.clone()) {
                return Err(PermissionError::Invalid("duplicate request_id"));
            } // No ambiguous decisions.
            if items
                .iter()
                .any(|previous: &ReviewItem| previous.request.right == right)
            {
                return Err(PermissionError::Invalid("duplicate permission scope"));
            } // Prevent contradictory choices for aliases of the same right.
            if !self.manifest.iter().any(|ceiling| ceiling.covers(&right)) {
                return Err(PermissionError::BeyondManifest(request_id));
            } // Fail before prompting.
            let binding = bindings.get(&request_id).cloned(); // Only native picker/table values enter here.
            check_binding(&right, binding.as_ref(), false)?; // Missing selections remain explicit.
            let mut verdict = self.verdict(&right, binding.as_ref()); // Manifest, policy, grants, and revocations intersect.
            if verdict == Verdict::Allowed && right.needs_binding() && binding.is_none() {
                verdict = Verdict::NeedsSelection;
            } // Automatic rights cannot invent files/devices.
            items.push(ReviewItem {
                request: ReviewedRequest {
                    request_id,
                    right,
                    required: input.required,
                    reason: input.reason,
                },
                verdict,
                binding,
            }); // Seal normalized input.
        } // An absent permission remains absent even for verified Ilium packages.
        if bindings.keys().any(|id| !ids.contains(id)) {
            return Err(PermissionError::Invalid("binding for unknown request"));
        } // No hidden grant injection.
        let mut demand_ids = BTreeSet::new(); // Demand IDs cannot alias one another.
        for demand in &plan.demands {
            // Every privileged demand declares all request dependencies.
            valid_id(&demand.demand_id)?; // Bound identifiers before storing them.
            if !demand_ids.insert(demand.demand_id.clone())
                || demand.request_ids.is_empty()
                || demand.request_ids.len() > MAX_ITEMS
                || !demand.request_ids.is_subset(&ids)
            {
                return Err(PermissionError::Invalid("demand dependencies"));
            } // Reject orphaned or empty dependencies.
        } // Baseline drawing/time/bundle-only work uses separate nonprivileged APIs.
        let nonce = self.allocate_id()?; // A newer review invalidates any older answer for this instance.
        let slot = self.instances.entry(instance_id).or_insert(InstanceSlot {
            last_revision: 0,
            pending_nonce: 0,
            active: None,
        }); // No V8 ownership here.
        slot.last_revision = plan_revision; // Never let delayed UI answers select an earlier plan.
        slot.pending_nonce = nonce; // Keep one pending review per instance.
        Ok(PlanReview {
            issuer: Arc::clone(&self.issuer),
            stamp: Stamp {
                instance_id,
                plan_revision,
                epoch: self.epoch,
                nonce,
            },
            items,
            demands: plan.demands,
        }) // No acquisition yet.
    } // The picker may re-review a selected binding using a fresh plan revision.
    pub fn resolve(
        &mut self,
        review: PlanReview,
        answers: BTreeMap<String, UserChoice>,
    ) -> Result<Resolution> {
        // Exact host UI decision boundary.
        self.check_review(&review)?; // Reject stale epoch, principal, instance, nonce, or revision.
        if answers.keys().any(|id| {
            !review.items.iter().any(|item| {
                item.request.request_id == *id
                    && matches!(item.verdict, Verdict::Prompt | Verdict::NeedsSelection)
            })
        }) {
            return Err(PermissionError::Invalid("answer outside pending prompts"));
        } // Cannot override a remembered denial here.
        let mut records = self.decisions.clone(); // Validate the entire decision transaction first.
        for item in &review.items {
            // Resolve each right independently.
            let request = &item.request; // The review owns the scope the user actually saw.
            match item.verdict {
                // Automatic/remembered results cannot be script overridden.
                Verdict::Allowed | Verdict::UserDenied | Verdict::HostDenied => {} // Preserve resolved choices.
                Verdict::Prompt | Verdict::NeedsSelection => {
                    // A canceled optional selection can be explicitly denied.
                    let choice = answers.get(&request.request_id).ok_or_else(|| {
                        if item.verdict == Verdict::NeedsSelection {
                            PermissionError::SelectionRequired(request.request_id.clone())
                        } else {
                            PermissionError::DecisionRequired(request.request_id.clone())
                        }
                    })?; // No default choice.
                    let (allowed, remembered) = choice.parts(); // Session lifetime is distinct from allowance.
                    if allowed && item.verdict == Verdict::NeedsSelection {
                        return Err(PermissionError::SelectionRequired(
                            request.request_id.clone(),
                        ));
                    } // A UI answer cannot invent a handle.
                    if allowed {
                        check_binding(&request.right, item.binding.as_ref(), true)?;
                    } // Exact selection before grant publication.
                    upsert(
                        &mut records,
                        DecisionRecord {
                            right: request.right.clone(),
                            allowed,
                            remembered,
                            binding_hash: if allowed {
                                item.binding.as_ref().map(HostBinding::fingerprint)
                            } else {
                                None
                            },
                            approved_hash: self.identity.content_hash,
                        },
                    )?; // Record approved content.
                } // Prompt decisions remain bound to the reviewed nonce.
            } // No grant originates from the returned JS accepted-plan object.
        } // Decisions persist even when another required right is denied.
        check_size(&records, MAX_STATE_BYTES.saturating_sub(1024))?; // No partially published oversized ledger.
        let mut grants = BTreeMap::new(); // Resolve the complete ledger so overlapping denials win in any order.
        let mut denied_required = Vec::new(); // Required rights use the final effective decision set.
        for item in &review.items {
            // A broad allowed request can contain a separately denied scope.
            let request = &item.request; // Keep the exact reviewed request identity.
            let allowed = self.verdict_using(&request.right, item.binding.as_ref(), &records)
                == Verdict::Allowed; // Final intersection.
            if allowed {
                grants.insert(
                    request.request_id.clone(),
                    Grant {
                        request_id: request.request_id.clone(),
                        right: request.right.clone(),
                        binding: item.binding.clone(),
                    },
                );
            } // Only final grants survive.
            if !allowed && request.required {
                denied_required.push(request.request_id.clone());
            } // Prevent false required activation.
        } // Optional demands are pruned using this recomputed grant set below.
        let invalidation = if answers.is_empty() {
            self.invalidate_instance(review.stamp.instance_id)
        } else {
            // Replan versus authority change.
            let invalidation = self.invalidate_all()?; // Epoch changes invalidate every consumer of this principal.
            self.decisions = records; // Commit all decisions under the same serialized boundary.
            invalidation // Native owners receive the complete cancellation inventory.
        }; // Outstanding operation records are deliberately retained for settlement.
        if !denied_required.is_empty() {
            return Ok(Resolution {
                activation: None,
                denied_required,
                invalidation,
            });
        } // No half-active required plan.
        let demands = review
            .demands
            .into_iter()
            .filter(|demand| demand.request_ids.iter().all(|id| grants.contains_key(id)))
            .map(|demand| (demand.demand_id.clone(), demand))
            .collect(); // Prune optional dependents atomically.
        let stamp = Stamp {
            epoch: self.epoch,
            ..review.stamp
        }; // The new plan uses the committed grant epoch.
        let channel = Channel {
            issuer: Arc::clone(&self.issuer),
            stamp,
        }; // Authority remains in native state.
        let plan = AcceptedPlan {
            principal: self.identity.clone(),
            instance_id: stamp.instance_id,
            plan_revision: stamp.plan_revision,
            authorization_epoch: stamp.epoch,
            grants,
            demands,
        }; // Script gets a snapshot only.
        let slot = self
            .instances
            .get_mut(&stamp.instance_id)
            .ok_or(PermissionError::Stale)?; // The validated instance still exists.
        slot.pending_nonce = 0; // Consume the exact review and prevent replay.
        slot.active = Some(ActivePlan {
            channel: channel.clone(),
            plan: plan.clone(),
        }); // Atomic accepted-plan publication.
        Ok(Resolution {
            activation: Some(Activation { channel, plan }),
            denied_required,
            invalidation,
        }) // Activation is separate from acquisition.
    } // The caller starts only the demands in this accepted plan.
    pub fn grant(&self, channel: &Channel, request_id: &str) -> Result<Option<&Grant>> {
        // Advisory inspection.
        Ok(self.active(channel)?.plan.grants.get(request_id)) // Exact request IDs avoid ambiguous capability lookup.
    } // Dispatch independently rechecks actual scope and every dependency.
    pub fn dispatch(
        &mut self,
        channel: &Channel,
        phase: CallPhase,
        demand_id: &str,
        needs: Vec<OperationNeed>,
    ) -> Result<OperationTicket> {
        // Reserve authorization metadata.
        if !matches!(phase, CallPhase::Create | CallPhase::Async) {
            return Err(PermissionError::Denied);
        } // Rendering/planning cannot acquire resources.
        check_size(&needs, MAX_WIRE_BYTES)?; // Bound captured requirements before retention.
        self.check_needs(channel, demand_id, &needs)?; // No default-allow for empty or missing requirements.
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(PermissionError::Capacity);
        } // Revocation never releases unsettled slots.
        let operation_id = self.allocate_id()?; // Never recycle an operation identity within this broker.
        self.operations.insert(
            operation_id,
            PendingOperation {
                channel: channel.clone(),
                demand_id: demand_id.to_owned(),
                needs,
                committed: false,
            },
        ); // Keep exact lineage.
        Ok(OperationTicket {
            issuer: Arc::clone(&self.issuer),
            operation_id,
            stamp: channel.stamp,
        }) // Queue admission is not an issued effect.
    } // The actual native action must still call commit at its effect boundary.
    /// Pure SAME original operation/current channel check, including queued
    /// metadata. No new ticket/grant/effect or changes to dependency IDs.
    /// Read-only native correlation after PRIVATE current-channel authentication.
    /// These coordinates never construct a Channel/Activation/OperationTicket.
    pub(crate) fn channel_coordinates(&self, channel: &Channel) -> Result<(u64, u64, u64)> {
        let active = self.active(channel)?;
        Ok((
            active.plan.instance_id,
            active.plan.plan_revision,
            active.plan.authorization_epoch,
        ))
    }
    /// Commit a trusted host frame to the terminal owner before its first
    /// backend call. This operation uses the current private channel, not a
    /// guest grant. Revocation after this point blocks later commitments but
    /// cannot undo this already admitted in-flight effect.
    pub(crate) fn begin_frame_emission(
        &mut self,
        channel: &Channel,
    ) -> Result<FrameEmissionTicket> {
        self.active(channel)?;
        if self.frame_emissions.len() >= MAX_FRAME_EMISSIONS {
            return Err(PermissionError::Capacity);
        }
        let emission_id = self.allocate_id()?;
        self.frame_emissions.insert(
            emission_id,
            PendingFrameEmission {
                stamp: channel.stamp,
            },
        );
        Ok(FrameEmissionTicket {
            issuer: Arc::clone(&self.issuer),
            emission_id,
            stamp: channel.stamp,
        })
    }
    /// Settle only the original committed operation. A stale channel is allowed
    /// here because the write may have completed after revocation. The outcome
    /// is an output fact, not permission for another operation or source credit.
    pub(crate) fn settle_frame_emission(
        &mut self,
        ticket: &FrameEmissionTicket,
        _outcome: FrameEmissionOutcome,
    ) -> Result<()> {
        if !Arc::ptr_eq(&ticket.issuer, &self.issuer) {
            return Err(PermissionError::WrongBroker);
        }
        let pending = self
            .frame_emissions
            .get(&ticket.emission_id)
            .ok_or(PermissionError::UnknownOperation)?;
        if pending.stamp != ticket.stamp {
            return Err(PermissionError::Stale);
        }
        self.frame_emissions.remove(&ticket.emission_id);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn pending_frame_emissions(&self) -> usize {
        self.frame_emissions.len()
    }
    pub(crate) fn check_operation_lineage(
        &self,
        ticket: &OperationTicket,
        channel: &Channel,
    ) -> Result<()> {
        let operation = self.operation(ticket)?;
        if !Arc::ptr_eq(&channel.issuer, &self.issuer) {
            return Err(PermissionError::WrongBroker);
        }
        if operation.channel.stamp != channel.stamp {
            return Err(PermissionError::Stale);
        }
        self.check_needs(channel, &operation.demand_id, &operation.needs)
    }
    pub(crate) fn check_committed_operation(
        &self,
        ticket: &OperationTicket,
        channel: &Channel,
    ) -> Result<()> {
        self.check_operation_lineage(ticket, channel)?;
        if !self.operation(ticket)?.committed {
            return Err(PermissionError::NotCommitted);
        }
        Ok(())
    }
    /// Return the actual accepted grants that covered this already committed
    /// operation. The caller can record lineage; the returned data is not an
    /// operation ticket and cannot authorize a later effect by itself.
    pub(crate) fn committed_operation_grants(
        &self,
        ticket: &OperationTicket,
        channel: &Channel,
    ) -> Result<Vec<Grant>> {
        self.check_committed_operation(ticket, channel)?;
        let operation = self.operation(ticket)?;
        let active = self.active(channel)?;
        let demand = active
            .plan
            .demands
            .get(&operation.demand_id)
            .ok_or(PermissionError::Denied)?;
        let mut chosen = Vec::new();
        let mut seen = BTreeSet::new();
        for need in &operation.needs {
            let grant = demand
                .request_ids
                .iter()
                .filter_map(|id| active.plan.grants.get(id))
                .find(|grant| {
                    grant.right.covers(&need.right)
                        && grant.binding == need.binding
                        && self.verdict(&need.right, need.binding.as_ref()) == Verdict::Allowed
                })
                .ok_or(PermissionError::Denied)?;
            if seen.insert(grant.request_id.clone()) {
                chosen.push(grant.clone());
            }
        }
        Ok(chosen)
    }
    /// Replay checks the live original broker and policy for every lineage
    /// member at prepare, delivery, playback and emitted-pixel boundaries.
    pub(crate) fn current_grant(&self, channel: &Channel, request_id: &str) -> Result<Grant> {
        let grant = self
            .active(channel)?
            .plan
            .grants
            .get(request_id)
            .ok_or(PermissionError::Denied)?;
        if self.verdict(&grant.right, grant.binding.as_ref()) != Verdict::Allowed {
            return Err(PermissionError::Denied);
        }
        Ok(grant.clone())
    }
    pub(crate) fn check_committed_needs(
        &self,
        ticket: &OperationTicket,
        channel: &Channel,
        needs: &[OperationNeed],
    ) -> Result<()> {
        self.check_committed_operation(ticket, channel)?;
        check_size(&needs, MAX_WIRE_BYTES)?;
        let operation = self.operation(ticket)?;
        self.check_needs(channel, &operation.demand_id, needs)
    }
    pub fn commit<T>(&mut self, ticket: &OperationTicket, issue: impl FnOnce() -> T) -> Result<T> {
        // Bounded synchronous native issue only.
        let operation = self.operation(ticket)?; // Retain both ticket and native quota custody.
        if operation.committed {
            return Err(PermissionError::AlreadyCommitted);
        } // No duplicate side effect.
        self.check_needs(&operation.channel, &operation.demand_id, &operation.needs)?; // Recheck after queueing/revocation races.
        self.operations
            .get_mut(&ticket.operation_id)
            .ok_or(PermissionError::UnknownOperation)?
            .committed = true; // Mark before any possibly partial effect.
        Ok(issue()) // Hold the external broker lock through this bounded issue; never wait for I/O completion here.
    } // Actual completion and uncertain outcome belong to the native operation receipt.
    /// Atomically commit one bounded native effect which depends on several
    /// independently dispatched original requests. Each ticket keeps its own
    /// accepted demand and exact scoped needs; no union creates a new grant.
    pub fn commit_group<T>(
        &mut self,
        tickets: &[&OperationTicket],
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        let ids = self.check_operation_group(tickets, false)?;
        // All fallible validation finished before changing any ticket. The
        // exclusive broker owner prevents operation-map mutation in this loop.
        for (id, operation) in &mut self.operations {
            if ids.contains(id) {
                operation.committed = true;
            }
        }
        Ok(issue())
    }
    /// Publish one result only while every original dependency remains current.
    /// Refusal retains every ticket for the native completion/cleanup owner.
    pub fn deliver_group<T>(
        &mut self,
        tickets: &[&OperationTicket],
        publish: impl FnOnce() -> T,
    ) -> Result<T> {
        let ids = self.check_operation_group(tickets, true)?;
        for id in ids {
            self.operations.remove(&id);
        }
        Ok(publish())
    }
    /// Actual native failure/cancellation has completed. Authenticate the whole
    /// original same-activation group before removing any metadata; stale grants
    /// can settle, but foreign/duplicate tickets cannot partially release a group.
    pub fn settle_group_without_delivery(&mut self, tickets: &[&OperationTicket]) -> Result<()> {
        if tickets.is_empty() || tickets.len() > 8 {
            return Err(PermissionError::Invalid(
                "native operation settlement count",
            ));
        }
        let stamp = tickets[0].stamp;
        let mut ids = BTreeSet::new();
        for ticket in tickets {
            if ticket.stamp != stamp || !ids.insert(ticket.operation_id) {
                return Err(PermissionError::Invalid(
                    "native operation settlement identity",
                ));
            }
            self.operation(ticket)?;
        }
        for id in ids {
            self.operations.remove(&id);
        }
        Ok(())
    }
    fn check_operation_group(
        &self,
        tickets: &[&OperationTicket],
        committed: bool,
    ) -> Result<BTreeSet<u64>> {
        if tickets.is_empty() || tickets.len() > 8 {
            return Err(PermissionError::Invalid(
                "native operation dependency count",
            ));
        }
        let mut ids = BTreeSet::new();
        let stamp = tickets[0].stamp;
        for ticket in tickets {
            if ticket.stamp != stamp || !ids.insert(ticket.operation_id) {
                return Err(PermissionError::Invalid(
                    "native operation dependency identity",
                ));
            }
            let operation = self.operation(ticket)?;
            if operation.committed != committed {
                return Err(if committed {
                    PermissionError::NotCommitted
                } else {
                    PermissionError::AlreadyCommitted
                });
            }
            self.check_needs(&operation.channel, &operation.demand_id, &operation.needs)?;
        }
        Ok(ids)
    }
    pub fn deliver<T>(
        &mut self,
        ticket: &OperationTicket,
        publish: impl FnOnce() -> T,
    ) -> Result<T> {
        // Call at the final protected-data handoff.
        let operation = self.operation(ticket)?; // Another principal's result cannot borrow this channel.
        if !operation.committed {
            return Err(PermissionError::NotCommitted);
        } // No invented issued operation.
        self.check_needs(&operation.channel, &operation.demand_id, &operation.needs)?; // Current grants, epoch, and plan are authoritative.
        self.operations.remove(&ticket.operation_id); // Settle authorization metadata only on successful handoff.
        Ok(publish()) // Bounded copy/resolve on the V8 owner; no JS execution or microtask checkpoint under this lock.
    } // If this only queues data, queue consumption needs a fresh authorization ticket/check.
    pub fn settle_without_delivery(&mut self, ticket: &OperationTicket) -> Result<()> {
        // Actual canceled/failed work has now completed.
        self.operation(ticket)?; // Validate issuer and original stamp, but allow stale authorization.
        self.operations.remove(&ticket.operation_id); // Releasing metadata does not release external native quotas.
        Ok(()) // A revoked result can always be cleaned up without delivering protected data.
    } // Calling this on mere cancellation request would violate native custody.
    pub fn revoke(&mut self, right: Right) -> Result<Invalidation> {
        // Explicit revocation is always remembered.
        let right = right.normalized()?; // Reject malformed UI/native inputs without partial mutation.
        let mut records = self.decisions.clone(); // Preserve other remembered scopes.
        let record = DecisionRecord {
            right,
            allowed: false,
            remembered: true,
            binding_hash: None,
            approved_hash: self.identity.content_hash,
        }; // Denial beats auto-grants.
        let fits = upsert(&mut records, record).is_ok()
            && check_size(&records, MAX_STATE_BYTES.saturating_sub(1024)).is_ok(); // Bound persistent revocation storage.
        let mut invalidation = self.invalidate_all()?; // Immediately fence dispatch, delivery, and old reviews.
        if fits {
            self.decisions = records;
        } else {
            self.block_all = true;
        } // Saturated denial storage fails closed for the whole principal.
        invalidation.all_rights_blocked = self.block_all; // Surface the conservative fallback to the Permissions UI.
        Ok(invalidation) // Keep issued operation records until actual native settlement.
    } // Export and durably save the resulting ledger before reporting remembered success.
    pub fn restore_scope(&mut self, right: Right) -> Result<Invalidation> {
        // Explicit host Permissions-screen action only.
        let right = right.normalized()?; // Restore only the exact stored scope, never overlapping broader scopes.
        let invalidation = self.invalidate_all()?; // Previously issued handles cannot resurrect automatically.
        self.decisions.retain(|record| record.right != right); // Trusted origin may now use initial automatic policy again.
        Ok(invalidation) // An overlapping broader revocation remains in force.
    } // Third-party packages must obtain a fresh decision after restoration.
    pub fn reset_all_decisions(&mut self) -> Result<Invalidation> {
        // Explicit host-owned reset/review action.
        let mut invalidation = self.invalidate_all()?; // Revoke derived channels before removing remembered choices.
        self.decisions.clear(); // Reset applies only to this broker's principal.
        self.block_all = false; // Clear the persistent capacity-failure deny latch explicitly.
        invalidation.all_rights_blocked = false; // The initial origin policy is now visible again.
        Ok(invalidation) // The parent persists this explicit reset.
    } // Never call this implicitly on malformed persistence or package upgrade.
    pub fn replace_host_policy(&mut self, policy: Ceiling) -> Result<Invalidation> {
        // Availability/policy changes revoke derived access.
        let policy = validate_ceiling(policy)?; // Validate before replacing current supported authority.
        let invalidation = self.invalidate_all()?; // Fence all in-flight operations and pending reviews.
        self.policy = policy; // An empty policy deliberately disables privileged access.
        Ok(invalidation) // Resource owners close unavailable devices/services independently.
    } // Reconfiguration must prepare a fresh accepted plan.
    pub fn retire(&mut self, instance_id: u64) -> Invalidation {
        // Retire one scene without changing other grants.
        let invalidation = self.invalidate_instance(instance_id); // Old channels and tickets stop delivering immediately.
        self.instances.remove(&instance_id); // A fresh nonce prevents ABA if the numeric ID is later reused.
        invalidation // Native pending operations retain their custody until settlement.
    } // Emitted-frame evidence is owned separately and must outlive this retirement.
    pub fn pending_operations(&self) -> usize {
        self.operations.len()
    } // Includes revoked and retiring operations.
    fn allocate_id(&mut self) -> Result<u64> {
        // Share one monotone namespace for reviews and operations.
        let id = self.next_id; // Zero is never generated.
        self.next_id = id.checked_add(1).ok_or(PermissionError::Capacity)?; // No wrapping identity reuse.
        Ok(id) // Exhaustion refuses new work rather than recycling tokens.
    } // Issuer identity also changes when a broker is reconstructed.
    fn verdict(&self, right: &Right, binding: Option<&HostBinding>) -> Verdict {
        // Evaluate current effective authority.
        self.verdict_using(right, binding, &self.decisions) // Dispatch uses the current committed ledger.
    } // Resolution can evaluate a complete candidate ledger before publication.
    fn verdict_using(
        &self,
        right: &Right,
        binding: Option<&HostBinding>,
        decisions: &[DecisionRecord],
    ) -> Verdict {
        // Denial precedence is global.
        if self.block_all || !self.policy.iter().any(|ceiling| ceiling.covers(right)) {
            return Verdict::HostDenied;
        } // Host policy is mandatory.
        if decisions
            .iter()
            .any(|record| !record.allowed && record.right.overlaps(right))
        {
            return Verdict::UserDenied;
        } // Narrow revocation blocks a broad request containing it.
        let binding_hash = binding.map(HostBinding::fingerprint); // A pathname/selector change cannot inherit the old handle.
        if self.identity.verified_ilium
            || decisions.iter().any(|record| {
                record.allowed && record.right.covers(right) && record.binding_hash == binding_hash
            })
        {
            return Verdict::Allowed;
        } // Auto-grants remain inside policy and plan.
        Verdict::Prompt // Unverified/unknown origins start without privileged grants.
    } // Denial precedence is independent of request ordering.
    fn check_review(&self, review: &PlanReview) -> Result<()> {
        // UI answers never supply their own authority stamps.
        if !Arc::ptr_eq(&review.issuer, &self.issuer) {
            return Err(PermissionError::WrongBroker);
        } // Exact owner binding.
        let slot = self
            .instances
            .get(&review.stamp.instance_id)
            .ok_or(PermissionError::Stale)?; // Instance still exists.
        if review.stamp.epoch != self.epoch
            || slot.pending_nonce != review.stamp.nonce
            || slot.last_revision != review.stamp.plan_revision
        {
            return Err(PermissionError::Stale);
        } // Atomically bind plan and epoch.
        Ok(()) // The consuming resolve method prevents duplicate answer submission.
    } // Old accepted-plan revisions may continue only until a replacement commits.
    fn active(&self, channel: &Channel) -> Result<&ActivePlan> {
        // Resolve native channel authority only.
        if !Arc::ptr_eq(&channel.issuer, &self.issuer) {
            return Err(PermissionError::WrongBroker);
        } // Even identical package bytes cannot share tokens.
        if self.block_all || channel.stamp.epoch != self.epoch {
            return Err(PermissionError::Stale);
        } // Revocation fences retained grants too.
        let active = self
            .instances
            .get(&channel.stamp.instance_id)
            .and_then(|slot| slot.active.as_ref())
            .ok_or(PermissionError::Stale)?; // No retired authority.
        if active.channel.stamp != channel.stamp {
            return Err(PermissionError::Stale);
        } // Include nonce, not only numeric revision.
        Ok(active) // The caller never supplies an AcceptedPlan as proof.
    } // All effects and protected delivery pass through this lookup.
    fn check_needs(
        &self,
        channel: &Channel,
        demand_id: &str,
        needs: &[OperationNeed],
    ) -> Result<()> {
        // Real operation authorization.
        if needs.is_empty() || needs.len() > MAX_ITEMS {
            return Err(PermissionError::Denied);
        } // No vacuous all-of approval.
        let active = self.active(channel)?; // Current principal, epoch, instance, and accepted revision.
        let demand = active
            .plan
            .demands
            .get(demand_id)
            .ok_or(PermissionError::Denied)?; // Denied optional dependencies remove this demand.
        for need in needs {
            // Providers can require multiple independent rights.
            if !demand
                .request_ids
                .iter()
                .filter_map(|id| active.plan.grants.get(id))
                .any(|grant| grant.right.covers(&need.right) && grant.binding == need.binding)
            {
                return Err(PermissionError::Denied);
            } // A generic feed cannot borrow unrelated networking.
            if self.verdict(&need.right, need.binding.as_ref()) != Verdict::Allowed {
                return Err(PermissionError::Denied);
            } // Recheck user/host policy at each boundary.
        } // Service adapters still enforce every non-permission resource limit.
        Ok(()) // Only the exact derived requirement set is covered.
    } // Never accept a requirements array decoded from a helper message.
    fn operation(&self, ticket: &OperationTicket) -> Result<&PendingOperation> {
        // Authenticate ticket ownership before any cleanup/effect.
        if !Arc::ptr_eq(&ticket.issuer, &self.issuer) {
            return Err(PermissionError::WrongBroker);
        } // Integer collisions do not cross brokers.
        let operation = self
            .operations
            .get(&ticket.operation_id)
            .ok_or(PermissionError::UnknownOperation)?; // One bounded operation slot.
        if operation.channel.stamp != ticket.stamp {
            return Err(PermissionError::Stale);
        } // Exact original instance/plan/epoch.
        Ok(operation) // Stale authorization is still recognizable for native settlement.
    } // Authorization freshness is checked separately for commit and deliver.
    fn invalidation(&self, instance_id: Option<u64>) -> Invalidation {
        // Collect custody obligations without dropping them.
        Invalidation {
            authorization_epoch: self.epoch,
            instance_ids: self
                .instances
                .keys()
                .copied()
                .filter(|id| instance_id.is_none_or(|wanted| *id == wanted))
                .collect(),
            operations: self
                .operations
                .iter()
                .filter(|(_, operation)| {
                    instance_id.is_none_or(|wanted| operation.channel.stamp.instance_id == wanted)
                })
                .map(|(id, operation)| InvalidatedOperation {
                    operation_id: *id,
                    may_have_effects: operation.committed,
                })
                .collect(),
            all_rights_blocked: self.block_all,
        } // Bounded by broker caps.
    } // The parent can cancel queued work while retaining issued/uncertain outcomes.
    fn invalidate_instance(&mut self, instance_id: u64) -> Invalidation {
        // Replan/retire only this instance.
        let invalidation = self.invalidation(Some(instance_id)); // Capture affected ticket IDs first.
        if let Some(slot) = self.instances.get_mut(&instance_id) {
            slot.active = None;
            slot.pending_nonce = 0;
        } // Disable old channels and review tokens.
        invalidation // Underlying workers/handles remain owned until actual completion.
    } // Future preparation receives a fresh monotonically allocated nonce.
    fn invalidate_all(&mut self) -> Result<Invalidation> {
        // Authority changes affect every consumer of this principal.
        let Some(next) = self.epoch.checked_add(1) else {
            self.block_all = true;
            return Err(PermissionError::Capacity);
        }; // Counter exhaustion fails closed.
        self.epoch = next; // Serialized epoch publication blocks dispatch and delivery immediately.
        let invalidation = self.invalidation(None); // Include committed and uncommitted tickets.
        for slot in self.instances.values_mut() {
            slot.active = None;
            slot.pending_nonce = 0;
        } // Replan required after grant changes.
        Ok(invalidation) // Neither operation records nor external admission guards are released here.
    } // Saved history receipts are not authorization tickets and are not erased here.
} // End the independent broker implementation.
impl Right {
    // Typed scope containment is shared by ceilings, grants, and operation checks.
    pub(crate) fn normalized(mut self) -> Result<Self> {
        // Reject mismatched capability/scope pairs.
        match (&self.id, &mut self.scope) {
            // No catch-all capability can authorize new API families.
            (
                Capability::NetworkHttp | Capability::NetworkLocal,
                Scope::Network { origins, methods },
            ) => {
                // Exact HTTPS origins/methods.
                valid_count(origins.len())?;
                valid_count(methods.len())?; // Empty sets grant nothing and are rejected.
                let mut normalized = BTreeSet::new(); // Sort and deduplicate canonical origins.
                for origin in origins.iter() {
                    // Reject paths, queries, fragments, and credentials in scopes.
                    let parsed = checked_url(origin)?; // Canonical origin parsing handles IDNA/default ports.
                    if parsed.path() != "/" || parsed.query().is_some() {
                        return Err(PermissionError::Invalid("origin contains path or query"));
                    } // No prefix semantics.
                    normalized.insert(parsed.origin().ascii_serialization()); // Exact scheme/host/effective port.
                } // Different ports remain different scopes.
                *origins = normalized; // Request IDs now ignore equivalent spelling/order differences.
            } // Public and local HTTP rights share scope shape, never capability identity.
            (Capability::DiskRead | Capability::DiskWrite, Scope::Disk { slot, .. }) => {
                valid_id(slot)?
            } // Only picker slot selectors.
            (
                Capability::AudioLoopback | Capability::AudioMicrophone,
                Scope::Audio { device, products },
            ) => {
                valid_id(device)?;
                valid_count(products.len())?;
            } // No implicit product expansion.
            (Capability::LocationObserver, Scope::Observer) => {} // A separate future right is needed for device-derived position.
            (Capability::InputPointer | Capability::ScreenOcclusion, Scope::AnimationViewport) => {} // Geometry-only local inputs.
            (Capability::DeviceGpu, Scope::Gpu { kernels }) => {
                valid_count(kernels.len())?;
                for kernel in kernels.iter() {
                    valid_id(kernel)?;
                }
            } // Host policy bounds implemented kernels.
            (Capability::StatePersist, Scope::Namespace { name }) => valid_id(name)?, // No slashes, traversal, or foreign namespace paths.
            _ => return Err(PermissionError::Invalid("capability/scope mismatch")), // Unknown combinations fail closed.
        } // All active requests and stored decisions pass this validation.
        Ok(self) // The caller keeps normalized owned data.
    } // Numeric work/byte/rate limits are validated by the parent resource planner.
    fn needs_binding(&self) -> bool {
        matches!(&self.scope, Scope::Disk { .. } | Scope::Audio { .. })
    } // Files/devices need a host identity.
    fn covers(&self, wanted: &Self) -> bool {
        // One complete right must cover the requested cross product.
        if self.id != wanted.id {
            return false;
        } // Read/write, output/input capture, and local/public stay distinct.
        match (&self.scope, &wanted.scope) {
            // Exact identities plus subset relationships only.
            (
                Scope::Network {
                    origins: a,
                    methods: am,
                },
                Scope::Network {
                    origins: b,
                    methods: bm,
                },
            ) => b.is_subset(a) && bm.is_subset(am), // No suffix or wildcard matching.
            (
                Scope::Audio {
                    device: a,
                    products: ap,
                },
                Scope::Audio {
                    device: b,
                    products: bp,
                },
            ) => a == b && bp.is_subset(ap), // Product narrowing is allowed.
            (Scope::Gpu { kernels: a }, Scope::Gpu { kernels: b }) => b.is_subset(a), // Explicit bounded native kernels only.
            (a, b) => a == b, // Disk selection kind/slot and namespace must match exactly.
        } // Separate ceiling rows are never cross-multiplied into wider authority.
    } // Split a plan request when its scopes require different ceiling rows.
    fn overlaps(&self, wanted: &Self) -> bool {
        // Denials block any overlapping portion of a broader request.
        if self.id != wanted.id {
            return false;
        } // Independent capabilities retain separate decisions.
        match (&self.scope, &wanted.scope) {
            // Refuse the complete broad request until it is narrowed.
            (
                Scope::Network {
                    origins: a,
                    methods: am,
                },
                Scope::Network {
                    origins: b,
                    methods: bm,
                },
            ) => !a.is_disjoint(b) && !am.is_disjoint(bm), // Denied origin/method pairs survive widening.
            (
                Scope::Audio {
                    device: a,
                    products: ap,
                },
                Scope::Audio {
                    device: b,
                    products: bp,
                },
            ) => a == b && !ap.is_disjoint(bp), // Denied analysis product remains denied.
            (Scope::Gpu { kernels: a }, Scope::Gpu { kernels: b }) => !a.is_disjoint(b), // One revoked kernel cannot hide in a batch.
            (Scope::Disk { slot: a, .. }, Scope::Disk { slot: b, .. }) => a == b, // Changing file/folder shape cannot bypass a slot revocation.
            (a, b) => a == b, // Other scopes are exact singleton authorities.
        } // Explicit restoration removes exact records rather than broadly subtracting denies.
    } // Binding identity remains an additional intersection for allowed handle access.
} // End scope algebra.
fn validate_ceiling(ceiling: Ceiling) -> Result<Vec<Right>> {
    // Validate manifest and host policy independently.
    check_size(&ceiling, MAX_WIRE_BYTES)?; // Bound before normalizing/cloning scope strings.
    if ceiling.permissions.len() > MAX_ITEMS {
        return Err(PermissionError::Capacity);
    } // Empty ceilings intentionally deny everything.
    ceiling
        .permissions
        .into_iter()
        .map(Right::normalized)
        .collect() // Unknown/mismatched scope pairs are errors.
} // No implicit wildcard is introduced for verified packages.
fn valid_id(value: &str) -> Result<()> {
    // Stable IDs are also safe namespace components.
    if value.is_empty()
        || value.len() > 128
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(PermissionError::Invalid("identifier"));
    } // No paths, spaces, or control characters.
    Ok(()) // Display labels remain separate localized text.
} // Canonical request hashes fit comfortably inside this bound.
fn valid_reason(value: &str) -> Result<()> {
    // Application explanations are plain bounded text.
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(|c| c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) { return Err(PermissionError::Invalid("reason")); } // Prevent terminal/control and bidi spoofing.
    Ok(()) // Host UI still draws this inside protected application chrome.
} // A reason has no role in the effective authority calculation.
fn valid_count(count: usize) -> Result<()> {
    // Scope sets cannot be empty or arbitrarily large.
    if count == 0 || count > MAX_SCOPE_ITEMS {
        return Err(PermissionError::Invalid("scope item count"));
    } // No empty-set default allowance.
    Ok(()) // The overall serialized cap also applies.
} // Sets use canonical order for stable identity.
fn checked_url(value: &str) -> Result<Url> {
    // Parsing is lexical; DNS and connection stay native-owned.
    if value.len() > 4096 || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(PermissionError::Invalid("URL text"));
    } // Do not let URL parsing hide controls.
    let url = Url::parse(value).map_err(|_| PermissionError::Invalid("URL"))?; // Never compare raw prefixes.
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err(PermissionError::Invalid("HTTPS target"));
    } // No userinfo or unsupported protocol.
    Ok(url) // Paths/query are allowed only on actual operations, not origin scopes.
} // Native transport must disable automatic redirects/proxies unless independently brokered.
fn check_binding(right: &Right, binding: Option<&HostBinding>, required: bool) -> Result<()> {
    // Bind selected resources to typed rights.
    let expected = match &right.scope {
        Scope::Disk {
            selection: Selection::File,
            ..
        } => Some(BindingKind::File),
        Scope::Disk {
            selection: Selection::Folder,
            ..
        } => Some(BindingKind::Folder),
        Scope::Audio { .. } => Some(BindingKind::AudioDevice),
        _ => None,
    }; // Exact selection kind.
    match (expected, binding) {
        // Missing selections may be reviewed but cannot be activated as grants.
        (Some(kind), Some(binding)) if kind == binding.kind => Ok(()), // Actual identity is compared during dispatch.
        (Some(_), None) if !required => Ok(()), // UI can present a pending selection requirement.
        (None, None) => Ok(()), // Pure scope capabilities carry no opaque native handle.
        _ => Err(PermissionError::Invalid("resource binding")), // No file/folder/device substitution.
    } // Host-issued opaque identities never prove path containment by themselves.
} // File adapters must use safe handle-relative access and revalidate reopened roots.
fn canonical_id(right: &Right) -> Result<String> {
    // Canonical fallback when request_id is omitted.
    let mut digest = Sha256::new(); // Hash the normalized closed schema.
    digest.update(b"ilium-permission-request-v1\0"); // Version the identity algorithm explicitly.
    digest.update(serde_json::to_vec(right)?); // BTreeSets ensure deterministic origin/product order.
    Ok(format!("auto_{}", hex(&digest.finalize()))) // Stable through reason/localization changes.
} // Duplicate resulting IDs are rejected during review.
fn hex(bytes: &[u8]) -> String {
    // Deterministic lowercase digest formatting.
    const DIGITS: &[u8; 16] = b"0123456789abcdef"; // Avoid locale-sensitive formatting.
    let mut text = String::with_capacity(bytes.len() * 2); // Used only for fixed-size SHA256 values.
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 15)] as char);
    } // Encode both nibbles.
    text // Stable principal and request keys use these bytes.
} // No secret credential data is hashed or exported by this module.
fn upsert(records: &mut Vec<DecisionRecord>, record: DecisionRecord) -> Result<()> {
    // One latest explicit decision per exact scope.
    if let Some(index) = records
        .iter()
        .position(|previous| previous.right == record.right)
    {
        records[index] = record;
        return Ok(());
    } // Preserve unrelated scopes.
    if records.len() >= MAX_DECISIONS {
        return Err(PermissionError::Capacity);
    } // Denials cannot grow memory without bound.
    records.push(record); // Publication occurs only after complete candidate validation.
    Ok(()) // Callers check serialized ledger size before accepting the update.
} // Narrower overlapping deny records coexist with broader allowed records.
fn check_size(value: &impl Serialize, maximum: usize) -> Result<()> {
    // Count without producing a JSON copy.
    struct Counter {
        used: usize,
        maximum: usize,
    } // Capture only two machine-sized counters.
    impl Write for Counter {
        // serde_json streams serialized chunks through this bound.
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            // No retained input buffer.
            let next = self
                .used
                .checked_add(bytes.len())
                .ok_or_else(|| io::Error::other("size overflow"))?; // Checked arithmetic.
            if next > self.maximum {
                return Err(io::Error::other("permission byte budget"));
            } // Stop before allocation/publication.
            self.used = next; // Count exactly the accepted serialized bytes.
            Ok(bytes.len()) // Tell serde that this entire bounded chunk was consumed.
        } // No partial writes or hidden buffering.
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        } // There is no external sink.
    } // Serialization errors never become default policy.
    serde_json::to_writer(Counter { used: 0, maximum }, value)
        .map_err(|_| PermissionError::Capacity) // The schema contains no fallible custom serializers.
} // Native retained capacity/allocator peaks still need parent admission declarations.
fn public_address(address: IpAddr) -> bool {
    // Conservative local/special-address classification, not a routing oracle.
    match address {
        // Any unrecognized special range requires explicit network.local.
        IpAddr::V4(ip) => {
            // Keep loopback/private/link-local/reserved targets out of ordinary HTTPS grants.
            let [a, b, c, _] = ip.octets(); // Compare complete address prefixes, never hostname spelling.
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113)) // Include shared, benchmark, and documentation space.
        } // Native connection policy may impose stricter restrictions.
        IpAddr::V6(ip) => {
            // Inspect mapped IPv4 before the conservative global-unicast allowlist.
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_address(IpAddr::V4(mapped));
            } // Reject mapped local bypasses.
            let segments = ip.segments(); // IPv6 scopes and transition ranges need conservative handling.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] < 0x1000) // Exclude special, Teredo/6to4, and documentation ranges.
        } // ULA, link-local, multicast, unspecified, and NAT64 fall outside this allowlist.
    } // This function never trusts a helper's claimed address classification.
} // Bind connections to checked addresses; DNS validation alone cannot stop rebinding.
#[cfg(test)] // These are executable unit tests; the handoff does not claim they were run here.
mod tests {
    // Exercise authority boundaries, not only serialization round trips.
    use super::*; // Tests can construct the host-only trust and picker fixtures.
    fn http(origins: &[&str], methods: &[HttpMethod]) -> Right {
        // Typed fixture for exact HTTPS scopes.
        Right {
            id: Capability::NetworkHttp,
            scope: Scope::Network {
                origins: origins.iter().map(|origin| (*origin).to_owned()).collect(),
                methods: methods.iter().copied().collect(),
            },
        } // No wildcard origins.
    } // Test callers choose every origin and method explicitly.
    fn net() -> Right {
        http(&["https://example.org"], &[HttpMethod::Get])
    } // One common authorized scope.
    fn pointer() -> Right {
        Right {
            id: Capability::InputPointer,
            scope: Scope::AnimationViewport,
        }
    } // Independent optional right.
    fn request(id: &str, right: Right, required: bool) -> PermissionRequest {
        // Complete untrusted-plan fixture.
        PermissionRequest {
            request_id: Some(id.to_owned()),
            id: right.id,
            scope: right.scope,
            required,
            reason: "Use this scoped input for this animation.".to_owned(),
        } // No omitted explanation.
    } // Test authority still comes from the broker, not this constructor.
    fn plan(requests: Vec<PermissionRequest>) -> PermissionPlan {
        // Give each request one dependent demand.
        let demands = requests
            .iter()
            .map(|request| {
                let id = request.request_id.as_ref().expect("fixture request ID");
                Demand {
                    demand_id: format!("work_{id}"),
                    request_ids: BTreeSet::from([id.clone()]),
                }
            })
            .collect(); // Stable dependency mapping.
        PermissionPlan {
            permissions: requests,
            demands,
        } // More complex dependency tests replace this list.
    } // Baseline demands need no entry in this privileged projection.
    fn identity(verified: bool, bytes: &[u8]) -> PackageIdentity {
        // Simulate the host's authenticated inventory.
        if verified {
            PackageIdentity::from_ilium_inventory(
                "test".to_owned(),
                bytes,
                Sha256::digest(bytes).into(),
            )
            .expect("trusted fixture")
        } else {
            PackageIdentity::unverified("test".to_owned(), bytes).expect("unsigned fixture")
        } // Never read a publisher field.
    } // Changing unsigned bytes changes principal identity.
    fn broker(verified: bool, rights: Vec<Right>) -> PermissionBroker {
        // Explicit manifest and supported policy.
        PermissionBroker::new(
            identity(verified, b"package-v1"),
            Ceiling {
                permissions: rights.clone(),
            },
            Ceiling {
                permissions: rights,
            },
        )
        .expect("fixture broker") // No grants implicitly added for unsigned packages.
    } // Every test owns an independent issuer.
    fn choices(entries: &[(&str, UserChoice)]) -> BTreeMap<String, UserChoice> {
        entries
            .iter()
            .map(|(id, choice)| ((*id).to_owned(), *choice))
            .collect()
    } // Host UI fixture.
    fn net_active(broker: &mut PermissionBroker, instance: u64, revision: u64) -> Activation {
        // Activate one exact request.
        let review = broker
            .prepare(
                instance,
                revision,
                plan(vec![request("net", net(), true)]),
                BTreeMap::new(),
            )
            .expect("valid review"); // No acquisition in plan.
        let answers = if review.items()[0].verdict == Verdict::Prompt {
            choices(&[("net", UserChoice::AllowRemembered)])
        } else {
            BTreeMap::new()
        }; // Only explicit fixture decisions.
        broker
            .resolve(review, answers)
            .expect("resolved review")
            .activation
            .expect("active plan") // Tests below independently inspect invalidations.
    } // Native callers must handle the returned invalidation inventory too.
    fn need() -> Vec<OperationNeed> {
        vec![OperationNeed::new(net(), None).expect("valid actual need")]
    } // Native-derived fixture.
    #[test]
    fn recorded_source_lineage_requires_original_committed_current_grant() {
        let mut original = broker(true, vec![net()]);
        let active = net_active(&mut original, 1, 1);
        let ticket = original
            .dispatch(&active.channel, CallPhase::Async, "work_net", need())
            .expect("accepted original demand");
        assert!(matches!(
            original.committed_operation_grants(&ticket, &active.channel),
            Err(PermissionError::NotCommitted)
        ));
        original
            .commit(&ticket, || ())
            .expect("original issued effect");
        let grants = original
            .committed_operation_grants(&ticket, &active.channel)
            .expect("actual committed covering grant");
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].request_id, "net");
        assert_eq!(
            original
                .current_grant(&active.channel, "net")
                .unwrap()
                .request_id,
            grants[0].request_id
        );
        let foreign = broker(true, vec![net()]);
        assert!(matches!(
            foreign.current_grant(&active.channel, "net"),
            Err(PermissionError::WrongBroker)
        ));
        let invalidation = original.revoke(net()).expect("host revocation");
        assert_eq!(invalidation.operations.len(), 1);
        assert_eq!(
            invalidation.operations[0].operation_id,
            ticket.operation_id()
        );
        assert!(invalidation.operations[0].may_have_effects);
        assert!(original.current_grant(&active.channel, "net").is_err());
        assert!(original
            .committed_operation_grants(&ticket, &active.channel)
            .is_err());
        assert_eq!(original.pending_operations(), 1);
        original
            .settle_without_delivery(&ticket)
            .expect("original physical completion cleanup");
        assert_eq!(original.pending_operations(), 0);
    }
    #[test] // Canonical origins are stable but ports/methods are not widened.
    fn canonical_scopes_and_exact_containment() {
        // Exercise normalization and containment together.
        let a = http(&["https://EXAMPLE.org:443/"], &[HttpMethod::Get])
            .normalized()
            .expect("origin"); // Equivalent spelling.
        let b = net().normalized().expect("origin"); // Already canonical.
        assert_eq!(
            canonical_id(&a).expect("key"),
            canonical_id(&b).expect("key")
        ); // Same stable request identity.
        assert!(!a.covers(&http(&["https://example.org:8443"], &[HttpMethod::Get]))); // A new port needs authority.
        assert!(!a.covers(&http(
            &["https://example.org.evil.invalid"],
            &[HttpMethod::Get]
        ))); // No hostname prefix matching.
        assert!(!a.covers(&http(&["https://example.org"], &[HttpMethod::Post]))); // Read-like HTTP does not grant writes.
        assert!(http(&["https://example.org/path"], &[HttpMethod::Get])
            .normalized()
            .is_err()); // A scope must be an origin.
    } // Operation URLs may contain paths after origin authorization.
    #[test] // Neither publisher strings nor accepted-plan fields create authority.
    fn spoofed_wire_authority_and_unknown_capabilities_fail() {
        // Reject unsupported input instead of ignoring it.
        for bytes in [br#"{"permissions":[],"demands":[],"verified_ilium":true}"#.as_slice(), br#"{"permissions":[{"id":"os.exec","scope":{"kind":"observer"},"required":true,"reason":"Run code"}],"demands":[]}"#.as_slice(), br#"{"permissions":[{"id":"input.pointer","scope":{"kind":"animation_viewport"},"reason":"Read pointer"}],"demands":[]}"#.as_slice()] { assert!(PermissionBroker::parse_plan(bytes).is_err()); } // Unknown fields, IDs, and missing required flag all fail.
        assert!(matches!(
            PackageIdentity::from_ilium_inventory("test".to_owned(), b"replaced", [0; 32]),
            Err(PermissionError::Integrity)
        )); // Wrong archive bytes never become verified.
        let mut unverified = broker(false, vec![net()]); // Marketplace/URL appearance supplies no grant.
        let review = unverified
            .prepare(
                1,
                1,
                plan(vec![request("net", net(), true)]),
                BTreeMap::new(),
            )
            .expect("review"); // Valid declaration only.
        assert_eq!(review.items()[0].verdict, Verdict::Prompt); // No accidental initial allowance.
        assert!(matches!(
            unverified.resolve(review, BTreeMap::new()),
            Err(PermissionError::DecisionRequired(_))
        )); // No preselected Allow.
    } // Missing user answers never activate an unsigned package.
    #[test] // Verified origin fills only the user-grant position of the intersection.
    fn automatic_grants_still_require_manifest_and_host_policy() {
        // Validate both ceilings independently.
        let mut trusted = broker(true, vec![net()]); // Verified fixture has automatic scoped grants.
        assert_eq!(net_active(&mut trusted, 1, 1).plan.grants.len(), 1); // No consent dialog required.
        let denied = trusted.prepare(
            2,
            1,
            plan(vec![request("pointer", pointer(), true)]),
            BTreeMap::new(),
        ); // Undeclared capability.
        assert!(matches!(denied, Err(PermissionError::BeyondManifest(_)))); // Trust does not expand the manifest.
        let _invalidation = trusted
            .replace_host_policy(Ceiling {
                permissions: Vec::new(),
            })
            .expect("policy change"); // Supported policy can remove all authority.
        let review = trusted
            .prepare(
                1,
                2,
                plan(vec![request("net", net(), true)]),
                BTreeMap::new(),
            )
            .expect("review"); // Request still fits manifest.
        let result = trusted
            .resolve(review, BTreeMap::new())
            .expect("resolved denial"); // No new prompt for host denial.
        assert!(result.activation.is_none());
        assert_eq!(result.denied_required, vec!["net"]); // Required denial prevents activation.
    } // Automatic grants never bypass host availability/policy.
    #[test] // A denied optional dependency removes its entire dependent demand.
    fn optional_denials_prune_all_dependent_work() {
        // Test an all-of dependency rather than independent grants alone.
        let mut unverified = broker(false, vec![net(), pointer()]); // Two separately reviewed rights.
        let mut requested = plan(vec![
            request("net", net(), true),
            request("pointer", pointer(), false),
        ]); // A valid fallback remains possible.
        requested.demands.push(Demand {
            demand_id: "combined".to_owned(),
            request_ids: BTreeSet::from(["net".to_owned(), "pointer".to_owned()]),
        }); // Depends on both rights.
        let review = unverified
            .prepare(1, 1, requested, BTreeMap::new())
            .expect("review"); // No acquisition before decisions.
        let active = unverified
            .resolve(
                review,
                choices(&[
                    ("net", UserChoice::AllowSession),
                    ("pointer", UserChoice::DenyRemembered),
                ]),
            )
            .expect("resolution")
            .activation
            .expect("fallback active"); // Explicit independent choices.
        assert_eq!(
            active.plan.demands.keys().cloned().collect::<Vec<_>>(),
            vec!["work_net"]
        ); // Combined and pointer demands disappear atomically.
        assert!(matches!(
            unverified.dispatch(&active.channel, CallPhase::Async, "combined", need()),
            Err(PermissionError::Denied)
        )); // Native dispatch cannot resurrect pruned work.
    } // The script selects its declared fallback using this accepted projection.
    #[test] // Dialog order cannot override a narrower denial with a broad allowance.
    fn overlapping_denials_win_and_identical_aliases_are_rejected() {
        // Prevent two forms of contradictory authorization.
        let broad = http(
            &["https://example.org", "https://other.example"],
            &[HttpMethod::Get],
        ); // Two exact origins.
        for reverse in [false, true] {
            // Test both presentation orders.
            let mut unverified = broker(false, vec![broad.clone()]); // Manifest permits the full request.
            let mut requests = vec![
                request("broad", broad.clone(), true),
                request("narrow", net(), false),
            ]; // Narrow denial overlaps the required request.
            if reverse {
                requests.reverse();
            } // Reverse the ledger insertion order.
            let review = unverified
                .prepare(1, 1, plan(requests), BTreeMap::new())
                .expect("review"); // Both choices start unresolved.
            let result = unverified
                .resolve(
                    review,
                    choices(&[
                        ("broad", UserChoice::AllowRemembered),
                        ("narrow", UserChoice::DenyRemembered),
                    ]),
                )
                .expect("resolution"); // Apply the complete choice set.
            assert!(result.activation.is_none());
            assert_eq!(result.denied_required, vec!["broad"]); // Required broad authority is unavailable in both orders.
        } // A subsequent plan can explicitly narrow itself to the non-denied origin.
        let mut unverified = broker(false, vec![net()]); // Identical rights must share one request identity.
        assert!(unverified
            .prepare(
                1,
                1,
                plan(vec![request("a", net(), false), request("b", net(), false)]),
                BTreeMap::new()
            )
            .is_err()); // Reject aliases that could overwrite a denial.
    } // This also keeps capability queries unambiguous for identical scopes.
    #[test] // Revoking queued work must stop the issue closure without releasing its slot.
    fn queued_revocation_blocks_commit_and_keeps_custody() {
        // Model cancellation racing with dispatch.
        let mut trusted = broker(true, vec![net()]);
        let active = net_active(&mut trusted, 1, 1); // Active authenticated plan.
        let ticket = trusted
            .dispatch(&active.channel, CallPhase::Create, "work_net", need())
            .expect("queued"); // No effect yet.
        let invalidation = trusted.revoke(net()).expect("revoke"); // Atomic epoch change.
        assert_eq!(invalidation.operations.len(), 1);
        assert!(!invalidation.operations[0].may_have_effects); // Distinguish queued from issued work.
        let mut issued = false;
        assert!(trusted
            .commit(&ticket, || {
                issued = true;
            })
            .is_err());
        assert!(!issued); // Effect boundary was never crossed.
        assert_eq!(trusted.pending_operations(), 1); // Cancellation request does not imply native completion.
        trusted
            .settle_without_delivery(&ticket)
            .expect("actual cancellation settled");
        assert_eq!(trusted.pending_operations(), 0); // Explicit completed cleanup.
    } // External native admission guards follow the same actual-completion rule.
    #[test] // Issued effects may survive revocation, while protected result delivery must stop.
    fn committed_revocation_blocks_delivery_and_reports_possible_effects() {
        // Exercise both asynchronous boundaries.
        let mut trusted = broker(true, vec![net()]);
        let active = net_active(&mut trusted, 1, 1); // Valid authority.
        let ticket = trusted
            .dispatch(&active.channel, CallPhase::Async, "work_net", need())
            .expect("queued"); // Reserve one ticket.
        assert_eq!(trusted.commit(&ticket, || 17).expect("issued"), 17); // A bounded native issue can return an operation handle.
        assert!(matches!(
            trusted.commit(&ticket, || 0),
            Err(PermissionError::AlreadyCommitted)
        )); // Never issue twice.
        let invalidation = trusted.revoke(net()).expect("revoke");
        assert!(invalidation.operations[0].may_have_effects); // Do not claim an issued request was undone.
        let mut delivered = false;
        assert!(trusted
            .deliver(&ticket, || {
                delivered = true;
            })
            .is_err());
        assert!(!delivered); // Final protected handoff rechecks the epoch.
        assert_eq!(trusted.pending_operations(), 1);
        trusted
            .settle_without_delivery(&ticket)
            .expect("completed without delivery"); // Real completion releases the slot.
    } // The native owner separately records actual or uncertain outcome.
    #[test] // Tokens bind an issuer as well as package, instance, revision, and epoch.
    fn cross_broker_and_reused_instance_tokens_cannot_authorize() {
        // Prevent integer/identity collisions and ABA reuse.
        let mut first = broker(true, vec![net()]);
        let mut second = broker(true, vec![net()]); // Identical bytes but distinct owners.
        let old = net_active(&mut first, 1, 1);
        let ticket = first
            .dispatch(&old.channel, CallPhase::Async, "work_net", need())
            .expect("queued"); // Original lineage.
        assert!(matches!(
            second.dispatch(&old.channel, CallPhase::Async, "work_net", need()),
            Err(PermissionError::WrongBroker)
        )); // Channels cannot cross owners.
        assert!(matches!(
            second.settle_without_delivery(&ticket),
            Err(PermissionError::WrongBroker)
        )); // Nor can cleanup steal another owner's ticket.
        let _invalidation = first.retire(1);
        let new = net_active(&mut first, 1, 1); // Reuse numeric identity/revision after retirement.
        assert!(first.grant(&old.channel, "net").is_err());
        assert!(first
            .grant(&new.channel, "net")
            .expect("new channel")
            .is_some()); // Nonce distinguishes the new instance.
        assert!(first.commit(&ticket, || ()).is_err());
        first
            .settle_without_delivery(&ticket)
            .expect("old native completion"); // Old work stays stale but settleable.
    } // The IPC host must map helper correlation IDs to these native tokens.
    #[test] // A delayed permission dialog cannot undo a subsequent revocation.
    fn stale_review_and_replan_results_are_fenced() {
        // Include accepted-plan replacement in the same test.
        let mut trusted = broker(true, vec![net()]);
        let old = net_active(&mut trusted, 1, 1); // Initial accepted revision.
        let review = trusted
            .prepare(
                1,
                2,
                plan(vec![request("net", net(), true)]),
                BTreeMap::new(),
            )
            .expect("replacement review"); // Old plan may continue during review.
        assert!(trusted
            .grant(&old.channel, "net")
            .expect("old plan still active")
            .is_some()); // No premature partial reconfiguration.
        let replacement = trusted
            .resolve(review, BTreeMap::new())
            .expect("replace")
            .activation
            .expect("active replacement"); // Atomic replacement.
        assert!(trusted.grant(&old.channel, "net").is_err());
        assert!(trusted.grant(&replacement.channel, "net").is_ok()); // Old revision loses authority.
        let review = trusted
            .prepare(
                1,
                3,
                plan(vec![request("net", net(), true)]),
                BTreeMap::new(),
            )
            .expect("pending review"); // Simulate an open dialog.
        let _invalidation = trusted.revoke(net()).expect("revoke"); // Newer authorization epoch wins.
        assert!(matches!(
            trusted.resolve(review, BTreeMap::new()),
            Err(PermissionError::Stale)
        )); // Late answers cannot reactivate it.
    } // UI decisions and reconfiguration commit against one sealed plan stamp.
    #[test] // Explicit verified-origin revocation survives both restart and authentic update.
    fn remembered_revocation_and_unsigned_content_identity_survive_restart() {
        // Test positive trust without automatic regrant.
        let mut trusted = broker(true, vec![net()]);
        let _invalidation = trusted.revoke(net()).expect("revoke"); // Persist an explicit denial.
        let saved = trusted.export_remembered().expect("saved bytes"); // Parent still owes durable publication.
        let mut updated = PermissionBroker::new(
            identity(true, b"package-v2"),
            Ceiling {
                permissions: vec![net()],
            },
            Ceiling {
                permissions: vec![net()],
            },
        )
        .expect("updated broker")
        .restore_remembered(&saved)
        .expect("same authenticated lineage"); // Approved old hash remains recorded.
        let review = updated
            .prepare(
                1,
                1,
                plan(vec![request("net", net(), true)]),
                BTreeMap::new(),
            )
            .expect("review");
        assert_eq!(review.items()[0].verdict, Verdict::UserDenied); // No silent automatic restoration.
        assert!(updated
            .resolve(review, BTreeMap::new())
            .expect("denial")
            .activation
            .is_none()); // Required request stays inactive.
        let _invalidation = updated.restore_scope(net()).expect("explicit restore");
        assert_eq!(net_active(&mut updated, 1, 2).plan.grants.len(), 1); // User restoration re-enables initial verified policy.
        let mut unsigned = broker(false, vec![net()]);
        let _active = net_active(&mut unsigned, 1, 1);
        let saved = unsigned.export_remembered().expect("unsigned saved"); // Remember one exact archive's allowance.
        let replacement = PermissionBroker::new(
            identity(false, b"package-v2"),
            Ceiling {
                permissions: vec![net()],
            },
            Ceiling {
                permissions: vec![net()],
            },
        )
        .expect("replacement"); // Same package name is insufficient.
        assert!(matches!(
            replacement.restore_remembered(&saved),
            Err(PermissionError::WrongPrincipal)
        )); // Unsigned replacement cannot inherit authority.
    } // Failed restore consumes its broker and has no permissive default path.
    #[test] // Host-selected identity is part of the grant, independently of the picker slot.
    fn remembered_disk_grants_require_the_same_revalidated_resource() {
        // Test changed roots and forged operation substitution.
        let disk = Right {
            id: Capability::DiskRead,
            scope: Scope::Disk {
                slot: "world".to_owned(),
                selection: Selection::Folder,
            },
        }; // Narrow stable picker slot.
        let a = HostBinding::new(BindingKind::Folder, "root_a".to_owned()).expect("binding A");
        let b = HostBinding::new(BindingKind::Folder, "root_b".to_owned()).expect("binding B"); // Native resolved identities.
        let mut unsigned = broker(false, vec![disk.clone()]); // No initial disk access.
        let review = unsigned
            .prepare(
                1,
                1,
                plan(vec![request("disk", disk.clone(), true)]),
                BTreeMap::from([("disk".to_owned(), a.clone())]),
            )
            .expect("selected review"); // Native picker supplied A.
        let active = unsigned
            .resolve(review, choices(&[("disk", UserChoice::AllowRemembered)]))
            .expect("allow A")
            .activation
            .expect("active"); // Remember A's fingerprint.
        let wrong = vec![OperationNeed::new(disk.clone(), Some(b.clone())).expect("typed B need")]; // Same selector but different actual target.
        assert!(matches!(
            unsigned.dispatch(&active.channel, CallPhase::Async, "work_disk", wrong),
            Err(PermissionError::Denied)
        )); // The selected root cannot be substituted.
        let saved = unsigned.export_remembered().expect("saved");
        let mut reopened = broker(false, vec![disk.clone()])
            .restore_remembered(&saved)
            .expect("restore ledger"); // No native handle was resurrected.
        let review = reopened
            .prepare(
                1,
                1,
                plan(vec![request("disk", disk, true)]),
                BTreeMap::from([("disk".to_owned(), b)]),
            )
            .expect("reopened B"); // The pathname may have changed identity.
        assert_eq!(review.items()[0].verdict, Verdict::Prompt); // A newly resolved root needs a new decision.
    } // File containment/symlink-safe opening remains the native adapter's responsibility.
    #[test] // A verified package still cannot invent a user-selected file/device.
    fn canceled_optional_selection_preserves_fallback() {
        // An absent optional selection is an explicit outcome.
        let disk = Right {
            id: Capability::DiskRead,
            scope: Scope::Disk {
                slot: "picture".to_owned(),
                selection: Selection::File,
            },
        }; // An optional external asset.
        let mut trusted = broker(true, vec![disk.clone()]);
        let review = trusted
            .prepare(
                1,
                1,
                plan(vec![request("disk", disk, false)]),
                BTreeMap::new(),
            )
            .expect("review"); // No host-selected handle yet.
        assert_eq!(review.items()[0].verdict, Verdict::NeedsSelection); // Automatic grant does not create a resource identity.
        let active = trusted
            .resolve(review, choices(&[("disk", UserChoice::DenySession)]))
            .expect("picker canceled")
            .activation
            .expect("fallback"); // Explicitly deny the optional selection.
        assert!(active.plan.grants.is_empty());
        assert!(active.plan.demands.is_empty()); // Dependent acquisition is absent.
    } // A required request would instead appear in denied_required.
    #[test] // Planning, rendering, and cleanup are never privilege-acquisition phases.
    fn render_cannot_dispatch_and_returned_plan_cannot_grant() {
        // Script-visible copies are not authority.
        let mut trusted = broker(true, vec![net(), pointer()]);
        let mut active = net_active(&mut trusted, 1, 1); // Plan declares only networking.
        for phase in [CallPhase::Plan, CallPhase::Render, CallPhase::Dispose] {
            assert!(matches!(
                trusted.dispatch(&active.channel, phase, "work_net", need()),
                Err(PermissionError::Denied)
            ));
        } // Native phase is checked before dispatch.
        active.plan.grants.insert(
            "pointer".to_owned(),
            Grant {
                request_id: "pointer".to_owned(),
                right: pointer(),
                binding: None,
            },
        ); // Mutate the informational copy deliberately.
        active.plan.demands.insert(
            "fake".to_owned(),
            Demand {
                demand_id: "fake".to_owned(),
                request_ids: BTreeSet::from(["pointer".to_owned()]),
            },
        ); // Attempt to fabricate accepted work.
        assert!(matches!(
            trusted.dispatch(
                &active.channel,
                CallPhase::Async,
                "fake",
                vec![OperationNeed::new(pointer(), None).expect("need")]
            ),
            Err(PermissionError::Denied)
        )); // Broker's independent accepted copy wins.
        let ticket = trusted
            .dispatch(&active.channel, CallPhase::Async, "work_net", need())
            .expect("real demand"); // Real asynchronous work still works.
        assert!(matches!(
            trusted.deliver(&ticket, || ()),
            Err(PermissionError::NotCommitted)
        )); // No fabricated result before issue.
        trusted.commit(&ticket, || ()).expect("commit");
        assert_eq!(
            trusted
                .deliver(&ticket, || 42)
                .expect("authorized delivery"),
            42
        ); // Successful handoff settles once.
        assert!(matches!(
            trusted.deliver(&ticket, || 0),
            Err(PermissionError::UnknownOperation)
        )); // No duplicate delivery.
    } // Native binding phase and requirement derivation are mandatory integration responsibilities.
    #[test] // A public-looking hostname does not authorize its loopback/private resolution.
    fn actual_http_addresses_require_additional_local_authority() {
        // Include mapped IPv4 and mixed DNS answers.
        let public: IpAddr = "8.8.8.8".parse().expect("address");
        let local: IpAddr = "127.0.0.1".parse().expect("address"); // Native resolver fixtures.
        assert_eq!(
            OperationNeed::http_hop("https://example.org/a", HttpMethod::Get, &[public])
                .expect("public hop")
                .len(),
            1
        ); // Public HTTPS still needs network.http.
        let local_needs =
            OperationNeed::http_hop("https://example.org/a", HttpMethod::Get, &[public, local])
                .expect("mixed hop");
        assert_eq!(local_needs.len(), 2); // Any local candidate requires additional authority.
        assert_eq!(local_needs[1].right.id, Capability::NetworkLocal); // It supplements rather than replaces HTTP.
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().expect("mapped address");
        assert!(!public_address(mapped)); // IPv6 spelling cannot hide IPv4 loopback.
        let mut trusted = broker(true, vec![net()]);
        let active = net_active(&mut trusted, 1, 1); // Only the HTTP right was declared.
        assert!(matches!(
            trusted.dispatch(&active.channel, CallPhase::Async, "work_net", local_needs),
            Err(PermissionError::Denied)
        )); // Even verified origin cannot skip declared local authority.
        for url in [
            "http://example.org",
            "https://user@example.org",
            "https://example.org/#fragment",
            "https://example.org:0/",
        ] {
            assert!(OperationNeed::http_hop(url, HttpMethod::Get, &[public]).is_err());
        } // Reject unsupported or misleading targets.
        assert!(OperationNeed::http_hop("https://example.org", HttpMethod::Get, &[]).is_err());
        // No unchecked resolution.
    } // Native transport must also pin connection addresses and validate every redirect.
    #[test] // Revocation cannot be used to release slots for still-owned operations.
    fn unsettled_operations_remain_bounded_after_retirement() {
        // Model a native service that has not completed cancellation.
        let mut trusted = broker(true, vec![net()]);
        let active = net_active(&mut trusted, 1, 1);
        let mut tickets = Vec::new(); // One bounded consumer.
        for _ in 0..MAX_OPERATIONS {
            tickets.push(
                trusted
                    .dispatch(&active.channel, CallPhase::Async, "work_net", need())
                    .expect("admitted slot"),
            );
        } // Fill the explicit metadata envelope.
        let _invalidation = trusted.retire(1);
        let replacement = net_active(&mut trusted, 1, 1); // Replacement does not release old custody.
        assert!(matches!(
            trusted.dispatch(&replacement.channel, CallPhase::Async, "work_net", need()),
            Err(PermissionError::Capacity)
        )); // Backpressure includes revoked records.
        let first = tickets.pop().expect("old ticket");
        trusted
            .settle_without_delivery(&first)
            .expect("actual native completion"); // Exactly one slot is now reusable.
        let new = trusted
            .dispatch(&replacement.channel, CallPhase::Async, "work_net", need())
            .expect("new slot");
        assert_eq!(trusted.pending_operations(), MAX_OPERATIONS); // No unbounded restart escape.
        trusted
            .settle_without_delivery(&new)
            .expect("new cancellation completed");
        for ticket in tickets {
            trusted
                .settle_without_delivery(&ticket)
                .expect("old completion");
        } // Clean up actual fixture ownership.
        assert_eq!(trusted.pending_operations(), 0); // All ownership has settled explicitly.
    } // Admission guards outside this module must not be tied merely to plan lifetime.
    #[test] // Malformed dependencies, explanations, and oversized persistence fail closed.
    fn malformed_and_oversized_inputs_are_rejected() {
        // Exercise distinct validation boundaries.
        let mut trusted = broker(true, vec![net()]);
        let mut orphan = plan(vec![request("net", net(), true)]); // Start from valid input.
        orphan.demands[0].request_ids.insert("missing".to_owned());
        assert!(trusted.prepare(1, 1, orphan, BTreeMap::new()).is_err()); // Unknown dependency cannot disappear silently.
        let mut empty_reason = plan(vec![request("net", net(), true)]);
        empty_reason.permissions[0].reason = " ".to_owned();
        assert!(trusted
            .prepare(1, 1, empty_reason, BTreeMap::new())
            .is_err()); // Human explanation is mandatory.
        let mut duplicate = plan(vec![request("net", net(), true)]);
        duplicate.demands.push(duplicate.demands[0].clone());
        assert!(trusted.prepare(1, 1, duplicate, BTreeMap::new()).is_err()); // Demand aliases are rejected.
        assert!(matches!(
            PermissionBroker::parse_plan(&vec![b' '; MAX_WIRE_BYTES + 1]),
            Err(PermissionError::Capacity)
        )); // Check wire bound before deserialization.
        assert!(broker(true, vec![net()]).restore_remembered(b"{}").is_err()); // Malformed stored decisions never become an empty ledger.
        assert!(matches!(
            broker(true, vec![net()]).restore_remembered(&vec![b' '; MAX_STATE_BYTES + 1]),
            Err(PermissionError::Capacity)
        )); // Stored payload has its own cap.
    } // The parent should display these failures without activating privileged work.
    fn group_active(broker: &mut PermissionBroker, instance: u64) -> Activation {
        let review = broker
            .prepare(
                instance,
                1,
                plan(vec![
                    request("net", net(), true),
                    request("pointer", pointer(), true),
                ]),
                BTreeMap::new(),
            )
            .unwrap();
        broker
            .resolve(review, BTreeMap::new())
            .unwrap()
            .activation
            .unwrap()
    }
    fn group_tickets(
        broker: &mut PermissionBroker,
        active: &Activation,
    ) -> (OperationTicket, OperationTicket) {
        let first = broker
            .dispatch(&active.channel, CallPhase::Async, "work_net", need())
            .unwrap();
        let second = broker
            .dispatch(
                &active.channel,
                CallPhase::Async,
                "work_pointer",
                vec![OperationNeed::new(pointer(), None).unwrap()],
            )
            .unwrap();
        (first, second)
    }
    #[test]
    fn grouped_original_dependencies_commit_and_deliver_once() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, b) = group_tickets(&mut owner, &active);
        let mut issued = 0;
        owner.commit_group(&[&a, &b], || issued += 1).unwrap();
        assert_eq!(issued, 1);
        assert!(owner
            .committed_operation_grants(&a, &active.channel)
            .is_ok());
        assert!(owner
            .committed_operation_grants(&b, &active.channel)
            .is_ok());
        assert_eq!(owner.deliver_group(&[&a, &b], || 123).unwrap(), 123);
        assert!(owner.operations.is_empty());
        assert!(owner.deliver_group(&[&a, &b], || ()).is_err());
    }
    #[test]
    fn grouped_commit_refusal_does_not_partially_commit_or_issue() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, b) = group_tickets(&mut owner, &active);
        owner.commit(&b, || ()).unwrap();
        let mut issued = false;
        assert!(owner.commit_group(&[&a, &b], || issued = true).is_err());
        assert!(!issued);
        assert!(!owner.operation(&a).unwrap().committed);
        owner.commit(&a, || ()).unwrap();
    }
    #[test]
    fn grouped_original_dependencies_refuse_duplicate_foreign_and_cross_instance_tickets() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, _b) = group_tickets(&mut owner, &active);
        assert!(owner.commit_group(&[], || ()).is_err());
        assert!(owner.commit_group(&[&a, &a], || ()).is_err());
        let other_active = group_active(&mut owner, 2);
        let (_, cross) = group_tickets(&mut owner, &other_active);
        assert!(owner.commit_group(&[&a, &cross], || ()).is_err());
        let mut foreign_owner = broker(true, vec![net(), pointer()]);
        let foreign_active = group_active(&mut foreign_owner, 1);
        let (_, foreign) = group_tickets(&mut foreign_owner, &foreign_active);
        assert!(owner.commit_group(&[&a, &foreign], || ()).is_err());
        assert!(!owner.operation(&a).unwrap().committed);
    }
    #[test]
    fn grouped_delivery_after_revocation_retains_every_original_custody_ticket() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, b) = group_tickets(&mut owner, &active);
        owner.commit_group(&[&a, &b], || ()).unwrap();
        let _invalidation = owner.revoke(pointer()).unwrap();
        let mut published = false;
        assert!(owner.deliver_group(&[&a, &b], || published = true).is_err());
        assert!(!published);
        assert_eq!(owner.operations.len(), 2);
        // Synthetic body-completion point: only now may the native owner settle.
        owner.settle_without_delivery(&a).unwrap();
        owner.settle_without_delivery(&b).unwrap();
        assert!(owner.operations.is_empty());
    }
    #[test]
    fn grouped_native_settlement_refuses_foreign_dependency_without_partial_release() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, b) = group_tickets(&mut owner, &active);
        let mut foreign_owner = broker(true, vec![net(), pointer()]);
        let foreign_active = group_active(&mut foreign_owner, 1);
        let (_, foreign) = group_tickets(&mut foreign_owner, &foreign_active);
        assert!(owner
            .settle_group_without_delivery(&[&a, &foreign])
            .is_err());
        assert_eq!(owner.operations.len(), 2);
        assert!(owner.settle_group_without_delivery(&[&a, &a]).is_err());
        assert_eq!(owner.operations.len(), 2);
        owner.settle_group_without_delivery(&[&a, &b]).unwrap();
        assert!(owner.operations.is_empty());
    }
    #[test]
    fn grouped_native_completion_settles_original_tickets_after_revocation() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, b) = group_tickets(&mut owner, &active);
        owner.commit_group(&[&a, &b], || ()).unwrap();
        let invalidation = owner.revoke(pointer()).unwrap();
        assert!(invalidation.authorization_epoch > 0);
        owner.settle_group_without_delivery(&[&a, &b]).unwrap();
        assert!(owner.operations.is_empty());
    }
    #[test]
    fn grouped_delivery_can_settle_metadata_even_when_native_publication_returns_error() {
        let mut owner = broker(true, vec![net(), pointer()]);
        let active = group_active(&mut owner, 1);
        let (a, b) = group_tickets(&mut owner, &active);
        owner.commit_group(&[&a, &b], || ()).unwrap();
        let settled = std::sync::atomic::AtomicBool::new(false);
        let outcome: std::result::Result<(), &str> = owner
            .deliver_group(&[&a, &b], || {
                settled.store(true, std::sync::atomic::Ordering::Release);
                Err("synthetic helper delivery error after broker settlement")
            })
            .unwrap();
        assert!(outcome.is_err());
        assert!(settled.load(std::sync::atomic::Ordering::Acquire));
        assert!(owner.operations.is_empty());
        assert!(owner.settle_group_without_delivery(&[&a, &b]).is_err());
    }
} // End complete permission tests.
