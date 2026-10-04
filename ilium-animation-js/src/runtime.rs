//! Worker-owned package activation. Informational script plans never become
//! authority: only the native broker's current channel can activate an instance.
use crate::{
    engine::{
        ArraySpec, CompletionState, CreateState, HostRequest, ServiceAuthority, ServicePhase,
        ServiceValue,
    }, // Use the admitted binary service boundary.
    error::{AnimationError, Result},
    helper::{HelperAuthority, HelperLimits, HelperSession, PreparedSeed, RetainedHelperFrame},
    manifest::AnimationMode,
    package::{Package, PackageLimits},
    permissions::{
        Activation,
        CallPhase,
        Ceiling,
        HostBinding,
        Invalidation,
        OperationNeed,
        OperationTicket,
        PermissionBroker, // Retain genuine native operation authority.
        PlanReview,
        UserChoice, // Keep host-reviewed consent independent of helper transport.
    },
    plan::{AnimationPlan, PlanBudget},
    plan_authorization::AuthorizationProjection,
    trust::{PackageIdentity as VerifiedIdentity, TrustVerifier},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

pub struct PackageInstance {
    helper: HelperSession,
    quota: QuotaGroup,
    pending_resolution: Option<Arc<()>>,
    helper_retired: bool, // Logical retirement intent never proves that the child or its pipe workers exited.
    broker: Arc<Mutex<PermissionBroker>>,
    identity: VerifiedIdentity,
    package: Arc<Package>,
    mode: AnimationMode, // Retain the native-selected mode so every emission path enforces pre-rendered retirement.
    plan: AnimationPlan,
    projection: AuthorizationProjection,
    activation: Option<Activation>,
    creation: Option<CreateState>,
    settings: Value,
    // Original storage is owned by the shared broker through its FINAL native owner.
}

pub struct InstancePreparation<'a> {
    pub archive: &'a [u8],
    pub verifier: &'a TrustVerifier,
    pub helper_executable: &'a Path,
    pub trusted_bootstrap: &'a str,
    pub settings: &'a Value,
    pub mode: AnimationMode,
    pub environment: &'a Value,
    pub host_policy: Ceiling,
    pub instance_id: u64,
    pub limits: HelperLimits,
    pub quota: QuotaGroup,
}

/// Admitted immutable native preparation, before helper launch and ledger restore.
/// This owner retains the SAME parsed Package and a bounded copy of its archive.
pub struct VerifiedPreparation {
    archive: Vec<u8>,
    helper_executable: PathBuf,
    trusted_bootstrap: String,
    environment: Value,
    package: Arc<Package>,
    identity: VerifiedIdentity,
    broker: PermissionBroker,
    settings: Value,
    mode: AnimationMode,
    instance_id: u64,
    limits: HelperLimits,
    quota: QuotaGroup,
    _storage: StorageAdmission,
}

/// Genuine native pending activation; not Clone, Deserialize or constructible.
/// The trusted host controller retains exclusive custody through real durable IO.
/// No copied hash, boolean, JSON or operation ID can construct this token.
pub struct PendingResolution {
    issuer: Arc<()>,
    instance_id: u64,
    plan_revision: u64,
    epoch: u64,
    activation: Option<Activation>,
    invalidation: Invalidation,
    denied_required: Vec<String>,
    candidate: Option<Vec<u8>>,
    persistence_error: Option<AnimationError>,
    digest: Option<[u8; 32]>,
    _storage: StorageAdmission,
}
impl PendingResolution {
    pub fn invalidation(&self) -> &Invalidation {
        &self.invalidation
    }
    pub fn denied_required(&self) -> &[String] {
        &self.denied_required
    }
    pub fn remembered_bytes(&self) -> Option<&[u8]> {
        self.candidate.as_deref()
    }
    pub fn persistence_error(&self) -> Option<&AnimationError> {
        self.persistence_error.as_ref()
    }
    pub fn remembered_digest(&self) -> Option<[u8; 32]> {
        self.digest
    }
    pub fn authorization_epoch(&self) -> u64 {
        self.epoch
    }
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
    pub fn plan_revision(&self) -> u64 {
        self.plan_revision
    }
}
impl VerifiedPreparation {
    pub fn principal(&self) -> &crate::permissions::PackageIdentity {
        self.broker.identity()
    }
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }
    pub fn authorization_epoch(&self) -> u64 {
        self.broker.authorization_epoch()
    }
    pub fn package(&self) -> &Package {
        &self.package
    }
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }

    /// Only for packages declaring ZERO rights: no remembered right can grant
    /// anything. Rights-bearing packages must load a native namespace first.
    pub fn prepare_without_rights(self) -> Result<(PackageInstance, PlanReview)> {
        if !self.package.manifest().capabilities.is_empty() {
            return Err(AnimationError::PermissionDenied(
                "native permission ledger load required".into(),
            ));
        }
        self.prepare(None)
    }
    /// Some is a current admitted native ledger; None only follows a genuine
    /// current missing-state load (apart from the restricted zero-right wrapper).
    pub fn prepare(self, remembered: Option<&[u8]>) -> Result<(PackageInstance, PlanReview)> {
        let Self {
            archive,
            helper_executable,
            trusted_bootstrap,
            environment,
            package,
            identity,
            mut broker,
            settings,
            mode,
            instance_id,
            limits,
            quota,
            _storage: storage,
        } = self;
        // Consume restore BEFORE launching a helper or creating a review. Failure
        // drops this unused broker; there is no empty-ledger retry or live rollback.
        if let Some(bytes) = remembered {
            broker = broker.restore_remembered(bytes).map_err(permission_error)?;
        }
        // Transfer the ORIGINAL admission before first sharing: audio/source
        // factory clones may outlive PackageInstance and retain broker metadata.
        broker
            .retain_authority_storage(storage)
            .map_err(permission_error)?;
        let mut helper = HelperSession::launch(
            &helper_executable,
            &archive,
            &trusted_bootstrap,
            HelperAuthority {
                package_digest: identity.digest().into(),
                instance_id,
                plan_generation: 1,
                authorization_epoch: broker.authorization_epoch(),
            },
            limits,
            quota.clone(),
        )?;
        let raw = helper.plan(&settings, mode.clone(), &environment)?;
        let mut plan_budget = PlanBudget::default();
        plan_budget.max_clip_seconds = plan_budget
            .max_clip_seconds
            .min(package.manifest().limits.clip_seconds as f64);
        let plan = AnimationPlan::parse(&raw, mode.clone(), plan_budget)?;
        let projection = AuthorizationProjection::build(package.manifest(), &plan)?;
        let review = broker
            .prepare(
                instance_id,
                1,
                projection.permission_plan.clone(),
                BTreeMap::new(),
            )
            .map_err(permission_error)?;
        Ok((
            PackageInstance {
                helper,
                quota,
                pending_resolution: None,
                helper_retired: false,
                broker: Arc::new(Mutex::new(broker)),
                identity,
                package,
                mode,
                plan,
                projection,
                activation: None,
                creation: None,
                settings,
            },
            review,
        ))
    }
}

// Allocation-free preflight before cloning host environment into a suspended owner.
fn environment_bytes(value: &Value, depth: usize) -> Result<usize> {
    if depth > 32 {
        return Err(AnimationError::Budget(
            "preparation environment depth".into(),
        ));
    }
    let mut bytes = 64usize;
    match value {
        Value::String(text) => bytes = bytes.saturating_add(text.len()),
        Value::Array(values) => {
            for value in values {
                bytes = bytes.saturating_add(environment_bytes(value, depth + 1)?);
                if bytes > 256 * 1024 {
                    return Err(AnimationError::Budget(
                        "preparation environment size".into(),
                    ));
                }
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                bytes = bytes
                    .saturating_add(1024 + key.len())
                    .saturating_add(environment_bytes(value, depth + 1)?);
                if bytes > 256 * 1024 {
                    return Err(AnimationError::Budget(
                        "preparation environment size".into(),
                    ));
                }
            }
        }
        _ => {}
    }
    if bytes > 256 * 1024 {
        return Err(AnimationError::Budget(
            "preparation environment size".into(),
        ));
    }
    Ok(bytes)
}

pub struct StoppedInstance {
    pub invalidation: Option<Invalidation>,
    pub cancellation: Result<()>,
    pub authority_error: Option<AnimationError>, // Poisoning never fabricates retirement or clears the ledger.
}

pub struct InstanceResolution {
    pub invalidation: Invalidation,
    pub denied_required: Vec<String>,
    pub creation: Option<CreateState>,
    pub teardown_error: Option<AnimationError>,
    pub activation_invalidation: Option<Invalidation>, // Return additional cleanup if accepted activation fails during native startup.
    pub authority_error: Option<AnimationError>, // Independent inability to retire poisoned native authority.
    pub creation_error: Option<AnimationError>, // Keep the original startup failure distinct from physical teardown failure.
} // Preserve consent, activation cleanup, startup failure, and physical retirement as independent results.
impl InstanceResolution {
    /// A native creation may still be awaiting real asynchronous services. Only
    /// an accepted resolution with no startup, authority or teardown failure
    /// permits exposing the instance; Pending never implies animation readiness.
    pub fn accepted_creation(&self) -> Option<CreateState> {
        if !self.denied_required.is_empty()
            || self.creation_error.is_some()
            || self.authority_error.is_some()
            || self.teardown_error.is_some()
            || self.activation_invalidation.is_some()
        {
            None
        } else {
            self.creation
        }
    }
}

pub(crate) struct ServiceOperation {
    // The native dispatcher retains this noncloneable request and ticket association until actual work terminates.
    request: HostRequest, // Keep the exact admitted input and native correlation stamp alive with the operation.
    ticket: Arc<OperationTicket>, // SAME original ticket shared with native IO, never reconstruct native authority from an exposed operation identifier.
} // This wrapper does not implement any service or mint a service handle.
impl ServiceOperation {
    // Expose immutable input to concrete native service adapters in this crate.
    pub(crate) fn request(&self) -> &HostRequest {
        &self.request
    } // Borrow immutable binary planes without separating their original-root admission.
    pub(crate) fn operation_id(&self) -> u64 {
        self.ticket.operation_id()
    } // Native diagnostics may correlate this ID but cannot use it as authority.
} // Cancellation may signal request().stop_token(), but actual job ownership still determines terminal settlement.

// Observations describe the broker's current actual grant, never the requested
// ceiling. A native binding identity is not a registered guest AssetHandle and
// must not be serialized as one; the asset owner separately publishes that handle.
fn permission_observation(grant: &crate::permissions::Grant, epoch: u64) -> Result<Value> {
    if epoch == 0 || epoch > 9_007_199_254_740_991 {
        return Err(AnimationError::PermissionDenied(
            "native permission observation epoch is outside the SDK range".into(),
        ));
    }
    Ok(serde_json::json!({
        "request_id": grant.request_id,
        "id": grant.right.id,
        "scope": grant.right.scope,
        "epoch": epoch,
    }))
}

fn permission_error(error: crate::permissions::PermissionError) -> AnimationError {
    AnimationError::PermissionDenied(error.to_string())
}

fn lock_broker(owner: &Mutex<PermissionBroker>) -> Result<MutexGuard<'_, PermissionBroker>> {
    owner.lock().map_err(|_| {
        AnimationError::PermissionDenied(
            "native permission authority poisoned; activation refused".into(),
        )
    })
}
fn authority_from(broker: &PermissionBroker, active: &Activation) -> Result<ServiceAuthority> {
    broker
        .grant(&active.channel, "_host_channel_check")
        .map_err(permission_error)?;
    Ok(ServiceAuthority {
        instance_id: active.plan.instance_id,
        plan_generation: active.plan.plan_revision,
        authorization_epoch: active.plan.authorization_epoch,
    })
}

fn validate_baseline_request(request: &HostRequest) -> Result<()> {
    if request.is_cancelled()
        || !matches!(
            request.method.as_str(),
            "compute.submit" | "compute.result" | "compute.status" | "compute.close"
        )
    {
        return Err(AnimationError::PermissionDenied(
            "invalid baseline math operation".into(),
        ));
    }
    Ok(())
}
fn validate_request(
    digest: &str,
    request: &HostRequest,
    authority: ServiceAuthority,
) -> Result<ServiceAuthority> {
    if request.package_digest != digest || request.authority != authority {
        return Err(AnimationError::PermissionDenied(
            "service request does not match current native activation".into(),
        ));
    }
    Ok(authority)
}

fn constrained_limits(
    manifest: &crate::manifest::RuntimeLimits,
    mut host: HelperLimits,
) -> Result<HelperLimits> {
    let bytes = |value| {
        usize::try_from(value).map_err(|_| AnimationError::Budget("manifest resource size".into()))
    };
    host.engine.heap_bytes = host.engine.heap_bytes.min(bytes(manifest.heap_bytes)?);
    host.engine.backing_bytes = host.engine.backing_bytes.min(bytes(manifest.heap_bytes)?);
    host.engine.frame_bytes = host.engine.frame_bytes.min(bytes(manifest.frame_bytes)?);
    host.engine.render_ms = host.engine.render_ms.min(manifest.render_ms);
    host.engine.preparation_ms = host.engine.preparation_ms.min(manifest.preparation_ms);
    Ok(host)
}

impl PackageInstance {
    /// Original quota identity; copied settings or metadata cannot create it.
    pub fn shares_root(&self, quota: &QuotaGroup) -> bool {
        self.quota.shares_root(quota)
    }
    /// Native caller scratch follows the same original instance root.
    pub fn reserve_input_seed_storage(&self, bytes: usize) -> Result<StorageAdmission> {
        self.live_helper_authority()?;
        let maximum = self
            .engine_limits()
            .frame_bytes
            .checked_add(128 * 1024)
            .ok_or_else(|| AnimationError::Budget("input seed capacity overflow".into()))?;
        if bytes > maximum {
            return Err(AnimationError::Budget(
                "input seed copy exceeds original capacity".into(),
            ));
        }
        self.quota
            .reserve_external_storage(bytes)
            .map_err(|error| AnimationError::Budget(format!("input seed copy: {error:?}")))
    }
    pub fn settings(&self) -> &Value {
        &self.settings
    }

    /// Fresh live helper authority for input preparation in either animation mode.
    /// This check does not authorize acquisition or a later copy/publication.
    pub fn check_live_input_authority(&self, expected: &HelperAuthority) -> Result<()> {
        let active = self.live_helper_authority()?;
        let current = HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: active.instance_id,
            plan_generation: active.plan_generation,
            authorization_epoch: active.authorization_epoch,
        };
        if &current != expected {
            return Err(AnimationError::PermissionDenied(
                "input owner activation changed".into(),
            ));
        }
        Ok(())
    }

    /// Inspect native pending review coordinates while the original broker is held.
    /// The bounded native closure must not execute JS, perform IO, wait for jobs,
    /// acquire services, or call back into this broker.
    pub fn with_permission_review_state<T>(
        &self,
        instance_id: u64,
        inspect: impl FnOnce(&Package, &crate::permissions::PackageIdentity, u64, u64, u64) -> T,
    ) -> Result<T> {
        if instance_id != self.helper.authority().instance_id
            || self.activation.is_some()
            || self.helper_retired
            || self.pending_resolution.is_some()
        {
            return Err(AnimationError::PermissionDenied(
                "review owner unavailable".into(),
            ));
        }
        let broker = lock_broker(&self.broker)?;
        let (revision, epoch) = broker
            .pending_review_coordinates(instance_id)
            .map_err(permission_error)?;
        Ok(inspect(
            &self.package,
            broker.identity(),
            instance_id,
            revision,
            epoch,
        ))
    }
    /// Retain exact verified native identity before any helper or permission review.
    pub fn verify(preparation: InstancePreparation<'_>) -> Result<VerifiedPreparation> {
        let InstancePreparation {
            archive,
            verifier,
            helper_executable,
            trusted_bootstrap,
            settings,
            mode,
            environment,
            host_policy,
            instance_id,
            limits,
            quota,
        } = preparation;
        if instance_id == 0
            || trusted_bootstrap.len() > 1024 * 1024
            || helper_executable.as_os_str().len() > 16 * 1024
        {
            return Err(AnimationError::Budget(
                "native preparation identity or metadata".into(),
            ));
        }
        environment_bytes(environment, 0)?;
        let package_limits = PackageLimits::default();
        if archive.len() as u64 > package_limits.archive_bytes {
            return Err(AnimationError::Budget("archive size".into()));
        }
        let expanded = usize::try_from(package_limits.expanded_bytes)
            .map_err(|_| AnimationError::Budget("package expansion size".into()))?;
        // Archive/path/bootstrap/environment copies and retained package have an
        // original-root debit BEFORE allocation, including suspended ledger load.
        let bytes = expanded
            .checked_add(archive.len())
            .and_then(|value| value.checked_add(2 * 1024 * 1024))
            .ok_or_else(|| AnimationError::Budget("native preparation storage overflow".into()))?;
        let storage = quota
            .reserve_external_storage(bytes)
            .map_err(|error| AnimationError::Budget(format!("package admission: {error:?}")))?;
        let package = Arc::new(Package::from_bytes(archive, package_limits)?);
        let limits = constrained_limits(&package.manifest().limits, limits)?;
        if !package.manifest().modes.contains(&mode) {
            return Err(AnimationError::Runtime(
                "package does not support this mode".into(),
            ));
        }
        let settings = crate::settings::validate_settings(&package.manifest().settings, settings)?;
        let identity = verifier.verify(&package);
        let principal = verifier.permission_identity(&package)?;
        let manifest = Ceiling {
            permissions: package
                .manifest()
                .capabilities
                .iter()
                .map(crate::permission_projection::right)
                .collect::<Result<Vec<_>>>()?,
        };
        let broker =
            PermissionBroker::new(principal, manifest, host_policy).map_err(permission_error)?;
        Ok(VerifiedPreparation {
            archive: archive.to_vec(),
            helper_executable: helper_executable.to_owned(),
            trusted_bootstrap: trusted_bootstrap.to_owned(),
            environment: environment.clone(),
            package,
            identity,
            broker,
            settings,
            mode,
            instance_id,
            limits,
            quota,
            _storage: storage,
        })
    }

    /// A picker changes the reviewed binding, so it always consumes a fresh
    /// review revision rather than modifying a grant returned to the script.
    pub fn review_selected(
        &mut self,
        instance_id: u64,
        revision: u64,
        bindings: BTreeMap<String, HostBinding>,
    ) -> Result<PlanReview> {
        if instance_id != self.helper.authority().instance_id
            || self.activation.is_some()
            || self.helper_retired
            || self.pending_resolution.is_some()
        {
            // A logically retired helper cannot consume a new native review.
            return Err(AnimationError::PermissionDenied(
                "selection review does not match the inactive package instance".into(),
            ));
        }
        lock_broker(&self.broker)?
            .prepare(
                instance_id,
                revision,
                self.projection.permission_plan.clone(),
                bindings,
            )
            .map_err(permission_error)
    }

    /// Resolve genuine review into privately held authority, without create,
    /// host seeding, dispatch, or frame publication. Caller handles invalidation.
    pub fn begin_resolution(
        &mut self,
        review: PlanReview,
        answers: BTreeMap<String, UserChoice>,
    ) -> Result<PendingResolution> {
        if self.activation.is_some() || self.helper_retired || self.pending_resolution.is_some() {
            return Err(AnimationError::PermissionDenied(
                "resolution requires an inactive native owner".into(),
            ));
        }
        let instance_id = review.instance_id();
        let plan_revision = review.plan_revision();
        let remembered = answers.values().any(|choice| {
            matches!(
                choice,
                UserChoice::AllowRemembered | UserChoice::DenyRemembered
            )
        });
        let storage = self
            .quota
            .reserve_external_storage(384 * 1024)
            .map_err(|error| {
                AnimationError::Budget(format!("pending permission admission: {error:?}"))
            })?;
        let mut broker = lock_broker(&self.broker)?;
        let resolution = broker.resolve(review, answers).map_err(permission_error)?;
        let issuer = Arc::new(());
        self.pending_resolution = Some(Arc::clone(&issuer));
        let (candidate, persistence_error) = if remembered {
            match broker.export_remembered() {
                Ok(bytes) => (Some(bytes), None),
                Err(error) => (None, Some(permission_error(error))),
            }
        } else {
            (None, None)
        };
        let epoch = broker.authorization_epoch();
        drop(broker);
        // Even failed export retains the original consent invalidation in this
        // genuine token; caller receives cleanup instead of a lost Error path.
        let digest = candidate.as_ref().map(|bytes| Sha256::digest(bytes).into());
        Ok(PendingResolution {
            issuer,
            instance_id,
            plan_revision,
            epoch,
            activation: resolution.activation,
            invalidation: resolution.invalidation,
            denied_required: resolution.denied_required,
            candidate,
            persistence_error,
            digest,
            _storage: storage,
        })
    }

    /// Trusted host-only continuation. The controller keeps this noncloneable
    /// token private and consumes it ONLY after checking its actual native durable
    /// receipt (if bytes exist). No helper/JS callback may expose this method.
    pub fn finish_resolution(&mut self, pending: PendingResolution) -> Result<InstanceResolution> {
        if pending.persistence_error.is_some()
            || self.helper_retired
            || self.activation.is_some()
            || lock_broker(&self.broker)?.authorization_epoch() != pending.epoch
            || !self
                .pending_resolution
                .as_ref()
                .is_some_and(|issuer| Arc::ptr_eq(issuer, &pending.issuer))
        {
            return Err(AnimationError::PermissionDenied(
                "stale pending native resolution".into(),
            ));
        }
        self.pending_resolution = None;
        // Retain candidate/guard through the complete startup/denial result.
        let resolution = crate::permissions::Resolution {
            activation: pending.activation,
            invalidation: pending.invalidation,
            denied_required: pending.denied_required,
        };
        let Some(activation) = resolution.activation else {
            // Required denial must never leave the helper able to create an instance.
            self.creation = None; // There is no accepted script instance.
            self.activation = None; // The broker already invalidated any previous native activation.
            let teardown_error = self.retire_helper().err(); // Report physical retirement failure without claiming that the helper exited.
            return Ok(InstanceResolution {
                invalidation: resolution.invalidation,
                denied_required: resolution.denied_required,
                creation: None,
                teardown_error,
                activation_invalidation: None,
                authority_error: None,
                creation_error: None,
            }); // Return every consent and teardown obligation together.
        }; // An accepted native channel is now owned by this activation.
        self.activation = Some(activation); // Store native authority before every fallible projection, seed, or create operation.
        let creation = (|| {
            let owner = Arc::clone(&self.broker);
            let broker = lock_broker(&owner)?;
            let active = self.activation.as_ref().ok_or_else(|| {
                AnimationError::PermissionDenied("native activation missing".into())
            })?;
            let authority = authority_from(&broker, active)?;
            self.plan = self
                .projection
                .prune_inputs(&broker, &active.channel, &self.plan)?;
            let mut accepted = serde_json::to_value(&self.plan)?;
            let mut accepted_requests = Vec::new();
            let mut observed_grants = Vec::new();
            for request in &self.projection.permission_plan.permissions {
                let id = request.request_id.as_deref().ok_or_else(|| {
                    AnimationError::PermissionDenied("native request identity missing".into())
                })?;
                if let Some(grant) = broker
                    .grant(&active.channel, id)
                    .map_err(permission_error)?
                {
                    accepted_requests.push(request);
                    observed_grants.push(permission_observation(
                        grant,
                        active.plan.authorization_epoch,
                    )?);
                }
            }
            accepted["permissions"] = serde_json::to_value(&accepted_requests)?;
            accepted["generation"] = Value::from(active.plan.plan_revision);
            accepted["authorization_epoch"] = Value::from(active.plan.authorization_epoch);
            drop(broker);
            // Recheck SAME owner through inert binding ACK. Deferred retirement
            // occurs only after callback-local broker guard has dropped.
            self.helper.with_native_publication(|helper| {
                let broker = lock_broker(&owner)?;
                authority_from(&broker, active)?;
                helper.bind_service_authority(authority)
            })?;
            self.live_helper_authority()?;
            let seed = self.helper.with_native_publication(|helper| {
                let broker = lock_broker(&owner)?;
                authority_from(&broker, active)?;
                helper.prepare_frame_seed(&serde_json::json!({"frame": null, "host": {"permissions": observed_grants, "cancelled": false, "package": {"id": self.identity.id(), "version": self.package.manifest().version, "digest": self.identity.digest(), "verified_ilium": self.identity.is_ilium()}, "bundle": "bundle", "selection": {"generation": active.plan.plan_revision, "authorization_epoch": active.plan.authorization_epoch}}}), &[], &BTreeMap::new())
            })?;
            self.activate_frame_seed(seed)?;
            // JS execution is outside the serialized native authority. All resulting
            // effects/requests/publication still independently require current gates.
            self.live_helper_authority()?;
            self.helper.start_create(&self.settings, &accepted)
        })();
        let creation = match creation {
            // Preserve both consent and cleanup inventories on partial startup failure.
            Ok(creation) => creation, // A successfully started instance can now be pumped under its private channel.
            Err(error) => {
                // Native acceptance may have succeeded even when the child could not finish startup.
                let (activation_invalidation, authority_error) = match self.revoke_activation() {
                    Ok(invalidation) => (invalidation, None),
                    Err(error) => (None, Some(error)),
                }; // Retire the exact stored native activation before returning startup failure.
                let teardown_error = self.retire_helper().err(); // Independently require physical helper and pipe-worker retirement.
                return Ok(InstanceResolution {
                    invalidation: resolution.invalidation,
                    denied_required: resolution.denied_required,
                    creation: None,
                    teardown_error,
                    activation_invalidation,
                    authority_error,
                    creation_error: Some(error),
                }); // Caller must process both invalidations and report both independent failures.
            } // Duplicate operation IDs across invalidations denote the same native cleanup obligation.
        }; // Native activation remains current only after startup succeeded.
        self.creation = Some(creation); // Retain the actual child creation state.
        Ok(InstanceResolution {
            invalidation: resolution.invalidation,
            denied_required: resolution.denied_required,
            creation: Some(creation),
            teardown_error: None,
            activation_invalidation: None,
            authority_error: None,
            creation_error: None,
        }) // Startup success does not claim a service dispatcher exists.
    } // End the native activation transaction.
    fn current_service_authority(&self) -> Result<ServiceAuthority> {
        let broker = lock_broker(&self.broker)?;
        self.current_service_authority_locked(&broker)
    }
    fn current_service_authority_locked(
        &self,
        broker: &PermissionBroker,
    ) -> Result<ServiceAuthority> {
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("instance is not accepted".into()))?;
        authority_from(broker, active)
    }
    /// Existing root math operation gate: bounded issue under SAME authority,
    /// no callback may recursively lock owner/run JS/pump/wait for worker exit.
    pub(crate) fn with_baseline_math<T>(
        &mut self,
        request: &HostRequest,
        issue: impl FnOnce(ServiceAuthority) -> Result<T>,
    ) -> Result<T> {
        if self.helper_retired {
            return Err(AnimationError::PermissionDenied("helper retired".into()));
        }
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        validate_baseline_request(request)?;
        issue(authority)
    }
    /// Original native receipt polling only, including after helper retirement.
    /// No request/effect/service acquisition is permitted in this callback.
    pub(crate) fn with_native_math_authority<T>(
        &mut self,
        poll: impl FnOnce(ServiceAuthority) -> Result<T>,
    ) -> Result<T> {
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        poll(authority)
    }
    pub(crate) fn complete_baseline_math(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        if self.helper_retired {
            return Err(AnimationError::PermissionDenied("helper retired".into()));
        }
        validate_baseline_request(request)?;
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        })
    }
    /// Constructor is native-only, with a genuine channel cloned only AFTER
    /// durable pending resolution was finished. The factory rechecks at issue.
    #[cfg(feature = "native-host")]
    pub(crate) fn audio_capture_factory(
        &self,
        selected: crate::native_audio_capture::QualifiedCaptureBinding,
        demand_id: String,
    ) -> Result<crate::native_audio_capture::NativeAudioCaptureFactory> {
        if self.helper_retired {
            return Err(AnimationError::PermissionDenied("helper retired".into()));
        }
        let broker = lock_broker(&self.broker)?;
        self.current_service_authority_locked(&broker)?;
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let channel = active.channel.clone();
        drop(broker);
        crate::native_audio_capture::NativeAudioCaptureFactory::new(
            selected,
            Arc::clone(&self.broker),
            channel,
            demand_id,
            self.quota.clone(),
        )
    }
    fn live_helper_authority(&self) -> Result<ServiceAuthority> {
        // Guard every runtime path that could execute package code or issue new work.
        if self.helper_retired {
            return Err(AnimationError::Runtime(
                "helper retirement has begun".into(),
            ));
        } // Failed physical teardown cannot make the helper logically usable again.
        self.current_service_authority() // Recheck the native channel even when no privileged grants were requested.
    } // Retained native playback uses current_service_authority directly after independent physical proof.
    fn validate_service_request(&self, request: &HostRequest) -> Result<ServiceAuthority> {
        // Bind a retained request to its original native package activation.
        let authority = self.current_service_authority()?; // A previous epoch or retired broker channel fails before native effects or publication.
        self.validate_service_request_locked(request, authority)?;
        Ok(authority)
    }
    fn validate_service_request_locked(
        &self,
        request: &HostRequest,
        authority: ServiceAuthority,
    ) -> Result<ServiceAuthority> {
        validate_request(self.package.digest(), request, authority)
    }
    pub fn pump(&mut self) -> Result<CreateState> {
        // Only a fresh native-authorized pump may execute completion continuations.
        self.live_helper_authority()?; // Preserve the native channel check after consent changes and helper retirement.
        let state = self.helper.pump()?; // The helper command performs the only service-reaction checkpoint.
        self.creation = Some(state); // Record the actual resulting creation state.
        Ok(state) // This operation can expose queued requests but never authorizes their native effects itself.
    } // Complete and diagnostic paths remain separate from this pump.
    pub fn requests(&mut self) -> Result<Vec<HostRequest>> {
        // Return only retained requests belonging to the currently accepted native activation.
        let authority = self.live_helper_authority()?; // Do not drain requests for a stale or logically retired helper.
        let requests = self.helper.take_requests(); // Move admitted request ownership without deep-copying binary planes.
        if requests.iter().any(|request| {
            request.package_digest != self.package.digest() || request.authority != authority
        }) {
            // Validate the complete batch before publishing any request to the dispatcher.
            for request in &requests {
                request.stop_token().stop();
            } // Signal all affected native request owners without claiming that any body exited.
            let retirement = self.retire_helper(); // A mixed or stale authenticated response retires its helper only.
            return Err(AnimationError::PermissionDenied(format!(
                "helper request activation mismatch; helper retirement: {retirement:?}"
            ))); // Native activation and unrelated native owners remain independently owned.
        } // No request reaches a native service before the whole batch's authority has been checked.
        Ok(requests
            .into_iter()
            .filter(|request| !request.is_cancelled())
            .collect()) // Suppress expired or cancelled unissued work while surviving aliases keep their leases.
    } // Actual dispatch still derives endpoint requirements and obtains a private operation ticket.
    pub fn take_cancelled_requests(&mut self) -> Vec<HostRequest> {
        // Drain terminal notifications even after logical retirement or native revocation.
        self.helper.take_cancelled_requests() // These immutable records signal cancellation and do not establish actual worker termination.
    } // Native operation owners retain their own receipts until real completion.
    pub(crate) fn dispatch_service(
        &mut self,
        request: HostRequest,
        demand_id: &str,
        needs: Vec<OperationNeed>,
    ) -> Result<ServiceOperation> {
        self.dispatch_service_selected(request, |_, broker, channel, phase| {
            broker
                .dispatch(channel, phase, demand_id, needs)
                .map_err(permission_error)
        })
    }
    fn dispatch_service_selected(
        &mut self,
        request: HostRequest,
        select: impl FnOnce(
            &AuthorizationProjection,
            &mut PermissionBroker,
            &crate::permissions::Channel,
            CallPhase,
        ) -> Result<OperationTicket>,
    ) -> Result<ServiceOperation> {
        self.live_helper_authority()?;
        self.validate_service_request(&request)?;
        if !request.payload.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign native service request root".into(),
            ));
        }
        if request.is_cancelled() {
            return Err(AnimationError::Runtime(
                "service request is already terminal".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let phase = match request.phase {
            ServicePhase::Create => CallPhase::Create,
            ServicePhase::Async => CallPhase::Async,
        };
        let mut broker = lock_broker(&self.broker)?;
        let ticket = select(&self.projection, &mut broker, &active.channel, phase)?;
        drop(broker);
        Ok(ServiceOperation {
            request,
            ticket: Arc::new(ticket),
        })
    }
    /// Original operation factory prepared BEFORE actual finite IO submission.
    /// Only original commit_service(... actual client.try_submit ...) marks issued.
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn http_authority_factory(
        &self,
        operation: &ServiceOperation,
        credentials: Option<Box<dyn crate::native_http_authority::HostCredentialAdapter>>,
    ) -> Result<crate::native_http_authority::NativeHttpAuthorityFactory> {
        self.live_helper_authority()?;
        self.validate_service_request(&operation.request)?;
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        crate::native_http_authority::NativeHttpAuthorityFactory::from_native(
            Arc::clone(&self.broker),
            active.channel.clone(),
            Arc::clone(&operation.ticket),
            operation.request.clone(),
            self.quota.clone(),
            credentials,
        )
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn source_planning_authority(
        &self,
        request: &HostRequest,
        quota: &QuotaGroup,
    ) -> Result<crate::native_source_host::SourcePlanningAuthority> {
        self.live_helper_authority()?;
        self.validate_service_request(request)?;
        if !self.quota.shares_root(quota)
            || !request.payload.shares_root(quota)
            || !crate::native_source_host::SourceCall::recognized(&request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "source planning original owner/call mismatch".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("source activation missing".into()))?;
        crate::native_source_host::SourcePlanningAuthority::from_runtime(
            Arc::clone(&self.broker),
            active.channel.clone(),
            request.clone(),
            self.quota.clone(),
        )
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_baseline_source<T>(
        &mut self,
        request: &HostRequest,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        if !request.payload.shares_root(&self.quota)
            || !crate::native_source_host::SourceCall::recognized(&request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "closed native source planning call".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        self.validate_service_request_locked(request, authority)?;
        if request.is_cancelled() {
            return Err(AnimationError::PermissionDenied(
                "source planning cancelled".into(),
            ));
        }
        Ok(issue()) // Actual bounded original CPU queue issue only; no transport, credential, JS, wait or baseline grant.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_source_http_service(
        &mut self,
        request: HostRequest,
        need: OperationNeed,
    ) -> Result<ServiceOperation> {
        if !crate::native_source_host::SourceCall::recognized(&request.method) {
            return Err(AnimationError::PermissionDenied(
                "closed native source transport call".into(),
            ));
        }
        self.dispatch_service_selected(request, |projection, broker, channel, phase| {
            projection.dispatch_http(broker, channel, phase, need)
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    fn with_current_source_operation<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce(&PermissionBroker, &crate::permissions::Channel) -> Result<T>,
    ) -> Result<T> {
        self.live_helper_authority()?;
        self.validate_service_request(&operation.request)?;
        if operation.request.is_cancelled()
            || !crate::native_source_host::SourceCall::recognized(&operation.request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "source operation lifetime/call".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("source activation missing".into()))?;
        let broker = lock_broker(&self.broker)?;
        broker
            .check_operation_lineage(&operation.ticket, &active.channel)
            .map_err(permission_error)?;
        issue(&broker, &active.channel) // SAME owner through bounded native issue; no JS/wait/network under guard.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_unissued_authority<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_current_source_operation(operation, |_, _| issue()) // Pure bounded cadence only, NEVER commit or issue IO.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_cpu_authority<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.with_current_source_operation(operation, |broker, channel| {
            broker
                .check_committed_operation(&operation.ticket, channel)
                .map_err(permission_error)?;
            Ok(issue()) // Actual original CPU submit, no nested wait.
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_source_http_authority<T>(
        &self,
        operation: &ServiceOperation,
        needs: &[OperationNeed],
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.with_current_source_operation(operation, |broker, channel| {
            broker
                .check_committed_needs(&operation.ticket, channel, needs)
                .map_err(permission_error)?;
            Ok(issue()) // Additional exact native provider page under SAME original committed operation.
        })
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn complete_baseline_source(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        self.live_helper_authority()?;
        if request.is_cancelled()
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
            || !crate::native_source_host::SourceCall::recognized(&request.method)
        {
            return Err(AnimationError::PermissionDenied(
                "source baseline publication root/call".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let digest = self.package.digest();
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("source activation missing".into()))?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            if request.is_cancelled() {
                return Err(AnimationError::PermissionDenied(
                    "source publication cancelled".into(),
                ));
            }
            helper.complete_service_request(request.id, authority, value)
        }) // Actual inert copy ACK; no helper JS/checkpoint under original native owner.
    }

    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn native_http_owner(
        &self,
        quota: &QuotaGroup,
    ) -> Result<Arc<Mutex<PermissionBroker>>> {
        self.live_helper_authority()?;
        if !self.quota.shares_root(quota) {
            return Err(AnimationError::PermissionDenied(
                "HTTP original-root mismatch".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("HTTP activation missing".into()))?;
        {
            let broker = lock_broker(&self.broker)?;
            authority_from(&broker, active)?;
        }
        Ok(Arc::clone(&self.broker))
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn check_http_owner(
        &self,
        owner: &Arc<Mutex<PermissionBroker>>,
        quota: &QuotaGroup,
    ) -> Result<()> {
        if !Arc::ptr_eq(owner, &self.broker) || !quota.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "foreign HTTP native owner".into(),
            ));
        }
        Ok(()) // Owner identity only; every issue/publication separately authenticates current authority.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn dispatch_http_service(
        &mut self,
        request: HostRequest,
        need: OperationNeed,
    ) -> Result<ServiceOperation> {
        if request.method != "http.request" {
            return Err(AnimationError::PermissionDenied(
                "invalid native HTTP request".into(),
            ));
        }
        self.dispatch_service_selected(request, |projection, broker, channel, phase| {
            projection.dispatch_http(broker, channel, phase, need)
        }) // SAME dispatch_service owner path; only the native dependency selector differs.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn with_http_operation_authority<T>(
        &self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        self.live_helper_authority()?;
        self.validate_service_request(&operation.request)?;
        if operation.request.method != "http.request" || operation.request.is_cancelled() {
            return Err(AnimationError::PermissionDenied(
                "HTTP decode issue cancelled".into(),
            ));
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("HTTP activation missing".into()))?;
        let broker = lock_broker(&self.broker)?;
        broker
            .check_committed_operation(&operation.ticket, &active.channel)
            .map_err(permission_error)?;
        Ok(issue()) // SAME original ticket; bounded pure CPU queue issue only, no second commit or nested wait.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn complete_http_refusal(
        &mut self,
        request: &HostRequest,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        if request.method != "http.request"
            || !request.payload.shares_root(&self.quota)
            || !value.shares_root(&self.quota)
            || value.metadata().get("ok").and_then(Value::as_bool) != Some(false)
            || !value.arrays().is_empty()
        {
            return Err(AnimationError::PermissionDenied(
                "HTTP refusal schema".into(),
            ));
        }
        self.live_helper_authority()?;
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("HTTP activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, request, authority)?;
            helper.complete_service_request(request.id, authority, value)
        }) // Nonacquiring fixed native error only; SAME guard through inert copy/ACK, no minted effect ticket.
    }
    #[cfg(all(feature = "native-host", feature = "native-network"))]
    pub(crate) fn settle_http_terminal(&mut self, operation: &ServiceOperation) -> Result<()> {
        // Caller MUST own real terminal outcome/rejected input. Lost/cancel signal is not terminal proof.
        let mut broker = lock_broker(&self.broker)?;
        match broker.settle_without_delivery(&operation.ticket) {
            Ok(()) | Err(crate::permissions::PermissionError::UnknownOperation) => Ok(()), // Genuine issuer already settled by final publication.
            Err(error) => Err(permission_error(error)), // Foreign/poisoned authority cannot be reset.
        }
    }

    pub(crate) fn commit_service<T>(
        &mut self,
        operation: &ServiceOperation,
        issue: impl FnOnce() -> T,
    ) -> Result<T> {
        // Commit only a concrete bounded native effect prepared by the service owner.
        self.live_helper_authority()?; // Logical helper retirement prevents new effects while retained playback remains authorized separately.
        self.validate_service_request(&operation.request)?; // Recheck the original package and activation after queue residence.
        if operation.request.is_cancelled() {
            return Err(AnimationError::Runtime(
                "service request is already terminal".into(),
            ));
        } // A cancellation signal prevents uncommitted work from issuing.
        lock_broker(&self.broker)?
            .commit(&operation.ticket, issue)
            .map_err(permission_error) // Native issue must be bounded and must not wait for I/O or run JavaScript.
    } // Actual body lifetime remains with the native job/receipt, not the committed ticket alone.
    pub(crate) fn complete_authorized(
        &mut self,
        operation: &ServiceOperation,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        // Call only after the actual native body has finished producing its admitted immutable result.
        let owner = Arc::clone(&self.broker);
        if self.helper_retired {
            lock_broker(&owner)?
                .settle_without_delivery(&operation.ticket)
                .map_err(permission_error)?;
            return Ok(CompletionState::Cancelled);
        }
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        let digest = self.package.digest();
        self.helper.with_native_publication(|helper| {
            let mut broker = lock_broker(&owner)?;
            let authority = authority_from(&broker, active)?;
            validate_request(digest, &operation.request, authority)?;
            broker
                .deliver(&operation.ticket, || {
                    helper.complete_service_request(operation.request.id, authority, value)
                })
                .map_err(permission_error)?
        })
    }
    pub(crate) fn settle_service(&mut self, operation: &ServiceOperation) -> Result<()> {
        // Call only after native failure/cancellation has actually completed or a committed result is intentionally discarded.
        lock_broker(&self.broker)?
            .settle_without_delivery(&operation.ticket)
            .map_err(permission_error) // Stale authorization can be settled without releasing the external job's remaining guards.
    } // Cancellation notification, helper EOF, and a dropped waiter are not evidence that this precondition holds.
    pub fn render(&mut self, context: &Value, arrays: &[ArraySpec]) -> Result<RetainedHelperFrame> {
        self.live_helper_authority()?; // Rendering requires both a current native channel and a logically live helper.
        if self.creation != Some(CreateState::Ready) {
            return Err(AnimationError::Runtime(
                "animation preparation is pending".into(),
            ));
        }
        self.helper.render_retained(context, arrays)
    }

    /// Native typed copy/ACK under original broker guard, never a JS seed hook.
    pub fn prepare_frame_seed(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<PreparedSeed> {
        self.live_helper_authority()?;
        if self.creation != Some(CreateState::Ready) {
            return Err(AnimationError::Runtime(
                "animation preparation is pending".into(),
            ));
        }
        let owner = Arc::clone(&self.broker);
        let active = self
            .activation
            .as_ref()
            .ok_or_else(|| AnimationError::PermissionDenied("native activation missing".into()))?;
        self.helper.with_native_publication(|helper| {
            let broker = lock_broker(&owner)?;
            authority_from(&broker, active)?;
            helper.prepare_frame_seed(metadata, arrays, planes)
        })
    }
    /// Actual native receipt only; recheck current original activation then
    /// release authority guard BEFORE trusted JS hook (never pump here).
    pub fn activate_frame_seed(&mut self, seed: PreparedSeed) -> Result<()> {
        let check = self.live_helper_authority().and_then(|authority| {
            if authority != seed.authority() {
                return Err(AnimationError::PermissionDenied(
                    "stale native seed activation".into(),
                ));
            }
            Ok(())
        });
        if let Err(error) = check {
            let discard = self.helper.discard_frame_seed(seed);
            return Err(match discard {
                Ok(()) => error,
                Err(discard) => AnimationError::Runtime(format!(
                    "seed activation refused: {error}; discard failed: {discard}"
                )),
            });
        }
        self.helper.activate_frame_seed(seed)
    }
    pub fn discard_frame_seed(&mut self, seed: PreparedSeed) -> Result<()> {
        self.helper.discard_frame_seed(seed) // Only native cleanup; no grant or hook.
    }
    pub fn seed_frame(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let seed = self.prepare_frame_seed(metadata, arrays, planes)?;
        self.activate_frame_seed(seed)
    }

    /// Current native coordinates are advisory until with_frame_authority or with_playback_authority protects actual emission.
    pub fn frame_authority(&self) -> Option<HelperAuthority> {
        // Keep publication coordinates independent from the immutable IPC session stamp.
        let active = self.current_service_authority().ok()?; // Authenticate the broker's private channel even after physical helper retirement.
        Some(HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: active.instance_id,
            plan_generation: active.plan_generation,
            authorization_epoch: active.authorization_epoch,
        }) // These copied fields cannot authorize a later handoff by themselves.
    } // Retained frame ownership and source provenance remain the native presentation owner's responsibility.
    pub fn with_frame_authority<T>(
        &mut self,
        expected: &HelperAuthority,
        emit: impl FnOnce() -> T,
    ) -> Result<T> {
        // Run the actual bounded native frame handoff while the authority owner is exclusively borrowed.
        if self.mode == AnimationMode::PreRendered
            && (!self.helper_retired || !self.helper.is_physically_retired())
        {
            return Err(AnimationError::Runtime(
                "pre-rendered emission requires confirmed physical helper retirement".into(),
            ));
        } // Every public emission gate enforces the native-selected mode, including callers that choose this common path.
        let owner = Arc::clone(&self.broker);
        let broker = lock_broker(&owner)?;
        let authority = self.current_service_authority_locked(&broker)?;
        let current = HelperAuthority {
            package_digest: self.package.digest().into(),
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        }; // Recheck issuer, native nonce, revision, and epoch at the emission boundary.
        if &current != expected {
            return Err(AnimationError::PermissionDenied(
                "retained frame belongs to a different native activation".into(),
            ));
        } // Match the native retained frame's original package and activation coordinates.
        Ok(emit()) // The closure must perform the final native handoff; queued later consumption requires another current-authority check.
    } // This native closure may not execute helper code, acquire services, or invent protected replay provenance.
    pub fn with_playback_authority<T>(
        &mut self,
        expected: &HelperAuthority,
        emit: impl FnOnce() -> T,
    ) -> Result<T> {
        // Permit pre-rendered playback only after physical V8 helper retirement is proved.
        if !self.helper_retired || !self.helper.is_physically_retired() {
            return Err(AnimationError::Runtime(
                "pre-rendered playback requires confirmed physical helper retirement".into(),
            ));
        } // Pipe closure, cancellation notification, and a failed prior join never satisfy this gate.
        self.with_frame_authority(expected, emit) // Retain and recheck the independently owned native activation through every actual playback emission.
    } // Native clip eligibility, protected source ownership, and retained bytes remain separate mandatory checks at the caller.
    pub fn accept_frame(&mut self, accepted: bool) -> Result<()> {
        // Acknowledgement can execute package hooks and therefore needs a current native lifecycle check.
        self.live_helper_authority()?; // Never run acknowledgement JavaScript after logical helper retirement or native revocation.
        self.helper.accept_frame(accepted) // Engine checkpoints still enforce the synchronous acknowledgement acquisition prohibition.
    } // Completion result publication never calls this hook.
    pub fn active_identity(&self) -> Option<&VerifiedIdentity> {
        // Report identity only while the native activation remains current.
        self.current_service_authority().ok()?; // An occupied activation Option alone is not authorization after broker revocation.
        Some(&self.identity) // Identity inspection does not keep the V8 helper alive.
    } // Immutable package metadata remains available separately through package().
    /// Read-only original instance limits, never a newly configured ceiling.
    pub fn engine_limits(&self) -> &crate::engine::EngineLimits {
        self.helper.engine_limits()
    }
    pub fn plan(&self) -> &AnimationPlan {
        &self.plan
    }
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// Read original child/pipe-worker exit evidence; logical retirement alone
    /// never satisfies this predicate.
    pub fn is_physically_retired(&self) -> bool {
        self.helper_retired && self.helper.is_physically_retired()
    }

    pub fn retire_helper(&mut self) -> Result<()> {
        // End package execution without revoking the independently owned native activation.
        self.helper_retired = true; // Logical retirement becomes permanent before signalling or waiting for the child.
        self.creation = None; // The retired V8 instance may never be pumped, rendered, seeded, or acknowledged again.
        self.helper.cancel()?; // Failed shutdown or worker join remains a failed physical retirement on every later call.
        if !self.helper.is_physically_retired() {
            return Err(AnimationError::Runtime(
                "helper cancellation did not prove physical retirement".into(),
            ));
        } // Require explicit child and worker exit evidence even after a nominal cancellation return.
        Ok(()) // Native activation, broker decisions, retained clips, and actual native jobs remain separately owned.
    } // Pre-rendered playback must still use with_playback_authority at actual emission.
    pub fn revoke_activation(&mut self) -> Result<Option<Invalidation>> {
        self.creation = None;
        let had_pending = self.pending_resolution.take().is_some();
        let active = self.activation.take();
        if !had_pending && active.is_none() {
            return Ok(None);
        }
        let instance_id = active
            .as_ref()
            .map_or(self.helper.authority().instance_id, |active| {
                active.plan.instance_id
            });
        // Removing local authority precedes locking; poisoning never restores it.
        let mut broker = lock_broker(&self.broker)?;
        Ok(Some(broker.retire(instance_id)))
    }
    pub fn stop(&mut self) -> StoppedInstance {
        let (invalidation, authority_error) = match self.revoke_activation() {
            Ok(invalidation) => (invalidation, None),
            Err(error) => (None, Some(error)),
        };
        // All mutex guards have exited BEFORE signalling/joining helper workers.
        let cancellation = self.retire_helper();
        StoppedInstance {
            invalidation,
            cancellation,
            authority_error,
        }
    }
}

#[cfg(test)]
mod shared_authority_tests {
    use super::*;
    fn broker() -> PermissionBroker {
        PermissionBroker::new(
            crate::permissions::PackageIdentity::unverified("test".into(), b"shared-owner-fixture")
                .unwrap(),
            Ceiling {
                permissions: vec![],
            },
            Ceiling {
                permissions: vec![],
            },
        )
        .unwrap()
    }
    #[test]
    fn genuine_shared_channel_rechecks_same_owner_and_retirement() {
        let owner = Arc::new(Mutex::new(broker()));
        let other = Arc::clone(&owner);
        let active = {
            let mut broker = lock_broker(&owner).unwrap();
            let review = broker
                .prepare(
                    1,
                    1,
                    crate::permissions::PermissionPlan {
                        permissions: vec![],
                        demands: vec![],
                    },
                    BTreeMap::new(),
                )
                .unwrap();
            broker
                .resolve(review, BTreeMap::new())
                .unwrap()
                .activation
                .unwrap()
        };
        let guard = lock_broker(&owner).unwrap();
        assert_eq!(authority_from(&guard, &active).unwrap().instance_id, 1);
        assert!(matches!(
            other.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        drop(guard);
        let _invalidation = lock_broker(&other).unwrap().retire(1);
        assert!(authority_from(&lock_broker(&owner).unwrap(), &active).is_err());
        assert!(authority_from(&broker(), &active).is_err());
    }
    #[test]
    fn poisoned_shared_authority_is_never_recovered() {
        let owner = Arc::new(Mutex::new(broker()));
        let held = Arc::clone(&owner);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = owner.lock().unwrap();
            panic!("synthetic task-local poisoned authority");
        }));
        assert!(panic.is_err());
        assert!(lock_broker(&owner).is_err());
        assert!(lock_broker(&held).is_err());
    }
    #[test]
    fn original_admission_survives_last_shared_native_owner() {
        let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
            worker_threads: 1,
            worker_bytes: 8192,
        });
        let admission = quota.reserve_external_storage(4096).unwrap();
        let mut broker = broker();
        broker.retain_authority_storage(admission).unwrap();
        let owner = Arc::new(Mutex::new(broker));
        let source_owner = Arc::clone(&owner);
        drop(owner);
        assert_eq!(quota.snapshot().worker_bytes, 4096);
        drop(source_owner);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}

#[cfg(test)]
mod permission_observation_tests {
    use super::*;
    use crate::permissions::{Capability, Grant, Right, Scope};
    #[test]
    fn informational_snapshot_has_actual_scope_epoch_and_no_fabricated_handle() {
        let grant = Grant {
            request_id: "pointer".into(),
            right: Right {
                id: Capability::InputPointer,
                scope: Scope::AnimationViewport,
            },
            binding: None,
        };
        assert_eq!(
            permission_observation(&grant, 7).unwrap(),
            serde_json::json!({
                "request_id":"pointer", "id":"input.pointer",
                "scope":{"kind":"animation_viewport"}, "epoch":7,
            })
        );
        assert!(permission_observation(&grant, 0).is_err());
        assert!(permission_observation(&grant, 9_007_199_254_740_992).is_err());
    }
}

#[cfg(test)]
mod activation_observation_tests {
    use super::*;
    use crate::permissions::{Capability, PackageIdentity, Right, Scope};
    fn resolution(creation: Option<CreateState>) -> InstanceResolution {
        let identity =
            PackageIdentity::unverified("activation-observation".into(), b"fixture").unwrap();
        let mut broker = PermissionBroker::new(
            identity,
            Ceiling {
                permissions: vec![],
            },
            Ceiling {
                permissions: vec![],
            },
        )
        .unwrap();
        let invalidation = broker
            .revoke(Right {
                id: Capability::InputPointer,
                scope: Scope::AnimationViewport,
            })
            .unwrap();
        InstanceResolution {
            invalidation,
            denied_required: vec![],
            creation,
            teardown_error: None,
            activation_invalidation: None,
            authority_error: None,
            creation_error: None,
        }
    }
    #[test]
    fn accepted_creation_preserves_actual_pending_and_ready_states() {
        assert_eq!(resolution(None).accepted_creation(), None);
        assert_eq!(
            resolution(Some(CreateState::Pending)).accepted_creation(),
            Some(CreateState::Pending)
        );
        assert_eq!(
            resolution(Some(CreateState::Ready)).accepted_creation(),
            Some(CreateState::Ready)
        );
    }
    #[test]
    fn every_independent_failure_blocks_even_a_ready_creation_observation() {
        for failure in 0..5 {
            let mut value = resolution(Some(CreateState::Ready));
            match failure {
                0 => value.denied_required.push("pointer".into()),
                1 => {
                    value.creation_error =
                        Some(AnimationError::Runtime("fixture startup failure".into()))
                }
                2 => {
                    value.authority_error =
                        Some(AnimationError::Runtime("fixture authority failure".into()))
                }
                3 => {
                    value.teardown_error =
                        Some(AnimationError::Runtime("fixture teardown failure".into()))
                }
                _ => value.activation_invalidation = Some(resolution(None).invalidation),
            }
            assert_eq!(value.accepted_creation(), None, "failure {failure}");
        }
    }
}

// Test-only forcing of the exact logical-before-physical retirement interval.
// No mutator or additional publication gate exists in a production build.
#[cfg(test)]
mod retirement_boundary_tests {
    #[test]
    #[ignore = "requires actual ILIUM_ANIMATION_HELPER and delegated Linux sandbox"]
    fn logical_retirement_without_physical_exit_refuses_actual_playback_sink() {
        use crate::{
            engine::CreateState,
            helper::{isolation_qualification as fixture, HelperLimits},
            manifest::AnimationMode,
            permissions::Ceiling,
            runtime::{InstancePreparation, PackageInstance},
            trust::TrustVerifier,
        };
        use serde_json::json;
        use std::{collections::BTreeMap, path::Path};
        let bytes = fixture::archive(
            "export function plan(){return {format:'gray32',fps:30,inputs:{}}} export async function create(){if(fixture_seed_calls!==1)throw Error('missing_fixture_seed_activation');return {render(){},dispose(){}}}",
        );
        let trusted_bootstrap = format!(
            "{}\n{}",
            fixture::BOOTSTRAP,
            r#"
            globalThis.fixture_seed_calls = 0;
            globalThis.__ilium_seed_frame = (metadata, planes) => {
                if (metadata.frame !== null || !metadata.host ||
                    !Array.isArray(metadata.host.permissions) || metadata.host.permissions.length !== 0 ||
                    metadata.host.cancelled !== false || metadata.host.package.id !== 'helper-qualification' ||
                    !Number.isSafeInteger(metadata.host.selection.generation) || metadata.host.selection.generation <= 0 ||
                    !Number.isSafeInteger(metadata.host.selection.authorization_epoch) || metadata.host.selection.authorization_epoch <= 0 ||
                    Reflect.ownKeys(planes).length !== 0) throw Error('unexpected_fixture_host_seed');
                if (++fixture_seed_calls !== 1) throw Error('repeated_fixture_seed_activation');
            };
        "#
        );
        let quota = fixture::quota();
        let executable =
            std::env::var("ILIUM_ANIMATION_HELPER").expect("actual helper absolute path required");
        let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
        let settings = json!({});
        let environment = json!({});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &bytes,
            verifier: &verifier,
            helper_executable: Path::new(&executable),
            trusted_bootstrap: &trusted_bootstrap,
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
        assert_eq!(resolution.accepted_creation(), Some(CreateState::Ready));
        let expected = instance.frame_authority().unwrap();
        assert!(!instance.helper.is_physically_retired());
        // Force only the real native logical state assigned by retire_helper BEFORE
        // cancellation/join. Do not fabricate a child, activation, physical proof,
        // grant, successful join or configured producer limit.
        instance.helper_retired = true;
        assert!(!instance.is_physically_retired());
        let mut emitted = 0;
        assert!(instance
            .with_playback_authority(&expected, || {
                emitted += 1;
            })
            .is_err());
        assert_eq!(emitted, 0);
        assert_eq!(instance.frame_authority(), Some(expected));
        // Retire the actual test-owned helper through its ordinary physical path.
        instance.retire_helper().unwrap();
        assert!(instance.is_physically_retired());
        drop(instance);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}
