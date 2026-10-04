use super::animation::{AnimationPlan, PixelRect};
use super::budget::{ByteBudget, Cancel, Reservation};
use super::error::{AssetError, Result};
use super::identity::Digest256;
use super::pixels::PixelImage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// Color channels are sRGB; opacity is linear coverage.
    SrgbColor,
    /// All four channels are data. Alpha may be height/emission, NOT opacity.
    LinearData,
}

/// Linear-light, premultiplied RGBA. Private fields keep malformed/nonfinite
/// values out of the compositing operations. Opaque black is not uncovered air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearRgba {
    rgb: [f32; 3],
    alpha: f32,
}
impl LinearRgba {
    pub const CLEAR: Self = Self {
        rgb: [0.0; 3],
        alpha: 0.0,
    };
    pub fn from_straight(rgb: [f32; 3], alpha: f32) -> Option<Self> {
        if !alpha.is_finite() || rgb.iter().any(|c| !c.is_finite()) {
            return None;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        Some(Self {
            rgb: rgb.map(|c| c.clamp(0.0, 1.0) * alpha),
            alpha,
        })
    }
    pub fn premultiplied(self) -> [f32; 4] {
        [self.rgb[0], self.rgb[1], self.rgb[2], self.alpha]
    }
    pub fn alpha(self) -> f32 {
        self.alpha
    }
    pub fn straight(self) -> [f32; 3] {
        if self.alpha > 0.0 {
            self.rgb.map(|c| (c / self.alpha).clamp(0.0, 1.0))
        } else {
            [0.0; 3]
        }
    }
    /// `self` is the foreground. No byte/gamma-space blending or division by 0.
    pub fn over(self, background: Self) -> Self {
        let remaining = 1.0 - self.alpha;
        Self {
            rgb: std::array::from_fn(|i| self.rgb[i] + background.rgb[i] * remaining),
            alpha: self.alpha + background.alpha * remaining,
        }
    }
    /// A multiplicative linear-light tint; opacity is unchanged. The model
    /// resolver decides which faces have tintindex, not the PNG filename.
    pub fn tint(self, linear_tint: [f32; 3]) -> Option<Self> {
        if linear_tint.iter().any(|c| !c.is_finite()) {
            return None;
        }
        Some(Self {
            rgb: std::array::from_fn(|i| self.rgb[i] * linear_tint[i].clamp(0.0, 1.0)),
            ..self
        })
    }
}

// Precomputed IEC sRGB transfer at all byte values, rounded to f32.
// A const table avoids even first-use OnceLock contention on the render path.
const SRGB_TO_LINEAR: [f32; 256] = [
    0.0f32,
    0.000303527f32,
    0.000607054f32,
    0.000910581f32,
    0.001214108f32,
    0.001517635f32,
    0.001821162f32,
    0.0021246888f32,
    0.002428216f32,
    0.0027317428f32,
    0.00303527f32,
    0.0033465358f32,
    0.0036765074f32,
    0.004024717f32,
    0.004391442f32,
    0.0047769533f32,
    0.0051815165f32,
    0.0056053917f32,
    0.006048833f32,
    0.0065120906f32,
    0.00699541f32,
    0.007499032f32,
    0.008023193f32,
    0.008568126f32,
    0.009134059f32,
    0.009721218f32,
    0.010329823f32,
    0.010960094f32,
    0.011612245f32,
    0.012286488f32,
    0.0129830325f32,
    0.013702083f32,
    0.014443844f32,
    0.015208514f32,
    0.015996294f32,
    0.016807375f32,
    0.017641954f32,
    0.01850022f32,
    0.019382361f32,
    0.020288562f32,
    0.02121901f32,
    0.022173885f32,
    0.023153367f32,
    0.024157632f32,
    0.02518686f32,
    0.026241222f32,
    0.027320892f32,
    0.02842604f32,
    0.029556835f32,
    0.030713445f32,
    0.031896032f32,
    0.033104766f32,
    0.034339808f32,
    0.035601314f32,
    0.03688945f32,
    0.038204372f32,
    0.039546236f32,
    0.0409152f32,
    0.04231141f32,
    0.04373503f32,
    0.045186203f32,
    0.046665087f32,
    0.048171826f32,
    0.049706567f32,
    0.051269457f32,
    0.052860647f32,
    0.054480277f32,
    0.05612849f32,
    0.05780543f32,
    0.059511237f32,
    0.061246052f32,
    0.063010015f32,
    0.064803265f32,
    0.06662594f32,
    0.06847817f32,
    0.070360094f32,
    0.07227185f32,
    0.07421357f32,
    0.07618538f32,
    0.07818742f32,
    0.08021982f32,
    0.08228271f32,
    0.08437621f32,
    0.08650046f32,
    0.08865558f32,
    0.09084171f32,
    0.093058966f32,
    0.09530747f32,
    0.09758735f32,
    0.099898726f32,
    0.10224173f32,
    0.104616486f32,
    0.107023105f32,
    0.10946171f32,
    0.11193243f32,
    0.114435375f32,
    0.116970666f32,
    0.11953843f32,
    0.122138776f32,
    0.12477182f32,
    0.12743768f32,
    0.13013647f32,
    0.13286832f32,
    0.13563333f32,
    0.13843161f32,
    0.14126329f32,
    0.14412847f32,
    0.14702727f32,
    0.14995979f32,
    0.15292615f32,
    0.15592647f32,
    0.15896083f32,
    0.16202937f32,
    0.1651322f32,
    0.1682694f32,
    0.17144111f32,
    0.1746474f32,
    0.17788842f32,
    0.18116425f32,
    0.18447499f32,
    0.18782078f32,
    0.19120169f32,
    0.19461784f32,
    0.19806932f32,
    0.20155625f32,
    0.20507874f32,
    0.20863687f32,
    0.21223076f32,
    0.2158605f32,
    0.2195262f32,
    0.22322796f32,
    0.22696587f32,
    0.23074006f32,
    0.23455058f32,
    0.23839757f32,
    0.24228112f32,
    0.24620132f32,
    0.25015828f32,
    0.2541521f32,
    0.25818285f32,
    0.26225066f32,
    0.2663556f32,
    0.2704978f32,
    0.2746773f32,
    0.27889428f32,
    0.28314874f32,
    0.28744084f32,
    0.29177064f32,
    0.29613826f32,
    0.30054379f32,
    0.3049873f32,
    0.30946892f32,
    0.31398872f32,
    0.31854677f32,
    0.3231432f32,
    0.3277781f32,
    0.33245152f32,
    0.33716363f32,
    0.34191442f32,
    0.34670407f32,
    0.3515326f32,
    0.35640013f32,
    0.3613068f32,
    0.3662526f32,
    0.3712377f32,
    0.37626213f32,
    0.38132602f32,
    0.38642943f32,
    0.39157248f32,
    0.39675522f32,
    0.40197778f32,
    0.4072402f32,
    0.4125426f32,
    0.41788507f32,
    0.42326766f32,
    0.4286905f32,
    0.43415365f32,
    0.43965718f32,
    0.4452012f32,
    0.4507858f32,
    0.45641103f32,
    0.462077f32,
    0.4677838f32,
    0.47353148f32,
    0.47932017f32,
    0.48514995f32,
    0.49102086f32,
    0.49693298f32,
    0.5028865f32,
    0.50888133f32,
    0.5149177f32,
    0.52099556f32,
    0.5271151f32,
    0.5332764f32,
    0.5394795f32,
    0.54572445f32,
    0.55201143f32,
    0.5583404f32,
    0.5647115f32,
    0.57112485f32,
    0.57758045f32,
    0.58407843f32,
    0.59061885f32,
    0.59720176f32,
    0.60382736f32,
    0.61049557f32,
    0.6172066f32,
    0.6239604f32,
    0.63075715f32,
    0.63759685f32,
    0.6444797f32,
    0.65140563f32,
    0.65837485f32,
    0.6653873f32,
    0.67244315f32,
    0.6795425f32,
    0.6866853f32,
    0.69387174f32,
    0.7011019f32,
    0.70837575f32,
    0.7156935f32,
    0.7230551f32,
    0.73046076f32,
    0.7379104f32,
    0.7454042f32,
    0.7529422f32,
    0.7605245f32,
    0.76815116f32,
    0.7758222f32,
    0.7835378f32,
    0.7912979f32,
    0.7991027f32,
    0.80695224f32,
    0.8148466f32,
    0.82278574f32,
    0.8307699f32,
    0.838799f32,
    0.8468732f32,
    0.8549926f32,
    0.8631572f32,
    0.8713671f32,
    0.8796224f32,
    0.8879231f32,
    0.8962694f32,
    0.9046612f32,
    0.91309863f32,
    0.92158186f32,
    0.9301109f32,
    0.9386857f32,
    0.9473065f32,
    0.9559733f32,
    0.9646863f32,
    0.9734453f32,
    0.9822506f32,
    0.9911021f32,
    1.0f32,
];
pub fn srgb_byte_to_linear(value: u8) -> f32 {
    SRGB_TO_LINEAR[usize::from(value)]
}

#[derive(Debug)]
pub struct Texture {
    image: Arc<PixelImage>,
    animation: AnimationPlan,
    encoding: Encoding,
    fingerprint: Digest256,
    _reservation: Reservation,
}
impl Texture {
    pub fn new(
        image: Arc<PixelImage>,
        animation: AnimationPlan,
        encoding: Encoding,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if image.dimensions() != animation.image_dimensions() {
            return Err(AssetError::InvalidMetadata(
                "timeline and image dimensions differ".into(),
            ));
        }
        if !image.uses_budget(budget) || !animation.uses_budget(budget) {
            return Err(AssetError::InvalidMetadata(
                "image and timeline must share the texture-bank byte account".into(),
            ));
        }
        let reservation = budget.reserve(512, cancel)?;
        let mut hash = Sha256::new();
        hash.update(b"ilium-overworld-texture-v1\0");
        hash.update(image.source_sha256().bytes());
        hash.update(image.rgba_sha256().bytes());
        hash.update(animation.fingerprint().bytes());
        hash.update([match encoding {
            Encoding::SrgbColor => 0,
            Encoding::LinearData => 1,
        }]);
        let fingerprint = Digest256::of(&hash.finalize());
        Ok(Self {
            image,
            animation,
            encoding,
            fingerprint,
            _reservation: reservation,
        })
    }
    pub fn image(&self) -> &PixelImage {
        &self.image
    }
    pub fn animation(&self) -> &AnimationPlan {
        &self.animation
    }
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }
    pub fn fingerprint(&self) -> Digest256 {
        self.fingerprint
    }
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }

    /// Normalized coordinates refer to ONE active frame/crop, not the entire
    /// animation sheet. These methods allocate nothing and never read a clock.
    pub fn sample_color(&self, uv: [f32; 2], time: Duration) -> Option<LinearRgba> {
        if self.encoding != Encoding::SrgbColor {
            return None;
        }
        let sample = self.sample(uv, time)?;
        Some(LinearRgba {
            rgb: [sample[0], sample[1], sample[2]],
            alpha: sample[3],
        })
    }
    /// Native shader input keeps RGB independent of alpha until the exact
    /// render-layer discard/blend decision. The generated sampler continues to
    /// use premultiplied interpolation through sample_color.
    pub(crate) fn sample_native_color(&self, uv: [f32; 2], time: Duration) -> Option<[f32; 4]> {
        (self.encoding == Encoding::SrgbColor)
            .then(|| self.sample_with_alpha(uv, time, false))
            .flatten()
    }
    /// Preserve material-channel values independently; do not gamma-decode or
    /// premultiply a normal/specular/height map by its fourth data channel.
    pub fn sample_data(&self, uv: [f32; 2], time: Duration) -> Option<[f32; 4]> {
        if self.encoding != Encoding::LinearData {
            return None;
        }
        self.sample(uv, time)
    }
    /// Categorical LabPBR channels use a single texel and a single active frame.
    /// In particular a metal ID cannot be created by spatial or temporal interpolation.
    pub fn sample_data_nearest(&self, uv: [f32; 2], time: Duration) -> Option<[u8; 4]> {
        if self.encoding != Encoding::LinearData || uv.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let rect = self.animation.at(time).current;
        let sampler = self.animation.sampler();
        let address = |value: f32, extent: u32| -> u32 {
            let normalized = if sampler.clamp {
                f64::from(value).clamp(0.0, 1.0)
            } else {
                f64::from(value).rem_euclid(1.0)
            };
            let pixel = (normalized * f64::from(extent)).floor() as i64;
            if sampler.clamp {
                pixel.clamp(0, i64::from(extent) - 1) as u32
            } else {
                pixel.rem_euclid(i64::from(extent)) as u32
            }
        };
        self.image.pixel(
            rect.x + address(uv[0], rect.width),
            rect.y + address(uv[1], rect.height),
        )
    }
    fn sample(&self, uv: [f32; 2], time: Duration) -> Option<[f32; 4]> {
        self.sample_with_alpha(uv, time, true)
    }
    fn sample_with_alpha(
        &self,
        uv: [f32; 2],
        time: Duration,
        premultiply_color: bool,
    ) -> Option<[f32; 4]> {
        if uv.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let phase = self.animation.at(time);
        let a = self.frame(phase.current, uv, premultiply_color)?;
        if phase.blend == 0.0 || phase.current == phase.next {
            return Some(a);
        }
        let b = self.frame(phase.next, uv, premultiply_color)?;
        Some(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * phase.blend))
    }
    fn frame(&self, rect: PixelRect, uv: [f32; 2], premultiply_color: bool) -> Option<[f32; 4]> {
        let sampler = self.animation.sampler();
        let normalized = uv.map(|value| {
            let value = f64::from(value);
            if sampler.clamp {
                value.clamp(0.0, 1.0)
            } else {
                value.rem_euclid(1.0)
            }
        });
        let at = |x: i64, y: i64| {
            let wrap = |i: i64, extent: u32| -> u32 {
                let extent = i64::from(extent);
                if sampler.clamp {
                    i.clamp(0, extent - 1) as u32
                } else {
                    i.rem_euclid(extent) as u32
                }
            };
            let pixel = self
                .image
                .pixel(rect.x + wrap(x, rect.width), rect.y + wrap(y, rect.height))?;
            Some(match self.encoding {
                Encoding::SrgbColor => {
                    let alpha = f32::from(pixel[3]) / 255.0;
                    let multiplier = if premultiply_color { alpha } else { 1.0 };
                    [
                        srgb_byte_to_linear(pixel[0]) * multiplier,
                        srgb_byte_to_linear(pixel[1]) * multiplier,
                        srgb_byte_to_linear(pixel[2]) * multiplier,
                        alpha,
                    ]
                }
                Encoding::LinearData => pixel.map(|c| f32::from(c) / 255.0),
            })
        };
        if !sampler.blur {
            let x = (normalized[0] * f64::from(rect.width)).floor() as i64;
            let y = (normalized[1] * f64::from(rect.height)).floor() as i64;
            return at(x, y);
        }
        // Half-texel centers; every neighbor is wrapped/clamped inside `rect`,
        // so bilinear filtering cannot leak a neighboring animation frame.
        let x = normalized[0] * f64::from(rect.width) - 0.5;
        let y = normalized[1] * f64::from(rect.height) - 0.5;
        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
        let (tx, ty) = ((x - x.floor()) as f32, (y - y.floor()) as f32);
        let (a, b, c, d) = (
            at(ix, iy)?,
            at(ix + 1, iy)?,
            at(ix, iy + 1)?,
            at(ix + 1, iy + 1)?,
        );
        Some(std::array::from_fn(|i| {
            let upper = a[i] + (b[i] - a[i]) * tx;
            let lower = c[i] + (d[i] - c[i]) * tx;
            upper + (lower - upper) * ty
        }))
    }
}

#[cfg(test)]
pub(crate) fn fixture_texture(
    dims: [u32; 2],
    pixels: &[u8],
    metadata: Option<&str>,
    fallback: &super::animation::MissingAnimation,
    encoding: Encoding,
    origin: super::identity::BlobOrigin,
    budget: &ByteBudget,
) -> Arc<Texture> {
    use super::{
        budget::Limits,
        identity::SourceBlob,
        pixels::{fixture_png, ImageExpectations},
    };
    use std::sync::atomic::AtomicBool;
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let limits = Limits::default();
    let blob = SourceBlob::new(
        fixture_png(dims[0], dims[1], pixels),
        origin.clone(),
        None,
        &limits,
        budget,
        cancel,
    )
    .unwrap();
    let image = Arc::new(
        PixelImage::decode_png(&blob, ImageExpectations::default(), &limits, budget, cancel)
            .unwrap(),
    );
    let metadata = metadata.map(|text| {
        SourceBlob::new(
            text.as_bytes().to_vec(),
            origin,
            None,
            &limits,
            budget,
            cancel,
        )
        .unwrap()
    });
    let plan =
        AnimationPlan::build(dims, metadata.as_ref(), fallback, &limits, budget, cancel).unwrap();
    Arc::new(Texture::new(image, plan, encoding, budget, cancel).unwrap())
}

#[cfg(test)]
mod tests {
    use super::super::{
        animation::MissingAnimation,
        identity::{Label, OriginKind},
        review::fixture_origin,
    };
    use super::*;
    fn budget() -> ByteBudget {
        ByteBudget::new(512 * 1024 * 1024).unwrap()
    }
    fn texture(dims: [u32; 2], data: &[u8], meta: Option<&str>) -> Arc<Texture> {
        fixture_texture(
            dims,
            data,
            meta,
            &MissingAnimation::StaticImage,
            Encoding::SrgbColor,
            fixture_origin(OriginKind::DiagnosticFixture),
            &budget(),
        )
    }
    #[test]
    fn opaque_black_and_transparent_black_have_different_occlusion() {
        let t = texture([2, 1], &[0, 0, 0, 255, 0, 0, 0, 0], None);
        let black = t.sample_color([0.1, 0.5], Duration::ZERO).unwrap();
        let air = t.sample_color([0.8, 0.5], Duration::ZERO).unwrap();
        let ground = LinearRgba::from_straight([0.2, 0.5, 0.1], 1.0).unwrap();
        assert_eq!(black.alpha(), 1.0);
        assert_eq!(air.alpha(), 0.0);
        assert_eq!(black.over(ground), black);
        assert_eq!(air.over(ground), ground);
    }
    #[test]
    fn nearest_repeat_and_clamp_have_explicit_edge_semantics() {
        let data = [255, 0, 0, 255, 0, 0, 255, 255];
        let repeat = texture([2, 1], &data, None);
        let clamp = texture([2, 1], &data, Some(r#"{"texture":{"clamp":true}}"#));
        assert_eq!(
            repeat
                .sample_color([1.0, 1.0], Duration::ZERO)
                .unwrap()
                .straight(),
            [1.0, 0.0, 0.0]
        );
        assert_eq!(
            clamp
                .sample_color([1.0, 1.0], Duration::ZERO)
                .unwrap()
                .straight(),
            [0.0, 0.0, 1.0]
        );
        assert_eq!(
            repeat
                .sample_color([-0.25, 0.0], Duration::ZERO)
                .unwrap()
                .straight(),
            [0.0, 0.0, 1.0]
        );
        assert!(clamp
            .sample_color([f32::NAN, 0.0], Duration::ZERO)
            .is_none());
    }
    #[test]
    fn premultiplied_filtering_avoids_transparent_color_fringes() {
        let t = texture(
            [2, 1],
            &[255, 0, 0, 255, 0, 0, 255, 0],
            Some(r#"{"texture":{"blur":true,"clamp":true}}"#),
        );
        let sample = t.sample_color([0.5, 0.5], Duration::ZERO).unwrap();
        assert_eq!(sample.alpha(), 0.5);
        assert_eq!(sample.straight(), [1.0, 0.0, 0.0]);
        let native = t.sample_native_color([0.5, 0.5], Duration::ZERO).unwrap();
        assert_eq!(native, [0.5, 0.0, 0.5, 0.5]);
        let invisible_blue = t.sample_native_color([0.8, 0.5], Duration::ZERO).unwrap();
        assert_eq!(invisible_blue, [0.0, 0.0, 1.0, 0.0]);
    }
    #[test]
    fn bilinear_sampling_never_leaks_into_adjacent_animation_frame() {
        let data: Vec<_> = [[255, 0, 0, 255]; 4]
            .into_iter()
            .chain([[0, 0, 255, 255]; 4])
            .flatten()
            .collect();
        let t = texture(
            [2, 4],
            &data,
            Some(r#"{"texture":{"blur":true,"clamp":true},"animation":{}}"#),
        );
        for uv in [[0.0, 0.0], [0.5, 0.9999], [1.0, 1.0]] {
            assert_eq!(
                t.sample_color(uv, Duration::ZERO).unwrap().straight(),
                [1.0, 0.0, 0.0]
            );
            assert_eq!(
                t.sample_color(uv, Duration::from_millis(50))
                    .unwrap()
                    .straight(),
                [0.0, 0.0, 1.0]
            );
        }
    }
    #[test]
    fn explicit_crop_and_non_square_entity_atlas_retain_measured_coordinates() {
        let dims = [128, 64];
        let mut data = vec![0; 128 * 64 * 4];
        for y in 20..24 {
            for x in 100..108 {
                data[(y * 128 + x) * 4..(y * 128 + x) * 4 + 4].copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        let policy = MissingAnimation::StaticCrop {
            rect: PixelRect {
                x: 100,
                y: 20,
                width: 8,
                height: 4,
            },
            reason: Label::new("synthetic entity part crop, not an authored animation").unwrap(),
        };
        let t = fixture_texture(
            dims,
            &data,
            None,
            &policy,
            Encoding::SrgbColor,
            fixture_origin(OriginKind::DiagnosticFixture),
            &budget(),
        );
        assert_eq!(t.image().dimensions(), dims);
        assert_eq!(
            t.sample_color([0.99, 0.99], Duration::MAX)
                .unwrap()
                .straight(),
            [0.0, 1.0, 0.0]
        );
        assert!(!t.animation().authored_schedule());
    }
    #[test]
    fn temporal_interpolation_is_linear_premultiplied_and_camera_independent() {
        let t = texture(
            [1, 2],
            &[255, 0, 0, 255, 0, 0, 255, 0],
            Some(r#"{"animation":{"frametime":2,"interpolate":true}}"#),
        );
        let half = t.sample_color([0.5; 2], Duration::from_millis(50)).unwrap();
        assert_eq!(half.alpha(), 0.5);
        assert_eq!(half.straight(), [1.0, 0.0, 0.0]);
        assert_eq!(
            half,
            t.sample_color([0.5; 2], Duration::from_millis(250))
                .unwrap()
        );
    }
    #[test]
    fn data_channels_are_not_gamma_decoded_or_alpha_multiplied() {
        let t = fixture_texture(
            [1, 1],
            &[128, 128, 64, 0],
            None,
            &MissingAnimation::StaticImage,
            Encoding::LinearData,
            fixture_origin(OriginKind::DiagnosticFixture),
            &budget(),
        );
        assert_eq!(
            t.sample_data([0.5; 2], Duration::ZERO).unwrap(),
            [128.0 / 255.0, 128.0 / 255.0, 64.0 / 255.0, 0.0]
        );
        assert!(t.sample_color([0.5; 2], Duration::ZERO).is_none());
        assert!((srgb_byte_to_linear(128) - 0.2158605).abs() < 0.00001);
    }
    #[test]
    fn constant_color_table_is_bounded_monotonic_and_has_exact_endpoints() {
        assert_eq!(srgb_byte_to_linear(0), 0.0);
        assert_eq!(srgb_byte_to_linear(255), 1.0);
        for value in 0..255_u8 {
            assert!(srgb_byte_to_linear(value) < srgb_byte_to_linear(value + 1));
        }
    }
    #[test]
    fn tint_preserves_alpha_and_rejects_nonfinite_input() {
        let a = LinearRgba::from_straight([0.8, 0.5, 0.25], 0.5).unwrap();
        let tinted = a.tint([0.5, 1.0, 0.0]).unwrap();
        assert_eq!(tinted.alpha(), a.alpha());
        assert_eq!(tinted.straight(), [0.4, 0.5, 0.0]);
        assert!(a.tint([f32::INFINITY, 0.0, 0.0]).is_none());
    }
}
