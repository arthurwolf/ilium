//! Original selected-child adapter for the existing native SavedScene graph.
//! Construction receives host-retained authorities; none of the labels grants
//! filesystem access. Prepare must run inside the original committed IO job.
use crate::{
    error::{AnimationError, Result},
    native_worlds::{HostWorldScene, SavedWorldFactory, SavedWorldGrant, SourceIdentity},
};
use ilium_ambient::{
    minecraft::{
        saved_runtime::SavedRuntime,
        saved_scene::{PinnedSceneSource, SavedScene},
    },
    resources::AmbientResources,
    scene::SceneEnv,
    VoxelLandscapeSettings,
};
use ilium_platform::{
    animation_files::{FileIdentity, PinnedDirectory, PinnedFile},
    owned_worker::StopToken,
    secure_fs::NoFollowDirectory,
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};

const MAX_LEVEL_BYTES: usize = 2 * 1024 * 1024;
const OPEN_WORK_BYTES: usize = 8 * 1024 * 1024;
const BINDING_BYTES: usize = 128 * 1024;
fn invalid(reason: &str) -> AnimationError {
    AnimationError::Runtime(format!("native saved-world factory: {reason}"))
}

/// The composition root supplies each authority separately. The original
/// selected child Arc must also be the Arc in SavedWorldGrant. The caller owns
/// the shared runtime across scene replacement and final history drain.
/// Settings are host-owned native settings, with their resource-pack rights
/// checked independently; selected-world rights never authorize pack paths.
pub struct NativeSavedSource {
    pub parent_label: PathBuf,
    pub selected_label: PathBuf,
    pub parent: Arc<PinnedDirectory>,
    pub child: Arc<NoFollowDirectory>,
    pub child_identity: FileIdentity,
    pub epoch: u64,
    pub native_jar: Arc<PinnedFile>,
    pub history_storage: PathBuf,
    pub history_root: Arc<PinnedDirectory>,
    pub runtime: Arc<SavedRuntime>,
    pub settings: VoxelLandscapeSettings,
    pub environment: SceneEnv,
    pub stop: StopToken,
}

/// One committed source request creates at most one admitted scene. A failure
/// consumes the factory, rather than retrying an old operation ticket.
pub struct NativeSavedFactory {
    source: Option<NativeSavedSource>,
}
impl NativeSavedFactory {
    /// Input allocation remains owned by the original admitted native request.
    /// No filesystem access, worker launch or history mutation occurs here.
    pub fn from_host(source: NativeSavedSource) -> Self {
        Self {
            source: Some(source),
        }
    }
}

fn checkpoint(stop: &StopToken) -> Result<()> {
    if stop.is_stopped() {
        return Err(invalid("original open was cancelled"));
    }
    Ok(())
}

/// Recompute the exact listing revision from original metadata bytes, under
/// retained handles, before handing the selected source to the native worker.
fn verify_revision(source: &NativeSavedSource, grant: &SavedWorldGrant) -> Result<()> {
    checkpoint(&source.stop)?;
    if !source.parent_label.is_absolute()
        || source.selected_label.parent() != Some(source.parent_label.as_path())
        || source.history_storage.as_os_str().len() > 4096
        || source.parent_label.as_os_str().len() > 4096
        || source.epoch == 0
        || source.epoch != grant.epoch()
        || !source.history_storage.is_absolute()
        || !std::ptr::eq(grant.root(), source.child.as_ref())
    {
        return Err(invalid("original child, epoch or host label mismatch"));
    }
    let leaf = source
        .selected_label
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.len() <= 128)
        .ok_or_else(|| invalid("selected child label bound"))?;
    let retained = PinnedDirectory::from_host(Arc::clone(&source.child))
        .map_err(|_| invalid("original child descriptor unavailable"))?;
    let reopened = source
        .parent
        .child(leaf, false)
        .map_err(|_| invalid("original child entry changed"))?;
    if retained.identity() != source.child_identity || reopened.identity() != source.child_identity
    {
        return Err(invalid("selected child identity changed"));
    }
    let file = retained
        .open_file("level.dat")
        .map_err(|_| invalid("original metadata unavailable"))?;
    let length = usize::try_from(file.len().map_err(|_| invalid("metadata length"))?)
        .map_err(|_| invalid("metadata length range"))?;
    if length == 0 || length > MAX_LEVEL_BYTES {
        return Err(invalid("metadata byte limit"));
    }
    let modified = file.modified().map_err(|_| invalid("metadata age"))?;
    let mut bytes = vec![0; length];
    let mut offset = 0;
    while offset < length {
        checkpoint(&source.stop)?;
        let end = (offset + 8192).min(length);
        let count = file
            .read_at(&mut bytes[offset..end], offset as u64)
            .map_err(|_| invalid("metadata read"))?;
        if count == 0 {
            return Err(invalid("metadata short read"));
        }
        offset += count;
    }
    let replacement = retained
        .open_file("level.dat")
        .map_err(|_| invalid("metadata entry changed"))?;
    if replacement.identity() != file.identity()
        || file.len().map_err(|_| invalid("metadata restat"))? != length as u64
        || file.modified().map_err(|_| invalid("metadata restat"))? != modified
    {
        return Err(invalid("metadata changed while opening"));
    }
    // A second bounded capture detects same-inode writes, including changes
    // which preserve the metadata length and deliberately restored timestamps.
    let mut confirmation = vec![0; length];
    let mut offset = 0;
    while offset < length {
        checkpoint(&source.stop)?;
        let end = (offset + 8192).min(length);
        let count = replacement
            .read_at(&mut confirmation[offset..end], offset as u64)
            .map_err(|_| invalid("metadata confirmation read"))?;
        if count == 0 {
            return Err(invalid("metadata confirmation short read"));
        }
        offset += count;
    }
    if bytes != confirmation
        || replacement
            .len()
            .map_err(|_| invalid("metadata confirmation length"))?
            != length as u64
        || replacement
            .modified()
            .map_err(|_| invalid("metadata confirmation age"))?
            != modified
    {
        return Err(invalid("metadata changed between captures"));
    }
    let parent = source.parent.identity();
    let mut digest = Sha256::new();
    digest.update(b"ilium-selected-world-metadata-v1");
    for value in [
        parent.device,
        parent.inode,
        source.child_identity.device,
        source.child_identity.inode,
        source.epoch,
    ] {
        digest.update(value.to_be_bytes());
    }
    digest.update(&bytes);
    if SourceIdentity::from_host_digest(digest.finalize().into()) != grant.identity() {
        return Err(invalid("original listed metadata revision changed"));
    }
    checkpoint(&source.stop)
}

impl SavedWorldFactory for NativeSavedFactory {
    fn prepare(
        &mut self,
        grant: &SavedWorldGrant,
        resources: &AmbientResources,
    ) -> Result<HostWorldScene> {
        let source = self
            .source
            .take()
            .ok_or_else(|| invalid("original factory already consumed"))?;
        if !grant.history_authorized() {
            return Err(AnimationError::PermissionDenied(
                "saved world requires original history authorization".into(),
            ));
        }
        if !resources
            .finite()
            .quota_group()
            .shares_root(&source.environment.resources.finite().quota_group())
        {
            return Err(invalid("scene and world service quota roots differ"));
        }
        // Reserve temporary metadata decoding and descriptor work before reads;
        // the original 1 GiB scene/64 MiB worker charges remain in SavedScene.
        let _work = resources
            .reserve_storage(OPEN_WORK_BYTES)
            .map_err(|error| AnimationError::Budget(format!("saved open scratch: {error:?}")))?;
        let binding = resources
            .reserve_storage(BINDING_BYTES)
            .map_err(|error| AnimationError::Budget(format!("saved scene binding: {error:?}")))?;
        verify_revision(&source, grant)?;
        let selected = Arc::clone(&source.child);
        let scene = SavedScene::new_pinned(
            PinnedSceneSource {
                root_label: source.parent_label,
                selected_world: Some(source.selected_label),
                selected_identity: Some(source.child_identity),
                root: source.parent,
                native_jar: source.native_jar,
                history_storage: source.history_storage,
                history_root: Some(source.history_root),
                stop: Some(source.stop.clone()),
                runtime: source.runtime,
            },
            &source.settings,
            &source.environment,
        )
        .map_err(|reason| AnimationError::Budget(format!("saved scene admission: {reason}")))?;
        checkpoint(&source.stop)?;
        // Admission is not catalog readiness: the dispatcher must publish preparing
        // and later reports the native scene's actual asynchronous status.
        Ok(HostWorldScene::from_host(
            Box::new(scene),
            grant.identity(),
            binding,
            selected,
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "native_saved_factory_tests.rs"]
mod tests;
