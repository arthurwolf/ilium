//! Worker-only, digest-pinned installed Java assets beneath exact selected overrides.
//! No download, extraction, class execution, or generated model is admitted.
use crate::voxel_landscape::{
    assets::{
        animation::{ExplicitFrame, MissingAnimation, PixelRect},
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
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const PROFILE: &str = "java-1.19.3-stored-quart-r1";
pub const NATIVE_JAR_SHA256: &str =
    "b7228c23dbc8988129561af3918dd469577de842d2eb3c7dabe00316bf9a44d6";
pub const NATIVE_PACK: &str = "ilium:installed-java-1.19.3";
const PATH_BYTES: usize = 4096;
const GOODVIBES_PACK: &str = "ilium-pack:goodvibes";

#[derive(Clone)]
struct GoodVibesWaterPin {
    digest: Digest256,
    dimensions: [u32; 2],
    missing_animation: MissingAnimation,
}

fn goodvibes_aliases() -> Result<BTreeMap<ResourceId, Vec<AssetPath>>, AssetError> {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../scenes/voxel_landscape/pack_aliases.json"))
            .map_err(|error| AssetError::InvalidMetadata(format!("pack alias catalog: {error}")))?;
    let entries = catalog
        .get("aliases")
        .and_then(|aliases| aliases.get("goodvibes"))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| AssetError::InvalidMetadata("missing GoodVibes pack aliases".into()))?;
    let mut output = BTreeMap::new();
    for (key, paths) in entries {
        let id = ResourceId::parse(key)?;
        let paths = paths
            .as_array()
            .ok_or_else(|| AssetError::InvalidMetadata("invalid GoodVibes alias paths".into()))?;
        let mut converted = Vec::new();
        converted
            .try_reserve_exact(paths.len())
            .map_err(|_| AssetError::Allocation)?;
        for path in paths.iter().filter_map(serde_json::Value::as_str) {
            converted.push(AssetPath::parse(path)?);
        }
        output.insert(id, converted);
    }
    Ok(output)
}

fn goodvibes_water_pin(id: &ResourceId) -> Result<Option<GoodVibesWaterPin>, AssetError> {
    let (digest, dimensions, frame_dimensions, frame_count, ticks) = match id.as_str() {
        "minecraft:block/water_still" => (
            "f35e3a02b81bb359bf3eced106c12324f5523d9fb85e13599887205c0e513247",
            [512, 16384],
            [512, 512],
            32_u32,
            2_u32,
        ),
        "minecraft:block/water_flow" => (
            "75994f61cfd8a4e56480010e91b1df098ab71c70ba373d858effe9d3a2613f66",
            [1021, 16384],
            [1021, 1024],
            16_u32,
            1_u32,
        ),
        _ => return Ok(None),
    };
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(frame_count as usize)
        .map_err(|_| AssetError::Allocation)?;
    for index in 0..frame_count {
        frames.push(ExplicitFrame {
            rect: PixelRect {
                x: 0,
                y: index * frame_dimensions[1],
                width: frame_dimensions[0],
                height: frame_dimensions[1],
            },
            ticks,
        });
    }
    Ok(Some(GoodVibesWaterPin {
        digest: Digest256::try_from(digest.to_owned())?,
        dimensions,
        missing_animation: MissingAnimation::ExplicitFrames {
            frames,
            interpolate: false,
            reason: Label::new(
                "Ilium compatibility timing for digest-pinned GoodVibes water sheet; not pack-authored; no interpolation",
            )?,
        },
    }))
}

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
        evidence: Label::new(
            "native-tint-investigation-001/native-biome-climate-1.19.3.json; SHA-256-pinned installed archive",
        )?,
        author_credit: Label::new("Mojang Studios / Minecraft; local installed source")?,
        license_record: Label::new(
            "Local private rendering source only; no redistribution permission inferred",
        )?,
        restrictions: vec![Label::new(
            "Do not bundle, extract to the project, upload, or redistribute game assets",
        )?],
        known_missing: vec![Label::new(
            "Source scope is not renderer coverage: unsupported model, block-color, entity and fluid behavior must remain explicit",
        )?],
    })
}

fn native_digest() -> Result<Digest256, AssetError> {
    Digest256::try_from(NATIVE_JAR_SHA256.to_owned())
}

#[derive(Clone, Copy)]
pub(crate) enum NativeArchive<'a> {
    Path(&'a Path),
    Pinned(&'a ilium_platform::animation_files::PinnedFile),
}
struct PinnedArchiveReader<'a> {
    file: &'a ilium_platform::animation_files::PinnedFile,
    offset: u64,
}
impl std::io::Read for PinnedArchiveReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.file.read_at(buffer, self.offset)?;
        self.offset = self
            .offset
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("native archive read offset overflow"))?;
        Ok(count)
    }
}
fn read_jar_pinned(
    file: &ilium_platform::animation_files::PinnedFile,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<SourceBytes, Error> {
    cancel.check()?;
    let length = file.len()?;
    let modified = file.modified()?;
    let bytes = SourceBytes::read_exact_size(
        PinnedArchiveReader { file, offset: 0 },
        length,
        SourceLimits::default().archive_bytes,
        budget,
        cancel,
    )?;
    if file.len()? != length || file.modified()? != modified {
        return Err(Error::Invalid(
            "selected native archive changed during capture",
        ));
    }
    // NativeSources' common ZIP mount still checks the exact reviewed digest.
    Ok(bytes)
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
        Self::open_archive(NativeArchive::Path(path), selected, limits, budget, cancel)
    }

    /// The host separately selects and admits this original archive descriptor.
    /// A world directory grant never grants access to the installed JAR.
    pub fn open_pinned(
        file: &ilium_platform::animation_files::PinnedFile,
        selected: Option<&VoxelLandscapeSettings>,
        limits: Limits,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, Error> {
        Self::open_archive(
            NativeArchive::Pinned(file),
            selected,
            limits,
            budget,
            cancel,
        )
    }

    pub(crate) fn open_archive(
        archive: NativeArchive<'_>,
        selected: Option<&VoxelLandscapeSettings>,
        limits: Limits,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, Error> {
        cancel.check()?;
        limits.validate()?;
        let reservation = budget.reserve(32 * 1024, cancel)?;
        let bytes = match archive {
            NativeArchive::Path(path) => read_jar(path, &budget, cancel)?,
            NativeArchive::Pinned(file) => read_jar_pinned(file, &budget, cancel)?,
        };
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
        let goodvibes_selected = self.packs.first().is_some_and(|pack| {
            pack.review().pack.as_str() == GOODVIBES_PACK
                && pack.layout() == MountLayout::ExtractedJava
        });
        let goodvibes_aliases = if goodvibes_selected {
            goodvibes_aliases()?
        } else {
            BTreeMap::new()
        };
        let maximum_aliases = goodvibes_aliases
            .values()
            .map(Vec::len)
            .max()
            .unwrap_or_default();
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
                .try_reserve_exact(self.packs.len() + 1 + maximum_aliases)
                .map_err(|_| AssetError::Allocation)?;
            let water_pin = if goodvibes_selected {
                goodvibes_water_pin(id)?
            } else {
                None
            };
            let water_schedule = || {
                water_pin
                    .as_ref()
                    .map_or(ScheduleSource::AutomaticJava, |pin| {
                        ScheduleSource::AutomaticJavaWithMissing(pin.missing_animation.clone())
                    })
            };
            let water_digest = water_pin.as_ref().map(|pin| pin.digest);
            let water_dimensions = water_pin.as_ref().map(|pin| pin.dimensions);
            let mut emitted_aliases = BTreeSet::new();
            for (pack_index, pack) in self.packs.iter().enumerate() {
                candidates.push(TextureCandidate {
                    pack: pack.review().pack.clone(),
                    location: TextureLocation::Resource {
                        id: id.clone(),
                        alias_reason: None,
                    },
                    schedule: if pack_index == 0 && goodvibes_selected {
                        water_schedule()
                    } else {
                        ScheduleSource::AutomaticJava
                    },
                    expected_source_sha256: if pack_index == 0 && goodvibes_selected {
                        water_digest
                    } else {
                        None
                    },
                    expected_image: ImageExpectations {
                        dimensions: if pack_index == 0 && goodvibes_selected {
                            water_dimensions
                        } else {
                            None
                        },
                        ..ImageExpectations::default()
                    },
                });
                if pack_index != 0 || !goodvibes_selected || id.parts().0 != "minecraft" {
                    continue;
                }
                let root_path = AssetPath::parse(&format!("textures/{}.png", id.parts().1))?;
                emitted_aliases.insert(root_path.clone());
                candidates.push(TextureCandidate {
                    pack: pack.review().pack.clone(),
                    location: TextureLocation::Literal {
                        path: root_path,
                        evidence: Label::new(
                            "GoodVibes extracted Minecraft root: exact textures path for this Java resource",
                        )?,
                    },
                    schedule: water_schedule(),
                    expected_source_sha256: water_digest,
                    expected_image: ImageExpectations {
                        dimensions: water_dimensions,
                        ..ImageExpectations::default()
                    },
                });
                if let Some(paths) = goodvibes_aliases.get(id) {
                    for path in paths {
                        if !emitted_aliases.insert(path.clone()) {
                            continue;
                        }
                        candidates.push(TextureCandidate {
                            pack: pack.review().pack.clone(),
                            location: TextureLocation::Literal {
                                path: path.clone(),
                                evidence: Label::new(
                                    "GoodVibes pack_aliases.json exact retained resource path",
                                )?,
                            },
                            schedule: water_schedule(),
                            expected_source_sha256: water_digest,
                            expected_image: ImageExpectations {
                                dimensions: water_dimensions,
                                ..ImageExpectations::default()
                            },
                        });
                    }
                }
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
