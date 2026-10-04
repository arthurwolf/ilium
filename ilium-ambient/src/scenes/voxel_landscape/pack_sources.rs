//! Selected local pack mount on the single owned preparation worker.
//! No source lookup or archive inflation is legal on the render thread.
use super::{
    assets::{
        archive::{DuplicateMember, DuplicatePolicy, ZipSource},
        budget::{ByteBudget, Cancel, Limits},
        error::{AssetError, Result},
        identity::{AssetPath, Digest256, Label, OriginKind},
        layers::{LayeredPack, MountLayout, PackFormat},
        source::{AssetSource, LocalTree, SourceBytes, SourceLimits},
    },
    pack_profiles::{self, PackSourceKind},
    settings::VoxelLandscapeSettings,
};
use ilium_platform::secure_fs::NoFollowDirectory;
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub struct MountSource {
    pub source: Arc<dyn AssetSource>,
    pub source_sha256: Option<Digest256>,
    pub duplicate_members: Vec<DuplicateMember>,
}
pub struct MountedPack {
    pub pack: LayeredPack,
    pub source_sha256: Option<Digest256>,
    pub duplicate_members: Vec<DuplicateMember>,
    /// True only when every selected base/add-on layer was mounted from an
    /// in-memory ZIP snapshot. A directory layer remains a live source.
    pub immutable_definition_sources: bool,
}
fn source(
    path: &Path,
    directory: bool,
    duplicate_policy: DuplicatePolicy,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<MountSource> {
    if !path.is_absolute() || path.as_os_str().is_empty() {
        return Err(AssetError::InvalidPath(
            "pack source needs an absolute local path".into(),
        ));
    }
    let limits = SourceLimits::default();
    if directory {
        Ok(MountSource {
            source: Arc::new(LocalTree::open(
                path,
                BTreeMap::new(),
                limits,
                budget,
                cancel,
            )?),
            source_sha256: None,
            duplicate_members: Vec::new(),
        })
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| AssetError::InvalidPath("archive has no parent".into()))?;
        let name = path
            .file_name()
            .ok_or_else(|| AssetError::InvalidPath("archive has no filename".into()))?;
        let directory = NoFollowDirectory::open_root(parent)
            .map_err(|error| AssetError::InvalidPath(format!("archive parent: {error}")))?;
        let file = directory
            .open_regular(name)
            .map_err(|error| AssetError::InvalidPath(format!("archive open: {error}")))?;
        let size = file
            .metadata()
            .map_err(|error| AssetError::InvalidPath(format!("archive metadata: {error}")))?
            .len();
        let bytes = SourceBytes::read_exact_size(file, size, limits.archive_bytes, budget, cancel)?;
        let digest = bytes.digest();
        let archive = ZipSource::open_with_duplicate_policy(
            bytes,
            None,
            limits,
            budget,
            cancel,
            duplicate_policy,
        )?;
        let duplicate_members = archive.duplicate_members().to_vec();
        Ok(MountSource {
            source: Arc::new(archive),
            source_sha256: Some(digest),
            duplicate_members,
        })
    }
}

pub fn mount_selected(
    settings: &VoxelLandscapeSettings,
    budget: ByteBudget,
    cancel: Cancel<'_>,
) -> Result<MountedPack> {
    mount_with_origin(settings, budget, OriginKind::SelectedPack, cancel)
}

/// A separately reviewed installed full pack for private visual compatibility.
/// Its members may satisfy only explicit fallback requirements, never selected art.
pub fn mount_reviewed_fallback(
    settings: &VoxelLandscapeSettings,
    budget: ByteBudget,
    cancel: Cancel<'_>,
) -> Result<MountedPack> {
    mount_with_origin(
        settings,
        budget,
        OriginKind::ExplicitFullPackFallback,
        cancel,
    )
}

fn mount_with_origin(
    settings: &VoxelLandscapeSettings,
    budget: ByteBudget,
    origin_kind: OriginKind,
    cancel: Cancel<'_>,
) -> Result<MountedPack> {
    cancel.check()?;
    let profile = *pack_profiles::profile(settings.pack_profile)?;
    if settings.pack_path.is_empty() {
        return Err(AssetError::InvalidPath(
            "choose a local full-pack file or folder".into(),
        ));
    }
    let mut review = profile.review()?;
    let mut kind = profile.source_kind;
    if profile.id == "plasticator" && settings.pack_edition == 1 {
        review.edition = super::assets::review::SourceEdition::Bedrock;
        review.release = Label::new("Bedrock 2.4.0")?;
        kind = PackSourceKind::Bedrock;
    } else if settings.pack_edition != 0 {
        return Err(AssetError::InvalidReview(
            "Bedrock edition is only reviewed for Plasticator".into(),
        ));
    }
    let layout = match kind {
        PackSourceKind::Java => MountLayout::Java,
        PackSourceKind::ExtractedJavaArt => MountLayout::ExtractedJava,
        PackSourceKind::Bedrock => MountLayout::Bedrock,
    };
    let root = if settings.pack_root.is_empty() {
        None
    } else {
        Some(AssetPath::parse(&settings.pack_root)?)
    };
    if settings.pack_duplicate_last_wins && profile.id != "textureless" {
        return Err(AssetError::InvalidReview(
            "duplicate ZIP override is reviewed only for Textureless".into(),
        ));
    }
    let duplicate_policy = if settings.pack_duplicate_last_wins {
        DuplicatePolicy::LastCentralEntry
    } else {
        DuplicatePolicy::Reject
    };
    let selected_source = source(
        Path::new(&settings.pack_path),
        settings.pack_mount == 1,
        duplicate_policy,
        &budget,
        cancel,
    )?;
    let source_sha256 = selected_source.source_sha256;
    let mut immutable_definition_sources = source_sha256.is_some();
    let duplicate_members = selected_source.duplicate_members;
    let target = PackFormat::new(settings.pack_format_major, settings.pack_format_minor)?;
    let limits = Limits::default();
    let pack = LayeredPack::mount(
        review,
        selected_source.source,
        root,
        layout,
        origin_kind,
        limits,
        budget.clone(),
        cancel,
    )?;
    let mut pack=match kind {
        PackSourceKind::Java=>pack.with_java_overlays(target,
            Some(Label::new("Explicit private-test target; non-native version compatibility is measured per resource")?),cancel)?,
        PackSourceKind::ExtractedJavaArt=>pack.with_external_target(target,
            Label::new("Extracted GoodVibes art has no authored Java pack metadata")?,cancel)?,
        PackSourceKind::Bedrock=>pack,
    };
    if !settings.pack_addon_path.is_empty() {
        if profile.id != "textureless" {
            return Err(AssetError::InvalidReview(
                "only Textureless has a reviewed official model add-on".into(),
            ));
        }
        let addon = source(
            Path::new(&settings.pack_addon_path),
            settings.pack_addon_mount == 1,
            DuplicatePolicy::Reject,
            &budget,
            cancel,
        )?;
        immutable_definition_sources &= addon.source_sha256.is_some();
        pack = pack.with_internal_pack(
            addon.source,
            None,
            Label::new("Textureless official models add-on")?,
            Some(Label::new(
                "Official add-on target compatibility reviewed separately",
            )?),
            cancel,
        )?;
    }
    Ok(MountedPack {
        pack,
        source_sha256,
        duplicate_members,
        immutable_definition_sources,
    })
}

#[cfg(test)]
mod immutable_source_tests {
    use super::*;
    use crate::voxel_landscape::assets::archive::synthetic_zip;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn mounted_directory_is_live_while_loaded_zip_is_an_immutable_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("selected-directory");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("stone.json"), b"{}").unwrap();
        let archive = root.path().join("selected.zip");
        std::fs::write(
            &archive,
            synthetic_zip(&[("stone.json", b"{}")], false, false),
        )
        .unwrap();
        let budget = ByteBudget::new(64 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let live = source(&directory, true, DuplicatePolicy::Reject, &budget, cancel).unwrap();
        let frozen = source(&archive, false, DuplicatePolicy::Reject, &budget, cancel).unwrap();
        assert!(live.source_sha256.is_none());
        assert!(frozen.source_sha256.is_some());
        assert!(!(frozen.source_sha256.is_some() && live.source_sha256.is_some())); // A directory add-on disqualifies an otherwise immutable ZIP base.
        drop(live);
        drop(frozen);
        assert_eq!(budget.used(), 0);
    }
}
