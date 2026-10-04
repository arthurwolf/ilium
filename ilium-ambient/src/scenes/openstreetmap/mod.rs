//! Real OpenStreetMap geometry, loaded off the presentation thread.
pub mod address_search;
mod address_settings;
mod bundle;
mod catalogue;
pub(crate) mod geometry;
mod loader;
pub mod places;
pub(crate) mod render;
mod request;
mod settings;
mod tour;
mod transport;

use crate::{
    control::SceneSettings,
    scene::{Frame, Scene, SceneEnv},
    source::Worker,
};
pub use address_settings::{AddressProvider, AddressSearchSettings};
pub use geometry::{GeometryMap, SourceElement};
use loader::LoadedMap;
use request::MapRequest;
pub use settings::OpenStreetMapSettings;
#[cfg(test)]
use settings::SelectionMode;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
};

// At most two OSM workers exist, including detached cancellation cleanup.
// A timed HTTP read may finish after a settings change; admission stays bounded.
static ACTIVE_LOADS: AtomicUsize = AtomicUsize::new(0);
struct Admission;
impl Admission {
    fn acquire() -> Option<Self> {
        ACTIVE_LOADS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        ACTIVE_LOADS.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Loading {
    request: MapRequest,
    receiver: Receiver<Result<LoadedMap, String>>,
    worker: Option<Worker>,
}
impl Drop for Loading {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}
pub struct OpenStreetMapScene {
    settings: OpenStreetMapSettings,
    seed: u64,
    current: Option<Arc<LoadedMap>>,
    // Labels are presentation state: changing one must not clone or reload geometry.
    current_label: Option<String>,
    cache: VecDeque<Arc<LoadedMap>>,
    loading: Option<Loading>,
    failure: Option<(MapRequest, String)>,
    source_error: Option<String>,
    desired: Option<(MapRequest, String)>,
    /// At most one failed entry per compiled destination; no automatic replay
    /// when a tour returns to a failed stop. Explicit acquisition edits clear it.
    failed: Vec<(MapRequest, String)>,
    tour_origin: Option<f64>,
    width: u16,
    height: u16,
}
impl OpenStreetMapScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: &OpenStreetMapSettings, _env: &SceneEnv) -> Self {
        Self {
            settings: settings.normalized(),
            seed: std::hash::BuildHasher::hash_one(
                &std::collections::hash_map::RandomState::new(),
                0x4f534d5f544f5552u64,
            ),
            current: None,
            current_label: None,
            cache: VecDeque::new(),
            loading: None,
            failure: None,
            source_error: None,
            desired: None,
            failed: Vec::new(),
            tour_origin: None,
            width: 0,
            height: 0,
        }
    }
    /// Styles and camera changes keep decoded geometry and any active request.
    pub fn apply_settings(&mut self, settings: &OpenStreetMapSettings) {
        let next = settings.normalized();
        if !self.settings.same_acquisition(&next) {
            // Retire before a new target can reuse the same identity (ABA).
            self.loading = None;
            self.failure = None;
            self.failed.clear();
            self.desired = None;
            self.tour_origin = None;
        }
        self.settings = next;
    }
    fn remember_failure(&mut self, request: MapRequest, error: String) {
        if !self.failed.iter().any(|(known, _)| *known == request)
            && self.failed.len() < places::DESTINATIONS.len()
        {
            self.failed.push((request.clone(), error.clone()));
        }
        self.failure = Some((request, error));
    }
    fn receive(&mut self) {
        let Some(loading) = &self.loading else {
            return;
        };
        let request = loading.request.clone();
        let outcome = loading.receiver.try_recv();
        match outcome {
            Ok(Ok(loaded)) if loaded.request == request => {
                self.current_label = Some(loaded.label.clone());
                let loaded = Arc::new(loaded);
                self.cache.retain(|cached| cached.request != loaded.request);
                self.cache.push_back(Arc::clone(&loaded));
                while self.cache.len() > 2 {
                    self.cache.pop_front();
                }
                self.current = Some(loaded);
                self.failure = None;
                self.loading = None;
            }
            Ok(Ok(_)) => {
                self.remember_failure(
                    request,
                    "OSM worker returned a different source identity".into(),
                );
                self.loading = None;
            }
            Ok(Err(error)) => {
                self.remember_failure(request, error);
                self.loading = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.remember_failure(request, "OSM loading worker disconnected".into());
                self.loading = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    fn request(&mut self, desired: MapRequest, label: String) {
        if self
            .failure
            .as_ref()
            .is_some_and(|(request, _)| *request != desired)
        {
            self.failure = None;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|map| map.request == desired)
        {
            self.loading = None;
            self.failure = None;
            self.current_label = Some(label);
            return;
        }
        if let Some(index) = self
            .cache
            .iter()
            .position(|cached| cached.request == desired)
        {
            if let Some(cached) = self.cache.remove(index) {
                self.current = Some(Arc::clone(&cached));
                self.current_label = Some(label);
                self.cache.push_back(cached);
            }
            self.loading = None;
            self.failure = None;
            return;
        }
        if let Some((_, error)) = self.failed.iter().find(|(request, _)| *request == desired) {
            self.failure = Some((desired, error.clone()));
            self.loading = None;
            return;
        }
        if self.failed.len() >= places::DESTINATIONS.len() {
            self.failure = Some((
                desired,
                "OSM failure ledger is full; edit source/selection to try again".into(),
            ));
            self.loading = None;
            return;
        }
        if self
            .loading
            .as_ref()
            .is_some_and(|loading| loading.request == desired)
        {
            return;
        }
        self.loading = None;
        let Some(admission) = Admission::acquire() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let request = desired.clone();
        let worker = Worker::try_spawn("openstreetmap", move |stop| {
            let _admission = admission;
            let loaded = loader::load_map(request, &stop, transport::fetch).map(|mut loaded| {
                loaded.label = label;
                loaded
            });
            if !stop.load(Ordering::Relaxed) {
                let _ = sender.send(loaded);
            }
        });
        match worker {
            Ok(worker) => {
                self.failure = None;
                self.loading = Some(Loading {
                    request: desired,
                    receiver,
                    worker: Some(worker),
                });
            }
            Err(error) => {
                self.remember_failure(desired, format!("OSM worker start: {error}"));
            }
        }
    }
}
impl Scene for OpenStreetMapScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.width = frame.width;
        self.height = frame.height;
        let seconds = frame.time.as_secs_f64();
        // Resolve first: stale source results are discarded before publication.
        let tour_seconds = if self.settings.is_list_tour() {
            let wall = frame.wall.as_secs_f64();
            let origin = self.tour_origin.get_or_insert(wall);
            if wall < *origin {
                *origin = wall;
            }
            wall - *origin
        } else {
            seconds
        };
        let desired = MapRequest::selection(
            &self.settings,
            tour::place_index(&self.settings, tour_seconds, self.seed),
        );
        match desired {
            Ok((desired, label)) => {
                self.desired = Some((desired.clone(), label.clone()));
                self.source_error = None;
                if self
                    .loading
                    .as_ref()
                    .is_some_and(|loading| loading.request != desired)
                {
                    self.loading = None;
                }
                self.receive();
                self.request(desired, label);
            }
            Err(error) => {
                self.source_error = Some(error);
                self.desired = None;
                self.loading = None;
            }
        }
        if let Some(map) = &self.current {
            render::draw_map(frame.raster, &map.map.features, &self.settings, seconds);
        } else {
            frame.raster.dots.fill(0.);
        }
    }
    fn reconfigure(&mut self, settings: &crate::registry::AmbientSettings) -> bool {
        self.apply_settings(&settings.openstreetmap);
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        // The host forwards this as actual terminal text, independent of dots.
        const CREDIT: &str = "(c) OpenStreetMap contributors / ODbL";
        if self.width == 0 || x >= self.width || y >= self.height {
            return None;
        }
        // Wrap ASCII attribution across the final rows of narrow panes instead
        // of dropping it whenever one full line cannot fit. No per-cell allocation.
        let width = usize::from(self.width);
        let rows = CREDIT.len().div_ceil(width);
        if rows > usize::from(self.height) {
            return None;
        }
        let first_row = usize::from(self.height) - rows;
        let row = usize::from(y).checked_sub(first_row)?;
        CREDIT
            .as_bytes()
            .get(row * width + usize::from(x))
            .copied()
            .map(char::from)
    }
    fn status(&self) -> Option<String> {
        let credit = "© OpenStreetMap contributors · ODbL";
        let retained = self.current.as_ref().map_or(String::new(), |map| {
            format!(
                " · showing previous {}",
                self.current_label.as_deref().unwrap_or(map.label.as_str())
            )
        });
        let requested = self
            .desired
            .as_ref()
            .map_or("destination", |(_, label)| label.as_str());
        if let Some(error) = &self.source_error {
            return Some(format!("{error}{retained} · {credit}"));
        }
        if let Some((_, error)) = &self.failure {
            return Some(format!("{requested}: {error}{retained} · {credit}"));
        }
        if self.loading.is_some() {
            return Some(format!("Loading {requested}{retained} · {credit}"));
        }
        if self.desired.as_ref().is_some_and(|(request, _)| {
            self.current
                .as_ref()
                .is_none_or(|map| map.request != *request)
        }) {
            return Some(format!(
                "Waiting for OSM worker slot for {requested}{retained} · {credit}"
            ));
        }
        let Some(map) = &self.current else {
            return Some(format!("Waiting for OSM worker slot · {credit}"));
        };
        let geometry = &map.map;
        let diagnostics = if geometry.geometry_budget_exhausted {
            " · geometry work limit reached; affected areas shown as outlines".to_owned()
        } else if geometry.incomplete_rings + geometry.orphan_holes > 0 {
            format!(
                " · {} incomplete rings, {} unassociated holes shown as outlines",
                geometry.incomplete_rings, geometry.orphan_holes
            )
        } else {
            String::new()
        };
        Some(format!(
            "{} · {} features{} · {credit}",
            self.desired
                .as_ref()
                .filter(|(request, _)| *request == map.request)
                .map_or_else(
                    || self.current_label.as_deref().unwrap_or(map.label.as_str()),
                    |(_, label)| label.as_str(),
                ),
            geometry.features.len(),
            diagnostics
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::{
        sync::Mutex,
        time::{Duration, Instant, SystemTime},
    };
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    fn frame<'a>(raster: &'a mut Raster, colors: &'a mut Vec<[u8; 3]>) -> Frame<'a> {
        Frame {
            raster,
            cell_colors: colors,
            width: 80,
            height: 24,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        }
    }
    #[test]
    fn attribution_wraps_without_disappearing_in_a_narrow_viewport() {
        let mut scene = OpenStreetMapScene::new(
            &OpenStreetMapSettings::default(),
            &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
        );
        scene.width = 20;
        scene.height = 10;
        let mut text = String::new();
        for y in 0..scene.height {
            for x in 0..scene.width {
                if let Some(glyph) = scene.native_glyph(x, y) {
                    text.push(glyph);
                }
            }
        }
        assert_eq!(text, "(c) OpenStreetMap contributors / ODbL");
        assert_eq!(scene.native_glyph(20, 9), None);
        assert_eq!(scene.native_glyph(0, 10), None);
        scene.width = 0;
        assert_eq!(scene.native_glyph(0, 9), None);
    }
    #[test]
    fn worker_admission_is_bounded_even_before_results_are_received() {
        let _lock = TEST_LOCK.lock().unwrap();
        let first = Admission::acquire().unwrap();
        let second = Admission::acquire().unwrap();
        assert!(Admission::acquire().is_none());
        drop(first);
        assert!(Admission::acquire().is_some());
        drop(second);
    }
    #[test]
    fn real_offline_worker_loads_without_network_and_style_retains_geometry() {
        let _lock = TEST_LOCK.lock().unwrap();
        let settings = OpenStreetMapSettings::default();
        let mut scene = OpenStreetMapScene::new(
            &settings,
            &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
        );
        let mut raster = Raster::default();
        raster.resize(160, 96);
        let mut colors = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while scene.current.is_none() && Instant::now() < deadline {
            scene.render(&mut frame(&mut raster, &mut colors));
            std::thread::sleep(Duration::from_millis(10));
        }
        let loaded = Arc::clone(scene.current.as_ref().expect("offline worker must publish"));
        assert!(raster.dots.iter().any(|dot| *dot > 0.1));
        assert!(scene
            .status()
            .unwrap()
            .contains("OpenStreetMap contributors"));
        assert_eq!(scene.native_glyph(0, 23), Some('('));
        let mut changed = settings;
        changed.brightness_percent = 0;
        changed.roads = false;
        assert!(scene.reconfigure(&crate::registry::AmbientSettings {
            openstreetmap: changed,
            ..Default::default()
        }));
        scene.render(&mut frame(&mut raster, &mut colors));
        assert!(raster.dots.iter().all(|dot| *dot == 0.));
        assert!(Arc::ptr_eq(&loaded, scene.current.as_ref().unwrap()));
        assert!(scene.loading.is_none());
    }
    #[test]
    fn changed_source_discards_a_queued_old_result_before_publication() {
        let _lock = TEST_LOCK.lock().unwrap();
        let mut scene = OpenStreetMapScene::new(
            &OpenStreetMapSettings::default(),
            &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
        );
        let (sender, receiver) = mpsc::channel();
        let old = loader::load_map(
            MapRequest::Catalogue(0),
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| panic!("unexpected network"),
        )
        .unwrap();
        sender.send(Ok(old)).unwrap();
        scene.loading = Some(Loading {
            request: MapRequest::Catalogue(0),
            receiver,
            worker: None,
        });
        scene.apply_settings(&OpenStreetMapSettings {
            source: 1,
            coordinates: "invalid".into(),
            ..Default::default()
        });
        let mut raster = Raster::default();
        raster.resize(160, 96);
        let mut colors = Vec::new();
        scene.render(&mut frame(&mut raster, &mut colors));
        assert!(scene.current.is_none());
        assert!(scene.loading.is_none());
        assert!(scene.status().unwrap().contains("latitude"));
    }
}

#[cfg(test)]
mod selection_lifecycle_tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn fake_map(request: MapRequest, label: &str) -> Arc<LoadedMap> {
        Arc::new(LoadedMap {
            request,
            label: label.into(),
            map: GeometryMap {
                features: Vec::new(),
                sources: Vec::new(),
                timestamp: None,
                incomplete_rings: 0,
                orphan_holes: 0,
                geometry_budget_exhausted: false,
            },
        })
    }
    fn render_at(scene: &mut OpenStreetMapScene, wall: f64, time: f64) {
        let mut raster = Raster::default();
        raster.resize(160, 96);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 80,
            height: 24,
            wall: Duration::from_secs_f64(wall),
            time: Duration::from_secs_f64(time),
            now: SystemTime::UNIX_EPOCH,
        });
    }
    fn scene(settings: &OpenStreetMapSettings) -> OpenStreetMapScene {
        OpenStreetMapScene::new(
            settings,
            &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
        )
    }
    #[test]
    fn corrected_label_survives_a_later_failure_without_reloading_geometry() {
        let mut scene = scene(&OpenStreetMapSettings::default());
        let geometry = fake_map(MapRequest::Catalogue(0), "Original label");
        scene.current = Some(Arc::clone(&geometry));
        scene.desired = Some((MapRequest::Catalogue(0), "Corrected label".into()));
        scene.request(MapRequest::Catalogue(0), "Corrected label".into());
        assert!(scene.status().unwrap().starts_with("Corrected label"));
        scene.source_error = Some("Next destination has no configured source".into());
        assert!(scene
            .status()
            .unwrap()
            .contains("showing previous Corrected label"));
        assert!(Arc::ptr_eq(&geometry, scene.current.as_ref().unwrap()));
        assert!(scene.loading.is_none());
    }
    #[test]
    fn cached_geometry_uses_the_current_authored_label_when_retained() {
        let mut scene = scene(&OpenStreetMapSettings::default());
        let geometry = fake_map(MapRequest::Catalogue(0), "Cached original label");
        scene.cache.push_back(Arc::clone(&geometry));
        scene.desired = Some((MapRequest::Catalogue(0), "Cached corrected label".into()));
        scene.request(MapRequest::Catalogue(0), "Cached corrected label".into());
        scene.source_error = Some("Next destination is invalid".into());
        assert!(scene
            .status()
            .unwrap()
            .contains("showing previous Cached corrected label"));
        assert!(Arc::ptr_eq(&geometry, scene.current.as_ref().unwrap()));
        assert_eq!(scene.cache.len(), 1);
        assert!(scene.loading.is_none());
    }
    #[test]
    fn configured_list_clock_is_wall_based_and_starts_with_selected_member() {
        let settings = OpenStreetMapSettings {
            source: 2,
            selection: SelectionMode::List,
            tour: 1,
            dwell_seconds: 60,
            ..Default::default()
        };
        let mut scene = scene(&settings);
        // Preload the exact requested identities: rendering this fixture never
        // starts a loader, reads an asset or accesses the network.
        scene.current = Some(fake_map(MapRequest::Catalogue(0), "Paris"));
        render_at(&mut scene, 100., 9000.);
        assert_eq!(scene.desired.as_ref().unwrap().0, MapRequest::Catalogue(0));
        render_at(&mut scene, 159., 18000.);
        assert_eq!(scene.desired.as_ref().unwrap().0, MapRequest::Catalogue(0));
        scene.current = Some(fake_map(MapRequest::Catalogue(1), "London"));
        render_at(&mut scene, 160., 27000.);
        assert_eq!(scene.desired.as_ref().unwrap().0, MapRequest::Catalogue(1));
        let mut changed = settings;
        changed.destination_id = "venice".into();
        scene.apply_settings(&changed);
        scene.current = Some(fake_map(MapRequest::Catalogue(2), "Venice"));
        render_at(&mut scene, 1000., 1.);
        assert_eq!(scene.desired.as_ref().unwrap().0, MapRequest::Catalogue(2));
        assert!(scene.loading.is_none());
    }
    #[test]
    fn revisiting_failed_stop_does_not_start_another_worker_after_a_success() {
        let mut scene = scene(&OpenStreetMapSettings::default());
        let failed = MapRequest::Custom {
            url: "https://unused.invalid/api".into(),
            center: [1., 2.],
        };
        scene.remember_failure(failed.clone(), "fixture HTTP 503".into());
        scene.current = Some(fake_map(MapRequest::Catalogue(0), "Paris"));
        scene.request(MapRequest::Catalogue(0), "Paris".into());
        assert!(scene.failure.is_none());
        scene.desired = Some((failed.clone(), "Requested stop".into()));
        scene.request(failed.clone(), "Requested stop".into());
        assert!(scene.loading.is_none());
        assert_eq!(scene.failure.as_ref().unwrap().0, failed);
        let status = scene.status().unwrap();
        assert!(status.contains("Requested stop") && status.contains("showing previous Paris"));
        assert!(status.contains("OpenStreetMap contributors"));
        assert_eq!(scene.failed.len(), 1);
    }
    #[test]
    fn presentation_edits_preserve_failure_latches_but_explicit_acquisition_edits_clear_them() {
        let settings = OpenStreetMapSettings::default();
        let mut scene = scene(&settings);
        scene.remember_failure(MapRequest::Catalogue(1), "fixture failure".into());
        let mut changed = settings.clone();
        changed.camera = 0;
        changed.brightness_percent = 0;
        changed.search.provider = AddressProvider::Disabled;
        scene.apply_settings(&changed);
        assert_eq!(scene.failed.len(), 1);
        changed.place = 2;
        scene.apply_settings(&changed);
        assert!(scene.failed.is_empty() && scene.failure.is_none());
    }
    #[test]
    fn changing_away_and_back_retires_the_old_result_channel() {
        let settings = OpenStreetMapSettings::default();
        let mut scene = scene(&settings);
        let (sender, receiver) = mpsc::channel();
        scene.loading = Some(Loading {
            request: MapRequest::Catalogue(0),
            receiver,
            worker: None,
        });
        let mut changed = settings.clone();
        changed.place = 1;
        scene.apply_settings(&changed);
        scene.apply_settings(&settings);
        assert!(scene.loading.is_none());
        assert!(sender.send(Err("late result".into())).is_err());
        assert!(scene.current.is_none());
    }
    #[test]
    fn malformed_settings_render_an_error_without_loader_admission_or_source_substitution() {
        let settings = OpenStreetMapSettings {
            source: 2,
            selection: SelectionMode::List,
            place_list: "parks".into(),
            ..Default::default()
        };
        let mut scene = scene(&settings);
        scene.current = Some(fake_map(MapRequest::Catalogue(0), "Paris"));
        for seconds in [0., 60., 120., 1e6] {
            render_at(&mut scene, seconds, seconds);
        }
        assert!(scene.loading.is_none());
        assert_eq!(
            scene.current.as_ref().unwrap().request,
            MapRequest::Catalogue(0)
        );
        let status = scene.status().unwrap();
        assert!(
            status.contains("no bundled geometry") && status.contains("showing previous Paris")
        );
        assert!(scene.desired.is_none());
    }
    #[test]
    fn full_failure_ledger_is_bounded_and_fails_closed_before_worker_admission() {
        let mut scene = scene(&OpenStreetMapSettings::default());
        for index in 0..places::DESTINATIONS.len() + 2 {
            scene.remember_failure(
                MapRequest::Custom {
                    url: format!("https://unused.invalid/{index}"),
                    center: [0., 0.],
                },
                "fixture failure".into(),
            );
        }
        assert_eq!(scene.failed.len(), places::DESTINATIONS.len());
        scene.request(
            MapRequest::Custom {
                url: "https://unused.invalid/new".into(),
                center: [0., 0.],
            },
            "new".into(),
        );
        assert!(scene.loading.is_none());
        assert!(scene.failure.as_ref().unwrap().1.contains("ledger is full"));
    }
}
