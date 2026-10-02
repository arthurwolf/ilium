//! Parallel isometric hatching lifted by hidden spheres and capsules.
//! Simulation state belongs to this scene; the host owns input, look and persistence.
mod chess;
pub mod model;
pub mod render;
mod settings;
mod simulations;
mod snake_planner;

use crate::{Frame, Scene, SceneEnv, SceneSettings};
use chess::{CarpetChess, ChessOptions};
use model::{Body, Mode};
use render::{Camera, RenderOptions, Renderer};
pub use settings::CarpetSettings;
use simulations::{SimulationOptions, Simulations};
use std::time::UNIX_EPOCH;

// Simulations express lift in ground units. The largest configurable chess
// piece is 250% of the shared 0.075-unit base; use that stable scene bound for
// camera headroom rather than reserving an entire ground unit of empty sky.
const MAX_BODY_HEIGHT: f32 = 0.075 * 2.5;

pub struct CarpetScene {
    settings: CarpetSettings,
    simulation: Simulations,
    chess: Option<CarpetChess>,
    bodies: Vec<Body>,
    pointer: Option<[f32; 2]>,
    last_wall: Option<f64>,
    simulation_time: f64,
    dimensions: [usize; 2],
    renderer: Renderer,
}

impl CarpetScene {
    pub fn new(settings: &CarpetSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            simulation: Simulations::new(settings.seed as u64),
            settings,
            chess: None,
            bodies: Vec::with_capacity(1024),
            pointer: None,
            last_wall: None,
            simulation_time: 0.0,
            dimensions: [0, 0],
            renderer: Renderer::default(),
        }
    }

    fn simulation_options(&self) -> SimulationOptions {
        let s = &self.settings;
        SimulationOptions {
            hunters_count: s.hunters_count as u32,
            hunters_speed: s.hunters_speed as f32 / 100.0,
            hunters_separation: s.hunters_separation as f32 / 1000.0,
            snake_grid: s.snake_grid as u32,
            snake_step_seconds: f64::from(s.snake_step_ms) / 1000.0,
            snake_initial_length: s.snake_initial_length as u32,
            snake_food_count: s.snake_food_count as u32,
            life_grid: s.life_grid as u32,
            life_generation_seconds: f64::from(s.life_generation_ms) / 1000.0,
            life_density: s.life_density as f32 / 100.0,
            life_wrap: s.life_wrap,
            dvd_speed: s.dvd_speed as f32 / 100.0,
            orbit_speed: s.orbit_speed as f32 / 100.0 * 0.35,
            orbit_scale: s.orbit_scale as f32 / 100.0,
            clock_seconds: s.clock_seconds,
            clock_24h: s.clock_24h,
            clock_tubes: s.clock_tubes,
            radius: s.radius as f32 / 1000.0,
            height: 0.075,
            easing_seconds: f64::from(s.easing_ms) / 1000.0,
        }
    }

    fn chess_options(&self) -> ChessOptions {
        let s = &self.settings;
        ChessOptions {
            move_interval: f64::from(s.chess_move_ms) / 1000.0,
            easing_duration: f64::from(s.chess_easing_ms) / 1000.0,
            radius: s.radius as f32 / 1000.0,
            heights: [
                s.pawn_height,
                s.knight_height,
                s.bishop_height,
                s.rook_height,
                s.queen_height,
                s.king_height,
            ]
            .map(|height| height as f32 * 0.075 / 100.0),
            ai_depth: s.chess_ai_depth as u8,
            node_budget: s.chess_node_budget as usize,
            restart_delay: f64::from(s.chess_restart_ms) / 1000.0,
            max_plies: s.chess_max_plies as u16,
        }
    }

    fn render_options(&self) -> RenderOptions {
        let s = &self.settings;
        RenderOptions {
            yaw: s.yaw as f32,
            pitch: s.pitch as f32,
            zoom: s.zoom as f32 / 100.0,
            hatch_direction: s.hatch_direction as f32,
            spacing_in_dots: s.spacing as f32,
            line_width: s.line_width as f32 / 100.0,
            height_scale: MAX_BODY_HEIGHT * s.height as f32 / 100.0,
            softness: s.softness as f32 / 100.0,
        }
    }
}

impl Scene for CarpetScene {
    fn pointer(&mut self, position: Option<[f32; 2]>) {
        self.pointer = position;
    }

    fn reconfigure(&mut self, settings: &crate::AmbientSettings) -> bool {
        let next = settings.carpet.normalized();
        if next.seed != self.settings.seed {
            return false;
        }
        if next.mode != self.settings.mode {
            self.chess = None;
            self.simulation.deactivate();
        }
        self.settings = next;
        true
    }

    fn frames_per_second(&self) -> u32 {
        self.settings.fps as u32
    }

    fn status(&self) -> Option<String> {
        if matches!(self.settings.mode(), Mode::AutoChess | Mode::LiveChess) {
            self.chess.as_ref().and_then(CarpetChess::status)
        } else if matches!(self.settings.mode(), Mode::DigitalClock | Mode::AnalogClock) {
            Some(format!(
                "Civil time UTC{}{:02}:{:02}; independent of playback speed",
                if self.settings.utc_offset_minutes < 0 {
                    "-"
                } else {
                    "+"
                },
                self.settings.utc_offset_minutes.abs() / 60,
                self.settings.utc_offset_minutes.abs() % 60
            ))
        } else {
            None
        }
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        self.dimensions = [frame.raster.width, frame.raster.height];
        let wall = frame.wall.as_secs_f64();
        let delta = self
            .last_wall
            .map_or(0.0, |previous| (wall - previous).clamp(0.0, 2.0));
        // Retain the high-water mark so a backward wall-clock correction cannot
        // replay simulation steps while the clock catches up.
        self.last_wall = Some(self.last_wall.map_or(wall, |previous| previous.max(wall)));
        // Integrate the current speed over wall-time differences. Multiplying absolute
        // time by a newly edited speed would jump a game forward or backward.
        let global_speed = if wall > 0.0 {
            frame.time.as_secs_f64() / wall
        } else {
            1.0
        };
        self.simulation_time +=
            delta * global_speed * f64::from(self.settings.simulation_speed) / 100.0;
        let unix_seconds = frame.now.duration_since(UNIX_EPOCH).map_or_else(
            |before| -before.duration().as_secs_f64(),
            |after| after.as_secs_f64(),
        );
        let civil_seconds = unix_seconds + f64::from(self.settings.utc_offset_minutes) * 60.0;
        self.bodies.clear();
        let render_options = self.render_options();
        let ground_pointer = self.pointer.and_then(|pointer| {
            Camera::new(self.dimensions[0], self.dimensions[1], &render_options)
                .and_then(|camera| camera.inverse_ground(pointer.map(f64::from)))
        });
        let mode = self.settings.mode();
        if matches!(mode, Mode::AutoChess | Mode::LiveChess) {
            let options = self.chess_options();
            let chess = self
                .chess
                .get_or_insert_with(|| CarpetChess::new(self.settings.seed as u64));
            chess.update(
                mode == Mode::LiveChess,
                &options,
                self.simulation_time,
                unix_seconds,
                &mut self.bodies,
            );
        } else {
            self.chess = None;
            let options = self.simulation_options();
            self.simulation.update(
                mode,
                &options,
                self.simulation_time,
                civil_seconds,
                ground_pointer,
                &mut self.bodies,
            );
        }
        for body in &mut self.bodies {
            body.height /= MAX_BODY_HEIGHT;
        }
        self.renderer
            .render(frame.raster, &self.bodies, &render_options);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample(scene: &mut CarpetScene, wall: f64, speed: f64, civil: f64) {
        let mut raster = crate::Raster::default();
        raster.resize(160, 96);
        let mut colors = vec![[0; 3]; 80 * 24];
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 80,
            height: 24,
            wall: Duration::from_secs_f64(wall),
            time: Duration::from_secs_f64(wall * speed),
            now: UNIX_EPOCH + Duration::from_secs_f64(civil),
        });
    }

    fn scene(mode: i32) -> CarpetScene {
        CarpetScene::new(
            &CarpetSettings {
                mode,
                ..Default::default()
            },
            &SceneEnv::for_test(std::path::PathBuf::new()),
        )
    }

    #[test]
    fn editing_speed_integrates_forward_without_absolute_time_jumps() {
        let mut scene = scene(5);
        sample(&mut scene, 0.0, 1.0, 1000.0);
        sample(&mut scene, 100.0, 1.0, 1100.0);
        assert_eq!(scene.simulation_time, 2.0, "suspension is bounded");
        sample(&mut scene, 100.05, 1.0, 1100.05);
        sample(&mut scene, 100.10, 2.0, 1100.10);
        assert!(
            (scene.simulation_time - 2.15).abs() < 1e-9,
            "{}",
            scene.simulation_time
        );
        sample(&mut scene, 50.0, 1.0, 1100.10);
        assert!(
            (scene.simulation_time - 2.15).abs() < 1e-9,
            "backward wall time cannot replay moves"
        );
        sample(&mut scene, 50.1, 1.0, 1100.20);
        assert!(
            (scene.simulation_time - 2.15).abs() < 1e-9,
            "backward recovery below high-water mark cannot replay moves"
        );
        sample(&mut scene, 100.15, 1.0, 1100.25);
        assert!(
            (scene.simulation_time - 2.20).abs() < 1e-9,
            "recovery continues from high-water mark"
        );
    }

    #[test]
    fn real_clocks_are_independent_of_animation_speed_and_clock_changes_apply() {
        for mode in [7, 8] {
            let mut slow = scene(mode);
            let mut fast = scene(mode);
            sample(&mut slow, 100.0, 0.25, 45_678.25);
            sample(&mut fast, 100.0, 4.0, 45_678.25);
            let signature = |bodies: &[Body]| {
                bodies
                    .iter()
                    .map(|body| (body.from, body.to, body.radius, body.height))
                    .collect::<Vec<_>>()
            };
            assert_eq!(signature(&slow.bodies), signature(&fast.bodies));
            let before = signature(&slow.bodies);
            sample(&mut slow, 100.1, 0.25, 45_679.75);
            assert_ne!(before, signature(&slow.bodies));
        }
    }

    #[test]
    fn presentation_reconfiguration_preserves_game_and_seed_requests_restart() {
        let mut scene = scene(1);
        sample(&mut scene, 0.0, 1.0, 0.0);
        sample(&mut scene, 0.4, 1.0, 0.4);
        let clock = scene.simulation_time;
        let mut settings = crate::AmbientSettings::default();
        settings.carpet = scene.settings.clone();
        settings.carpet.spacing = 9;
        settings.carpet.yaw = 70;
        assert!(scene.reconfigure(&settings));
        assert_eq!(scene.simulation_time, clock);
        assert_eq!(scene.settings.spacing, 9);
        settings.carpet.seed += 1;
        assert!(!scene.reconfigure(&settings));
    }

    #[test]
    fn returning_from_chess_starts_a_new_snake_game() {
        let mut resumed = scene(1);
        sample(&mut resumed, 0.0, 1.0, 0.0);
        sample(&mut resumed, 0.4, 1.0, 0.4);
        let advanced_head = resumed.bodies[0].from;

        let mut settings = crate::AmbientSettings::default();
        settings.carpet = resumed.settings.clone();
        settings.carpet.mode = 3;
        assert!(resumed.reconfigure(&settings));
        settings.carpet.mode = 1;
        assert!(resumed.reconfigure(&settings));
        sample(&mut resumed, 0.4, 1.0, 0.4);

        let mut fresh = scene(1);
        sample(&mut fresh, 0.4, 1.0, 0.4);
        assert_ne!(advanced_head, fresh.bodies[0].from);
        assert_eq!(resumed.bodies[0].from, fresh.bodies[0].from);
        assert_eq!(resumed.bodies[0].to, fresh.bodies[0].to);
    }

    #[test]
    fn hunters_follow_ground_targets_through_the_actual_screen_pointer_bridge() {
        let mut final_positions = Vec::new();
        for target in [[0.25, 0.25], [0.75, 0.75]] {
            let settings = CarpetSettings {
                mode: 0, // Hunters remains explicit when the preferred default mode changes.
                hunters_count: 1,
                ..Default::default()
            };
            let mut scene =
                CarpetScene::new(&settings, &SceneEnv::for_test(std::path::PathBuf::new()));
            let camera = Camera::new(160, 96, &scene.render_options()).unwrap();
            scene.pointer(Some(camera.project(target, 0.0).unwrap().map(|v| v as f32)));
            sample(&mut scene, 0.0, 1.0, 0.0);
            let distance = |p: [f32; 2]| (p[0] - target[0]).hypot(p[1] - target[1]);
            let initial = distance(scene.bodies[0].from);
            for frame in 1..=160 {
                sample(&mut scene, f64::from(frame) / 20.0, 1.0, 0.0);
            }
            let position = scene.bodies[0].from;
            assert!(
                distance(position) < initial * 0.5,
                "hunter must pursue the projected target"
            );
            assert!(distance(position) < 0.12);
            final_positions.push(position);
        }
        assert!((final_positions[0][0] - final_positions[1][0]).abs() > 0.3);
    }

    #[test]
    fn largest_derived_objects_and_lift_control_do_not_saturate_silently() {
        for (mode, multiplier) in [(5, 1.6), (6, 1.5)] {
            let settings = CarpetSettings {
                mode,
                radius: 120,
                height: 250,
                ..Default::default()
            };
            let mut scene =
                CarpetScene::new(&settings, &SceneEnv::for_test(std::path::PathBuf::new()));
            sample(&mut scene, 0.0, 1.0, 0.0);
            let body = scene.bodies[0];
            assert!((body.radius - 0.12 * multiplier).abs() < 1e-6);
            assert_eq!(body.normalized().unwrap().radius, body.radius);
            assert_eq!(
                scene.render_options().normalized().height_scale / MAX_BODY_HEIGHT,
                2.5
            );
        }
    }

    #[test]
    fn civil_clock_status_preserves_negative_subhour_offset() {
        let mut settings = CarpetSettings {
            mode: 8,
            ..Default::default()
        };
        settings.utc_offset_minutes = -30;
        let scene = CarpetScene::new(&settings, &SceneEnv::for_test(std::path::PathBuf::new()));
        assert!(scene.status().unwrap().contains("UTC-00:30"));
    }
}
