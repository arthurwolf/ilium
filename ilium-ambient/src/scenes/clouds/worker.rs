//! The clouds scene's background worker: find the newest image time, download
//! a bounded set of frames (latest first, then history), decode them into
//! compact lon/lat registered grids and hand them to the scene.

use super::providers::{fallback, plan_frame_times, provider, CloudSource, Endpoint, Provider};
use crate::scenes::night_lights::tiles::{
    download_tiles, gibs_domains_url, gibs_tile_url, grid_from_tiles, level_for_density,
    level_within_budget, parse_domains, prune_cache, tiles_for_box, wms_capabilities_url,
    wms_default_time, wms_map_url, DownloadError, GeoBox, GeoGrid, NowFn, TileError, TileFetcher,
    UtcTime, IMMUTABLE_AGE,
};
use crate::source::sleep_unless_stopped;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

/// Upper bound on tiles fetched for one frame.
pub const MAX_TILES_PER_FRAME: usize = 9;
/// Upper bound on frames held for a time-lapse.
pub const MAX_FRAMES: usize = 24;
const CACHE_BUDGET: u64 = 150 * 1024 * 1024;
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
const CATALOGUE_MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// How far back the GIBS DescribeDomains window reaches.
const DOMAIN_WINDOW_SECONDS: i64 = 36 * 3600;
/// Instants tried (newest first) when only the latest frame is wanted and the
/// newest one is not published yet.
const LIVE_CANDIDATES: i64 = 4;

/// One decoded image: 8-bit luminance (and optionally colour) on a lon/lat box.
pub struct CloudFrame {
    pub time: UtcTime,
    pub grid: GeoGrid,
    histogram: [u32; 256],
}

impl CloudFrame {
    pub fn new(time: UtcTime, grid: GeoGrid) -> Self {
        let mut histogram = [0u32; 256];
        for value in &grid.luma {
            histogram[usize::from(*value)] += 1;
        }
        Self {
            time,
            grid,
            histogram,
        }
    }
}

/// Everything the scene needs to draw: frames oldest first plus the luminance
/// levels (0..1) used for automatic contrast across the whole set.
pub struct FrameSet {
    pub name: &'static str,
    pub bbox: GeoBox,
    pub frames: Vec<Arc<CloudFrame>>,
    pub black: f32,
    pub white: f32,
}

impl FrameSet {
    pub fn new(name: &'static str, bbox: GeoBox, mut frames: Vec<Arc<CloudFrame>>) -> Self {
        frames.sort_by_key(|frame| frame.time);
        let mut histogram = [0u64; 256];
        for frame in &frames {
            for (total, count) in histogram.iter_mut().zip(frame.histogram) {
                *total += u64::from(count);
            }
        }
        let (black, white) = levels(&histogram);
        Self {
            name,
            bbox,
            frames,
            black,
            white,
        }
    }
}

/// (5th, 99.5th) percentile as 0..1 with a minimum span, so a flat picture is
/// not stretched into noise.
fn levels(histogram: &[u64; 256]) -> (f32, f32) {
    let total: u64 = histogram.iter().sum();
    if total == 0 {
        return (0.0, 1.0);
    }
    let find = |fraction: f64| -> usize {
        let target = (total as f64 * fraction) as u64;
        let mut seen = 0;
        for (value, count) in histogram.iter().enumerate() {
            seen += count;
            if seen > target {
                return value;
            }
        }
        255
    };
    let black = find(0.05) as f32 / 255.0;
    let white = (find(0.995) as f32 / 255.0).max(black + 0.125);
    (black, white.min(1.0))
}

pub enum Update {
    /// Progress text ("Downloading frames 3/19"), `None` when finished.
    Progress(Option<String>),
    Frames(Arc<FrameSet>),
    /// Nothing could be fetched; the message explains why.
    Offline(String),
}

pub struct Job {
    pub cache_dir: PathBuf,
    pub primary: CloudSource,
    pub bbox: GeoBox,
    pub grid_width: usize,
    pub grid_height: usize,
    pub want_rgb: bool,
    pub history_hours: i64,
    pub refresh: Duration,
    pub fetcher: Arc<dyn TileFetcher>,
    pub now: NowFn,
}

#[derive(Debug)]
enum WorkerError {
    Stopped,
    Offline(String),
    NoData(String),
}

enum FrameError {
    Missing,
    Offline(String),
    Stopped,
}

impl From<TileError> for FrameError {
    fn from(error: TileError) -> Self {
        match error {
            TileError::Missing => Self::Missing,
            TileError::Network(message) => Self::Offline(message),
            TileError::Stopped => Self::Stopped,
        }
    }
}

impl From<DownloadError> for FrameError {
    fn from(error: DownloadError) -> Self {
        match error {
            DownloadError::Stopped => Self::Stopped,
            DownloadError::Network(message) => Self::Offline(message),
        }
    }
}

/// The newest instant the service can show.
fn latest_time(
    job: &Job,
    provider: &Provider,
    directory: &Path,
    stop: &AtomicBool,
) -> Result<UtcTime, WorkerError> {
    let map_error = |error: TileError| match error {
        TileError::Stopped => WorkerError::Stopped,
        TileError::Missing => WorkerError::NoData("catalogue not found".to_owned()),
        TileError::Network(message) => WorkerError::Offline(message),
    };
    let now = UtcTime::from_system_time((job.now)());
    match provider.endpoint {
        Endpoint::Wms { layer, workspace } => {
            let bytes = job
                .fetcher
                .fetch(
                    directory,
                    &wms_capabilities_url(workspace),
                    CATALOGUE_MAX_AGE,
                    stop,
                )
                .map_err(map_error)?;
            wms_default_time(&String::from_utf8_lossy(&bytes), layer)
                .ok_or_else(|| WorkerError::NoData(format!("no time listed for {layer}")))
        }
        Endpoint::GibsTimed {
            layer, matrix_set, ..
        } => {
            // A window that changes only every ten minutes keeps the URL cacheable.
            let end = now.floor_to(600);
            let url = gibs_domains_url(
                layer,
                matrix_set,
                end.plus_seconds(-DOMAIN_WINDOW_SECONDS),
                end,
            );
            let bytes = job
                .fetcher
                .fetch(directory, &url, Duration::from_secs(300), stop)
                .map_err(map_error)?;
            parse_domains(&String::from_utf8_lossy(&bytes))
                .last()
                .copied()
                .ok_or_else(|| WorkerError::NoData(format!("no recent images of {layer}")))
        }
        Endpoint::GibsDaily { .. } => Ok(now.start_of_day()),
    }
}

/// Download and decode the frame of one instant.
fn fetch_frame(
    job: &Job,
    provider: &Provider,
    time: UtcTime,
    directory: &Path,
    stop: &AtomicBool,
) -> Result<CloudFrame, FrameError> {
    let (layer, matrix_set, max_level, daily) = match provider.endpoint {
        Endpoint::Wms { layer, .. } => {
            let url = wms_map_url(
                layer,
                &job.bbox,
                job.grid_width,
                job.grid_height,
                Some(time),
            );
            let bytes = job.fetcher.fetch(directory, &url, IMMUTABLE_AGE, stop)?;
            // A WMS service exception arrives as XML with a success status.
            let image = crate::scenes::night_lights::tiles::decode_image(&bytes, job.want_rgb)
                .map_err(|_| FrameError::Missing)?;
            return Ok(CloudFrame::new(time, GeoGrid::from_image(job.bbox, image)));
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
    let download = download_tiles(
        &*job.fetcher,
        directory,
        &ids,
        &|id| gibs_tile_url(layer, Some(&time_text), matrix_set, id, extension),
        job.want_rgb,
        stop,
        &mut |_, _| {},
    )?;
    // Tiles beyond the edge of a satellite disc do not exist; a frame with
    // fewer than half of its tiles is not a usable picture.
    if download.tiles.len() * 2 < ids.len() {
        return Err(FrameError::Missing);
    }
    let grid = grid_from_tiles(
        job.bbox,
        job.grid_width,
        job.grid_height,
        level,
        &download.tiles,
        job.want_rgb,
    );
    Ok(CloudFrame::new(time, grid))
}

/// Bring `have` up to date for one source and publish after every new frame.
fn refresh(
    job: &Job,
    source: CloudSource,
    directory: &Path,
    have: &mut BTreeMap<UtcTime, Arc<CloudFrame>>,
    updates: &Sender<Update>,
    stop: &AtomicBool,
) -> Result<(), WorkerError> {
    let provider = provider(source);
    let latest = latest_time(job, provider, directory, stop)?;
    let live_only = job.history_hours <= 0;
    let wanted: Vec<UtcTime> = if live_only {
        (0..LIVE_CANDIDATES)
            .map(|back| latest.plus_seconds(-back * provider.cadence_minutes * 60))
            .collect()
    } else {
        plan_frame_times(
            latest,
            job.history_hours,
            provider.cadence_minutes,
            MAX_FRAMES,
        )
    };
    let oldest = wanted.iter().min().copied().unwrap_or(latest);
    have.retain(|time, _| *time >= oldest);
    let todo = wanted
        .iter()
        .filter(|time| !have.contains_key(time))
        .count();
    let mut done = 0;
    for time in &wanted {
        if have.contains_key(time) {
            if live_only {
                break;
            }
            continue;
        }
        if stop.load(Ordering::Relaxed) {
            return Err(WorkerError::Stopped);
        }
        done += 1;
        let _ = updates.send(Update::Progress(Some(format!(
            "Downloading frame {done}/{}",
            if live_only { 1 } else { todo }
        ))));
        match fetch_frame(job, provider, *time, directory, stop) {
            Ok(frame) => {
                have.insert(*time, Arc::new(frame));
                if live_only {
                    // Keep only the newest picture.
                    let newest = have.keys().next_back().copied();
                    have.retain(|key, _| Some(*key) == newest);
                }
                publish(job, provider, have, updates);
                if live_only {
                    break;
                }
            }
            Err(FrameError::Missing) => {}
            Err(FrameError::Stopped) => return Err(WorkerError::Stopped),
            Err(FrameError::Offline(message)) => {
                if have.is_empty() {
                    return Err(WorkerError::Offline(message));
                }
                break;
            }
        }
    }
    if have.is_empty() {
        return Err(WorkerError::NoData(format!(
            "no {} images are available",
            provider.name
        )));
    }
    prune_cache(directory, CACHE_BUDGET);
    Ok(())
}

fn publish(
    job: &Job,
    provider: &Provider,
    have: &BTreeMap<UtcTime, Arc<CloudFrame>>,
    updates: &Sender<Update>,
) {
    let set = FrameSet::new(provider.name, job.bbox, have.values().cloned().collect());
    let _ = updates.send(Update::Frames(Arc::new(set)));
}

pub fn run_worker(job: Job, updates: Sender<Update>, stop: Arc<AtomicBool>) {
    let directory = job.cache_dir.join("clouds");
    let mut have: BTreeMap<UtcTime, Arc<CloudFrame>> = BTreeMap::new();
    let mut have_source: Option<CloudSource> = None;
    while !stop.load(Ordering::Relaxed) {
        // Once a source has produced frames only that source is retried; the
        // fallback chain applies while nothing has been shown yet.
        let mut chain = vec![job.primary];
        while let Some(next) = fallback(*chain.last().unwrap_or(&job.primary)) {
            chain.push(next);
        }
        if let (Some(source), false) = (have_source, have.is_empty()) {
            chain = vec![source];
        }
        let mut failure: Option<WorkerError> = None;
        let mut succeeded = false;
        for source in chain {
            if have_source != Some(source) {
                have.clear();
            }
            match refresh(&job, source, &directory, &mut have, &updates, &stop) {
                Ok(()) => {
                    have_source = Some(source);
                    succeeded = true;
                    break;
                }
                Err(WorkerError::Stopped) => return,
                Err(error) => failure = Some(error),
            }
        }
        let _ = updates.send(Update::Progress(None));
        let wait = if succeeded {
            job.refresh
        } else {
            let message = match failure {
                Some(WorkerError::Offline(message) | WorkerError::NoData(message)) => message,
                _ => "no source answered".to_owned(),
            };
            let _ = updates.send(Update::Offline(message));
            RETRY_AFTER.min(job.refresh)
        };
        if !sleep_unless_stopped(&stop, wait) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_ignore_outliers_and_keep_a_minimum_span() {
        let mut histogram = [0u64; 256];
        histogram[40] = 900;
        histogram[200] = 99;
        histogram[255] = 1;
        let (black, white) = levels(&histogram);
        assert!((black - 40.0 / 255.0).abs() < 1e-6);
        assert!((white - 200.0 / 255.0).abs() < 0.01, "{white}");
        // A flat image gets at least a small span instead of a divide by zero.
        let mut flat = [0u64; 256];
        flat[100] = 1000;
        let (black, white) = levels(&flat);
        assert!(white - black >= 0.05, "{black} {white}");
        assert_eq!(levels(&[0; 256]), (0.0, 1.0));
    }

    #[test]
    fn frame_sets_are_sorted_and_summarised() {
        let bbox = GeoBox::WORLD;
        let dark = GeoGrid::blank(bbox, 4, 2, false);
        let mut bright = GeoGrid::blank(bbox, 4, 2, false);
        bright.luma.fill(255);
        let newer = Arc::new(CloudFrame::new(UtcTime(2000), bright));
        let older = Arc::new(CloudFrame::new(UtcTime(1000), dark));
        let set = FrameSet::new("test", bbox, vec![newer, older]);
        assert_eq!(set.frames[0].time, UtcTime(1000));
        assert_eq!(set.frames[1].time, UtcTime(2000));
        assert!(set.white > set.black);
    }
}
