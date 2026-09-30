//! Images: colored Braille pictures from a file, folders/globs or https URLs,
//! shown as a slideshow with cross-fades and slow Ken-Burns motion.
//!
//! This replaces the client's former single "static image" background and
//! keeps its colour math (see `adjust`). Loading, decoding and discovery run
//! on one owned worker thread; `render` never blocks.
//!
//! Data sources: only files the user names and https URLs the user (or the
//! built-in list) names; downloads go through `source::fetch_cached`
//! (User-Agent, per-host spacing, size and time bounds, on-disk cache).
//!
//! Redraw cadence: `frames_per_second()` is 1 when nothing can change
//! (one image, motion off, fully loaded, or nothing to show) and 12 while a
//! move, cross-fade, slideshow or load is under way. The value is re-read
//! after every render, so hosts should ask again each frame.

mod adjust;
mod decode;
mod discover;
mod motion;
mod render;
mod settings;
mod slideshow;
mod worker;

// The setting enums are part of `ImagesSettings`' public fields; lib.rs only
// re-exports the struct today, so the re-export is unreachable until the host
// crate names these types.
#[allow(unused_imports)]
pub use settings::{
    Easing, FitMode, ImagePreset, ImageSource, ImagesMode, ImagesSettings, Motion, SlideOrder,
};

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use adjust::Adjustment;
use decode::{DecodeLimits, DecodedImage, Lru};
use motion::{hash64, ping_pong, pose_at, view_rect};
use render::{layers_key, render_layers, render_placeholder, Layer};
use slideshow::{build_order, Availability, Scheduler, Slide, Timing};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use worker::{Entry, ImageLoader, ListSpec, LoadRequest, LoaderConfig, WorkerEvent};

/// Decoded images are kept at up to this multiple of the dot resolution.
const DECODE_OVERSAMPLE: u32 = 3;
const MIN_DECODE: (u32, u32) = (1024, 576);
const MAX_DECODE: (u32, u32) = (3840, 2160);
/// Current + outgoing + two prefetched images, with a little slack.
const LRU_CAPACITY: usize = 6;
/// How many images ahead of the current one are decoded in advance.
const PREFETCH_AHEAD: usize = 2;
const ACTIVE_FPS: u32 = 12;
const IDLE_FPS: u32 = 1;

struct FrameCache {
    key: Vec<u64>,
    dots: Vec<f32>,
    colors: Vec<[u8; 3]>,
}

pub struct ImagesScene {
    settings: ImagesSettings,
    adjustment: Adjustment,
    timing: Timing,
    strength: f32,
    loader: ImageLoader,
    entries: Option<Vec<Entry>>,
    list_error: Option<String>,
    notes: Vec<String>,
    order: Vec<usize>,
    images: Lru<Arc<DecodedImage>>,
    pending: HashSet<usize>,
    failed: HashMap<usize, String>,
    last_error: Option<String>,
    scheduler: Scheduler,
    generation: u32,
    decode_target: Option<(u32, u32)>,
    frame_cache: Option<FrameCache>,
    status: Option<String>,
    animating: bool,
}

impl ImagesScene {
    pub fn new(settings: &ImagesSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let loader = ImageLoader::start(LoaderConfig {
            list: ListSpec::from_settings(&settings),
            cache_dir: env.cache_dir.join("images"),
            limits: DecodeLimits::default(),
        });
        let status = Some(
            match settings.mode {
                ImagesMode::Single => "Loading image...",
                ImagesMode::Folders => "Scanning folders...",
                ImagesMode::UrlList => "Loading image list...",
            }
            .to_owned(),
        );
        Self {
            adjustment: Adjustment::from_settings(&settings),
            timing: Timing::new(
                settings.display_seconds,
                if settings.is_slideshow_mode() {
                    settings.transition_seconds
                } else {
                    0
                },
            ),
            strength: f32::from(settings.motion_strength_percent) / 100.0,
            settings,
            loader,
            entries: None,
            list_error: None,
            notes: Vec::new(),
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
        }
    }

    fn pump_events(&mut self, seed_time: std::time::SystemTime) {
        while let Some(event) = self.loader.try_recv() {
            match event {
                WorkerEvent::List { entries, notes } => {
                    self.notes = notes;
                    let seed = if self.settings.shuffle_seed == 0 {
                        seed_time
                            .duration_since(UNIX_EPOCH)
                            .map(|elapsed| elapsed.as_secs())
                            .unwrap_or(0)
                    } else {
                        u64::from(self.settings.shuffle_seed)
                    };
                    self.order = build_order(entries.len(), self.settings.order, seed);
                    self.entries = Some(entries);
                }
                WorkerEvent::ListFailed(message) => self.list_error = Some(message),
                WorkerEvent::Loaded {
                    index,
                    generation,
                    image,
                } => {
                    if generation == self.generation {
                        self.pending.remove(&index);
                        self.images.insert(index, image);
                    }
                }
                WorkerEvent::Failed {
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

    /// Decode resolution follows the first frame size; a much larger frame
    /// (window enlarged) restarts loading at the higher resolution.
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
                self.frame_cache = None;
                self.decode_target = Some(wanted);
            }
            Some(_) => {}
        }
    }

    fn entry_at(&self, position: usize) -> Option<(usize, &Entry)> {
        let index = *self.order.get(position)?;
        Some((index, self.entries.as_ref()?.get(index)?))
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

    /// Ask the worker for the current image and the next few.
    fn request_needed(&mut self) {
        let len = self.order.len();
        let Some((max_width, max_height)) = self.decode_target else {
            return;
        };
        let mut positions = Vec::new();
        match self.scheduler.current() {
            Some(current) => {
                positions.push(current.position);
                if len > 1 {
                    positions
                        .extend((1..=PREFETCH_AHEAD).map(|step| (current.position + step) % len));
                }
            }
            None => positions.extend(0..len.min(PREFETCH_AHEAD)),
        }
        for position in positions {
            let Some(index) = self.order.get(position).copied() else {
                continue;
            };
            if self.images.contains(index)
                || self.pending.contains(&index)
                || self.failed.contains_key(&index)
            {
                continue;
            }
            self.pending.insert(index);
            self.loader.request(LoadRequest {
                index,
                generation: self.generation,
                max_width,
                max_height,
            });
        }
    }

    fn slide_view(
        &self,
        slide: &Slide,
        image: &DecodedImage,
        now: Duration,
        screen_aspect: f32,
    ) -> motion::View {
        let playable = self.order.len().saturating_sub(self.failed.len());
        let progress = if playable <= 1 {
            // A lone image moves back and forth so it never jumps.
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
        let Some(entries) = &self.entries else {
            return self.status.clone();
        };
        let Some(current) = self.scheduler.current() else {
            if !entries.is_empty() && self.failed.len() >= entries.len() {
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
        self.pump_events(frame.now);
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        frame.cell_colors.fill([0, 0, 0]);
        self.update_decode_target(frame);

        let now = frame.time;
        let mut scheduler = std::mem::take(&mut self.scheduler);
        scheduler.update(now, &self.timing, self.order.len(), |position| {
            self.availability(position)
        });
        self.scheduler = scheduler;
        self.request_needed();

        let snapshot = self.scheduler.snapshot(now, &self.timing);
        let mut images: Vec<(Slide, Arc<DecodedImage>, f32)> = Vec::new();
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
            self.animating = self.entries.is_none()
                || (self.list_error.is_none() && self.failed.len() < self.order.len());
            if self.animating {
                render_placeholder(frame);
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

        self.animating = self.settings.motion != Motion::None
            || self.scheduler.is_fading()
            || self.order.len().saturating_sub(self.failed.len()) > 1;
        self.refresh_status();
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

#[cfg(test)]
mod tests;
