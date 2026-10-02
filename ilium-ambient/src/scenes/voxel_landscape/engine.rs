//! One owned low-priority pack/world/model worker. Frames only raster immutable
//! bank-matched geometry; pan speed never controls animation time or world seed.
use super::{
    assets::{
        budget::Cancel,
        error::{AssetError, Result},
    },
    color,
    generation::{PreparedWorld, Region as LegacyRegion},
    render::{self, Canvas},
    settings::VoxelLandscapeSettings,
    surface_binding::{self, PreparedSurface},
    surface_context::SceneAtmosphere,
    surface_entity_raster,
    surface_generation::Region,
    surface_raster::{self, RasterFrame, RasterLimits},
    surface_retirement::Retirement,
    terrain_fields::TerrainFields,
};
use crate::{
    control::SceneSettings,
    scene::{Frame, Scene, SceneEnv},
    source::Worker,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone, Copy)]
struct Request {
    revision: u64,
    region: Region,
}
struct Response {
    revision: u64,
    result: Result<PreparedSurface>,
}

pub struct VoxelLandscapeScene {
    settings: VoxelLandscapeSettings,
    requests: Arc<Mutex<Option<Request>>>,
    revision: Arc<AtomicU64>,
    results: Arc<Mutex<Option<Response>>>,
    worker: Option<Worker>,
    requested: Option<Region>,
    prepared: Option<PreparedSurface>,
    retired: Arc<Retirement<PreparedSurface>>,
    error: Option<String>,
}
impl VoxelLandscapeScene {
    pub fn new(settings: &VoxelLandscapeSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let requests = Arc::new(Mutex::new(None::<Request>));
        let revision = Arc::new(AtomicU64::new(0));
        let results = Arc::new(Mutex::new(None::<Response>));
        let worker_requests = Arc::clone(&requests);
        let worker_revision = Arc::clone(&revision);
        let worker_results = Arc::clone(&results);
        let worker_settings = settings.clone();
        let cache_dir = env.cache_dir.clone();
        let retired = Arc::new(Retirement::new());
        let worker_retired = Arc::clone(&retired);
        let worker = Worker::try_spawn("voxel-surface-pack", move |stop| {
            ilium_platform::thread_priority::lower_current_thread(
                ilium_platform::thread_priority::WorkerPriority::BelowNormal,
            );
            while !stop.load(Ordering::Relaxed) {
                drop(worker_retired.drain());
                let request = match worker_requests.lock() {
                    Ok(mut slot) => slot.take(),
                    Err(poisoned) => poisoned.into_inner().take(),
                };
                let Some(request) = request else {
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                };
                let cancel = Cancel::for_revision(&stop, &worker_revision, request.revision);
                let result =
                    super::pack_registry::resolve_registered(&worker_settings, &cache_dir, cancel)
                        .and_then(|resolved| {
                            let fallback = if resolved.pack_profile == 2 {
                                None
                            } else {
                                let candidate = VoxelLandscapeSettings {
                                    pack_profile: 2,
                                    ..Default::default()
                                };
                                match super::pack_registry::resolve_registered(
                                    &candidate, &cache_dir, cancel,
                                ) {
                                    Ok(settings) => Some(settings),
                                    Err(AssetError::Cancelled) => {
                                        return Err(AssetError::Cancelled)
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            "reviewed fauna fallback unavailable: {error}"
                                        );
                                        None
                                    }
                                }
                            };
                            surface_binding::prepare_with_fallback(
                                request.region,
                                &resolved,
                                fallback.as_ref(),
                                cancel,
                            )
                        });
                if cancel.is_cancelled() {
                    continue;
                }
                let response = Response {
                    revision: request.revision,
                    result,
                };
                let replaced = match worker_results.lock() {
                    Ok(mut slot) => slot.replace(response),
                    Err(poisoned) => poisoned.into_inner().replace(response),
                };
                drop(replaced);
            }
            // Scene teardown transfers its last snapshot before signalling stop.
            // The worker retains these mailbox Arcs until all heavy data are gone.
            drop(worker_retired.drain());
            let pending = worker_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            drop(pending);
        });
        let (worker, error) = match worker {
            Ok(worker) => (Some(worker), None),
            Err(error) => (
                None,
                Some(format!("Could not start surface preparation: {error}")),
            ),
        };
        Self {
            settings,
            requests,
            revision,
            results,
            worker,
            requested: None,
            prepared: None,
            retired,
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
        let x = (64. + direction[0] * distance)
            .clamp(f64::from(i32::MIN + 2048), f64::from(i32::MAX - 2048));
        let y = (64. + direction[1] * distance)
            .clamp(f64::from(i32::MIN + 2048), f64::from(i32::MAX - 2048));
        let ground = TerrainFields::new(u64::from(settings.seed))
            .sample(x.floor() as i32, y.floor() as i32, settings.rivers)
            .height;
        [x, y, f64::from(ground) + 7.]
    }
    pub fn scale(settings: &VoxelLandscapeSettings) -> f32 {
        2.8 * settings.zoom_percent as f32 / 100.0
    }
    /// Retained only for the old kernel's regression probes; the active scene
    /// requests `surface_region` and never prepares procedural tile materials.
    pub fn region(camera: [f64; 3], scale: f32, size: [usize; 2]) -> LegacyRegion {
        let radius = (size[0] as f32 / (3.4641016 * scale) + size[1] as f32 / (2. * scale) + 70.)
            .ceil()
            .clamp(32., 480.) as i32;
        let center = [camera[0].floor() as i32, camera[1].floor() as i32]
            .map(|coordinate| coordinate.div_euclid(16) * 16);
        let radius = (radius + 15) / 16 * 16;
        LegacyRegion {
            minimum: center.map(|coordinate| coordinate - radius),
            maximum: center.map(|coordinate| coordinate + radius + 16),
        }
    }
    pub fn surface_region(camera: [f64; 3], scale: f32, size: [usize; 2]) -> Region {
        let center = [camera[0].floor() as i32, camera[1].floor() as i32]
            .map(|coordinate| coordinate.div_euclid(16) * 16);
        let wanted = (size[0] as f32 / (1.7320508 * scale))
            .max(size[1] as f32 / scale)
            .ceil() as i32
            + 32;
        let width = ((wanted + 15) / 16 * 16).clamp(32, 96);
        let start = center.map(|coordinate| coordinate - width / 2);
        Region {
            minimum: start,
            maximum: start.map(|coordinate| coordinate + width),
        }
    }
    pub fn prepared(&self) -> Option<&PreparedSurface> {
        self.prepared.as_ref()
    }
    fn receive_prepared(&mut self) {
        if let Ok(mut slot) = self.results.try_lock() {
            if let Some(response) = slot.take() {
                if response.revision == self.revision.load(Ordering::Acquire) {
                    match response.result {
                        Ok(prepared) => {
                            if let Some(previous) = self.prepared.take() {
                                if let Err(previous) = self.retired.try_retire(previous) {
                                    self.prepared = Some(previous);
                                    *slot = Some(Response {
                                        revision: response.revision,
                                        result: Ok(prepared),
                                    });
                                    return;
                                }
                            }
                            self.error = None;
                            self.prepared = Some(prepared);
                        }
                        Err(AssetError::Cancelled) => {}
                        Err(error) => {
                            self.error = Some(format!("Surface preparation: {error}"));
                        }
                    }
                } else if let Ok(prepared) = response.result {
                    if let Err(prepared) = self.retired.try_retire(prepared) {
                        *slot = Some(Response {
                            revision: response.revision,
                            result: Ok(prepared),
                        });
                    }
                }
            }
        }
    }
    fn request(&mut self, region: Region) {
        if self.requested == Some(region) || self.worker.is_none() {
            return;
        }
        let Ok(mut slot) = self.requests.try_lock() else {
            return;
        };
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        *slot = Some(Request { revision, region });
        self.requested = Some(region);
    }
}
impl Scene for VoxelLandscapeScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.receive_prepared();
        let camera = Self::camera(&self.settings, frame.time);
        let scale = Self::scale(&self.settings);
        let region = Self::surface_region(camera, scale, [frame.raster.width, frame.raster.height]);
        self.request(region);
        let Some(prepared) = self.prepared.as_ref() else {
            frame.cell_colors.fill([0; 3]);
            frame.raster.dots.fill(0.);
            return;
        };
        if let Err(error) = render_surface(prepared, frame, &self.settings, camera) {
            self.error = Some(format!("Surface raster: {error}"));
            frame.cell_colors.fill([0; 3]);
            frame.raster.dots.fill(0.);
        }
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn status(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        let Some(prepared) = self.prepared.as_ref() else {
            return Some("Preparing selected full pack and surface…".into());
        };
        let (found, required) = prepared.material_coverage();
        let fauna_gaps = prepared.entities.gaps.len();
        let fallback_atlases = prepared
            .entities
            .atlases
            .iter()
            .filter(|atlas| atlas.used_fallback)
            .count();
        if found < required
            || !prepared.skipped_states.is_empty()
            || !prepared.compatibility_aliases.is_empty()
            || !prepared.model_substitutions.is_empty()
            || fauna_gaps > 0
            || fallback_atlases > 0
            || !prepared.material_fallbacks.is_empty()
        {
            Some(format!("Surface art {found}/{required}; {} texture aliases; {} model substitutions; {} state gaps; {} textured fauna candidates; {} fauna gaps; {} fauna and {} material images from reviewed full-pack fallback (not selected-native)",
                    prepared.compatibility_aliases.len(),prepared.model_substitutions.len(),
                    prepared.skipped_states.len(),prepared.entities.rendered_entities,
                    fauna_gaps,fallback_atlases,prepared.material_fallbacks.len()))
        } else {
            None
        }
    }
}
impl Drop for VoxelLandscapeScene {
    fn drop(&mut self) {
        if let Some(prepared) = self.prepared.take() {
            self.retired.retire_final(prepared);
        }
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}

static RASTER_STOP: AtomicBool = AtomicBool::new(false);
fn render_surface(
    prepared: &PreparedSurface,
    frame: &mut Frame<'_>,
    settings: &VoxelLandscapeSettings,
    camera: [f64; 3],
) -> Result<()> {
    if prepared.mesh.bank != prepared.bank_epoch
        || prepared.bank().identity() != prepared.bank_epoch
    {
        return Err(AssetError::InvalidMetadata(
            "surface model/bank epoch mismatch".into(),
        ));
    }
    let cancel = Cancel::new(&RASTER_STOP);
    let size = [frame.raster.width, frame.raster.height];
    let mut pixels = RasterFrame::new(size, RasterLimits::default(), &prepared.budget, cancel)?;
    let light = SceneAtmosphere::from_index(settings.atmosphere).light();
    surface_raster::draw_mesh(
        &prepared.mesh,
        prepared.bank(),
        camera,
        f64::from(VoxelLandscapeScene::scale(settings)),
        frame.time,
        light,
        &mut pixels,
        cancel,
    )?;
    surface_entity_raster::draw_entity_mesh(
        &prepared.entities,
        prepared.bank(),
        &prepared.budget,
        camera,
        f64::from(VoxelLandscapeScene::scale(settings)),
        frame.time,
        light,
        &mut pixels,
        cancel,
    )?;
    if let Some(fluid) = &prepared.fluid {
        surface_raster::draw_fluid_mesh(
            fluid,
            prepared.bank(),
            camera,
            f64::from(VoxelLandscapeScene::scale(settings)),
            frame.time,
            light,
            &mut pixels,
            cancel,
        )?;
    }
    let mut colors = Vec::with_capacity(size[0].saturating_mul(size[1]));
    let mut covered = Vec::with_capacity(size[0].saturating_mul(size[1]));
    for y in 0..size[1] {
        for x in 0..size[0] {
            let pixel = pixels.pixel(x, y)?;
            let rgb = if pixel.color.alpha() == 0. {
                [0; 3]
            } else {
                pixel
                    .color
                    .straight()
                    .map(surface_raster::linear_to_srgb_byte)
            };
            colors.push(rgb);
            covered.push(pixel.color.alpha() > 0.);
        }
    }
    color::composite_selected(&colors, &covered, frame, settings);
    Ok(())
}

/// The old material-kernel probe remains available to compare its retained
/// regressions; the active scene above renders selected-pack geometry instead.
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
    fn freeze_stops_pan_but_surface_frame_time_remains_external() {
        let settings = VoxelLandscapeSettings {
            pan_speed_percent: 0,
            ..Default::default()
        };
        assert_eq!(
            VoxelLandscapeScene::camera(&settings, Duration::ZERO),
            VoxelLandscapeScene::camera(&settings, Duration::from_secs(1000))
        );
    }
    #[test]
    fn surface_requests_are_bounded_and_height_relative() {
        let settings = VoxelLandscapeSettings::default();
        let camera = VoxelLandscapeScene::camera(&settings, Duration::ZERO);
        assert!(camera[2] > 10.);
        let region = VoxelLandscapeScene::surface_region(
            camera,
            VoxelLandscapeScene::scale(&settings),
            [160, 96],
        );
        region.validate().unwrap();
    }
}
