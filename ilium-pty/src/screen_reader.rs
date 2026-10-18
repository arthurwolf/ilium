//! Nonblocking presentation reads; automation explicitly requests current data.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::query::TerminalQueryResponder;

type Parser = vt100::Parser<TerminalQueryResponder>;

struct ScreenFrame {
    screen: vt100::Screen,
    generation: u64,
}

/// A cheap clone of one PTY's existing screen reader for deferred current reads.
///
/// Obtain this handle and the originating session identity under a short
/// registry lock, then format the screen outside that lock. The handle keeps
/// its original parser alive; it never follows a replacement session.
#[derive(Clone)]
pub struct ScreenReader {
    parser: Arc<RwLock<Parser>>,
    generation: Arc<AtomicU64>,
    resize_epoch: Arc<AtomicU64>,
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
            resize_epoch: Arc::new(AtomicU64::new(0)),
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

    /// Tries the current frame without waiting for parser mutation. Returns
    /// `None` while the owner holds the parser, including during resize; no
    /// retained presentation frame is substituted. The callback holds a read
    /// guard and must not wait for parser mutation or perform blocking I/O.
    pub fn try_with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> Option<R> {
        self.try_with_frame(|screen, _| f(screen))
    }

    /// Nonblocking current-only text, with a preflight before allocation.
    /// Oversized or busy screens stay unavailable rather than becoming partial evidence.
    pub fn try_text_with_limit(&self, limit: usize) -> Option<Result<String, &'static str>> {
        self.try_with_screen(|screen| {
            let (rows, columns) = screen.size();
            if usize::from(rows).saturating_mul(usize::from(columns)) > limit {
                return Err("terminal evidence geometry exceeds limit");
            }
            // Blank cells may become spaces between populated cells. Charge
            // geometry as well as UTF-8 contents before contents() allocates.
            let mut bytes = usize::from(rows)
                .saturating_mul(usize::from(columns))
                .saturating_add(usize::from(rows));
            for row in 0..rows {
                for column in 0..columns {
                    if let Some(cell) = screen.cell(row, column) {
                        bytes = bytes.saturating_add(cell.contents().len());
                        if bytes > limit {
                            return Err("terminal evidence text exceeds limit");
                        }
                    }
                }
            }
            let text = screen.contents();
            if text.len() > limit {
                return Err("terminal evidence text exceeds limit");
            }
            Ok(text)
        })
    }

    /// Reads current screen geometry and its session-local resize epoch under
    /// the same guard. Every successful resize, including a same-size resize,
    /// advances this epoch; ordinary output does not. A busy parser returns
    /// `None`, without substituting a retained frame. The callback must not
    /// block or wait for parser mutation.
    pub fn try_with_screen_and_resize_epoch<R>(
        &self,
        f: impl FnOnce(&vt100::Screen, u64) -> R,
    ) -> Option<R> {
        let parser = self.parser.try_read().ok()?;
        Some(f(
            parser.screen(),
            self.resize_epoch.load(Ordering::Acquire),
        ))
    }

    /// Called only by the owner while holding the parser write guard, after
    /// native geometry was verified and the parser resize was committed.
    pub(crate) fn advance_resize_epoch(&self) {
        self.resize_epoch.fetch_add(1, Ordering::Release);
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
    fn recap_text_is_current_bounded_and_never_uses_retained_resize_frame() {
        let parser = Arc::new(RwLock::new(Parser::new_with_callbacks(
            4,
            20,
            0,
            TerminalQueryResponder::new(),
        )));
        let reader = ScreenReader::new(Arc::clone(&parser), Arc::new(AtomicU64::new(0)));
        assert!(reader.try_text_with_limit(1).unwrap().is_err());
        let mut writer = parser.write().unwrap();
        writer.process(b"selected marker");
        reader.retain_before_resize(writer.screen());
        writer.process(b" current");
        assert!(reader.try_text_with_limit(1024).is_none());
        drop(writer);
        assert!(reader
            .try_text_with_limit(1024)
            .unwrap()
            .unwrap()
            .contains("selected marker curr"));
    }

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
        assert!(reader.clone().try_with_screen(|_| ()).is_none());
        reader.with_frame(|screen, revision| {
            assert_eq!(revision, 7);
            assert_eq!(screen.contents(), "old");
            assert_eq!(screen.cursor_position(), (0, 3));
            assert_eq!(screen.size(), (4, 20));
            assert!(screen.cell(0, 0).unwrap().dim());
        });
        drop(writer);
        assert_eq!(
            reader.clone().try_with_screen(|screen| screen.contents()),
            Some("oldnew".into())
        );
        assert_eq!(
            reader.try_with_frame(|screen, revision| (screen.contents(), revision)),
            Some(("oldnew".into(), 8))
        );
    }
}
