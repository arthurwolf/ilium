//! The satellite refresh of the clouds scene as a state machine the scene advances once per
//! frame: find the newest image time, request a bounded set of frames (latest first, then
//! history) over the host http import, decode them into compact lon/lat registered grids one
//! slice per call, and publish the frame set. Nothing here waits: a request that has not
//! answered yet simply leaves the machine where it is.

use super::frames::{CloudFrame, FrameSet};
use super::providers::{fallback, plan_frame_times, provider, CloudSource, Endpoint, Provider};
use crate::kit::geo_tiles::{
    decode_image, decode_image_bounded, gibs_domains_url, gibs_tile_url, grid_from_tiles,
    level_for_density, level_within_budget, looks_like_image, parse_domains, tiles_for_box,
    wms_capabilities_url, wms_default_time, wms_map_url, DecodedImage, GeoBox, GeoGrid, TileId,
    UtcTime, MAX_TILE_BYTES, TILE_TIMEOUT,
};
use ilium_plugin_sdk::host::HttpRequest;
use ilium_plugin_sdk::types::RequestId;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

/// Upper bound on tiles fetched for one frame.
pub const MAX_TILES_PER_FRAME: usize = 9;
/// Upper bound on frames held for a time-lapse.
pub const MAX_FRAMES: usize = 24;
const RETRY_AFTER_MS: i64 = 5 * 60 * 1000;
/// How far back the GIBS DescribeDomains window reaches.
const DOMAIN_WINDOW_SECONDS: i64 = 36 * 3600;
/// Instants tried (newest first) when only the latest frame is wanted and the newest one is
/// not published yet.
const LIVE_CANDIDATES: i64 = 4;
/// The host reports a transport failure as a request that never answers, so a request is given
/// up on after its own timeout plus this margin.
const DEADLINE_MARGIN_MS: i64 = 7_000;

/// One finished answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Body(Vec<u8>),
    /// A non-success HTTP status.
    Status(u16),
}

/// The transport under a [`Refresher`].
pub trait Transport: Send {
    fn start(&mut self, url: &str) -> Result<u64, String>;
    /// The finished answer of request `id`, or `None` while it is still running.
    fn take(&mut self, id: u64) -> Option<Answer>;
}

/// The host's http import.
#[derive(Default)]
pub struct HostTransport;

impl Transport for HostTransport {
    fn start(&mut self, url: &str) -> Result<u64, String> {
        ilium_plugin_sdk::host()
            .fetch(HttpRequest {
                method: "GET".into(),
                url: url.into(),
                headers: vec![ilium_plugin_sdk::types::net::Header {
                    name: "Accept".into(),
                    value: "image/png, image/jpeg, application/xml, text/xml, */*".into(),
                }],
                body: None,
                timeout_millis: TILE_TIMEOUT.as_millis() as u32,
            })
            .map(|RequestId(id)| id)
            .map_err(|error| error.message)
    }

    fn take(&mut self, id: u64) -> Option<Answer> {
        let response = ilium_plugin_sdk::host().take_response(RequestId(id))?;
        Some(if (200..300).contains(&response.status) {
            Answer::Body(response.body)
        } else {
            Answer::Status(response.status)
        })
    }
}

/// What the refresh tells the scene.
pub enum Update {
    /// Progress text ("Downloading frame 3/19"), `None` when finished.
    Progress(Option<String>),
    Frames(Arc<FrameSet>),
    /// Nothing could be fetched; the message explains why.
    Offline(String),
}

/// What to fetch, for which box and at which grid size.
#[derive(Debug, Clone)]
pub struct Job {
    pub primary: CloudSource,
    pub bbox: GeoBox,
    pub grid_width: usize,
    pub grid_height: usize,
    pub want_rgb: bool,
    pub history_hours: i64,
    pub refresh: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    Offline(String),
    NoData(String),
}

/// How one request ended.
enum Fetched {
    Body(Vec<u8>),
    /// The server answered "not found", or something that is not the expected kind of data.
    Missing,
    /// Transport failure or server error: retrying later may work.
    Offline(String),
}

struct Pending {
    id: u64,
    deadline_ms: i64,
    /// The body must be an image; a WMS service exception is XML behind a success status.
    image: bool,
}

/// How one frame ended.
enum FrameError {
    Missing,
    Offline(String),
}

/// One tile download and the slices that decode it.
struct TileFetch {
    time: UtcTime,
    level: u8,
    ids: Vec<TileId>,
    pending: Vec<(TileId, Pending)>,
    to_decode: VecDeque<(TileId, Vec<u8>)>,
    tiles: HashMap<TileId, DecodedImage>,
    network_failures: usize,
    last_network: String,
}

enum FrameFetch {
    Wms { time: UtcTime, pending: Pending },
    Tiles(TileFetch),
}

struct FramesRun {
    provider: &'static Provider,
    wanted: Vec<UtcTime>,
    todo: usize,
    done: usize,
    cursor: usize,
    live_only: bool,
    fetch: Option<FrameFetch>,
}

enum Stage {
    /// Pick `chain[at]`, forgetting frames of another source.
    Start,
    /// Waiting for the catalogue that names the newest image time.
    Latest(&'static Provider, Pending),
    Frames(FramesRun),
}

struct SourceRun {
    chain: Vec<CloudSource>,
    at: usize,
    failure: Option<Failure>,
    stage: Stage,
}

enum Phase {
    /// Nothing runs until the clock reaches `until_ms`.
    Waiting {
        until_ms: i64,
    },
    Source(Box<SourceRun>),
}

/// The owned refresh loop of one scene. [`Refresher::poll`] never waits.
pub struct Refresher {
    job: Job,
    transport: Box<dyn Transport>,
    have: BTreeMap<UtcTime, Arc<CloudFrame>>,
    have_source: Option<CloudSource>,
    phase: Phase,
    updates: Vec<Update>,
}

impl Refresher {
    pub fn new(job: Job) -> Self {
        Self::with_transport(job, Box::new(HostTransport))
    }

    pub fn with_transport(job: Job, transport: Box<dyn Transport>) -> Self {
        Self {
            job,
            transport,
            have: BTreeMap::new(),
            have_source: None,
            phase: Phase::Waiting { until_ms: i64::MIN },
            updates: Vec::new(),
        }
    }

    /// What happened since the last call, oldest first.
    pub fn take_updates(&mut self) -> Vec<Update> {
        std::mem::take(&mut self.updates)
    }

    /// Advances the refresh to the wall clock `now_ms` (milliseconds since the epoch). Does at
    /// most one decode slice per call and never waits for the network.
    pub fn poll(&mut self, now_ms: i64) {
        loop {
            let phase = std::mem::replace(&mut self.phase, Phase::Waiting { until_ms: i64::MIN });
            let (next, again) = self.advance(phase, now_ms);
            self.phase = next;
            if !again {
                break;
            }
        }
    }

    fn advance(&mut self, phase: Phase, now_ms: i64) -> (Phase, bool) {
        match phase {
            Phase::Waiting { until_ms } => {
                if now_ms < until_ms {
                    return (Phase::Waiting { until_ms }, false);
                }
                // Once a source has produced frames only that source is retried; the
                // fallback chain applies while nothing has been shown yet.
                let mut chain = vec![self.job.primary];
                while let Some(next) = fallback(*chain.last().unwrap_or(&self.job.primary)) {
                    chain.push(next);
                }
                if let (Some(source), false) = (self.have_source, self.have.is_empty()) {
                    chain = vec![source];
                }
                let run = SourceRun {
                    chain,
                    at: 0,
                    failure: None,
                    stage: Stage::Start,
                };
                (Phase::Source(Box::new(run)), true)
            }
            Phase::Source(run) => self.advance_source(*run, now_ms),
        }
    }

    fn advance_source(&mut self, mut run: SourceRun, now_ms: i64) -> (Phase, bool) {
        let stage = std::mem::replace(&mut run.stage, Stage::Start);
        match stage {
            Stage::Start => {
                let source = run.chain[run.at];
                if self.have_source != Some(source) {
                    self.have.clear();
                }
                let provider = provider(source);
                match provider.endpoint {
                    Endpoint::GibsDaily { .. } => {
                        let latest = UtcTime::from_unix_millis(now_ms).start_of_day();
                        run.stage = Stage::Frames(self.begin_frames(provider, latest));
                    }
                    Endpoint::Wms { workspace, .. } => {
                        match self.request(&wms_capabilities_url(workspace), false, now_ms) {
                            Ok(pending) => run.stage = Stage::Latest(provider, pending),
                            Err(message) => {
                                return self.fail_source(run, Failure::Offline(message), now_ms)
                            }
                        }
                    }
                    Endpoint::GibsTimed {
                        layer, matrix_set, ..
                    } => {
                        // A window that changes only every ten minutes keeps the URL stable.
                        let end = UtcTime::from_unix_millis(now_ms).floor_to(600);
                        let url = gibs_domains_url(
                            layer,
                            matrix_set,
                            end.plus_seconds(-DOMAIN_WINDOW_SECONDS),
                            end,
                        );
                        match self.request(&url, false, now_ms) {
                            Ok(pending) => run.stage = Stage::Latest(provider, pending),
                            Err(message) => {
                                return self.fail_source(run, Failure::Offline(message), now_ms)
                            }
                        }
                    }
                }
                (Phase::Source(Box::new(run)), true)
            }
            Stage::Latest(provider, pending) => match self.poll_request(&pending, now_ms) {
                None => {
                    run.stage = Stage::Latest(provider, pending);
                    (Phase::Source(Box::new(run)), false)
                }
                Some(Fetched::Offline(message)) => {
                    self.fail_source(run, Failure::Offline(message), now_ms)
                }
                Some(Fetched::Missing) => self.fail_source(
                    run,
                    Failure::NoData("catalogue not found".to_owned()),
                    now_ms,
                ),
                Some(Fetched::Body(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let latest = match provider.endpoint {
                        Endpoint::Wms { layer, .. } => wms_default_time(&text, layer)
                            .ok_or_else(|| format!("no time listed for {layer}")),
                        Endpoint::GibsTimed { layer, .. } => parse_domains(&text)
                            .last()
                            .copied()
                            .ok_or_else(|| format!("no recent images of {layer}")),
                        Endpoint::GibsDaily { .. } => {
                            Ok(UtcTime::from_unix_millis(now_ms).start_of_day())
                        }
                    };
                    match latest {
                        Ok(latest) => {
                            run.stage = Stage::Frames(self.begin_frames(provider, latest));
                            (Phase::Source(Box::new(run)), true)
                        }
                        Err(message) => self.fail_source(run, Failure::NoData(message), now_ms),
                    }
                }
            },
            Stage::Frames(frames) => self.advance_frames(run, frames, now_ms),
        }
    }

    fn begin_frames(&mut self, provider: &'static Provider, latest: UtcTime) -> FramesRun {
        let live_only = self.job.history_hours <= 0;
        let wanted: Vec<UtcTime> = if live_only {
            (0..LIVE_CANDIDATES)
                .map(|back| latest.plus_seconds(-back * provider.cadence_minutes * 60))
                .collect()
        } else {
            plan_frame_times(
                latest,
                self.job.history_hours,
                provider.cadence_minutes,
                MAX_FRAMES,
            )
        };
        let oldest = wanted.iter().min().copied().unwrap_or(latest);
        self.have.retain(|time, _| *time >= oldest);
        let todo = wanted
            .iter()
            .filter(|time| !self.have.contains_key(time))
            .count();
        FramesRun {
            provider,
            wanted,
            todo,
            done: 0,
            cursor: 0,
            live_only,
            fetch: None,
        }
    }

    fn advance_frames(
        &mut self,
        mut run: SourceRun,
        mut frames: FramesRun,
        now_ms: i64,
    ) -> (Phase, bool) {
        if frames.fetch.is_none() {
            // The next instant that is not held yet.
            loop {
                let Some(time) = frames.wanted.get(frames.cursor).copied() else {
                    return self.finish_frames(run, frames, now_ms);
                };
                if self.have.contains_key(&time) {
                    if frames.live_only {
                        return self.finish_frames(run, frames, now_ms);
                    }
                    frames.cursor += 1;
                    continue;
                }
                frames.done += 1;
                self.updates
                    .push(Update::Progress(Some(if frames.live_only {
                        "Downloading the latest image".to_owned()
                    } else {
                        format!("Downloading frame {}/{}", frames.done, frames.todo)
                    })));
                match self.begin_fetch(frames.provider, time, now_ms) {
                    Ok(fetch) => frames.fetch = Some(fetch),
                    Err(FrameError::Missing) => {
                        frames.cursor += 1;
                        continue;
                    }
                    Err(FrameError::Offline(message)) => {
                        return self.frame_offline(run, frames, message, now_ms);
                    }
                }
                break;
            }
        }
        let Some(fetch) = frames.fetch.take() else {
            run.stage = Stage::Frames(frames);
            return (Phase::Source(Box::new(run)), false);
        };
        match self.poll_fetch(fetch, now_ms) {
            Poll::Waiting(fetch) => {
                // Still downloading, or one decode slice was all this call did.
                frames.fetch = Some(fetch);
                run.stage = Stage::Frames(frames);
                (Phase::Source(Box::new(run)), false)
            }
            Poll::Done(Ok(frame)) => {
                let time = frame.time;
                self.have.insert(time, Arc::new(frame));
                if frames.live_only {
                    // Keep only the newest picture.
                    let newest = self.have.keys().next_back().copied();
                    self.have.retain(|key, _| Some(*key) == newest);
                }
                self.publish(frames.provider);
                if frames.live_only {
                    return self.finish_frames(run, frames, now_ms);
                }
                frames.cursor += 1;
                run.stage = Stage::Frames(frames);
                // One finished frame (one decode) is all a call does.
                (Phase::Source(Box::new(run)), false)
            }
            Poll::Done(Err(FrameError::Missing)) => {
                frames.cursor += 1;
                run.stage = Stage::Frames(frames);
                (Phase::Source(Box::new(run)), true)
            }
            Poll::Done(Err(FrameError::Offline(message))) => {
                self.frame_offline(run, frames, message, now_ms)
            }
        }
    }

    /// A network failure ends the source with an error while nothing is held, and otherwise
    /// just ends the refresh with what is held.
    fn frame_offline(
        &mut self,
        run: SourceRun,
        frames: FramesRun,
        message: String,
        now_ms: i64,
    ) -> (Phase, bool) {
        if self.have.is_empty() {
            return self.fail_source(run, Failure::Offline(message), now_ms);
        }
        self.finish_frames(run, frames, now_ms)
    }

    fn finish_frames(&mut self, run: SourceRun, frames: FramesRun, now_ms: i64) -> (Phase, bool) {
        if self.have.is_empty() {
            let failure =
                Failure::NoData(format!("no {} images are available", frames.provider.name));
            return self.fail_source(run, failure, now_ms);
        }
        self.have_source = Some(frames.provider.source);
        self.finish_cycle(true, None, now_ms)
    }

    fn fail_source(&mut self, mut run: SourceRun, failure: Failure, now_ms: i64) -> (Phase, bool) {
        run.failure = Some(failure);
        run.at += 1;
        if run.at < run.chain.len() {
            run.stage = Stage::Start;
            return (Phase::Source(Box::new(run)), true);
        }
        self.finish_cycle(false, run.failure, now_ms)
    }

    fn finish_cycle(
        &mut self,
        succeeded: bool,
        failure: Option<Failure>,
        now_ms: i64,
    ) -> (Phase, bool) {
        self.updates.push(Update::Progress(None));
        let wait_ms = if succeeded {
            self.job.refresh.as_millis().min(i64::MAX as u128) as i64
        } else {
            let message = match failure {
                Some(Failure::Offline(message) | Failure::NoData(message)) => message,
                None => "no source answered".to_owned(),
            };
            self.updates.push(Update::Offline(message));
            RETRY_AFTER_MS.min(self.job.refresh.as_millis().min(i64::MAX as u128) as i64)
        };
        (
            Phase::Waiting {
                until_ms: now_ms.saturating_add(wait_ms),
            },
            false,
        )
    }

    fn publish(&mut self, provider: &Provider) {
        let set = FrameSet::new(
            provider.name,
            self.job.bbox,
            self.have.values().cloned().collect(),
        );
        self.updates.push(Update::Frames(Arc::new(set)));
    }

    fn request(&mut self, url: &str, image: bool, now_ms: i64) -> Result<Pending, String> {
        let id = self.transport.start(url)?;
        let timeout = TILE_TIMEOUT.as_millis() as i64 + DEADLINE_MARGIN_MS;
        Ok(Pending {
            id,
            deadline_ms: now_ms.saturating_add(timeout),
            image,
        })
    }

    fn poll_request(&mut self, pending: &Pending, now_ms: i64) -> Option<Fetched> {
        match self.transport.take(pending.id) {
            Some(Answer::Body(bytes)) => Some(
                if bytes.len() > MAX_TILE_BYTES || (pending.image && !looks_like_image(&bytes)) {
                    Fetched::Missing
                } else {
                    Fetched::Body(bytes)
                },
            ),
            Some(Answer::Status(404)) => Some(Fetched::Missing),
            Some(Answer::Status(status)) => Some(Fetched::Offline(format!("HTTP {status}"))),
            None if now_ms >= pending.deadline_ms => {
                Some(Fetched::Offline("request timed out".to_owned()))
            }
            None => None,
        }
    }

    /// Issues the requests of one frame.
    fn begin_fetch(
        &mut self,
        provider: &'static Provider,
        time: UtcTime,
        now_ms: i64,
    ) -> Result<FrameFetch, FrameError> {
        let job = self.job.clone();
        let (layer, matrix_set, max_level, daily) = match provider.endpoint {
            Endpoint::Wms { layer, .. } => {
                let url = wms_map_url(
                    layer,
                    &job.bbox,
                    job.grid_width,
                    job.grid_height,
                    Some(time),
                );
                let pending = self
                    .request(&url, true, now_ms)
                    .map_err(FrameError::Offline)?;
                return Ok(FrameFetch::Wms { time, pending });
            }
            Endpoint::GibsTimed {
                layer,
                matrix_set,
                max_level,
            } => (layer, matrix_set, max_level, false),
            Endpoint::GibsDaily {
                layer,
                matrix_set,
                max_level,
            } => (layer, matrix_set, max_level, true),
        };
        let dots_per_degree = job.grid_width as f64 / job.bbox.width_degrees();
        let level = level_within_budget(
            level_for_density(dots_per_degree, max_level),
            &job.bbox,
            MAX_TILES_PER_FRAME,
        );
        let ids = tiles_for_box(level, &job.bbox);
        let (time_text, extension) = if daily {
            (time.date(), "jpeg")
        } else {
            (time.iso_seconds(), "png")
        };
        let mut pending = Vec::with_capacity(ids.len());
        let mut network_failures = 0;
        let mut last_network = String::new();
        for id in &ids {
            let url = gibs_tile_url(layer, Some(&time_text), matrix_set, *id, extension);
            match self.request(&url, true, now_ms) {
                Ok(request) => pending.push((*id, request)),
                Err(message) => {
                    network_failures += 1;
                    last_network = message;
                }
            }
        }
        if pending.is_empty() {
            return Err(FrameError::Offline(last_network));
        }
        Ok(FrameFetch::Tiles(TileFetch {
            time,
            level,
            ids,
            pending,
            to_decode: VecDeque::new(),
            tiles: HashMap::new(),
            network_failures,
            last_network,
        }))
    }

    fn poll_fetch(&mut self, fetch: FrameFetch, now_ms: i64) -> Poll {
        match fetch {
            FrameFetch::Wms { time, pending } => match self.poll_request(&pending, now_ms) {
                None => Poll::Waiting(FrameFetch::Wms { time, pending }),
                Some(Fetched::Offline(message)) => Poll::Done(Err(FrameError::Offline(message))),
                Some(Fetched::Missing) => Poll::Done(Err(FrameError::Missing)),
                Some(Fetched::Body(bytes)) => {
                    // A WMS service exception arrives as XML with a success status.
                    let want_rgb = self.job.want_rgb;
                    let (width, height) = (self.job.grid_width, self.job.grid_height);
                    let Ok(image) = decode_image_bounded(&bytes, want_rgb, width, height) else {
                        return Poll::Done(Err(FrameError::Missing));
                    };
                    if image.width != width || image.height != height {
                        return Poll::Done(Err(FrameError::Missing));
                    }
                    Poll::Done(Ok(CloudFrame::new(
                        time,
                        GeoGrid::from_image(self.job.bbox, image),
                    )))
                }
            },
            FrameFetch::Tiles(mut tiles) => {
                if !tiles.pending.is_empty() {
                    let mut waiting = Vec::new();
                    for (id, pending) in std::mem::take(&mut tiles.pending) {
                        match self.poll_request(&pending, now_ms) {
                            None => waiting.push((id, pending)),
                            Some(Fetched::Body(bytes)) => tiles.to_decode.push_back((id, bytes)),
                            Some(Fetched::Missing) => {}
                            Some(Fetched::Offline(message)) => {
                                tiles.network_failures += 1;
                                tiles.last_network = message;
                            }
                        }
                    }
                    tiles.pending = waiting;
                    if !tiles.pending.is_empty() {
                        return Poll::Waiting(FrameFetch::Tiles(tiles));
                    }
                }
                if let Some((id, bytes)) = tiles.to_decode.pop_front() {
                    if let Ok(image) = decode_image(&bytes, self.job.want_rgb) {
                        tiles.tiles.insert(id, image);
                    }
                    return Poll::Waiting(FrameFetch::Tiles(tiles));
                }
                // Every tile failed on the network.
                if tiles.tiles.is_empty() && tiles.network_failures > 0 {
                    return Poll::Done(Err(FrameError::Offline(tiles.last_network)));
                }
                // Tiles beyond the edge of a satellite disc do not exist; a frame with fewer
                // than half of its tiles is not a usable picture.
                if tiles.tiles.len() * 2 < tiles.ids.len() {
                    return Poll::Done(Err(FrameError::Missing));
                }
                let grid = grid_from_tiles(
                    self.job.bbox,
                    self.job.grid_width,
                    self.job.grid_height,
                    tiles.level,
                    &tiles.tiles,
                    self.job.want_rgb,
                );
                Poll::Done(Ok(CloudFrame::new(tiles.time, grid)))
            }
        }
    }
}

enum Poll {
    Waiting(FrameFetch),
    Done(Result<CloudFrame, FrameError>),
}
