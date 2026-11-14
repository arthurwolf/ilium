//! Shared, presentation-independent primitives. No simulation or clock state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hunters,
    Snake,
    Life,
    AutoChess,
    LiveChess,
    Dvd,
    Orbits,
    DigitalClock,
    AnalogClock,
}

#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub radius: f32,
    pub height: f32,
}

pub const MAX_BODIES: usize = 4096;
pub const MIN_RADIUS: f32 = 0.005;
pub const MAX_RADIUS: f32 = 0.24;

impl Body {
    /// Invalid/out-of-ground endpoints, nonfinite values, and nonpositive
    /// radius/height have no influence. Positive radii clamp to the supported
    /// range; heights clamp to one ground unit. Input records are never mutated.
    pub fn normalized(self) -> Option<Self> {
        if !self
            .from
            .into_iter()
            .chain(self.to)
            .all(|x| x.is_finite() && (0.0..=1.0).contains(&x))
            || !self.radius.is_finite()
            || self.radius <= 0.0
            || !self.height.is_finite()
            || self.height <= 0.0
        {
            return None;
        }
        let clean = |v: f32| if v == 0.0 { 0.0 } else { v };
        Some(Self {
            from: self.from.map(clean),
            to: self.to.map(clean),
            radius: self.radius.clamp(MIN_RADIUS, MAX_RADIUS),
            height: self.height.min(1.0),
        })
    }

    /// Conservative closed support rectangle, intersected with the ground.
    #[cfg(test)]
    pub fn support(self) -> Option<([f32; 2], [f32; 2])> {
        let b = self.normalized()?;
        Some((
            std::array::from_fn(|i| (b.from[i].min(b.to[i]) - b.radius).max(0.0)),
            std::array::from_fn(|i| (b.from[i].max(b.to[i]) + b.radius).min(1.0)),
        ))
    }

    /// Exact analytic primitive, not the renderer's sampled approximation.
    /// softness 0..1 blends compact quadratic/cubic caps; it never expands
    /// support. Every cap and endpoint join is C1, including the flat boundary.
    #[cfg(test)]
    pub fn height_at(self, point: [f32; 2], softness: f32) -> f32 {
        if !point
            .into_iter()
            .all(|x| x.is_finite() && (0.0..=1.0).contains(&x))
        {
            return 0.0;
        }
        Prepared::new(self).map_or(0.0, |b| {
            b.sample(point, finite(softness, 0.5).clamp(0.0, 1.0))
        })
    }
}

pub(super) fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Prepared {
    pub key: [f32; 5], // canonical from, to, radius; equal shapes deduplicate
    pub height: f32,
    axis: [f32; 2],
    length: f32,
    inverse_radius: f32,
}

impl Prepared {
    pub fn new(body: Body) -> Option<Self> {
        let mut b = body.normalized()?;
        if b.from[0]
            .total_cmp(&b.to[0])
            .then(b.from[1].total_cmp(&b.to[1]))
            .is_gt()
        {
            std::mem::swap(&mut b.from, &mut b.to);
        }
        let delta = [b.to[0] - b.from[0], b.to[1] - b.from[1]];
        let length = delta[0].hypot(delta[1]);
        Some(Self {
            key: [b.from[0], b.from[1], b.to[0], b.to[1], b.radius],
            height: b.height,
            axis: if length > 0.0 {
                delta.map(|v| v / length)
            } else {
                [0.0; 2]
            },
            length,
            inverse_radius: 1.0 / (b.radius * b.radius),
        })
    }

    pub fn distance_squared(self, p: [f32; 2]) -> f32 {
        let v = [p[0] - self.key[0], p[1] - self.key[1]];
        let t = (v[0] * self.axis[0] + v[1] * self.axis[1]).clamp(0.0, self.length);
        let d = [v[0] - t * self.axis[0], v[1] - t * self.axis[1]];
        d[0] * d[0] + d[1] * d[1]
    }

    pub fn envelope(self, distance_squared: f32, softness: f32) -> f32 {
        let u = (1.0 - distance_squared * self.inverse_radius).clamp(0.0, 1.0);
        self.height * u * u * (1.0 - softness + softness * u)
    }

    pub fn sample(self, point: [f32; 2], softness: f32) -> f32 {
        self.envelope(self.distance_squared(point), softness)
    }
}
