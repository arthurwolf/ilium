//! File-backed editor buffer, wrapping `ratatui_textarea::TextArea`.
//!
//! `EditorPane` owns the actual text buffer plus the bookkeeping needed to
//! turn it into a real file on disk: the source path (if any) and a `dirty`
//! flag so callers (the leader-key `Save` action, the status bar, the pane
//! close confirmation, etc.) know whether there is unsaved content.

use std::cell::{Cell, Ref, RefCell};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::style::{Color, Style};
use ratatui_textarea::{CursorMove, Input, TextArea};

use crate::markdown::render::RenderedDocument;

/// How long an editor pane waits after the last edit before autosaving,
/// when `show_autosave` is on -- long enough that a burst of keystrokes
/// only triggers one write, short enough to feel automatic.
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_secs(1);

/// Which content the pane's main area currently shows. Only `.md`/
/// `.markdown` files ever leave `Source` (see `EditorPane::is_markdown`);
/// every other file type stays in `Source` regardless of what the
/// toolbar would otherwise allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorViewMode {
    /// Plain `TextArea` editing -- the only mode that accepts keystrokes.
    Source,
    /// Read-only rendered markdown (mdfried-style headers/images). Toggled
    /// back to `Source` to resume editing.
    Rendered,
}

/// Gutter/minimap accent color, shared so the two toggleable chrome
/// elements read as one visual language.
const CHROME_FG: Color = Color::DarkGray;

/// Cached Source-mode syntax highlighting -- see
/// `EditorPane::highlighted_lines`. Keyed on both the buffer's
/// `content_revision` and `path`: a Save As to a different-language file
/// must not keep showing the previous language's colors even if the
/// content and revision counter happen to still match.
struct HighlightCache {
    revision: u64,
    path: PathBuf,
    lines: Vec<crate::syntax::LineTokens>,
}

/// A single editor pane: a `TextArea` plus the file it is backed by (if
/// any), a dirty flag tracking unsaved edits, and the toolbar-controlled
/// display state (view mode, line numbers, minimap).
pub struct EditorPane {
    pub textarea: TextArea<'static>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub view_mode: EditorViewMode,
    pub show_line_numbers: bool,
    pub show_minimap: bool,
    /// When on, an edit schedules a debounced write-to-disk instead of
    /// waiting for an explicit Save -- see `autosave_pending_since` and
    /// `autosave_if_due`.
    pub show_autosave: bool,
    /// Set to `Instant::now()` on every modifying edit while
    /// `show_autosave` is on; cleared once that edit has been written to
    /// disk (or autosave is toggled off). `main.rs`'s event loop polls
    /// `autosave_if_due` every tick, so this is what turns "1 second since
    /// the last keystroke" into an actual write.
    autosave_pending_since: Option<Instant>,
    /// The last Rendered-mode build, kept until the next toggle-in or
    /// resize rebuilds it (see `app.rs::rebuild_rendered_markdown`) --
    /// `None` before the pane has ever been rendered. Rendering needs
    /// `&Picker`/`&mut HeaderRasterizer` from `App`, which `EditorPane`
    /// doesn't own, so it can't rebuild itself; it only holds the result.
    pub rendered: Option<RenderedDocument>,
    /// Scroll offset (in rendered terminal rows) while `view_mode` is
    /// `Rendered`. Independent of the `TextArea`'s own cursor/scroll,
    /// which stays exactly where editing left it for when the user
    /// switches back to `Source`.
    pub rendered_scroll: u16,
    /// The content width `rendered` was built for, so a resize while
    /// already in Rendered mode knows to rebuild (word-wrap and image
    /// sizing both depend on width).
    pub rendered_width: u16,
    /// Mirrors `ratatui_textarea::TextArea`'s own internal auto-scroll
    /// (the crate keeps its viewport private, with no public getter) so a
    /// Source-mode mouse click can be mapped back to a buffer row -- see
    /// `update_source_scroll_mirror` and `App::checkbox_at` in `app.rs`.
    /// `Cell` because `ui.rs::draw_editor` only holds `&EditorPane`.
    source_scroll_row: Cell<u16>,
    /// Mirrors `ratatui_textarea::TextArea`'s internal horizontal scroll
    /// column the same way `source_scroll_row` mirrors its vertical one --
    /// see `update_source_scroll_col_mirror`. Only read/written when
    /// Source mode renders through `editor_highlight` (a recognized
    /// language); the plain `TextArea` widget path manages its own
    /// horizontal scroll internally and never touches this field.
    source_scroll_col: Cell<u16>,
    /// Bumped on every buffer edit (see `mark_dirty`) so
    /// `highlighted_lines` knows its cached highlighting is stale and
    /// needs re-running through `syntax::highlight`.
    content_revision: u64,
    /// Cached Source-mode syntax highlighting, rebuilt lazily by
    /// `highlighted_lines`. `RefCell` because that method takes `&self`
    /// (same reason `source_scroll_row` above is a `Cell`).
    highlight_cache: RefCell<Option<HighlightCache>>,
}

impl EditorPane {
    /// A new, empty, unsaved buffer. The picker path only uses it in tests;
    /// production editor panes are always backed by a selected path.
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            textarea: TextArea::default(),
            path: None,
            dirty: false,
            view_mode: EditorViewMode::Source,
            show_line_numbers: true,
            show_minimap: true,
            show_autosave: false,
            autosave_pending_since: None,
            rendered: None,
            rendered_scroll: 0,
            rendered_width: 0,
            source_scroll_row: Cell::new(0),
            source_scroll_col: Cell::new(0),
            content_revision: 0,
            highlight_cache: RefCell::new(None),
        }
    }

    /// Loads a file's contents into a new buffer. If the file doesn't exist
    /// yet, starts an empty buffer with that path (so it gets created on
    /// first save). Errors on any other I/O failure (e.g. the path exists
    /// but is a directory, or is unreadable).
    pub fn load(path: PathBuf) -> anyhow::Result<Self> {
        let textarea = match fs::metadata(&path) {
            // Existing regular file: read it line-by-line into the buffer.
            Ok(metadata) if metadata.is_file() => io::BufReader::new(fs::File::open(&path)?)
                .lines()
                .collect::<io::Result<TextArea<'static>>>()?,
            // Path exists but isn't a regular file (e.g. a directory) — that's a real error.
            Ok(_) => {
                anyhow::bail!("{} is not a regular file", path.display());
            }
            // Missing file is fine: start empty, the path is still recorded so
            // save() creates it later.
            Err(err) if err.kind() == io::ErrorKind::NotFound => TextArea::default(),
            // Any other stat failure (permissions, etc.) is a real error.
            Err(err) => return Err(err.into()),
        };

        let mut pane = Self {
            textarea,
            path: Some(path),
            dirty: false,
            view_mode: EditorViewMode::Source,
            show_line_numbers: true,
            show_minimap: true,
            show_autosave: false,
            autosave_pending_since: None,
            rendered: None,
            rendered_scroll: 0,
            rendered_width: 0,
            source_scroll_row: Cell::new(0),
            source_scroll_col: Cell::new(0),
            content_revision: 0,
            highlight_cache: RefCell::new(None),
        };
        pane.apply_line_number_style();
        Ok(pane)
    }

    /// `.md`/`.markdown` only -- the only extensions Rendered mode applies
    /// to. Every other file type's toolbar view-mode control stays
    /// disabled (see `app.rs::toolbar_actions_for`).
    pub fn is_markdown(&self) -> bool {
        self.path
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
            })
    }

    /// Flips `Source` <-> `Rendered`. A no-op for non-markdown files --
    /// callers don't need to guard on `is_markdown` themselves.
    pub fn toggle_view_mode(&mut self) {
        if !self.is_markdown() {
            return;
        }
        self.view_mode = match self.view_mode {
            EditorViewMode::Source => EditorViewMode::Rendered,
            EditorViewMode::Rendered => EditorViewMode::Source,
        };
    }

    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
        self.apply_line_number_style();
    }

    pub fn toggle_minimap(&mut self) {
        self.show_minimap = !self.show_minimap;
    }

    /// Flips autosave on/off. Turning it on while the buffer already has
    /// unsaved edits schedules an immediate debounce window rather than
    /// waiting for the next keystroke; turning it off drops any pending
    /// debounce so a stale toggle never fires a write after the fact.
    pub fn toggle_autosave(&mut self) {
        self.show_autosave = !self.show_autosave;
        if self.show_autosave && self.dirty {
            self.autosave_pending_since = Some(Instant::now());
        } else if !self.show_autosave {
            self.autosave_pending_since = None;
        }
    }

    /// Applies or clears the `TextArea`'s built-in line-number gutter to
    /// match `self.show_line_numbers`.
    fn apply_line_number_style(&mut self) {
        if self.show_line_numbers {
            self.textarea
                .set_line_number_style(Style::new().fg(CHROME_FG));
        } else {
            self.textarea.remove_line_number();
        }
    }

    /// Writes the buffer's lines back to `self.path`, joined with '\n'.
    /// Errors if `self.path` is `None` (nothing to save to yet).
    pub fn save(&mut self) -> anyhow::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("editor pane has no path to save to"))?;
        self.save_to(&path)
    }

    /// Writes the buffer's lines to `path`, joined with '\n', without
    /// touching `self.path` -- callers that retarget the pane (Save As)
    /// must only commit the new path once the write actually succeeds, so
    /// a failed write never leaves the pane pointed at a location nothing
    /// was written to.
    pub fn save_to(&mut self, path: &Path) -> anyhow::Result<()> {
        let mut writer = io::BufWriter::new(fs::File::create(path)?);
        for line in self.textarea.lines() {
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;

        self.dirty = false;
        self.autosave_pending_since = None;
        Ok(())
    }

    /// Writes the buffer to disk if autosave is on, dirty, and the
    /// debounce window has elapsed since the last edit. Returns `None`
    /// when no write was attempted (autosave off, nothing dirty, or still
    /// within the debounce window), so a quiet no-op every poll tick
    /// doesn't cost more than an `Instant` comparison. Clears the pending
    /// timestamp before writing (not after) so a failed write doesn't spin
    /// retrying every tick -- the next edit reschedules it, same as any
    /// other debounce.
    pub fn autosave_if_due(&mut self) -> Option<anyhow::Result<()>> {
        if !self.show_autosave || !self.dirty {
            return None;
        }
        let due = self
            .autosave_pending_since
            .is_some_and(|since| since.elapsed() >= AUTOSAVE_DEBOUNCE);
        if !due {
            return None;
        }
        self.autosave_pending_since = None;
        Some(self.save())
    }

    /// Feeds one input event into the textarea; sets `self.dirty = true` if
    /// it actually changed the buffer content. Returns whether it was
    /// modified (same as `TextArea::input`'s return value).
    pub fn input(&mut self, input: Input) -> bool {
        let modified = self.textarea.input(input);
        if modified {
            self.mark_dirty();
        }
        modified
    }

    /// Marks the buffer dirty and, if autosave is on, (re)starts its
    /// debounce window -- shared by every mutation path (`input`,
    /// `toggle_checkbox`) so none of them can forget to arm the timer.
    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.content_revision += 1;
        if self.show_autosave {
            self.autosave_pending_since = Some(Instant::now());
        }
    }

    /// Recomputes the mirrored Source-mode scroll-top row for a
    /// `viewport_height`-row viewport. Must be called every time the
    /// `TextArea` widget itself is rendered, with the exact same height,
    /// so this mirror follows the widget's own (unexposed) viewport in
    /// lockstep -- both apply the identical "keep the cursor's row inside
    /// [top, top + height)" clamp to the same cursor position on the same
    /// frame, so given the same starting point (0) they can never drift.
    /// Reimplements `ratatui_textarea`'s internal `next_scroll_top`
    /// (`src/widget.rs`), which the crate does not expose a getter for.
    pub fn update_source_scroll_mirror(&self, viewport_height: u16) {
        let cursor_row = self.textarea.cursor().0 as u16;
        let previous_top = self.source_scroll_row.get();
        let next_top = if cursor_row < previous_top {
            cursor_row
        } else if previous_top.saturating_add(viewport_height) <= cursor_row {
            cursor_row + 1 - viewport_height
        } else {
            previous_top
        };
        self.source_scroll_row.set(next_top);
    }

    /// The buffer row currently mirrored as the top of the Source-mode
    /// viewport -- see `update_source_scroll_mirror`.
    pub fn source_scroll_row(&self) -> u16 {
        self.source_scroll_row.get()
    }

    /// Recomputes the mirrored Source-mode scroll-left column for a
    /// `viewport_width`-column viewport, only used by `editor_highlight`'s
    /// renderer (see that module). Mirrors `ratatui_textarea`'s private
    /// `scroll_top_col`: the cursor's *display* column
    /// (`TextArea::screen_cursor`, already tab/wide-char aware) is shifted
    /// right by the gutter width first, since the crate's own horizontal
    /// scroll shifts the line-number gutter along with the text rather
    /// than keeping it pinned.
    pub fn update_source_scroll_col_mirror(&self, viewport_width: u16) {
        let mut cursor_col = self.textarea.screen_cursor().col as u16;
        if self.show_line_numbers {
            let gutter =
                crate::editor_highlight::line_number_gutter_width(self.textarea.lines().len());
            if cursor_col <= gutter {
                cursor_col *= 2;
            } else {
                cursor_col += gutter;
            }
        }
        let previous_left = self.source_scroll_col.get();
        let next_left = if cursor_col < previous_left {
            cursor_col
        } else if previous_left.saturating_add(viewport_width) <= cursor_col {
            cursor_col + 1 - viewport_width
        } else {
            previous_left
        };
        self.source_scroll_col.set(next_left);
    }

    /// The display column currently mirrored as the left edge of the
    /// Source-mode viewport -- see `update_source_scroll_col_mirror`.
    pub fn source_scroll_col(&self) -> u16 {
        self.source_scroll_col.get()
    }

    /// This pane's Source-mode syntax highlighting, one entry per buffer
    /// line, rebuilt only when `content_revision` (or `path`, e.g. after a
    /// Save As to a different-language file) has moved past the cached
    /// build. `None` when `path`'s language isn't recognized by `syntax`,
    /// so `ui::draw_editor` falls back to the plain `TextArea` widget
    /// exactly as it did before syntax highlighting existed.
    pub fn highlighted_lines(&self) -> Option<Ref<'_, Vec<crate::syntax::LineTokens>>> {
        let path = self.path.as_ref()?;
        let stale = match self.highlight_cache.borrow().as_ref() {
            Some(cache) => cache.revision != self.content_revision || cache.path != *path,
            None => true,
        };
        if stale {
            let lines = crate::syntax::highlight(path, self.textarea.lines())?;
            *self.highlight_cache.borrow_mut() = Some(HighlightCache {
                revision: self.content_revision,
                path: path.clone(),
                lines,
            });
        }
        Some(Ref::map(self.highlight_cache.borrow(), |cache| {
            &cache.as_ref().expect("just populated above").lines
        }))
    }

    /// Moves the cursor to the start of `line` (clamped to the buffer's
    /// last line by `TextArea` itself) -- used by a minimap click-to-jump
    /// in Source mode, where the click maps straight to a cursor position.
    pub fn jump_to_line(&mut self, line: usize) {
        self.textarea.move_cursor(CursorMove::Jump(line as u16, 0));
    }

    /// Flips the checked state of the task-list checkbox at `bracket_col`
    /// (the char index of its `[`, as returned by
    /// `markdown::checkbox::find_checkbox`) on buffer `row`. Edits the
    /// single status character in place via the textarea's own cursor API
    /// (rather than rebuilding the line) so undo history and the cursor's
    /// own position stay meaningful.
    pub fn toggle_checkbox(&mut self, row: usize, bracket_col: usize, currently_checked: bool) {
        self.textarea
            .move_cursor(CursorMove::Jump(row as u16, (bracket_col + 1) as u16));
        self.textarea.delete_next_char();
        self.textarea
            .insert_char(if currently_checked { ' ' } else { 'x' });
        self.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// Unique scratch directory per test run/thread so parallel test runs
    /// don't collide on the same path.
    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-editor-pane-tests")
            .join(format!("{:?}", thread::current().id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn empty_buffer_has_no_path_and_is_not_dirty() {
        let pane = EditorPane::empty();
        assert!(pane.path.is_none());
        assert!(!pane.dirty);
        // A fresh TextArea starts with a single empty line.
        assert_eq!(pane.textarea.lines(), &[String::new()]);
    }

    #[test]
    fn load_missing_file_starts_empty_but_remembers_path() {
        let dir = scratch_dir();
        let path = dir.join("does-not-exist.txt");
        let _ = fs::remove_file(&path);

        let pane = EditorPane::load(path.clone()).expect("load should not error on missing file");
        assert_eq!(pane.path, Some(path));
        assert!(!pane.dirty);
        assert_eq!(pane.textarea.lines(), &[String::new()]);
    }

    #[test]
    fn load_errors_on_directory_path() {
        let dir = scratch_dir();
        let result = EditorPane::load(dir);
        assert!(result.is_err());
    }

    #[test]
    fn save_without_path_errors() {
        let mut pane = EditorPane::empty();
        assert!(pane.save().is_err());
    }

    #[test]
    fn save_then_load_round_trips_contents() {
        let dir = scratch_dir();
        let path = dir.join("round-trip.txt");
        let _ = fs::remove_file(&path);

        let mut pane = EditorPane::load(path.clone()).expect("load new file");
        for ch in "hello\nworld".chars() {
            if ch == '\n' {
                pane.input(Input {
                    key: ratatui_textarea::Key::Enter,
                    ..Default::default()
                });
            } else {
                pane.input(Input {
                    key: ratatui_textarea::Key::Char(ch),
                    ..Default::default()
                });
            }
        }
        assert!(pane.dirty);

        pane.save().expect("save should succeed");
        assert!(!pane.dirty);

        let reloaded = EditorPane::load(path.clone()).expect("reload saved file");
        assert_eq!(
            reloaded.textarea.lines(),
            &["hello".to_string(), "world".to_string()]
        );

        // Clean up so repeated test runs start fresh.
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn input_returns_and_tracks_modification() {
        let mut pane = EditorPane::empty();
        let modified = pane.input(Input {
            key: ratatui_textarea::Key::Char('x'),
            ..Default::default()
        });
        assert!(modified);
        assert!(pane.dirty);
    }
}
