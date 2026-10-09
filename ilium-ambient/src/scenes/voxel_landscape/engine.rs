//! One owned low-priority pack/world/model worker. Frames only raster immutable
//! bank-matched geometry; pan speed never controls animation time or world seed.
use super::{
    assets::{
        budget::Cancel,
        error::{AssetError, Result},
        identity::Digest256,
    },
    color,
    generation::{PreparedWorld, Region as LegacyRegion},
    render::{self, Canvas},
    settings::VoxelLandscapeSettings,
    surface_binding::{self, GeneratedViewportSession, PreparedSurface, StreamedViewport},
    surface_context::SceneAtmosphere,
    surface_entity_raster,
    surface_generation::Region,
    surface_raster::{self, RasterFrame, RasterLimits},
    surface_retirement::Retirement,
    surface_viewport,
    terrain_fields::TerrainFields,
};
use crate::{
    control::SceneSettings,
    scene::{Frame, Scene, SceneEnv},
    source::Worker,
    style::ScenePalette,
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
    size: [usize; 2],
    camera: [f64; 3],
    time: Duration,
    stream: bool,
}
enum PreparedResult {
    Retained(Box<PreparedSurface>),
    Streamed(Box<StreamedViewport>),
}
struct Response {
    revision: u64,
    region: Region,
    size: [usize; 2],
    result: Result<PreparedResult>,
}

/// Pick the other accepted Faithful profile for explicit missing-image
/// fallback. Private diagnostic profiles are not guaranteed to be installed
/// or compatible with the selected target format.
fn reviewed_fallback_profile(selected: usize) -> usize {
    if selected == 6 {
        5
    } else {
        6
    }
}

fn resolve_generated_sources(
    settings: &VoxelLandscapeSettings,
    cache_dir: &std::path::Path,
    cancel: Cancel<'_>,
) -> Result<(VoxelLandscapeSettings, Option<VoxelLandscapeSettings>)> {
    let settings = settings.normalized();
    if settings.generated_texture_source == super::settings::GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT {
        return Ok((settings, None));
    }
    let resolved = super::pack_registry::resolve_registered(&settings, cache_dir, cancel)?;
    let candidate = VoxelLandscapeSettings {
        pack_profile: reviewed_fallback_profile(resolved.pack_profile),
        ..Default::default()
    };
    let fallback = match super::pack_registry::resolve_registered(&candidate, cache_dir, cancel) {
        Ok(settings) => Some(settings),
        Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
        Err(error) => {
            tracing::warn!("reviewed image fallback unavailable: {error}");
            None
        }
    };
    Ok((resolved, fallback))
}

pub struct VoxelLandscapeScene {
    settings: VoxelLandscapeSettings,
    requests: Arc<Mutex<Option<Request>>>,
    revision: Arc<AtomicU64>,
    results: Arc<Mutex<Option<Response>>>,
    worker: Option<Worker>,
    requested: Option<(Region, [usize; 2])>,
    prepared: Option<PreparedSurface>,
    prepared_request: Option<(Region, [usize; 2])>,
    streamed: Option<StreamedViewport>,
    streamed_request: Option<(Region, [usize; 2])>,
    stream_pending: bool,
    retired: Arc<Retirement<PreparedSurface>>,
    error: Option<String>,
    palette: ScenePalette,
}

/// Source-bound evidence for the currently prepared generated viewport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSourceReceipt {
    pub texture_source: &'static str,
    pub source_sha256: Option<Digest256>,
    pub material_coverage: (usize, usize),
    pub state_gaps: usize,
    pub model_substitutions: usize,
    pub compatibility_aliases: usize,
    pub material_fallbacks: usize,
    pub fallback_atlases: usize,
}

impl VoxelLandscapeScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: `color::composite_selected`
    // moves every pixel onto the palette colour of equal brightness before
    // cell averaging, so `PaletteScene` skips its generic remap.
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
            // The fixed one-GiB value is exactly ByteBudget's validated maximum.
            let budget = surface_binding::scene_budget()
                .expect("fixed generated-scene account must satisfy ByteBudget ceiling");
            let mut stream_session: Option<GeneratedViewportSession> = None;
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
                let result = resolve_generated_sources(&worker_settings, &cache_dir, cancel)
                    .and_then(|(resolved, fallback)| {
                        if request.stream {
                            let scale = Self::scale(&worker_settings);
                            if !stream_session.as_ref().is_some_and(|session| {
                                session.matches(
                                    request.region,
                                    scale,
                                    request.size,
                                    &resolved,
                                    fallback.as_ref(),
                                )
                            }) {
                                stream_session = None;
                                stream_session = Some(GeneratedViewportSession::open(
                                    request.region,
                                    scale,
                                    request.size,
                                    &resolved,
                                    fallback.as_ref(),
                                    budget.clone(),
                                    cancel,
                                )?);
                            }
                            stream_session
                                .as_mut()
                                .ok_or_else(|| {
                                    AssetError::InvalidMetadata(
                                        "generated viewport session absent".into(),
                                    )
                                })?
                                .render(request.camera, request.time, cancel)
                                .map(|streamed| PreparedResult::Streamed(Box::new(streamed)))
                        } else {
                            stream_session = None;
                            surface_binding::prepare_viewport_with_fallback_in_budget(
                                request.region,
                                Self::scale(&worker_settings),
                                request.size,
                                &resolved,
                                fallback.as_ref(),
                                budget.clone(),
                                cancel,
                            )
                            .map(|prepared| PreparedResult::Retained(Box::new(prepared)))
                        }
                    });
                if cancel.is_cancelled() {
                    continue;
                }
                let response = Response {
                    revision: request.revision,
                    region: request.region,
                    size: request.size,
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
            prepared_request: None,
            streamed: None,
            streamed_request: None,
            stream_pending: false,
            retired,
            error,
            palette: env.palette.clone(),
        }
    }

    /// Returns provenance and material coverage only for the current request.
    /// A stale frame never qualifies a newer camera or source selection.
    pub fn texture_source_receipt(&self) -> Option<TextureSourceReceipt> {
        if self.streamed_request == self.requested {
            if let Some(streamed) = self.streamed.as_ref() {
                return Some(TextureSourceReceipt {
                    texture_source: self.texture_source_name(),
                    source_sha256: streamed.source_sha256.clone(),
                    material_coverage: streamed.material_coverage,
                    state_gaps: streamed.state_gaps,
                    model_substitutions: streamed.model_substitutions,
                    compatibility_aliases: streamed.compatibility_aliases,
                    material_fallbacks: streamed.material_fallbacks,
                    fallback_atlases: streamed.fallback_atlases,
                });
            }
        }
        if self.prepared_request == self.requested {
            if let Some(prepared) = self.prepared.as_ref() {
                return Some(TextureSourceReceipt {
                    texture_source: self.texture_source_name(),
                    source_sha256: prepared.source_sha256.clone(),
                    material_coverage: prepared.material_coverage(),
                    state_gaps: prepared.skipped_states.len(),
                    model_substitutions: prepared.model_substitutions.len(),
                    compatibility_aliases: prepared.compatibility_aliases.len(),
                    material_fallbacks: prepared.material_fallbacks.len(),
                    fallback_atlases: prepared
                        .entities
                        .atlases
                        .iter()
                        .filter(|atlas| atlas.used_fallback)
                        .count(),
                });
            }
        }
        None
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
    pub fn surface_region(camera: [f64; 3], scale: f32, size: [usize; 2]) -> Result<Region> {
        surface_viewport::region(camera, scale, size)
    }
    pub fn prepared(&self) -> Option<&PreparedSurface> {
        self.prepared.as_ref()
    }
    fn receive_prepared(&mut self) {
        if let Ok(mut slot) = self.results.try_lock() {
            if let Some(response) = slot.take() {
                if response.revision == self.revision.load(Ordering::Acquire) {
                    match response.result {
                        Ok(result) => {
                            if let Some(previous) = self.prepared.take() {
                                if let Err(previous) = self.retired.try_retire(previous) {
                                    self.prepared = Some(previous);
                                    *slot = Some(Response {
                                        revision: response.revision,
                                        region: response.region,
                                        size: response.size,
                                        result: Ok(result),
                                    });
                                    return;
                                }
                            }
                            self.error = None;
                            match result {
                                PreparedResult::Retained(prepared) => {
                                    self.streamed = None;
                                    self.streamed_request = None;
                                    self.prepared = Some(*prepared);
                                    self.prepared_request = Some((response.region, response.size));
                                }
                                PreparedResult::Streamed(streamed) => {
                                    self.prepared_request = None;
                                    self.streamed = Some(*streamed);
                                    self.streamed_request = Some((response.region, response.size));
                                    self.stream_pending = false;
                                }
                            }
                        }
                        Err(AssetError::Cancelled) => {}
                        Err(error) => {
                            self.error = Some(format!("Surface preparation: {error}"));
                        }
                    }
                } else if let Ok(PreparedResult::Retained(prepared)) = response.result {
                    if let Err(prepared) = self.retired.try_retire(*prepared) {
                        *slot = Some(Response {
                            revision: response.revision,
                            region: response.region,
                            size: response.size,
                            result: Ok(PreparedResult::Retained(Box::new(prepared))),
                        });
                    }
                }
            }
        }
    }
    fn request(&mut self, region: Region, size: [usize; 2], camera: [f64; 3], time: Duration) {
        let scale = Self::scale(&self.settings);
        let stream = requires_streamed_viewport(region, scale, size);
        if self.worker.is_none()
            || (self.requested == Some((region, size))
                && (!stream
                    || self.stream_pending
                    || self.streamed.as_ref().is_some_and(|snapshot| {
                        snapshot.camera == camera && snapshot.time == time
                    })))
        {
            return;
        }
        if self.prepared_request != Some((region, size)) {
            if let Some(previous) = self.prepared.take() {
                if let Err(previous) = self.retired.try_retire(previous) {
                    self.prepared = Some(previous);
                    return;
                }
            }
            self.prepared_request = None;
        }
        if self.streamed_request != Some((region, size)) {
            self.streamed = None;
            self.streamed_request = None;
        }
        let Ok(mut slot) = self.requests.try_lock() else {
            return;
        };
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        *slot = Some(Request {
            revision,
            region,
            size,
            camera,
            time,
            stream,
        });
        self.requested = Some((region, size));
        self.stream_pending = stream;
    }
}

fn requires_streamed_viewport(region: Region, scale: f32, size: [usize; 2]) -> bool {
    scale < 1.4
        || surface_viewport::visible_tiles(region, scale, size)
            .map_or(true, |tiles| tiles.len() > 1)
}
impl Scene for VoxelLandscapeScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.receive_prepared();
        let camera = Self::camera(&self.settings, frame.time);
        let scale = Self::scale(&self.settings);
        let size = [frame.raster.width, frame.raster.height];
        let region = match Self::surface_region(camera, scale, size) {
            Ok(region) => region,
            Err(error) => {
                self.error = Some(format!("Surface coverage: {error}"));
                frame.cell_colors.fill([0; 3]);
                frame.raster.dots.fill(0.);
                return;
            }
        };
        self.request(region, size, camera, frame.time);
        if self.streamed_request == Some((region, size)) {
            if let Some(streamed) = &self.streamed {
                color::composite_selected(
                    &streamed.colors,
                    &streamed.covered,
                    frame,
                    &self.settings,
                    &self.palette,
                );
                return;
            }
        }
        let Some(prepared) = self.prepared.as_ref() else {
            frame.cell_colors.fill([0; 3]);
            frame.raster.dots.fill(0.);
            return;
        };
        if self.prepared_request != Some((region, size)) {
            frame.cell_colors.fill([0; 3]);
            frame.raster.dots.fill(0.);
            return;
        }
        if let Err(error) = render_surface(prepared, frame, &self.settings, camera, &self.palette) {
            self.error = Some(format!("Surface raster: {error}"));
            frame.cell_colors.fill([0; 3]);
            frame.raster.dots.fill(0.);
        }
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn set_palette(&mut self, palette: &ScenePalette) {
        // Colours are composited every frame; nothing is cached.
        self.palette = palette.clone();
    }
    fn follows_palette(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn status(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        if self.streamed_request == self.requested {
            return self
                .streamed
                .as_ref()
                .and_then(|snapshot| snapshot.status.clone());
        }
        if self.prepared_request != self.requested {
            return Some(self.preparation_status());
        }
        let Some(prepared) = self.prepared.as_ref() else {
            return Some(self.preparation_status());
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
impl VoxelLandscapeScene {
    fn texture_source_name(&self) -> &'static str {
        if self.settings.generated_texture_source
            == super::settings::GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT
        {
            "java-default-1.19.3"
        } else {
            "selected-pack"
        }
    }

    fn preparation_status(&self) -> String {
        if self.settings.generated_texture_source
            == super::settings::GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT
        {
            return "Preparing installed Minecraft Java 1.19.3 textures and generated surface…"
                .into();
        }
        let source_name = super::pack_profiles::FULL_PACKS
            .get(self.settings.pack_profile)
            .map_or("selected texture pack", |profile| profile.name);
        format!("Preparing {source_name} and generated surface…")
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
    palette: &ScenePalette,
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
    color::composite_selected(&colors, &covered, frame, settings, palette);
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
    fn java_default_generation_skips_selected_pack_registry_resolution() {
        static CANCEL: AtomicBool = AtomicBool::new(false);
        let settings = VoxelLandscapeSettings {
            generated_texture_source: super::super::settings::GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT,
            pack_path: "/missing/selected-pack.zip".into(),
            ..Default::default()
        };
        let (resolved, fallback) = resolve_generated_sources(
            &settings,
            std::path::Path::new("/missing/pack-cache"),
            Cancel::new(&CANCEL),
        )
        .expect("native source selection does not resolve a selected pack");
        assert_eq!(
            resolved.generated_texture_source,
            settings.generated_texture_source
        );
        assert_eq!(resolved.pack_path, settings.pack_path);
        assert!(fallback.is_none());
    }

    #[test]
    fn reviewed_fallback_uses_the_other_faithful_profile() {
        for (selected, profile) in super::super::pack_profiles::FULL_PACKS.iter().enumerate() {
            let fallback = reviewed_fallback_profile(selected);
            assert_eq!(
                super::super::pack_profiles::FULL_PACKS[fallback].id,
                if profile.id == "faithful64" {
                    "faithful32"
                } else {
                    "faithful64"
                }
            );
            assert_ne!(selected, fallback);
        }
    }

    #[test]
    fn generated_texture_receipt_is_unavailable_before_current_surface_preparation() {
        let env = SceneEnv::for_test(
            std::env::temp_dir().join("voxel-texture-receipt-test"),
            crate::resources::test_resources(),
        );
        let scene = VoxelLandscapeScene::new(&VoxelLandscapeSettings::default(), &env);
        assert!(scene.texture_source_receipt().is_none());
    }

    #[test]
    fn follows_palette_natively_and_stores_updates() {
        let mut env = SceneEnv::for_test(
            std::env::temp_dir().join("voxel-palette-test"),
            crate::resources::test_resources(),
        );
        let none = VoxelLandscapeScene::new(&VoxelLandscapeSettings::default(), &env);
        assert!(none.follows_palette() && !none.palette.is_provided());
        env.palette = ScenePalette {
            stops: vec![[0, 0, 0], [255, 0, 0]],
            reverse: false,
            shift_percent: 0,
        };
        let mut scene = VoxelLandscapeScene::new(&VoxelLandscapeSettings::default(), &env);
        assert!(scene.palette.is_provided());
        scene.set_palette(&ScenePalette::default());
        assert!(!scene.palette.is_provided());
    }
    #[test]
    fn all_supported_zoom_witnesses_use_finite_tile_streaming() {
        let camera = [64.0, 64.0, 80.0];
        for (zoom, size) in [(25, [720, 480]), (100, [360, 240]), (400, [160, 96])] {
            let settings = VoxelLandscapeSettings {
                zoom_percent: zoom,
                ..Default::default()
            };
            let scale = VoxelLandscapeScene::scale(&settings);
            let region = VoxelLandscapeScene::surface_region(camera, scale, size).unwrap();
            let tiles = surface_viewport::visible_tiles(region, scale, size).unwrap();
            assert!(
                tiles.len() > 1,
                "zoom {zoom} selected {} tiles",
                tiles.len()
            );
            assert!(
                requires_streamed_viewport(region, scale, size),
                "zoom {zoom} must use the finite-bank tile renderer"
            );
            if zoom == 400 {
                assert!(tiles.len() <= 16, "the legacy >16 gate must be caught");
            }
        }
    }
    #[test]
    fn streamed_frame_request_waits_for_its_owned_revision() {
        let settings = VoxelLandscapeSettings {
            zoom_percent: 25,
            ..Default::default()
        };
        let env = SceneEnv::for_test(
            std::env::temp_dir().join("ilium-viewport-request-test"),
            crate::resources::test_resources(),
        );
        let mut scene = VoxelLandscapeScene::new(&settings, &env);
        let region = Region {
            minimum: [0, 0],
            maximum: [96, 96],
        };
        scene.request(region, [160, 96], [64.0, 64.0, 80.0], Duration::ZERO);
        let first = scene.revision.load(Ordering::Acquire);
        scene.request(
            region,
            [160, 96],
            [65.0, 64.0, 80.0],
            Duration::from_millis(83),
        );
        assert_eq!(scene.revision.load(Ordering::Acquire), first);
        scene.stream_pending = false;
        scene.request(
            region,
            [160, 96],
            [65.0, 64.0, 80.0],
            Duration::from_millis(83),
        );
        assert_eq!(scene.revision.load(Ordering::Acquire), first + 1);
    }
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
        )
        .unwrap();
        assert!(region.maximum[0] - region.minimum[0] > 128);
    }
}
