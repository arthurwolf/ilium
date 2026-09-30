use super::{AnimationFrame, AnimationSettings};
use std::time::{Duration, Instant};

pub(crate) const CACHE_FPS: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationCacheStatus {
    pub completed_frames: usize,
    pub total_frames: usize,
    pub estimated_bytes: usize,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
    pub is_ready: bool,
}

impl Default for AnimationCacheStatus {
    fn default() -> Self {
        Self {
            completed_frames: 0,
            total_frames: 0,
            estimated_bytes: 0,
            elapsed: Duration::ZERO,
            eta: None,
            is_ready: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct AnimationLoopCache {
    width: u16,
    height: u16,
    settings: Option<AnimationSettings>,
    frames: Vec<Vec<u8>>,
    building_frames: Vec<Vec<u8>>,
    next_frame: usize,
    started_at: Option<Instant>,
    status: AnimationCacheStatus,
}

impl AnimationLoopCache {
    pub fn begin(&mut self, settings: &AnimationSettings, width: u16, height: u16) {
        let settings = settings.normalized();
        if self.settings.as_ref() == Some(&settings) && self.width == width && self.height == height
        {
            return;
        }
        self.width = width;
        self.height = height;
        self.settings = Some(settings.clone());
        self.building_frames.clear();
        self.next_frame = 0;
        self.started_at = Some(Instant::now());
        self.status = AnimationCacheStatus {
            completed_frames: 0,
            total_frames: usize::from(settings.loop_seconds) * CACHE_FPS as usize,
            estimated_bytes: settings.estimated_loop_bytes(width, height),
            ..Default::default()
        };
    }

    pub fn step(
        &mut self,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        frame_budget: usize,
    ) -> bool {
        self.begin(settings, width, height);
        if self.status.is_ready || width == 0 || height == 0 {
            return self.status.is_ready;
        }
        let total_frames = self.status.total_frames;
        let mut generator = AnimationFrame::default();
        let end = (self.next_frame + frame_budget.max(1)).min(total_frames);
        while self.next_frame < end {
            let elapsed = Duration::from_nanos(
                (self.next_frame as u128 * 1_000_000_000 / u128::from(CACHE_FPS)) as u64,
            );
            generator.render(settings, width, height, elapsed);
            self.building_frames.push(generator.packed_cells().to_vec());
            self.next_frame += 1;
        }
        let elapsed = self
            .started_at
            .map_or(Duration::ZERO, |started| started.elapsed());
        self.status.completed_frames = self.next_frame;
        self.status.elapsed = elapsed;
        self.status.eta = if self.next_frame >= total_frames {
            Some(Duration::ZERO)
        } else if self.next_frame == 0 {
            None
        } else {
            Some(elapsed.mul_f64((total_frames - self.next_frame) as f64 / self.next_frame as f64))
        };
        if self.next_frame >= total_frames {
            self.frames = std::mem::take(&mut self.building_frames);
            self.status.is_ready = true;
        }
        self.status.is_ready
    }

    pub fn status(&self) -> AnimationCacheStatus {
        self.status
    }

    pub fn frame_index(&self, elapsed: Duration) -> usize {
        if self.frames.is_empty() {
            return 0;
        }
        ((elapsed.as_nanos() * u128::from(CACHE_FPS)) / 1_000_000_000) as usize % self.frames.len()
    }

    pub fn copy_frame_into(&self, elapsed: Duration, frame: &mut AnimationFrame) -> bool {
        let Some(cells) = self.frames.get(self.frame_index(elapsed)) else {
            return false;
        };
        frame.load_packed_cells(self.width, self.height, cells);
        true
    }
}
