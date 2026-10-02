use super::animation::AnimationEvidence;
use super::budget::{ByteBudget, Cancel, Limits, Reservation};
use super::error::{AssetError, Result};
use super::identity::{BlobOrigin, Digest256, OriginKind, ResourceId};
use super::review::{AssetPhase, FullPackReview};
use super::texture::{Encoding, Texture};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Ephemeral snapshot-local handle. A handle from another bank is rejected,
/// even when its slot happens to name a valid texture in this bank. Persist a
/// ResourceId and source review/configuration, never this handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextureHandle {
    bank: Digest256,
    slot: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredOrigin {
    SelectedPack,
    SelectedOrExplicitFullPackFallback,
    CustomArtist,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredSchedule {
    Any,
    /// A geometry/timeline property, NOT proof that frame pixels differ.
    ChangingRectangles,
    /// Source-supplied animation metadata, with format defaults recorded.
    AuthoredChangingRectangles,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureRequirement {
    pub id: ResourceId,
    pub required: bool,
    pub encoding: Encoding,
    pub origin: RequiredOrigin,
    pub schedule: RequiredSchedule,
    pub dimensions: Option<[u32; 2]>,
}
impl TextureRequirement {
    pub fn selected_color(id: ResourceId) -> Self {
        Self {
            id,
            required: true,
            encoding: Encoding::SrgbColor,
            origin: RequiredOrigin::SelectedPack,
            schedule: RequiredSchedule::Any,
            dimensions: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoverageProblem {
    Missing,
    WrongEncoding {
        expected: Encoding,
        actual: Encoding,
    },
    WrongDimensions {
        expected: [u32; 2],
        actual: [u32; 2],
    },
    WrongOrigin {
        expected: RequiredOrigin,
        actual: OriginKind,
    },
    NoChangingRectangles,
    NoAuthoredAnimation,
}
#[derive(Debug, Clone, Serialize)]
pub struct CoverageRow {
    pub requirement: TextureRequirement,
    pub handle: Option<TextureHandle>,
    pub problems: Vec<CoverageProblem>,
}
#[derive(Debug, Serialize)]
pub struct CoverageReport {
    pub rows: Vec<CoverageRow>,
    pub required_count: usize,
    pub required_satisfied: usize,
    /// Texture requirements only. False for an empty required set. This field
    /// must NEVER be shown as overall pack/world/renderer/visual acceptance.
    pub required_textures_satisfied: bool,
    pub selected_texture_count: usize,
    pub explicit_fallback_texture_count: usize,
    pub diagnostic_texture_count: usize,
}

#[derive(Debug)]
pub struct TextureBank {
    identity: Digest256,
    selected: FullPackReview,
    reviews: BTreeMap<ResourceId, FullPackReview>,
    textures: Vec<(ResourceId, Arc<Texture>)>,
    coverage: CoverageReport,
    _reservations: Vec<Reservation>,
}
impl TextureBank {
    pub fn identity(&self) -> Digest256 {
        self.identity
    }
    pub fn selected_review(&self) -> &FullPackReview {
        &self.selected
    }
    pub fn reviews(&self) -> &BTreeMap<ResourceId, FullPackReview> {
        &self.reviews
    }
    pub fn coverage(&self) -> &CoverageReport {
        &self.coverage
    }
    pub fn len(&self) -> usize {
        self.textures.len()
    }
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
    /// Resolve once while binding geometry on the worker, not per raster dot.
    pub fn resolve(&self, id: &ResourceId) -> Option<TextureHandle> {
        let slot = self
            .textures
            .binary_search_by(|(candidate, _)| candidate.cmp(id))
            .ok()?;
        Some(TextureHandle {
            bank: self.identity,
            slot: slot as u32,
        })
    }
    pub fn texture(&self, handle: TextureHandle) -> Option<&Texture> {
        if handle.bank != self.identity {
            return None;
        }
        self.textures
            .get(handle.slot as usize)
            .map(|(_, texture)| texture.as_ref())
    }
    pub fn resources(&self) -> impl Iterator<Item = (&ResourceId, &Texture)> {
        self.textures
            .iter()
            .map(|(id, texture)| (id, texture.as_ref()))
    }
}

/// A worker-only transaction. No partially populated bank is published. A failed
/// build drops reservations; successful immutable data remain charged until the
/// last Arc is dropped. Texture aliases should share Arc<Texture> or PixelImage.
/// Layer/overlay precedence belongs to the resolver upstream: duplicate FINAL
/// bindings here are errors, never an arbitrary last-file-wins policy.
#[derive(Debug)]
pub struct TextureBankBuilder {
    selected: FullPackReview,
    reviews: BTreeMap<ResourceId, FullPackReview>,
    review_digests: BTreeMap<ResourceId, Digest256>,
    textures: BTreeMap<ResourceId, Arc<Texture>>,
    limits: Limits,
    budget: ByteBudget,
    reservations: Vec<Reservation>,
}
impl TextureBankBuilder {
    pub fn new(
        selected: FullPackReview,
        fallback_reviews: Vec<FullPackReview>,
        limits: Limits,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        limits.validate()?;
        cancel.check()?;
        selected.validate()?;
        if fallback_reviews.len() > 32 {
            return Err(AssetError::Limit {
                resource: "full-pack fallback reviews",
                requested: fallback_reviews.len() as u64,
                limit: 32,
            });
        }
        // Each review has <= 102 bounded labels/IDs; account a conservative
        // logical allowance before cloning/indexing records and hashing them.
        let mut reservations = Vec::new();
        reservations
            .try_reserve_exact(limits.textures + 3)
            .map_err(|_| AssetError::Allocation)?;
        reservations.push(budget.reserve(
            ((fallback_reviews.len() + 2) * 112 * 1024) as u64
                + ((limits.textures + 3) * std::mem::size_of::<Reservation>()) as u64,
            cancel,
        )?);
        let mut reviews = BTreeMap::new();
        reviews.insert(selected.pack.clone(), selected.clone());
        for review in fallback_reviews {
            cancel.check()?;
            review.validate()?;
            if reviews.contains_key(&review.pack) {
                return Err(AssetError::Duplicate(review.pack.as_str().into()));
            }
            reviews.insert(review.pack.clone(), review);
        }
        let mut review_digests = BTreeMap::new();
        for (id, review) in &reviews {
            cancel.check()?;
            review_digests.insert(id.clone(), review.digest()?);
        }
        Ok(Self {
            selected,
            reviews,
            review_digests,
            textures: BTreeMap::new(),
            limits,
            budget,
            reservations,
        })
    }
    pub fn insert(
        &mut self,
        id: ResourceId,
        texture: Arc<Texture>,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        cancel.check()?;
        if self.textures.contains_key(&id) {
            return Err(AssetError::Duplicate(id.as_str().into()));
        }
        if self.textures.len() >= self.limits.textures {
            return Err(AssetError::Limit {
                resource: "textures",
                requested: self.textures.len() as u64 + 1,
                limit: self.limits.textures as u64,
            });
        }
        if !texture.uses_budget(&self.budget) {
            return Err(AssetError::InvalidMetadata(
                "texture was charged to a different byte account".into(),
            ));
        }
        self.validate_origin(texture.image().origin())?;
        if let AnimationEvidence::JavaMetadata { origin, .. }
        | AnimationEvidence::BedrockMetadata { origin, .. }
        | AnimationEvidence::MetadataWithoutAnimation { origin, .. } =
            texture.animation().evidence()
        {
            self.validate_origin(origin)?;
        }
        let reservation = self
            .budget
            .reserve(1024 + id.as_str().len() as u64, cancel)?;
        self.textures.insert(id, texture);
        self.reservations.push(reservation);
        Ok(())
    }
    fn validate_origin(&self, origin: &BlobOrigin) -> Result<()> {
        let review = self.reviews.get(&origin.pack).ok_or_else(|| {
            AssetError::InvalidReview(
                "pixel/metadata source has no explicit full-pack review".into(),
            )
        })?;
        if review.release != origin.release
            || self.review_digests.get(&origin.pack) != Some(&origin.review_digest)
        {
            return Err(AssetError::InvalidReview(
                "pixel/metadata release or review identity differs".into(),
            ));
        }
        let is_selected = origin.pack == self.selected.pack;
        let valid = match origin.kind {
            OriginKind::SelectedPack | OriginKind::OfficialInternalLayer => is_selected,
            OriginKind::ExplicitFullPackFallback => !is_selected,
            OriginKind::CustomArtist => review.phase == AssetPhase::CustomArtist,
            // Test/probe data can be inspected, but can never satisfy an
            // authentic selected/fallback/custom texture requirement below.
            OriginKind::DiagnosticFixture => true,
            OriginKind::OriginalCompatibilityGeometry => false,
        };
        if !valid {
            return Err(AssetError::InvalidReview(
                "source role does not match the selected/fallback/custom review".into(),
            ));
        }
        Ok(())
    }
    pub fn finish(
        mut self,
        requirements: Vec<TextureRequirement>,
        cancel: Cancel<'_>,
    ) -> Result<TextureBank> {
        cancel.check()?;
        if requirements.len() > self.limits.requirements {
            return Err(AssetError::Limit {
                resource: "texture requirements",
                requested: requirements.len() as u64,
                limit: self.limits.requirements as u64,
            });
        }
        self.reservations.push(self.budget.reserve(
            (requirements.len() as u64 * 2048) + self.textures.len() as u64 * 128,
            cancel,
        )?);
        let mut seen = BTreeSet::new();
        for requirement in &requirements {
            cancel.check()?;
            if !seen.insert(requirement.id.clone()) {
                return Err(AssetError::Duplicate(requirement.id.as_str().into()));
            }
            if let Some([w, h]) = requirement.dimensions {
                self.limits.rgba_bytes(w, h)?;
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"ilium-overworld-bank-v1\0");
        hash.update(self.selected.digest()?.bytes());
        for (id, review) in &self.reviews {
            cancel.check()?;
            hash.update((id.as_str().len() as u64).to_le_bytes());
            hash.update(id.as_str().as_bytes());
            hash.update(review.digest()?.bytes());
        }
        let mut textures = Vec::new();
        textures
            .try_reserve_exact(self.textures.len())
            .map_err(|_| AssetError::Allocation)?;
        for (id, texture) in self.textures {
            cancel.check()?;
            hash.update((id.as_str().len() as u64).to_le_bytes());
            hash.update(id.as_str().as_bytes());
            hash.update(texture.fingerprint().bytes());
            // Same RGBA with a different claimed origin is a different bank.
            let origin = serde_json::to_vec(texture.image().origin())
                .map_err(|e| AssetError::InvalidMetadata(super::error::summary(&e.to_string())))?;
            hash.update((origin.len() as u64).to_le_bytes());
            hash.update(origin);
            textures.push((id, texture));
        }
        let identity = Digest256::of(&hash.finalize());
        let mut report = CoverageReport {
            rows: Vec::new(),
            required_count: 0,
            required_satisfied: 0,
            required_textures_satisfied: false,
            selected_texture_count: 0,
            explicit_fallback_texture_count: 0,
            diagnostic_texture_count: 0,
        };
        report
            .rows
            .try_reserve_exact(requirements.len())
            .map_err(|_| AssetError::Allocation)?;
        for (_, texture) in &textures {
            match texture.image().origin().kind {
                OriginKind::DiagnosticFixture => report.diagnostic_texture_count += 1,
                OriginKind::OriginalCompatibilityGeometry => {
                    return Err(AssetError::InvalidReview(
                        "original compatibility geometry cannot be a texture origin".into(),
                    ))
                }
                OriginKind::ExplicitFullPackFallback => report.explicit_fallback_texture_count += 1,
                OriginKind::SelectedPack | OriginKind::OfficialInternalLayer => {
                    report.selected_texture_count += 1
                }
                OriginKind::CustomArtist => {
                    if texture.image().origin().pack == self.selected.pack {
                        report.selected_texture_count += 1;
                    } else {
                        report.explicit_fallback_texture_count += 1;
                    }
                }
            }
        }
        for requirement in requirements {
            cancel.check()?;
            let found = textures
                .binary_search_by(|(id, _)| id.cmp(&requirement.id))
                .ok();
            let mut problems = Vec::new();
            problems
                .try_reserve_exact(5)
                .map_err(|_| AssetError::Allocation)?;
            let handle = found.map(|slot| TextureHandle {
                bank: identity,
                slot: slot as u32,
            });
            if let Some(slot) = found {
                let texture = &textures[slot].1;
                if texture.encoding() != requirement.encoding {
                    problems.push(CoverageProblem::WrongEncoding {
                        expected: requirement.encoding,
                        actual: texture.encoding(),
                    });
                }
                if let Some(expected) = requirement.dimensions {
                    if expected != texture.image().dimensions() {
                        problems.push(CoverageProblem::WrongDimensions {
                            expected,
                            actual: texture.image().dimensions(),
                        });
                    }
                }
                let origin = texture.image().origin();
                let actual_selected = origin.pack == self.selected.pack
                    && matches!(
                        origin.kind,
                        OriginKind::SelectedPack
                            | OriginKind::OfficialInternalLayer
                            | OriginKind::CustomArtist
                    );
                let matches_origin = match requirement.origin {
                    RequiredOrigin::SelectedPack => actual_selected,
                    RequiredOrigin::SelectedOrExplicitFullPackFallback => {
                        actual_selected
                            || matches!(
                                origin.kind,
                                OriginKind::ExplicitFullPackFallback | OriginKind::CustomArtist
                            )
                    }
                    RequiredOrigin::CustomArtist => origin.kind == OriginKind::CustomArtist,
                };
                if !matches_origin {
                    problems.push(CoverageProblem::WrongOrigin {
                        expected: requirement.origin,
                        actual: origin.kind,
                    });
                }
                if requirement.schedule != RequiredSchedule::Any
                    && !texture.animation().changing_rects()
                {
                    problems.push(CoverageProblem::NoChangingRectangles);
                }
                if requirement.schedule == RequiredSchedule::AuthoredChangingRectangles
                    && !texture.animation().authored_schedule()
                {
                    problems.push(CoverageProblem::NoAuthoredAnimation);
                }
            } else {
                problems.push(CoverageProblem::Missing);
            }
            if requirement.required {
                report.required_count += 1;
                if problems.is_empty() {
                    report.required_satisfied += 1;
                }
            }
            report.rows.push(CoverageRow {
                requirement,
                handle,
                problems,
            });
        }
        report.required_textures_satisfied =
            report.required_count > 0 && report.required_satisfied == report.required_count;
        cancel.check()?;
        Ok(TextureBank {
            identity,
            selected: self.selected,
            reviews: self.reviews,
            textures,
            coverage: report,
            _reservations: self.reservations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        animation::{MissingAnimation, PixelRect},
        identity::{AssetPath, Label},
        review::{fixture_origin, fixture_review},
        texture::fixture_texture,
    };
    use super::*;
    use std::sync::atomic::AtomicBool;
    fn id(s: &str) -> ResourceId {
        ResourceId::parse(s).unwrap()
    }
    fn budget() -> ByteBudget {
        ByteBudget::new(512 * 1024 * 1024).unwrap()
    }
    fn tex(color: [u8; 4], kind: OriginKind, b: &ByteBudget) -> Arc<Texture> {
        fixture_texture(
            [1, 1],
            &color,
            None,
            &MissingAnimation::StaticImage,
            Encoding::SrgbColor,
            fixture_origin(kind),
            b,
        )
    }
    fn sampler_texture(origin: BlobOrigin, json: &str, b: &ByteBudget) -> Arc<Texture> {
        use super::super::{
            animation::AnimationPlan,
            identity::SourceBlob,
            pixels::{fixture_png, ImageExpectations, PixelImage},
        };
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let l = Limits::default();
        let blob = SourceBlob::new(
            fixture_png(1, 1, &[255; 4]),
            fixture_origin(OriginKind::SelectedPack),
            None,
            &l,
            b,
            c,
        )
        .unwrap();
        let image = Arc::new(
            PixelImage::decode_png(&blob, ImageExpectations::default(), &l, b, c).unwrap(),
        );
        let metadata = SourceBlob::new(json.as_bytes().to_vec(), origin, None, &l, b, c).unwrap();
        let plan = AnimationPlan::build(
            [1, 1],
            Some(&metadata),
            &MissingAnimation::StaticImage,
            &l,
            b,
            c,
        )
        .unwrap();
        Arc::new(Texture::new(image, plan, Encoding::SrgbColor, b, c).unwrap())
    }
    #[test]
    fn texture_only_metadata_requires_its_own_review() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let mut foreign = fixture_origin(OriginKind::SelectedPack);
        foreign.pack = id("fixture:unreviewed");
        let texture = sampler_texture(foreign, r#"{"texture":{"blur":true}}"#, &b);
        assert!(!texture.animation().authored_schedule());
        let mut builder =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b, c).unwrap();
        assert!(matches!(
            builder.insert(id("block/a"), texture, c),
            Err(AssetError::InvalidReview(_))
        ));
    }
    #[test]
    fn texture_only_metadata_identity_retains_origin_and_original_bytes() {
        let b = budget();
        let mut first = fixture_origin(OriginKind::SelectedPack);
        first.path = AssetPath::parse("first.mcmeta").unwrap();
        let mut second = first.clone();
        second.path = AssetPath::parse("second.mcmeta").unwrap();
        let a = sampler_texture(first.clone(), r#"{"texture":{"blur":true}}"#, &b);
        let changed_origin = sampler_texture(second, r#"{"texture":{"blur":true}}"#, &b);
        let changed_bytes = sampler_texture(first, r#"{ "texture":{"blur":true}}"#, &b);
        assert_ne!(a.fingerprint(), changed_origin.fingerprint());
        assert_ne!(a.fingerprint(), changed_bytes.fingerprint());
    }
    #[test]
    fn an_empty_requirement_set_never_claims_readiness() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let bank =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), budget(), c)
                .unwrap()
                .finish(vec![], c)
                .unwrap();
        assert!(!bank.coverage().required_textures_satisfied);
        assert!(bank.is_empty());
    }
    #[test]
    fn missing_and_diagnostic_assets_remain_visible_failures() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let mut builder =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b.clone(), c)
                .unwrap();
        builder
            .insert(
                id("block/grass_block_top"),
                tex([0, 255, 0, 255], OriginKind::DiagnosticFixture, &b),
                c,
            )
            .unwrap();
        let bank = builder
            .finish(
                vec![
                    TextureRequirement::selected_color(id("block/grass_block_top")),
                    TextureRequirement::selected_color(id("block/water_still")),
                ],
                c,
            )
            .unwrap();
        assert_eq!(bank.coverage().required_count, 2);
        assert_eq!(bank.coverage().required_satisfied, 0);
        assert!(matches!(
            bank.coverage().rows[0].problems[0],
            CoverageProblem::WrongOrigin { .. }
        ));
        assert_eq!(
            bank.coverage().rows[1].problems,
            vec![CoverageProblem::Missing]
        );
        assert!(!bank.coverage().required_textures_satisfied);
    }
    #[test]
    fn bank_identity_is_order_independent_and_stale_handles_are_rejected() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let make = |reverse: bool, blue: u8| {
            let mut builder =
                TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b.clone(), c)
                    .unwrap();
            let mut rows = vec![
                (
                    id("block/a"),
                    tex([255, 0, 0, 255], OriginKind::SelectedPack, &b),
                ),
                (
                    id("block/b"),
                    tex([0, 0, blue, 255], OriginKind::SelectedPack, &b),
                ),
            ];
            if reverse {
                rows.reverse();
            }
            for (id, texture) in rows {
                builder.insert(id, texture, c).unwrap();
            }
            builder
                .finish(vec![TextureRequirement::selected_color(id("block/a"))], c)
                .unwrap()
        };
        let first = make(false, 255);
        let reordered = make(true, 255);
        let changed = make(false, 128);
        assert_eq!(first.identity(), reordered.identity());
        assert_ne!(first.identity(), changed.identity());
        let handle = first.resolve(&id("block/a")).unwrap();
        assert!(reordered.texture(handle).is_some());
        assert!(changed.texture(handle).is_none());
        assert!(first.coverage().required_textures_satisfied);
    }
    #[test]
    fn fallback_requires_its_own_review_and_never_counts_as_selected_pack() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let mut other = fixture_review();
        other.pack = id("fixture:other");
        let origin = BlobOrigin {
            pack: other.pack.clone(),
            release: other.release.clone(),
            layer: Label::new("explicit entity fallback").unwrap(),
            path: AssetPath::parse("cow.png").unwrap(),
            review_digest: other.digest().unwrap(),
            kind: OriginKind::ExplicitFullPackFallback,
        };
        let texture = fixture_texture(
            [1, 1],
            &[255; 4],
            None,
            &MissingAnimation::StaticImage,
            Encoding::SrgbColor,
            origin,
            &b,
        );
        let mut absent =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b.clone(), c)
                .unwrap();
        assert!(absent
            .insert(id("entity/cow"), Arc::clone(&texture), c)
            .is_err());
        let make = |requirement| {
            let mut builder = TextureBankBuilder::new(
                fixture_review(),
                vec![other.clone()],
                Limits::default(),
                b.clone(),
                c,
            )
            .unwrap();
            builder
                .insert(id("entity/cow"), Arc::clone(&texture), c)
                .unwrap();
            builder.finish(vec![requirement], c).unwrap()
        };
        let mut requirement = TextureRequirement::selected_color(id("entity/cow"));
        assert!(
            !make(requirement.clone())
                .coverage()
                .required_textures_satisfied
        );
        requirement.origin = RequiredOrigin::SelectedOrExplicitFullPackFallback;
        let bank = make(requirement);
        assert!(bank.coverage().required_textures_satisfied);
        assert_eq!(bank.coverage().selected_texture_count, 0);
        assert_eq!(bank.coverage().explicit_fallback_texture_count, 1);
    }
    #[test]
    fn goodvibes_style_static_crop_cannot_satisfy_authored_frame_requirement() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let t = fixture_texture(
            [2, 5],
            &[153; 40],
            None,
            &MissingAnimation::StaticCrop {
                rect: PixelRect::whole(2, 2),
                reason: Label::new("explicit static crop, authored frames unavailable").unwrap(),
            },
            Encoding::SrgbColor,
            fixture_origin(OriginKind::SelectedPack),
            &b,
        );
        let mut builder =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b, c).unwrap();
        builder.insert(id("block/water_flow"), t, c).unwrap();
        let mut requirement = TextureRequirement::selected_color(id("block/water_flow"));
        requirement.schedule = RequiredSchedule::AuthoredChangingRectangles;
        let bank = builder.finish(vec![requirement], c).unwrap();
        assert_eq!(
            bank.coverage().rows[0].problems,
            vec![
                CoverageProblem::NoChangingRectangles,
                CoverageProblem::NoAuthoredAnimation
            ]
        );
    }
    #[test]
    fn duplicate_bindings_and_foreign_budget_are_rejected_without_overwrite() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let mut builder =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b.clone(), c)
                .unwrap();
        builder
            .insert(
                id("block/a"),
                tex([255; 4], OriginKind::SelectedPack, &b),
                c,
            )
            .unwrap();
        assert!(builder
            .insert(id("block/a"), tex([0; 4], OriginKind::SelectedPack, &b), c)
            .is_err());
        assert!(builder
            .insert(
                id("block/b"),
                tex([255; 4], OriginKind::SelectedPack, &budget()),
                c
            )
            .is_err());
    }
    #[test]
    fn cancellation_and_failed_transaction_release_all_live_reservations() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        {
            let mut builder =
                TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b.clone(), c)
                    .unwrap();
            builder
                .insert(
                    id("block/a"),
                    tex([255; 4], OriginKind::SelectedPack, &b),
                    c,
                )
                .unwrap();
            stop.store(true, std::sync::atomic::Ordering::Release);
            assert!(matches!(
                builder.finish(vec![], c),
                Err(AssetError::Cancelled)
            ));
        }
        assert_eq!(b.used(), 0);
    }
    #[test]
    fn explicit_author_credit_and_release_identity_cannot_be_silently_changed() {
        let stop = AtomicBool::new(false);
        let c = Cancel::new(&stop);
        let b = budget();
        let mut origin = fixture_origin(OriginKind::SelectedPack);
        origin.review_digest = Digest256::of(b"unreviewed");
        let t = fixture_texture(
            [1, 1],
            &[255; 4],
            None,
            &MissingAnimation::StaticImage,
            Encoding::SrgbColor,
            origin,
            &b,
        );
        let mut builder =
            TextureBankBuilder::new(fixture_review(), vec![], Limits::default(), b, c).unwrap();
        assert!(builder.insert(id("block/a"), t, c).is_err());
    }
}
