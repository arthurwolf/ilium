//! Images: coloured Braille pictures from a picked file, picked folders or one of four built-in
//! pictures, shown as a slideshow with cross-fades and slow Ken-Burns motion.
//!
//! The scene runs as a state machine advanced once per frame. A picked file or folder is read
//! through the host's read-only `picked/<n>` scopes (`source`), a built-in picture through the
//! host `http` import, and decoding goes through the host codec; the box-filter downscale to the
//! decode target runs in bounded slices (`loader`, `picture`). The colour treatment is the
//! shared look's: the scene only gives the picture one fixed quiet tone (`adjust`).
//!
//! PALETTE (future plugin contract): `env.palette` is the shared look's current palette. This
//! scene follows it natively: its cell colours are mapped onto the palette by brightness
//! (`ScenePalette::recolor_cells`) as each frame is produced, and `Scene::set_palette` delivers
//! later changes (applied from the next render). With no palette the colours are untouched.
//!
//! Redraw cadence: `frames_per_second()` is 1 when nothing can change (one picture, motion off,
//! fully loaded, or nothing to show) and 12 while a move, cross-fade, slideshow or load is under
//! way. The value is re-read after every render.

mod adjust;
mod loader;
mod lru;
mod motion;
mod picture;
mod render;
mod settings;
mod slideshow;
mod source;
#[cfg(test)]
mod tests;

use crate::control::SceneSettings;
use crate::kit::scene::{Frame, Scene, SceneEnv};
use crate::kit::style::ScenePalette;
use adjust::Adjustment;
use loader::{Event, LoadRequest, Loader};
use lru::Lru;
use motion::{hash64, ping_pong, pose_at, view_rect};
use picture::Picture;
use render::{layers_key, render_layers, render_placeholder, Layer};
pub use settings::ImagesSettings;
use settings::{ImagesMode, Motion, PICKED_FILE};
use slideshow::{build_order, Availability, Scheduler, Slide, Timing};
use source::{Discovery, Entry, EntrySource, HostIo, ImageIo, Progress};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

/// Decoded pictures are kept at up to this multiple of the dot resolution.
const DECODE_OVERSAMPLE: u32 = 3;
const MIN_DECODE: (u32, u32) = (1024, 576);
const MAX_DECODE: (u32, u32) = (3840, 2160);
/// Current + outgoing + two prefetched pictures, with a little slack.
const LRU_CAPACITY: usize = 6;
/// How many pictures ahead of the current one are loaded in advance.
const PREFETCH_AHEAD: usize = 2;
const ACTIVE_FPS: u32 = 12;
const IDLE_FPS: u32 = 1;

/// Builds the Images scene from the spec of the host.
pub fn create(spec: &serde_json::Value, env: &SceneEnv) -> Box<dyn Scene> {
    let settings: ImagesSettings = crate::kit::settings::from_spec(spec);
    Box::new(ImagesScene::new(&settings, env, Box::new(HostIo)))
}

/// The last frame's dots and colours, reused while nothing about it changed.
struct FrameCache {
    key: Vec<u64>,
    dots: Vec<f32>,
    colors: Vec<[u8; 3]>,
}

/// Where the list of pictures stands.
enum ListState {
    /// Folders are being listed, a few per frame.
    Discovering(Box<Discovery>),
    /// The list is final (possibly empty, with `list_error` saying why).
    Ready,
}

pub struct ImagesScene {
    settings: ImagesSettings,
    adjustment: Adjustment,
    timing: Timing,
    strength: f32,
    io: Box<dyn ImageIo>,
    loader: Loader,
    list: ListState,
    entries: Vec<Entry>,
    notes: Vec<String>,
    list_error: Option<String>,
    order: Vec<usize>,
    images: Lru<Arc<Picture>>,
    pending: HashSet<usize>,
    failed: HashMap<usize, String>,
    last_error: Option<String>,
    scheduler: Scheduler,
    generation: u32,
    decode_target: Option<(u32, u32)>,
    frame_cache: Option<FrameCache>,
    status: Option<String>,
    animating: bool,
    palette: ScenePalette,
}

impl ImagesScene {
    pub fn new(settings: &ImagesSettings, env: &SceneEnv, io: Box<dyn ImageIo>) -> Self {
        let settings = settings.normalized();
        let (list, entries, list_error, status) = Self::initial_list(&settings);
        Self {
            adjustment: Adjustment::standard(),
            timing: Timing::new(
                settings.display_seconds,
                settings.effective_transition_seconds(),
            ),
            strength: f32::from(settings.motion_strength_percent) / 100.0,
            settings,
            io,
            loader: Loader::default(),
            list,
            entries,
            notes: Vec::new(),
            list_error,
            order: Vec::new(),
            images: Lru::new(LRU_CAPACITY),
            pending: HashSet::new(),
            failed: HashMap::new(),
            last_error: None,
            scheduler: Scheduler::default(),
            generation: 0,
            decode_target: None,
            frame_cache: None,
            status,
            animating: true,
            palette: env.palette.clone(),
        }
    }

    /// The first state of the list: one entry for a single picture, a discovery for folders.
    fn initial_list(
        settings: &ImagesSettings,
    ) -> (ListState, Vec<Entry>, Option<String>, Option<String>) {
        match settings.mode {
            ImagesMode::Builtin => {
                let (label, url) = settings.source.picture();
                (
                    ListState::Ready,
                    vec![Entry {
                        name: label.to_owned(),
                        source: EntrySource::Url(url.to_owned()),
                    }],
                    None,
                    Some("Loading image...".to_owned()),
                )
            }
            ImagesMode::File if settings.file.is_empty() => {
                let message = "Choose an image file".to_owned();
                (
                    ListState::Ready,
                    Vec::new(),
                    Some(message.clone()),
                    Some(message),
                )
            }
            ImagesMode::File => (
                ListState::Ready,
                vec![Entry {
                    name: settings.file.clone(),
                    source: EntrySource::File {
                        scope: PICKED_FILE,
                        path: String::new(),
                    },
                }],
                None,
                Some("Loading image...".to_owned()),
            ),
            ImagesMode::Folders => {
                let roots: Vec<(usize, String)> = settings
                    .folders()
                    .into_iter()
                    .map(|(scope, text)| (scope, text.to_owned()))
                    .collect();
                (
                    ListState::Discovering(Box::new(Discovery::new(&roots, settings.recursive))),
                    Vec::new(),
                    None,
                    Some("Scanning folders...".to_owned()),
                )
            }
        }
    }

    /// Lists a few folders; when the walk ends, builds the final list and the show order.
    fn pump_discovery(&mut self, seed_time: std::time::SystemTime) {
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
        self.list_error = found.error;
        self.start_order(seed_time);
    }

    fn start_order(&mut self, seed_time: std::time::SystemTime) {
        let seed = if self.settings.shuffle_seed == 0 {
            seed_time
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0)
        } else {
            u64::from(self.settings.shuffle_seed)
        };
        self.order = build_order(self.entries.len(), self.settings.order, seed);
    }

    fn pump_loader(&mut self, wall: Duration) {
        // One finished load per frame: the next stage waits for the next frame.
        if let Some(event) = self.loader.step(self.io.as_mut(), wall) {
            match event {
                Event::Loaded {
                    index,
                    generation,
                    image,
                } => {
                    if generation == self.generation {
                        self.pending.remove(&index);
                        self.images.insert(index, image);
                    }
                }
                Event::Failed {
                    index,
                    generation,
                    message,
                } => {
                    if generation == self.generation {
                        self.pending.remove(&index);
                        self.last_error = Some(message.clone());
                        self.failed.insert(index, message);
                    }
                }
            }
        }
    }

    /// Decode resolution follows the first frame size; a much larger frame (window enlarged)
    /// restarts loading at the higher resolution.
    fn update_decode_target(&mut self, frame: &Frame<'_>) {
        let wanted = (
            (u32::from(frame.width) * 2 * DECODE_OVERSAMPLE).clamp(MIN_DECODE.0, MAX_DECODE.0),
            (u32::from(frame.height) * 4 * DECODE_OVERSAMPLE).clamp(MIN_DECODE.1, MAX_DECODE.1),
        );
        match self.decode_target {
            None => self.decode_target = Some(wanted),
            Some((width, height)) if wanted.0 > width * 3 / 2 || wanted.1 > height * 3 / 2 => {
                self.generation = self.generation.wrapping_add(1);
                self.images.clear();
                self.pending.clear();
                self.loader.cancel();
                self.frame_cache = None;
                self.decode_target = Some(wanted);
            }
            Some(_) => {}
        }
    }

    fn entry_at(&self, position: usize) -> Option<(usize, &Entry)> {
        let index = *self.order.get(position)?;
        Some((index, self.entries.get(index)?))
    }

    fn availability(&self, position: usize) -> Availability {
        let Some(index) = self.order.get(position).copied() else {
            return Availability::Failed;
        };
        if self.images.contains(index) {
            Availability::Ready
        } else if self.failed.contains_key(&index) {
            Availability::Failed
        } else {
            Availability::Pending
        }
    }

    /// Asks the loader for the first of the current picture and the next few that is missing.
    fn request_needed(&mut self) {
        if self.loader.is_busy() {
            return;
        }
        let len = self.order.len();
        let Some((max_width, max_height)) = self.decode_target else {
            return;
        };
        let mut positions = [None; PREFETCH_AHEAD + 1];
        match self.scheduler.current() {
            Some(current) => {
                positions[0] = Some(current.position);
                if len > 1 {
                    for (step, position) in positions.iter_mut().enumerate().skip(1) {
                        *position = Some((current.position + step) % len);
                    }
                }
            }
            None => {
                for (index, position) in positions
                    .iter_mut()
                    .take(len.min(PREFETCH_AHEAD))
                    .enumerate()
                {
                    *position = Some(index);
                }
            }
        }
        for position in positions.into_iter().flatten() {
            let Some(index) = self.order.get(position).copied() else {
                continue;
            };
            if self.images.contains(index)
                || self.pending.contains(&index)
                || self.failed.contains_key(&index)
            {
                continue;
            }
            let Some(entry) = self.entries.get(index).cloned() else {
                continue;
            };
            let accepted = self.loader.request(LoadRequest {
                index,
                generation: self.generation,
                entry,
                max_width,
                max_height,
            });
            if accepted {
                self.pending.insert(index);
            }
            return;
        }
    }

    fn slide_view(
        &self,
        slide: &Slide,
        image: &Picture,
        now: Duration,
        screen_aspect: f32,
    ) -> motion::View {
        let playable = self.order.len().saturating_sub(self.failed.len());
        let progress = if playable <= 1 {
            // A lone picture moves back and forth so it never jumps.
            ping_pong(now.as_secs_f32(), self.timing.display.as_secs_f32())
        } else {
            slide.progress(now, &self.timing)
        };
        let variant = self
            .entry_at(slide.position)
            .map(|(_, entry)| {
                entry
                    .key()
                    .bytes()
                    .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                        hash64(hash ^ u64::from(byte))
                    })
            })
            .unwrap_or(0);
        let pose = pose_at(
            self.settings.motion,
            self.strength,
            self.settings.easing,
            progress,
            variant,
            slide.serial % 2 == 1,
        );
        view_rect(self.settings.fit, image.aspect(), screen_aspect, pose)
    }

    fn refresh_status(&mut self) {
        self.status = self.compute_status();
    }

    fn compute_status(&self) -> Option<String> {
        if let Some(error) = &self.list_error {
            return Some(error.clone());
        }
        if matches!(self.list, ListState::Discovering(_)) {
            return self.status.clone();
        }
        let Some(current) = self.scheduler.current() else {
            if !self.entries.is_empty() && self.failed.len() >= self.entries.len() {
                return Some(
                    self.last_error
                        .clone()
                        .unwrap_or_else(|| "No readable images".to_owned()),
                );
            }
            return Some("Loading image...".to_owned());
        };
        let mut unreadable = if self.failed.is_empty() {
            String::new()
        } else {
            format!(" ({} unreadable)", self.failed.len())
        };
        if let Some(note) = self.notes.first() {
            unreadable.push_str(&format!(" [{note}]"));
        }
        let (_, entry) = self.entry_at(current.position)?;
        if self.order.len() > 1 {
            Some(format!(
                "Slide {}/{}: {}{unreadable}",
                current.position + 1,
                self.order.len(),
                entry.name
            ))
        } else {
            None
        }
    }
}

impl Scene for ImagesScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        if self.order.is_empty() && !self.entries.is_empty() {
            // A single picture: the list is final from the start.
            self.start_order(frame.now);
        }
        self.pump_discovery(frame.now);
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        frame.cell_colors.fill([0, 0, 0]);
        self.update_decode_target(frame);
        self.pump_loader(frame.wall);

        let now = frame.time;
        let mut scheduler = std::mem::take(&mut self.scheduler);
        scheduler.update(now, &self.timing, self.order.len(), |position| {
            self.availability(position)
        });
        self.scheduler = scheduler;
        self.request_needed();

        let snapshot = self.scheduler.snapshot(now, &self.timing);
        let mut images: Vec<(Slide, Arc<Picture>, f32)> = Vec::with_capacity(2);
        if let Some(snapshot) = snapshot {
            let mut take = |slide: Slide, weight: f32, images: &mut Vec<_>| {
                let index = self.order.get(slide.position).copied();
                if let Some(image) = index.and_then(|index| self.images.get(index)).cloned() {
                    images.push((slide, image, weight));
                }
            };
            if let Some(outgoing) = snapshot.outgoing {
                take(outgoing, 1.0 - snapshot.fade, &mut images);
            }
            take(snapshot.current, snapshot.fade, &mut images);
        }

        if images.is_empty() {
            self.animating = self.list_error.is_none()
                && (matches!(self.list, ListState::Discovering(_))
                    || self.failed.len() < self.order.len());
            if self.animating {
                render_placeholder(frame);
                self.palette.recolor_cells(frame.cell_colors);
            }
            self.refresh_status();
            return;
        }

        let screen_aspect = f32::from(frame.width) * 2.0 / (f32::from(frame.height) * 4.0);
        let layers: Vec<Layer<'_>> = images
            .iter()
            .map(|(slide, image, weight)| Layer {
                image,
                view: self.slide_view(slide, image, now, screen_aspect),
                weight: *weight,
            })
            .collect();
        let key = layers_key(&layers, frame.width, frame.height);
        match &self.frame_cache {
            Some(cache)
                if cache.key == key
                    && cache.dots.len() == frame.raster.dots.len()
                    && cache.colors.len() == frame.cell_colors.len() =>
            {
                frame.raster.dots.copy_from_slice(&cache.dots);
                frame.cell_colors.copy_from_slice(&cache.colors);
            }
            _ => {
                render_layers(frame, &layers, &self.adjustment);
                self.frame_cache = Some(FrameCache {
                    key,
                    dots: frame.raster.dots.clone(),
                    colors: frame.cell_colors.clone(),
                });
            }
        }
        drop(layers);
        // The frame cache keeps the picture's own colours; the palette is applied after it so
        // a palette change needs no cache invalidation.
        self.palette.recolor_cells(frame.cell_colors);

        self.animating = self.settings.motion != Motion::None
            || self.scheduler.is_fading()
            || self.order.len().saturating_sub(self.failed.len()) > 1;
        self.refresh_status();
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn uses_cell_colors(&self) -> bool {
        true
    }

    fn frames_per_second(&self) -> u32 {
        if self.animating {
            ACTIVE_FPS
        } else {
            IDLE_FPS
        }
    }

    fn status(&self) -> Option<String> {
        self.status.clone()
    }
}
