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
        self.try_text_with_generation_and_limit(limit).map(|(_, text)| text)
    }

    /// Read the generation under the same parser guard as the visible text so
    /// consumers can reject out-of-order observations without a race. The
    /// screen's cell count and its text length are both bounded by `limit`.
    pub fn try_text_with_generation_and_limit(&self, limit: usize) -> Option<(u64, Result<String, &'static str>)> {
        self.try_text_with_generation_and_limits(limit, limit)
    }

    /// Like `try_text_with_generation_and_limit`, with separate bounds for the
    /// scanned grid (`cell_limit`, rows x columns) and the returned text
    /// (`byte_limit`, UTF-8 bytes). A large but mostly blank grid stays
    /// readable as long as its text fits: blank cells after the last populated
    /// cell of a row and trailing blank rows are not part of the text, so they
    /// are not charged against `byte_limit`.
    pub fn try_text_with_generation_and_limits(
        &self,
        cell_limit: usize,
        byte_limit: usize,
    ) -> Option<(u64, Result<String, &'static str>)> {
        self.try_with_frame(|screen, generation| {
            (generation, bounded_contents(screen, cell_limit, byte_limit))
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

/// The visible text (`vt100::Screen::contents`) of a screen whose geometry
/// and text both fit, computed without allocating an oversized string.
fn bounded_contents(
    screen: &vt100::Screen,
    cell_limit: usize,
    byte_limit: usize,
) -> Result<String, &'static str> {
    let (rows, columns) = screen.size();
    // Bounds the preflight scan itself, which visits every cell.
    if usize::from(rows).saturating_mul(usize::from(columns)) > cell_limit {
        return Err("terminal evidence geometry exceeds limit");
    }
    if contents_length_upper_bound(screen, byte_limit) > byte_limit {
        return Err("terminal evidence text exceeds limit");
    }
    let text = screen.contents();
    // The preflight is an upper bound; this keeps the contract exact even if
    // vt100's formatting ever emits more than the preflight predicts.
    if text.len() > byte_limit {
        return Err("terminal evidence text exceeds limit");
    }
    Ok(text)
}

/// An upper bound on `screen.contents().len()`, mirroring vt100's row
/// formatter: blank cells before a populated cell become one space each,
/// populated cells contribute their UTF-8 contents, the continuation half of
/// a wide character contributes nothing, and every row may end with one
/// newline. Stops counting once `stop_above` is exceeded.
fn contents_length_upper_bound(screen: &vt100::Screen, stop_above: usize) -> usize {
    let (rows, columns) = screen.size();
    let mut bytes = 0usize;
    for row in 0..rows {
        let mut next_column = 0u16;
        let mut skip_wide_continuation = false;
        for column in 0..columns {
            if skip_wide_continuation {
                skip_wide_continuation = false;
                continue;
            }
            let Some(cell) = screen.cell(row, column) else {
                continue;
            };
            skip_wide_continuation = cell.is_wide();
            if cell.has_contents() {
                let gap = usize::from(column.saturating_sub(next_column));
                bytes = bytes
                    .saturating_add(gap)
                    .saturating_add(cell.contents().len());
                next_column = column.saturating_add(if cell.is_wide() { 2 } else { 1 });
            }
        }
        bytes = bytes.saturating_add(1);
        if bytes > stop_above {
            return bytes;
        }
    }
    bytes
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
        let (generation, text) = reader.try_text_with_generation_and_limit(1024).unwrap();
        assert_eq!(generation, 8);
        assert_eq!(text.unwrap(), "oldnew");
        assert_eq!(
            reader.clone().try_with_screen(|screen| screen.contents()),
            Some("oldnew".into())
        );
        assert_eq!(
            reader.try_with_frame(|screen, revision| (screen.contents(), revision)),
            Some(("oldnew".into(), 8))
        );
    }

    fn reader_with(rows: u16, columns: u16, output: &[u8]) -> ScreenReader {
        let parser = Arc::new(RwLock::new(Parser::new_with_callbacks(
            rows,
            columns,
            0,
            TerminalQueryResponder::new(),
        )));
        parser.write().unwrap().process(output);
        ScreenReader::new(parser, Arc::new(AtomicU64::new(0)))
    }

    #[test]
    fn large_mostly_blank_grids_are_charged_by_their_text_not_their_geometry() {
        // 200 x 500 = 100,000 cells: above a 64 KiB text budget by geometry
        // alone, but its text is a few bytes, so it stays readable.
        let reader = reader_with(200, 500, b"ready\r\n\x1b[150;10Hprompt");
        let (_, text) = reader
            .try_text_with_generation_and_limits(256 * 1024, 64 * 1024)
            .unwrap();
        let text = text.expect("sparse text fits the byte budget");
        assert!(text.starts_with("ready\n"));
        assert!(text.ends_with("         prompt"));
        // The single-limit form still bounds the scanned geometry by `limit`.
        assert!(reader.try_text_with_limit(64 * 1024).unwrap().is_err());
        // The cell bound still refuses a grid too large to scan.
        assert!(reader
            .try_text_with_generation_and_limits(99_999, 64 * 1024)
            .unwrap()
            .1
            .is_err());
    }

    #[test]
    fn text_budget_counts_wide_characters_and_refuses_instead_of_truncating() {
        let reader = reader_with(2, 10, "ab\r\n\u{4e2d}x".as_bytes());
        let (_, text) = reader.try_text_with_generation_and_limits(20, 64).unwrap();
        let text = text.unwrap();
        // "ab\n" + a three-byte wide character + "x".
        assert_eq!(text, "ab\n\u{4e2d}x");
        let bound = reader
            .try_with_screen(|screen| contents_length_upper_bound(screen, usize::MAX))
            .unwrap();
        assert!(bound >= text.len(), "{bound} must cover {}", text.len());
        // A budget below the real text refuses instead of truncating.
        let refused = reader
            .try_text_with_generation_and_limits(20, text.len() - 1)
            .unwrap()
            .1;
        assert!(refused.is_err());
        // An empty screen is readable and empty.
        let empty = reader_with(3, 3, b"");
        assert_eq!(
            empty.try_text_with_generation_and_limits(9, 16).unwrap().1,
            Ok(String::new())
        );
    }

    #[test]
    fn dense_screens_above_the_text_budget_are_refused() {
        // Four fully written, soft-wrapped rows: 80 bytes of text; the
        // preflight also reserves one newline per row.
        let reader = reader_with(4, 20, &[b'x'; 80]);
        assert!(reader
            .try_text_with_generation_and_limits(80, 79)
            .unwrap()
            .1
            .is_err());
        assert_eq!(
            reader
                .try_text_with_generation_and_limits(80, 84)
                .unwrap()
                .1
                .map(|text| text.len()),
            Ok(80)
        );
    }
}
