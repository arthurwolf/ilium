//! The per-frame playback state machine of the Video scene.
//!
//! One `advance` call per rendered frame does a bounded amount of work and never waits: it
//! lists a few folders, moves a download on by one request, opens the next clip, or pulls the
//! frames that are due from the host decoder (`TimedOut` from the host means "not ready yet").
//! The scene turns the newest due frame it gets back into dots.

use super::convert::request_size;
use super::germination::{Download, Step};
use super::schedule::{resolve_seed, PlayItem, Scheduler};
use super::settings::{PlaybackMode, VideoSettings, VideoSource, PICKED_FILE};
use super::source::{
    catalogue, CatalogueClip, ClipInfo, Discovery, Entry, EntrySource, FramePull, Opened, Progress,
    VideoIo,
};
use crate::control::SceneSettings;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Frames pulled from the decoder by one `advance`.
const MAX_PULLS: usize = 6;
/// A display that has fallen this far behind the clock is moved up to it.
const MAX_LAG: Duration = Duration::from_millis(750);
/// Pause after every entry failed, before the list is tried again.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// Frame rate assumed when a container states none.
const FALLBACK_FPS: f64 = 25.0;

/// A decoded picture on its way to the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct RawFrame {
    /// Packed `rgb8`.
    pub rgb: Vec<u8>,
    pub size: (u32, u32),
    pub name: String,
    pub source_seconds: f64,
    pub duration_seconds: Option<f64>,
}

/// Where the list of clips stands.
enum ListState {
    Discovering(Box<Discovery>),
    Ready,
}

struct Held {
    offset_millis: f64,
    data: Vec<u8>,
    size: (u32, u32),
}

struct Playing {
    video: u64,
    info: ClipInfo,
    item: PlayItem,
    name: String,
    frame_millis: f64,
    pulled: u64,
    /// The wall time at which offset 0 of the clip was due; set by the first frame.
    origin: Option<Duration>,
    held: Option<Held>,
    shown_any: bool,
}

enum Stage {
    Idle,
    Downloading {
        entry: usize,
        download: Box<Download>,
    },
    Playing(Box<Playing>),
}

/// The playback state machine.
pub struct Player {
    settings: VideoSettings,
    io: Box<dyn VideoIo>,
    list: ListState,
    entries: Vec<Entry>,
    clips: Vec<CatalogueClip>,
    notes: Vec<String>,
    scheduler: Scheduler,
    seeded: bool,
    stage: Stage,
    failures: usize,
    retry_at: Option<Duration>,
    notice: Option<String>,
}

impl Player {
    pub fn new(settings: &VideoSettings, io: Box<dyn VideoIo>) -> Self {
        Self::build(settings, io, Vec::new())
    }

    /// A player that plays `clips` for the Germination source instead of the shipped catalogue.
    #[cfg(test)]
    pub fn with_catalogue(
        settings: &VideoSettings,
        io: Box<dyn VideoIo>,
        clips: Vec<CatalogueClip>,
    ) -> Self {
        Self::build(settings, io, clips)
    }

    fn build(settings: &VideoSettings, io: Box<dyn VideoIo>, clips: Vec<CatalogueClip>) -> Self {
        let settings = SceneSettings::normalized(settings);
        let mut player = Self {
            scheduler: Scheduler::new(&settings, 0),
            io,
            list: ListState::Ready,
            entries: Vec::new(),
            clips,
            notes: Vec::new(),
            seeded: false,
            stage: Stage::Idle,
            failures: 0,
            retry_at: None,
            notice: None,
            settings,
        };
        player.build_list();
        player
    }

    fn build_list(&mut self) {
        match self.settings.source {
            VideoSource::Germination => {
                if self.clips.is_empty() {
                    self.clips = catalogue();
                }
                self.entries = self
                    .clips
                    .iter()
                    .enumerate()
                    .map(|(index, clip)| Entry {
                        name: clip.display_name(),
                        source: EntrySource::Catalogue(index),
                    })
                    .collect();
                if self.entries.is_empty() {
                    self.notice = Some("The Germination catalogue is empty".to_owned());
                }
            }
            VideoSource::File if self.settings.file.is_empty() => {
                self.notice = Some("Choose a video file".to_owned());
            }
            VideoSource::File => {
                self.entries = vec![Entry {
                    name: self.settings.file.clone(),
                    source: EntrySource::File {
                        scope: PICKED_FILE,
                        path: String::new(),
                    },
                }];
            }
            VideoSource::Folders => {
                let scopes: Vec<usize> = self
                    .settings
                    .folders()
                    .into_iter()
                    .map(|(scope, _)| scope)
                    .collect();
                if scopes.is_empty() {
                    self.notice = Some("Choose a folder of videos".to_owned());
                } else {
                    self.list = ListState::Discovering(Box::new(Discovery::new(
                        &scopes,
                        self.settings.recursive,
                    )));
                    self.notice = Some("Scanning video folders...".to_owned());
                }
            }
        }
    }

    /// The line the Settings panel shows instead of the playing clip, if any.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn pump_discovery(&mut self) {
        let ListState::Discovering(discovery) = &mut self.list else {
            return;
        };
        if discovery.step(self.io.as_mut()) != Progress::Done {
            return;
        }
        let ListState::Discovering(discovery) = std::mem::replace(&mut self.list, ListState::Ready)
        else {
            return;
        };
        let found = discovery.finish();
        self.entries = found.entries;
        self.notes = found.notes;
        self.notice = found.error;
    }

    fn seed_scheduler(&mut self, now: SystemTime) {
        if self.seeded || self.entries.is_empty() {
            return;
        }
        let clock = now
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        self.scheduler = Scheduler::new(&self.settings, resolve_seed(self.settings.seed, clock));
        self.scheduler.set_len(self.entries.len());
        self.seeded = true;
    }

    fn fail(&mut self, entry: usize, message: &str, wall: Duration) {
        let name = self
            .entries
            .get(entry)
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        self.notice = Some(format!("{name}: {message}"));
        self.scheduler.mark_failed();
        self.stage = Stage::Idle;
        self.failures += 1;
        if self.failures >= self.entries.len().max(1) {
            self.failures = 0;
            self.retry_at = Some(wall + RETRY_AFTER);
        }
    }

    fn begin(&mut self, entry: usize, opened: Opened) {
        let info = opened.info;
        let item = self.scheduler.excerpt(entry, info.duration_millis);
        if item.start_millis > 0 {
            if let Err(message) = self.io.seek(opened.video, item.start_millis) {
                self.io.close(opened.video);
                self.scheduler.mark_failed();
                self.notice = Some(message);
                self.stage = Stage::Idle;
                return;
            }
        }
        let fps = f64::from(info.frames_per_second);
        let fps = if fps.is_finite() && fps > 0.5 {
            fps
        } else {
            FALLBACK_FPS
        };
        let name = self
            .entries
            .get(entry)
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        self.stage = Stage::Playing(Box::new(Playing {
            video: opened.video,
            info,
            item,
            name,
            frame_millis: 1000.0 / fps,
            pulled: 0,
            origin: None,
            held: None,
            shown_any: false,
        }));
    }

    fn open_next(&mut self, wall: Duration) {
        if self.retry_at.is_some_and(|at| wall < at) {
            return;
        }
        self.retry_at = None;
        let Some(entry_index) = self.scheduler.next_entry() else {
            return;
        };
        let Some(entry) = self.entries.get(entry_index).cloned() else {
            return;
        };
        match entry.source {
            EntrySource::File { scope, path } => match self.io.open_file(scope, &path) {
                Ok(opened) => self.begin(entry_index, opened),
                Err(message) => self.fail(entry_index, &message, wall),
            },
            EntrySource::Catalogue(index) => match self.clips.get(index).cloned() {
                Some(clip) => {
                    self.notice = Some(format!("Downloading: {}", clip.title));
                    self.stage = Stage::Downloading {
                        entry: entry_index,
                        download: Box::new(Download::new(clip)),
                    };
                }
                None => self.fail(entry_index, "not in the catalogue", wall),
            },
        }
    }

    fn pump_download(&mut self, wall: Duration) {
        let Stage::Downloading { entry, download } = &mut self.stage else {
            return;
        };
        let entry = *entry;
        match download.step(self.io.as_mut()) {
            Step::Working => {
                self.notice = Some(format!(
                    "Downloading: {} ({}%)",
                    download.clip().title,
                    download.percent()
                ));
            }
            Step::Failed(message) => self.fail(entry, &message, wall),
            Step::Done(bytes) => {
                let mime = download.clip().mime.clone();
                match self.io.open_bytes(bytes, &mime) {
                    Ok(opened) => self.begin(entry, opened),
                    Err(message) => self.fail(entry, &message, wall),
                }
            }
        }
    }

    fn finish_clip(&mut self) {
        if let Stage::Playing(playing) = std::mem::replace(&mut self.stage, Stage::Idle) {
            self.io.close(playing.video);
        }
    }

    /// Pulls the frames that are due at `wall`; `canvas` is the dot canvas (columns * 2 by
    /// rows * 4). Returns the newest due frame.
    fn pump_playing(&mut self, wall: Duration, canvas: (u32, u32)) -> Option<RawFrame> {
        let speed = if self.settings.mode == PlaybackMode::Slowed {
            f64::from(self.settings.slowed_percent) / 100.0
        } else {
            1.0
        };
        let Stage::Playing(playing) = &mut self.stage else {
            return None;
        };
        let size = request_size(
            (playing.info.width, playing.info.height),
            canvas,
            self.settings.fit,
        );
        if playing.held.as_ref().is_some_and(|held| held.size != size) {
            playing.held = None;
        }
        let mut newest: Option<Held> = None;
        let mut ended = false;
        let mut failure: Option<String> = None;
        let mut lag = Duration::ZERO;
        for _ in 0..MAX_PULLS {
            let frame = match playing.held.take() {
                Some(held) => held,
                None => match self.io.next_frame(playing.video, size.0, size.1) {
                    Ok(FramePull::Frame(data)) => {
                        let held = Held {
                            offset_millis: playing.pulled as f64 * playing.frame_millis,
                            data,
                            size,
                        };
                        playing.pulled += 1;
                        held
                    }
                    Ok(FramePull::Pending) => break,
                    Ok(FramePull::End) => {
                        ended = true;
                        break;
                    }
                    Err(message) => {
                        failure = Some(message);
                        break;
                    }
                },
            };
            if playing
                .item
                .limit_millis
                .is_some_and(|limit| frame.offset_millis >= limit as f64)
            {
                ended = true;
                break;
            }
            let origin = *playing.origin.get_or_insert_with(|| {
                wall.saturating_sub(Duration::from_secs_f64(
                    frame.offset_millis / 1000.0 / speed,
                ))
            });
            let due = origin + Duration::from_secs_f64(frame.offset_millis / 1000.0 / speed);
            if due <= wall {
                lag = wall - due;
                newest = Some(frame);
            } else {
                playing.held = Some(frame);
                break;
            }
        }
        if lag > MAX_LAG {
            // Decoding cannot keep up: move the clock instead of fast-forwarding.
            playing.origin = playing.origin.map(|origin| origin + lag);
        }
        let shown = newest.map(|frame| {
            playing.shown_any = true;
            RawFrame {
                rgb: frame.data,
                size: frame.size,
                name: playing.name.clone(),
                source_seconds: (playing.item.start_millis as f64 + frame.offset_millis) / 1000.0,
                duration_seconds: playing
                    .info
                    .duration_millis
                    .map(|millis| millis as f64 / 1000.0),
            }
        });
        if shown.is_some() {
            self.failures = 0;
            self.notice = None;
        }
        if let Some(message) = failure {
            let shown_any = playing.shown_any || playing.pulled > 0;
            self.finish_clip();
            if !shown_any {
                self.scheduler.mark_failed();
                self.notice = Some(message);
                self.failures += 1;
                if self.failures >= self.entries.len().max(1) {
                    self.failures = 0;
                    self.retry_at = Some(wall + RETRY_AFTER);
                }
            }
        } else if ended {
            self.finish_clip();
        }
        shown
    }

    /// Advances the machine by one frame; returns the picture to show, if a new one is due.
    pub fn advance(
        &mut self,
        wall: Duration,
        now: SystemTime,
        canvas: (u32, u32),
    ) -> Option<RawFrame> {
        self.pump_discovery();
        self.seed_scheduler(now);
        match self.stage {
            Stage::Idle => {
                if matches!(self.list, ListState::Ready) && !self.scheduler.is_empty() {
                    self.open_next(wall);
                }
                None
            }
            Stage::Downloading { .. } => {
                self.pump_download(wall);
                None
            }
            Stage::Playing(_) => self.pump_playing(wall, canvas),
        }
    }

    /// Short remark about the folders just read (a cut list, an unreadable folder).
    pub fn note(&self) -> Option<&str> {
        self.notes.first().map(String::as_str)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.finish_clip();
    }
}
