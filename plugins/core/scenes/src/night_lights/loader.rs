//! The imagery loader of the "Earth at night" scene.
//!
//! A scene runs inside one render frame and owns no thread, so the former worker thread is a
//! small state machine the scene advances once per frame (`Loader::step`). Every step does a
//! bounded amount of work: it issues a few tile requests over the host's http import, collects
//! finished answers without waiting, decodes at most one tile and resamples at most one band of
//! the mosaic.
//!
//! Stages: `Start` (show what the plugin's shared cache holds, also fully offline), `Probe`
//! (find the newest daily layer), `Collect` (fetch and decode the tiles), `Assemble` (resample
//! them into one world grid, a band of rows per step), `Wait` (until the next refresh).
//! Tiles are cached in the plugin's shared storage bucket, which the host may evict at any time
//! and which holds only data that can be fetched again.

use super::settings::NightSource;
use crate::kit::geo_tiles::{
    decode_image, fill_grid_rows, gibs_tile_url, looks_like_image, tiles_for_box, DecodedImage,
    GeoBox, GeoGrid, MipGrid, TileId, UtcTime, GIBS_ROOT, TILE_PIXELS,
};
use crate::live_data::poll::Answer;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub(super) const DAILY_LAYER: &str = "VIIRS_NOAA20_GapFilled_BRDF_Corrected_DayNightBand_Radiance";
pub(super) const BLACK_MARBLE_LAYER: &str = "VIIRS_Black_Marble";
pub(super) const BLACK_MARBLE_DATE: &str = "2016-01-01";
const MATRIX_SET: &str = "500m";
/// Newest day probed first, then this many earlier days.
const DAILY_LOOKBACK_DAYS: i64 = 4;
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
/// Longest a tile request may run before the host reports a timeout.
const REQUEST_TIMEOUT_MILLIS: u32 = 8_000;
/// Tile requests in flight at once.
const MAX_IN_FLIGHT: usize = 4;
/// Rows of the mosaic resampled per step.
const ASSEMBLE_ROWS_PER_STEP: usize = 48;
/// Three consecutive transport failures end a download.
const CONSECUTIVE_FAILURE_LIMIT: usize = 3;
/// The host refuses storage values over 1 MiB; a larger tile is not cached.
const MAX_CACHED_BYTES: usize = 1024 * 1024;

/// The bytes under the loader: the network and the tile cache.
pub trait Imagery: Send {
    /// Starts a GET of `url`; the request id for `take`.
    fn start(&mut self, url: &str) -> Result<u64, String>;
    /// The finished answer of request `id`, or `None` while it is still running.
    fn take(&mut self, id: u64) -> Option<Answer>;
    /// A cached value, or `None`.
    fn recall(&mut self, key: &str) -> Option<Vec<u8>>;
    /// Keeps a value for later starts; a failure only loses the cache entry.
    fn remember(&mut self, key: &str, value: &[u8]);
}

/// The host's http import and the plugin's shared storage bucket.
#[derive(Default)]
pub struct HostImagery;

impl Imagery for HostImagery {
    fn start(&mut self, url: &str) -> Result<u64, String> {
        use ilium_plugin_sdk::host::HttpRequest;
        use ilium_plugin_sdk::types::RequestId;
        ilium_plugin_sdk::host()
            .fetch(HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![ilium_plugin_sdk::types::net::Header {
                    name: "Accept".into(),
                    value: "image/png,*/*".into(),
                }],
                body: None,
                timeout_millis: REQUEST_TIMEOUT_MILLIS,
            })
            .map(|RequestId(id)| id)
            .map_err(|error| error.message)
    }

    fn take(&mut self, id: u64) -> Option<Answer> {
        use ilium_plugin_sdk::types::RequestId;
        let response = ilium_plugin_sdk::host().take_response(RequestId(id))?;
        Some(if (200..300).contains(&response.status) {
            Answer::Body(response.body)
        } else {
            Answer::Status(response.status)
        })
    }

    fn recall(&mut self, key: &str) -> Option<Vec<u8>> {
        use ilium_plugin_sdk::host::{Bucket, HostStorage};
        HostStorage::get(
            &mut *ilium_plugin_sdk::host(),
            Bucket::Shared,
            key.to_owned(),
        )
    }

    fn remember(&mut self, key: &str, value: &[u8]) {
        use ilium_plugin_sdk::host::{Bucket, HostStorage};
        if value.len() > MAX_CACHED_BYTES {
            return;
        }
        let _ = HostStorage::put(
            &mut *ilium_plugin_sdk::host(),
            Bucket::Shared,
            key.to_owned(),
            value.to_vec(),
        );
    }
}

/// Why a tile did not arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileError {
    /// The server answered "not found": that tile or day does not exist.
    Missing,
    /// Transport failure or server error: retrying later may work.
    Network(String),
}

/// What a step tells the scene.
#[derive(Debug)]
pub enum Update {
    /// Progress text ("Downloading imagery 12/30 tiles"), `None` when finished.
    Progress(Option<String>),
    /// A new (or cached) mosaic.
    Mosaic { grid: Arc<MipGrid>, label: String },
    /// Nothing could be fetched; the message explains why.
    Offline(String),
}

fn tile_url(layer: &str, date: &str, id: TileId) -> String {
    gibs_tile_url(layer, Some(date), MATRIX_SET, id, "png")
}

/// The cache key of a tile URL.
fn cache_key(url: &str) -> String {
    format!(
        "night-lights/{}",
        url.strip_prefix(GIBS_ROOT).unwrap_or(url)
    )
}

/// The cache key of the date last assembled for `source`.
fn latest_key(source: NightSource) -> String {
    format!("night-lights/latest/{}", source.id())
}

fn mosaic_key(source: NightSource, date: &str, level: u8) -> String {
    format!("{}_{date}_L{level}", source.id())
}

/// One mosaic to build: which layer and day, at which level.
#[derive(Clone)]
struct Plan {
    source: NightSource,
    layer: &'static str,
    date: String,
    level: u8,
    ids: Vec<TileId>,
}

impl Plan {
    fn new(source: NightSource, date: String, level: u8) -> Self {
        let layer = match source {
            NightSource::Daily => DAILY_LAYER,
            NightSource::BlackMarble => BLACK_MARBLE_LAYER,
        };
        Self {
            source,
            layer,
            date,
            level,
            ids: tiles_for_box(level, &GeoBox::WORLD),
        }
    }

    fn key(&self) -> String {
        mosaic_key(self.source, &self.date, self.level)
    }

    fn url(&self, id: TileId) -> String {
        tile_url(self.layer, &self.date, id)
    }
}

struct Probe {
    today: UtcTime,
    back: i64,
    pending: Option<(u64, String)>,
}

struct Collect {
    plan: Plan,
    /// Only the cache may answer: the start-up pass that shows the last mosaic offline.
    cache_only: bool,
    next: usize,
    in_flight: Vec<(u64, TileId, String)>,
    ready: VecDeque<(TileId, Vec<u8>)>,
    tiles: HashMap<TileId, DecodedImage>,
    done: usize,
    missing: usize,
    consecutive_failures: usize,
    network_failures: usize,
    last_network_message: String,
}

impl Collect {
    fn new(plan: Plan, cache_only: bool) -> Self {
        Self {
            plan,
            cache_only,
            next: 0,
            in_flight: Vec::new(),
            ready: VecDeque::new(),
            tiles: HashMap::new(),
            done: 0,
            missing: 0,
            consecutive_failures: 0,
            network_failures: 0,
            last_network_message: String::new(),
        }
    }

    fn total(&self) -> usize {
        self.plan.ids.len()
    }

    fn finished(&self) -> bool {
        self.next >= self.total() && self.in_flight.is_empty() && self.ready.is_empty()
    }

    /// Records one transport failure; `Err` when the download must be abandoned.
    fn network_failure(&mut self, message: String) -> Result<(), String> {
        self.consecutive_failures += 1;
        self.network_failures += 1;
        self.missing += 1;
        self.done += 1;
        if self.consecutive_failures >= CONSECUTIVE_FAILURE_LIMIT {
            return Err(message);
        }
        self.last_network_message = message;
        Ok(())
    }
}

struct Assemble {
    plan: Plan,
    tiles: HashMap<TileId, DecodedImage>,
    grid: GeoGrid,
    next_row: usize,
    missing: usize,
    cached: bool,
}

enum Stage {
    /// Replaced while a step runs; never observed between steps.
    Idle,
    Start,
    Wait {
        until: SystemTime,
    },
    Probe(Probe),
    Collect(Collect),
    Assemble(Box<Assemble>),
}

/// The refresh loop of one scene instance: one source at one tile level.
pub struct Loader {
    imagery: Box<dyn Imagery>,
    source: NightSource,
    level: u8,
    refresh: Duration,
    stage: Stage,
    /// The key of the mosaic the scene shows, when it is complete.
    held_key: Option<String>,
}

impl Loader {
    pub fn new(
        imagery: Box<dyn Imagery>,
        source: NightSource,
        level: u8,
        refresh: Duration,
    ) -> Self {
        Self {
            imagery,
            source,
            level,
            refresh,
            stage: Stage::Start,
            held_key: None,
        }
    }

    /// Advances the loader by one bounded step at wall-clock time `now`.
    pub fn step(&mut self, now: SystemTime) -> Vec<Update> {
        let mut updates = Vec::new();
        let stage = std::mem::replace(&mut self.stage, Stage::Idle);
        self.stage = match stage {
            Stage::Idle => Stage::Idle,
            Stage::Start => self.start_cached(now),
            Stage::Wait { until } => {
                if now >= until {
                    self.begin_refresh(now, &mut updates)
                } else {
                    Stage::Wait { until }
                }
            }
            Stage::Probe(probe) => self.step_probe(probe, now, &mut updates),
            Stage::Collect(collect) => self.step_collect(collect, now, &mut updates),
            Stage::Assemble(assemble) => self.step_assemble(*assemble, now, &mut updates),
        };
        updates
    }

    /// Sources whose last assembled day may be shown before the network answers.
    fn cached_sources(&self) -> Vec<NightSource> {
        match self.source {
            NightSource::Daily => vec![NightSource::Daily, NightSource::BlackMarble],
            NightSource::BlackMarble => vec![NightSource::BlackMarble],
        }
    }

    /// Shows whatever the cache holds immediately (this also works fully offline).
    fn start_cached(&mut self, now: SystemTime) -> Stage {
        for source in self.cached_sources() {
            let date = self
                .imagery
                .recall(&latest_key(source))
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|date| UtcTime::parse(date).is_some());
            if let Some(date) = date {
                return Stage::Collect(Collect::new(Plan::new(source, date, self.level), true));
            }
        }
        let mut ignored = Vec::new();
        self.begin_refresh(now, &mut ignored)
    }

    /// Decides which layer and day to show next.
    fn begin_refresh(&mut self, now: SystemTime, updates: &mut Vec<Update>) -> Stage {
        match self.source {
            NightSource::BlackMarble => self.plan_stage(
                Plan::new(
                    NightSource::BlackMarble,
                    BLACK_MARBLE_DATE.to_owned(),
                    self.level,
                ),
                now,
                updates,
            ),
            NightSource::Daily => Stage::Probe(Probe {
                today: UtcTime::from_system_time(now).start_of_day(),
                back: 0,
                pending: None,
            }),
        }
    }

    /// A day was chosen: download it, unless the scene already shows it.
    fn plan_stage(&mut self, plan: Plan, now: SystemTime, updates: &mut Vec<Update>) -> Stage {
        if self.held_key.as_deref() == Some(plan.key().as_str()) {
            updates.push(Update::Progress(None));
            return Stage::Wait {
                until: now + self.refresh,
            };
        }
        updates.push(Update::Progress(Some(format!(
            "Downloading imagery 0/{} tiles",
            plan.ids.len()
        ))));
        Stage::Collect(Collect::new(plan, false))
    }

    fn offline(&mut self, message: String, now: SystemTime, updates: &mut Vec<Update>) -> Stage {
        updates.push(Update::Offline(message));
        Stage::Wait {
            until: now + RETRY_AFTER,
        }
    }

    fn step_probe(
        &mut self,
        mut probe: Probe,
        now: SystemTime,
        updates: &mut Vec<Update>,
    ) -> Stage {
        let first = tiles_for_box(self.level, &GeoBox::WORLD)[0];
        let (request, url) = match probe.pending.take() {
            Some(pending) => pending,
            None => {
                let date = probe.today.plus_seconds(-probe.back * 86_400).date();
                let url = tile_url(DAILY_LAYER, &date, first);
                if let Some(bytes) = self.imagery.recall(&cache_key(&url)) {
                    if looks_like_image(&bytes) {
                        return self.probe_found(date, now, updates);
                    }
                }
                match self.imagery.start(&url) {
                    Ok(request) => {
                        probe.pending = Some((request, url));
                        return Stage::Probe(probe);
                    }
                    Err(message) => return self.offline(message, now, updates),
                }
            }
        };
        let Some(answer) = self.imagery.take(request) else {
            probe.pending = Some((request, url));
            return Stage::Probe(probe);
        };
        let date = probe.today.plus_seconds(-probe.back * 86_400).date();
        match classify(answer) {
            Ok(bytes) => {
                self.imagery.remember(&cache_key(&url), &bytes);
                self.probe_found(date, now, updates)
            }
            Err(TileError::Missing) => {
                probe.back += 1;
                if probe.back > DAILY_LOOKBACK_DAYS {
                    let plan = Plan::new(
                        NightSource::BlackMarble,
                        BLACK_MARBLE_DATE.to_owned(),
                        self.level,
                    );
                    self.plan_stage(plan, now, updates)
                } else {
                    Stage::Probe(probe)
                }
            }
            Err(TileError::Network(message)) => self.offline(message, now, updates),
        }
    }

    fn probe_found(&mut self, date: String, now: SystemTime, updates: &mut Vec<Update>) -> Stage {
        let plan = Plan::new(NightSource::Daily, date, self.level);
        self.plan_stage(plan, now, updates)
    }

    fn step_collect(
        &mut self,
        mut collect: Collect,
        now: SystemTime,
        updates: &mut Vec<Update>,
    ) -> Stage {
        // Cached tiles are taken at once; the rest go to the network, a few at a time.
        while collect.in_flight.len() < MAX_IN_FLIGHT && collect.next < collect.total() {
            let id = collect.plan.ids[collect.next];
            let url = collect.plan.url(id);
            collect.next += 1;
            if let Some(bytes) = self.imagery.recall(&cache_key(&url)) {
                collect.ready.push_back((id, bytes));
            } else if collect.cache_only {
                collect.missing += 1;
                collect.done += 1;
            } else {
                match self.imagery.start(&url) {
                    Ok(request) => collect.in_flight.push((request, id, url)),
                    Err(message) => {
                        if let Err(message) = collect.network_failure(message) {
                            return self.offline(message, now, updates);
                        }
                    }
                }
            }
        }
        // Collect the finished answers without waiting.
        let mut index = 0;
        while index < collect.in_flight.len() {
            let (request, id, url) = collect.in_flight[index].clone();
            let Some(answer) = self.imagery.take(request) else {
                index += 1;
                continue;
            };
            collect.in_flight.remove(index);
            match classify(answer) {
                Ok(bytes) => {
                    collect.consecutive_failures = 0;
                    self.imagery.remember(&cache_key(&url), &bytes);
                    collect.ready.push_back((id, bytes));
                }
                Err(TileError::Missing) => {
                    collect.consecutive_failures = 0;
                    collect.missing += 1;
                    collect.done += 1;
                }
                Err(TileError::Network(message)) => {
                    if let Err(message) = collect.network_failure(message) {
                        return self.offline(message, now, updates);
                    }
                }
            }
        }
        // Decode at most one tile per step.
        if let Some((id, bytes)) = collect.ready.pop_front() {
            match decode_image(&bytes, false) {
                Ok(image) => {
                    collect.tiles.insert(id, image);
                }
                Err(_) => collect.missing += 1,
            }
            collect.done += 1;
        }
        if !collect.cache_only {
            updates.push(Update::Progress(Some(format!(
                "Downloading imagery {}/{} tiles",
                collect.done.min(collect.total()),
                collect.total()
            ))));
        }
        if !collect.finished() {
            return Stage::Collect(collect);
        }
        // Every tile failed on the network (a short list never reaches three in a row).
        if collect.tiles.is_empty()
            && collect.network_failures > 0
            && collect.network_failures == collect.total()
        {
            let message = collect.last_network_message.clone();
            return self.offline(message, now, updates);
        }
        // The start-up pass only shows a complete cached mosaic; a half-filled cache is
        // refilled by the refresh instead.
        if collect.cache_only && collect.missing > 0 {
            return self.begin_refresh(now, updates);
        }
        if collect.tiles.is_empty() {
            return self.offline("no imagery tiles are available".to_owned(), now, updates);
        }
        let scale = 1usize << collect.plan.level;
        let grid = GeoGrid::blank(
            GeoBox::WORLD,
            TILE_PIXELS * 5 * scale / 4,
            TILE_PIXELS * 5 * scale / 8,
            false,
        );
        Stage::Assemble(Box::new(Assemble {
            plan: collect.plan,
            tiles: collect.tiles,
            grid,
            next_row: 0,
            missing: collect.missing,
            cached: collect.cache_only,
        }))
    }

    fn step_assemble(
        &mut self,
        mut assemble: Assemble,
        now: SystemTime,
        updates: &mut Vec<Update>,
    ) -> Stage {
        let end = (assemble.next_row + ASSEMBLE_ROWS_PER_STEP).min(assemble.grid.height);
        fill_grid_rows(
            &mut assemble.grid,
            assemble.plan.level,
            &assemble.tiles,
            assemble.next_row..end,
        );
        assemble.next_row = end;
        if end < assemble.grid.height {
            return Stage::Assemble(Box::new(assemble));
        }
        let Assemble {
            plan,
            tiles,
            mut grid,
            missing,
            cached,
            ..
        } = assemble;
        grid.apply_black_level(plan.source.black_level());
        let complete = missing == 0;
        let mut label = format!("{} {}", plan.source.name(), plan.date);
        if cached {
            label.push_str(" (cached)");
        } else {
            if plan.source != self.source {
                label.push_str(" (no recent daily data)");
            }
            if !complete {
                label.push_str(&format!(
                    " (partial: {} of {} tiles)",
                    tiles.len(),
                    plan.ids.len()
                ));
            }
        }
        updates.push(Update::Mosaic {
            grid: Arc::new(MipGrid::build(grid)),
            label,
        });
        updates.push(Update::Progress(None));
        self.held_key = complete.then(|| plan.key());
        if complete && !cached {
            self.imagery
                .remember(&latest_key(plan.source), plan.date.as_bytes());
        }
        if cached {
            return self.begin_refresh(now, updates);
        }
        Stage::Wait {
            until: now + if complete { self.refresh } else { RETRY_AFTER },
        }
    }
}

/// An answer as a tile: the image bytes, or why there are none. A body that is not an image is
/// a service exception behind a success status and counts as missing.
fn classify(answer: Answer) -> Result<Vec<u8>, TileError> {
    match answer {
        Answer::Body(bytes) if looks_like_image(&bytes) => Ok(bytes),
        Answer::Body(_) => Err(TileError::Missing),
        Answer::Status(404) => Err(TileError::Missing),
        Answer::Status(status) => Err(TileError::Network(format!("HTTP status {status}"))),
        Answer::Failed(message) => Err(TileError::Network(message)),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Answers one URL: the bytes, or the failure.
    pub type Responder = Box<dyn Fn(&str) -> Answer + Send>;

    /// An in-memory imagery source: every answer comes at once, every request is recorded.
    #[derive(Clone)]
    pub struct FakeImagery {
        pub requests: Arc<Mutex<Vec<String>>>,
        pub cache: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        respond: Arc<Mutex<Responder>>,
        answers: Arc<Mutex<HashMap<u64, Answer>>>,
        counter: Arc<Mutex<u64>>,
    }

    impl FakeImagery {
        pub fn new(respond: impl Fn(&str) -> Answer + Send + 'static) -> Self {
            Self {
                requests: Arc::default(),
                cache: Arc::default(),
                respond: Arc::new(Mutex::new(Box::new(respond))),
                answers: Arc::default(),
                counter: Arc::default(),
            }
        }

        pub fn urls(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Imagery for FakeImagery {
        fn start(&mut self, url: &str) -> Result<u64, String> {
            self.requests.lock().unwrap().push(url.to_owned());
            let answer = (self.respond.lock().unwrap())(url);
            let mut counter = self.counter.lock().unwrap();
            *counter += 1;
            self.answers.lock().unwrap().insert(*counter, answer);
            Ok(*counter)
        }

        fn take(&mut self, id: u64) -> Option<Answer> {
            self.answers.lock().unwrap().remove(&id)
        }

        fn recall(&mut self, key: &str) -> Option<Vec<u8>> {
            self.cache.lock().unwrap().get(key).cloned()
        }

        fn remember(&mut self, key: &str, value: &[u8]) {
            self.cache
                .lock()
                .unwrap()
                .insert(key.to_owned(), value.to_vec());
        }
    }
}
