//! A fake hosted scene for tests: no network, ffmpeg, audio device or clock.

use super::{AmbientHost, SceneFactory};
use ilium_ambient::{Frame, Scene};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};

/// Shared observation and control surface of every fake scene a factory builds.
#[derive(Default)]
pub struct FakeProbe {
    pub constructed: AtomicUsize,
    pub dropped: AtomicUsize,
    pub rendered: AtomicUsize,
    pub last_time_ms: AtomicU64,
    pub last_wall_ms: AtomicU64,
    pub uses_colors: AtomicBool,
    pub fps: AtomicU32,
    pub panic_next_render: AtomicBool,
    pub status: Mutex<Option<String>>,
}

impl FakeProbe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            fps: AtomicU32::new(12),
            ..Self::default()
        })
    }

    pub fn alive(&self) -> usize {
        self.constructed.load(Ordering::SeqCst) - self.dropped.load(Ordering::SeqCst)
    }
}

pub struct FakeScene {
    probe: Arc<FakeProbe>,
}

impl Scene for FakeScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.probe.rendered.fetch_add(1, Ordering::SeqCst);
        self.probe
            .last_time_ms
            .store(frame.time.as_millis() as u64, Ordering::SeqCst);
        self.probe
            .last_wall_ms
            .store(frame.wall.as_millis() as u64, Ordering::SeqCst);
        if self.probe.panic_next_render.swap(false, Ordering::SeqCst) {
            panic!("fake scene exploded");
        }
        // A lit top row, a lit dot column every fourth column and one bright
        // column that walks with scene time.
        let width = frame.raster.width.max(1);
        let column = (frame.time.as_millis() / 100) as usize % width;
        for y in 0..frame.raster.height {
            for x in 0..frame.raster.width {
                if x % 4 == 0 || x == column || y == 0 {
                    frame.raster.dots[y * width + x] = 1.0;
                }
            }
        }
        if self.probe.uses_colors.load(Ordering::SeqCst) {
            for (index, color) in frame.cell_colors.iter_mut().enumerate() {
                *color = if index % 2 == 0 {
                    [200, 20, 40]
                } else {
                    [20, 40, 200]
                };
            }
        }
    }

    fn uses_cell_colors(&self) -> bool {
        self.probe.uses_colors.load(Ordering::SeqCst)
    }

    fn frames_per_second(&self) -> u32 {
        self.probe.fps.load(Ordering::SeqCst)
    }

    fn status(&self) -> Option<String> {
        self.probe.status.lock().unwrap().clone()
    }
}

impl Drop for FakeScene {
    fn drop(&mut self) {
        self.probe.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn fake_factory(probe: &Arc<FakeProbe>) -> SceneFactory {
    let probe = Arc::clone(probe);
    Box::new(move |_kind, _settings, _env| {
        probe.constructed.fetch_add(1, Ordering::SeqCst);
        Box::new(FakeScene {
            probe: Arc::clone(&probe),
        })
    })
}

pub fn fake_host(probe: &Arc<FakeProbe>) -> AmbientHost {
    AmbientHost::with_factory(fake_factory(probe))
}
