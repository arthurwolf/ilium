//! Published LabPBR channel interpretation, not a claim of completed LabPBR lighting.
//! Categorical channels must be sampled as labels, not bilinearly mixed metal IDs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FresnelSource {
    Dielectric(f32),
    MetalId(u8),
    AlbedoBased,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlueMaterial {
    Porosity(f32),
    Subsurface(f32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabNormal {
    pub tangent: [f32; 3],
    pub ambient_occlusion: f32,
    pub height: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabSpecular {
    pub roughness: f32,
    pub fresnel: FresnelSource,
    pub blue: BlueMaterial,
    pub emission: Option<f32>,
}
impl LabNormal {
    pub fn decode([red, green, blue, alpha]: [u8; 4]) -> Self {
        let x = 2.0 * f32::from(red) / 255.0 - 1.0;
        let y = 2.0 * f32::from(green) / 255.0 - 1.0;
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        Self {
            tangent: [x, y, z],
            ambient_occlusion: f32::from(blue) / 255.0,
            height: f32::from(alpha) / 255.0,
        }
    }
}
impl LabSpecular {
    pub fn decode([red, green, blue, alpha]: [u8; 4]) -> Self {
        let roughness = (1.0 - f32::from(red) / 255.0).powi(2);
        let fresnel = match green {
            0..=229 => FresnelSource::Dielectric(f32::from(green) / 255.0),
            230..=254 => FresnelSource::MetalId(green),
            255 => FresnelSource::AlbedoBased,
        };
        let blue = match blue {
            0..=64 => BlueMaterial::Porosity(f32::from(blue) / 64.0),
            65..=255 => BlueMaterial::Subsurface(f32::from(blue - 65) / 190.0),
        };
        Self {
            roughness,
            fresnel,
            blue,
            emission: (alpha != 255).then_some(f32::from(alpha) / 254.0),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_alpha_is_height_or_emission_not_diffuse_transparency() {
        let normal = LabNormal::decode([128, 128, 255, 0]);
        assert!(normal.tangent[2] > 0.999);
        assert_eq!(normal.height, 0.0);
        assert_eq!(normal.ambient_occlusion, 1.0);
        assert_eq!(LabSpecular::decode([255, 255, 255, 255]).emission, None);
        assert_eq!(
            LabSpecular::decode([255, 255, 255, 254]).emission,
            Some(1.0)
        );
        assert_eq!(LabSpecular::decode([0, 0, 0, 0]).emission, Some(0.0));
    }
    #[test]
    fn categorical_boundaries_and_perceptual_roughness_are_not_generic_rgba() {
        let a = LabSpecular::decode([128, 229, 64, 1]);
        assert_eq!(a.fresnel, FresnelSource::Dielectric(229.0 / 255.0));
        assert_eq!(a.blue, BlueMaterial::Porosity(1.0));
        assert!((a.roughness - (127.0_f32 / 255.0).powi(2)).abs() < 1e-7);
        let b = LabSpecular::decode([0, 230, 65, 255]);
        assert_eq!(b.fresnel, FresnelSource::MetalId(230));
        assert_eq!(b.blue, BlueMaterial::Subsurface(0.0));
        assert_eq!(
            LabSpecular::decode([0, 254, 255, 255]).fresnel,
            FresnelSource::MetalId(254)
        );
        assert_eq!(
            LabSpecular::decode([0, 255, 255, 255]).fresnel,
            FresnelSource::AlbedoBased
        );
    }
}
