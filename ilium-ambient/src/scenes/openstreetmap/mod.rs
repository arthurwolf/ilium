//! Real OpenStreetMap geometry, loaded off the presentation thread.
mod bundle;
mod catalogue;
mod geometry;
mod loader;
mod render;
mod request;
mod settings;
mod tour;
mod transport;

use crate::{
    control::SceneSettings,
    scene::{Frame, Scene, SceneEnv},
    source::Worker,
};
pub use geometry::{GeometryMap, SourceElement};
use loader::LoadedMap;
use request::MapRequest;
pub use settings::OpenStreetMapSettings;
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
    cache: VecDeque<Arc<LoadedMap>>,
    loading: Option<Loading>,
    failure: Option<(MapRequest, String)>,
    source_error: Option<String>,
    width: u16,
    height: u16,
}
impl OpenStreetMapScene {
    pub fn new(settings: &OpenStreetMapSettings, _env: &SceneEnv) -> Self {
        Self {
            settings: settings.normalized(),
            seed: std::hash::BuildHasher::hash_one(
                &std::collections::hash_map::RandomState::new(),
                0x4f534d5f544f5552u64,
            ),
            current: None,
            cache: VecDeque::new(),
            loading: None,
            failure: None,
            source_error: None,
            width: 0,
            height: 0,
        }
    }
    /// Styles and camera changes keep decoded geometry and any active request.
    pub fn apply_settings(&mut self, settings: &OpenStreetMapSettings) {
        self.settings = settings.normalized();
    }
    fn receive(&mut self) {
        let Some(loading) = &self.loading else {
            return;
        };
        match loading.receiver.try_recv() {
            Ok(Ok(loaded)) => {
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
            Ok(Err(error)) => {
                self.failure = Some((loading.request.clone(), error));
                self.loading = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.failure = Some((
                    loading.request.clone(),
                    "OSM loading worker disconnected".into(),
                ));
                self.loading = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    fn request(&mut self, desired: MapRequest) {
        if self
            .current
            .as_ref()
            .is_some_and(|map| map.request == desired)
        {
            return;
        }
        if let Some(index) = self
            .cache
            .iter()
            .position(|cached| cached.request == desired)
        {
            if let Some(cached) = self.cache.remove(index) {
                self.current = Some(Arc::clone(&cached));
                self.cache.push_back(cached);
            }
            self.loading = None;
            self.failure = None;
            return;
        }
        if self
            .loading
            .as_ref()
            .is_some_and(|loading| loading.request == desired)
            || self
                .failure
                .as_ref()
                .is_some_and(|(request, _)| *request == desired)
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
            let loaded = loader::load_map(request, &stop, transport::fetch);
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
                self.failure = Some((desired, format!("OSM worker start: {error}")));
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
        let desired = MapRequest::from_settings(
            &self.settings,
            tour::place_index(&self.settings, seconds, self.seed),
        );
        match desired {
            Ok(desired) => {
                self.source_error = None;
                if self
                    .loading
                    .as_ref()
                    .is_some_and(|loading| loading.request != desired)
                {
                    self.loading = None;
                }
                self.receive();
                self.request(desired);
            }
            Err(error) => {
                self.source_error = Some(error);
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
        if let Some(error) = &self.source_error {
            return Some(format!("{error} · {credit}"));
        }
        if let Some((_, error)) = &self.failure {
            return Some(format!("{error} · {credit}"));
        }
        if self.loading.is_some() {
            return Some(format!("Loading real OSM geometry · {credit}"));
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
            map.label,
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
            &SceneEnv::for_test(std::env::temp_dir()),
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
        let mut scene =
            OpenStreetMapScene::new(&settings, &SceneEnv::for_test(std::env::temp_dir()));
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
            &SceneEnv::for_test(std::env::temp_dir()),
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
