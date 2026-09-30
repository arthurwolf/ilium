//! Embedded low-resolution land mask for the location picker and map overlays.
//!
//! Data: Natural Earth 110m "land" (public domain,
//! <https://www.naturalearthdata.com/about/terms-of-use/>) rasterised to a
//! 360x180 one-bit grid by `tools/build_land_mask.py` into
//! `assets/land_mask.bin` (8100 bytes, row-major, north row first, LSB first).

/// Mask size in samples: `WIDTH` columns span longitude -180..180, `HEIGHT`
/// rows span latitude 90..-90 (row 0 is north).
pub const WIDTH: usize = 360;
pub const HEIGHT: usize = 180;

static MASK: &[u8] = include_bytes!("../assets/land_mask.bin");

/// Braille bit for each dot of a cell, indexed `[dot_row][dot_column]`; the
/// same layout `debug.rs` and the host use.
pub const BRAILLE_BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];

/// True when the mask sample at `column`, `row` is land. Out-of-range samples
/// are sea.
pub fn land_dot(column: usize, row: usize) -> bool {
    if column >= WIDTH || row >= HEIGHT {
        return false;
    }
    let index = row * WIDTH + column;
    MASK.get(index / 8)
        .is_some_and(|byte| byte >> (index % 8) & 1 == 1)
}

/// The mask sample containing a point, clamped to the grid.
pub fn sample_of(longitude: f64, latitude: f64) -> (usize, usize) {
    let column = (longitude + 180.0).floor().clamp(0.0, (WIDTH - 1) as f64) as usize;
    let row = (90.0 - latitude).floor().clamp(0.0, (HEIGHT - 1) as f64) as usize;
    (column, row)
}

/// True when the point is land. Longitude/latitude in degrees.
pub fn is_land(longitude: f64, latitude: f64) -> bool {
    if !longitude.is_finite() || !latitude.is_finite() {
        return false;
    }
    let wrapped = (longitude + 180.0).rem_euclid(360.0) - 180.0;
    let (column, row) = sample_of(wrapped, latitude);
    land_dot(column, row)
}

/// Fraction (0..1) of the mask covered by land in the box
/// `[x0, x1) x [y0, y1)`, expressed in mask sample units.
fn land_coverage(x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    let mut land = 0.0;
    let mut area = 0.0;
    let first_row = y0.floor().max(0.0) as usize;
    let last_row = (y1.ceil() as usize).min(HEIGHT);
    let first_column = x0.floor().max(0.0) as usize;
    let last_column = (x1.ceil() as usize).min(WIDTH);
    for row in first_row..last_row {
        let height = (row as f64 + 1.0).min(y1) - (row as f64).max(y0);
        if height <= 0.0 {
            continue;
        }
        for column in first_column..last_column {
            let width = (column as f64 + 1.0).min(x1) - (column as f64).max(x0);
            if width <= 0.0 {
                continue;
            }
            area += width * height;
            if land_dot(column, row) {
                land += width * height;
            }
        }
    }
    if area > 0.0 {
        land / area
    } else {
        0.0
    }
}

/// Braille bit masks (2x4 dots per cell, layout `BRAILLE_BITS`) of the
/// equirectangular land mask resampled to `width_cells` x `height_cells`.
/// Result is indexed `[cell_row][cell_column]`. Each dot is lit when land
/// covers at least half of its footprint (area-weighted box sampling).
pub fn braille_map(width_cells: u16, height_cells: u16) -> Vec<Vec<u8>> {
    let dots_wide = usize::from(width_cells) * 2;
    let dots_high = usize::from(height_cells) * 4;
    if dots_wide == 0 || dots_high == 0 {
        return vec![vec![0; usize::from(width_cells)]; usize::from(height_cells)];
    }
    let step_x = WIDTH as f64 / dots_wide as f64;
    let step_y = HEIGHT as f64 / dots_high as f64;
    (0..usize::from(height_cells))
        .map(|cell_y| {
            (0..usize::from(width_cells))
                .map(|cell_x| {
                    let mut mask = 0u8;
                    for (dot_y, row) in BRAILLE_BITS.iter().enumerate() {
                        for (dot_x, bit) in row.iter().enumerate() {
                            let x = (cell_x * 2 + dot_x) as f64;
                            let y = (cell_y * 4 + dot_y) as f64;
                            if land_coverage(
                                x * step_x,
                                (x + 1.0) * step_x,
                                y * step_y,
                                (y + 1.0) * step_y,
                            ) >= 0.5
                            {
                                mask |= bit;
                            }
                        }
                    }
                    mask
                })
                .collect()
        })
        .collect()
}

/// Longitude/latitude of the centre of a terminal cell of a `width_cells` x
/// `height_cells` map (the geometry `braille_map` draws).
pub fn cell_to_lonlat(cell_x: u16, cell_y: u16, width_cells: u16, height_cells: u16) -> (f64, f64) {
    let longitude = (f64::from(cell_x) + 0.5) / f64::from(width_cells.max(1)) * 360.0 - 180.0;
    let latitude = 90.0 - (f64::from(cell_y) + 0.5) / f64::from(height_cells.max(1)) * 180.0;
    (longitude, latitude)
}

/// The terminal cell of the map containing a point, clamped to the map.
pub fn lonlat_to_cell(
    longitude: f64,
    latitude: f64,
    width_cells: u16,
    height_cells: u16,
) -> (u16, u16) {
    let width = f64::from(width_cells.max(1));
    let height = f64::from(height_cells.max(1));
    let x = ((longitude + 180.0) / 360.0 * width)
        .floor()
        .clamp(0.0, width - 1.0);
    let y = ((90.0 - latitude) / 180.0 * height)
        .floor()
        .clamp(0.0, height - 1.0);
    (x as u16, y as u16)
}

/// Braille dot (column, row) of a point, in the dot grid of the map
/// (`2 * width_cells` x `4 * height_cells`), for drawing a marker.
pub fn lonlat_to_dot(
    longitude: f64,
    latitude: f64,
    width_cells: u16,
    height_cells: u16,
) -> (usize, usize) {
    let dots_wide = f64::from(width_cells.max(1)) * 2.0;
    let dots_high = f64::from(height_cells.max(1)) * 4.0;
    let x = ((longitude + 180.0) / 360.0 * dots_wide)
        .floor()
        .clamp(0.0, dots_wide - 1.0);
    let y = ((90.0 - latitude) / 180.0 * dots_high)
        .floor()
        .clamp(0.0, dots_high - 1.0);
    (x as usize, y as usize)
}

/// Longitude/latitude of the centre of a dot of the map's dot grid.
pub fn dot_to_lonlat(
    dot_x: usize,
    dot_y: usize,
    width_cells: u16,
    height_cells: u16,
) -> (f64, f64) {
    let dots_wide = f64::from(width_cells.max(1)) * 2.0;
    let dots_high = f64::from(height_cells.max(1)) * 4.0;
    let longitude = (dot_x as f64 + 0.5) / dots_wide * 360.0 - 180.0;
    let latitude = 90.0 - (dot_y as f64 + 0.5) / dots_high * 180.0;
    (longitude, latitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_has_the_advertised_size_and_a_plausible_land_share() {
        assert_eq!(MASK.len(), WIDTH * HEIGHT / 8);
        let land = (0..HEIGHT)
            .flat_map(|row| (0..WIDTH).map(move |column| (column, row)))
            .filter(|(column, row)| land_dot(*column, *row))
            .count();
        let share = land as f64 / (WIDTH * HEIGHT) as f64;
        // Earth is 29 percent land; an equirectangular grid over-weights the
        // polar continents, so accept a band around 0.33.
        assert!((0.28..0.38).contains(&share), "land share {share}");
    }

    #[test]
    fn well_known_places_are_land() {
        let places = [
            ("Paris", 2.35, 48.86),
            ("London", -0.13, 51.5),
            ("Pittsburgh", -80.0, 40.4),
            ("Tokyo", 139.7, 35.7),
            ("Sydney outskirts", 150.9, -33.8),
            ("Cairo", 31.24, 30.05),
            ("Moscow", 37.6, 55.75),
            ("Delhi", 77.2, 28.6),
            ("Beijing", 116.4, 39.9),
            ("Sao Paulo", -46.6, -23.55),
            ("Mexico City", -99.13, 19.43),
            ("Johannesburg", 28.05, -26.2),
            ("Nairobi", 36.8, -1.29),
            ("Sahara", 10.0, 25.0),
            ("Amazon", -60.0, -5.0),
            ("Greenland", -40.0, 72.0),
            ("Antarctica", 0.0, -80.0),
            ("Siberia", 100.0, 62.0),
            ("Australia interior", 134.0, -25.0),
            ("Kansas", -98.0, 38.5),
        ];
        for (name, longitude, latitude) in places {
            assert!(is_land(longitude, latitude), "{name} should be land");
        }
    }

    #[test]
    fn oceans_are_sea() {
        let seas = [
            ("mid Pacific", -150.0, 0.0),
            ("mid Atlantic", -30.0, 30.0),
            ("Indian Ocean", 80.0, -20.0),
            ("south Pacific", -130.0, -40.0),
            ("Arctic pole", 0.0, 89.0),
            ("south Atlantic", -15.0, -30.0),
            ("Southern Ocean", 100.0, -60.0),
            ("north Pacific", 180.0, 40.0),
            ("Bay of Bengal", 88.0, 14.0),
            ("Gulf of Guinea", 0.0, 0.0),
        ];
        for (name, longitude, latitude) in seas {
            assert!(!is_land(longitude, latitude), "{name} should be sea");
        }
    }

    #[test]
    fn longitude_wraps_and_bad_input_is_sea() {
        assert_eq!(is_land(2.35 + 360.0, 48.86), is_land(2.35, 48.86));
        assert!(!is_land(f64::NAN, 10.0));
        assert!(!land_dot(WIDTH, 0));
        assert!(!land_dot(0, HEIGHT));
    }

    #[test]
    fn sample_indexing_corners() {
        assert_eq!(sample_of(-180.0, 90.0), (0, 0));
        assert_eq!(sample_of(180.0, -90.0), (WIDTH - 1, HEIGHT - 1));
        assert_eq!(sample_of(0.5, 0.5), (180, 89));
    }

    #[test]
    fn cell_round_trip_and_corners() {
        let (width, height) = (60u16, 20u16);
        for cell_y in 0..height {
            for cell_x in 0..width {
                let (longitude, latitude) = cell_to_lonlat(cell_x, cell_y, width, height);
                assert_eq!(
                    lonlat_to_cell(longitude, latitude, width, height),
                    (cell_x, cell_y)
                );
            }
        }
        assert_eq!(lonlat_to_cell(-180.0, 90.0, width, height), (0, 0));
        assert_eq!(
            lonlat_to_cell(180.0, -90.0, width, height),
            (width - 1, height - 1)
        );
        assert_eq!(lonlat_to_cell(500.0, 500.0, width, height), (width - 1, 0));
    }

    #[test]
    fn dot_round_trip() {
        let (width, height) = (40u16, 12u16);
        for dot_y in 0..usize::from(height) * 4 {
            for dot_x in 0..usize::from(width) * 2 {
                let (longitude, latitude) = dot_to_lonlat(dot_x, dot_y, width, height);
                assert_eq!(
                    lonlat_to_dot(longitude, latitude, width, height),
                    (dot_x, dot_y)
                );
            }
        }
    }

    #[test]
    fn braille_map_has_requested_shape_and_marks_paris_and_not_the_pacific() {
        let (width, height) = (72u16, 24u16);
        let map = braille_map(width, height);
        assert_eq!(map.len(), usize::from(height));
        assert!(map.iter().all(|row| row.len() == usize::from(width)));
        let lit = |longitude: f64, latitude: f64| -> bool {
            let (dot_x, dot_y) = lonlat_to_dot(longitude, latitude, width, height);
            let mask = map[dot_y / 4][dot_x / 2];
            mask & BRAILLE_BITS[dot_y % 4][dot_x % 2] != 0
        };
        assert!(lit(2.35, 48.86), "Paris");
        assert!(lit(-98.0, 38.5), "Kansas");
        assert!(!lit(-150.0, 0.0), "mid Pacific");
        assert!(!lit(-30.0, 30.0), "mid Atlantic");
        let lit_dots: u32 = map.iter().flatten().map(|mask| mask.count_ones()).sum();
        let share = f64::from(lit_dots) / f64::from(u32::from(width) * u32::from(height) * 8);
        assert!((0.25..0.40).contains(&share), "share {share}");
    }

    #[test]
    fn braille_map_at_native_resolution_matches_the_mask() {
        let map = braille_map((WIDTH / 2) as u16, (HEIGHT / 4) as u16);
        for row in (0..HEIGHT).step_by(7) {
            for column in (0..WIDTH).step_by(5) {
                let mask = map[row / 4][column / 2];
                let lit = mask & BRAILLE_BITS[row % 4][column % 2] != 0;
                assert_eq!(lit, land_dot(column, row), "sample {column},{row}");
            }
        }
        assert!(braille_map(0, 5).iter().all(|row| row.is_empty()));
        assert!(braille_map(5, 0).is_empty());
    }

    /// Visual check: `cargo test -p ilium-ambient world_map_preview -- --ignored --nocapture`
    #[test]
    #[ignore = "prints a preview for a human"]
    fn world_map_preview() {
        for row in braille_map(90, 22) {
            let line: String = row
                .iter()
                .map(|mask| char::from_u32(0x2800 + u32::from(*mask)).unwrap_or(' '))
                .collect();
            println!("{line}");
        }
    }
}
