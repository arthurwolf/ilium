//! Nonblocking presentation reads; automation explicitly requests current data.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::query::TerminalQueryResponder;

type Parser = vt100::Parser<TerminalQueryResponder>;

struct ScreenFrame {
    screen: vt100::Screen,
    generation: u64,
}

#[derive(Clone)]
pub(crate) struct ScreenReader {
    parser: Arc<RwLock<Parser>>,
    generation: Arc<AtomicU64>,
    retained: Arc<Mutex<Arc<ScreenFrame>>>,
}

impl ScreenReader {
    pub(crate) fn new(parser: Arc<RwLock<Parser>>, generation: Arc<AtomicU64>) -> Self {
        // Constructed before the owner starts mutating the parser.
        let retained = Arc::new(ScreenFrame {
            screen: parser
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .screen()
                .clone(),
            generation: generation.load(Ordering::Acquire),
        });
        Self {
            parser,
            generation,
            retained: Arc::new(Mutex::new(retained)),
        }
    }

    /// Called with the owner's parser write guard, before native resize can stall.
    /// Output processing never copies a full screen into this fallback.
    pub(crate) fn retain_before_resize(&self, screen: &vt100::Screen) {
        let frame = Arc::new(ScreenFrame {
            screen: screen.clone(),
            generation: self.generation.load(Ordering::Acquire),
        });
        *self
            .retained
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = frame;
    }

    pub(crate) fn with_frame<R>(&self, f: impl FnOnce(&vt100::Screen, u64) -> R) -> R {
        if let Ok(parser) = self.parser.try_read() {
            return f(parser.screen(), self.generation.load(Ordering::Acquire));
        }
        let frame = Arc::clone(
            &self
                .retained
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        // No fallback lock is held while running caller code.
        f(&frame.screen, frame.generation)
    }

    pub(crate) fn try_with_frame<R>(&self, f: impl FnOnce(&vt100::Screen, u64) -> R) -> Option<R> {
        let parser = self.parser.try_read().ok()?;
        Some(f(parser.screen(), self.generation.load(Ordering::Acquire)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_frame_stays_consistent_and_current_reads_fail_closed() {
        let parser = Arc::new(RwLock::new(Parser::new_with_callbacks(
            4,
            20,
            0,
            TerminalQueryResponder::new(),
        )));
        let generation = Arc::new(AtomicU64::new(0));
        let reader = ScreenReader::new(Arc::clone(&parser), Arc::clone(&generation));
        let mut writer = parser.write().unwrap();
        writer.process(b"\x1b[2mold\x1b[0m");
        generation.store(7, Ordering::Release);
        reader.retain_before_resize(writer.screen());
        writer.process(b"new");
        generation.store(8, Ordering::Release);
        assert!(reader.try_with_frame(|_, _| ()).is_none());
        reader.with_frame(|screen, revision| {
            assert_eq!(revision, 7);
            assert_eq!(screen.contents(), "old");
            assert_eq!(screen.cursor_position(), (0, 3));
            assert_eq!(screen.size(), (4, 20));
            assert!(screen.cell(0, 0).unwrap().dim());
        });
        drop(writer);
        assert_eq!(
            reader.try_with_frame(|screen, revision| (screen.contents(), revision)),
            Some(("oldnew".into(), 8))
        );
    }
}
