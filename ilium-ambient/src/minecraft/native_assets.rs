//! Worker-only, digest-pinned installed Java assets beneath exact selected overrides.
//! No download, extraction, class execution, generated model, or resource alias.
use crate::voxel_landscape::{
    assets::{
        animation::MissingAnimation,
        archive::{DuplicateMember, ZipSource},
        bank::{RequiredOrigin, TextureRequirement},
        budget::{ByteBudget, Cancel, Limits, Reservation},
        error::AssetError,
        identity::{AssetPath, Digest256, Label, OriginKind, ResourceId, SourceBlob},
        importer::{
            DefinitionSources, ScheduleSource, TextureCandidate, TextureLocation, TextureRequest,
        },
        layers::{LayeredPack, MountLayout, ResourceKey},
        pixels::ImageExpectations,
        review::{AssetPhase, FullPackReview, PackScope, SourceEdition},
        source::{AssetSource, SourceBytes, SourceLimits},
    },
    pack_sources, VoxelLandscapeSettings,
};
use ilium_platform::secure_fs::NoFollowDirectory;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const PROFILE: &str = "java-1.19.3-stored-quart-r1";
pub const NATIVE_JAR_SHA256: &str =
    "b7228c23dbc8988129561af3918dd469577de842d2eb3c7dabe00316bf9a44d6";
pub const NATIVE_PACK: &str = "ilium:installed-java-1.19.3";
const PATH_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "official Java installation directory is unavailable; supply an absolute 1.19.3 JAR path"
    )]
    InstallationUnavailable,
    #[error("invalid installed-native source request: {0}")]
    Invalid(&'static str),
    #[error("installed-native source I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Asset(#[from] AssetError),
}

/// Lexical selection only. An alternate saves directory never implies an asset
/// directory. Empty selects this ONE pinned release, not the latest installation.
pub fn jar_path(authored: &str) -> Result<PathBuf, Error> {
    if authored.is_empty() {
        return ilium_platform::minecraft::java_directory()
            .map(|root| root.join("versions/1.19.3/1.19.3.jar"))
            .ok_or(Error::InstallationUnavailable);
    }
    if authored.len() > PATH_BYTES || authored.contains('\0') {
        return Err(Error::Invalid("JAR path length or NUL"));
    }
    let path = PathBuf::from(authored);
    if !path.is_absolute() {
        return Err(Error::Invalid("JAR path must be absolute"));
    }
    Ok(path)
}

/// The field is an observed custody digest, not a certificate of renderer parity
/// or permission to redistribute the source. Directory overrides have no whole-
/// source digest; each resolved SourceBlob still carries its actual byte digest.
#[derive(Clone, Debug)]
pub struct Provenance {
    pub profile: &'static str,
    pub native_archive_sha256: Digest256,
    pub selected_archive_sha256: Option<Digest256>,
    pub selected_override: bool,
}

/// Packs are private so later callers cannot append authored compatibility data
/// to a value which has passed this native boundary. All allocations use the
/// caller's existing scene-family account, including outstanding generations.
pub struct NativeSources {
    packs: Vec<LayeredPack>,
    native_index: usize,
    provenance: Provenance,
    selected_duplicates: Vec<DuplicateMember>,
    immutable_definition_sources: bool,
    _duplicates_reservation: Reservation,
    limits: Limits,
    budget: ByteBudget,
    _reservation: Reservation,
}

pub struct NativeRequests {
    requests: Vec<TextureRequest>,
    _reservation: Reservation,
}
impl NativeRequests {
    pub fn as_slice(&self) -> &[TextureRequest] {
        &self.requests
    }
}

fn native_review() -> Result<FullPackReview, AssetError> {
    Ok(FullPackReview {
        pack: ResourceId::parse(NATIVE_PACK)?,
        release: Label::new("installed Java 1.19.3; packet-pinned archive")?,
        edition: SourceEdition::Java,
        scope: PackScope::FullWorld,
        phase: AssetPhase::PrivateTestPlaceholder,
        evidence: Label::new("native-tint-investigation-001/native-biome-climate-1.19.3.json; SHA-256-pinned installed archive")?,
        author_credit: Label::new("Mojang Studios / Minecraft; local installed source")?,
        license_record: Label::new("Local private rendering source only; no redistribution permission inferred")?,
        restrictions: vec![Label::new("Do not bundle, extract to the project, upload, or redistribute game assets")?],
        known_missing: vec![Label::new("Source scope is not renderer coverage: unsupported model, block-color, entity and fluid behavior must remain explicit")?],
    })
}

fn native_digest() -> Result<Digest256, AssetError> {
    Digest256::try_from(NATIVE_JAR_SHA256.to_owned())
}

fn read_jar(path: &Path, budget: &ByteBudget, cancel: Cancel<'_>) -> Result<SourceBytes, Error> {
    cancel.check()?;
    if !path.is_absolute() || path.as_os_str().len() > PATH_BYTES {
        return Err(Error::Invalid("absolute bounded JAR path required"));
    }
    let parent = path.parent().ok_or(Error::Invalid("JAR has no parent"))?;
    let name = path
        .file_name()
        .ok_or(Error::Invalid("JAR has no filename"))?;
    let directory = NoFollowDirectory::open_root(parent)?;
    // The supplied OS boundary rejects symlinks and non-regular handles without
    // blocking on a FIFO. No canonicalize-then-reopen check is introduced here.
    let file = directory.open_regular(name)?;
    let length = file.metadata()?.len();
    let bytes = SourceBytes::read_exact_size(
        file,
        length,
        SourceLimits::default().archive_bytes,
        budget,
        cancel,
    )?;
    bytes.verify(native_digest()?)?;
    Ok(bytes)
}

impl NativeSources {
    /// Blocking worker operation. The caller owns cancellation, admission and
    /// off-UI destruction. A changed/missing JAR is an error, never a download.
    pub fn open(
        path: &Path,
        selected: Option<&VoxelLandscapeSettings>,
        limits: Limits,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, Error> {
        cancel.check()?;
        limits.validate()?;
        let reservation = budget.reserve(32 * 1024, cancel)?;
        let bytes = read_jar(path, &budget, cancel)?;
        let digest = bytes.digest();
        // Reject duplicate archive members in the native profile. A selected
        // override's existing explicit duplicate policy is kept separately.
        let source: Arc<dyn AssetSource> = Arc::new(ZipSource::open(
            bytes,
            Some(native_digest()?),
            SourceLimits::default(),
            &budget,
            cancel,
        )?);
        let mut packs = Vec::new();
        packs
            .try_reserve_exact(2)
            .map_err(|_| AssetError::Allocation)?;
        let mut selected_archive_sha256 = None;
        // The pinned native JAR is a verified in-memory ZIP. A selected
        // directory, including an add-on directory beneath an archive base,
        // prevents route-wide definition caching.
        let mut immutable_definition_sources = true;
        let mut selected_duplicates = Vec::new();
        let mut duplicates_reservation = budget.reserve(0, cancel)?;
        if let Some(settings) = selected {
            // An explicit override that fails admission is not equivalent to
            // choosing no override. Its exact existing pack settings are used.
            let mounted = pack_sources::mount_selected(settings, budget.clone(), cancel)?;
            if mounted.pack.layout() == MountLayout::Bedrock {
                return Err(Error::Invalid(
                    "Bedrock override requires an exact Java mapping not supplied by this profile",
                ));
            }
            if mounted.pack.review().pack.as_str() == NATIVE_PACK {
                return Err(Error::Invalid(
                    "selected override uses reserved installed-native identity",
                ));
            }
            if !mounted.pack.uses_budget(&budget) {
                return Err(Error::Invalid(
                    "selected override uses a different byte account",
                ));
            }
            selected_archive_sha256 = mounted.source_sha256;
            immutable_definition_sources = mounted.immutable_definition_sources;
            duplicates_reservation =
                budget.reserve(4096 + mounted.duplicate_members.len() as u64 * 4096, cancel)?;
            selected_duplicates = mounted.duplicate_members;
            packs.push(mounted.pack);
        }
        let native_index = packs.len();
        let role = if native_index == 0 {
            OriginKind::SelectedPack
        } else {
            OriginKind::ExplicitFullPackFallback
        };
        let pack = LayeredPack::mount(
            native_review()?,
            source,
            None,
            MountLayout::Java,
            role,
            limits,
            budget.clone(),
            cancel,
        )?;
        // Do not invent pack.mcmeta or a native format tuple. This exact JAR is
        // a resource base, not an ordinary selected overlay manifest. Selected
        // packs retain their own explicitly configured target/overlay receipts.
        packs.push(pack);
        cancel.check()?;
        Ok(Self {
            packs,
            native_index,
            provenance: Provenance {
                profile: PROFILE,
                native_archive_sha256: digest,
                selected_archive_sha256,
                selected_override: native_index != 0,
            },
            selected_duplicates,
            immutable_definition_sources,
            _duplicates_reservation: duplicates_reservation,
            limits,
            budget,
            _reservation: reservation,
        })
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    pub fn selected_duplicate_members(&self) -> &[DuplicateMember] {
        &self.selected_duplicates
    }
    pub fn immutable_definition_sources(&self) -> bool {
        self.immutable_definition_sources
    }
    pub fn budget(&self) -> &ByteBudget {
        &self.budget
    }
    pub fn limits(&self) -> Limits {
        self.limits
    }
    pub fn packs(&self) -> &[LayeredPack] {
        &self.packs
    }

    /// Selected definitions first, installed-native definitions beneath them.
    /// Missing parents may come from native assets; malformed selected metadata
    /// propagates as an error. There is deliberately no DefinitionSet fallback.
    pub fn definitions(&self) -> Result<DefinitionSources<'_>, AssetError> {
        DefinitionSources::new(&self.packs, &[], None)
    }

    /// Exact resource lookup, also used for colormap images. The winning blob's
    /// pack/layer/path/review/member hash is retained by SourceBlob/PixelImage.
    /// A parser/decode failure after this selection must NOT retry another pack.
    pub fn resolve(
        &self,
        key: &ResourceKey,
        cancel: Cancel<'_>,
    ) -> Result<Option<SourceBlob>, AssetError> {
        cancel.check()?;
        for pack in &self.packs {
            if let Some(blob) = pack.read_resource(key, cancel)?.blob {
                return Ok(Some(blob));
            }
        }
        Ok(None)
    }

    /// Climate is game data, not selected-pack artwork. A selected resource pack
    /// must not replace native climate through a similarly named data/ member.
    pub fn native_biome(
        &self,
        id: &ResourceId,
        cancel: Cancel<'_>,
    ) -> Result<Option<SourceBlob>, AssetError> {
        if id.parts().0 != "minecraft" {
            return Err(AssetError::Unsupported("native climate namespace".into()));
        }
        let path = AssetPath::parse(&format!(
            "data/minecraft/worldgen/biome/{}.json",
            id.parts().1
        ))?;
        self.packs[self.native_index]
            .read_path(&path, self.limits.metadata_bytes, cancel)
            .map(|resolution| resolution.blob)
    }

    /// Native model texture IDs are not semantic aliases. No generated-owner
    /// mud/dirt or grass-name substitutions, entity fallbacks or diagnostic art
    /// are admitted by this request builder. The importer retains its full
    /// candidate records; the saved binder must require successful coverage.
    pub fn texture_requests(
        &self,
        ids: &[ResourceId],
        cancel: Cancel<'_>,
    ) -> Result<NativeRequests, AssetError> {
        cancel.check()?;
        if ids.len() > self.limits.requirements || ids.len() > self.limits.textures {
            return Err(AssetError::Limit {
                resource: "native texture requests",
                requested: ids.len() as u64,
                limit: self.limits.requirements.min(self.limits.textures) as u64,
            });
        }
        let reservation = self
            .budget
            .reserve(4096 + ids.len() as u64 * 16 * 1024, cancel)?;
        let mut seen = BTreeSet::new();
        let mut requests = Vec::new();
        requests
            .try_reserve_exact(ids.len())
            .map_err(|_| AssetError::Allocation)?;
        for id in ids {
            cancel.check()?;
            if !seen.insert(id.clone()) {
                return Err(AssetError::Duplicate(id.to_string()));
            }
            let mut requirement = TextureRequirement::selected_color(id.clone());
            if self.native_index != 0 {
                requirement.origin = RequiredOrigin::SelectedOrExplicitFullPackFallback;
            }
            let mut candidates = Vec::new();
            candidates
                .try_reserve_exact(self.packs.len())
                .map_err(|_| AssetError::Allocation)?;
            for pack in &self.packs {
                candidates.push(TextureCandidate {
                    pack: pack.review().pack.clone(),
                    location: TextureLocation::Resource {
                        id: id.clone(),
                        alias_reason: None,
                    },
                    schedule: ScheduleSource::AutomaticJava,
                    expected_source_sha256: None,
                    expected_image: ImageExpectations::default(),
                });
            }
            requests.push(TextureRequest {
                requirement,
                candidates,
                missing_animation: MissingAnimation::StaticImage,
            });
        }
        Ok(NativeRequests {
            requests,
            _reservation: reservation,
        })
    }
}

#[cfg(test)]
#[path = "native_assets_tests.rs"]
pub(super) mod tests;
