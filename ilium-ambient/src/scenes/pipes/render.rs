//! Ray-cast rasterizer for axis-aligned cylinders and spheres.
//!
//! Every dot of the Braille raster is one pixel (dots are square). Primitives
//! are drawn one at a time inside their projected bounding box against a
//! z-buffer, so the cost is proportional to the covered area, not to the
//! number of primitives times the screen size.
//!
//! Rays are not normalized: `direction = forward + right * nx + up * ny`, so
//! the ray parameter `t` equals the depth along the view axis and the
//! z-buffer compares `t` directly.

use super::settings::{PipePattern, PipeShading};
use super::sim::perpendicular;
use crate::raster::{smoothstep, Raster};
use std::f32::consts::{PI, TAU};

type Vec3 = [f32; 3];

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: Vec3) -> Vec3 {
    let length = dot(a, a).sqrt().max(1e-9);
    [a[0] / length, a[1] / length, a[2] / length]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Pinhole camera looking at a target from an orbit position.
#[derive(Debug, Clone)]
pub struct Camera {
    pub eye: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    tan_v: f32,
    tan_h: f32,
    width: usize,
    height: usize,
    /// Depth of the nearest possible surface and the span to the farthest.
    pub near_depth: f32,
    pub depth_span: f32,
}

pub struct CameraSpec {
    pub target: Vec3,
    /// Radius of a sphere that contains the whole structure.
    pub bounding_radius: f32,
    pub fov_degrees: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Extra distance factor (1.0 = fitted).
    pub zoom_scale: f32,
    pub width: usize,
    pub height: usize,
}

impl Camera {
    /// Fraction of the bounding sphere radius that must fit the view: below 1
    /// crops the volume's corners so the pipes fill the frame.
    const FIT: f32 = 0.76;

    pub fn orbit(spec: &CameraSpec) -> Self {
        let aspect = spec.width as f32 / spec.height.max(1) as f32;
        let half_v = (spec.fov_degrees.to_radians() * 0.5).clamp(0.05, 1.4);
        let tan_v = half_v.tan();
        let tan_h = tan_v * aspect;
        let limiting_half = half_v.min(tan_h.atan());
        let radius = spec.bounding_radius;
        // Never closer than the bounding sphere: every surface stays in front
        // of the camera, so projection needs no near-plane clipping.
        let distance =
            (Self::FIT * radius / limiting_half.sin() * spec.zoom_scale).max(1.15 * radius);
        let from_target = [
            spec.pitch.cos() * spec.yaw.sin(),
            spec.pitch.sin(),
            spec.pitch.cos() * spec.yaw.cos(),
        ];
        let eye = [
            spec.target[0] + from_target[0] * distance,
            spec.target[1] + from_target[1] * distance,
            spec.target[2] + from_target[2] * distance,
        ];
        let forward = [-from_target[0], -from_target[1], -from_target[2]];
        let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        Self {
            eye,
            right,
            up,
            forward,
            tan_v,
            tan_h,
            width: spec.width,
            height: spec.height,
            near_depth: distance - radius,
            depth_span: 2.0 * radius,
        }
    }

    fn project(&self, point: Vec3) -> Option<(f32, f32)> {
        let relative = sub(point, self.eye);
        let depth = dot(relative, self.forward);
        if depth < 0.05 {
            return None;
        }
        let x = dot(relative, self.right) / depth / self.tan_h;
        let y = dot(relative, self.up) / depth / self.tan_v;
        Some((
            (x * 0.5 + 0.5) * self.width as f32,
            (0.5 - y * 0.5) * self.height as f32,
        ))
    }

    /// Screen rectangle `[x0, x1) x [y0, y1)` covering an axis-aligned box.
    fn bounds(&self, min: Vec3, max: Vec3) -> Option<[usize; 4]> {
        let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
        let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for corner in 0..8 {
            let point = [
                if corner & 1 == 0 { min[0] } else { max[0] },
                if corner & 2 == 0 { min[1] } else { max[1] },
                if corner & 4 == 0 { min[2] } else { max[2] },
            ];
            let Some((x, y)) = self.project(point) else {
                return Some([0, self.width, 0, self.height]);
            };
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        let left = (x0.floor() - 1.0).max(0.0) as usize;
        let top = (y0.floor() - 1.0).max(0.0) as usize;
        let right = ((x1.ceil() + 1.0).max(0.0) as usize).min(self.width);
        let bottom = ((y1.ceil() + 1.0).max(0.0) as usize).min(self.height);
        (left < right && top < bottom).then_some([left, right, top, bottom])
    }
}

/// Lighting and surface look for one frame.
#[derive(Debug, Clone)]
pub struct Look {
    pub shading: PipeShading,
    pub pattern: PipePattern,
    /// Unit vector from the surface towards the light.
    pub light: Vec3,
    /// Overall brightness multiplier (fade to black).
    pub gain: f32,
}

impl Look {
    /// Key light fixed relative to the camera (upper left, in front), so a
    /// pipe keeps a consistent gradient while the camera orbits.
    pub fn for_camera(
        camera: &Camera,
        shading: PipeShading,
        pattern: PipePattern,
        gain: f32,
    ) -> Self {
        let light = std::array::from_fn(|axis| {
            -0.55 * camera.right[axis] + 0.6 * camera.up[axis] - 0.75 * camera.forward[axis]
        });
        Self {
            shading,
            pattern,
            light: normalize(light),
            gain,
        }
    }
}

/// A straight axis-aligned cylinder.
#[derive(Debug, Clone, Copy)]
pub struct Cylinder {
    pub axis: usize,
    /// Coordinates on the two perpendicular axes, ascending.
    pub center: [f32; 2],
    pub lo: f32,
    pub hi: f32,
    pub radius: f32,
    /// Draw flat end caps (mitered pipes have no spheres to hide them).
    pub caps: bool,
    pub albedo: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
    pub albedo: f32,
}

#[derive(Default)]
pub struct Renderer {
    depth: Vec<f32>,
    ray_x: Vec<f32>,
    ray_y: Vec<f32>,
}

/// Everything about one surface hit that shading needs.
struct Hit {
    depth: f32,
    normal: Vec3,
    albedo: f32,
    /// Position along the pipe axis, for surface patterns.
    axial: f32,
    /// Angle around the pipe axis in radians, 0 for spheres.
    angle: f32,
    is_pipe: bool,
}

impl Renderer {
    /// Prepare per-frame tables; call before drawing primitives.
    pub fn begin(&mut self, camera: &Camera) {
        self.depth.clear();
        self.depth
            .resize(camera.width * camera.height, f32::INFINITY);
        self.ray_x.clear();
        self.ray_x.extend(
            (0..camera.width)
                .map(|x| ((x as f32 + 0.5) / camera.width as f32 * 2.0 - 1.0) * camera.tan_h),
        );
        self.ray_y.clear();
        self.ray_y.extend(
            (0..camera.height)
                .map(|y| (1.0 - (y as f32 + 0.5) / camera.height as f32 * 2.0) * camera.tan_v),
        );
    }

    fn direction(&self, camera: &Camera, x: usize, y: usize) -> Vec3 {
        let (rx, ry) = (self.ray_x[x], self.ray_y[y]);
        [
            camera.forward[0] + camera.right[0] * rx + camera.up[0] * ry,
            camera.forward[1] + camera.right[1] * rx + camera.up[1] * ry,
            camera.forward[2] + camera.right[2] * rx + camera.up[2] * ry,
        ]
    }

    pub fn cylinder(
        &mut self,
        camera: &Camera,
        look: &Look,
        raster: &mut Raster,
        cylinder: &Cylinder,
    ) {
        let (first, second) = perpendicular(cylinder.axis);
        let radius = cylinder.radius;
        let mut min = [0.0; 3];
        let mut max = [0.0; 3];
        min[cylinder.axis] = cylinder.lo;
        max[cylinder.axis] = cylinder.hi;
        min[first] = cylinder.center[0] - radius;
        max[first] = cylinder.center[0] + radius;
        min[second] = cylinder.center[1] - radius;
        max[second] = cylinder.center[1] + radius;
        let Some([x0, x1, y0, y1]) = camera.bounds(min, max) else {
            return;
        };
        let origin_first = camera.eye[first] - cylinder.center[0];
        let origin_second = camera.eye[second] - cylinder.center[1];
        let origin_axial = camera.eye[cylinder.axis];
        let radius_squared_offset =
            origin_first * origin_first + origin_second * origin_second - radius * radius;
        for y in y0..y1 {
            for x in x0..x1 {
                let ray = self.direction(camera, x, y);
                let (df, ds, da) = (ray[first], ray[second], ray[cylinder.axis]);
                let mut best = f32::INFINITY;
                let mut cap_sign = 0.0;
                let quad_a = df * df + ds * ds;
                if quad_a > 1e-12 {
                    let half_b = origin_first * df + origin_second * ds;
                    let discriminant = half_b * half_b - quad_a * radius_squared_offset;
                    if discriminant > 0.0 {
                        let t = (-half_b - discriminant.sqrt()) / quad_a;
                        let axial = origin_axial + t * da;
                        if t > 0.0 && axial >= cylinder.lo && axial <= cylinder.hi {
                            best = t;
                        }
                    }
                }
                if cylinder.caps && da.abs() > 1e-9 {
                    for (plane, sign) in [(cylinder.lo, -1.0), (cylinder.hi, 1.0)] {
                        let t = (plane - origin_axial) / da;
                        if t > 0.0 && t < best {
                            let pf = origin_first + t * df;
                            let ps = origin_second + t * ds;
                            if pf * pf + ps * ps <= radius * radius {
                                best = t;
                                cap_sign = sign;
                            }
                        }
                    }
                }
                let index = y * camera.width + x;
                if best >= self.depth[index] {
                    continue;
                }
                let mut normal = [0.0; 3];
                let mut angle = 0.0;
                if cap_sign != 0.0 {
                    normal[cylinder.axis] = cap_sign;
                } else {
                    let nf = (origin_first + best * df) / radius;
                    let ns = (origin_second + best * ds) / radius;
                    normal[first] = nf;
                    normal[second] = ns;
                    angle = ns.atan2(nf);
                }
                let hit = Hit {
                    depth: best,
                    normal,
                    albedo: cylinder.albedo,
                    axial: origin_axial + best * da,
                    angle,
                    is_pipe: true,
                };
                self.depth[index] = best;
                raster.dots[index] = shade(camera, look, &hit, ray);
            }
        }
    }

    pub fn sphere(&mut self, camera: &Camera, look: &Look, raster: &mut Raster, sphere: &Sphere) {
        let radius = sphere.radius;
        let min = sphere.center.map(|c| c - radius);
        let max = sphere.center.map(|c| c + radius);
        let Some([x0, x1, y0, y1]) = camera.bounds(min, max) else {
            return;
        };
        let offset = sub(camera.eye, sphere.center);
        let offset_squared = dot(offset, offset) - radius * radius;
        for y in y0..y1 {
            for x in x0..x1 {
                let ray = self.direction(camera, x, y);
                let quad_a = dot(ray, ray);
                let half_b = dot(offset, ray);
                let discriminant = half_b * half_b - quad_a * offset_squared;
                if discriminant <= 0.0 {
                    continue;
                }
                let t = (-half_b - discriminant.sqrt()) / quad_a;
                let index = y * camera.width + x;
                if t <= 0.0 || t >= self.depth[index] {
                    continue;
                }
                let normal = [
                    (offset[0] + t * ray[0]) / radius,
                    (offset[1] + t * ray[1]) / radius,
                    (offset[2] + t * ray[2]) / radius,
                ];
                let hit = Hit {
                    depth: t,
                    normal,
                    albedo: sphere.albedo,
                    axial: 0.0,
                    angle: 0.0,
                    is_pipe: false,
                };
                self.depth[index] = t;
                raster.dots[index] = shade(camera, look, &hit, ray);
            }
        }
    }
}

/// Multiplier for the surface pattern at a pipe hit (1.0 = untouched).
fn pattern_factor(pattern: PipePattern, axial: f32, angle: f32) -> f32 {
    match pattern {
        PipePattern::Plain => 1.0,
        PipePattern::Rings => {
            let phase = (axial * 2.0).rem_euclid(1.0);
            let band = smoothstep(0.02, 0.07, phase) * (1.0 - smoothstep(0.17, 0.22, phase));
            1.0 - 0.62 * band
        }
        PipePattern::Checker => {
            let sector = ((angle + PI) / TAU * 8.0).floor() as i32;
            let tile = (axial * 3.0).floor() as i32;
            if (sector + tile).rem_euclid(2) == 0 {
                1.0
            } else {
                0.48
            }
        }
    }
}

/// Final intensity 0..=1 of one hit.
fn shade(camera: &Camera, look: &Look, hit: &Hit, ray: Vec3) -> f32 {
    let inverse_length = 1.0 / dot(ray, ray).sqrt();
    let view = [
        -ray[0] * inverse_length,
        -ray[1] * inverse_length,
        -ray[2] * inverse_length,
    ];
    let normal = hit.normal;
    let facing = dot(normal, look.light);
    let view_facing = dot(normal, view).max(0.0);
    let half = normalize([
        look.light[0] + view[0],
        look.light[1] + view[1],
        look.light[2] + view[2],
    ]);
    let specular_angle = dot(normal, half).max(0.0);
    let pattern = if hit.is_pipe {
        pattern_factor(look.pattern, hit.axial, hit.angle)
    } else {
        1.0
    };
    let base = hit.albedo * pattern;
    let wrap = ((facing + 0.25) / 1.25).clamp(0.0, 1.0);
    let rim = (1.0 - view_facing).powi(3);
    let value = match look.shading {
        PipeShading::SoftLit => {
            let specular = specular_angle.powi(28);
            base * (0.07 + 0.93 * wrap.powf(0.85)) + 0.5 * specular + 0.16 * rim * base.max(0.4)
        }
        PipeShading::Flat => {
            let edge = smoothstep(0.03, 0.32, view_facing);
            (0.92 * base) * (0.25 + 0.75 * edge)
        }
        PipeShading::HighContrast => {
            let lit = smoothstep(0.12, 0.72, wrap);
            let specular = specular_angle.powi(48);
            base * (0.03 + 0.97 * lit) + 0.7 * specular
        }
    };
    let fog = 1.0 - 0.42 * ((hit.depth - camera.near_depth) / camera.depth_span).clamp(0.0, 1.0);
    // A gamma below one lifts the mid tones so the dither has more to work with.
    (value.clamp(0.0, 1.0).powf(0.78) * fog * look.gain).clamp(0.0, 1.0)
}
