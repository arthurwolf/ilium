//! Drawing primitives on the Braille dot raster with optional per-cell colour.

use crate::raster::Raster;

pub type Rgb = [u8; 3];

/// A cluster of dots relative to the star centre with an intensity weight.
type Cluster = &'static [(i32, i32, f32)];

/// One full dot.
pub const SINGLE: Cluster = &[(0, 0, 1.0)];
/// Plus shape: five dots.
pub const PLUS: Cluster = &[
    (0, 0, 1.0),
    (-1, 0, 0.8),
    (1, 0, 0.8),
    (0, -1, 0.8),
    (0, 1, 0.8),
];
/// 3x3 blob with softer corners.
pub const BLOB: Cluster = &[
    (0, 0, 1.0),
    (-1, 0, 0.9),
    (1, 0, 0.9),
    (0, -1, 0.9),
    (0, 1, 0.9),
    (-1, -1, 0.6),
    (1, -1, 0.6),
    (-1, 1, 0.6),
    (1, 1, 0.6),
];
/// Large disc for the brightest planets.
pub const DISC: Cluster = &[
    (0, 0, 1.0),
    (-1, 0, 1.0),
    (1, 0, 1.0),
    (0, -1, 1.0),
    (0, 1, 1.0),
    (-1, -1, 0.9),
    (1, -1, 0.9),
    (-1, 1, 0.9),
    (1, 1, 0.9),
    (-2, 0, 0.7),
    (2, 0, 0.7),
    (0, -2, 0.7),
    (0, 2, 0.7),
];

pub struct Canvas<'a> {
    raster: &'a mut Raster,
    colors: &'a mut Vec<Rgb>,
    /// Strongest intensity plotted so far in each cell, for colour ownership.
    strongest: &'a mut Vec<f32>,
    width_cells: usize,
    use_colors: bool,
}

impl<'a> Canvas<'a> {
    pub fn new(
        raster: &'a mut Raster,
        colors: &'a mut Vec<Rgb>,
        strongest: &'a mut Vec<f32>,
        width_cells: usize,
        height_cells: usize,
        use_colors: bool,
        background: Rgb,
    ) -> Self {
        strongest.clear();
        strongest.resize(width_cells * height_cells, 0.0);
        if use_colors {
            colors.clear();
            colors.resize(width_cells * height_cells, background);
        }
        Self {
            raster,
            colors,
            strongest,
            width_cells,
            use_colors,
        }
    }

    pub fn width(&self) -> usize {
        self.raster.width
    }

    pub fn height(&self) -> usize {
        self.raster.height
    }

    /// Light one dot with `max` semantics; the cell takes the colour of its
    /// strongest dot.
    pub fn plot(&mut self, x: i64, y: i64, intensity: f32, color: Rgb) {
        if x < 0 || y < 0 || x as usize >= self.raster.width || y as usize >= self.raster.height {
            return;
        }
        let intensity = intensity.clamp(0.0, 1.0);
        if intensity <= 0.0 {
            return;
        }
        let index = y as usize * self.raster.width + x as usize;
        if intensity > self.raster.dots[index] {
            self.raster.dots[index] = intensity;
        }
        if self.use_colors {
            let cell = (y as usize / 4) * self.width_cells + x as usize / 2;
            if let (Some(best), Some(slot)) =
                (self.strongest.get_mut(cell), self.colors.get_mut(cell))
            {
                if intensity >= *best {
                    *best = intensity;
                    *slot = color;
                }
            }
        }
    }

    /// Place a cluster centred on the dot containing `(x, y)`.
    pub fn cluster(&mut self, x: f64, y: f64, shape: Cluster, intensity: f32, color: Rgb) {
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        for (dx, dy, weight) in shape {
            self.plot(
                cx + i64::from(*dx),
                cy + i64::from(*dy),
                intensity * weight,
                color,
            );
        }
    }

    /// Two horizontally adjacent dots, the second on the side the position
    /// leans to, so that slow motion alternates between them.
    pub fn pair(&mut self, x: f64, y: f64, intensity: f32, color: Rgb) {
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let side = if x - x.floor() < 0.5 { -1 } else { 1 };
        self.plot(cx, cy, intensity, color);
        self.plot(cx + side, cy, intensity * 0.8, color);
    }

    /// A thin line of single dots, clipped to the raster, with `max` blending.
    pub fn line(&mut self, from: (f64, f64), to: (f64, f64), intensity: f32, color: Rgb) {
        let Some((from, to)) = clip_segment(
            from,
            to,
            self.raster.width as f64,
            self.raster.height as f64,
        ) else {
            return;
        };
        let length = (to.0 - from.0).hypot(to.1 - from.1);
        let steps = (length * 2.0).ceil().max(1.0) as usize;
        let mut previous = (i64::MIN, i64::MIN);
        for step in 0..=steps {
            let fraction = step as f64 / steps as f64;
            let x = (from.0 + (to.0 - from.0) * fraction).floor() as i64;
            let y = (from.1 + (to.1 - from.1) * fraction).floor() as i64;
            if (x, y) != previous {
                self.plot(x, y, intensity, color);
                previous = (x, y);
            }
        }
    }
}

/// Liang-Barsky clipping of a segment to `[0, width) x [0, height)`.
pub fn clip_segment(
    from: (f64, f64),
    to: (f64, f64),
    width: f64,
    height: f64,
) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let mut low = 0.0_f64;
    let mut high = 1.0_f64;
    let bound = 1e-6;
    for (p, q) in [
        (-dx, from.0),
        (dx, width - bound - from.0),
        (-dy, from.1),
        (dy, height - bound - from.1),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
        } else {
            let ratio = q / p;
            if p < 0.0 {
                low = low.max(ratio);
            } else {
                high = high.min(ratio);
            }
            if low > high {
                return None;
            }
        }
    }
    Some((
        (from.0 + dx * low, from.1 + dy * low),
        (from.0 + dx * high, from.1 + dy * high),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_canvas(
        width_cells: usize,
        height_cells: usize,
        use_colors: bool,
        body: impl FnOnce(&mut Canvas<'_>),
    ) -> (Raster, Vec<Rgb>) {
        let mut raster = Raster::default();
        raster.resize(width_cells * 2, height_cells * 4);
        let mut colors = Vec::new();
        let mut strongest = Vec::new();
        {
            let mut canvas = Canvas::new(
                &mut raster,
                &mut colors,
                &mut strongest,
                width_cells,
                height_cells,
                use_colors,
                [1, 2, 3],
            );
            body(&mut canvas);
        }
        (raster, colors)
    }

    #[test]
    fn plot_clips_uses_max_and_keeps_strongest_cell_colour() {
        let (raster, colors) = with_canvas(4, 2, true, |canvas| {
            canvas.plot(-1, 0, 1.0, [9, 9, 9]);
            canvas.plot(0, 100, 1.0, [9, 9, 9]);
            canvas.plot(1, 1, 0.4, [10, 0, 0]);
            canvas.plot(0, 0, 0.9, [0, 20, 0]);
            canvas.plot(1, 1, 0.2, [30, 30, 30]);
            canvas.plot(3, 0, 5.0, [0, 0, 40]);
        });
        assert_eq!(raster.dots.iter().filter(|dot| **dot > 0.0).count(), 3);
        assert!((raster.dots[8 + 1] - 0.4).abs() < 1e-6);
        assert_eq!(
            colors[0],
            [0, 20, 0],
            "the 0.9 dot owns cell 0 over the 0.4 and 0.2 dots"
        );
        assert_eq!(colors[1], [0, 0, 40]);
        assert_eq!(colors[2], [1, 2, 3], "untouched cells keep the background");
        assert_eq!(colors.len(), 8);
    }

    #[test]
    fn colours_are_left_alone_when_unused() {
        let (_, colors) = with_canvas(3, 3, false, |canvas| canvas.plot(0, 0, 1.0, [5, 5, 5]));
        assert!(colors.is_empty());
    }

    #[test]
    fn clusters_have_the_advertised_dot_counts() {
        let count = |shape: Cluster| {
            with_canvas(8, 4, false, |canvas| {
                canvas.cluster(8.2, 8.7, shape, 1.0, [0; 3])
            })
            .0
            .dots
            .iter()
            .filter(|dot| **dot > 0.0)
            .count()
        };
        assert_eq!(count(SINGLE), 1);
        assert_eq!(count(PLUS), 5);
        assert_eq!(count(BLOB), 9);
        assert_eq!(count(DISC), 13);
        let (pair, _) = with_canvas(8, 4, false, |canvas| canvas.pair(8.9, 8.2, 1.0, [0; 3]));
        assert_eq!(pair.dots.iter().filter(|dot| **dot > 0.0).count(), 2);
        assert!(pair.dots[8 * 16 + 8] > 0.99 && pair.dots[8 * 16 + 9] > 0.7);
    }

    #[test]
    fn line_is_connected_and_clipped() {
        let (raster, _) = with_canvas(10, 5, false, |canvas| {
            canvas.line((-50.0, -50.0), (500.0, 500.0), 1.0, [0; 3]);
        });
        // The 20 x 20 dot diagonal is crossed by exactly one dot per column.
        let lit: Vec<usize> = raster
            .dots
            .iter()
            .enumerate()
            .filter(|(_, dot)| **dot > 0.0)
            .map(|(index, _)| index)
            .collect();
        assert!(lit.len() >= 19 && lit.len() <= 21, "{}", lit.len());
        let (outside, _) = with_canvas(10, 5, false, |canvas| {
            canvas.line((-50.0, -10.0), (-5.0, 100.0), 1.0, [0; 3]);
        });
        assert!(outside.dots.iter().all(|dot| *dot == 0.0));
        let (horizontal, _) = with_canvas(10, 5, false, |canvas| {
            canvas.line((2.0, 3.5), (100.0, 3.5), 1.0, [0; 3]);
        });
        assert_eq!(horizontal.dots.iter().filter(|dot| **dot > 0.0).count(), 18);
    }

    #[test]
    fn clipping_keeps_inside_segments_untouched() {
        let inside = clip_segment((1.0, 1.0), (5.0, 7.0), 10.0, 10.0).unwrap();
        assert_eq!(inside, ((1.0, 1.0), (5.0, 7.0)));
        assert!(clip_segment((-5.0, -5.0), (-1.0, 20.0), 10.0, 10.0).is_none());
    }
}
