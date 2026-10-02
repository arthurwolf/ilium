use super::budget::{ByteBudget, Cancel, Limits, Reservation};
use super::error::{AssetError, Result};
use super::identity::{BlobOrigin, Digest256, Label, OriginKind, SourceBlob};
use serde::{de, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, marker::PhantomData, time::Duration};

pub const TICK_NANOS: u128 = 50_000_000;
const HARD_FRAME_LIMIT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl PixelRect {
    pub fn whole(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
    pub fn validate(self, image: [u32; 2]) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || u64::from(self.x) + u64::from(self.width) > u64::from(image[0])
            || u64::from(self.y) + u64::from(self.height) > u64::from(image[1])
        {
            return Err(AssetError::InvalidMetadata(
                "frame/crop rectangle is empty or outside the decoded image".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitFrame {
    pub rect: PixelRect,
    pub ticks: u32,
}

/// These policies are explicit LOCAL compatibility decisions for a missing
/// animation section. An existing malformed animation section always errors.
/// There is intentionally no infer-square-strip-with-invented-frametime mode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissingAnimation {
    #[default]
    StaticImage,
    StaticCrop {
        rect: PixelRect,
        reason: Label,
    },
    ExplicitFrames {
        #[serde(deserialize_with = "bounded_vec")]
        frames: Vec<ExplicitFrame>,
        interpolate: bool,
        reason: Label,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sampler {
    pub blur: bool,
    pub clamp: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum MipmapStrategy {
    Mean,
    DarkCutout,
    StrictCutout,
}

/// Java26.3 also describes mipmap generation in the texture section. The
/// software renderer samples the original image and generates no mip levels;
/// these controls therefore do not change its base-level pixels. Their exact
/// source bytes remain retained by AnimationEvidence's metadata identity.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TextureMetadata {
    blur: bool,
    clamp: bool,
    mipmap_strategy: Option<MipmapStrategy>,
    alpha_cutoff_bias: Option<f32>,
}

impl TextureMetadata {
    fn base_sampler(self) -> Result<Sampler> {
        if self.alpha_cutoff_bias.is_some_and(|bias| !bias.is_finite()) {
            return Err(AssetError::InvalidMetadata(
                "nonfinite alpha cutoff bias".into(),
            ));
        }
        // Recognized strategy values are validated by the typed parser.
        let _base_level_only = self.mipmap_strategy;
        Ok(Sampler {
            blur: self.blur,
            clamp: self.clamp,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnimationEvidence {
    StaticImage,
    JavaMetadata {
        origin: BlobOrigin,
        sha256: Digest256,
        default_frame_time: bool,
        default_frame_size: bool,
        default_sequence: bool,
    },
    BedrockMetadata {
        origin: BlobOrigin,
        sha256: Digest256,
        defaults: Option<Label>,
        initial_frame: Option<u32>,
        default_frame_time: bool,
        default_interpolation: bool,
        default_sequence: bool,
        default_frame_size: bool,
    },
    Compatibility {
        reason: Label,
        static_crop: bool,
    },
    /// Supplied metadata remains a provenance-bearing source even when it
    /// provides only sampler settings or other non-animation sections.
    MetadataWithoutAnimation {
        origin: BlobOrigin,
        sha256: Digest256,
        fallback: Box<AnimationEvidence>,
    },
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    rect: PixelRect,
    end_tick: u64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSample {
    pub current: PixelRect,
    pub next: PixelRect,
    pub blend: f32,
}

/// Validated immutable timeline; sampling allocates nothing and uses an integer
/// nanosecond phase, including Duration::MAX. It is separate from image pixels.
#[derive(Debug)]
pub struct AnimationPlan {
    image: [u32; 2],
    frames: Vec<Frame>,
    cycle_ticks: u64,
    interpolate: bool,
    sampler: Sampler,
    evidence: AnimationEvidence,
    changing_rects: bool,
    fingerprint: Digest256,
    _reservation: Reservation,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default, deserialize_with = "present_animation")]
    animation: Option<JavaAnimation>,
    #[serde(default)]
    texture: TextureMetadata,
}
fn present_animation<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<JavaAnimation>, D::Error> {
    // Missing is allowed by serde(default); an explicitly null section is bad
    // metadata, not permission to silently select the missing-metadata fallback.
    JavaAnimation::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JavaAnimation {
    #[serde(default, deserialize_with = "present_u32")]
    width: Option<u32>,
    #[serde(default, deserialize_with = "present_u32")]
    height: Option<u32>,
    #[serde(default, deserialize_with = "present_u32")]
    frametime: Option<u32>,
    #[serde(default)]
    interpolate: bool,
    #[serde(default, deserialize_with = "bounded_optional_vec")]
    frames: Option<Vec<JavaFrame>>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum JavaFrame {
    Index(u32),
    Timed(TimedJavaFrame),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimedJavaFrame {
    index: u32,
    #[serde(default, deserialize_with = "present_u32")]
    time: Option<u32>,
}
fn present_u32<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u32>, D::Error> {
    u32::deserialize(d).map(Some)
}

// Reject an oversized array during deserialization, not after allocating it.
fn bounded_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an array containing at most 4096 frames")
        }
        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Vec<T>, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == HARD_FRAME_LIMIT {
                    return Err(de::Error::custom("too many animation frames"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Visitor(PhantomData))
}
fn bounded_optional_vec<'de, D, T>(deserializer: D) -> std::result::Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // serde(default) handles absence. A supplied null/non-array is malformed.
    bounded_vec(deserializer).map(Some)
}

impl AnimationPlan {
    pub fn build(
        image: [u32; 2],
        metadata: Option<&SourceBlob>,
        fallback: &MissingAnimation,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        limits.rgba_bytes(image[0], image[1])?;
        cancel.check()?;
        let mut sampler = Sampler::default();
        if let Some(blob) = metadata {
            if !blob.uses_budget(budget) {
                return Err(AssetError::InvalidMetadata(
                    "metadata and timeline must share the texture-bank byte account".into(),
                ));
            }
            if blob.bytes().len() as u64 > limits.metadata_bytes {
                return Err(AssetError::Limit {
                    resource: "metadata bytes",
                    requested: blob.bytes().len() as u64,
                    limit: limits.metadata_bytes,
                });
            }
            // Parsing is worker-only. The typed arrays cap their own growth;
            // serde_json retains its normal recursion limit.
            let _parse = budget.reserve(limits.metadata_bytes * 4, cancel)?;
            let parsed: Metadata = serde_json::from_slice(blob.bytes())
                .map_err(|e| AssetError::InvalidMetadata(super::error::summary(&e.to_string())))?;
            cancel.check()?;
            sampler = parsed.texture.base_sampler()?;
            if let Some(animation) = parsed.animation {
                return Self::java(image, animation, blob, sampler, limits, budget, cancel);
            }
        }
        let _temporary_frames = budget.reserve(
            (limits.animation_frames * std::mem::size_of::<ExplicitFrame>()) as u64,
            cancel,
        )?;
        let (frames, interpolation, evidence) = match fallback {
            MissingAnimation::StaticImage => (
                vec![ExplicitFrame {
                    rect: PixelRect::whole(image[0], image[1]),
                    ticks: 1,
                }],
                false,
                AnimationEvidence::StaticImage,
            ),
            MissingAnimation::StaticCrop { rect, reason } => (
                vec![ExplicitFrame {
                    rect: *rect,
                    ticks: 1,
                }],
                false,
                AnimationEvidence::Compatibility {
                    reason: reason.clone(),
                    static_crop: true,
                },
            ),
            MissingAnimation::ExplicitFrames {
                frames,
                interpolate,
                reason,
            } => {
                if frames.len() > limits.animation_frames {
                    return Err(AssetError::Limit {
                        resource: "animation frames",
                        requested: frames.len() as u64,
                        limit: limits.animation_frames as u64,
                    });
                }
                (
                    frames.clone(),
                    *interpolate,
                    AnimationEvidence::Compatibility {
                        reason: reason.clone(),
                        static_crop: false,
                    },
                )
            }
        };
        let evidence = if let Some(blob) = metadata {
            AnimationEvidence::MetadataWithoutAnimation {
                origin: blob.origin().clone(),
                sha256: blob.digest(),
                fallback: Box::new(evidence),
            }
        } else {
            evidence
        };
        Self::from_frames(
            image,
            &frames,
            interpolation,
            sampler,
            evidence,
            limits,
            budget,
            cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn java(
        image: [u32; 2],
        animation: JavaAnimation,
        blob: &SourceBlob,
        sampler: Sampler,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        // Versioned Java compatibility rule: one supplied dimension leaves the
        // other at the complete image extent; neither means min(width,height).
        let extent = match (animation.width, animation.height) {
            (Some(w), Some(h)) => [w, h],
            (Some(w), None) => [w, image[1]],
            (None, Some(h)) => [image[0], h],
            (None, None) => [image[0].min(image[1]); 2],
        };
        if extent.contains(&0)
            || !image[0].is_multiple_of(extent[0])
            || !image[1].is_multiple_of(extent[1])
        {
            return Err(AssetError::InvalidMetadata(
                "animation grid does not exactly divide the decoded image".into(),
            ));
        }
        let columns = image[0] / extent[0];
        let grid_count = u64::from(columns) * u64::from(image[1] / extent[1]);
        let default_ticks = animation.frametime.unwrap_or(1);
        if default_ticks == 0 || default_ticks > limits.frame_ticks {
            return Err(AssetError::InvalidMetadata(
                "frametime must be a positive bounded tick count".into(),
            ));
        }
        let default_sequence = animation.frames.as_ref().is_none_or(Vec::is_empty);
        let count = if default_sequence {
            grid_count
        } else {
            animation.frames.as_ref().map_or(0, Vec::len) as u64
        };
        if count == 0 || count > limits.animation_frames as u64 {
            return Err(AssetError::Limit {
                resource: "animation frames",
                requested: count,
                limit: limits.animation_frames as u64,
            });
        }
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(count as usize)
            .map_err(|_| AssetError::Allocation)?;
        let mut push = |index: u64, ticks: u32| -> Result<()> {
            cancel.check()?;
            if index >= grid_count {
                return Err(AssetError::InvalidMetadata(
                    "animation frame index outside the image grid".into(),
                ));
            }
            frames.push(ExplicitFrame {
                rect: PixelRect {
                    x: (index % u64::from(columns)) as u32 * extent[0],
                    y: (index / u64::from(columns)) as u32 * extent[1],
                    width: extent[0],
                    height: extent[1],
                },
                ticks,
            });
            Ok(())
        };
        if default_sequence {
            for index in 0..count {
                push(index, default_ticks)?;
            }
        } else if let Some(selected) = animation.frames.as_ref() {
            for frame in selected {
                match frame {
                    JavaFrame::Index(index) => push(u64::from(*index), default_ticks)?,
                    JavaFrame::Timed(timed) => {
                        push(u64::from(timed.index), timed.time.unwrap_or(default_ticks))?
                    }
                }
            }
        }
        let evidence = AnimationEvidence::JavaMetadata {
            origin: blob.origin().clone(),
            sha256: blob.digest(),
            default_frame_time: animation.frametime.is_none(),
            default_frame_size: animation.width.is_none() || animation.height.is_none(),
            default_sequence,
        };
        Self::from_frames(
            image,
            &frames,
            animation.interpolate,
            sampler,
            evidence,
            limits,
            budget,
            cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_normalized_frames(
        image: [u32; 2],
        frames: &[ExplicitFrame],
        interpolate: bool,
        sampler: Sampler,
        evidence: AnimationEvidence,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        Self::from_frames(
            image,
            frames,
            interpolate,
            sampler,
            evidence,
            limits,
            budget,
            cancel,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "frame provenance, budget and cancellation remain explicit at decode admission"
    )]
    fn from_frames(
        image: [u32; 2],
        requested: &[ExplicitFrame],
        interpolate: bool,
        sampler: Sampler,
        evidence: AnimationEvidence,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        if requested.is_empty() || requested.len() > limits.animation_frames {
            return Err(AssetError::InvalidMetadata(
                "animation sequence is empty or too large".into(),
            ));
        }
        let extent = [requested[0].rect.width, requested[0].rect.height];
        // Covers the Vec and bounded evidence labels; decoder/parser temporary
        // buffers are separately reserved. This is a logical-data allowance.
        let reservation = budget.reserve(
            (requested.len() * std::mem::size_of::<Frame>()) as u64 + 16384,
            cancel,
        )?;
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(requested.len())
            .map_err(|_| AssetError::Allocation)?;
        let mut end = 0_u64;
        let mut hasher = Sha256::new();
        hasher.update(b"ilium-overworld-animation-v1\0");
        hasher.update(image[0].to_le_bytes());
        hasher.update(image[1].to_le_bytes());
        hasher.update([
            u8::from(interpolate),
            u8::from(sampler.blur),
            u8::from(sampler.clamp),
        ]);
        for item in requested {
            cancel.check()?;
            item.rect.validate(image)?;
            if [item.rect.width, item.rect.height] != extent
                || item.ticks == 0
                || item.ticks > limits.frame_ticks
            {
                return Err(AssetError::InvalidMetadata(
                    "frame extents must match and durations must be positive bounded ticks".into(),
                ));
            }
            end = end
                .checked_add(u64::from(item.ticks))
                .ok_or_else(|| AssetError::InvalidMetadata("animation duration overflow".into()))?;
            for number in [
                item.rect.x,
                item.rect.y,
                item.rect.width,
                item.rect.height,
                item.ticks,
            ] {
                hasher.update(number.to_le_bytes());
            }
            frames.push(Frame {
                rect: item.rect,
                end_tick: end,
            });
        }
        // Evidence is a bounded struct. Include its exact normalized identity,
        // not merely pixel rectangles, in the cache/reload fingerprint.
        let evidence_bytes = serde_json::to_vec(&evidence)
            .map_err(|e| AssetError::InvalidMetadata(super::error::summary(&e.to_string())))?;
        hasher.update(evidence_bytes);
        let digest = hasher.finalize();
        let fingerprint = Digest256::of(&digest);
        let changing_rects = frames.iter().any(|frame| frame.rect != frames[0].rect);
        Ok(Self {
            image,
            frames,
            cycle_ticks: end,
            interpolate,
            sampler,
            evidence,
            changing_rects,
            fingerprint,
            _reservation: reservation,
        })
    }

    pub fn at(&self, time: Duration) -> FrameSample {
        // Nonempty frames and positive cycle are construction invariants.
        let phase = time.as_nanos() % (u128::from(self.cycle_ticks) * TICK_NANOS);
        let tick = (phase / TICK_NANOS) as u64;
        let index = self.frames.partition_point(|frame| frame.end_tick <= tick);
        let frame = self.frames[index];
        let next = self.frames[(index + 1) % self.frames.len()];
        let start = if index == 0 {
            0
        } else {
            self.frames[index - 1].end_tick
        };
        let blend = if self.interpolate && self.frames.len() > 1 {
            ((phase - u128::from(start) * TICK_NANOS) as f64
                / (u128::from(frame.end_tick - start) * TICK_NANOS) as f64) as f32
        } else {
            0.0
        };
        FrameSample {
            current: frame.rect,
            next: next.rect,
            blend,
        }
    }
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
    pub fn image_dimensions(&self) -> [u32; 2] {
        self.image
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
    pub fn changing_rects(&self) -> bool {
        self.changing_rects
    }
    /// Source metadata evidence, not an assertion that every timing field was
    /// authored: Java default flags are retained in AnimationEvidence.
    pub fn authored_schedule(&self) -> bool {
        matches!(&self.evidence, AnimationEvidence::JavaMetadata { origin, .. } | AnimationEvidence::BedrockMetadata { origin, .. }
            if origin.kind != OriginKind::DiagnosticFixture)
    }
    pub fn evidence(&self) -> &AnimationEvidence {
        &self.evidence
    }
    pub fn sampler(&self) -> Sampler {
        self.sampler
    }
    pub fn fingerprint(&self) -> Digest256 {
        self.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::super::{identity::OriginKind, review::fixture_origin};
    use super::*;
    use std::sync::atomic::AtomicBool;
    fn make(
        image: [u32; 2],
        json: Option<&str>,
        policy: MissingAnimation,
    ) -> Result<AnimationPlan> {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(16 * 1024 * 1024).unwrap();
        let limits = Limits::default();
        let metadata = json.map(|s| {
            SourceBlob::new(
                s.as_bytes().to_vec(),
                fixture_origin(OriginKind::SelectedPack),
                None,
                &limits,
                &budget,
                cancel,
            )
            .unwrap()
        });
        AnimationPlan::build(image, metadata.as_ref(), &policy, &limits, &budget, cancel)
    }
    #[test]
    fn no_sidecar_never_invents_a_strip_or_timing() {
        for dims in [[512, 16384], [1021, 16384], [128, 64]] {
            let plan = make(dims, None, MissingAnimation::StaticImage).unwrap();
            assert_eq!(plan.frame_count(), 1);
            assert_eq!(
                plan.at(Duration::ZERO).current,
                PixelRect::whole(dims[0], dims[1])
            );
            assert_eq!(plan.at(Duration::ZERO), plan.at(Duration::MAX));
            assert!(!plan.authored_schedule());
        }
    }
    #[test]
    fn explicit_goodvibes_crop_is_valid_and_labeled_not_authored() {
        let rect = PixelRect::whole(1021, 1021);
        let plan = make(
            [1021, 16384],
            None,
            MissingAnimation::StaticCrop {
                rect,
                reason: Label::new("static top-left crop; authored flow frames and timing unknown")
                    .unwrap(),
            },
        )
        .unwrap();
        assert_eq!(plan.at(Duration::ZERO).current, rect);
        assert!(matches!(
            plan.evidence(),
            AnimationEvidence::Compatibility {
                static_crop: true,
                ..
            }
        ));
        assert!(make(
            [1021, 16384],
            Some(r#"{"animation":{}}"#),
            MissingAnimation::StaticImage
        )
        .is_err());
    }
    #[test]
    fn weighted_repeated_frames_and_duration_boundaries_are_exact() {
        let plan = make(
            [2, 6],
            Some(r#"{"animation":{"frametime":2,"frames":[2,{"index":0,"time":3},2,1]}}"#),
            MissingAnimation::StaticImage,
        )
        .unwrap();
        for (ms, y) in [
            (0, 4),
            (99, 4),
            (100, 0),
            (249, 0),
            (250, 4),
            (349, 4),
            (350, 2),
            (449, 2),
            (450, 4),
        ] {
            assert_eq!(plan.at(Duration::from_millis(ms)).current.y, y, "{ms}");
        }
        assert!(plan.authored_schedule() && plan.changing_rects());
        assert_eq!(plan.frame_count(), 4);
    }
    #[test]
    fn frame_size_defaults_are_independent_of_block_resolution() {
        let width = make(
            [6, 4],
            Some(r#"{"animation":{"width":2}}"#),
            MissingAnimation::StaticImage,
        )
        .unwrap();
        assert_eq!(width.frame_count(), 3);
        assert_eq!(width.at(Duration::ZERO).current, PixelRect::whole(2, 4));
        let height = make(
            [6, 4],
            Some(r#"{"animation":{"height":2}}"#),
            MissingAnimation::StaticImage,
        )
        .unwrap();
        assert_eq!(height.frame_count(), 2);
        assert_eq!(height.at(Duration::ZERO).current, PixelRect::whole(6, 2));
        let grid = make(
            [6, 4],
            Some(r#"{"animation":{"width":2,"height":2,"frames":[4]}}"#),
            MissingAnimation::StaticImage,
        )
        .unwrap();
        assert_eq!(
            grid.at(Duration::ZERO).current,
            PixelRect {
                x: 2,
                y: 2,
                width: 2,
                height: 2
            }
        );
    }
    #[test]
    fn malformed_authored_metadata_does_not_activate_a_fallback() {
        for json in [
            r#"{"animation":null}"#,
            r#"{"animation":{"frames":null}}"#,
            r#"{"animation":{"frametime":null}}"#,
            r#"{"animation":{"frametime":0}}"#,
            r#"{"animation":{"frames":[-1]}}"#,
            r#"{"animation":{"frames":[8]}}"#,
            r#"{"animation":{"frames":[{"index":0,"time":0}]}}"#,
            r#"{"animation":{"width":0}}"#,
            r#"{"animation":{"unknown_execution":"run me"}}"#,
            r#"{"animation":{"frametime":1,"frametime":2}}"#,
            "{",
        ] {
            assert!(
                make([2, 4], Some(json), MissingAnimation::StaticImage).is_err(),
                "{json}"
            );
        }
    }
    #[test]
    fn interpolation_phase_and_empty_frame_array_follow_declared_policy() {
        let plan = make(
            [2, 4],
            Some(r#"{"animation":{"frametime":2,"interpolate":true,"frames":[]}}"#),
            MissingAnimation::StaticImage,
        )
        .unwrap();
        let sample = plan.at(Duration::from_millis(50));
        assert_eq!(sample.current.y, 0);
        assert_eq!(sample.next.y, 2);
        assert_eq!(sample.blend, 0.5);
        assert!(plan.at(Duration::MAX).blend.is_finite());
    }
    #[test]
    fn explicit_rectangles_allow_nondivisible_sheets_without_fabrication() {
        let policy = MissingAnimation::ExplicitFrames {
            frames: vec![
                ExplicitFrame {
                    rect: PixelRect::whole(3, 2),
                    ticks: 2,
                },
                ExplicitFrame {
                    rect: PixelRect {
                        x: 1,
                        y: 3,
                        width: 3,
                        height: 2,
                    },
                    ticks: 5,
                },
            ],
            interpolate: false,
            reason: Label::new("explicit compatibility experiment, not authored timing").unwrap(),
        };
        let plan = make([5, 7], None, policy).unwrap();
        assert_eq!(plan.at(Duration::from_millis(100)).current.y, 3);
        assert!(!plan.authored_schedule());
        assert!(make(
            [2, 2],
            None,
            MissingAnimation::StaticCrop {
                rect: PixelRect {
                    x: u32::MAX,
                    y: 0,
                    width: 2,
                    height: 2
                },
                reason: Label::new("negative control").unwrap(),
            }
        )
        .is_err());
    }
    #[test]
    fn typed_parser_rejects_huge_frame_arrays_during_read() {
        let entries = std::iter::repeat_n("0", HARD_FRAME_LIMIT + 1)
            .collect::<Vec<_>>()
            .join(",");
        let json = format!("{{\"animation\":{{\"frames\":[{entries}]}}}}");
        assert!(make([2, 2], Some(&json), MissingAnimation::StaticImage).is_err());
    }
    #[test]
    fn sampler_metadata_without_animation_does_not_create_frames() {
        let plan = make(
            [8, 24],
            Some(r#"{"texture":{"blur":true,"clamp":true}}"#),
            MissingAnimation::StaticImage,
        )
        .unwrap();
        assert_eq!(
            plan.sampler(),
            Sampler {
                blur: true,
                clamp: true
            }
        );
        assert_eq!(plan.frame_count(), 1);
    }
    #[test]
    fn modern_cutout_mipmap_metadata_keeps_the_original_static_image() {
        for strategy in ["mean", "dark_cutout", "strict_cutout"] {
            let metadata = format!(
                "{{\"texture\":{{\"mipmap_strategy\":\"{strategy}\",\"alpha_cutoff_bias\":0.1}}}}"
            );
            let plan = make([32, 32], Some(&metadata), MissingAnimation::StaticImage).unwrap();
            assert_eq!(plan.frame_count(), 1);
            assert_eq!(plan.at(Duration::ZERO).current, PixelRect::whole(32, 32));
            assert_eq!(plan.sampler(), Sampler::default());
            assert!(matches!(
                plan.evidence(),
                AnimationEvidence::MetadataWithoutAnimation { .. }
            ));
        }
        assert!(make(
            [32, 32],
            Some(r#"{"texture":{"mipmap_strategy":"invented"}}"#),
            MissingAnimation::StaticImage
        )
        .is_err());
        assert!(make(
            [32, 32],
            Some(r#"{"texture":{"mipmap_strategy":7}}"#),
            MissingAnimation::StaticImage
        )
        .is_err());
    }
}
