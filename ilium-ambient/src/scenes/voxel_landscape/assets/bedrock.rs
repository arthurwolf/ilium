//! Bedrock resource metadata and explicit edition bridges. Missing inherited game
//! tables stay absent; a filename is never promoted into a block/model contract.
use super::{
    animation::{AnimationEvidence, AnimationPlan, ExplicitFrame, PixelRect, Sampler},
    budget::{ByteBudget, Cancel, Limits},
    error::{AssetError, Result},
    identity::{AssetPath, Label, ResourceId},
    layers::{LayeredPack, MountLayout, Resolution},
    metadata::{self, allowed, array, boolean, object, required, text, uint, Document},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug)]
pub struct BedrockManifest {
    pub format_version: u32,
    pub version: [u32; 3],
    pub source: Document,
}
impl BedrockManifest {
    pub fn parse(source: Document) -> Result<Self> {
        let fields = object(&source.value)?;
        let format_version = uint(required(fields, "format_version")?)?;
        if ![1, 2].contains(&format_version) {
            return Err(AssetError::Unsupported(
                "Bedrock resource manifest version".into(),
            ));
        }
        let header = object(required(fields, "header")?)?;
        let parts = array(required(header, "version")?)?;
        if parts.len() != 3 {
            return Err(metadata::invalid(
                "Bedrock release requires three version components",
            ));
        }
        let version = [uint(&parts[0])?, uint(&parts[1])?, uint(&parts[2])?];
        let modules = array(required(fields, "modules")?)?;
        if modules.len() > 64
            || !modules.iter().any(|value| {
                value.get("type").and_then(serde_json::Value::as_str) == Some("resources")
            })
        {
            return Err(metadata::invalid("Bedrock archive has no resource module"));
        }
        Ok(Self {
            format_version,
            version,
            source,
        })
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct TerrainTextureEntry {
    pub path: AssetPath,
    pub tint_color: Option<String>,
    pub overlay_color: Option<String>,
}
#[derive(Debug)]
pub struct TerrainTextureTable {
    pub entries: BTreeMap<String, Vec<TerrainTextureEntry>>,
    pub source: Document,
}
impl TerrainTextureTable {
    pub fn parse(source: Document) -> Result<Self> {
        let fields = object(&source.value)?;
        let data = object(required(fields, "texture_data")?)?;
        let mut entries = BTreeMap::new();
        for (name, value) in data {
            Label::new(name)?;
            let texture = required(object(value)?, "textures")?;
            let choices = match texture {
                serde_json::Value::Array(values) => values.as_slice(),
                _ => std::slice::from_ref(texture),
            };
            if choices.is_empty() || choices.len() > 256 {
                return Err(metadata::invalid("Bedrock texture variant count"));
            }
            let mut variants = Vec::new();
            for choice in choices {
                if let Some(path) = choice.as_str() {
                    variants.push(TerrainTextureEntry {
                        path: AssetPath::parse(path)?,
                        tint_color: None,
                        overlay_color: None,
                    });
                    continue;
                }
                let fields = object(choice)?;
                allowed(fields, &["path", "tint_color", "overlay_color"])?;
                let tint_color = fields
                    .get("tint_color")
                    .map(text)
                    .transpose()?
                    .map(str::to_owned);
                let overlay_color = fields
                    .get("overlay_color")
                    .map(text)
                    .transpose()?
                    .map(str::to_owned);
                for color in [tint_color.as_deref(), overlay_color.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    if color.len() > 128 || color.chars().any(char::is_control) {
                        return Err(metadata::invalid("invalid Bedrock color metadata"));
                    }
                }
                variants.push(TerrainTextureEntry {
                    path: AssetPath::parse(text(required(fields, "path")?)?)?,
                    tint_color,
                    overlay_color,
                });
            }
            entries.insert(name.clone(), variants);
        }
        Ok(Self { entries, source })
    }
    pub fn variant(&self, tile: &str, index: usize) -> Result<&TerrainTextureEntry> {
        self.entries
            .get(tile)
            .and_then(|entries| entries.get(index))
            .ok_or_else(|| {
                metadata::invalid("missing Bedrock tile/variant; inherited table required")
            })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditionTextureBinding {
    pub semantic: ResourceId,
    pub source_path: AssetPath,
    pub evidence: Label,
    /// Explicit priority only when both extensions exist; never arbitrary enumeration order.
    pub extensions: Vec<String>,
}
impl EditionTextureBinding {
    pub fn resolve(&self, pack: &LayeredPack, cancel: Cancel<'_>) -> Result<Resolution> {
        if pack.layout() != MountLayout::Bedrock {
            return Err(metadata::invalid(
                "Bedrock bridge applied to another source edition",
            ));
        }
        if self.extensions.is_empty()
            || self.extensions.len() > 2
            || self
                .extensions
                .iter()
                .any(|ext| ext != "png" && ext != "tga")
        {
            return Err(metadata::invalid(
                "explicit Bedrock extension priority must be PNG/TGA",
            ));
        }
        let exact = self.source_path.as_str().ends_with(".png")
            || self.source_path.as_str().ends_with(".tga");
        if exact {
            return pack.read_path(&self.source_path, pack.limits().encoded_bytes, cancel);
        }
        let mut result: Option<Resolution> = None;
        for extension in &self.extensions {
            cancel.check()?;
            let path = AssetPath::parse(&format!("{}.{}", self.source_path.as_str(), extension))?;
            let mut candidate = pack.read_path(&path, pack.limits().encoded_bytes, cancel)?;
            if let Some(previous) = result {
                candidate.attempts.splice(0..0, previous.attempts);
                candidate.reservations.extend(previous.reservations);
            }
            let found = candidate.blob.is_some();
            result = Some(candidate);
            if found {
                break;
            }
        }
        result.ok_or_else(|| metadata::invalid("empty extension resolution"))
    }
}
#[derive(Clone, Debug)]
pub struct FlipbookEntry {
    pub texture: AssetPath,
    pub tile: String,
    pub variant: Option<u32>,
    pub initial_frame: Option<u32>,
    pub ticks: Option<u32>,
    pub frames: Option<Vec<u32>>,
    pub blend: Option<bool>,
}
#[derive(Debug)]
pub struct FlipbookTable {
    pub entries: Vec<FlipbookEntry>,
    pub source: Document,
}
#[derive(Clone, Debug)]
pub struct FlipbookDefaults {
    pub ticks: u32,
    pub blend: bool,
    pub evidence: Label,
}
impl FlipbookTable {
    pub fn parse(source: Document) -> Result<Self> {
        let mut entries = Vec::new();
        for value in array(&source.value)? {
            if entries.len() >= 4096 {
                return Err(metadata::invalid("flipbook entry count"));
            }
            let fields = object(value)?;
            allowed(
                fields,
                &[
                    "flipbook_texture",
                    "atlas_tile",
                    "atlas_index",
                    "atlas_tile_variant",
                    "ticks_per_frame",
                    "frames",
                    "blend_frames",
                ],
            )?;
            let frames = fields
                .get("frames")
                .map(|value| {
                    let items = array(value)?;
                    if items.is_empty() || items.len() > 4096 {
                        return Err(metadata::invalid("flipbook frame count"));
                    }
                    items.iter().map(uint).collect::<Result<Vec<_>>>()
                })
                .transpose()?;
            let tile = text(required(fields, "atlas_tile")?)?.to_owned();
            Label::new(&tile)?;
            let variant = fields.get("atlas_tile_variant").map(uint).transpose()?;
            if entries
                .iter()
                .any(|entry: &FlipbookEntry| entry.tile == tile && entry.variant == variant)
            {
                return Err(AssetError::Duplicate(format!("Bedrock flipbook {tile}")));
            }
            entries.push(FlipbookEntry {
                texture: AssetPath::parse(text(required(fields, "flipbook_texture")?)?)?,
                tile,
                variant,
                initial_frame: fields.get("atlas_index").map(uint).transpose()?,
                ticks: fields.get("ticks_per_frame").map(uint).transpose()?,
                frames,
                blend: fields.get("blend_frames").map(boolean).transpose()?,
            });
        }
        Ok(Self { entries, source })
    }
    pub fn plan(
        &self,
        index: usize,
        dimensions: [u32; 2],
        defaults: &FlipbookDefaults,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<AnimationPlan> {
        cancel.check()?;
        limits.rgba_bytes(dimensions[0], dimensions[1])?;
        if !self.source.uses_budget(budget) {
            return Err(metadata::invalid(
                "flipbook metadata and timeline accounts differ",
            ));
        } // Preserve shared accounting across format normalization.
        let entry = self
            .entries
            .get(index)
            .ok_or_else(|| metadata::invalid("flipbook index outside table"))?;
        if !dimensions[1].is_multiple_of(dimensions[0]) {
            return Err(metadata::invalid(
                "Bedrock vertical frame grid does not divide image",
            ));
        }
        let count = dimensions[1] / dimensions[0];
        super::source::check_limit(
            "flipbook frames",
            u64::from(count),
            limits.animation_frames as u64,
        )?;
        let mut order = entry.frames.clone().unwrap_or_else(|| (0..count).collect());
        if let Some(initial) = entry.initial_frame {
            let position = order
                .iter()
                .position(|frame| *frame == initial)
                .ok_or_else(|| metadata::invalid("initial flipbook frame absent from sequence"))?;
            order.rotate_left(position);
        }
        let ticks = entry.ticks.unwrap_or(defaults.ticks);
        let mut frames = Vec::new();
        for frame in order {
            if frame >= count {
                return Err(metadata::invalid("flipbook frame outside source image"));
            }
            frames.push(ExplicitFrame {
                rect: PixelRect {
                    x: 0,
                    y: frame * dimensions[0],
                    width: dimensions[0],
                    height: dimensions[0],
                },
                ticks,
            });
        }
        let defaults_used = entry.ticks.is_none() || entry.blend.is_none();
        let evidence = AnimationEvidence::BedrockMetadata {
            origin: self.source.origin.clone(),
            sha256: self.source.sha256,
            defaults: defaults_used.then(|| defaults.evidence.clone()),
            initial_frame: entry.initial_frame,
            default_frame_time: entry.ticks.is_none(),
            default_interpolation: entry.blend.is_none(),
            default_sequence: entry.frames.is_none(),
            default_frame_size: true,
        };
        AnimationPlan::from_normalized_frames(
            dimensions,
            &frames,
            entry.blend.unwrap_or(defaults.blend),
            Sampler::default(),
            evidence,
            limits,
            budget,
            cancel,
        )
    }
}
#[derive(Debug)]
pub struct BedrockIndex {
    pub manifest: BedrockManifest,
    pub terrain: Option<TerrainTextureTable>,
    pub flipbooks: Option<FlipbookTable>,
    pub missing_inherited_tables: Vec<&'static str>,
}
impl BedrockIndex {
    pub fn load(pack: &LayeredPack, cancel: Cancel<'_>) -> Result<Self> {
        if pack.layout() != MountLayout::Bedrock {
            return Err(metadata::invalid("not a Bedrock mount"));
        }
        let read = |name: &str| -> Result<Option<Document>> {
            let found = pack.read_path(
                &AssetPath::parse(name)?,
                pack.limits().metadata_bytes,
                cancel,
            )?;
            found
                .blob
                .as_ref()
                .map(|blob| Document::parse(blob, pack.limits(), pack.budget(), cancel))
                .transpose()
        };
        let manifest = BedrockManifest::parse(read("manifest.json")?.ok_or_else(|| {
            metadata::invalid("missing Bedrock manifest at explicit nested root")
        })?)?;
        let terrain = read("textures/terrain_texture.json")?
            .map(TerrainTextureTable::parse)
            .transpose()?;
        let flipbooks = read("textures/flipbook_textures.json")?
            .map(FlipbookTable::parse)
            .transpose()?;
        let mut missing_inherited_tables = Vec::new();
        if terrain.is_none() {
            missing_inherited_tables
                .push("terrain_texture.json: explicit edition bindings required");
        }
        if flipbooks.is_none() {
            missing_inherited_tables.push("flipbook_textures.json: no authored animation asserted");
        }
        Ok(Self {
            manifest,
            terrain,
            flipbooks,
            missing_inherited_tables,
        })
    }
}
