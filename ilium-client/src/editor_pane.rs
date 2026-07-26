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
use ratatui_textarea::{CursorMove, Input, TextArea, WrapMode};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::markdown::render::{HeadingRendering, RenderedDocument};

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

/// Converts a buffer position (`ratatui_textarea` addresses rows/columns as
/// `usize` internally, with no cap) into the `u16` this pane's scroll-mirror
/// fields use, saturating rather than truncating. A plain `as u16` would
/// silently wrap for a buffer past 65535 lines or a line past 65535
/// columns (large logs, generated/minified files), turning "scroll to the
/// cursor" into "scroll to an unrelated nearby-looking row" instead of the
/// graceful "can't address past here" a saturating cap gives.
fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

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

/// One terminal row in the Source view. In Clip mode each source line maps
/// to one row; in Wrap mode a source line can contribute several rows while
/// retaining its exact source-coordinate range for mouse interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceVisualRow {
    pub source_row: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub first_in_source_row: bool,
    pub last_in_source_row: bool,
}

/// A single editor pane: a `TextArea` plus the file it is backed by (if
/// any), a dirty flag tracking unsaved edits, and the toolbar-controlled
/// display state (view mode, line numbers, minimap).
pub struct EditorPane {
    pub textarea: TextArea<'static>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub view_mode: EditorViewMode,
    /// Source-mode overflow policy shared by global editor defaults and the
    /// per-editor toolbar. It changes presentation only, never file content.
    pub line_display: crate::config::LineDisplay,
    /// Per rendered editor-pane preference for terminals that cannot show
    /// graphics protocols. The toolbar exposes this only in Rendered mode.
    pub heading_rendering: HeadingRendering,
    pub show_line_numbers: bool,
    pub show_minimap: bool,
    /// When on, an edit schedules a debounced write-to-disk instead of
    /// waiting for an explicit Save -- see `autosave_pending_since` and
    /// `autosave_if_due`.
    pub show_autosave: bool,
    autosave_delay: Duration,
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
    /// The Source-mode viewport's top buffer row. Highlighted source files
    /// render through `editor_highlight` rather than `TextArea`, so they need
    /// an explicit shared position for drawing, hit-testing, wheel movement,
    /// and the Ratatui scrollbar. `Cell` lets `ui.rs::draw_editor` maintain
    /// that position while holding only `&EditorPane`.
    source_scroll_row: Cell<u16>,
    /// Whether the next Source render must bring the text cursor back into
    /// view. Wheel scrolling deliberately clears this so reading elsewhere
    /// in a file does not snap straight back to the insertion point.
    source_scroll_should_follow_cursor: Cell<bool>,
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
            line_display: crate::config::LineDisplay::default(),
            heading_rendering: HeadingRendering::Rasterized,
            show_line_numbers: true,
            show_minimap: true,
            show_autosave: false,
            autosave_delay: AUTOSAVE_DEBOUNCE,
            autosave_pending_since: None,
            rendered: None,
            rendered_scroll: 0,
            rendered_width: 0,
            source_scroll_row: Cell::new(0),
            source_scroll_should_follow_cursor: Cell::new(true),
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
            line_display: crate::config::LineDisplay::default(),
            heading_rendering: HeadingRendering::Rasterized,
            show_line_numbers: true,
            show_minimap: true,
            show_autosave: false,
            autosave_delay: AUTOSAVE_DEBOUNCE,
            autosave_pending_since: None,
            rendered: None,
            rendered_scroll: 0,
            rendered_width: 0,
            source_scroll_row: Cell::new(0),
            source_scroll_should_follow_cursor: Cell::new(true),
            source_scroll_col: Cell::new(0),
            content_revision: 0,
            highlight_cache: RefCell::new(None),
        };
        pane.apply_line_number_style();
        Ok(pane)
    }

    pub fn apply_defaults(&mut self, settings: &crate::config::EditorSettings) {
        self.set_line_display(settings.line_display);
        self.show_line_numbers = settings.show_line_numbers;
        self.show_minimap = settings.show_minimap;
        self.show_autosave = settings.autosave_enabled;
        self.autosave_delay = Duration::from_millis(u64::from(settings.autosave_delay_ms));
        if settings.markdown_rendered_by_default && self.is_markdown() {
            self.view_mode = EditorViewMode::Rendered;
        }
        self.apply_line_number_style();
    }

    /// `.md`/`.markdown` only -- the only extensions Rendered mode applies
    /// to. Every other file type's toolbar view-mode control stays
    /// disabled (see `app.rs::toolbar_actions_for`).
    pub fn is_markdown(&self) -> bool {
        self.path
            .as_ref()
            .is_some_and(|path| is_markdown_path(path))
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

    /// Flips the rendered-heading representation without changing the
    /// document source or whether the pane is in Rendered mode.
    pub fn toggle_heading_rendering(&mut self) {
        self.heading_rendering = match self.heading_rendering {
            HeadingRendering::Rasterized => HeadingRendering::PlainText,
            HeadingRendering::PlainText => HeadingRendering::Rasterized,
        };
    }

    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
        self.apply_line_number_style();
    }

    /// Switches Source view between horizontally clipped logical lines and
    /// soft-wrapped visual lines. `TextArea` owns cursor geometry for plain
    /// files; the highlighted renderer reads this same value.
    pub fn toggle_line_display(&mut self) {
        self.set_line_display(self.line_display.toggled());
    }

    /// Clamps rendered Markdown's independent row offset after a viewport
    /// resize or an overflow-policy change. Switching Wrap to Clip can make
    /// the document shorter without changing its source, so retaining an
    /// old offset would otherwise leave the rendered viewport blank.
    pub fn clamp_rendered_scroll(&mut self, viewport_width: u16, viewport_height: u16) {
        let max_scroll = self
            .rendered
            .as_ref()
            .map(|document| {
                crate::markdown::view::max_scroll(
                    document,
                    viewport_width,
                    viewport_height,
                    self.line_display,
                )
            })
            .unwrap_or(0);
        self.rendered_scroll = self.rendered_scroll.min(max_scroll);
    }

    fn set_line_display(&mut self, line_display: crate::config::LineDisplay) {
        self.line_display = line_display;
        self.textarea.set_wrap_mode(match line_display {
            crate::config::LineDisplay::Clip => WrapMode::None,
            crate::config::LineDisplay::Wrap => WrapMode::Glyph,
        });
        self.source_scroll_col.set(0);
        self.source_scroll_should_follow_cursor.set(true);
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
    ///
    /// Writes to a temp file in the same directory and renames it into
    /// place, mirroring `workspace_file::save`'s identical reasoning: a
    /// disk-full or killed-mid-write failure must never leave `path`
    /// truncated to a partial file on disk -- the rename is atomic, so the
    /// file at `path` is always either the previous complete contents or
    /// the new ones, never a half-written mix of both. Any failure after
    /// the temp file was created (a failed write, a failed fsync, or a
    /// failed rename) still cleans it up below, the same as
    /// `workspace_file::save`, rather than leaving a stray
    /// `.<file>.tmp-<pid>` behind forever on an error path.
    pub fn save_to(&mut self, path: &Path) -> anyhow::Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "buffer".to_string());
        let temp_path = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));

        let result = (|| -> anyhow::Result<()> {
            let mut writer = io::BufWriter::new(fs::File::create(&temp_path)?);
            for line in self.textarea.lines() {
                writer.write_all(line.as_bytes())?;
                writer.write_all(b"\n")?;
            }
            writer.flush()?;
            writer.into_inner().map_err(io::Error::from)?.sync_all()?;
            fs::rename(&temp_path, path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
            return result;
        }

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
            .is_some_and(|since| since.elapsed() >= self.autosave_delay);
        if !due {
            return None;
        }
        self.autosave_pending_since = None;
        Some(self.save())
    }

    /// Returns the one autosave deadline this editor currently contributes
    /// to the event loop.  Keeping this calculation beside the debounce
    /// state prevents the client from polling every editor on a fixed tick
    /// simply to discover that no write can happen yet.
    pub fn autosave_deadline(&self) -> Option<Instant> {
        (self.show_autosave && self.dirty)
            .then_some(self.autosave_pending_since)
            .flatten()
            .map(|pending_since| pending_since + self.autosave_delay)
    }

    /// Feeds one input event into the textarea; sets `self.dirty = true` if
    /// it actually changed the buffer content. Returns whether it was
    /// modified (same as `TextArea::input`'s return value).
    pub fn input(&mut self, input: Input) -> bool {
        let modified = self.textarea.input(input);
        // Keyboard navigation and edits should always reveal the insertion
        // point again after a user has independently scrolled the viewport.
        self.source_scroll_should_follow_cursor.set(true);
        if modified {
            self.mark_dirty();
        }
        modified
    }

    /// Inserts a semantic text payload in one operation. Voice and future
    /// non-keyboard controllers use this instead of synthesizing hundreds of
    /// terminal key events, while the same dirty/autosave invariants remain
    /// owned by the editor.
    pub fn insert_text(&mut self, text: &str) -> bool {
        let modified = self.textarea.insert_str(text);
        self.source_scroll_should_follow_cursor.set(true);
        if modified {
            self.mark_dirty();
        }
        modified
    }

    /// Replaces the complete buffer while preserving the editor's path and
    /// presentation settings. The caller owns the explicit confirmation for
    /// this destructive semantic operation; the editor owns dirty tracking.
    pub fn replace_contents(&mut self, text: &str) {
        let lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                .collect()
        };
        self.textarea = TextArea::from(lines);
        self.apply_line_number_style();
        self.source_scroll_row.set(0);
        self.source_scroll_col.set(0);
        self.source_scroll_should_follow_cursor.set(true);
        self.mark_dirty();
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

    /// Keeps the Source viewport valid for this render. Keyboard navigation
    /// follows the insertion point, while wheel navigation remains an
    /// independent reading position until the next keyboard action.
    pub fn update_source_scroll_mirror(&self, viewport_height: u16, viewport_width: u16) {
        let visual_rows = self.source_visual_rows(viewport_width);
        let cursor = self.textarea.cursor();
        let cursor_row = saturating_u16(
            visual_rows
                .iter()
                .position(|visual_row| {
                    visual_row.source_row == cursor.0
                        && visual_row.start_column <= cursor.1
                        && (cursor.1 < visual_row.end_column
                            || (visual_row.last_in_source_row && cursor.1 == visual_row.end_column))
                })
                .unwrap_or(cursor.0),
        );
        let previous_top = self.source_scroll_row.get();
        let max_top = saturating_u16(visual_rows.len()).saturating_sub(viewport_height);
        let next_top = if self.source_scroll_should_follow_cursor.replace(false) {
            if cursor_row < previous_top {
                cursor_row
            } else if previous_top.saturating_add(viewport_height) <= cursor_row {
                // `cursor_row >= viewport_height` here (the branch above
                // guarantees it), so the subtraction can't underflow; the
                // trailing `saturating_add(1)` only matters at the extreme
                // edge where `cursor_row` is already `u16::MAX`.
                (cursor_row - viewport_height).saturating_add(1)
            } else {
                previous_top
            }
        } else {
            previous_top
        };
        self.source_scroll_row.set(next_top.min(max_top));
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
        if matches!(self.line_display, crate::config::LineDisplay::Wrap) {
            self.source_scroll_col.set(0);
            return;
        }
        let mut cursor_col = saturating_u16(self.textarea.screen_cursor().col);
        if self.show_line_numbers {
            let gutter =
                crate::editor_highlight::line_number_gutter_width(self.textarea.lines().len());
            cursor_col = cursor_col.saturating_add(gutter);
        }
        let previous_left = self.source_scroll_col.get();
        let next_left = if cursor_col < previous_left {
            cursor_col
        } else if previous_left.saturating_add(viewport_width) <= cursor_col {
            // Same reasoning as `update_source_scroll_mirror`: the branch
            // above guarantees `cursor_col >= viewport_width`, so only the
            // `+ 1` can overflow, at the `cursor_col == u16::MAX` edge.
            (cursor_col - viewport_width).saturating_add(1)
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
        self.textarea
            .move_cursor(CursorMove::Jump(saturating_u16(line), 0));
        self.source_scroll_should_follow_cursor.set(true);
    }

    /// Moves to an editor location expressed in zero-based source coordinates.
    pub fn jump_to_location(&mut self, line: usize, column: usize) {
        self.textarea.move_cursor(CursorMove::Jump(
            saturating_u16(line),
            saturating_u16(column),
        ));
        self.source_scroll_should_follow_cursor.set(true);
    }

    /// Moves the Source-mode viewport by `delta` rows. It updates both the
    /// explicit source position used by highlighted rendering and
    /// `TextArea`'s native viewport used by plain files, then clamps the
    /// shared position so the final page remains aligned with the bottom.
    pub fn scroll_source_view(&mut self, delta: i16, viewport_height: u16, viewport_width: u16) {
        // Keep `TextArea`'s native renderer in sync for unhighlighted files;
        // highlighted files use the explicit mirror below because they render
        // through `editor_highlight` instead of the widget itself.
        self.textarea.scroll((delta, 0));

        let max_top = saturating_u16(self.source_visual_rows(viewport_width).len())
            .saturating_sub(viewport_height);
        let current = self.source_scroll_row.get();
        let next = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as u16)
        };
        self.source_scroll_row.set(next.min(max_top));
        self.source_scroll_should_follow_cursor.set(false);
    }

    /// Returns the visual Source rows for the current width. The same map is
    /// used by the highlighter, scrollbar, wheel handling, checkbox hit test,
    /// and source-line context menu so wrap mode never turns a screen row
    /// into the wrong physical line.
    pub fn source_visual_rows(&self, viewport_width: u16) -> Vec<SourceVisualRow> {
        let width = self.source_wrap_width(viewport_width);
        self.textarea
            .lines()
            .iter()
            .enumerate()
            .flat_map(|(source_row, line)| self.visual_rows_for_line(source_row, line, width))
            .collect()
    }

    /// Resolves one visible Source row back to its source coordinates.
    pub fn source_visual_row_at(
        &self,
        viewport_width: u16,
        visual_row: usize,
    ) -> Option<SourceVisualRow> {
        self.source_visual_rows(viewport_width)
            .get(visual_row)
            .copied()
    }

    fn source_wrap_width(&self, viewport_width: u16) -> usize {
        let gutter_width = if self.show_line_numbers {
            crate::editor_highlight::line_number_gutter_width(self.textarea.lines().len())
        } else {
            0
        };
        usize::from(viewport_width.saturating_sub(gutter_width).max(1))
    }

    fn visual_rows_for_line(
        &self,
        source_row: usize,
        line: &str,
        width: usize,
    ) -> Vec<SourceVisualRow> {
        if matches!(self.line_display, crate::config::LineDisplay::Clip) {
            return vec![SourceVisualRow {
                source_row,
                start_byte: 0,
                end_byte: line.len(),
                start_column: 0,
                end_column: line.chars().count(),
                first_in_source_row: true,
                last_in_source_row: true,
            }];
        }

        let mut rows = Vec::new();
        let mut segment_start_byte = 0;
        let mut segment_start_column = 0;
        let mut segment_width = 0;
        let tab_width = usize::from(self.textarea.tab_length()).max(1);
        let mut column = 0;
        for (byte, grapheme) in line.grapheme_indices(true) {
            let grapheme_width = if grapheme == "\t" {
                tab_width - segment_width % tab_width
            } else {
                UnicodeWidthStr::width(grapheme).max(1)
            };
            if segment_width > 0 && segment_width + grapheme_width > width {
                rows.push(SourceVisualRow {
                    source_row,
                    start_byte: segment_start_byte,
                    end_byte: byte,
                    start_column: segment_start_column,
                    end_column: column,
                    first_in_source_row: rows.is_empty(),
                    last_in_source_row: false,
                });
                segment_start_byte = byte;
                segment_start_column = column;
                segment_width = 0;
            }
            segment_width += grapheme_width;
            column += grapheme.chars().count();
        }
        rows.push(SourceVisualRow {
            source_row,
            start_byte: segment_start_byte,
            end_byte: line.len(),
            start_column: segment_start_column,
            end_column: column,
            first_in_source_row: rows.is_empty(),
            last_in_source_row: true,
        });
        rows
    }

    /// Flips the checked state of the task-list checkbox at `bracket_col`
    /// (the char index of its `[`, as returned by
    /// `markdown::checkbox::find_checkbox`) on buffer `row`. Edits the
    /// single status character in place via the textarea's own cursor API
    /// (rather than rebuilding the line) so undo history and the cursor's
    /// own position stay meaningful.
    pub fn toggle_checkbox(&mut self, row: usize, bracket_col: usize, currently_checked: bool) {
        self.textarea.move_cursor(CursorMove::Jump(
            saturating_u16(row),
            saturating_u16(bracket_col + 1),
        ));
        self.textarea.delete_next_char();
        self.textarea
            .insert_char(if currently_checked { ' ' } else { 'x' });
        self.source_scroll_should_follow_cursor.set(true);
        self.mark_dirty();
    }
}

/// Whether `path` names a Markdown document supported by the editor and
/// board adapters. Keeping this check at the editor/file boundary prevents
/// tree menus and explorer menus from drifting on `.md` versus `.markdown`.
pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// Unique scratch directory per test run/thread so parallel test runs
    /// don't collide on the same path.
    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-editor-pane-tests")
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

    #[test]
    fn heading_rendering_switches_between_graphics_and_plain_text() {
        let mut pane = EditorPane::empty();
        assert_eq!(pane.heading_rendering, HeadingRendering::Rasterized);

        pane.toggle_heading_rendering();
        assert_eq!(pane.heading_rendering, HeadingRendering::PlainText);

        pane.toggle_heading_rendering();
        assert_eq!(pane.heading_rendering, HeadingRendering::Rasterized);
    }

    #[test]
    fn wrapping_tracks_visual_rows_without_changing_the_buffer() {
        let mut pane = EditorPane::empty();
        pane.textarea = TextArea::from(["abcdefgh"]);
        let settings = crate::config::EditorSettings {
            line_display: crate::config::LineDisplay::Wrap,
            show_line_numbers: false,
            ..crate::config::EditorSettings::default()
        };

        pane.apply_defaults(&settings);

        assert_eq!(pane.textarea.wrap_mode(), WrapMode::Glyph);
        assert_eq!(pane.source_visual_rows(3).len(), 3);
        assert_eq!(pane.source_visual_rows(3)[1].start_column, 3);
        assert_eq!(pane.textarea.lines(), &["abcdefgh".to_string()]);
    }

    #[test]
    fn source_wheel_scroll_moves_and_clamps_the_independent_viewport() {
        let mut pane = EditorPane::empty();
        pane.textarea = TextArea::from((0..10).map(|row| format!("line {row}")));

        pane.scroll_source_view(3, 4, 20);
        assert_eq!(pane.source_scroll_row(), 3);

        // Rendering preserves a wheel-selected reading position instead of
        // snapping to the insertion point on the first line.
        pane.update_source_scroll_mirror(4, 20);
        assert_eq!(pane.source_scroll_row(), 3);

        pane.scroll_source_view(100, 4, 20);
        assert_eq!(pane.source_scroll_row(), 6);

        pane.scroll_source_view(-100, 4, 20);
        assert_eq!(pane.source_scroll_row(), 0);
    }

    #[test]
    fn cursor_navigation_reveals_cursor_after_source_wheel_scroll() {
        let mut pane = EditorPane::empty();
        pane.textarea = TextArea::from((0..10).map(|row| format!("line {row}")));
        pane.scroll_source_view(6, 4, 20);

        pane.jump_to_line(0);
        pane.update_source_scroll_mirror(4, 20);

        assert_eq!(pane.source_scroll_row(), 0);
    }
}
