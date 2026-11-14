//! A small signed-distance painter over the dot raster. Sprites are described
//! as shapes (ellipses, triangles, capsules, boxes) with a shader closure, so
//! they scale with the tile size and animate by moving their parameters; no
//! bitmaps are involved. Tone goes to the raster, colour to a parallel buffer
//! that is reduced to one colour per terminal cell at the end of the frame.

pub type Rgb = [u8; 3];

/// Tone (0..1, dithered by the host) and colour of one dot.
pub type Paint = (f32, Rgb);

pub struct Canvas<'a> {
    width: usize,
    height: usize,
    tone: &'a mut [f32],
    color: &'a mut [Rgb],
}

type Point = (f32, f32);

fn dot(first: Point, second: Point) -> f32 {
    first.0 * second.0 + first.1 * second.1
}

fn sub(first: Point, second: Point) -> Point {
    (first.0 - second.0, first.1 - second.1)
}

pub fn sd_ellipse(px: f32, py: f32, rx: f32, ry: f32) -> f32 {
    let rx = rx.max(0.01);
    let ry = ry.max(0.01);
    let k0 = ((px / rx).powi(2) + (py / ry).powi(2)).sqrt();
    let k1 = ((px / (rx * rx)).powi(2) + (py / (ry * ry)).powi(2)).sqrt();
    if k1 < 1e-6 {
        return -rx.min(ry);
    }
    k0 * (k0 - 1.0) / k1
}

pub fn sd_segment(p: Point, from: Point, to: Point) -> f32 {
    let pa = sub(p, from);
    let ba = sub(to, from);
    let length_squared = dot(ba, ba).max(1e-6);
    let fraction = (dot(pa, ba) / length_squared).clamp(0.0, 1.0);
    let offset = (pa.0 - ba.0 * fraction, pa.1 - ba.1 * fraction);
    dot(offset, offset).sqrt()
}

pub fn sd_box(px: f32, py: f32, half_width: f32, half_height: f32) -> f32 {
    let dx = px.abs() - half_width;
    let dy = py.abs() - half_height;
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0)
}

pub fn sd_triangle(p: Point, a: Point, b: Point, c: Point) -> f32 {
    let edges = [sub(b, a), sub(c, b), sub(a, c)];
    let vectors = [sub(p, a), sub(p, b), sub(p, c)];
    let sign = if edges[0].0 * edges[2].1 - edges[0].1 * edges[2].0 >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let mut best_distance = f32::MAX;
    let mut best_side = f32::MAX;
    for index in 0..3 {
        let edge = edges[index];
        let vector = vectors[index];
        let fraction = (dot(vector, edge) / dot(edge, edge).max(1e-6)).clamp(0.0, 1.0);
        let nearest = (vector.0 - edge.0 * fraction, vector.1 - edge.1 * fraction);
        best_distance = best_distance.min(dot(nearest, nearest));
        best_side = best_side.min(sign * (vector.0 * edge.1 - vector.1 * edge.0));
    }
    -best_distance.sqrt() * if best_side >= 0.0 { 1.0 } else { -1.0 }
}

impl<'a> Canvas<'a> {
    pub fn new(width: usize, height: usize, tone: &'a mut [f32], color: &'a mut [Rgb]) -> Self {
        debug_assert_eq!(tone.len(), width * height);
        debug_assert_eq!(color.len(), width * height);
        Self {
            width,
            height,
            tone,
            color,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Blend one dot towards `tone`. Dark paint leaves the colour alone so
    /// outlines never recolour what they surround.
    pub fn blend(&mut self, x: usize, y: usize, coverage: f32, tone: f32, color: Rgb) {
        if x >= self.width || y >= self.height || coverage <= 0.0 {
            return;
        }
        let index = y * self.width + x;
        let old = self.tone[index];
        self.tone[index] = old + (tone - old) * coverage.min(1.0);
        if coverage >= 0.5 && tone > 0.0 {
            self.color[index] = color;
        }
    }

    pub fn tone_at(&self, x: usize, y: usize) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.tone[y * self.width + x]
    }

    pub fn scale_tone_at(&mut self, x: usize, y: usize, factor: f32) {
        if x < self.width && y < self.height {
            self.tone[y * self.width + x] *= factor;
        }
    }

    /// Paint `distance < 0` regions with `paint`, ringed by `outline` dots of
    /// black. `bounds` is (min x, min y, max x, max y) of the shape in dots.
    pub fn shape(
        &mut self,
        bounds: (f32, f32, f32, f32),
        outline: f32,
        distance: impl Fn(f32, f32) -> f32,
        paint: impl Fn(f32, f32) -> Paint,
    ) {
        let margin = outline + 1.0;
        let left = (bounds.0 - margin).floor().max(0.0) as usize;
        let top = (bounds.1 - margin).floor().max(0.0) as usize;
        let right = ((bounds.2 + margin).ceil().max(0.0) as usize).min(self.width);
        let bottom = ((bounds.3 + margin).ceil().max(0.0) as usize).min(self.height);
        for y in top..bottom {
            let py = y as f32 + 0.5;
            for x in left..right {
                let px = x as f32 + 0.5;
                let d = distance(px, py);
                if d > outline + 0.5 {
                    continue;
                }
                if outline > 0.0 {
                    let ring = (outline + 0.5 - d).clamp(0.0, 1.0);
                    self.blend(x, y, ring, 0.0, [0, 0, 0]);
                }
                let fill = (0.5 - d).clamp(0.0, 1.0);
                if fill > 0.0 {
                    let (tone, color) = paint(px, py);
                    self.blend(x, y, fill, tone, color);
                }
            }
        }
    }

    pub fn ellipse(
        &mut self,
        center: Point,
        radii: Point,
        outline: f32,
        paint: impl Fn(f32, f32) -> Paint,
    ) {
        let bounds = (
            center.0 - radii.0,
            center.1 - radii.1,
            center.0 + radii.0,
            center.1 + radii.1,
        );
        self.shape(
            bounds,
            outline,
            |px, py| sd_ellipse(px - center.0, py - center.1, radii.0, radii.1),
            paint,
        );
    }

    pub fn triangle(
        &mut self,
        a: Point,
        b: Point,
        c: Point,
        outline: f32,
        paint: impl Fn(f32, f32) -> Paint,
    ) {
        let bounds = (
            a.0.min(b.0).min(c.0),
            a.1.min(b.1).min(c.1),
            a.0.max(b.0).max(c.0),
            a.1.max(b.1).max(c.1),
        );
        self.shape(
            bounds,
            outline,
            |px, py| sd_triangle((px, py), a, b, c),
            paint,
        );
    }

    /// A line of constant `width` (full thickness in dots).
    pub fn capsule(
        &mut self,
        from: Point,
        to: Point,
        width: f32,
        outline: f32,
        paint: impl Fn(f32, f32) -> Paint,
    ) {
        let half = width * 0.5;
        let bounds = (
            from.0.min(to.0) - half,
            from.1.min(to.1) - half,
            from.0.max(to.0) + half,
            from.1.max(to.1) + half,
        );
        self.shape(
            bounds,
            outline,
            |px, py| sd_segment((px, py), from, to) - half,
            paint,
        );
    }

    pub fn rect(
        &mut self,
        center: Point,
        half: Point,
        outline: f32,
        paint: impl Fn(f32, f32) -> Paint,
    ) {
        let bounds = (
            center.0 - half.0,
            center.1 - half.1,
            center.0 + half.0,
            center.1 + half.1,
        );
        self.shape(
            bounds,
            outline,
            |px, py| sd_box(px - center.0, py - center.1, half.0, half.1),
            paint,
        );
    }

    /// Replaces every dot colour with `map(colour)`.
    pub fn map_colors(&mut self, map: impl Fn(Rgb) -> Rgb) {
        for color in self.color.iter_mut() {
            *color = map(*color);
        }
    }

    /// Reduces dot colours to one colour per terminal cell, weighted by tone.
    /// Cells with no lit dot take the plain mean so dim cells keep their hue.
    pub fn reduce_to_cells(&self, cell_columns: usize, cell_rows: usize, out: &mut [[u8; 3]]) {
        for cell_y in 0..cell_rows {
            for cell_x in 0..cell_columns {
                let mut weight_sum = 0.0f32;
                let mut sums = [0.0f32; 3];
                let mut plain = [0.0f32; 3];
                let mut count = 0.0f32;
                for dy in 0..4 {
                    for dx in 0..2 {
                        let x = cell_x * 2 + dx;
                        let y = cell_y * 4 + dy;
                        if x >= self.width || y >= self.height {
                            continue;
                        }
                        let index = y * self.width + x;
                        let weight = self.tone[index];
                        let color = self.color[index];
                        for channel in 0..3 {
                            sums[channel] += weight * f32::from(color[channel]);
                            plain[channel] += f32::from(color[channel]);
                        }
                        weight_sum += weight;
                        count += 1.0;
                    }
                }
                let Some(slot) = out.get_mut(cell_y * cell_columns + cell_x) else {
                    continue;
                };
                for channel in 0..3 {
                    let value = if weight_sum > 0.05 {
                        sums[channel] / weight_sum
                    } else if count > 0.0 {
                        plain[channel] / count
                    } else {
                        0.0
                    };
                    slot[channel] = value.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}
