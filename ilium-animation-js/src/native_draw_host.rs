//! Worker-owned native draw composition under the genuine frame publication gate.
//! Prepared handles are retained native values, never reconstructed from guest IDs.
use crate::{
    engine::HostRequest,
    error::{AnimationError, Result},
    native_draw::{DrawAuthority, DrawBinding, DrawLimits, NativeDraw, PreparedBlit},
    native_media::{ImageHandle, MediaLimits, NativeMedia},
    native_video::TimedVideoFrame,
    runtime::PackageInstance,
    sources::NativeSourceImage,
    surface::{FrameMeta, NoNativeRenderer, Outcome, Planes, Snapshot, Surface, SurfaceError},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
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
    source_images: BTreeMap<String, crate::native_media::ImageHandle>,
    video_images: BTreeMap<String, crate::native_media::ImageHandle>,
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
            source_images: BTreeMap::new(),
            video_images: BTreeMap::new(),
            quota,
            limits,
            _metadata: metadata,
        })
    }
    /// One original registry backs SDK image handles, source imports and
    /// PreparedBlit snapshots. Keep mutations on the owning animation worker.
    pub fn media(&self) -> &NativeMedia {
        &self.media
    }
    pub fn media_mut(&mut self) -> &mut NativeMedia {
        &mut self.media
    }
    pub fn register_image(
        &mut self,
        instance: &PackageInstance,
        request: &HostRequest,
        handle: ImageHandle,
    ) -> Result<()> {
        let authority = instance.frame_authority().ok_or_else(|| {
            AnimationError::PermissionDenied("native image activation missing".into())
        })?;
        let binding = DrawBinding {
            package_digest: authority.package_digest,
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        };
        let key = handle.id().to_string();
        if key.len() > 64 || self.prepared.len() >= MAX_PREPARED || self.prepared.contains_key(&key)
        {
            return Err(AnimationError::Budget(
                "native prepared image registry".into(),
            ));
        }
        let prepared = PreparedBlit::image(&self.media, handle, binding, None)
            .map_err(|error| AnimationError::Runtime(format!("prepare native image: {error}")))?;
        instance.with_baseline_media_registry(request, || {
            self.prepared.insert(key, prepared);
            Ok(())
        })
    }
    pub fn close_image(&mut self, handle: ImageHandle) -> Result<()> {
        self.media.close(handle)?;
        self.prepared.remove(&handle.id().to_string());
        Ok(())
    }
    /// Source V2 owns these mappings and its own close/seed invalidation. A
    /// generic image operation may borrow only the existing native allocation.
    pub(crate) fn source_image_handle(&self, key: &str) -> Option<ImageHandle> {
        self.source_images.get(key).copied()
    }
    pub(crate) fn video_image_handle(&self, key: &str) -> Option<ImageHandle> {
        self.video_images.get(key).copied()
    }
    /// Move the decoder's original admitted RGBA Arc into the shared image and
    /// prepared-draw registries. The caller must have obtained this frame from
    /// its actual NativeVideo owner; the frame's native resource ID and current
    /// activation must still agree. Replay provenance is attached only when
    /// the real FrozenEvidence owner has issued a token for these bytes.
    pub fn retain_video_frame(
        &mut self,
        instance: &mut PackageInstance,
        frame: &TimedVideoFrame,
        resource_id: u64,
        evidence_token: Option<u64>,
    ) -> Result<Value> {
        let authority = instance.frame_authority().ok_or_else(|| {
            AnimationError::PermissionDenied("video image activation missing".into())
        })?;
        if frame.resource_id != resource_id
            || frame.authority.package_digest != authority.package_digest
            || frame.authority.instance_id != authority.instance_id
            || frame.authority.plan_revision != authority.plan_generation
            || frame.authority.authorization_epoch != authority.authorization_epoch
        {
            return Err(AnimationError::PermissionDenied(
                "video frame has no current original owner".into(),
            ));
        }
        let image = frame
            .admitted_rgba()
            .ok_or_else(|| AnimationError::Runtime("video decoder did not publish RGBA".into()))?;
        if !image.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "video image original quota mismatch".into(),
            ));
        }
        let binding = DrawBinding {
            package_digest: authority.package_digest.clone(),
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        };
        let handle = self.media.retain_admitted_image(Arc::clone(&image))?;
        let key = format!("video-image-{}", handle.id());
        let registered = (|| {
            if key.len() > 64
                || self.prepared.len() >= MAX_PREPARED
                || self.prepared.contains_key(&key)
                || self.video_images.contains_key(&key)
            {
                return Err(AnimationError::Budget("native video image registry".into()));
            }
            let prepared = PreparedBlit::image(&self.media, handle, binding, evidence_token)
                .map_err(|error| AnimationError::Runtime(error.to_string()))?;
            instance.with_resource_registry_authority(|| {
                self.prepared.insert(key.clone(), prepared);
            })?;
            Ok::<_, AnimationError>(())
        })();
        if let Err(error) = registered {
            let _ = self.media.close(handle);
            return Err(error);
        }
        let pixels = image.view();
        let sha256 = format!("{:x}", Sha256::digest(&pixels.rgba));
        self.video_images.insert(key.clone(), handle);
        Ok(
            json!({"id":key,"kind":"image","width":pixels.width,"height":pixels.height,"format":"rgba8","sha256":sha256}),
        )
    }
    pub fn release_video_image(&mut self, key: &str) -> Result<()> {
        let handle = self.video_images.remove(key).ok_or_else(|| {
            AnimationError::PermissionDenied("unknown original video image".into())
        })?;
        self.prepared.remove(key);
        self.media.close(handle)
    }
    /// Call after actual helper retirement when a seed/ACK might have exposed
    /// Video image identities. Logical revocation alone is not that proof.
    pub fn release_video_images_after_helper_retirement(&mut self) -> Result<()> {
        for key in self.video_images.keys().cloned().collect::<Vec<_>>() {
            self.release_video_image(&key)?;
        }
        Ok(())
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
        instance.frame_authority().ok_or_else(|| {
            AnimationError::PermissionDenied("native draw activation missing".into())
        })?;
        instance.with_resource_registry_authority(|| {
            self.prepared.insert(key.to_owned(), prepared);
        })?;
        Ok(())
    }
    /// Import exactly an already admitted native source allocation. The caller
    /// must first authenticate its SourceCompletion/SourceFeed owner; this
    /// method additionally checks the current frame binding before publishing.
    pub fn retain_source_image(
        &mut self,
        instance: &mut PackageInstance,
        image: &NativeSourceImage,
    ) -> Result<Value> {
        if !image.shares_root(&self.quota) {
            return Err(AnimationError::PermissionDenied(
                "source image original quota mismatch".into(),
            ));
        }
        let authority = instance.frame_authority().ok_or_else(|| {
            AnimationError::PermissionDenied("source image activation missing".into())
        })?;
        let binding = DrawBinding {
            package_digest: authority.package_digest.clone(),
            instance_id: authority.instance_id,
            plan_generation: authority.plan_generation,
            authorization_epoch: authority.authorization_epoch,
        };
        let handle = self
            .media
            .retain_admitted_image(Arc::clone(image.admitted_pixels()))?;
        let key = format!("source-image-{}", handle.id());
        let registered = (|| {
            if key.len() > 64
                || self.prepared.len() >= MAX_PREPARED
                || self.prepared.contains_key(&key)
                || self.source_images.contains_key(&key)
            {
                return Err(AnimationError::Budget(
                    "native source image registry full or duplicate".into(),
                ));
            }
            let prepared = PreparedBlit::image(&self.media, handle, binding, None)
                .map_err(|error| AnimationError::Runtime(error.to_string()))?;
            instance.with_source_registration_authority(&authority, || {
                self.prepared.insert(key.clone(), prepared);
            })?;
            Ok::<_, AnimationError>(())
        })();
        if let Err(error) = registered {
            let _ = self.media.close(handle);
            return Err(error);
        }
        let pixels = image.admitted_pixels().view();
        let sha256 = format!("{:x}", Sha256::digest(&pixels.rgba));
        self.source_images.insert(key.clone(), handle);
        Ok(
            json!({"id":key,"kind":"image","width":pixels.width,"height":pixels.height,"format":"rgba8","sha256":sha256}),
        )
    }
    /// Registry lookup is only an observation; the caller also authenticates
    /// the original request and current package channel before close ACK.
    pub fn owns_source_image(&self, key: &str) -> bool {
        self.source_images.contains_key(key)
    }

    /// A refused helper copy cannot leave an image ID visible to later frames.
    pub fn release_source_image(&mut self, key: &str) -> Result<()> {
        let handle = self
            .source_images
            .remove(key)
            .ok_or_else(|| AnimationError::Runtime("unknown admitted source image".into()))?;
        self.prepared.remove(key);
        self.media.close(handle)
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
        for (_, handle) in std::mem::take(&mut self.source_images) {
            let _ = self.media.close(handle);
        }
    }
}
