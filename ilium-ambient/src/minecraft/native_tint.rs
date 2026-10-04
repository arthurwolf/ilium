//! Installed-source climate and colormap resolution over exact saved biome cells.
//!
//! This is a typed COLOR-KIND provider, not an inferred BlockColors registry.
//! The caller must prove which native provider a model tint index invokes.
//! Missing biome data, unsupported modifiers and unknown bindings are not white.
use super::{
    chunk::BiomeSample,
    evidence::Source,
    native_assets::{self, NativeSources},
    native_biome, native_colormap,
    native_swamp_noise::SwampNoise,
    region,
    tours::PreparedMap,
};
use crate::voxel_landscape::assets::{
    budget::{ByteBudget, Cancel, Reservation},
    error::AssetError,
    identity::{AssetPath, BlobOrigin, Digest256, ResourceId},
    layers::{ResourceKey, ResourceKind},
    pixels::{ImageExpectations, PixelImage},
};
use serde::Deserialize;

const MANIFEST: &str = include_str!("native_climate_1193.json");
const CLIMATE_ROWS: usize = 63;
const REGISTRY_CHARGE: u64 = 2 * 1024 * 1024;
const MAP_PIXELS: usize = 256 * 256;
const NATIVE_DATA_VERSION: i32 = 3218;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorKind {
    Grass,
    Foliage,
    Water,
}

/// Native FoliageColor constants verified by the supplied class bytes. A caller
/// cannot infer a block->constant registration from these numeric values alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoliageConstant {
    Evergreen,
    Birch,
    Default,
    Mangrove,
}
impl FoliageConstant {
    pub const fn rgb(self) -> [u8; 3] {
        rgb(match self {
            Self::Evergreen => 6_396_257,
            Self::Birch => 8_431_445,
            Self::Default => 4_764_952,
            Self::Mangrove => 9_619_016,
        })
    }
}

/// Rendering older saved identities using a modern source is a separate explicit
/// policy, never proof that their original climate/model behavior was identical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClimatePolicy {
    Require1193,
    #[default]
    RenderEarlierWith1193,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionRelation {
    Java1193,
    EarlierSaveWith1193Climate { saved_data_version: i32 },
}
impl ClimatePolicy {
    pub fn relation(self, data_version: i32) -> Result<VersionRelation, Error> {
        if data_version == NATIVE_DATA_VERSION {
            return Ok(VersionRelation::Java1193);
        }
        if !(region::MIN_DATA_VERSION..=region::MAX_DATA_VERSION).contains(&data_version) {
            return Err(Error::UnsupportedDataVersion(data_version));
        }
        match self {
            Self::Require1193 => Err(Error::EarlierClimateNotVerified(data_version)),
            Self::RenderEarlierWith1193 => Ok(VersionRelation::EarlierSaveWith1193Climate {
                saved_data_version: data_version,
            }),
        }
    }
}

/// Pinned 1.19.3 CavesAndCliffsRenames (aqj) table, registered by DataFixers
/// (ape) at schema 2838. This changes only the explicitly requested modern
/// rendering climate; retained stored palette identities remain untouched.
/// Unlisted names stay exact and still fail registry lookup when unsupported.
fn render_climate_identity(name: &str, relation: VersionRelation) -> &str {
    if !matches!(relation, VersionRelation::EarlierSaveWith1193Climate { saved_data_version } if saved_data_version < 2838)
    {
        return name;
    }
    match name {
        "minecraft:badlands_plateau" => "minecraft:badlands",
        "minecraft:bamboo_jungle_hills" => "minecraft:bamboo_jungle",
        "minecraft:birch_forest_hills" => "minecraft:birch_forest",
        "minecraft:dark_forest_hills" => "minecraft:dark_forest",
        "minecraft:desert_hills" => "minecraft:desert",
        "minecraft:desert_lakes" => "minecraft:desert",
        "minecraft:giant_spruce_taiga" => "minecraft:old_growth_spruce_taiga",
        "minecraft:giant_spruce_taiga_hills" => "minecraft:old_growth_spruce_taiga",
        "minecraft:giant_tree_taiga" => "minecraft:old_growth_pine_taiga",
        "minecraft:giant_tree_taiga_hills" => "minecraft:old_growth_pine_taiga",
        "minecraft:gravelly_mountains" => "minecraft:windswept_gravelly_hills",
        "minecraft:jungle_edge" => "minecraft:sparse_jungle",
        "minecraft:jungle_hills" => "minecraft:jungle",
        "minecraft:lofty_peaks" => "minecraft:jagged_peaks",
        "minecraft:modified_badlands_plateau" => "minecraft:badlands",
        "minecraft:modified_gravelly_mountains" => "minecraft:windswept_gravelly_hills",
        "minecraft:modified_jungle" => "minecraft:jungle",
        "minecraft:modified_jungle_edge" => "minecraft:sparse_jungle",
        "minecraft:modified_wooded_badlands_plateau" => "minecraft:wooded_badlands",
        "minecraft:mountain_edge" => "minecraft:windswept_hills",
        "minecraft:mountains" => "minecraft:windswept_hills",
        "minecraft:mushroom_field_shore" => "minecraft:mushroom_fields",
        "minecraft:shattered_savanna" => "minecraft:windswept_savanna",
        "minecraft:shattered_savanna_plateau" => "minecraft:windswept_savanna",
        "minecraft:snowcapped_peaks" => "minecraft:frozen_peaks",
        "minecraft:snowy_mountains" => "minecraft:snowy_plains",
        "minecraft:snowy_taiga_hills" => "minecraft:snowy_taiga",
        "minecraft:snowy_taiga_mountains" => "minecraft:snowy_taiga",
        "minecraft:snowy_tundra" => "minecraft:snowy_plains",
        "minecraft:stone_shore" => "minecraft:stony_shore",
        "minecraft:swamp_hills" => "minecraft:swamp",
        "minecraft:taiga_hills" => "minecraft:taiga",
        "minecraft:taiga_mountains" => "minecraft:taiga",
        "minecraft:tall_birch_forest" => "minecraft:old_growth_birch_forest",
        "minecraft:tall_birch_hills" => "minecraft:old_growth_birch_forest",
        "minecraft:wooded_badlands_plateau" => "minecraft:wooded_badlands",
        "minecraft:wooded_hills" => "minecraft:forest",
        "minecraft:wooded_mountains" => "minecraft:windswept_forest",
        _ => name,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub source: Source,
    pub java_position: [i32; 3],
    pub kind: ColorKind,
    pub climate_policy: ClimatePolicy,
}
/// This renderer explicitly uses 1.19.3 color sources for all admitted saves.
/// A missing seed is not equivalent to the valid seed zero.
#[derive(Clone, Copy, Debug)]
pub struct NativeRequest {
    pub source: Source,
    pub java_position: [i32; 3],
    pub kind: ColorKind,
    pub climate_policy: ClimatePolicy,
    pub world_seed: Option<i64>,
    /// Native Options.biomeBlendRadius is 0..=7, default 2.
    pub blend_radius: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingBiome {
    SectionList,
    Section,
    Palette,
    OutsideChunk,
    OutsideSectionRange,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error(transparent)]
    Colormap(#[from] native_colormap::Error),
    #[error("invalid compiled native climate manifest: {0}")]
    Manifest(&'static str),
    #[error("native climate row differs from its observed source member")]
    ClimateIntegrity,
    #[error("saved tint source generation does not match retained map")]
    StaleSource,
    #[error("saved tint coordinate is outside the admitted Overworld height range")]
    OutsideHeight,
    #[error("saved tint coordinate has no retained qualified chunk")]
    MissingChunk,
    #[error("canonical Data.WorldGenSettings.seed Long is required for native biome selection")]
    MissingSeed,
    #[error("native biome blend radius must be 0..=7")]
    BlendRadius,
    #[error("native biome sample coordinate exceeds Java signed block range")]
    Coordinate,
    #[error(transparent)]
    BiomeSelection(#[from] native_biome::Error),
    #[error("saved biome is unavailable: {0:?}")]
    MissingBiome(MissingBiome),
    #[error("saved biome has no exact identity in the pinned native climate registry")]
    UnknownBiome,
    #[error("saved tint data version is unsupported: {0}")]
    UnsupportedDataVersion(i32),
    #[error("original climate for saved data version {0} is unverified; explicit 1.19.3 rendering policy required")]
    EarlierClimateNotVerified(i32),
    #[error("native grass modifier needs unprovided implementation evidence: {0:?}")]
    GrassModifier(GrassModifier),
    #[error("required source colormap is missing: {0:?}")]
    MissingColormap(ColorKind),
    #[error("source colormap must be opaque, static, 8-bit, 256x256 pixels")]
    ColormapFormat,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrassModifier {
    DarkForest,
    Swamp,
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Effects {
    water_color: u32,
    foliage_color: Option<u32>,
    grass_color: Option<u32>,
    grass_color_modifier: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Climate {
    id: ResourceId,
    source: AssetPath,
    sha256: Digest256,
    temperature: f32,
    downfall: f32,
    temperature_modifier: Option<String>,
    effects: Effects,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    profile: String,
    source_sha256: Digest256,
    rows: Vec<Climate>,
}
impl Manifest {
    fn parse() -> Result<Self, Error> {
        // This is a compiled bounded research record, not untrusted pack JSON.
        if MANIFEST.len() > 128 * 1024 {
            return Err(Error::Manifest("compiled bytes"));
        }
        let value: Self = serde_json::from_str(MANIFEST).map_err(|_| Error::Manifest("schema"))?;
        if value.profile != native_assets::PROFILE
            || value.source_sha256.to_string() != native_assets::NATIVE_JAR_SHA256
            || value.rows.len() != CLIMATE_ROWS
            || value.rows.windows(2).any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(Error::Manifest("profile, digest, sorted unique rows"));
        }
        for row in &value.rows {
            if row.id.parts().0 != "minecraft"
                || row.source.as_str()
                    != format!("data/minecraft/worldgen/biome/{}.json", row.id.parts().1)
                || !row.temperature.is_finite()
                || !row.downfall.is_finite()
                || row.effects.water_color > 0xff_ffff
                || row
                    .effects
                    .foliage_color
                    .is_some_and(|value| value > 0xff_ffff)
                || row
                    .effects
                    .grass_color
                    .is_some_and(|value| value > 0xff_ffff)
                || row
                    .temperature_modifier
                    .as_deref()
                    .is_some_and(|value| value != "frozen")
                || row
                    .effects
                    .grass_color_modifier
                    .as_ref()
                    .is_some_and(|value| value.len() > 64)
            {
                return Err(Error::Manifest("row values"));
            }
        }
        Ok(value)
    }
    fn climate(&self, name: &str) -> Result<&Climate, Error> {
        self.rows
            .binary_search_by(|row| row.id.as_str().cmp(name))
            .map(|index| &self.rows[index])
            .map_err(|_| Error::UnknownBiome)
    }
}

#[derive(Debug)]
pub struct ColormapEvidence {
    pub origin: BlobOrigin,
    pub source_sha256: Digest256,
    pub rgba_sha256: Digest256,
    pub dimensions: [u32; 2],
}
struct MapPixels {
    rgb: Vec<[u8; 3]>,
    evidence: ColormapEvidence,
    _reservation: Reservation,
}
impl MapPixels {
    fn load(sources: &NativeSources, kind: ColorKind, cancel: Cancel<'_>) -> Result<Self, Error> {
        let id = match kind {
            ColorKind::Grass => "minecraft:colormap/grass",
            ColorKind::Foliage => "minecraft:colormap/foliage",
            ColorKind::Water => return Err(Error::Manifest("water has no grass/foliage image")),
        };
        let key = ResourceKey {
            kind: ResourceKind::Texture,
            id: ResourceId::parse(id)?,
        };
        let blob = sources
            .resolve(&key, cancel)?
            .ok_or(Error::MissingColormap(kind))?;
        // A present colormap sidecar is not silently treated as a static source.
        // No colormap-animation semantics are established by the evidence packet.
        let sidecar = ResourceKey {
            kind: ResourceKind::TextureMetadata,
            id: key.id.clone(),
        };
        if sources.resolve(&sidecar, cancel)?.is_some() {
            return Err(Error::ColormapFormat);
        }
        let image = PixelImage::decode_png(
            &blob,
            ImageExpectations {
                dimensions: Some([256, 256]),
                rgba_sha256: None,
            },
            &sources.limits(),
            sources.budget(),
            cancel,
        )?;
        if image.info().source_bits_per_channel != 8 || image.info().alpha_min != 255 {
            return Err(Error::ColormapFormat);
        }
        let reservation = sources
            .budget()
            .reserve((MAP_PIXELS * 3 + 8192) as u64, cancel)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(MAP_PIXELS)
            .map_err(|_| AssetError::Allocation)?;
        for (index, pixel) in image.bytes().chunks_exact(4).enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            pixels.push([pixel[0], pixel[1], pixel[2]]);
        }
        // Preserve the scalar kernel rather than rewriting its f32-clamp then
        // f64 multiply/truncation order. The RGB copy is explicitly charged.
        native_colormap::Colormap::new(&pixels)?;
        let evidence = ColormapEvidence {
            origin: image.origin().clone(),
            source_sha256: image.source_sha256(),
            rgba_sha256: image.rgba_sha256(),
            dimensions: image.dimensions(),
        };
        cancel.check()?;
        Ok(Self {
            rgb: pixels,
            evidence,
            _reservation: reservation,
        })
    }
    fn sample(&self, climate: &Climate) -> Result<[u8; 3], Error> {
        Ok(native_colormap::Colormap::new(&self.rgb)?
            .sample(climate.temperature, climate.downfall)?)
    }
}

/// Owned by the native preparation generation; no file reads or image decoding
/// occur in sample_kind. Drop on that worker, not through the UI's last Arc.
pub struct NativeTint {
    registry: Manifest,
    grass: MapPixels,
    foliage: MapPixels,
    budget: ByteBudget,
    swamp: SwampNoise,
    _reservation: Reservation,
}
#[derive(Debug)]
pub struct Sample<'a> {
    pub source: Source,
    pub java_position: [i32; 3],
    pub kind: ColorKind,
    pub rgb: [u8; 3],
    pub biome: &'a str,
    pub climate_source: &'a AssetPath,
    pub climate_sha256: Digest256,
    pub version_relation: VersionRelation,
    /// None for a native explicit biome color. This is not missing provenance:
    /// climate_source/digest still identify the numeric source of that color.
    pub colormap: Option<&'a ColormapEvidence>,
}
#[derive(Debug)]
pub struct NativeContribution<'a> {
    pub selected_quart: [i32; 3],
    /// ChunkAccess clamps only the vertical quart coordinate for palette lookup.
    pub lookup_quart: [i32; 3],
    pub sample: Sample<'a>,
}
#[derive(Debug)]
pub struct NativeBlend<'a> {
    pub rgb: [u8; 3],
    pub contributions: Vec<NativeContribution<'a>>,
    _reservation: Reservation,
}
impl NativeBlend<'_> {
    pub fn multiplier(&self) -> [f32; 3] {
        self.rgb.map(|channel| f32::from(channel) / 255.0)
    }
}
impl Sample<'_> {
    /// Raw-channel multiplier expected by the existing material API. Shared
    /// scene appearance and the renderer's color-space policy remain separate.
    pub fn multiplier(&self) -> [f32; 3] {
        self.rgb.map(|channel| f32::from(channel) / 255.0)
    }
}
impl NativeTint {
    /// Validate every captured climate row against the actual installed member.
    /// Selected resource packs affect colormap artwork, never climate JSON.
    pub fn load(sources: &NativeSources, cancel: Cancel<'_>) -> Result<Self, Error> {
        cancel.check()?;
        if sources.provenance().profile != native_assets::PROFILE
            || sources.provenance().native_archive_sha256.to_string()
                != native_assets::NATIVE_JAR_SHA256
        {
            return Err(Error::ClimateIntegrity);
        }
        let reservation = sources.budget().reserve(REGISTRY_CHARGE, cancel)?;
        let registry = Manifest::parse()?;
        for row in &registry.rows {
            cancel.check()?;
            let blob = sources
                .native_biome(&row.id, cancel)?
                .ok_or(Error::ClimateIntegrity)?;
            if blob.digest() != row.sha256
                || blob.origin().path != row.source
                || blob.origin().pack.as_str() != native_assets::NATIVE_PACK
            {
                return Err(Error::ClimateIntegrity);
            }
        }
        let grass = MapPixels::load(sources, ColorKind::Grass, cancel)?;
        let foliage = MapPixels::load(sources, ColorKind::Foliage, cancel)?;
        cancel.check()?;
        Ok(Self {
            registry,
            grass,
            foliage,
            budget: sources.budget().clone(),
            swamp: SwampNoise::new(),
            _reservation: reservation,
        })
    }

    pub fn colormap_evidence(&self, kind: ColorKind) -> Option<&ColormapEvidence> {
        match kind {
            ColorKind::Grass => Some(&self.grass.evidence),
            ColorKind::Foliage => Some(&self.foliage.evidence),
            ColorKind::Water => None,
        }
    }

    /// One exact stored 4x4x4 biome cell, without a generated climate, fallback
    /// biome, noise seed, nearest-biome search, horizontal average or y=0 lookup.
    /// Request.kind is a prerequisite supplied by a separately verified block-
    /// color binding. This result alone must never authorize history credit.
    pub fn sample_kind<'a>(
        &'a self,
        map: &'a PreparedMap,
        request: Request,
        cancel: Cancel<'_>,
    ) -> Result<Sample<'a>, Error> {
        cancel.check()?;
        let selected = stored_biome(map, request)?;
        let row = self
            .registry
            .climate(render_climate_identity(selected.name, selected.relation))?;
        let (color, colormap) = match request.kind {
            ColorKind::Grass => {
                if let Some(modifier) = row.effects.grass_color_modifier.as_deref() {
                    return Err(Error::GrassModifier(match modifier {
                        "dark_forest" => GrassModifier::DarkForest,
                        "swamp" => GrassModifier::Swamp,
                        _ => GrassModifier::Other,
                    }));
                }
                match row.effects.grass_color {
                    Some(value) => (rgb(value), None),
                    None => (self.grass.sample(row)?, Some(&self.grass.evidence)),
                }
            }
            ColorKind::Foliage => match row.effects.foliage_color {
                Some(value) => (rgb(value), None),
                None => (self.foliage.sample(row)?, Some(&self.foliage.evidence)),
            },
            ColorKind::Water => (rgb(row.effects.water_color), None),
        };
        cancel.check()?;
        Ok(Sample {
            source: request.source,
            java_position: request.java_position,
            kind: request.kind,
            rgb: color,
            biome: selected.name,
            climate_source: &row.source,
            climate_sha256: row.sha256,
            version_relation: selected.relation,
            colormap,
        })
    }

    /// Native seeded eight-quart lookup at every block in the inclusive square
    /// blend window. Retains each climate/member/sample identity so a caller can
    /// audit mixed source colors; the RGB mean is integer division per channel.
    pub fn sample_native<'a>(
        &'a self,
        map: &'a PreparedMap,
        request: NativeRequest,
        cancel: Cancel<'_>,
    ) -> Result<NativeBlend<'a>, Error> {
        let seed = request.world_seed.ok_or(Error::MissingSeed)?;
        if request.blend_radius > 7 {
            return Err(Error::BlendRadius);
        }
        if map.source() != request.source {
            return Err(Error::StaleSource);
        }
        let radius = i32::from(request.blend_radius);
        let width = (radius * 2 + 1) as usize;
        let count = width * width;
        let reservation = self.budget.reserve(
            4096 + count as u64 * std::mem::size_of::<NativeContribution<'_>>() as u64,
            cancel,
        )?;
        let mut contributions = Vec::new();
        contributions
            .try_reserve_exact(count)
            .map_err(|_| AssetError::Allocation)?;
        let mut channels = [0_u32; 3];
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                cancel.check()?;
                let position = [
                    request.java_position[0]
                        .checked_add(dx)
                        .ok_or(Error::Coordinate)?,
                    request.java_position[1],
                    request.java_position[2]
                        .checked_add(dz)
                        .ok_or(Error::Coordinate)?,
                ];
                let quart = native_biome::choose_quart(seed, position)?;
                // Pinned ChunkAccess.getNoiseBiome clamps quart Y before both
                // section indexing and local palette lookup. The selected
                // fiddled corner remains separately recorded for provenance.
                let lookup_quart = lookup_quart(quart);
                let stored_position = [
                    lookup_quart[0].checked_mul(4).ok_or(Error::Coordinate)?,
                    lookup_quart[1].checked_mul(4).ok_or(Error::Coordinate)?,
                    lookup_quart[2].checked_mul(4).ok_or(Error::Coordinate)?,
                ];
                let stored = stored_biome(
                    map,
                    Request {
                        source: request.source,
                        java_position: stored_position,
                        kind: request.kind,
                        climate_policy: request.climate_policy,
                    },
                )?;
                let row = self
                    .registry
                    .climate(render_climate_identity(stored.name, stored.relation))?;
                let (color, colormap) = match request.kind {
                    ColorKind::Grass => {
                        let base = match row.effects.grass_color {
                            Some(value) => value,
                            None => packed(self.grass.sample(row)?),
                        };
                        let value = match row.effects.grass_color_modifier.as_deref() {
                            None | Some("none") => base,
                            Some("dark_forest") => ((base & 0xfe_fe_fe) + 0x28_34_0a) >> 1,
                            Some("swamp") => packed(self.swamp.grass_rgb(position[0], position[2])),
                            Some(_) => return Err(Error::GrassModifier(GrassModifier::Other)),
                        };
                        (
                            rgb(value),
                            row.effects
                                .grass_color
                                .is_none()
                                .then_some(&self.grass.evidence),
                        )
                    }
                    ColorKind::Foliage => match row.effects.foliage_color {
                        Some(value) => (rgb(value), None),
                        None => (self.foliage.sample(row)?, Some(&self.foliage.evidence)),
                    },
                    ColorKind::Water => (rgb(row.effects.water_color), None),
                };
                for (sum, channel) in channels.iter_mut().zip(color) {
                    *sum += u32::from(channel);
                }
                contributions.push(NativeContribution {
                    selected_quart: quart,
                    lookup_quart,
                    sample: Sample {
                        source: request.source,
                        java_position: position,
                        kind: request.kind,
                        rgb: color,
                        biome: stored.name,
                        climate_source: &row.source,
                        climate_sha256: row.sha256,
                        version_relation: stored.relation,
                        colormap,
                    },
                });
            }
        }
        let denominator = count as u32;
        let rgb = channels.map(|sum| (sum / denominator) as u8);
        Ok(NativeBlend {
            rgb,
            contributions,
            _reservation: reservation,
        })
    }
}
fn lookup_quart(selected: [i32; 3]) -> [i32; 3] {
    [selected[0], selected[1].clamp(-16, 79), selected[2]]
}
struct StoredBiome<'a> {
    name: &'a str,
    relation: VersionRelation,
}
fn stored_biome(map: &PreparedMap, request: Request) -> Result<StoredBiome<'_>, Error> {
    if map.source() != request.source {
        return Err(Error::StaleSource);
    }
    if !(-64..=319).contains(&request.java_position[1]) {
        return Err(Error::OutsideHeight);
    }
    let [x, _, z] = request.java_position;
    let chunk = map
        .loaded()
        .chunks
        .get(&[x.div_euclid(16), z.div_euclid(16)])
        .ok_or(Error::MissingChunk)?;
    let relation = request
        .climate_policy
        .relation(chunk.identity.data_version)?;
    let name = match chunk.biome_at(request.java_position) {
        BiomeSample::Name(name) => name,
        BiomeSample::MissingSectionList => {
            return Err(Error::MissingBiome(MissingBiome::SectionList))
        }
        BiomeSample::MissingSection => return Err(Error::MissingBiome(MissingBiome::Section)),
        BiomeSample::MissingBiomes => return Err(Error::MissingBiome(MissingBiome::Palette)),
        BiomeSample::OutsideChunk => return Err(Error::MissingBiome(MissingBiome::OutsideChunk)),
        BiomeSample::OutsideSectionRange => {
            return Err(Error::MissingBiome(MissingBiome::OutsideSectionRange))
        }
    };
    Ok(StoredBiome { name, relation })
}
const fn rgb(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}
const fn packed(color: [u8; 3]) -> u32 {
    (color[0] as u32) << 16 | (color[1] as u32) << 8 | color[2] as u32
}

#[cfg(test)]
#[path = "native_tint_tests.rs"]
mod tests;
