//! Modal filesystem picker overlay, opened for an editor file or sidebar
//! folder root. Owns its own directory listing and renders it as a bordered
//! `ratatui::widgets::Table` -- folder/file icons to the left of each name,
//! human-formatted size and "modified" columns that collapse as the popup
//! narrows (name always wins the remaining space) -- and drives both
//! keyboard and mouse navigation directly, rather than delegating to a
//! third-party widget. The previous implementation wrapped
//! `ratatui_explorer::FileExplorer`, which never wired up `crossterm`
//! mouse events at all: every click while the picker was open was
//! silently dropped, forcing keyboard-only navigation.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Clear, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::layout::centered_rect;
use crate::theme;

/// One listed directory entry: a real file/directory, or the synthetic
/// `..` entry that steps up to the parent directory.
struct ExplorerEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_symlink: bool,
    /// `None` for directories and the `..` entry -- only files show a size.
    size: Option<u64>,
    modified: Option<SystemTime>,
}

/// A modal file-picker overlay, rooted at the originating pane's cwd and
/// re-listed every time the user navigates into a different directory.
pub struct ExplorerOverlay {
    current_dir: PathBuf,
    entries: Vec<ExplorerEntry>,
    selected: usize,
    offset: usize,
    show_hidden: bool,
    selection: ExplorerSelection,
    /// An explicit path entry field. Keeping it inside the same overlay
    /// preserves the directory listing while a user pastes/types a path.
    manual_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSelection {
    File,
    Folder,
}

impl ExplorerOverlay {
    /// Opens the picker rooted at `dir` (the originating pane's cwd).
    pub fn open_at(dir: &Path) -> anyhow::Result<Self> {
        Self::open(dir, ExplorerSelection::File)
    }

    pub fn open_folder_at(dir: &Path) -> anyhow::Result<Self> {
        Self::open(dir, ExplorerSelection::Folder)
    }

    fn open(dir: &Path, selection: ExplorerSelection) -> anyhow::Result<Self> {
        let mut overlay = Self {
            current_dir: dir.to_path_buf(),
            entries: Vec::new(),
            selected: 0,
            offset: 0,
            show_hidden: false,
            selection,
            manual_path: None,
        };
        overlay.reload()?;
        Ok(overlay)
    }

    /// Re-lists `current_dir` and resets the selection to the top. See
    /// `list_entries` for the listing/sort rules. Only reachable via
    /// `toggle_hidden`, which lists before committing the flag flip (see
    /// its doc comment) -- `reload` itself never needs to leave stale
    /// state behind because it never has a "new" directory to roll back
    /// to; the directory-changing paths go through `navigate_to` instead.
    fn reload(&mut self) -> anyhow::Result<()> {
        self.entries = list_entries(&self.current_dir, self.show_hidden, self.selection)?;
        self.selected = 0;
        self.offset = 0;
        Ok(())
    }

    /// Lists `dir` and, only if that succeeds, commits it as the new
    /// `current_dir`. Listing before committing keeps `current_dir` and
    /// `entries` from ever going out of sync: if `dir` turns out to be
    /// unreadable (e.g. execute-only permissions, or removed mid-session),
    /// the picker stays on its current, still-valid directory and listing
    /// instead of showing a title for a directory whose contents were
    /// never actually loaded.
    fn navigate_to(&mut self, dir: PathBuf) -> anyhow::Result<()> {
        let entries = list_entries(&dir, self.show_hidden, self.selection)?;
        self.current_dir = dir;
        self.entries = entries;
        self.selected = 0;
        self.offset = 0;
        Ok(())
    }

    /// Moves the selection by `delta` rows (negative = up), clamped to the
    /// listing's bounds, and scrolls just enough to keep it in view.
    fn move_selection(&mut self, delta: i64, viewport_rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        let next = (self.selected as i64 + delta).clamp(0, last as i64);
        self.selected = next as usize;

        let viewport_rows = viewport_rows.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + viewport_rows {
            self.offset = self.selected + 1 - viewport_rows;
        }
    }

    fn select_first(&mut self) {
        self.selected = 0;
        self.offset = 0;
    }

    fn select_last(&mut self, viewport_rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = self.entries.len() - 1;
        self.offset = self.selected.saturating_sub(viewport_rows.max(1) - 1);
    }

    /// Descends into the selected directory (re-listing in place, cursor
    /// back at the top) or, for a file, returns its path so the caller can
    /// open it.
    fn activate(&mut self) -> anyhow::Result<Option<PathBuf>> {
        let Some(entry) = self.entries.get(self.selected) else {
            return Ok(None);
        };
        if self.selection == ExplorerSelection::Folder {
            return Ok(Some(self.current_dir.clone()));
        }
        if entry.is_dir {
            self.navigate_to(entry.path.clone())?;
            Ok(None)
        } else {
            Ok(Some(entry.path.clone()))
        }
    }

    /// Activates the row under a repeated mouse click. Keyboard folder mode
    /// deliberately selects `current_dir` on Enter, but a pointer gesture
    /// names a visible child directory and must therefore return that child.
    fn activate_clicked_entry(&mut self) -> anyhow::Result<Option<PathBuf>> {
        let Some(entry) = self.entries.get(self.selected) else {
            return Ok(None);
        };
        if self.selection != ExplorerSelection::Folder {
            return self.activate();
        }
        if entry.name == ".." {
            self.navigate_up()?;
            return Ok(None);
        }
        Ok(Some(entry.path.clone()))
    }

    /// Descends into the selected directory without selecting it. Folder
    /// mode reserves Enter for "select this current folder", so Right/`l`
    /// retains a way to browse down into a child directory.
    fn navigate_selected_directory(&mut self) -> anyhow::Result<()> {
        let Some(entry) = self.entries.get(self.selected) else {
            return Ok(());
        };
        if entry.is_dir {
            self.navigate_to(entry.path.clone())?;
        }
        Ok(())
    }

    fn navigate_up(&mut self) -> anyhow::Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            self.navigate_to(parent.to_path_buf())?;
        }
        Ok(())
    }

    /// Lists `current_dir` with the flipped hidden-files flag before
    /// committing it, for the same reason `navigate_to` lists before
    /// committing a directory change: a failed re-list must not leave
    /// `show_hidden` disagreeing with what `entries` actually contains.
    fn toggle_hidden(&mut self) -> anyhow::Result<()> {
        let show_hidden = !self.show_hidden;
        let entries = list_entries(&self.current_dir, show_hidden, self.selection)?;
        self.show_hidden = show_hidden;
        self.entries = entries;
        self.selected = 0;
        self.offset = 0;
        Ok(())
    }

    /// Feeds a crossterm event to the picker. Returns `Some(path)` when the
    /// event chooses a file or folder -- callers close the overlay and route
    /// that path to the appropriate pane/tree action. Returns `None` for
    /// every other event, including selection and navigation.
    pub fn handle(&mut self, event: &Event, screen_area: Rect) -> anyhow::Result<Option<PathBuf>> {
        match event {
            Event::Key(key) => self.handle_key(key, screen_area),
            Event::Mouse(mouse) => self.handle_mouse(*mouse, screen_area),
            _ => Ok(None),
        }
    }

    /// Returns the ordinary file under `mouse` in this picker. Used by the
    /// picker-specific context menu; directories and the synthetic parent
    /// row deliberately have no file action.
    pub fn file_at_mouse(&self, mouse: MouseEvent, screen_area: Rect) -> Option<PathBuf> {
        if self.selection != ExplorerSelection::File {
            return None;
        }
        let layout = layout_for(screen_area);
        let index = row_at(
            &layout,
            self.offset,
            self.entries.len(),
            Position::new(mouse.column, mouse.row),
        )?;
        let entry = self.entries.get(index)?;
        (!entry.is_dir && entry.name != "..").then(|| entry.path.clone())
    }

    fn handle_key(&mut self, key: &KeyEvent, screen_area: Rect) -> anyhow::Result<Option<PathBuf>> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(None);
        }
        if let Some(manual_path) = self.manual_path.as_mut() {
            match key.code {
                KeyCode::Esc => self.manual_path = None,
                KeyCode::Enter => {
                    let entered = PathBuf::from(manual_path.trim());
                    let candidate = if entered.is_absolute() {
                        entered
                    } else {
                        self.current_dir.join(entered)
                    };
                    let canonical = candidate.canonicalize()?;
                    if !canonical.is_dir() {
                        anyhow::bail!("entered path is not a directory");
                    }
                    return Ok(Some(canonical));
                }
                KeyCode::Backspace => {
                    manual_path.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    manual_path.clear();
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    manual_path.push(character);
                }
                _ => {}
            }
            return Ok(None);
        }
        let viewport_rows = usize::from(layout_for(screen_area).rows_area.height);
        match key.code {
            KeyCode::Char('l')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.selection == ExplorerSelection::Folder =>
            {
                self.manual_path = Some(self.current_dir.display().to_string());
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_hidden()?;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1, viewport_rows),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1, viewport_rows),
            KeyCode::PageUp => self.move_selection(-(viewport_rows as i64), viewport_rows),
            KeyCode::PageDown => self.move_selection(viewport_rows as i64, viewport_rows),
            KeyCode::Home => self.select_first(),
            KeyCode::End => self.select_last(viewport_rows),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => self.navigate_up()?,
            KeyCode::Enter => return self.activate(),
            KeyCode::Right | KeyCode::Char('l') if self.selection == ExplorerSelection::Folder => {
                self.navigate_selected_directory()?
            }
            KeyCode::Right | KeyCode::Char('l') => return self.activate(),
            _ => {}
        }
        Ok(None)
    }

    fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        screen_area: Rect,
    ) -> anyhow::Result<Option<PathBuf>> {
        let layout = layout_for(screen_area);
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.move_selection(-3, usize::from(layout.rows_area.height));
            }
            MouseEventKind::ScrollDown => {
                self.move_selection(3, usize::from(layout.rows_area.height));
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let position = Position::new(mouse.column, mouse.row);
                if let Some(index) = row_at(&layout, self.offset, self.entries.len(), position) {
                    // A click on the already-selected row activates it; in
                    // folder mode that selects the visible directory itself
                    // rather than silently choosing the parent currently in
                    // the picker title.
                    if index == self.selected {
                        return self.activate_clicked_entry();
                    }
                    self.selected = index;
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

/// Lists `dir`'s entries: `..` first when it has a parent, then
/// directories, then files, each group sorted alphabetically
/// case-insensitively. A single unreadable entry (permission race,
/// vanished mid-scan) is skipped rather than failing the whole listing.
/// Pure with respect to `ExplorerOverlay` -- callers decide whether/when
/// to commit the result, which is what lets `navigate_to` and
/// `toggle_hidden` list before mutating any picker state.
fn list_entries(
    dir: &Path,
    show_hidden: bool,
    selection: ExplorerSelection,
) -> anyhow::Result<Vec<ExplorerEntry>> {
    let mut entries = Vec::new();
    if let Some(parent) = dir.parent() {
        entries.push(ExplorerEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
        });
    }

    for dir_entry in std::fs::read_dir(dir)? {
        let Ok(dir_entry) = dir_entry else {
            continue;
        };
        let name = dir_entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = dir_entry.path();
        let is_symlink = dir_entry
            .file_type()
            .map(|file_type| file_type.is_symlink())
            .unwrap_or(false);
        // `metadata()` follows symlinks, so a symlink-to-directory is
        // still browsable; fall back to `symlink_metadata()` so a
        // broken link still shows up (as a zero-size, non-directory
        // entry) instead of silently vanishing from the listing.
        let Ok(metadata) = std::fs::metadata(&path).or_else(|_| path.symlink_metadata()) else {
            continue;
        };
        if selection == ExplorerSelection::Folder && !metadata.is_dir() {
            continue;
        }
        entries.push(ExplorerEntry {
            name,
            path,
            is_dir: metadata.is_dir(),
            is_symlink,
            size: (!metadata.is_dir()).then_some(metadata.len()),
            modified: metadata.modified().ok(),
        });
    }

    let parent_offset = usize::from(entries.first().is_some_and(|entry| entry.name == ".."));
    entries[parent_offset..].sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

/// The picker's popup geometry, derived purely from the screen size so
/// rendering (`render`) and mouse hit-testing (`handle`) always agree on
/// where each row lands without either one caching the other's output.
struct ExplorerLayout {
    popup_area: Rect,
    /// Header row + entry rows, handed to `Table` as-is (it reserves its
    /// own header row internally); excludes the bottom hint line.
    table_area: Rect,
    /// Entry rows only, i.e. `table_area` minus the header row -- used for
    /// mouse hit-testing and for sizing a page-up/page-down jump.
    rows_area: Rect,
    hint_area: Rect,
}

fn layout_for(screen_area: Rect) -> ExplorerLayout {
    let popup_area = centered_rect(70, 70, screen_area);
    let inner = theme::block(true).inner(popup_area);
    let sections = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let table_area = sections[0];
    let hint_area = sections[1];
    let rows_area = Rect::new(
        table_area.x,
        table_area.y.saturating_add(1),
        table_area.width,
        table_area.height.saturating_sub(1),
    );
    ExplorerLayout {
        popup_area,
        table_area,
        rows_area,
        hint_area,
    }
}

/// Maps a screen position to an entry index, or `None` when the position
/// falls outside the row list or past the end of a short listing.
fn row_at(
    layout: &ExplorerLayout,
    offset: usize,
    entry_count: usize,
    position: Position,
) -> Option<usize> {
    if !layout.rows_area.contains(position) {
        return None;
    }
    let local_row = usize::from(position.y - layout.rows_area.y);
    let index = offset + local_row;
    (index < entry_count).then_some(index)
}

/// A column the table can show. `Name` never hides -- it's the one thing
/// the user actually opens a file by -- the others drop out first as the
/// popup narrows.
#[derive(Clone, Copy)]
enum Column {
    Name,
    Size,
    Modified,
}

impl Column {
    fn label(self) -> &'static str {
        match self {
            Column::Name => "Name",
            Column::Size => "Size",
            Column::Modified => "Modified",
        }
    }

    fn width(self) -> Constraint {
        match self {
            Column::Name => Constraint::Fill(1),
            Column::Size => Constraint::Length(10),
            Column::Modified => Constraint::Length(11),
        }
    }
}

/// Picks which columns fit `width` (the row list's inner width), Name
/// first and unconditional, then Size, then Modified -- the same
/// priority order most file managers use when their window narrows.
fn visible_columns(width: u16) -> Vec<Column> {
    if width >= 64 {
        vec![Column::Name, Column::Size, Column::Modified]
    } else if width >= 46 {
        vec![Column::Name, Column::Size]
    } else {
        vec![Column::Name]
    }
}

/// Draws the picker: a bordered, titled `Table` with per-row icons and a
/// one-line key-binding hint along the bottom. `now` drives the "modified"
/// column's relative-time text and is sampled once per frame by the
/// caller, not re-sampled per row.
pub fn render(frame: &mut Frame, screen_area: Rect, overlay: &ExplorerOverlay, now: SystemTime) {
    let layout = layout_for(screen_area);
    frame.render_widget(Clear, layout.popup_area);

    let action = match overlay.selection {
        ExplorerSelection::File => "Open File",
        ExplorerSelection::Folder => "Open Folder",
    };
    let title = theme::chrome_title(&format!("{action} — {}", overlay.current_dir.display()));
    frame.render_widget(theme::block(true).title(title), layout.popup_area);

    let columns = visible_columns(layout.rows_area.width);
    let widths: Vec<Constraint> = columns.iter().map(|column| column.width()).collect();
    let header = Row::new(
        columns
            .iter()
            .map(|column| Cell::from(column.label()))
            .collect::<Vec<_>>(),
    )
    .style(Style::new().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = overlay
        .entries
        .iter()
        .map(|entry| build_row(entry, &columns, now))
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(theme::selected_style());

    let mut table_state = TableState::new()
        .with_offset(overlay.offset)
        .with_selected((!overlay.entries.is_empty()).then_some(overlay.selected));
    frame.render_stateful_widget(table, layout.table_area, &mut table_state);

    let hint = if let Some(manual_path) = &overlay.manual_path {
        format!("Path: {manual_path}_ · Enter select folder · Esc return to browser")
    } else if overlay.entries.is_empty() {
        "(empty directory) · ←/⌫ up · Ctrl+L enter path · Ctrl+H hidden files · Esc cancel"
            .to_string()
    } else {
        if overlay.selection == ExplorerSelection::Folder {
            "↑↓/j/k move · Enter select folder · →/l descend · Ctrl+L enter path · ←/⌫/h up · Ctrl+H hidden files · Esc cancel".to_string()
        } else {
            "↑↓/j/k move · →/Enter/l open · right-click .md board · ←/⌫/h up · Ctrl+H hidden files · Esc cancel".to_string()
        }
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::new().add_modifier(Modifier::DIM)),
        layout.hint_area,
    );
}

fn build_row(entry: &ExplorerEntry, columns: &[Column], now: SystemTime) -> Row<'static> {
    let cells = columns
        .iter()
        .map(|column| match column {
            Column::Name => Cell::from(name_line(entry)),
            Column::Size => Cell::from(Line::from(size_text(entry)).alignment(Alignment::Right)),
            Column::Modified => {
                Cell::from(Line::from(modified_text(entry, now)).alignment(Alignment::Right))
            }
        })
        .collect::<Vec<_>>();
    Row::new(cells)
}

fn name_line(entry: &ExplorerEntry) -> Line<'static> {
    let (icon, style) = if entry.name == ".." {
        ("⬆ ", Style::new().fg(Color::Gray))
    } else if entry.is_dir {
        ("📁 ", Style::new().fg(Color::Cyan))
    } else if entry.is_symlink {
        ("🔗 ", Style::new().fg(Color::Magenta))
    } else {
        ("📄 ", Style::new().fg(Color::Gray))
    };
    Line::from(vec![
        Span::raw(icon),
        Span::styled(entry.name.clone(), style),
    ])
}

fn size_text(entry: &ExplorerEntry) -> String {
    match entry.size {
        Some(bytes) => format_size(bytes),
        None => "—".to_string(),
    }
}

fn modified_text(entry: &ExplorerEntry, now: SystemTime) -> String {
    match entry.modified {
        Some(modified) => format_modified(modified, now),
        None => "—".to_string(),
    }
}

/// Formats a byte count the way a file manager would: whole bytes below
/// 1 KB, one decimal place from KB up to TB.
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

/// Formats a modification time relative to `now` as a compact age, e.g.
/// `3m ago`, `5h ago`, `2d ago`, `6w ago`, `4mo ago`, `2y ago`.
fn format_modified(modified: SystemTime, now: SystemTime) -> String {
    let Ok(elapsed) = now.duration_since(modified) else {
        // `modified` is in the future (clock skew, or a file touched with
        // a forged timestamp) -- there's no sensible age to show.
        return "just now".to_string();
    };
    let secs = elapsed.as_secs();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else if secs < 604_800 {
        format!("{}d ago", secs / 86_400)
    } else if secs < 2_592_000 {
        format!("{}w ago", secs / 604_800)
    } else if secs < 31_536_000 {
        format!("{}mo ago", secs / 2_592_000)
    } else {
        format!("{}y ago", secs / 31_536_000)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-explorer-overlay-tests")
            .join(format!("{:?}-{label}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn key_event(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    const SCREEN: Rect = Rect::new(0, 0, 120, 40);

    #[test]
    fn lists_directories_before_files_alphabetically_and_skips_hidden_entries() {
        let dir = scratch_dir("listing");
        std::fs::create_dir(dir.join("zeta")).expect("mkdir");
        std::fs::create_dir(dir.join("Alpha")).expect("mkdir");
        std::fs::write(dir.join("beta.txt"), b"hi").expect("write");
        std::fs::write(dir.join(".hidden"), b"hi").expect("write");

        let overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        let names: Vec<&str> = overlay.entries.iter().map(|e| e.name.as_str()).collect();

        // ".." (dir has a parent), then dirs alphabetically, then files;
        // ".hidden" is filtered out until hidden files are shown.
        assert_eq!(names, vec!["..", "Alpha", "zeta", "beta.txt"]);
    }

    #[test]
    fn ctrl_h_toggles_hidden_files() {
        let dir = scratch_dir("hidden-toggle");
        std::fs::write(dir.join(".env"), b"secret").expect("write");

        let mut overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        assert!(overlay.entries.iter().all(|e| e.name != ".env"));

        overlay
            .handle(
                &key_event(KeyCode::Char('h'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("toggle hidden should not error");
        assert!(overlay.entries.iter().any(|e| e.name == ".env"));
    }

    #[test]
    fn enter_on_a_directory_descends_and_enter_on_a_file_returns_its_path() {
        let dir = scratch_dir("descend");
        std::fs::create_dir(dir.join("sub")).expect("mkdir");
        std::fs::write(dir.join("sub").join("note.txt"), b"hi").expect("write");

        let mut overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        // Entries are ["..", "sub"] -- select "sub".
        overlay.selected = overlay
            .entries
            .iter()
            .position(|e| e.name == "sub")
            .expect("sub dir listed");

        let picked = overlay
            .handle(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("descend should not error");
        assert_eq!(picked, None, "descending into a directory picks nothing");
        assert_eq!(overlay.current_dir, dir.join("sub"));

        overlay.selected = overlay
            .entries
            .iter()
            .position(|e| e.name == "note.txt")
            .expect("note.txt listed");
        let picked = overlay
            .handle(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("pick should not error");
        assert_eq!(picked, Some(dir.join("sub").join("note.txt")));
    }

    #[test]
    fn folder_mode_selects_the_current_directory_and_uses_right_to_descend() {
        let dir = scratch_dir("folder-select");
        std::fs::create_dir(dir.join("sub")).expect("mkdir");
        std::fs::write(dir.join("ignored.txt"), b"not selectable").expect("write file");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir).expect("open folder picker");
        assert!(overlay.entries.iter().all(|entry| entry.is_dir));
        overlay.selected = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "sub")
            .expect("sub dir listed");

        let descended = overlay
            .handle(&key_event(KeyCode::Right, KeyModifiers::NONE), SCREEN)
            .expect("right should descend");
        assert_eq!(descended, None);
        assert_eq!(overlay.current_dir, dir.join("sub"));

        let selected = overlay
            .handle(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("enter should select folder");
        assert_eq!(selected, Some(dir.join("sub")));
    }

    #[test]
    fn folder_mode_accepts_a_manually_entered_directory_path() {
        let dir = scratch_dir("manual-path");
        let selected_directory = dir.join("nested");
        std::fs::create_dir(&selected_directory).expect("mkdir");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir).expect("open folder picker");

        overlay
            .handle(
                &key_event(KeyCode::Char('l'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("open manual path entry");
        overlay
            .handle(
                &key_event(KeyCode::Char('u'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("clear manual path entry");
        for character in selected_directory.display().to_string().chars() {
            overlay
                .handle(
                    &key_event(KeyCode::Char(character), KeyModifiers::NONE),
                    SCREEN,
                )
                .expect("type manual path");
        }

        let selected = overlay
            .handle(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("manual path should select its directory");
        assert_eq!(selected, Some(selected_directory));
    }

    #[test]
    fn folder_mode_double_click_selects_the_clicked_directory() {
        let dir = scratch_dir("folder-mouse-select");
        let selected_directory = dir.join("docs");
        std::fs::create_dir(&selected_directory).expect("mkdir");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir).expect("open folder picker");
        let index = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "docs")
            .expect("docs directory listed");
        let layout = layout_for(SCREEN);
        let row = layout.rows_area.y + index as u16;

        let first_click = mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            layout.rows_area.x,
            row,
        );
        assert_eq!(
            overlay.handle(&first_click, SCREEN).expect("select docs"),
            None
        );

        let second_click = mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            layout.rows_area.x,
            row,
        );
        assert_eq!(
            overlay
                .handle(&second_click, SCREEN)
                .expect("select docs folder"),
            Some(selected_directory)
        );
    }

    #[test]
    fn backspace_navigates_to_the_parent_directory() {
        let dir = scratch_dir("ascend");
        std::fs::create_dir(dir.join("sub")).expect("mkdir");

        let mut overlay = ExplorerOverlay::open_at(&dir.join("sub")).expect("open picker");
        overlay
            .handle(&key_event(KeyCode::Backspace, KeyModifiers::NONE), SCREEN)
            .expect("ascend should not error");
        assert_eq!(overlay.current_dir, dir);
    }

    #[test]
    fn arrow_keys_move_the_selection_and_clamp_at_the_ends() {
        let dir = scratch_dir("arrows");
        for name in ["a", "b", "c"] {
            std::fs::write(dir.join(name), b"hi").expect("write");
        }
        let mut overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        assert_eq!(overlay.selected, 0);

        overlay
            .handle(&key_event(KeyCode::Up, KeyModifiers::NONE), SCREEN)
            .expect("up at top should not error");
        assert_eq!(overlay.selected, 0, "cannot move above the first row");

        overlay
            .handle(&key_event(KeyCode::Down, KeyModifiers::NONE), SCREEN)
            .expect("down should not error");
        assert_eq!(overlay.selected, 1);

        overlay
            .handle(&key_event(KeyCode::End, KeyModifiers::NONE), SCREEN)
            .expect("end should not error");
        assert_eq!(overlay.selected, overlay.entries.len() - 1);
    }

    #[test]
    fn clicking_a_row_selects_it_and_clicking_it_again_activates_it() {
        let dir = scratch_dir("mouse-click");
        std::fs::write(dir.join("note.txt"), b"hi").expect("write");

        let mut overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        let index = overlay
            .entries
            .iter()
            .position(|e| e.name == "note.txt")
            .expect("note.txt listed");
        let layout = layout_for(SCREEN);
        let row = layout.rows_area.y + index as u16;
        let column = layout.rows_area.x;

        let first_click = mouse_event(MouseEventKind::Down(MouseButton::Left), column, row);
        let picked = overlay.handle(&first_click, SCREEN).expect("click");
        assert_eq!(picked, None, "the first click only selects the row");
        assert_eq!(overlay.selected, index);

        let second_click = mouse_event(MouseEventKind::Down(MouseButton::Left), column, row);
        let picked = overlay.handle(&second_click, SCREEN).expect("click");
        assert_eq!(
            picked,
            Some(dir.join("note.txt")),
            "clicking the already-selected row activates it"
        );
    }

    #[test]
    fn file_at_mouse_returns_only_an_ordinary_file_row() {
        let dir = scratch_dir("file-context-target");
        std::fs::create_dir(dir.join("folder")).expect("mkdir");
        std::fs::write(dir.join("board.md"), b"# Backlog\n").expect("write markdown");
        let overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        let layout = layout_for(SCREEN);

        let markdown_index = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "board.md")
            .expect("markdown file listed");
        let markdown_mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: layout.rows_area.x,
            row: layout.rows_area.y + markdown_index as u16,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            overlay.file_at_mouse(markdown_mouse, SCREEN),
            Some(dir.join("board.md"))
        );

        let directory_index = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "folder")
            .expect("directory listed");
        let directory_mouse = MouseEvent {
            row: layout.rows_area.y + directory_index as u16,
            ..markdown_mouse
        };
        assert_eq!(overlay.file_at_mouse(directory_mouse, SCREEN), None);
    }

    #[test]
    fn scroll_wheel_moves_the_selection() {
        let dir = scratch_dir("scroll");
        for name in ["a", "b", "c", "d", "e"] {
            std::fs::write(dir.join(name), b"hi").expect("write");
        }
        let mut overlay = ExplorerOverlay::open_at(&dir).expect("open picker");
        let layout = layout_for(SCREEN);
        let inside = Position::new(layout.rows_area.x, layout.rows_area.y);

        overlay
            .handle(
                &mouse_event(MouseEventKind::ScrollDown, inside.x, inside.y),
                SCREEN,
            )
            .expect("scroll should not error");
        assert_eq!(overlay.selected, 3);
    }

    #[test]
    fn clicking_outside_the_row_list_does_nothing() {
        let dir = scratch_dir("outside-click");
        std::fs::write(dir.join("note.txt"), b"hi").expect("write");
        let mut overlay = ExplorerOverlay::open_at(&dir).expect("open picker");

        let picked = overlay
            .handle(
                &mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0),
                SCREEN,
            )
            .expect("click outside should not error");
        assert_eq!(picked, None);
        assert_eq!(overlay.selected, 0);
    }

    #[test]
    fn format_size_uses_whole_bytes_below_1kb_and_one_decimal_above() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn format_modified_buckets_into_compact_relative_units() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(format_modified(now, now), "just now");
        assert_eq!(
            format_modified(now - Duration::from_secs(120), now),
            "2m ago"
        );
        assert_eq!(
            format_modified(now - Duration::from_secs(3 * 3_600), now),
            "3h ago"
        );
        assert_eq!(
            format_modified(now - Duration::from_secs(2 * 86_400), now),
            "2d ago"
        );
    }

    #[test]
    fn visible_columns_drop_modified_then_size_as_width_shrinks() {
        assert!(matches!(
            visible_columns(80).as_slice(),
            [Column::Name, Column::Size, Column::Modified]
        ));
        assert!(matches!(
            visible_columns(50).as_slice(),
            [Column::Name, Column::Size]
        ));
        assert!(matches!(visible_columns(20).as_slice(), [Column::Name]));
    }
}
