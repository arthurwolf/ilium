//! Worker-side integration of exact sources, explicit aliases, image/time decoding and B1 banks.
//! Every failed/missing request is retained; no diagnostic pixel counts as selected-pack proof.
use super::{
    animation::{AnimationPlan, MissingAnimation},
    bank::{TextureBank, TextureBankBuilder, TextureRequirement},
    bedrock::{BedrockIndex, EditionTextureBinding, FlipbookDefaults},
    budget::{ByteBudget, Cancel, Limits, Reservation},
    compatibility::DefinitionSet,
    error::{self, AssetError, Result},
    identity::{AssetPath, BlobOrigin, Digest256, Label, ResourceId},
    layers::{
        LayeredPack, MountLayout, PackFormat, Resolution, ResolutionAttempt, ResourceKey,
        ResourceKind,
    },
    metadata::{self, Document},
    models::{DefinitionInput, DefinitionProvider},
    pixels::{ImageExpectations, PixelImage},
    texture::Texture,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone, Debug)]
pub enum TextureLocation {
    Resource {
        id: ResourceId,
        alias_reason: Option<Label>,
    },
    Literal {
        path: AssetPath,
        evidence: Label,
    },
    Bedrock {
        binding: EditionTextureBinding,
    },
}
#[derive(Clone, Debug)]
pub enum ScheduleSource {
    AutomaticJava,
    /// Candidate-local policy only when the selected Java member has no
    /// animation section. Real .mcmeta remains authoritative.
    AutomaticJavaWithMissing(MissingAnimation),
    NoMetadata,
    ExplicitPath(AssetPath),
    BedrockFlipbook {
        entry: usize,
        defaults: FlipbookDefaults,
    },
}
#[derive(Clone, Debug)]
pub struct TextureCandidate {
    pub pack: ResourceId,
    pub location: TextureLocation,
    pub schedule: ScheduleSource,
    pub expected_source_sha256: Option<Digest256>,
    pub expected_image: ImageExpectations,
}
#[derive(Clone, Debug)]
pub struct TextureRequest {
    pub requirement: TextureRequirement,
    pub candidates: Vec<TextureCandidate>,
    pub missing_animation: MissingAnimation,
}
#[derive(Debug, Serialize)]
pub struct CandidateRecord {
    pub pack: ResourceId,
    pub target_format: Option<PackFormat>,
    pub attempts: Vec<ResolutionAttempt>,
    #[serde(skip)]
    _reservations: Vec<Reservation>,
    pub alias_evidence: Option<Label>,
    pub found_origin: Option<BlobOrigin>,
    pub source_sha256: Option<Digest256>,
}
#[derive(Debug, Serialize)]
pub struct ImportRecord {
    pub resource: ResourceId,
    pub candidates: Vec<CandidateRecord>,
    pub source: Option<BlobOrigin>,
    pub source_sha256: Option<Digest256>,
    pub rgba_sha256: Option<Digest256>,
    pub dimensions: Option<[u32; 2]>,
    pub failure: Option<String>,
}
pub struct ImportResult {
    pub bank: TextureBank,
    pub records: Vec<ImportRecord>,
    _reservation: Reservation,
}
pub struct TextureImporter<'a> {
    packs: &'a [LayeredPack],
    limits: Limits,
    budget: ByteBudget,
}
impl<'a> TextureImporter<'a> {
    pub fn new(packs: &'a [LayeredPack], limits: Limits, budget: ByteBudget) -> Result<Self> {
        limits.validate()?;
        if packs.is_empty() || packs.len() > 32 {
            return Err(metadata::invalid(
                "one selected pack and at most 31 explicit fallback packs required",
            ));
        }
        let mut seen = BTreeSet::new();
        for pack in packs {
            // Keep the shared budget identity explicit.
            if !pack.uses_budget(&budget) {
                return Err(metadata::invalid(
                    "mounted source and importer accounts differ",
                ));
            } // Do not create independent per-pack budget escapes. // Traverse this bounded source, geometry, or sample sequence.
            if !seen.insert(pack.review().pack.clone()) {
                return Err(AssetError::Duplicate(pack.review().pack.to_string()));
            }
        }
        Ok(Self {
            packs,
            limits,
            budget,
        })
    }
    pub fn import(&self, requests: &[TextureRequest], cancel: Cancel<'_>) -> Result<ImportResult> {
        cancel.check()?;
        if requests.len() > self.limits.requirements {
            return Err(metadata::invalid("texture request count limit"));
        }
        let selected = self
            .packs
            .first()
            .ok_or_else(|| metadata::invalid("missing selected pack"))?
            .review()
            .clone();
        let fallbacks = self
            .packs
            .iter()
            .skip(1)
            .map(|pack| pack.review().clone())
            .collect();
        let mut builder = TextureBankBuilder::new(
            selected,
            fallbacks,
            self.limits,
            self.budget.clone(),
            cancel,
        )?;
        let reservation = self
            .budget
            .reserve(65536 + requests.len() as u64 * 32 * 1024, cancel)?;
        let mut image_cache = BTreeMap::<(Digest256, Digest256), Arc<PixelImage>>::new();
        let mut bedrock_cache = BTreeMap::<ResourceId, BedrockIndex>::new();
        let mut seen = BTreeSet::new();
        let mut records = Vec::new();
        for request in requests {
            cancel.check()?;
            if !seen.insert(request.requirement.id.clone()) {
                return Err(AssetError::Duplicate(request.requirement.id.to_string()));
            }
            let mut record = ImportRecord {
                resource: request.requirement.id.clone(),
                candidates: Vec::new(),
                source: None,
                source_sha256: None,
                rgba_sha256: None,
                dimensions: None,
                failure: None,
            };
            match self.load(
                request,
                &mut image_cache,
                &mut bedrock_cache,
                &mut record,
                cancel,
            ) {
                Ok(Some(texture)) => {
                    record.source = Some(texture.image().origin().clone());
                    record.source_sha256 = Some(texture.image().source_sha256());
                    record.rgba_sha256 = Some(texture.image().rgba_sha256());
                    record.dimensions = Some(texture.image().dimensions());
                    builder.insert(request.requirement.id.clone(), texture, cancel)?;
                }
                Ok(None) => {
                    record.failure = Some("resource absent from every explicit candidate".into());
                }
                Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
                Err(AssetError::Allocation) => return Err(AssetError::Allocation),
                Err(problem @ AssetError::Limit { .. }) => return Err(problem),
                Err(problem) => {
                    record.failure = Some(error::summary(&problem.to_string()));
                } // Preserve the explicit failure instead of treating it as absence.
            }
            records.push(record);
        }
        let requirements = requests
            .iter()
            .map(|request| request.requirement.clone())
            .collect();
        let bank = builder.finish(requirements, cancel)?;
        Ok(ImportResult {
            bank,
            records,
            _reservation: reservation,
        })
    }
    fn load(
        &self,
        request: &TextureRequest,
        images: &mut BTreeMap<(Digest256, Digest256), Arc<PixelImage>>,
        bedrock: &mut BTreeMap<ResourceId, BedrockIndex>,
        report: &mut ImportRecord,
        cancel: Cancel<'_>,
    ) -> Result<Option<Arc<Texture>>> {
        if request.candidates.is_empty() || request.candidates.len() > 16 {
            return Err(metadata::invalid("texture candidate count outside 1..16"));
        }
        for candidate in &request.candidates {
            cancel.check()?;
            let pack = self
                .packs
                .iter()
                .find(|pack| pack.review().pack == candidate.pack)
                .ok_or_else(|| {
                    metadata::invalid("candidate pack lacks an explicit mounted full-pack review")
                })?;
            let (resolved, reason) = match &candidate.location {
                TextureLocation::Resource { id, alias_reason } => {
                    if *id != request.requirement.id && alias_reason.is_none() {
                        return Err(metadata::invalid(
                            "renamed texture binding requires alias evidence",
                        ));
                    }
                    (
                        pack.read_resource(
                            &ResourceKey {
                                kind: ResourceKind::Texture,
                                id: id.clone(),
                            },
                            cancel,
                        )?,
                        alias_reason.clone(),
                    )
                }
                TextureLocation::Literal { path, evidence } => (
                    pack.read_path(path, self.limits.encoded_bytes, cancel)?,
                    Some(evidence.clone()),
                ),
                TextureLocation::Bedrock { binding } => {
                    if binding.semantic != request.requirement.id {
                        return Err(metadata::invalid(
                            "Bedrock semantic mapping identifies another resource",
                        ));
                    }
                    (
                        binding.resolve(pack, cancel)?,
                        Some(binding.evidence.clone()),
                    )
                }
            };
            let Resolution {
                blob,
                requested,
                target,
                attempts,
                reservations,
            } = resolved;
            report.candidates.push(CandidateRecord {
                pack: candidate.pack.clone(),
                target_format: target,
                attempts,
                _reservations: reservations,
                alias_evidence: reason,
                found_origin: blob.as_ref().map(|v| v.origin().clone()),
                source_sha256: blob.as_ref().map(|v| v.digest()),
            });
            let Some(blob) = blob else {
                continue;
            };
            if candidate
                .expected_source_sha256
                .is_some_and(|hash| hash != blob.digest())
            {
                return Err(AssetError::Integrity {
                    expected: candidate
                        .expected_source_sha256
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    actual: blob.digest().to_string(),
                });
            }
            let origin_bytes =
                serde_json::to_vec(blob.origin()).map_err(|e| metadata::invalid(&e.to_string()))?;
            let cache_key = (Digest256::of(&origin_bytes), blob.digest());
            if images
                .keys()
                .any(|(origin, digest)| *origin == cache_key.0 && *digest != cache_key.1)
            {
                return Err(metadata::invalid(
                    "one source origin changed bytes inside a single import transaction",
                ));
            }
            let image = match images.get(&cache_key) {
                Some(image) => Arc::clone(image),
                None => {
                    let decoded = if blob.origin().path.as_str().ends_with(".png") {
                        PixelImage::decode_png(
                            &blob,
                            candidate.expected_image,
                            &self.limits,
                            &self.budget,
                            cancel,
                        )?
                    } else if blob.origin().path.as_str().ends_with(".tga") {
                        PixelImage::decode_tga(
                            &blob,
                            candidate.expected_image,
                            &self.limits,
                            &self.budget,
                            cancel,
                        )?
                    } else {
                        return Err(AssetError::Unsupported(
                            "texture format is neither PNG nor TGA".into(),
                        ));
                    };
                    let image = Arc::new(decoded);
                    images.insert(cache_key, Arc::clone(&image));
                    image
                }
            };
            if candidate
                .expected_image
                .dimensions
                .is_some_and(|value| value != image.dimensions())
                || candidate
                    .expected_image
                    .rgba_sha256
                    .is_some_and(|value| value != image.rgba_sha256())
            {
                return Err(metadata::invalid(
                    "cached image differs from request's dimension/RGBA inspection pin",
                ));
            }
            let plan = match &candidate.schedule {
                ScheduleSource::BedrockFlipbook { entry, defaults } => {
                    if !bedrock.contains_key(&candidate.pack) {
                        bedrock.insert(candidate.pack.clone(), BedrockIndex::load(pack, cancel)?);
                    }
                    let table = bedrock
                        .get(&candidate.pack)
                        .and_then(|index| index.flipbooks.as_ref())
                        .ok_or_else(|| {
                            metadata::invalid("authored Bedrock flipbook table is absent")
                        })?;
                    let descriptor = table
                        .entries
                        .get(*entry)
                        .ok_or_else(|| metadata::invalid("missing flipbook entry"))?;
                    let supplied = requested.as_str();
                    let declared = descriptor.texture.as_str();
                    if supplied != declared
                        && supplied != format!("{declared}.png")
                        && supplied != format!("{declared}.tga")
                    {
                        return Err(metadata::invalid(
                            "flipbook texture does not match the resolved image path",
                        ));
                    }
                    table.plan(
                        *entry,
                        image.dimensions(),
                        defaults,
                        &self.limits,
                        &self.budget,
                        cancel,
                    )?
                }
                policy => {
                    let metadata_path = match policy {
                        ScheduleSource::NoMetadata => None,
                        ScheduleSource::ExplicitPath(path) => Some(path.clone()),
                        ScheduleSource::AutomaticJava
                        | ScheduleSource::AutomaticJavaWithMissing(_) => {
                            if pack.layout() == MountLayout::Bedrock {
                                return Err(metadata::invalid(
                                    "Bedrock animation requires explicit flipbook or no-metadata policy",
                                ));
                            }
                            Some(AssetPath::parse(&format!("{}.mcmeta", requested.as_str()))?)
                        }
                        ScheduleSource::BedrockFlipbook { .. } => {
                            return Err(metadata::invalid("unreachable flipbook policy"));
                        }
                    };
                    let metadata_blob = match metadata_path {
                        Some(path) => {
                            pack.read_path(&path, self.limits.metadata_bytes, cancel)?
                                .blob
                        }
                        None => None,
                    };
                    if matches!(policy, ScheduleSource::ExplicitPath(_)) && metadata_blob.is_none()
                    {
                        return Err(metadata::invalid(
                            "explicit animation metadata path is missing",
                        ));
                    }
                    let missing_animation = match policy {
                        ScheduleSource::AutomaticJavaWithMissing(missing) => missing,
                        _ => &request.missing_animation,
                    };
                    AnimationPlan::build(
                        image.dimensions(),
                        metadata_blob.as_ref(),
                        missing_animation,
                        &self.limits,
                        &self.budget,
                        cancel,
                    )?
                }
            };
            return Ok(Some(Arc::new(Texture::new(
                image,
                plan,
                request.requirement.encoding,
                &self.budget,
                cancel,
            )?)));
        }
        Ok(None)
    }
}
#[derive(Clone, Debug)]
pub struct DefinitionAlias {
    pub requested: ResourceKey,
    pub pack: ResourceId,
    pub source: ResourceKey,
    pub evidence: Label,
}
/// Explicit full-pack priority plus an original compatibility definition set.
/// Metadata failures stop resolution; a malformed selected model does not disappear into a fallback.
pub struct DefinitionSources<'a> {
    packs: &'a [LayeredPack],
    aliases: &'a [DefinitionAlias],
    compatibility: Option<&'a DefinitionSet>,
}
impl<'a> DefinitionSources<'a> {
    pub fn new(
        packs: &'a [LayeredPack],
        aliases: &'a [DefinitionAlias],
        compatibility: Option<&'a DefinitionSet>,
    ) -> Result<Self> {
        if packs.len() > 32 || aliases.len() > 8192 {
            return Err(metadata::invalid("definition source/alias limit"));
        }
        let mut seen = BTreeSet::new();
        for alias in aliases {
            if alias.requested.kind != alias.source.kind
                || !matches!(
                    alias.source.kind,
                    ResourceKind::Model | ResourceKind::Blockstate
                )
            {
                return Err(metadata::invalid("definition alias crosses resource kinds"));
            }
            if !seen.insert((alias.requested.clone(), alias.pack.clone())) {
                return Err(AssetError::Duplicate(alias.requested.id.to_string()));
            }
            if !packs.iter().any(|pack| pack.review().pack == alias.pack) {
                return Err(metadata::invalid("alias pack not mounted"));
            }
        }
        Ok(Self {
            packs,
            aliases,
            compatibility,
        })
    }
}
impl DefinitionProvider for DefinitionSources<'_> {
    fn definition(&self, key: &ResourceKey, cancel: Cancel<'_>) -> Result<Option<DefinitionInput>> {
        cancel.check()?;
        for pack in self.packs {
            if pack.layout() == MountLayout::Bedrock {
                continue;
            }
            let alias = self
                .aliases
                .iter()
                .find(|alias| alias.requested == *key && alias.pack == pack.review().pack);
            let actual = alias.map(|alias| &alias.source).unwrap_or(key);
            let result = pack.read_resource(actual, cancel)?;
            let Some(blob) = result.blob else {
                continue;
            };
            return Ok(Some(DefinitionInput {
                document: Document::parse(&blob, pack.limits(), pack.budget(), cancel)?,
                compatibility: alias.map(|alias| alias.evidence.clone()),
            }));
        }
        match self.compatibility {
            Some(definitions) => definitions.definition(key, cancel),
            None => Ok(None),
        }
    }
}
