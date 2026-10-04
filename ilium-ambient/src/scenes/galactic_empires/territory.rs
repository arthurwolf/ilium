//! Compact, smooth per-owner influence; never an input to the simulation.
//! Geometry is cached. Dirty channels are rebuilt in star order (no drift).
use super::simulation::Star;
use crate::raster::smoothstep;

const SIDE: usize = 192;
const OWNERS: usize = 12; // The existing settings/simulation limit.
const NEUTRAL: usize = OWNERS;
const CHANNELS: usize = OWNERS + 1;
const STEP: f32 = 2.0 / (SIDE - 1) as f32;
const BASE_RADIUS: f32 = 0.18;
const CORE: f32 = 0.025;
const LOW: f32 = 0.025;
const BASE_HIGH: f32 = 0.16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Sample {
    pub owner: usize,
    pub coverage: f32,
    pub contour: f32,
    pub contact: Option<(usize, f32)>,
}

pub(super) struct Territory {
    positions: Vec<(f32, f32)>,
    owners: Vec<Option<usize>>,
    stencils: Vec<Vec<(u32, f32)>>,
    fields: Vec<[f32; CHANNELS]>,
    radius_percent: i32,
    softness_percent: i32,
}

fn channel(owner: Option<usize>) -> usize {
    // Galaxy caps civilizations at twelve; this private field never accepts
    // an owner from transport or persistence independently of that invariant.
    assert!(owner.is_none_or(|owner| owner < OWNERS));
    owner.unwrap_or(NEUTRAL)
}

impl Territory {
    pub fn new(stars: &[Star], radius_percent: i32, softness_percent: i32) -> Self {
        let radius_percent = radius_percent.clamp(50, 150);
        let softness_percent = softness_percent.clamp(50, 200);
        let radius = BASE_RADIUS * radius_percent as f32 / 100.0;
        let stencils = stars
            .iter()
            .map(|star| {
                let (sx, sy) = star.position;
                // Galaxy's bounded seeded generator always emits finite points.
                assert!(sx.is_finite() && sy.is_finite());
                let bound = |value: f32| value.clamp(0.0, (SIDE - 1) as f32) as usize;
                let left = bound(((sx - radius + 1.0) / STEP).floor());
                let right = bound(((sx + radius + 1.0) / STEP).ceil());
                let top = bound(((sy - radius + 1.0) / STEP).floor());
                let bottom = bound(((sy + radius + 1.0) / STEP).ceil());
                let mut stencil = Vec::new();
                for y in top..=bottom {
                    for x in left..=right {
                        let dx = -1.0 + x as f32 * STEP - sx;
                        let dy = -1.0 + y as f32 * STEP - sy;
                        let d2 = dx * dx + dy * dy;
                        if d2 < radius * radius {
                            let q = 1.0 - d2 / (radius * radius);
                            // C2 at support; a strong core protects small colonies.
                            let weight = q * q * q * CORE * CORE / (d2 + CORE * CORE);
                            stencil.push(((y * SIDE + x) as u32, weight));
                        }
                    }
                }
                stencil
            })
            .collect();
        let mut result = Self {
            positions: stars.iter().map(|star| star.position).collect(),
            owners: stars.iter().map(|star| star.owner).collect(),
            stencils,
            fields: vec![[0.0; CHANNELS]; SIDE * SIDE],
            radius_percent,
            softness_percent,
        };
        result.rebuild(&[true; CHANNELS]);
        result
    }

    pub fn set_softness(&mut self, softness_percent: i32) {
        self.softness_percent = softness_percent.clamp(50, 200);
    }

    fn rebuild(&mut self, dirty: &[bool; CHANNELS]) {
        for node in &mut self.fields {
            for c in 0..CHANNELS {
                if dirty[c] {
                    node[c] = 0.0;
                }
            }
        }
        for (stencil, &owner) in self.stencils.iter().zip(&self.owners) {
            let c = channel(owner);
            if dirty[c] {
                for &(index, weight) in stencil {
                    let value = &mut self.fields[index as usize][c];
                    // Smooth union: 1 - product(1 - weight), bounded by one.
                    *value += (1.0 - *value) * weight;
                }
            }
        }
    }

    /// Call once after all ticks, not just when last_captures is nonempty.
    pub fn sync(&mut self, stars: &[Star]) -> bool {
        if stars.len() != self.positions.len()
            || stars
                .iter()
                .zip(&self.positions)
                .any(|(star, &p)| star.position != p)
        {
            *self = Self::new(stars, self.radius_percent, self.softness_percent);
            return true;
        }
        let mut dirty = [false; CHANNELS];
        for (old, star) in self.owners.iter_mut().zip(stars) {
            if *old != star.owner {
                dirty[channel(*old)] = true;
                dirty[channel(star.owner)] = true;
                *old = star.owner;
            }
        }
        if !dirty.iter().any(|&value| value) {
            return false;
        }
        self.rebuild(&dirty);
        true
    }

    pub fn sample(&self, point: (f32, f32)) -> Option<Sample> {
        let (px, py) = point;
        let r2 = px * px + py * py;
        if !px.is_finite() || !py.is_finite() || r2 >= 1.0 {
            return None;
        }
        let gx = (px + 1.0) / STEP;
        let gy = (py + 1.0) / STEP;
        let x = (gx as usize).min(SIDE - 2);
        let y = (gy as usize).min(SIDE - 2);
        let tx = (gx - x as f32).clamp(0.0, 1.0);
        let ty = (gy - y as f32).clamp(0.0, 1.0);
        let nodes = [
            &self.fields[y * SIDE + x],
            &self.fields[y * SIDE + x + 1],
            &self.fields[(y + 1) * SIDE + x],
            &self.fields[(y + 1) * SIDE + x + 1],
        ];
        let values: [f32; CHANNELS] = std::array::from_fn(|c| {
            let top = nodes[0][c] + (nodes[1][c] - nodes[0][c]) * tx;
            let bottom = nodes[2][c] + (nodes[3][c] - nodes[2][c]) * tx;
            top + (bottom - top) * ty
        });
        // Neutral wins exact ties, then the lowest numbered owned channel.
        let mut owner = NEUTRAL;
        for c in 0..OWNERS {
            if values[c] > values[owner] {
                owner = c;
            }
        }
        let best = values[owner];
        let high = LOW + (BASE_HIGH - LOW) * self.softness_percent as f32 / 100.0;
        if owner == NEUTRAL || best <= LOW {
            return None;
        }
        let coverage = smoothstep(LOW, high, best)
            * smoothstep(0.0, 0.2, (best - values[NEUTRAL]) / best)
            * smoothstep(0.0, 0.03, 1.0 - r2.sqrt());
        let mut rival = None;
        let mut second = values[NEUTRAL];
        for (c, &value) in values[..OWNERS].iter().enumerate() {
            if c != owner && value > second {
                second = value;
                rival = Some(c);
            }
        }
        Some(Sample {
            owner,
            coverage,
            contour: 4.0 * coverage * (1.0 - coverage),
            contact: rival.map(|other| {
                (
                    other,
                    smoothstep(LOW, high, second)
                        * (1.0 - smoothstep(0.0, 0.24, (best - second) / best)),
                )
            }),
        })
    }
}

#[cfg(test)]
#[path = "territory_tests.rs"]
mod tests;
