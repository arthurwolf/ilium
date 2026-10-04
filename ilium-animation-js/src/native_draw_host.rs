//! Worker-owned native draw composition under the genuine frame publication gate.
//! Prepared handles are retained native values, never reconstructed from guest IDs.
use crate::{
    error::{AnimationError, Result},
    native_draw::{DrawAuthority, DrawBinding, DrawLimits, NativeDraw, PreparedBlit},
    native_media::{MediaLimits, NativeMedia},
    runtime::PackageInstance,
    surface::{FrameMeta, NoNativeRenderer, Outcome, Planes, Snapshot, Surface, SurfaceError},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use std::collections::BTreeMap;
const MAX_PREPARED: usize = 64;
struct GuardedDraw<'a> {
    binding: &'a DrawBinding,
    prepared: &'a BTreeMap<String, PreparedBlit>,
}
impl DrawAuthority for GuardedDraw<'_> {
    fn check_frame(&mut self, binding: &DrawBinding) -> std::result::Result<(), SurfaceError> {
        // This capsule is constructed ONLY inside with_frame_authority and
        // cannot escape its callback. The SAME native broker guard remains
        // held across every native command and the final transactional copy.
        if binding != self.binding {
            return Err(SurfaceError::Stale);
        }
        Ok(())
    }
    fn prepared(
        &mut self,
        handle: &str,
        binding: &DrawBinding,
    ) -> std::result::Result<PreparedBlit, SurfaceError> {
        self.check_frame(binding)?;
        self.prepared
            .get(handle)
            .cloned()
            .ok_or(SurfaceError::Invalid("unknown prepared native handle"))
    }
}
pub struct NativeDrawHost {
    media: NativeMedia,
    prepared: BTreeMap<String, PreparedBlit>,
    quota: QuotaGroup,
    limits: DrawLimits,
    _metadata: StorageAdmission,
}
impl NativeDrawHost {
    pub fn new(quota: QuotaGroup, media_limits: MediaLimits, limits: DrawLimits) -> Result<Self> {
        let metadata = quota
            .reserve_external_storage(MAX_PREPARED * 1024)
            .map_err(|error| AnimationError::Budget(format!("native draw registry: {error:?}")))?;
        let media = NativeMedia::new(quota.clone(), media_limits)?;
        Ok(Self {
            media,
            prepared: BTreeMap::new(),
            quota,
            limits,
            _metadata: metadata,
        })
    }
    /// Trusted native service composition only. The producer must already have
    /// authenticated image/world source custody and constructed PreparedBlit.
    /// NativeDraw independently compares its original binding when used.
    pub fn retain_prepared(
        &mut self,
        instance: &mut PackageInstance,
        key: &str,
        prepared: PreparedBlit,
    ) -> Result<()> {
        if key.is_empty()
            || key.len() > 64
            || self.prepared.len() >= MAX_PREPARED
            || self.prepared.contains_key(key)
        {
            return Err(AnimationError::Runtime(
                "native prepared registry limit or duplicate".into(),
            ));
        }
        let authority = instance.frame_authority().ok_or_else(|| {
            AnimationError::PermissionDenied("native draw activation missing".into())
        })?;
        instance.with_frame_authority(&authority, || {
            self.prepared.insert(key.to_owned(), prepared);
        })?;
        Ok(())
    }
    /// Canonical surface composition AND immutable packing stay in one native
    /// authority window. This callback cannot pump JS/acquire/wait for workers.
    pub fn finish(
        &mut self,
        instance: &mut PackageInstance,
        surface: &mut Surface,
        metadata: FrameMeta,
        planes: Planes,
        stop: &StopToken,
        publish: impl FnOnce(&Snapshot, &Outcome) -> std::result::Result<(), SurfaceError>,
    ) -> Result<Outcome> {
        let authority = instance.frame_authority().ok_or_else(|| {
            AnimationError::PermissionDenied("native draw activation missing".into())
        })?;
        let binding = DrawBinding {
            package_digest: authority.package_digest.clone(),
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        };
        let shape = metadata.shape;
        instance
            .with_frame_authority(&authority, || {
                // Procedural typed-plane frames need no native geometry scratch.
                // Still commit their canonical frame under the same real guard.
                if metadata.commands.is_empty() {
                    return surface.finish_with(metadata, planes, &mut NoNativeRenderer, publish);
                }
                let mut guarded = GuardedDraw {
                    binding: &binding,
                    prepared: &self.prepared,
                };
                let mut renderer = NativeDraw::new(
                    &self.quota,
                    shape,
                    self.limits,
                    binding.clone(),
                    &self.media,
                    &mut guarded,
                    stop,
                )?;
                surface.finish_with(metadata, planes, &mut renderer, publish)
            })?
            .map_err(|error| AnimationError::Runtime(format!("native drawing: {error}")))
    }
    pub fn revoke(&mut self) {
        self.prepared.clear();
    }
}
