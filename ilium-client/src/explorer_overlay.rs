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
use ratatui::widgets::{
    Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState,
};
use ratatui::Frame;

use crate::filesystem::explorer::{ExplorerEntry, ExplorerRead};
use crate::layout::centered_rect;
use crate::theme;
use ilium_execution::{Client, JobOutcome, JobPoll, Receipt, RejectReason, Retained};
use std::sync::Arc;

/// One listed directory entry: a real file/directory, or the synthetic
/// `..` entry that steps up to the parent directory.
///
/// A modal file-picker overlay, rooted at the originating pane's cwd and
/// re-listed every time the user navigates into a different directory.
pub struct ExplorerOverlay {
    current_dir: PathBuf,
    entries: Vec<ExplorerEntry>,
    selected: usize,
    offset: usize,
    show_hidden: bool,
    selection: ExplorerSelection,
    /// Folder pickers keep navigation and confirmation distinct. This flag
    /// records whether keyboard focus is on the explicit bottom action.
    folder_action_focused: bool,
    /// The operation that will receive `current_dir` after explicit
    /// confirmation. File pickers have no such action.
    folder_action_label: Option<String>,
    /// An explicit path entry field. Keeping it inside the same overlay
    /// preserves the directory listing while a user pastes/types a path.
    manual_path: Option<String>,
    revision: u64,
    desired: Option<ExplorerRead>,
    active: Option<(ExplorerRead, Receipt<ExplorerRead>)>,
    execution: Option<Client>,
    listing_hold: Option<Retained<()>>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSelection {
    File,
    Folder,
}

/// What the picker did with one input event. Callers close the overlay only
/// on `Picked` (routing the path onward) or on an `Ignored` Escape press --
/// `Consumed` exists so an event the overlay already acted on (most
/// importantly Esc closing the manual-path field, which its hint promises
/// "return[s] to browser") is never double-interpreted by the caller as
/// "close the whole picker".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerOutcome {
    /// The event chose a file or folder; the caller closes the overlay and
    /// routes the path to the appropriate pane/tree action.
    Picked(PathBuf),
    /// The overlay handled the event itself (selection, navigation, the
    /// modal manual-path field); the overlay stays open.
    Consumed,
    /// The event means nothing to the overlay; the caller may apply its own
    /// bindings, e.g. Esc closing the picker.
    Ignored,
}

impl ExplorerOverlay {
    /// Opens the picker rooted at `dir` (the originating pane's cwd).
    pub fn open_at(dir: &Path) -> anyhow::Result<Self> {
        Self::open(dir, ExplorerSelection::File)
    }

    pub fn open_folder_at(dir: &Path) -> anyhow::Result<Self> {
        Self::open_folder_for(dir, "Select Folder")
    }

    /// Opens a directory-only picker with a caller-specific confirmation
    /// label, keeping the same navigation contract for every folder use.
    pub fn open_folder_for(
        dir: &Path,
        folder_action_label: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let mut overlay = Self::open(dir, ExplorerSelection::Folder)?;
        overlay.folder_action_label = Some(folder_action_label.into());
        Ok(overlay)
    }

    fn open(dir: &Path, selection: ExplorerSelection) -> anyhow::Result<Self> {
        let mut overlay = Self {
            current_dir: dir.to_path_buf(),
            entries: Vec::new(),
            selected: 0,
            offset: 0,
            show_hidden: false,
            selection,
            folder_action_focused: false,
            folder_action_label: None,
            manual_path: None,
            revision: 0,
            desired: None,
            active: None,
            execution: None,
            listing_hold: None,
            error: None,
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
        self.queue_read(self.current_dir.clone(), self.show_hidden, None)
    }
    fn navigate_to(&mut self, dir: PathBuf) -> anyhow::Result<()> {
        self.queue_read(
            dir,
            self.desired
                .as_ref()
                .map_or(self.show_hidden, |request| request.show_hidden),
            None,
        )
    }
    fn queue_read(
        &mut self,
        directory: PathBuf,
        show_hidden: bool,
        manual_input: Option<String>,
    ) -> anyhow::Result<()> {
        if directory.capacity() > 64 * 1024
            || manual_input
                .as_ref()
                .is_some_and(|input| input.capacity() > 64 * 1024)
        {
            anyhow::bail!("Explorer path exceeds retained limit");
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Explorer revision exhausted"))?;
        self.desired = Some(ExplorerRead {
            revision: self.revision,
            directory,
            show_hidden,
            selection: self.selection,
            manual_input,
        });
        if let Some((_, receipt)) = &self.active {
            receipt.cancel();
        }
        self.error = None;
        Ok(())
    }
    pub(crate) fn attach_execution(
        mut self,
        client: Client,
        ready: Arc<tokio::sync::Notify>,
    ) -> Self {
        self.execution = Some(client.with_completion_wake(move || ready.notify_one()));
        self.poll();
        self
    }
    #[cfg(test)]
    pub(crate) fn preparation_pending(&self) -> bool {
        self.active.is_some() || self.desired.is_some()
    }
    pub(crate) fn close_preparation(&mut self) {
        self.desired = None;
        self.execution = None;
        if let Some((_, receipt)) = &self.active {
            receipt.cancel();
        }
        self.active = None;
    }

    pub(crate) fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some((request, receipt)) = &mut self.active {
            let mut result = None;
            match receipt.try_take() {
                JobPoll::Pending => return false,
                JobPoll::Ready(outcome) => {
                    let hold = outcome.map(|outcome| {
                        result = Some(match outcome {
                            JobOutcome::Finished(result) => result,
                            _ => Err("Explorer worker did not complete preparation".into()),
                        });
                    });
                    if request.revision == self.revision
                        && request
                            .manual_input
                            .as_ref()
                            .is_none_or(|input| self.manual_path.as_ref() == Some(input))
                    {
                        match result {
                            Some(Ok(listing)) => {
                                self.current_dir = listing.directory;
                                self.entries = listing.entries;
                                self.selected = 0;
                                self.offset = 0;
                                self.show_hidden = request.show_hidden;
                                if request.manual_input.is_some() {
                                    self.manual_path = None;
                                }
                                self.listing_hold = Some(hold);
                                self.error = None;
                                changed = true;
                            }
                            Some(Err(error)) => {
                                self.error = Some(error);
                                changed = true;
                            }
                            None => {}
                        }
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.error = Some("Explorer receipt lost; listing unchanged".into());
                    changed = true;
                }
            }
            self.active = None;
        }
        let Some(client) = &self.execution else {
            return changed;
        };
        if let Some(request) = self.desired.take() {
            match client.try_submit(
                ilium_execution::Lane::Io,
                ExplorerRead::COST,
                request.clone(),
            ) {
                Ok(receipt) => self.active = Some((request, receipt)),
                Err(rejected)
                    if matches!(
                        rejected.reason,
                        RejectReason::Busy
                            | RejectReason::QueueFull
                            | RejectReason::JobLimit
                            | RejectReason::InputBytes
                            | RejectReason::ResultBytes
                    ) =>
                {
                    self.desired = Some(rejected.value)
                }
                Err(rejected) => {
                    self.error = Some(format!("Explorer not admitted: {:?}", rejected.reason));
                    changed = true;
                }
            }
        }
        changed
    }

    /// Moves the selection by `delta` rows (negative = up), clamped to the
    /// listing's bounds, and scrolls just enough to keep it in view. Also
    /// returns keyboard focus to the row list -- every operation that moves
    /// the row cursor (this one, `select_first`, `select_last`) must clear
    /// `folder_action_focused` here rather than at each call site, or a
    /// cursor move that forgets to clear it (as PageUp/PageDown/Home/End
    /// once did) leaves the bottom action highlighted while Enter silently
    /// confirms the folder instead of acting on the row the user just
    /// landed on.
    fn move_selection(&mut self, delta: i64, viewport_rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.folder_action_focused = false;
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
        self.folder_action_focused = false;
        self.selected = 0;
        self.offset = 0;
    }

    fn select_last(&mut self, viewport_rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.folder_action_focused = false;
        self.selected = self.entries.len() - 1;
        self.offset = self.selected.saturating_sub(viewport_rows.max(1) - 1);
    }

    /// Descends into the selected directory (re-listing in place, cursor
    /// back at the top) or, for a file, returns its path so the caller can
    /// open it. Folder rows always navigate: confirmation is deliberately
    /// reserved for the picker action at the bottom of the dialog.
    fn activate(&mut self) -> anyhow::Result<Option<PathBuf>> {
        let Some(entry) = self.entries.get(self.selected) else {
            return Ok(None);
        };
        if entry.is_dir {
            self.navigate_to(entry.path.clone())?;
            Ok(None)
        } else {
            Ok(Some(entry.path.clone()))
        }
    }

    /// Descends into the selected directory without treating it as a file.
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

    /// Returns the currently displayed directory only after an intentional
    /// confirmation gesture. This gives keyboard and mouse users one stable,
    /// discoverable way to commit a folder choice.
    fn confirm_current_folder(&self) -> Option<PathBuf> {
        (self.selection == ExplorerSelection::Folder).then(|| self.current_dir.clone())
    }

    /// Lists `current_dir` with the flipped hidden-files flag before
    /// committing it, for the same reason `navigate_to` lists before
    /// committing a directory change: a failed re-list must not leave
    /// `show_hidden` disagreeing with what `entries` actually contains.
    fn toggle_hidden(&mut self) -> anyhow::Result<()> {
        let hidden = self
            .desired
            .as_ref()
            .or_else(|| self.active.as_ref().map(|(request, _)| request))
            .map_or(self.show_hidden, |request| request.show_hidden);
        self.queue_read(self.current_dir.clone(), !hidden, None)
    }

    /// Feeds a crossterm event to the picker. See `ExplorerOutcome` for the
    /// contract: `Picked` closes the overlay with a chosen path, `Consumed`
    /// keeps it open, and `Ignored` lets the caller apply its own bindings
    /// (Esc closing the picker being the one that matters).
    pub fn handle(&mut self, event: &Event, screen_area: Rect) -> anyhow::Result<ExplorerOutcome> {
        match event {
            Event::Key(key) => self.handle_key(key, screen_area),
            Event::Mouse(mouse) => self.handle_mouse(*mouse, screen_area),
            _ => Ok(ExplorerOutcome::Ignored),
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

    fn handle_key(&mut self, key: &KeyEvent, screen_area: Rect) -> anyhow::Result<ExplorerOutcome> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(ExplorerOutcome::Ignored);
        }
        if let Some(manual_path) = self.manual_path.as_mut() {
            // The manual-path field is modal: every key press belongs to it
            // while it's open (its hint promises "Esc return to browser"),
            // so nothing here may report `Ignored` -- an `Ignored` Esc would
            // let the caller close the whole picker instead.
            match key.code {
                KeyCode::Esc => self.manual_path = None,
                KeyCode::Enter => {
                    let entered = PathBuf::from(manual_path.trim());
                    let candidate = if entered.is_absolute() {
                        entered
                    } else {
                        self.current_dir.join(entered)
                    };
                    let input = manual_path.clone();
                    self.queue_read(candidate, self.show_hidden, Some(input))?;
                    return Ok(ExplorerOutcome::Consumed);
                }
                KeyCode::Backspace => {
                    manual_path.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    manual_path.clear();
                }
                KeyCode::Char(character)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && manual_path.len() < 64 * 1024 - 4 =>
                {
                    manual_path.push(character);
                }
                _ => {}
            }
            return Ok(ExplorerOutcome::Consumed);
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
            KeyCode::Tab if self.selection == ExplorerSelection::Folder => {
                self.folder_action_focused = true;
            }
            KeyCode::BackTab if self.selection == ExplorerSelection::Folder => {
                self.folder_action_focused = false;
            }
            KeyCode::Enter | KeyCode::Char(' ') if self.folder_action_focused => {
                return Ok(confirmation_outcome(self.confirm_current_folder()));
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(confirmation_outcome(self.confirm_current_folder()));
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1, viewport_rows),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1, viewport_rows),
            KeyCode::PageUp => self.move_selection(-(viewport_rows as i64), viewport_rows),
            KeyCode::PageDown => self.move_selection(viewport_rows as i64, viewport_rows),
            KeyCode::Home => self.select_first(),
            KeyCode::End => self.select_last(viewport_rows),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => self.navigate_up()?,
            KeyCode::Enter => return self.activate().map(activation_outcome),
            KeyCode::Right | KeyCode::Char('l') if self.selection == ExplorerSelection::Folder => {
                self.navigate_selected_directory()?
            }
            KeyCode::Right | KeyCode::Char('l') => return self.activate().map(activation_outcome),
            _ => return Ok(ExplorerOutcome::Ignored),
        }
        Ok(ExplorerOutcome::Consumed)
    }

    fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        screen_area: Rect,
    ) -> anyhow::Result<ExplorerOutcome> {
        // The manual-path text field is a modal sub-state: `handle_key`
        // routes every key into editing it while it's open. Mouse events
        // must be gated the same way, or a click/scroll landing on the
        // still-rendered background table would change the selection --
        // or, worse, activate a row and close the picker -- silently
        // discarding whatever path the user was mid-way through typing.
        if self.manual_path.is_some() {
            return Ok(ExplorerOutcome::Consumed);
        }
        let layout = layout_for(screen_area);
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.move_selection(-3, usize::from(layout.rows_area.height));
                Ok(ExplorerOutcome::Consumed)
            }
            MouseEventKind::ScrollDown => {
                self.move_selection(3, usize::from(layout.rows_area.height));
                Ok(ExplorerOutcome::Consumed)
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let position = Position::new(mouse.column, mouse.row);
                if self.selection == ExplorerSelection::Folder
                    && layout.action_area.contains(position)
                {
                    return Ok(confirmation_outcome(self.confirm_current_folder()));
                }
                if let Some(index) = row_at(&layout, self.offset, self.entries.len(), position) {
                    if self.selection == ExplorerSelection::File && index == self.selected {
                        return self.activate().map(activation_outcome);
                    }
                    self.selected = index;
                    self.folder_action_focused = false;
                    if self.selection == ExplorerSelection::Folder {
                        self.navigate_selected_directory()?;
                    }
                    return Ok(ExplorerOutcome::Consumed);
                }
                Ok(ExplorerOutcome::Ignored)
            }
            _ => Ok(ExplorerOutcome::Ignored),
        }
    }
}

/// Maps `confirm_current_folder`'s result to an outcome: the confirmation
/// gestures are picker chrome, so even when they yield no path (a File
/// picker receiving Ctrl+Enter) they count as handled, not `Ignored`.
fn confirmation_outcome(confirmed: Option<PathBuf>) -> ExplorerOutcome {
    match confirmed {
        Some(path) => ExplorerOutcome::Picked(path),
        None => ExplorerOutcome::Consumed,
    }
}

/// Maps `activate`'s result to an outcome: a file pick closes the overlay,
/// while descending into a directory (or an empty listing) keeps it open.
fn activation_outcome(activated: Option<PathBuf>) -> ExplorerOutcome {
    match activated {
        Some(path) => ExplorerOutcome::Picked(path),
        None => ExplorerOutcome::Consumed,
    }
}

/// Lists `dir`'s entries: `..` first when it has a parent, then
/// directories, then files, each group sorted alphabetically
/// case-insensitively. A single unreadable entry (permission race,
/// vanished mid-scan) is skipped rather than failing the whole listing.
/// Pure with respect to `ExplorerOverlay` -- callers decide whether/when
/// to commit the result, which is what lets `navigate_to` and
/// `toggle_hidden` list before mutating any picker state.
///
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
    scrollbar_area: Rect,
    /// Explicit folder-confirmation control. It shares the popup's width so
    /// the large target remains easy to acquire with a mouse.
    action_area: Rect,
    hint_area: Rect,
}

fn layout_for(screen_area: Rect) -> ExplorerLayout {
    let popup_area = centered_rect(70, 70, screen_area);
    let inner = theme::block(true).inner(popup_area);
    let sections = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let scrollbar_width = u16::from(sections[0].width >= 2);
    let table_area = Rect {
        width: sections[0].width.saturating_sub(scrollbar_width),
        ..sections[0]
    };
    let action_area = sections[1];
    let hint_area = sections[2];
    let rows_area = Rect::new(
        table_area.x,
        table_area.y.saturating_add(table_area.height.min(1)),
        table_area.width,
        table_area.height.saturating_sub(1),
    );
    ExplorerLayout {
        popup_area,
        table_area,
        rows_area,
        scrollbar_area: Rect::new(
            table_area.right(),
            rows_area.y,
            scrollbar_width,
            rows_area.height,
        ),
        action_area,
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
/// explicit folder-confirmation control and a one-line key-binding hint along
/// the bottom. `now` drives the "modified"
/// column's relative-time text and is sampled once per frame by the
/// caller, not re-sampled per row.
pub fn render(frame: &mut Frame, screen_area: Rect, overlay: &ExplorerOverlay, now: SystemTime) {
    let layout = layout_for(screen_area);
    frame.render_widget(Clear, layout.popup_area);

    let action = match overlay.selection {
        ExplorerSelection::File => "Open File",
        ExplorerSelection::Folder => "Open Folder",
    };
    let preparation = if let Some(error) = &overlay.error {
        format!(" — {error}")
    } else if overlay.active.is_some() || overlay.desired.is_some() {
        " — Loading…".to_owned()
    } else {
        String::new()
    };
    let title = theme::chrome_title(&format!(
        "{action} — {}{preparation}",
        overlay.current_dir.display()
    ));
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
    if overlay.entries.len() > usize::from(layout.rows_area.height)
        && !layout.scrollbar_area.is_empty()
    {
        let mut state = ScrollbarState::new(overlay.entries.len())
            .position(table_state.offset())
            .viewport_content_length(usize::from(layout.rows_area.height));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .style(theme::border_style(false)),
            layout.scrollbar_area,
            &mut state,
        );
    }

    if let Some(folder_action_label) = &overlay.folder_action_label {
        let action_style = if overlay.folder_action_focused {
            theme::selected_style()
        } else {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        };
        let action_text = format!(" [ {folder_action_label} ] ");
        frame.render_widget(
            Paragraph::new(action_text)
                .alignment(Alignment::Center)
                .style(action_style),
            layout.action_area,
        );
    }

    let hint = if let Some(manual_path) = &overlay.manual_path {
        format!("Path: {manual_path}_ · Enter go to folder · Esc return to browser")
    } else {
        // The binding list depends only on picker mode, never on whether
        // the current directory happens to be empty -- an empty File
        // picker still needs "Ctrl+H hidden files" (the one binding that
        // can actually populate it), not the Folder-mode bindings.
        let bindings = if overlay.selection == ExplorerSelection::Folder {
            "↑↓/j/k move · Enter/→ open · Tab action · Ctrl+Enter confirm · ←/⌫/h up · Ctrl+L path · Esc cancel"
        } else {
            "↑↓/j/k move · →/Enter/l open · right-click .md board · ←/⌫/h up · Ctrl+H hidden files · Esc cancel"
        };
        if overlay.entries.is_empty() {
            format!("(empty directory) · {bindings}")
        } else {
            bindings.to_string()
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

impl Drop for ExplorerOverlay {
    fn drop(&mut self) {
        if let Some((_, receipt)) = &self.active {
            receipt.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn explorer_scrollbar_is_outside_file_rows_and_confirmation_controls() {
        for (width, height) in [(120, 40), (80, 24), (40, 12), (12, 6), (1, 1)] {
            let screen = Rect::new(0, 0, width, height);
            let layout = layout_for(screen);
            assert_eq!(layout.table_area.right(), layout.scrollbar_area.x);
            assert_eq!(layout.rows_area.width, layout.table_area.width);
            assert!(layout.scrollbar_area.right() <= screen.right());
            assert!(layout.scrollbar_area.bottom() <= layout.action_area.y);
            if !layout.scrollbar_area.is_empty() {
                assert_eq!(
                    row_at(
                        &layout,
                        0,
                        100,
                        Position::new(layout.scrollbar_area.x, layout.scrollbar_area.y)
                    ),
                    None
                );
            }
        }
    }

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-explorer-overlay-tests")
            .join(format!("{:?}-{label}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn real_worker_fences_old_listing_and_retains_it_on_scan_error() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("first.txt"), b"first").unwrap();
        let mut overlay = ExplorerOverlay::open_at(directory.path())
            .unwrap()
            .attach_execution(
                crate::execution::test_client(),
                Arc::new(tokio::sync::Notify::new()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while overlay.active.is_some() || overlay.desired.is_some() {
            overlay.poll();
            assert!(std::time::Instant::now() < deadline, "scan never completed");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(overlay
            .entries
            .iter()
            .any(|entry| entry.name == "first.txt"));
        let original = overlay.current_dir.clone();
        overlay
            .navigate_to(directory.path().join("missing"))
            .unwrap();
        // Navigation admission leaves the complete old listing available.
        assert_eq!(overlay.current_dir, original);
        assert!(overlay
            .entries
            .iter()
            .any(|entry| entry.name == "first.txt"));
        while overlay.active.is_some() || overlay.desired.is_some() {
            overlay.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "scan failure never completed"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(overlay.current_dir, original);
        assert!(overlay.error.is_some());
        // Coalesced replacement fences an obsolete missing-directory result.
        overlay
            .navigate_to(directory.path().join("missing"))
            .unwrap();
        overlay.poll();
        overlay.navigate_to(directory.path().to_path_buf()).unwrap();
        while overlay.active.is_some() || overlay.desired.is_some() {
            overlay.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "replacement never completed"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(overlay.error.is_none());
        assert!(overlay
            .entries
            .iter()
            .any(|entry| entry.name == "first.txt"));
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

    impl ExplorerOverlay {
        fn settle_preparation_for_test(&mut self) {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while self.active.is_some() || self.desired.is_some() {
                self.poll();
                assert!(
                    std::time::Instant::now() < deadline,
                    "real explorer preparation did not settle"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn prepared_for_test(self) -> Self {
            let mut overlay = self.attach_execution(
                crate::execution::test_client(),
                Arc::new(tokio::sync::Notify::new()),
            );
            overlay.settle_preparation_for_test();
            overlay
        }
        fn handle_prepared(
            &mut self,
            event: &Event,
            screen: Rect,
        ) -> anyhow::Result<ExplorerOutcome> {
            let outcome = self.handle(event, screen)?;
            self.settle_preparation_for_test();
            Ok(outcome)
        }
    }

    #[test]
    fn lists_directories_before_files_alphabetically_and_skips_hidden_entries() {
        let dir = scratch_dir("listing");
        std::fs::create_dir(dir.join("zeta")).expect("mkdir");
        std::fs::create_dir(dir.join("Alpha")).expect("mkdir");
        std::fs::write(dir.join("beta.txt"), b"hi").expect("write");
        std::fs::write(dir.join(".hidden"), b"hi").expect("write");

        let overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
        let names: Vec<&str> = overlay.entries.iter().map(|e| e.name.as_str()).collect();

        // ".." (dir has a parent), then dirs alphabetically, then files;
        // ".hidden" is filtered out until hidden files are shown.
        assert_eq!(names, vec!["..", "Alpha", "zeta", "beta.txt"]);
    }

    #[test]
    fn ctrl_h_toggles_hidden_files() {
        let dir = scratch_dir("hidden-toggle");
        std::fs::write(dir.join(".env"), b"secret").expect("write");

        let mut overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
        assert!(overlay.entries.iter().all(|e| e.name != ".env"));

        overlay
            .handle_prepared(
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

        let mut overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
        // Entries are ["..", "sub"] -- select "sub".
        overlay.selected = overlay
            .entries
            .iter()
            .position(|e| e.name == "sub")
            .expect("sub dir listed");

        let picked = overlay
            .handle_prepared(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("descend should not error");
        assert_eq!(
            picked,
            ExplorerOutcome::Consumed,
            "descending into a directory picks nothing"
        );
        assert_eq!(overlay.current_dir, dir.join("sub"));

        overlay.selected = overlay
            .entries
            .iter()
            .position(|e| e.name == "note.txt")
            .expect("note.txt listed");
        let picked = overlay
            .handle_prepared(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("pick should not error");
        assert_eq!(
            picked,
            ExplorerOutcome::Picked(dir.join("sub").join("note.txt"))
        );
    }

    #[test]
    fn folder_mode_navigates_rows_and_confirms_current_directory_explicitly() {
        let dir = scratch_dir("folder-select");
        std::fs::create_dir(dir.join("sub")).expect("mkdir");
        std::fs::write(dir.join("ignored.txt"), b"not selectable").expect("write file");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir)
            .expect("open folder picker")
            .prepared_for_test();
        assert!(overlay.entries.iter().all(|entry| entry.is_dir));
        overlay.selected = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "sub")
            .expect("sub dir listed");

        let descended = overlay
            .handle_prepared(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("enter should descend");
        assert_eq!(descended, ExplorerOutcome::Consumed);
        assert_eq!(overlay.current_dir, dir.join("sub"));

        let selected = overlay
            .handle_prepared(&key_event(KeyCode::Enter, KeyModifiers::CONTROL), SCREEN)
            .expect("control-enter should confirm the current directory");
        assert_eq!(selected, ExplorerOutcome::Picked(dir.join("sub")));

        let mut overlay = ExplorerOverlay::open_folder_at(&dir)
            .expect("reopen folder picker")
            .prepared_for_test();
        overlay
            .handle_prepared(&key_event(KeyCode::Tab, KeyModifiers::NONE), SCREEN)
            .expect("tab should focus the confirmation action");
        let selected = overlay
            .handle_prepared(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("action enter should confirm the current directory");
        assert_eq!(selected, ExplorerOutcome::Picked(dir));
    }

    #[test]
    fn folder_mode_accepts_a_manually_entered_directory_path() {
        let dir = scratch_dir("manual-path");
        let selected_directory = dir.join("nested");
        std::fs::create_dir(&selected_directory).expect("mkdir");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir)
            .expect("open folder picker")
            .prepared_for_test();

        overlay
            .handle_prepared(
                &key_event(KeyCode::Char('l'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("open manual path entry");
        overlay
            .handle_prepared(
                &key_event(KeyCode::Char('u'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("clear manual path entry");
        for character in selected_directory.display().to_string().chars() {
            overlay
                .handle_prepared(
                    &key_event(KeyCode::Char(character), KeyModifiers::NONE),
                    SCREEN,
                )
                .expect("type manual path");
        }

        let selected = overlay
            .handle_prepared(&key_event(KeyCode::Enter, KeyModifiers::NONE), SCREEN)
            .expect("manual path should navigate to its directory");
        assert_eq!(selected, ExplorerOutcome::Consumed);
        // The overlay resolves what was typed, so the expectation is resolved
        // the same way: `%TEMP%` hands out an 8.3 short path while resolving it
        // yields the long name, and the two are the same directory.
        assert_eq!(
            overlay.current_dir,
            ilium_platform::paths::canonicalize(&selected_directory).unwrap_or(selected_directory)
        );
    }

    #[test]
    fn mouse_events_are_ignored_while_a_manual_path_is_being_typed() {
        let dir = scratch_dir("manual-path-mouse-guard");
        std::fs::create_dir(dir.join("sub")).expect("mkdir");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir)
            .expect("open folder picker")
            .prepared_for_test();
        let index = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "sub")
            .expect("sub dir listed");
        let selected_before = overlay.selected;
        assert_ne!(
            selected_before, index,
            "the clicked row must differ from the current selection for this test to be meaningful"
        );

        overlay
            .handle_prepared(
                &key_event(KeyCode::Char('l'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("open manual path entry");
        overlay
            .handle_prepared(
                &key_event(KeyCode::Char('u'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("clear manual path entry");
        overlay
            .handle_prepared(&key_event(KeyCode::Char('x'), KeyModifiers::NONE), SCREEN)
            .expect("type into manual path entry");

        let layout = layout_for(SCREEN);
        let row = layout.rows_area.y + index as u16;
        let click = mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            layout.rows_area.x,
            row,
        );
        let picked = overlay
            .handle_prepared(&click, SCREEN)
            .expect("click while typing a manual path should not error");

        assert_eq!(
            picked,
            ExplorerOutcome::Consumed,
            "a background click must not activate a row while the manual path field is open"
        );
        assert_eq!(
            overlay.selected, selected_before,
            "a background click must not move the row selection while the manual path field is open"
        );
        assert_eq!(
            overlay.manual_path.as_deref(),
            Some("x"),
            "the manual path being typed must survive an unrelated background mouse event"
        );
    }

    #[test]
    fn folder_mode_click_navigates_and_action_button_confirms() {
        let dir = scratch_dir("folder-mouse-select");
        let selected_directory = dir.join("docs");
        std::fs::create_dir(&selected_directory).expect("mkdir");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir)
            .expect("open folder picker")
            .prepared_for_test();
        let index = overlay
            .entries
            .iter()
            .position(|entry| entry.name == "docs")
            .expect("docs directory listed");
        let layout = layout_for(SCREEN);
        let row = layout.rows_area.y + index as u16;

        let click = mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            layout.rows_area.x,
            row,
        );
        assert_eq!(
            overlay
                .handle_prepared(&click, SCREEN)
                .expect("navigate to docs"),
            ExplorerOutcome::Consumed
        );
        assert_eq!(overlay.current_dir, selected_directory);

        let confirm_click = mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            layout.action_area.x,
            layout.action_area.y,
        );
        assert_eq!(
            overlay
                .handle_prepared(&confirm_click, SCREEN)
                .expect("confirm docs folder"),
            ExplorerOutcome::Picked(selected_directory)
        );
    }

    #[test]
    fn backspace_navigates_to_the_parent_directory() {
        let dir = scratch_dir("ascend");
        std::fs::create_dir(dir.join("sub")).expect("mkdir");

        let mut overlay = ExplorerOverlay::open_at(&dir.join("sub"))
            .expect("open picker")
            .prepared_for_test();
        overlay
            .handle_prepared(&key_event(KeyCode::Backspace, KeyModifiers::NONE), SCREEN)
            .expect("ascend should not error");
        assert_eq!(overlay.current_dir, dir);
    }

    #[test]
    fn arrow_keys_move_the_selection_and_clamp_at_the_ends() {
        let dir = scratch_dir("arrows");
        for name in ["a", "b", "c"] {
            std::fs::write(dir.join(name), b"hi").expect("write");
        }
        let mut overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
        assert_eq!(overlay.selected, 0);

        overlay
            .handle_prepared(&key_event(KeyCode::Up, KeyModifiers::NONE), SCREEN)
            .expect("up at top should not error");
        assert_eq!(overlay.selected, 0, "cannot move above the first row");

        overlay
            .handle_prepared(&key_event(KeyCode::Down, KeyModifiers::NONE), SCREEN)
            .expect("down should not error");
        assert_eq!(overlay.selected, 1);

        overlay
            .handle_prepared(&key_event(KeyCode::End, KeyModifiers::NONE), SCREEN)
            .expect("end should not error");
        assert_eq!(overlay.selected, overlay.entries.len() - 1);
    }

    #[test]
    fn clicking_a_row_selects_it_and_clicking_it_again_activates_it() {
        let dir = scratch_dir("mouse-click");
        std::fs::write(dir.join("note.txt"), b"hi").expect("write");

        let mut overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
        let index = overlay
            .entries
            .iter()
            .position(|e| e.name == "note.txt")
            .expect("note.txt listed");
        let layout = layout_for(SCREEN);
        let row = layout.rows_area.y + index as u16;
        let column = layout.rows_area.x;

        let first_click = mouse_event(MouseEventKind::Down(MouseButton::Left), column, row);
        let picked = overlay
            .handle_prepared(&first_click, SCREEN)
            .expect("click");
        assert_eq!(
            picked,
            ExplorerOutcome::Consumed,
            "the first click only selects the row"
        );
        assert_eq!(overlay.selected, index);

        let second_click = mouse_event(MouseEventKind::Down(MouseButton::Left), column, row);
        let picked = overlay
            .handle_prepared(&second_click, SCREEN)
            .expect("click");
        assert_eq!(
            picked,
            ExplorerOutcome::Picked(dir.join("note.txt")),
            "clicking the already-selected row activates it"
        );
    }

    #[test]
    fn file_at_mouse_returns_only_an_ordinary_file_row() {
        let dir = scratch_dir("file-context-target");
        std::fs::create_dir(dir.join("folder")).expect("mkdir");
        std::fs::write(dir.join("board.md"), b"# Backlog\n").expect("write markdown");
        let overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
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
        let mut overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();
        let layout = layout_for(SCREEN);
        let inside = Position::new(layout.rows_area.x, layout.rows_area.y);

        overlay
            .handle_prepared(
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
        let mut overlay = ExplorerOverlay::open_at(&dir)
            .expect("open picker")
            .prepared_for_test();

        let picked = overlay
            .handle_prepared(
                &mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0),
                SCREEN,
            )
            .expect("click outside should not error");
        assert_eq!(picked, ExplorerOutcome::Ignored);
        assert_eq!(overlay.selected, 0);
    }

    #[test]
    fn escape_while_typing_a_manual_path_returns_to_the_browser_without_closing() {
        let dir = scratch_dir("manual-path-escape");
        let mut overlay = ExplorerOverlay::open_folder_at(&dir)
            .expect("open folder picker")
            .prepared_for_test();

        overlay
            .handle_prepared(
                &key_event(KeyCode::Char('l'), KeyModifiers::CONTROL),
                SCREEN,
            )
            .expect("open manual path entry");
        assert!(overlay.manual_path.is_some());

        let outcome = overlay
            .handle_prepared(&key_event(KeyCode::Esc, KeyModifiers::NONE), SCREEN)
            .expect("escape should close only the manual path field");
        assert_eq!(
            outcome,
            ExplorerOutcome::Consumed,
            "the field's hint promises 'Esc return to browser', so the overlay must \
             report the Esc as consumed -- an Ignored Esc lets the caller close the picker"
        );
        assert_eq!(overlay.manual_path, None);

        let outcome = overlay
            .handle_prepared(&key_event(KeyCode::Esc, KeyModifiers::NONE), SCREEN)
            .expect("escape in the browser is the caller's to handle");
        assert_eq!(
            outcome,
            ExplorerOutcome::Ignored,
            "with no manual path field open, Esc belongs to the caller (it closes the picker)"
        );
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
