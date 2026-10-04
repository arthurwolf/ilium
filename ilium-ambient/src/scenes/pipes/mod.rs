//! 3D pipes screensaver in dithered black and white.
//!
//! Pipes random-walk along a 3D integer grid (`sim`), are ray-cast as shaded
//! cylinders and spheres with perspective and a z-buffer at Braille dot
//! resolution (`render`), while the camera slowly orbits. When the volume is
//! full (or a timer expires) the picture fades out and a new layout grows.
//!
//! Determinism: the layout depends only on the seed, growth on the animation
//! clock (`Frame::time`) converted to fixed simulation ticks, so frame rate
//! never changes what is built. If the clock runs backwards the scene
//! restarts from scratch. Nothing here blocks or spawns threads; dropping the
//! scene releases plain memory only.
//!
//! No external data source is used.

mod render;
mod settings;
mod sim;

pub use settings::PipesSettings;

use settings::{JointStyle, ResetMode, SeedMode};

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use render::{Camera, CameraSpec, Cylinder, Look, Renderer, Sphere};
use sim::{Rng, Sim, SimParams};
use std::time::UNIX_EPOCH;

/// Seconds the finished structure stays before fading (reset when full).
const HOLD_SECONDS: f64 = 3.0;
/// Seconds of the fade to black before a new layout starts.
const FADE_SECONDS: f64 = 2.0;
/// Camera speed at 100% orbit, degrees per second.
const ORBIT_DEGREES_PER_SECOND: f32 = 20.0;
/// Simulation events one render call may process before giving up and
/// restarting (guards against absurd clock jumps).
const EVENT_BUDGET: u32 = 200_000;
/// Cycles one render call may skip through for the same reason.
const CYCLE_BUDGET: u32 = 64;

pub struct PipesScene {
    settings: PipesSettings,
    /// Seed material of the run; chosen at the first render in random mode.
    base_seed: Option<u64>,
    sim: Option<Sim>,
    cycle: u64,
    /// Animation time (seconds) at which the current cycle started.
    epoch: f64,
    last_time: Option<f64>,
    renderer: Renderer,
}

impl PipesScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: &PipesSettings, _env: &SceneEnv) -> Self {
        Self {
            settings: settings.normalized(),
            base_seed: None,
            sim: None,
            cycle: 0,
            epoch: 0.0,
            last_time: None,
            renderer: Renderer::default(),
        }
    }

    fn growth_rate(&self) -> f64 {
        f64::from(self.settings.growth_speed)
    }

    fn radius(&self) -> f32 {
        self.settings.pipe_thickness as f32 / 200.0
    }

    fn start_cycle(&mut self, epoch: f64) {
        let base = self.base_seed.unwrap_or(0);
        let seed = Rng::new(base ^ self.cycle.wrapping_mul(0xd1b5_4a32_d192_ed03)).next_u64();
        self.epoch = epoch;
        self.sim = Some(Sim::new(
            SimParams {
                grid: self.settings.volume_size as i32,
                pipe_count: self.settings.pipe_count as usize,
                turn_chance: f64::from(self.settings.turn_chance) / 100.0,
                radius: self.radius(),
                joint: self.settings.joint_style,
            },
            seed,
        ));
    }

    /// Forget everything and start cycle 0 at animation time `time`.
    fn restart_run(&mut self, time: f64) {
        self.cycle = 0;
        self.start_cycle(time);
    }

    /// Seconds into the cycle at which the fade starts, once known.
    fn fade_start(&self) -> Option<f64> {
        match self.settings.reset_mode {
            ResetMode::Timed => {
                Some((f64::from(self.settings.reset_seconds) - FADE_SECONDS).max(1.0))
            }
            ResetMode::WhenFull => self
                .sim
                .as_ref()?
                .full_tick()
                .map(|tick| tick / self.growth_rate() + HOLD_SECONDS),
        }
    }

    /// Bring the simulation up to animation time `time`, restarting cycles
    /// that have completed. Returns the seconds into the current cycle.
    fn update(&mut self, time: f64) -> f64 {
        let mut events = EVENT_BUDGET;
        if self.sim.is_none() {
            // The scene clock starts at zero, whenever the first frame comes.
            self.start_cycle(0.0);
        }
        for _ in 0..CYCLE_BUDGET {
            let elapsed = (time - self.epoch).max(0.0);
            let rate = self.growth_rate();
            let horizon = match (self.settings.reset_mode, self.fade_start()) {
                (ResetMode::Timed, Some(start)) => elapsed.min(start + FADE_SECONDS),
                _ => elapsed,
            };
            let Some(sim) = self.sim.as_mut() else {
                return 0.0;
            };
            if !sim.advance_to(horizon * rate, &mut events) {
                break;
            }
            match self.fade_start() {
                Some(start) if elapsed >= start + FADE_SECONDS => {
                    self.cycle += 1;
                    self.start_cycle(self.epoch + start + FADE_SECONDS);
                }
                _ => return elapsed,
            }
        }
        // Clock jump beyond any sensible catch-up: begin afresh now.
        self.cycle += 1;
        self.start_cycle(time);
        0.0
    }

    /// Camera for animation time `time` (absolute, so restarts do not jerk it).
    fn camera(&self, time: f64, width: usize, height: usize) -> Camera {
        let grid = self.settings.volume_size as f32;
        let half_extent = (grid - 1.0) * 0.5 + 0.5;
        let center = (grid - 1.0) * 0.5;
        let angle = (f64::from(self.settings.orbit_speed) / 100.0
            * f64::from(ORBIT_DEGREES_PER_SECOND)
            * time)
            .to_radians() as f32;
        let bounding_radius = 3.0f32.sqrt() * half_extent;
        let drift = 0.05 * bounding_radius;
        Camera::orbit(&CameraSpec {
            target: [
                center + drift * (0.31 * angle).sin(),
                center + drift * (0.23 * angle + 1.0).sin(),
                center + drift * (0.27 * angle + 2.0).sin(),
            ],
            bounding_radius,
            fov_degrees: self.settings.field_of_view as f32,
            yaw: 0.6 + angle,
            pitch: 0.42 + 0.28 * (0.31 * angle + 1.0).sin(),
            zoom_scale: 1.0 + 0.05 * (0.53 * angle).sin(),
            width,
            height,
        })
    }

    fn draw(&mut self, camera: &Camera, gain: f32, tick: f64, frame: &mut Frame<'_>) {
        let Some(sim) = self.sim.as_ref() else {
            return;
        };
        let look = Look::for_camera(camera, self.settings.shading, self.settings.pattern, gain);
        let radius = self.radius();
        let caps = self.settings.joint_style == JointStyle::Bare;
        let renderer = &mut self.renderer;
        renderer.begin(camera);
        let raster = &mut *frame.raster;
        let cylinder_of = |seg: &sim::Seg| Cylinder {
            axis: seg.axis,
            center: seg.center,
            lo: seg.lo,
            hi: seg.hi,
            radius,
            caps,
            albedo: seg.albedo,
        };
        for seg in sim.segs() {
            renderer.cylinder(camera, &look, raster, &cylinder_of(seg));
        }
        for ball in sim.balls() {
            renderer.sphere(
                camera,
                &look,
                raster,
                &Sphere {
                    center: ball.center,
                    radius: ball.radius,
                    albedo: ball.albedo,
                },
            );
        }
        let cap_radius = sim.cap_radius();
        for head in sim.heads(tick) {
            renderer.cylinder(camera, &look, raster, &cylinder_of(&head.seg));
            if cap_radius > 0.0 {
                renderer.sphere(
                    camera,
                    &look,
                    raster,
                    &Sphere {
                        center: head.tip,
                        radius: cap_radius,
                        albedo: head.seg.albedo,
                    },
                );
            }
        }
    }
}

impl Scene for PipesScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        let time = frame.time.as_secs_f64();
        if self.base_seed.is_none() {
            self.base_seed = Some(match self.settings.seed_mode {
                SeedMode::Fixed => u64::from(self.settings.seed),
                SeedMode::Random => frame
                    .now
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |since| since.as_nanos() as u64),
            });
        }
        if self.last_time.is_some_and(|last| time < last) {
            self.restart_run(time);
        }
        self.last_time = Some(time);

        let elapsed = self.update(time);
        let gain = match self.fade_start() {
            Some(start) if elapsed > start => {
                1.0 - smoothstep_f64((elapsed - start) / FADE_SECONDS) as f32
            }
            _ => 1.0,
        };
        let camera = self.camera(time, width, height);
        let tick = elapsed * self.growth_rate();
        self.draw(&camera, gain, tick, frame);
    }

    fn frames_per_second(&self) -> u32 {
        15
    }
}

fn smoothstep_f64(fraction: f64) -> f64 {
    let fraction = fraction.clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

#[cfg(test)]
mod tests;
