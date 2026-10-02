//! Native Java grass/foliage colormap sampling, without synthetic climate.
//!
//! The caller supplies decoded source pixels and verified native biome float
//! climate. This scalar kernel is not a block colour provider, biome resolver,
//! neighbour blend, resource-pack admission or provenance certificate.
const PIXELS: usize = 256 * 256;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("native Minecraft colormap must contain exactly256x256 RGB pixels")]
    Dimensions,
    #[error("native Minecraft climate must be finite")]
    Climate,
}

/// Borrow the complete row-major source image; no pixel copy or allocation.
pub struct Colormap<'a> {
    pixels: &'a [[u8; 3]],
}
impl<'a> Colormap<'a> {
    pub fn new(pixels: &'a [[u8; 3]]) -> Result<Self, Error> {
        if pixels.len() != PIXELS {
            return Err(Error::Dimensions);
        }
        Ok(Self { pixels })
    }

    /// Biome's native floats are clamped before GrassColor/FoliageColor's
    /// double multiplication. The row is humidity times temperature, and
    /// column is temperature. Image origin remains the native top-left.
    pub fn sample(&self, temperature: f32, downfall: f32) -> Result<[u8; 3], Error> {
        if !temperature.is_finite() || !downfall.is_finite() {
            return Err(Error::Climate);
        }
        let temperature = f64::from(temperature.clamp(0.0, 1.0));
        let humidity = f64::from(downfall.clamp(0.0, 1.0)) * temperature;
        let column = ((1.0 - temperature) * 255.0) as usize;
        let row = ((1.0 - humidity) * 255.0) as usize;
        // Finite, clamped inputs keep both indexes in0..=255, exactly as
        // the native integer truncation; dimensions were checked at admission.
        Ok(self.pixels[(row << 8) | column])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_pixels() -> Vec<[u8; 3]> {
        (0..PIXELS)
            .map(|index| [index as u8, (index >> 8) as u8, 37])
            .collect()
    }

    #[test]
    fn native_axes_multiply_humidity_and_preserve_image_orientation() {
        let pixels = synthetic_pixels();
        let map = Colormap::new(&pixels).unwrap();
        for (temperature, downfall, expected) in [
            (0.0, 0.0, [255, 255, 37]),
            (1.0, 1.0, [0, 0, 37]),
            (0.5, 0.0, [127, 255, 37]),
            (0.5, 0.5, [127, 191, 37]),
            (1.0, 0.0, [0, 255, 37]),
        ] {
            assert_eq!(map.sample(temperature, downfall).unwrap(), expected);
        }
        assert!(std::ptr::eq(map.pixels.as_ptr(), pixels.as_ptr()));
    }

    #[test]
    fn biome_float_clamping_precedes_native_double_lookup() {
        let pixels = synthetic_pixels();
        let map = Colormap::new(&pixels).unwrap();
        assert_eq!(map.sample(2.0, 1.0).unwrap(), [0, 0, 37]);
        assert_eq!(map.sample(-0.7, 0.4).unwrap(), [255, 255, 37]);
        assert_eq!(map.sample(1.0, 2.0).unwrap(), [0, 0, 37]);
        assert_eq!(map.sample(1.0, -1.0).unwrap(), [0, 255, 37]);
    }

    #[test]
    fn malformed_pixels_and_nonfinite_climate_never_invent_colour() {
        for length in [0, 65535, 65537] {
            assert!(matches!(
                Colormap::new(&vec![[0; 3]; length]),
                Err(Error::Dimensions)
            ));
        }
        let pixels = synthetic_pixels();
        let map = Colormap::new(&pixels).unwrap();
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(map.sample(value, 0.5), Err(Error::Climate));
            assert_eq!(map.sample(0.5, value), Err(Error::Climate));
        }
    }
}
