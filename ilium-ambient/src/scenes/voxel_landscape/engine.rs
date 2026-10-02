//! One owned low-priority generation worker, with one request/result slot.
//! Rendering never waits for the worker; camera motion uses absolute time.
use super::{
    chunks::ColumnCache,
    color,
    generation::{self, PreparedWorld, Region},
    render::{self, Canvas},
    settings::VoxelLandscapeSettings,
};
use crate::{
    control::SceneSettings,
    scene::{Frame, Scene, SceneEnv},
    source::Worker,
};
use std::sync::{
    atomic::Ordering,
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
use std::time::Duration;

pub struct VoxelLandscapeScene {
    settings: VoxelLandscapeSettings,
    requests: SyncSender<Region>,
    results: Arc<Mutex<Option<PreparedWorld>>>,
    worker: Option<Worker>,
    requested: Option<Region>,
    prepared: Option<PreparedWorld>,
    error: Option<String>,
}
impl VoxelLandscapeScene {
    pub fn new(settings: &VoxelLandscapeSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let (requests, receiver) = mpsc::sync_channel::<Region>(1);
        let results = Arc::new(Mutex::new(None));
        let worker_results = Arc::clone(&results);
        let worker_settings = settings.clone();
        let result = Worker::try_spawn("voxel-world", move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::BelowNormal,
            );
            let mut cache = ColumnCache::new(256);
            while !stop.load(Ordering::Relaxed) {
                let mut region = match receiver.recv_timeout(Duration::from_millis(25)) {
                    Ok(region) => region,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                };
                while let Ok(latest) = receiver.try_recv() {
                    region = latest;
                }
                let Some(world) = generation::prepare(region, &worker_settings, &mut cache, || {
                    stop.load(Ordering::Relaxed)
                }) else {
                    continue;
                };
                // A slow/hidden host cannot accumulate completed mesh history.
                let superseded = match worker_results.lock() {
                    Ok(mut slot) => slot.replace(world),
                    Err(_) => return,
                };
                // Drop stale mesh bytes after releasing the handoff lock.
                drop(superseded);
            }
        });
        let (worker, error) = match result {
            Ok(worker) => (Some(worker), None),
            Err(error) => (None, Some(format!("Could not prepare landscape: {error}"))),
        };
        Self {
            settings,
            requests,
            results,
            worker,
            requested: None,
            prepared: None,
            error,
        }
    }
    pub fn camera(settings: &VoxelLandscapeSettings, time: Duration) -> [f64; 3] {
        let distance = time.as_secs_f64() * f64::from(settings.pan_speed_percent) / 100.0 * 0.25;
        let direction = match settings.pan_direction {
            1 => [0., 1.],
            2 => [
                std::f64::consts::FRAC_1_SQRT_2,
                -std::f64::consts::FRAC_1_SQRT_2,
            ],
            3 => [
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
            _ => [1., 0.],
        };
        // Clamped only at the kernel's finite integer domain, centuries away.
        [
            (64. + direction[0] * distance)
                .clamp(f64::from(i32::MIN + 2048), f64::from(i32::MAX - 2048)),
            (64. + direction[1] * distance)
                .clamp(f64::from(i32::MIN + 2048), f64::from(i32::MAX - 2048)),
            26.,
        ]
    }
    pub fn scale(settings: &VoxelLandscapeSettings) -> f32 {
        2.8 * settings.zoom_percent as f32 / 100.0
    }
    pub fn region(camera: [f64; 3], scale: f32, size: [usize; 2]) -> Region {
        let radius = (size[0] as f32 / (3.4641016 * scale) + size[1] as f32 / (2. * scale) + 70.)
            .ceil()
            .clamp(32., 480.) as i32;
        let center = [camera[0].floor() as i32, camera[1].floor() as i32]
            .map(|coordinate| coordinate.div_euclid(16) * 16);
        let radius = (radius + 15) / 16 * 16;
        Region {
            minimum: center.map(|coordinate| coordinate - radius),
            maximum: center.map(|coordinate| coordinate + radius + 16),
        }
    }
    pub fn prepared(&self) -> Option<&PreparedWorld> {
        self.prepared.as_ref()
    }
}
impl Scene for VoxelLandscapeScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if let Ok(mut slot) = self.results.try_lock() {
            if let Some(prepared) = slot.take() {
                self.prepared = Some(prepared);
            }
        }
        let camera = Self::camera(&self.settings, frame.time);
        let scale = Self::scale(&self.settings);
        let region = Self::region(camera, scale, [frame.raster.width, frame.raster.height]);
        if self.requested != Some(region) && self.worker.is_some() {
            match self.requests.try_send(region) {
                Ok(()) => self.requested = Some(region),
                Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.error = Some("Landscape worker stopped".into())
                }
            }
        }
        let Some(world) = &self.prepared else {
            frame.cell_colors.fill([0; 3]);
            return;
        };
        render_prepared(world, frame, &self.settings, camera);
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn status(&self) -> Option<String> {
        self.error.clone().or_else(|| {
            self.prepared
                .is_none()
                .then(|| "Generating surface landscape…".into())
        })
    }
}
impl Drop for VoxelLandscapeScene {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

/// The production color/dither input pipeline, also used by capture probes.
pub fn render_prepared(
    world: &PreparedWorld,
    frame: &mut Frame<'_>,
    settings: &VoxelLandscapeSettings,
    camera: [f64; 3],
) -> Canvas {
    let canvas = render::draw_world(
        world,
        camera,
        VoxelLandscapeScene::scale(settings),
        [frame.raster.width, frame.raster.height],
        u64::from(settings.seed),
    );
    color::composite(&canvas.colors, frame, settings);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_does_not_wait_for_a_worker_handoff_lock() {
        let mut scene = VoxelLandscapeScene::new(
            &VoxelLandscapeSettings::default(),
            &SceneEnv::for_test(std::env::temp_dir()),
        );
        let results = Arc::clone(&scene.results);
        let _guard = results.lock().unwrap();
        let mut raster = crate::raster::Raster::default();
        raster.resize(16, 16);
        let mut colors = vec![[255; 3]; 32];
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 8,
            height: 4,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: std::time::SystemTime::UNIX_EPOCH,
        });
        assert!(colors.iter().all(|color| *color == [0; 3]));
        assert!(scene.prepared().is_none());
    }

    #[test]
    fn owned_worker_publishes_a_real_world_and_scene_renders_it() {
        let mut scene = VoxelLandscapeScene::new(
            &VoxelLandscapeSettings::default(),
            &SceneEnv::for_test(std::env::temp_dir()),
        );
        assert!(scene.error.is_none());
        scene
            .requests
            .try_send(Region {
                minimum: [48, 48],
                maximum: [80, 80],
            })
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if scene.results.lock().unwrap().is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "owned generation worker did not publish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut raster = crate::raster::Raster::default();
        raster.resize(64, 64);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 32,
            height: 16,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: std::time::SystemTime::UNIX_EPOCH,
        });
        assert!(scene.prepared().unwrap().blocks.len() >= 1000);
        assert!(raster.dots.iter().any(|dot| *dot > 0.2));
        assert!(colors.iter().any(|color| color[0] != color[1]));
        assert!(scene.status().is_none());
    }

    #[test]
    fn camera_does_not_jump_at_chunk_boundaries_and_zero_speed_freezes() {
        let settings = VoxelLandscapeSettings::default();
        let before = VoxelLandscapeScene::camera(&settings, Duration::from_secs_f64(255.999));
        let after = VoxelLandscapeScene::camera(&settings, Duration::from_secs_f64(256.001));
        assert!((after[0] - before[0]).abs() < 0.001);
        let frozen = VoxelLandscapeSettings {
            pan_speed_percent: 0,
            ..settings
        };
        assert_eq!(
            VoxelLandscapeScene::camera(&frozen, Duration::ZERO),
            VoxelLandscapeScene::camera(&frozen, Duration::from_secs(10000))
        );
    }
}
