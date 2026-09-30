//! The embedded bright-star catalogue: the Yale Bright Star Catalogue, 5th
//! revised edition (Hoffleit and Warren 1991, VizieR V/50), every star with
//! V <= 6.5 and a position: 8404 stars in `assets/bright_stars.bin`, brightest
//! first. Regenerate with `tools/build_star_catalog.py`; the binary layout is
//! documented there. Constellation stick figures live in
//! `assets/constellation_lines.bin` (pairs of star indices).

use super::astro::{equatorial_vector, Vec3};
use std::sync::OnceLock;

static STAR_BYTES: &[u8] = include_bytes!("../../../assets/bright_stars.bin");
static LINE_BYTES: &[u8] = include_bytes!("../../../assets/constellation_lines.bin");

#[derive(Debug, Clone, Copy)]
pub struct Star {
    /// J2000 equatorial unit vector.
    pub vector: Vec3,
    pub magnitude: f32,
    /// B-V colour index.
    pub color_index: f32,
}

pub struct Catalog {
    pub stars: Vec<Star>,
    pub lines: Vec<(u16, u16)>,
}

/// The decoded catalogue, shared by every scene instance.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog {
        stars: decode_stars(STAR_BYTES).unwrap_or_default(),
        lines: decode_lines(LINE_BYTES).unwrap_or_default(),
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

pub fn decode_stars(bytes: &[u8]) -> Option<Vec<Star>> {
    if bytes.get(..4)? != b"BSC1" {
        return None;
    }
    let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let mut stars = Vec::with_capacity(count);
    for index in 0..count {
        let base = 8 + index * 6;
        let record = bytes.get(base..base + 6)?;
        let ra = f64::from(read_u16(record, 0)?) / 65_536.0 * 360.0;
        let dec = f64::from(read_u16(record, 2)?) / 65_535.0 * 180.0 - 90.0;
        let magnitude = f32::from(record[4]) / 20.0 - 2.0;
        let color_index = if record[5] == 255 {
            0.6
        } else {
            f32::from(record[5]) / 64.0 - 0.6
        };
        stars.push(Star {
            vector: equatorial_vector(ra, dec),
            magnitude,
            color_index,
        });
    }
    Some(stars)
}

pub fn decode_lines(bytes: &[u8]) -> Option<Vec<(u16, u16)>> {
    if bytes.get(..4)? != b"CLN1" {
        return None;
    }
    let count = usize::from(read_u16(bytes, 4)?);
    (0..count)
        .map(|index| {
            let base = 6 + index * 4;
            Some((read_u16(bytes, base)?, read_u16(bytes, base + 2)?))
        })
        .collect()
}

/// Approximate display colour of a star from its B-V index: blackbody
/// temperature (Ballesteros 2012) mapped to sRGB (Tanner Helland fit), then
/// pulled towards white so that dim terminal dots stay readable.
pub fn color_of_index(color_index: f32) -> [u8; 3] {
    let bv = f64::from(color_index).clamp(-0.4, 2.0);
    let temperature = 4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62));
    let t = temperature.clamp(1000.0, 40_000.0) / 100.0;
    let red = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };
    let green = if t <= 66.0 {
        99.470_802_586_1 * t.ln() - 161.119_568_166_1
    } else {
        288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
    };
    let blue = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
    };
    let mix = |channel: f64| -> u8 {
        let saturated = channel.clamp(0.0, 255.0);
        (saturated * 0.72 + 255.0 * 0.28).round().clamp(0.0, 255.0) as u8
    };
    [mix(red), mix(green), mix(blue)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::stars::astro::{dot, vector_to_equatorial};

    fn find(name_ra: f64, name_dec: f64) -> &'static Star {
        let target = equatorial_vector(name_ra, name_dec);
        catalog()
            .stars
            .iter()
            .max_by(|a, b| dot(&a.vector, &target).total_cmp(&dot(&b.vector, &target)))
            .unwrap()
    }

    #[test]
    fn catalogue_is_complete_sorted_and_small() {
        let catalog = catalog();
        assert!(
            (8300..=9200).contains(&catalog.stars.len()),
            "{}",
            catalog.stars.len()
        );
        assert!(catalog
            .stars
            .windows(2)
            .all(|pair| pair[0].magnitude <= pair[1].magnitude));
        assert!(catalog.stars.last().unwrap().magnitude <= 6.55);
        assert!(STAR_BYTES.len() + LINE_BYTES.len() < 100_000);
        assert!(catalog.lines.len() > 100);
        assert!(catalog
            .lines
            .iter()
            .all(|(a, b)| usize::from(*a) < catalog.stars.len()
                && usize::from(*b) < catalog.stars.len()));
    }

    #[test]
    fn famous_stars_have_the_right_position_magnitude_and_colour() {
        // (name, ra deg, dec deg, magnitude, B-V)
        let famous = [
            ("Sirius", 101.2875, -16.7161, -1.46, 0.0),
            ("Vega", 279.2347, 38.7837, 0.03, 0.0),
            ("Betelgeuse", 88.7929, 7.4071, 0.42, 1.85),
            ("Polaris", 37.9546, 89.2641, 2.02, 0.6),
            ("Arcturus", 213.9153, 19.1824, -0.05, 1.23),
            ("Rigel", 78.6345, -8.2016, 0.12, -0.03),
        ];
        for (name, ra, dec, magnitude, color) in famous {
            let star = find(ra, dec);
            let (found_ra, found_dec) = vector_to_equatorial(&star.vector);
            assert!(
                (found_ra - ra).abs() < 0.02 && (found_dec - dec).abs() < 0.01,
                "{name}"
            );
            assert!(
                (f64::from(star.magnitude) - magnitude).abs() < 0.2,
                "{name} {}",
                star.magnitude
            );
            assert!(
                (f64::from(star.color_index) - color).abs() < 0.2,
                "{name} {}",
                star.color_index
            );
        }
        // Sirius is the brightest star of the sky.
        assert!(catalog().stars[0].magnitude < -1.4);
    }

    #[test]
    fn number_of_stars_per_magnitude_matches_the_sky() {
        let count = |limit: f32| {
            catalog()
                .stars
                .iter()
                .filter(|s| s.magnitude <= limit)
                .count()
        };
        // Well known tallies of the whole sky: 15 stars to magnitude 1.0
        // (the "first magnitude" stars), about 48 to 2.0, 170 to 3.0, 520 to 4.0.
        assert!((13..=17).contains(&count(1.0)), "{}", count(1.0));
        assert!((40..=60).contains(&count(2.0)), "{}", count(2.0));
        assert!((140..=200).contains(&count(3.0)), "{}", count(3.0));
        assert!((450..=620).contains(&count(4.0)), "{}", count(4.0));
    }

    #[test]
    fn constellation_lines_join_nearby_stars() {
        let catalog = catalog();
        for (a, b) in &catalog.lines {
            let (star_a, star_b) = (
                &catalog.stars[usize::from(*a)],
                &catalog.stars[usize::from(*b)],
            );
            let separation = dot(&star_a.vector, &star_b.vector)
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            assert!(
                separation < 31.0,
                "segment {a}-{b} spans {separation} degrees"
            );
        }
    }

    #[test]
    fn malformed_assets_are_rejected() {
        assert!(decode_stars(b"nope").is_none());
        assert!(decode_stars(b"BSC1\x05\x00\x00\x00").is_none());
        assert!(decode_lines(b"CLN1\x02\x00\x00\x00").is_none());
        assert!(decode_lines(b"").is_none());
    }

    #[test]
    fn colour_ramp_runs_from_blue_white_to_orange_red() {
        let blue = color_of_index(-0.3);
        let white = color_of_index(0.0);
        let yellow = color_of_index(0.65);
        let red = color_of_index(1.8);
        assert!(blue[2] >= blue[0], "{blue:?}");
        assert!(white.iter().all(|c| *c > 200), "{white:?}");
        assert!(yellow[0] > yellow[2] + 15, "{yellow:?}");
        assert!(
            red[0] > red[1] && red[1] > red[2] && red[0] > red[2] + 80,
            "{red:?}"
        );
    }
}
