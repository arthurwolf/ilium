//! Renders `crate::app::Mode::Settings`: the full-screen settings view.
//!
//! Design brief (the user's own words -- see `crate::app::SettingsState`'s
//! doc comment for the full quote, kept there since `App` owns the state
//! this module only renders): the **entire** screen is replaced, aerated
//! rather than boxy, tab list on the left / settings panel on the right
//! with *no separating border* between them (unlike every bordered panel
//! in `crate::theme::block`), usable at any terminal size via a scrollbar
//! plus mouse-wheel scrolling, and every control is a live, self-applying
//! toggle/stepper -- never a buffered form with a Save button.
//!
//! Adding a fourth Appearance setting? Add a variant to
//! `crate::app::AppearanceRow` and its `ALL` array, then extend
//! [`appearance_row_label`]/[`appearance_row_description`]/
//! [`appearance_row_value`] and `App::settings_adjust_row` -- row count,
//! scroll bounds, and mouse hit-testing all fall out of `AppearanceRow::ALL`
//! automatically, nothing else here needs to change. Adding another *tab*
//! is a bigger change: extend `crate::app::SettingsTab::ALL` and add a
//! render/hit-test branch here for it.
//!
//! Rendering and mouse hit-testing both read the same geometry
//! ([`compute_layout`], [`appearance_content_hit`], [`tab_at`]) so the two
//! can never drift apart -- the same pattern `crate::modal`'s create-group
//! dialog and `crate::tree_ui`'s row controls already use.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;
use tui_tree_widget::TreeState;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{
    App, AppearanceRow, IconPickerColumnMode, IconPickerSearchStatus, InferenceRow,
    InferenceSettingField, InferenceTestState, KanbanBoardRow, OllamaModelDiscoveryState,
    SettingsState, SettingsTab, SoundRow,
};
use crate::config::{
    EditorSettings, KanbanBoardSettings, KeyboardSettings, SessionSettings, TerminalSettings,
    UiSettings,
};
use crate::icon_settings::{
    catalogue_icon_count, catalogue_icon_count_for, IconCatalogEntry, IconCatalogFamily,
    IconPickerChapter, IconPickerSearchResults, IconTarget,
};
use crate::keymap::{self, Action, KeyBinding, KeymapPreset, ShortcutBase, SHORTCUT_BASE_PRESETS};
use crate::layout::centered_rect;
use crate::theme::{self, ColorScheme};

/// Fixed header height: a title line (with the close button right-aligned
/// on it) plus a dim "Esc or q to close" hint line before the tab list /
/// content body begins.
const HEADER_HEIGHT: u16 = 2;
/// Width of the left tab list column.
const TAB_LIST_WIDTH: u16 = 26;
/// Blank columns between the tab list and the content panel -- this, not a
/// border character, is what separates them (see the module doc comment).
const GAP_WIDTH: u16 = 3;

/// One blank line above the first tab, then one line per tab plus one blank
/// spacer line after it -- see [`tab_at`], which must reproduce this exact
/// arithmetic to hit-test a click back to a tab.
const TAB_LIST_TOP_PADDING: u16 = 1;
const TAB_ROW_HEIGHT: u16 = 2;

/// One blank line above the first row, then one label/value line + one dim
/// description line + one blank spacer per row -- see
/// [`appearance_content_hit`], which must reproduce this exact arithmetic.
const APPEARANCE_TOP_PADDING: u16 = 1;
const APPEARANCE_ROW_HEIGHT: u16 = 3;

/// Column where a row's value control (`‹ value ›`) begins -- two leading
/// spaces plus the padded label column. Shared by rendering and
/// [`appearance_content_hit`].
const LABEL_COLUMN_WIDTH: u16 = 40;
const ROW_LEFT_INSET: u16 = 2;
/// Width of the `‹ ` hot zone that decrements/cycles-back a value; a click
/// anywhere else in the control increments/cycles-forward.
const DECREMENT_ZONE_WIDTH: u16 = 2;

/// The icon table uses fixed terminal-cell columns rather than Rust string
/// formatting widths. Emoji and bracketed selected choices have different
/// UTF-8 byte lengths, but must never move a later column or its hit target.
const ICON_TABLE_LABEL_WIDTH: usize = 18;
const ICON_TABLE_CURRENT_WIDTH: usize = 7;
const ICON_TABLE_CHOICE_WIDTH: usize = 4;
const ICON_TABLE_CHOICE_COUNT: usize = 4;
const ICON_TABLE_PICKER_LABEL: &str = "[+]";
const ICON_TABLE_LEFT_INSET: usize = 1;
const ICON_TABLE_COMPACT_GAP: usize = 1;
const ICON_TABLE_COMFORTABLE_GAP: usize = 2;
const ICON_TABLE_PICKER_COLUMN: usize = ICON_TABLE_LEFT_INSET
    + ICON_TABLE_LABEL_WIDTH
    + ICON_TABLE_COMPACT_GAP
    + ICON_TABLE_CURRENT_WIDTH
    + ICON_TABLE_COMPACT_GAP
    + (ICON_TABLE_CHOICE_WIDTH * ICON_TABLE_CHOICE_COUNT)
    + (ICON_TABLE_CHOICE_COUNT - 1)
    + ICON_TABLE_COMPACT_GAP;
/// Outer width needed to show the fixed-width table row, including its two
/// border cells. At this width the `[+]` button is visible and clickable.
const ICON_TABLE_MIN_OUTER_WIDTH: u16 =
    (ICON_TABLE_PICKER_COLUMN + ICON_TABLE_PICKER_LABEL.len() + 2) as u16;
const ICON_TABLE_COMFORTABLE_OUTER_WIDTH: u16 = ICON_TABLE_MIN_OUTER_WIDTH + 3;

/// Result of a click on one visible row in the icon assignment table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconTableAction {
    CycleSuggestion,
    OpenCatalogue,
}

/// A target plus the precise control clicked on its table row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconTableHit {
    pub target: IconTarget,
    pub action: IconTableAction,
}

/// The screen regions [`render`] draws into and mouse hit-testing reads
/// back out -- computed fresh from `area` every frame/event rather than
/// cached, since it's cheap pure arithmetic (mirrors `crate::layout::UiLayout`
/// in spirit, just for this one overlay instead of the whole screen).
pub struct SettingsLayout {
    pub header_area: Rect,
    pub tab_list_area: Rect,
    pub content_area: Rect,
}

/// Splits `area` into the header/tab-list/content regions. No vertical
/// border between the tab list and content -- [`GAP_WIDTH`] blank columns
/// instead, per the module doc comment's "aerated, not boxy" brief.
pub fn compute_layout(area: Rect) -> SettingsLayout {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(HEADER_HEIGHT), Constraint::Min(1)])
        .split(area);
    let header_area = rows[0];
    let body_area = rows[1];

    let tab_list_width = TAB_LIST_WIDTH.min(body_area.width);
    let gap_width = GAP_WIDTH.min(body_area.width.saturating_sub(tab_list_width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(tab_list_width),
            Constraint::Length(gap_width),
            Constraint::Min(1),
        ])
        .split(body_area);

    SettingsLayout {
        header_area,
        tab_list_area: columns[0],
        content_area: columns[2],
    }
}

/// Label for the clickable close control in the top-right corner of the
/// header -- see [`close_button_hit`] for its matching hit-test rect. Kept
/// short so it reads as a button, not a sentence.
const CLOSE_LABEL: &str = "\u{2715} Close";

/// Draws the full-screen settings view. Callers must skip drawing anything
/// else this frame (see `crate::ui::draw`'s `Mode::Settings` branch) -- this
/// replaces the entire screen, it isn't a popup layered on top. Two ways to
/// leave are always on screen so the view never feels like a trap: the
/// `CLOSE_LABEL` button at the top right (mouse) and the hint line under the
/// title (keyboard) -- both call back to `Mode::Normal`, see
/// `crate::keys::handle_settings_event`/`crate::mouse::handle_settings_mouse`.
pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    frame.render_widget(Clear, area);

    // The catalogue is a complete interaction mode, not a translucent
    // overlay. Rendering its frame before the dense Icons settings table
    // prevents previous wide-glyph cells from becoming terminal-diff debris
    // while the user scrolls or closes the picker.
    if let Some(picker) = &state.icon_picker {
        render_icon_picker(frame, area, picker);
        return;
    }
    if let Some(action) = state.keyboard_picker {
        render_keyboard_picker(frame, area, app, action);
        return;
    }

    // The Icons assignment table follows a picker full of wide emoji. Give
    // it its own near-black canvas so returning from either density mode
    // repaints every physical terminal cell before this compact table draws.
    if state.tab == SettingsTab::Icons {
        frame.render_widget(
            Block::default().style(Style::new().bg(Color::Rgb(2, 2, 2))),
            area,
        );
    }

    let layout = compute_layout(area);

    render_header(frame, layout.header_area);
    render_tab_list(frame, layout.tab_list_area, state.tab);
    if state.tab == SettingsTab::Icons {
        render_icons_tab(frame, layout.content_area, app, state);
        return;
    }
    match state.tab {
        SettingsTab::Inference => {
            let lines = inference_lines(
                &app.inference_settings,
                app.inference_test_result.as_ref(),
                &app.inference_test_state,
                &app.ollama_model_discovery,
                &app.ollama_models,
                state.selected_row,
            );
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::Appearance => {
            let lines = appearance_lines(&app.ui_settings, state.selected_row);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::Terminal => render_scrollable(
            frame,
            layout.content_area,
            terminal_lines(&app.terminal_settings, state.selected_row),
            state.scroll,
        ),
        SettingsTab::Editor => render_scrollable(
            frame,
            layout.content_area,
            editor_lines(&app.editor_settings, state.selected_row),
            state.scroll,
        ),
        SettingsTab::Session => render_scrollable(
            frame,
            layout.content_area,
            session_lines(&app.session_settings, state.selected_row),
            state.scroll,
        ),
        SettingsTab::Keyboard => {
            let lines = keyboard_lines(&app.keyboard_settings, &app.keybindings);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::KanbanBoard => {
            let lines = kanban_board_lines(&app.kanban_board_settings, state.selected_row);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::Sound => {
            let lines = sound_lines(
                &app.sound_settings,
                &app.sound_discovery,
                state.selected_row,
            );
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::VoiceControl => render_scrollable(
            frame,
            layout.content_area,
            voice_lines(app, state.selected_row),
            state.scroll,
        ),
        SettingsTab::About => {
            render_scrollable(frame, layout.content_area, about_lines(), state.scroll);
        }
        SettingsTab::Icons => unreachable!("Icons returns before the standard settings match"),
    }
}

/// Renders the title line (with the [`CLOSE_LABEL`] button right-aligned on
/// it) and a dim keyboard-hint line beneath it -- see [`close_button_hit`]
/// for the button's hit-test rect, which must track this layout.
fn render_header(frame: &mut Frame, area: Rect) {
    let title = Line::from(Span::styled(
        "\u{2699} Settings",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let close = Line::from(Span::styled(
        CLOSE_LABEL,
        theme::selected_style().add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Right);

    frame.render_widget(Paragraph::new(title), area);
    frame.render_widget(Paragraph::new(close), area);

    if area.height > 1 {
        let hint_area = Rect::new(area.x, area.y + 1, area.width, 1);
        let hint = Line::from(Span::styled(
            "Esc or q to close",
            Style::new().add_modifier(Modifier::DIM),
        ));
        frame.render_widget(Paragraph::new(hint), hint_area);
    }
}

/// The header row's `CLOSE_LABEL` button rect, right-aligned exactly as
/// [`render_header`] draws it -- a click anywhere in this rect closes
/// settings, same as `Esc`/`q`. See `crate::mouse::handle_settings_mouse`.
pub fn close_button_hit(header_area: Rect, position: Position) -> bool {
    let label_width = CLOSE_LABEL.chars().count() as u16;
    if header_area.height == 0 || label_width > header_area.width {
        return false;
    }
    let button_area = Rect::new(
        header_area.right() - label_width,
        header_area.y,
        label_width,
        1,
    );
    button_area.contains(position)
}

/// One blank top-padding line, then each tab's label with one blank spacer
/// line after it -- see [`tab_at`] for the matching hit-test arithmetic.
fn render_tab_list(frame: &mut Frame, area: Rect, active: SettingsTab) {
    let mut lines = Vec::with_capacity(1 + SettingsTab::ALL.len() * usize::from(TAB_ROW_HEIGHT));
    lines.push(Line::from(""));
    for tab in SettingsTab::ALL {
        let style = if tab == active {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", tab.label()),
            style,
        )));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The tab a click at `position` lands on, if any -- reproduces
/// [`render_tab_list`]'s exact line arithmetic rather than re-deriving it.
pub fn tab_at(tab_list_area: Rect, position: Position) -> Option<SettingsTab> {
    if !tab_list_area.contains(position) {
        return None;
    }
    let row = position.y - tab_list_area.y;
    if row < TAB_LIST_TOP_PADDING {
        return None;
    }
    let row = row - TAB_LIST_TOP_PADDING;
    if !row.is_multiple_of(TAB_ROW_HEIGHT) {
        return None;
    }
    let index = usize::from(row / TAB_ROW_HEIGHT);
    SettingsTab::ALL.get(index).copied()
}

/// Renders `lines` scrolled by `scroll` rows, adding a vertical scrollbar
/// only once the content is actually taller than `area` -- matches
/// `crate::ui`'s own `draw_terminal_scrollbar`/`draw_rendered_scrollbar`.
fn render_scrollable(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>, scroll: u16) {
    let total_lines = lines.len() as u16;
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);

    if total_lines > area.height {
        let mut scrollbar_state =
            ScrollbarState::new(usize::from(total_lines)).position(usize::from(scroll));
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .style(theme::border_style(false));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// The largest valid `scroll` value for `tab` at `content_area`'s current
/// height -- callers (`crate::keys`/`crate::mouse`) clamp scroll-wheel and
/// keyboard scrolling to this so the view can never scroll past its own end.
pub fn max_scroll(tab: SettingsTab, app: &App, selected_row: usize, content_area: Rect) -> u16 {
    let total_lines = match tab {
        SettingsTab::Inference => inference_lines(
            &app.inference_settings,
            app.inference_test_result.as_ref(),
            &app.inference_test_state,
            &app.ollama_model_discovery,
            &app.ollama_models,
            selected_row,
        )
        .len() as u16,
        SettingsTab::Appearance => appearance_lines(&app.ui_settings, selected_row).len() as u16,
        SettingsTab::Icons => 0,
        SettingsTab::Terminal => terminal_lines(&app.terminal_settings, selected_row).len() as u16,
        SettingsTab::Editor => editor_lines(&app.editor_settings, selected_row).len() as u16,
        SettingsTab::Session => session_lines(&app.session_settings, selected_row).len() as u16,
        SettingsTab::Keyboard => {
            keyboard_lines(&app.keyboard_settings, &app.keybindings).len() as u16
        }
        SettingsTab::KanbanBoard => {
            kanban_board_lines(&app.kanban_board_settings, selected_row).len() as u16
        }
        SettingsTab::Sound => {
            sound_lines(&app.sound_settings, &app.sound_discovery, selected_row).len() as u16
        }
        SettingsTab::VoiceControl => voice_lines(app, selected_row).len() as u16,
        SettingsTab::About => about_lines().len() as u16,
    };
    total_lines.saturating_sub(content_area.height)
}

/// Two-sided icon editor: a compact three-column selector table on the left,
/// and an always-live miniature sidebar on the right. The preview stays
/// intentionally independent from `tree_ui` so it can show every icon role
/// even when the live session happens not to contain that node/status.
fn render_icons_tab(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    let (left, right) = icon_tab_columns(area);
    let table_height = left.height.saturating_sub(2) as usize;
    let column_gap = icon_table_column_gap(left.width);
    let start = usize::from(state.scroll).min(IconTarget::ALL.len());
    let mut lines = vec![Line::from(Span::styled(
        format_icon_table_header(column_gap),
        Style::new().add_modifier(Modifier::BOLD),
    ))];
    for (index, target) in IconTarget::ALL
        .into_iter()
        .enumerate()
        .skip(start)
        .take(table_height.saturating_sub(1) / 2)
    {
        let selected = index == state.selected_row;
        let style = if selected {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let suggestions = target
            .suggestions()
            .iter()
            .map(|glyph| icon_choice_slot(glyph, *glyph == app.ui_settings.icons.glyph(target)))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(Line::from(Span::styled(
            format_icon_assignment_row(
                target.label(),
                app.ui_settings.icons.glyph(target),
                &suggestions,
                column_gap,
            ),
            style,
        )));
        lines.push(Line::from(""));
    }
    let table = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Icon assignments "),
    );
    frame.render_widget(table, left);

    let mode = if state.icons_preview_real {
        "[ Demo ]  [● Real tree]"
    } else {
        "[● Demo ]  [ Real tree ]"
    };
    let toolbar = vec![
        Line::from(Span::styled(
            mode,
            theme::selected_style().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            " click a mode; changes apply immediately",
            Style::new().add_modifier(Modifier::DIM),
        )),
    ];
    let toolbar_area = Rect::new(right.x, right.y, right.width, right.height.min(2));
    frame.render_widget(Paragraph::new(toolbar), toolbar_area);
    let preview_area = Rect::new(
        right.x,
        right.y.saturating_add(2),
        right.width,
        right.height.saturating_sub(2),
    );
    if state.icons_preview_real {
        let mut preview_tree_state = TreeState::default();
        crate::tree_ui::render(
            frame,
            preview_area,
            &app.tree,
            &mut preview_tree_state,
            crate::tree_ui::TreeRenderOptions {
                focused: false,
                elapsed_ms: app.started_at.elapsed().as_millis(),
                current_unix_millis: crate::scheduled_input::unix_millis_now(),
                project_name: app.project_name.as_deref(),
                is_project_name_loading: app.is_project_name_loading,
                titles_loading: &app.titles_loading,
                recently_created: &app.recently_created,
                transitions: &app.tree_transitions,
                agent_identifiers: &app.ui_settings.agent_identifiers,
                icons: &app.ui_settings.icons,
                tree_order: app.ui_settings.tree_order,
                sidebar_density: app.ui_settings.sidebar_density,
                use_stable_glyphs: app.ui_settings.use_stable_glyphs,
                hover: crate::tree_ui::TreeHoverState::default(),
                panes: &app.panes,
            },
        );
    } else {
        let mut preview = vec![Line::from("")];
        let icons = &app.ui_settings.icons;
        preview.extend([
            Line::from(format!("{} Workspace", icons.glyph(IconTarget::Group))),
            Line::from(format!(
                "  {} vertical build",
                icons.glyph(IconTarget::SplitVertical)
            )),
            Line::from(format!(
                "    {} {} {} Claude task",
                icons.glyph(IconTarget::Claude),
                icons.glyph(IconTarget::Working),
                icons.glyph(IconTarget::Goal)
            )),
            Line::from(format!(
                "    {} {} Codex review",
                icons.glyph(IconTarget::Codex),
                icons.glyph(IconTarget::WaitingApproval)
            )),
            Line::from(format!("  {} Notes", icons.glyph(IconTarget::Folder))),
            Line::from(format!("    {} README.md", icons.glyph(IconTarget::Editor))),
            Line::from(format!(
                "    {} Planning board",
                icons.glyph(IconTarget::Board)
            )),
            Line::from(format!(
                "  {} {} shell",
                icons.glyph(IconTarget::Terminal),
                icons.glyph(IconTarget::Idle)
            )),
            Line::from(format!(
                "  {} {} background task",
                icons.glyph(IconTarget::OtherAgent),
                icons.glyph(IconTarget::WaitingBackground)
            )),
            Line::from(format!(
                "  {} {} finished agent",
                icons.glyph(IconTarget::Antigravity),
                icons.glyph(IconTarget::Done)
            )),
        ]);
        frame.render_widget(
            Paragraph::new(preview).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Sidebar preview "),
            ),
            preview_area,
        );
    }
}

/// Uses the exact same horizontal split for rendering and icon-table
/// hit-testing, preventing a layout tweak from making pointer input drift.
fn icon_tab_columns(area: Rect) -> (Rect, Rect) {
    let percentage_width = area.width.saturating_mul(57) / 100;
    // Keep the catalogue control on screen at compact sizes. Without this
    // floor a 120-column terminal clipped `[+]`, making it impossible for
    // the pointer to reach the full picker even though the row was visible.
    let left_width = percentage_width
        .max(ICON_TABLE_MIN_OUTER_WIDTH)
        .min(area.width.saturating_sub(3));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_width),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(area);
    (columns[0], columns[2])
}

/// Pads a value to a terminal-cell width. `format!("{value:<width$}")` is
/// wrong for emoji because it measures Rust characters, not rendered cells.
fn pad_icon_text(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(UnicodeWidthStr::width(value));
    format!("{value}{}", " ".repeat(padding))
}

/// Renders one fixed-width quick-choice slot. Both ` 📁 ` and `[📁]` occupy
/// four terminal cells, so selecting an icon cannot shift the later choices
/// or the catalogue button.
fn icon_choice_slot(glyph: &str, is_selected: bool) -> String {
    let value = if is_selected {
        format!("[{glyph}]")
    } else {
        format!(" {glyph} ")
    };
    pad_icon_text(&value, ICON_TABLE_CHOICE_WIDTH)
}

/// Builds a row with stable display-cell columns for label, current glyph,
/// suggestions, and the catalogue action.
fn format_icon_table_header(column_gap: usize) -> String {
    let gap = " ".repeat(column_gap);
    format!(
        " {}{}{}{gap}Choices",
        pad_icon_text("Type", ICON_TABLE_LABEL_WIDTH),
        gap,
        pad_icon_text("Now", ICON_TABLE_CURRENT_WIDTH),
    )
}

fn format_icon_assignment_row(
    label: &str,
    current_glyph: &str,
    suggestions: &str,
    column_gap: usize,
) -> String {
    let gap = " ".repeat(column_gap);
    format!(
        " {}{}{}{}{}{}{}",
        pad_icon_text(label, ICON_TABLE_LABEL_WIDTH),
        gap,
        pad_icon_text(current_glyph, ICON_TABLE_CURRENT_WIDTH),
        gap,
        suggestions,
        gap,
        ICON_TABLE_PICKER_LABEL,
    )
}

fn icon_table_column_gap(table_outer_width: u16) -> usize {
    if table_outer_width >= ICON_TABLE_COMFORTABLE_OUTER_WIDTH {
        ICON_TABLE_COMFORTABLE_GAP
    } else {
        ICON_TABLE_COMPACT_GAP
    }
}

fn icon_table_picker_column(column_gap: usize) -> usize {
    ICON_TABLE_LEFT_INSET
        + ICON_TABLE_LABEL_WIDTH
        + column_gap
        + ICON_TABLE_CURRENT_WIDTH
        + column_gap
        + (ICON_TABLE_CHOICE_WIDTH * ICON_TABLE_CHOICE_COUNT)
        + (ICON_TABLE_CHOICE_COUNT - 1)
        + column_gap
}

/// The scalable catalogue overlay geometry. The body is a single scrollable
/// document rather than a category menu paired with a disconnected grid.
pub struct IconPickerLayout {
    pub popup: Rect,
    pub search_area: Rect,
    pub view_switch_area: Rect,
    pub document_area: Rect,
}

const ICON_PICKER_HEADER_HEIGHT: u16 = 4;
/// A three-column desktop catalogue remains dense without forcing the five
/// narrow cells that made emoji-width differences visually fragile.
const ICON_PICKER_MULTICOLUMN_CELL_WIDTH: u16 = 48;
const ICON_PICKER_MIN_MULTICOLUMNS: usize = 2;
const ICON_PICKER_MAX_MULTICOLUMNS: usize = 3;
const ICON_PICKER_VIEW_SWITCH_WIDTH: u16 = 39;

pub fn icon_picker_layout(area: Rect) -> IconPickerLayout {
    let popup = Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(1),
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    );
    let inner = popup.inner(ratatui::layout::Margin::new(1, 1));
    IconPickerLayout {
        popup,
        search_area: Rect::new(inner.x, inner.y, inner.width, ICON_PICKER_HEADER_HEIGHT),
        view_switch_area: Rect::new(
            inner
                .x
                .saturating_add(inner.width.saturating_sub(ICON_PICKER_VIEW_SWITCH_WIDTH)),
            inner.y,
            inner.width.min(ICON_PICKER_VIEW_SWITCH_WIDTH),
            1,
        ),
        document_area: Rect::new(
            inner.x,
            inner.y.saturating_add(ICON_PICKER_HEADER_HEIGHT),
            inner.width,
            inner.height.saturating_sub(ICON_PICKER_HEADER_HEIGHT),
        ),
    }
}

/// Result of a click inside [`icon_picker_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconPickerHit {
    Close,
    ToggleColumnMode,
    ActivateSearch,
    Entry(usize),
    ScrollTo(usize),
}

enum IconPickerDocumentRow<'a> {
    Chapter(&'a IconPickerChapter),
    Entries {
        chapter: &'a IconPickerChapter,
        chapter_offset: usize,
        entry_offset: usize,
    },
    Spacer,
}

pub fn icon_picker_document_row_count(results: &IconPickerSearchResults, columns: usize) -> usize {
    results
        .chapters
        .iter()
        .map(|chapter| 2 + chapter.entries.len().div_ceil(columns))
        .sum()
}

/// Resolves only one visible document row. This avoids allocating thousands
/// of row/index objects for every terminal paint while scrolling.
fn icon_picker_document_row_at(
    results: &IconPickerSearchResults,
    columns: usize,
    mut row_index: usize,
) -> Option<IconPickerDocumentRow<'_>> {
    let mut entry_offset = 0;
    for chapter in &results.chapters {
        if row_index == 0 {
            return Some(IconPickerDocumentRow::Chapter(chapter));
        }
        row_index -= 1;
        let entry_row_count = chapter.entries.len().div_ceil(columns);
        if row_index < entry_row_count {
            return Some(IconPickerDocumentRow::Entries {
                chapter,
                chapter_offset: row_index * columns,
                entry_offset,
            });
        }
        row_index -= entry_row_count;
        if row_index == 0 {
            return Some(IconPickerDocumentRow::Spacer);
        }
        row_index -= 1;
        entry_offset += chapter.entries.len();
    }
    None
}

fn picker_document_columns(area: Rect, column_mode: IconPickerColumnMode) -> usize {
    match column_mode {
        IconPickerColumnMode::SingleColumn => 1,
        IconPickerColumnMode::MultiColumn => usize::from(area.width)
            .saturating_div(usize::from(ICON_PICKER_MULTICOLUMN_CELL_WIDTH))
            .clamp(ICON_PICKER_MIN_MULTICOLUMNS, ICON_PICKER_MAX_MULTICOLUMNS),
    }
}

/// Returns the current keyboard stride from the same responsive document
/// geometry the renderer and mouse hit tester use.
pub fn icon_picker_grid_columns(screen_area: Rect, column_mode: IconPickerColumnMode) -> usize {
    let layout = icon_picker_layout(screen_area);
    picker_document_columns(layout.document_area, column_mode)
}

fn truncate_icon_text(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut occupied_width = 0;
    for grapheme in UnicodeSegmentation::graphemes(value, true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if occupied_width + grapheme_width > width {
            break;
        }
        result.push_str(grapheme);
        occupied_width += grapheme_width;
    }
    result
}

fn icon_picker_cell(entry: &IconCatalogEntry, is_selected: bool, width: usize) -> String {
    let marker = if is_selected { "[" } else { " " };
    let suffix = if is_selected { "]" } else { " " };
    truncate_icon_text(
        &format!("{marker}{}{suffix} {}", entry.glyph, entry.name),
        width,
    )
}

/// Gives each density mode a distinct near-black canvas style. Switching the
/// style forces the terminal diff to rewrite otherwise-blank cells that can
/// be physical continuations of a previously rendered wide emoji.
fn icon_picker_canvas_style(column_mode: IconPickerColumnMode) -> Style {
    match column_mode {
        IconPickerColumnMode::MultiColumn => Style::new().bg(Color::Black),
        IconPickerColumnMode::SingleColumn => Style::new().bg(Color::Rgb(1, 1, 1)),
    }
}

pub fn icon_picker_row_for_entry(
    results: &IconPickerSearchResults,
    columns: usize,
    selected_entry: usize,
) -> usize {
    let mut entry_offset = 0;
    let mut row_offset = 0;
    for chapter in &results.chapters {
        if selected_entry < entry_offset + chapter.entries.len() {
            return row_offset + 1 + (selected_entry - entry_offset) / columns;
        }
        entry_offset += chapter.entries.len();
        row_offset += 2 + chapter.entries.len().div_ceil(columns);
    }
    0
}

pub fn icon_picker_scroll_for_entry(
    screen_area: Rect,
    picker: &crate::app::IconPickerState,
) -> usize {
    let layout = icon_picker_layout(screen_area);
    let columns = picker_document_columns(layout.document_area, picker.column_mode);
    let visible_rows = usize::from(layout.document_area.height).max(1);
    let total_rows = icon_picker_document_row_count(&picker.search_results, columns);
    let selected_row =
        icon_picker_row_for_entry(&picker.search_results, columns, picker.selected_entry);
    let max_scroll = total_rows.saturating_sub(visible_rows);
    if selected_row < picker.scroll_row {
        selected_row
    } else if selected_row >= picker.scroll_row + visible_rows {
        selected_row
            .saturating_sub(visible_rows.saturating_sub(1))
            .min(max_scroll)
    } else {
        picker.scroll_row.min(max_scroll)
    }
}

fn render_icon_picker(frame: &mut Frame, area: Rect, picker: &crate::app::IconPickerState) {
    let layout = icon_picker_layout(area);
    let entry_count = picker.search_results.entry_count;
    let selected_entry = picker.selected_entry.min(entry_count.saturating_sub(1));
    let columns = picker_document_columns(layout.document_area, picker.column_mode);
    let visible_rows = usize::from(layout.document_area.height).max(1);
    let total_rows = icon_picker_document_row_count(&picker.search_results, columns);
    let start = picker
        .scroll_row
        .min(total_rows.saturating_sub(visible_rows));
    // Paint a concrete dark canvas, rather than merely resetting cells. This
    // forces crossterm to overwrite every cell that belonged to the dense
    // settings table in the previous frame, including a terminal's stale
    // continuation cell after an emoji whose width differs from Unicode's
    // table. The catalogue deliberately owns the entire screen while open.
    frame.render_widget(
        Block::default().style(icon_picker_canvas_style(picker.column_mode)),
        area,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "Choose an icon for {} · {} official UTF-8 · {} Nerd Font · {} total",
                    picker.target.label(),
                    catalogue_icon_count_for(IconCatalogFamily::OfficialUtf8),
                    catalogue_icon_count_for(IconCatalogFamily::NerdFont),
                    catalogue_icon_count(),
                ),
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Official UTF-8 chapters first · Nerd Font chapters afterwards · / runs local CPU semantic search",
                Style::new().add_modifier(Modifier::DIM),
            )),
            Line::from(if picker.is_searching {
                format!("Search: {}_", picker.search_query)
            } else if picker.search_query.is_empty() {
                "Search: / to find by meaning with local CPU embeddings".to_string()
            } else {
                format!("Search: {}", picker.search_query)
            }),
            Line::from(Span::styled(
                format!(
                    "{} · {} chapter{} · {} icon{} · arrows move · V switches view · Enter chooses · Esc closes",
                    match &picker.search_status {
                        IconPickerSearchStatus::Browse if picker.search_query.is_empty() => "Browse catalogue".to_string(),
                        IconPickerSearchStatus::Browse => "CPU semantic results".to_string(),
                        IconPickerSearchStatus::Searching => "Loading CPU model and vector index…".to_string(),
                        IconPickerSearchStatus::Failed(message) => format!("Semantic search unavailable: {message}"),
                    },
                    picker.search_results.chapters.len(),
                    if picker.search_results.chapters.len() == 1 { "" } else { "s" },
                    entry_count,
                    if entry_count == 1 { "" } else { "s" },
                ),
                Style::new().add_modifier(Modifier::DIM),
            )),
        ]),
        layout.search_area,
    );
    let view_switch = match picker.column_mode {
        IconPickerColumnMode::MultiColumn => "[● Multi-column]  [ Single column ]",
        IconPickerColumnMode::SingleColumn => "[ Multi-column ]  [● Single column]",
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            view_switch,
            theme::selected_style().add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Right),
        layout.view_switch_area,
    );

    let cell_width = usize::from(layout.document_area.width)
        .div_ceil(columns)
        .max(1);
    for (visible_offset, row_index) in (start..(start + visible_rows).min(total_rows)).enumerate() {
        let row_area = Rect::new(
            layout.document_area.x,
            layout.document_area.y.saturating_add(visible_offset as u16),
            layout.document_area.width,
            1,
        );
        match icon_picker_document_row_at(&picker.search_results, columns, row_index) {
            Some(IconPickerDocumentRow::Chapter(chapter)) => frame.render_widget(
                Paragraph::new(Span::styled(
                    truncate_icon_text(
                        &format!(
                            "── {} · {} ({}) ──",
                            chapter.family.label(),
                            chapter.label,
                            chapter.entries.len()
                        ),
                        usize::from(row_area.width),
                    ),
                    Style::new().add_modifier(Modifier::BOLD),
                )),
                row_area,
            ),
            Some(IconPickerDocumentRow::Entries {
                chapter,
                chapter_offset,
                entry_offset,
            }) => {
                for (column, entry) in chapter.entries
                    [chapter_offset..(chapter_offset + columns).min(chapter.entries.len())]
                    .iter()
                    .enumerate()
                {
                    let cell_x = row_area.x.saturating_add((column * cell_width) as u16);
                    let remaining_width = row_area.right().saturating_sub(cell_x);
                    let cell_area = Rect::new(
                        cell_x,
                        row_area.y,
                        remaining_width.min(cell_width as u16),
                        1,
                    );
                    frame.render_widget(
                        Paragraph::new(icon_picker_cell(
                            entry,
                            entry_offset + chapter_offset + column == selected_entry,
                            usize::from(cell_area.width),
                        )),
                        cell_area,
                    );
                }
            }
            Some(IconPickerDocumentRow::Spacer) | None => {}
        }
    }
    if total_rows > visible_rows {
        let mut scrollbar_state = ScrollbarState::new(total_rows)
            .viewport_content_length(visible_rows)
            .position(start);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            layout.document_area,
            &mut scrollbar_state,
        );
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Icon catalogue "),
        layout.popup,
    );
}

/// Resolves mouse coordinates using the exact single-document geometry the
/// overlay renders. `Entry` is a filtered document ordinal.
pub fn icon_picker_hit(
    area: Rect,
    picker: &crate::app::IconPickerState,
    position: Position,
) -> Option<IconPickerHit> {
    let layout = icon_picker_layout(area);
    if !layout.popup.contains(position) {
        return Some(IconPickerHit::Close);
    }
    if position
        == Position::new(
            layout.popup.x + layout.popup.width.saturating_sub(1),
            layout.popup.y,
        )
    {
        return Some(IconPickerHit::Close);
    }
    if layout.view_switch_area.contains(position) {
        return Some(IconPickerHit::ToggleColumnMode);
    }
    if layout.search_area.contains(position) {
        return Some(IconPickerHit::ActivateSearch);
    }
    if layout.document_area.contains(position) {
        let columns = picker_document_columns(layout.document_area, picker.column_mode);
        let visible_rows = usize::from(layout.document_area.height).max(1);
        let total_rows = icon_picker_document_row_count(&picker.search_results, columns);
        let start = picker
            .scroll_row
            .min(total_rows.saturating_sub(visible_rows));
        if position.x == layout.document_area.right().saturating_sub(1) && total_rows > visible_rows
        {
            let track_row = usize::from(position.y.saturating_sub(layout.document_area.y));
            let max_scroll = total_rows.saturating_sub(visible_rows);
            let target =
                max_scroll.saturating_mul(track_row) / visible_rows.saturating_sub(1).max(1);
            return Some(IconPickerHit::ScrollTo(target));
        }
        let row = start + usize::from(position.y.saturating_sub(layout.document_area.y));
        let Some(IconPickerDocumentRow::Entries {
            chapter,
            chapter_offset,
            entry_offset,
        }) = icon_picker_document_row_at(&picker.search_results, columns, row)
        else {
            return None;
        };
        let cell_width = layout.document_area.width.div_ceil(columns as u16).max(1);
        let column = usize::from(position.x.saturating_sub(layout.document_area.x) / cell_width)
            .min(columns.saturating_sub(1));
        let entry_index = entry_offset + chapter_offset + column;
        return (entry_index < entry_offset + chapter.entries.len())
            .then_some(IconPickerHit::Entry(entry_index));
    }
    None
}

pub fn icons_table_hit(area: Rect, scroll: u16, position: Position) -> Option<IconTableHit> {
    let (table, _) = icon_tab_columns(area);
    let table_inner = table.inner(ratatui::layout::Margin::new(1, 1));
    if !table_inner.contains(position) || position.y <= table_inner.y {
        return None;
    }

    let row_offset = usize::from(position.y - table_inner.y - 1);
    if row_offset % 2 != 0 {
        return None;
    }
    let target_index = row_offset / 2 + usize::from(scroll);
    let target = IconTarget::ALL.get(target_index).copied()?;
    let picker_start = table_inner
        .x
        .saturating_add(icon_table_picker_column(icon_table_column_gap(table.width)) as u16);
    let picker_end = picker_start.saturating_add(ICON_TABLE_PICKER_LABEL.width() as u16);
    let action = if position.x >= picker_start && position.x < picker_end {
        IconTableAction::OpenCatalogue
    } else {
        IconTableAction::CycleSuggestion
    };
    Some(IconTableHit { target, action })
}

pub fn icons_preview_mode_hit(area: Rect, position: Position) -> Option<bool> {
    let (_, right) = icon_tab_columns(area);
    let right_start = right.x;
    (position.y == area.y && position.x >= right_start).then_some(position.x >= right_start + 11)
}

/// Visible settings depend on the selected provider. This keeps Kilo's
/// no-key configuration honest and prevents irrelevant empty fields.
pub fn inference_rows(settings: &ilium_inference::InferenceSettings) -> Vec<InferenceRow> {
    use ilium_inference::InferenceProviderKind;
    let mut rows = vec![InferenceRow::Provider];
    match settings.selected_provider {
        InferenceProviderKind::KiloGateway => {}
        InferenceProviderKind::Ollama => rows.extend([
            InferenceRow::Field(InferenceSettingField::OllamaUrl),
            InferenceRow::RefreshOllamaModels,
            InferenceRow::Field(InferenceSettingField::OllamaModel),
        ]),
        InferenceProviderKind::OpenAi => rows.extend([
            InferenceRow::Field(InferenceSettingField::OpenAiUrl),
            InferenceRow::Field(InferenceSettingField::OpenAiApiKey),
            InferenceRow::Field(InferenceSettingField::OpenAiModel),
        ]),
        InferenceProviderKind::Anthropic => rows.extend([
            InferenceRow::Field(InferenceSettingField::AnthropicUrl),
            InferenceRow::Field(InferenceSettingField::AnthropicApiKey),
            InferenceRow::Field(InferenceSettingField::AnthropicModel),
        ]),
        InferenceProviderKind::OpenRouter => rows.extend([
            InferenceRow::Field(InferenceSettingField::OpenRouterApiKey),
            InferenceRow::Field(InferenceSettingField::OpenRouterModel),
        ]),
    }
    rows.push(InferenceRow::Test);
    rows
}

fn inference_value(
    row: InferenceRow,
    settings: &ilium_inference::InferenceSettings,
    ollama_model_discovery: &OllamaModelDiscoveryState,
) -> String {
    match row {
        InferenceRow::Provider => format!("‹ {} ›", settings.selected_provider.label()),
        InferenceRow::RefreshOllamaModels => match ollama_model_discovery {
            OllamaModelDiscoveryState::Loading { started_at, .. } => {
                let frame_index = (started_at.elapsed().as_millis()
                    / crate::tree_ui::SPINNER_FRAME_MS) as usize
                    % crate::tree_ui::SPINNER_FRAMES.len();
                format!(
                    "[ {} Discovering models… ]",
                    crate::tree_ui::SPINNER_FRAMES[frame_index]
                )
            }
            OllamaModelDiscoveryState::Failed { .. } => "[ Retry model discovery ]".to_string(),
            OllamaModelDiscoveryState::Idle | OllamaModelDiscoveryState::Loaded { .. } => {
                "[ Load models from Ollama ]".to_string()
            }
        },
        InferenceRow::Test => "[ Test provider ]".to_string(),
        InferenceRow::Field(field) => match field {
            InferenceSettingField::OllamaUrl => settings.ollama.base_url.clone(),
            InferenceSettingField::OllamaModel => settings.ollama.model.clone(),
            InferenceSettingField::OpenAiUrl => settings.openai.base_url.clone(),
            InferenceSettingField::OpenAiApiKey => masked_key(&settings.openai.api_key),
            InferenceSettingField::OpenAiModel => settings.openai.model.clone(),
            InferenceSettingField::AnthropicUrl => settings.anthropic.base_url.clone(),
            InferenceSettingField::AnthropicApiKey => masked_key(&settings.anthropic.api_key),
            InferenceSettingField::AnthropicModel => settings.anthropic.model.clone(),
            InferenceSettingField::OpenRouterApiKey => masked_key(&settings.openrouter.api_key),
            InferenceSettingField::OpenRouterModel => settings.openrouter.model.clone(),
        },
    }
}

fn masked_key(key: &str) -> String {
    if key.is_empty() {
        "Not set (Enter to edit)".to_string()
    } else {
        // API keys are always ASCII; safe to use byte slicing to get last 4 chars
        let start = key.len().saturating_sub(4);
        format!("••••{}", &key[start..])
    }
}
fn inference_label(row: InferenceRow) -> &'static str {
    match row {
        InferenceRow::Provider => "Provider",
        InferenceRow::Field(field) => field.label(),
        InferenceRow::RefreshOllamaModels => "Available models",
        InferenceRow::Test => "Test",
    }
}
fn inference_lines(
    settings: &ilium_inference::InferenceSettings,
    test_result: Option<&crate::inference_test::InferenceTestResult>,
    test_state: &InferenceTestState,
    ollama_model_discovery: &OllamaModelDiscoveryState,
    ollama_models: &[String],
    selected_row: usize,
) -> Vec<Line<'static>> {
    let rows = inference_rows(settings);
    let mut lines = vec![Line::from("")];
    if settings.selected_provider == ilium_inference::InferenceProviderKind::KiloGateway {
        lines.push(Line::from(Span::styled("  Kilo Gateway is useful to try things out. Do not send secrets: requests may be used for LLM training.", Style::new().fg(Color::Yellow))));
        lines.push(Line::from(""));
    }
    for (index, row) in rows.into_iter().enumerate() {
        let label = inference_label(row);
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let value_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(
                inference_value(row, settings, ollama_model_discovery),
                value_style,
            ),
        ]));
        let description = match row {
            InferenceRow::Provider => {
                "Choose the backend used by background title and organization inference."
            }
            InferenceRow::Field(InferenceSettingField::OllamaModel) => {
                "Use left/right to select a loaded local model, or Enter to type a model name."
            }
            InferenceRow::Field(_) => {
                "Press Enter to edit this value. API keys remain masked in this view."
            }
            InferenceRow::RefreshOllamaModels => {
                "Fetch the selectable model list from this Ollama API URL."
            }
            InferenceRow::Test => {
                "Send a harmless title-and-organize prompt and preview the returned tree structure."
            }
        };
        lines.push(Line::from(Span::styled(
            format!("    {description}"),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    if settings.selected_provider == ilium_inference::InferenceProviderKind::Ollama {
        lines.extend(ollama_discovery_lines(
            settings,
            ollama_model_discovery,
            ollama_models,
        ));
    }
    lines.extend(inference_test_log_lines(test_state));
    if let Some(result) = test_result {
        lines.push(Line::from(Span::styled(
            format!(
                "  Test result · processed in {:.2}s",
                result.elapsed.as_secs_f64()
            ),
            theme::selected_style().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Preview (as it will appear in the left pane)",
            Style::new().add_modifier(Modifier::DIM),
        )));
        for group in &result.groups {
            lines.push(Line::from(Span::styled(
                format!("  ▾ {}", group.name),
                Style::new().add_modifier(Modifier::BOLD),
            )));
            for (index, pane) in group.panes.iter().enumerate() {
                let branch = if index + 1 == group.panes.len() {
                    "└─"
                } else {
                    "├─"
                };
                lines.push(Line::from(format!("     {branch} ▣ {pane}")));
            }
        }
    }
    lines
}

/// Renders a durable boxed operation trace within Settings. The full-screen
/// settings view hides the normal status bar, so explicit remote work must
/// remain visible here from launch through success or failure.
fn boxed_operation_log(
    title: &str,
    headline: String,
    headline_style: Style,
    log_lines: Vec<(String, Style)>,
) -> Vec<Line<'static>> {
    const HORIZONTAL_RULE: &str = "────────────────────────────────────────────────";
    let mut lines = vec![Line::from(Span::styled(
        format!("  ┌─ {title} {HORIZONTAL_RULE}┐"),
        Style::new().fg(Color::DarkGray),
    ))];
    lines.push(Line::from(vec![
        Span::styled("  │ ", Style::new().fg(Color::DarkGray)),
        Span::styled(headline, headline_style),
    ]));
    for (line, style) in log_lines {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::new().fg(Color::DarkGray)),
            Span::styled(line, style),
        ]));
    }
    lines.push(Line::from(Span::styled(
        format!("  └{HORIZONTAL_RULE}┘"),
        Style::new().fg(Color::DarkGray),
    )));
    lines
}

/// Renders test execution as an explicit operation log instead of relying on
/// an invisible worker or a transient status-bar message.
fn inference_test_log_lines(test_state: &InferenceTestState) -> Vec<Line<'static>> {
    match test_state {
        InferenceTestState::Idle => Vec::new(),
        InferenceTestState::Running {
            provider,
            started_at,
        } => {
            let frame_index = (started_at.elapsed().as_millis() / crate::tree_ui::SPINNER_FRAME_MS)
                as usize
                % crate::tree_ui::SPINNER_FRAMES.len();
            boxed_operation_log(
                "Provider test log",
                format!(
                    "{} Testing {} · {:.1}s elapsed",
                    crate::tree_ui::SPINNER_FRAMES[frame_index],
                    provider.label(),
                    started_at.elapsed().as_secs_f64()
                ),
                theme::selected_style().add_modifier(Modifier::BOLD),
                vec![
                    (
                        "1. Build a harmless title-and-organize request.".to_string(),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        format!("2. Send it to {}.", provider.label()),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        "3. Wait for and validate the required JSON response.".to_string(),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
            )
        }
        InferenceTestState::Succeeded { provider, elapsed } => boxed_operation_log(
            "Provider test log",
            format!(
                "✓ {} replied in {:.2}s",
                provider.label(),
                elapsed.as_secs_f64()
            ),
            theme::selected_style().add_modifier(Modifier::BOLD),
            vec![
                (
                    "1. Provider request completed successfully.".to_string(),
                    Style::new(),
                ),
                (
                    "2. JSON response validated; preview is shown below.".to_string(),
                    Style::new(),
                ),
            ],
        ),
        InferenceTestState::Failed {
            provider,
            error,
            elapsed,
        } => boxed_operation_log(
            "Provider test log",
            format!(
                "✗ {} test failed after {:.2}s",
                provider.label(),
                elapsed.as_secs_f64()
            ),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            vec![
                (
                    format!("Provider error: {error}"),
                    Style::new().fg(Color::Red),
                ),
                (
                    "No settings were changed. Correct the endpoint/model and retry.".to_string(),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            ],
        ),
    }
}

/// Renders the actual model-discovery trace inside Settings itself. The
/// normal status bar is intentionally not visible while Settings replaces the
/// whole screen, so an async operation must report its progress and outcome
/// here rather than only through `App::status_message`.
fn ollama_discovery_lines(
    settings: &ilium_inference::InferenceSettings,
    discovery: &OllamaModelDiscoveryState,
    models: &[String],
) -> Vec<Line<'static>> {
    let endpoint = settings.ollama.base_url.trim_end_matches('/');
    let tags_url = format!("{endpoint}/api/tags");
    let mut lines = vec![Line::from("")];
    match discovery {
        OllamaModelDiscoveryState::Idle => {
            lines.push(Line::from(Span::styled(
                "  Model discovery has not run in this client session.",
                Style::new().add_modifier(Modifier::DIM),
            )));
            lines.push(Line::from(Span::styled(
                format!("  Load models calls GET {tags_url}, then extracts each model name."),
                Style::new().add_modifier(Modifier::DIM),
            )));
        }
        OllamaModelDiscoveryState::Loading {
            endpoint,
            started_at,
        } => {
            let frame_index = (started_at.elapsed().as_millis() / crate::tree_ui::SPINNER_FRAME_MS)
                as usize
                % crate::tree_ui::SPINNER_FRAMES.len();
            let elapsed = started_at.elapsed().as_secs_f64();
            lines.extend(boxed_operation_log(
                "Ollama model discovery log",
                format!(
                    "{} Discovering local models from {endpoint} · {elapsed:.1}s elapsed",
                    crate::tree_ui::SPINNER_FRAMES[frame_index],
                ),
                theme::selected_style().add_modifier(Modifier::BOLD),
                vec![
                    (
                        format!("1. Connect to the configured Ollama API at {endpoint}."),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        format!("2. Request GET {endpoint}/api/tags."),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    (
                        "3. Validate the JSON response and extract model names.".to_string(),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
            ));
        }
        OllamaModelDiscoveryState::Loaded {
            endpoint,
            model_count,
            elapsed,
        } => {
            lines.extend(boxed_operation_log(
                "Ollama model discovery log",
                format!("✓ GET /api/tags completed in {:.2}s", elapsed.as_secs_f64()),
                theme::selected_style().add_modifier(Modifier::BOLD),
                vec![
                    (format!("1. Connected to {endpoint}."), Style::new()),
                    (
                        format!("2. Extracted {model_count} selectable model name(s)."),
                        Style::new(),
                    ),
                    (
                        "3. Updated the selected model when needed.".to_string(),
                        Style::new(),
                    ),
                ],
            ));
        }
        OllamaModelDiscoveryState::Failed {
            endpoint,
            error,
            elapsed,
        } => {
            lines.extend(boxed_operation_log(
                "Ollama model discovery log",
                format!(
                    "✗ Model discovery failed after {:.2}s",
                    elapsed.as_secs_f64()
                ),
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                vec![
                    (format!("Endpoint: {endpoint}"), Style::new().fg(Color::Red)),
                    (
                        format!("Ollama reported: {error}"),
                        Style::new().fg(Color::Red),
                    ),
                    (
                        format!("Retry calls GET {tags_url} after Ollama is available."),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                ],
            ));
        }
    }
    if !models.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Models returned by Ollama (use left/right on Ollama model to select)",
            Style::new().add_modifier(Modifier::BOLD),
        )));
        for model in models {
            let marker = if model == &settings.ollama.model {
                "[x]"
            } else {
                "[ ]"
            };
            let style = if marker == "[x]" {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!("    {marker} {model}"),
                style,
            )));
        }
    }
    lines
}

pub fn inference_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    settings: &ilium_inference::InferenceSettings,
) -> Option<(InferenceRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }
    let warning_lines =
        if settings.selected_provider == ilium_inference::InferenceProviderKind::KiloGateway {
            2
        } else {
            0
        };
    let line = usize::from(position.y.saturating_sub(content_area.y)) + usize::from(scroll);
    let offset = line.checked_sub(usize::from(APPEARANCE_TOP_PADDING) + warning_lines)?;
    if offset % usize::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row = *inference_rows(settings).get(offset / usize::from(APPEARANCE_ROW_HEIGHT))?;
    Some((
        row,
        if position.x < content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH + DECREMENT_ZONE_WIDTH
        {
            -1
        } else {
            1
        },
    ))
}

fn kanban_board_row_label(row: KanbanBoardRow) -> &'static str {
    match row {
        KanbanBoardRow::CardPreviewLines => "Card preview lines",
        KanbanBoardRow::MinimumColumnWidth => "Minimum column width",
    }
}

fn kanban_board_row_description(row: KanbanBoardRow) -> &'static str {
    match row {
        KanbanBoardRow::CardPreviewLines => {
            "Number of wrapped text lines shown on each card, from 1 to 10."
        }
        KanbanBoardRow::MinimumColumnWidth => {
            "Minimum terminal columns per board column before horizontal scrolling is used."
        }
    }
}

fn kanban_board_row_value(row: KanbanBoardRow, settings: &KanbanBoardSettings) -> String {
    match row {
        KanbanBoardRow::CardPreviewLines => format!("‹ {} ›", settings.card_preview_lines),
        KanbanBoardRow::MinimumColumnWidth => {
            format!("‹ {} ›", settings.minimum_column_width)
        }
    }
}

/// Kanban Board tab content. Both controls are client-global presentation
/// settings, so board-owned Markdown/folder data remains purely domain data.
fn kanban_board_lines(settings: &KanbanBoardSettings, selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for (index, row) in KanbanBoardRow::ALL.into_iter().enumerate() {
        let label = kanban_board_row_label(row);
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let value_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(kanban_board_row_value(row, settings), value_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {}", kanban_board_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "Click a card to open and edit it in the board's right-hand panel.",
        Style::new().add_modifier(Modifier::DIM),
    )));
    lines
}

/// Returns the row plus decrement/increment direction for either Kanban
/// Board stepper using the same three-line row geometry as rendering.
pub fn kanban_board_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<(KanbanBoardRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index =
        usize::try_from(i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll))
            .ok()?;
    let row_offset = line_index.checked_sub(usize::from(APPEARANCE_TOP_PADDING))?;
    if row_offset % usize::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row = *KanbanBoardRow::ALL.get(row_offset / usize::from(APPEARANCE_ROW_HEIGHT))?;
    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    };
    Some((row, direction))
}

fn sound_row_label(row: SoundRow) -> &'static str {
    match row {
        SoundRow::Source => "Sound source",
        SoundRow::File => "Selected sound file",
        SoundRow::Preview => "Preview",
        SoundRow::AgentFinished => "Agent finished",
        SoundRow::ApprovalRequired => "Agent needs approval",
        SoundRow::AgentStarted => "Agent started working",
        SoundRow::WaitingBackground => "Agent waits for background work",
    }
}

fn sound_row_description(row: SoundRow) -> &'static str {
    match row {
        SoundRow::Source => "Use the operating system beep or a discovered sound file.",
        SoundRow::File => "Left/Right cycles through files found in the folders listed below.",
        SoundRow::Preview => "Play the current choice once through the detached server.",
        SoundRow::AgentFinished => "A busy agent completed its turn and is waiting for you.",
        SoundRow::ApprovalRequired => "An agent entered a confirmation or approval prompt.",
        SoundRow::AgentStarted => "An idle or finished agent began a new turn.",
        SoundRow::WaitingBackground => "An agent is waiting for background workers it started.",
    }
}

fn selected_sound_label(
    settings: &ilium_sound::SoundSettings,
    discovery: &ilium_sound::SoundDiscovery,
) -> String {
    let Some(path) = settings.file.as_ref() else {
        return if discovery.sounds.is_empty() {
            "No sound files found".to_string()
        } else {
            "Not selected".to_string()
        };
    };
    discovery
        .sounds
        .iter()
        .find(|sound| &sound.path == path)
        .map(|sound| format!("{} — {}", sound.collection, sound.display_name))
        .unwrap_or_else(|| format!("{} (unavailable)", path.display()))
}

fn sound_row_value(
    row: SoundRow,
    settings: &ilium_sound::SoundSettings,
    discovery: &ilium_sound::SoundDiscovery,
) -> String {
    match row {
        SoundRow::Source => format!("‹ {} ›", settings.source.label()),
        SoundRow::File => format!("‹ {} ›", selected_sound_label(settings, discovery)),
        SoundRow::Preview => "[ Play ]".to_string(),
        event_row => event_row
            .event()
            .map(|event| {
                if settings.events.is_enabled(event) {
                    "[x] Enabled".to_string()
                } else {
                    "[ ] Disabled".to_string()
                }
            })
            .unwrap_or_default(),
    }
}

/// Sound-tab content: fixed controls first, then the exact existing folders
/// and files discovered for this operating system. The catalog both proves
/// where options came from and lets mouse users select a file directly.
fn sound_lines(
    settings: &ilium_sound::SoundSettings,
    discovery: &ilium_sound::SoundDiscovery,
    selected_row: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for (index, row) in SoundRow::ALL.into_iter().enumerate() {
        let label = sound_row_label(row);
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let value_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(label, label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(sound_row_value(row, settings, discovery), value_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {}", sound_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "Discovered system sound folders",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    let truncation = if discovery.was_truncated {
        " (scan limit reached)"
    } else {
        ""
    };
    lines.push(Line::from(format!(
        "  {}: {} playable files across {} existing folders{truncation}",
        discovery.platform,
        discovery.sounds.len(),
        discovery.directories.len()
    )));
    for directory in &discovery.directories {
        lines.push(Line::from(format!(
            "  • {} — {}",
            directory.origin,
            directory.path.display()
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Available system sounds (click one, or use the selector above)",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    if discovery.sounds.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No supported sound files were found in the common folders on this system.",
            Style::new().add_modifier(Modifier::DIM),
        )));
    } else {
        for sound in &discovery.sounds {
            let is_selected = settings.file.as_ref() == Some(&sound.path);
            let marker = if is_selected { "[x]" } else { "[ ]" };
            let style = if is_selected {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!("  {marker} {} — {}", sound.collection, sound.display_name),
                style,
            )));
        }
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundContentHit {
    Row { row: SoundRow, direction: i32 },
    File(usize),
}

/// Hit-tests fixed controls and dynamic discovered-file rows against the same
/// line arithmetic used by [`sound_lines`].
pub fn sound_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    discovery: &ilium_sound::SoundDiscovery,
) -> Option<SoundContentHit> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index =
        usize::try_from(i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll))
            .ok()?;
    if line_index >= usize::from(APPEARANCE_TOP_PADDING) {
        let row_offset = line_index - usize::from(APPEARANCE_TOP_PADDING);
        if row_offset % usize::from(APPEARANCE_ROW_HEIGHT) == 0 {
            let row_index = row_offset / usize::from(APPEARANCE_ROW_HEIGHT);
            if let Some(row) = SoundRow::ALL.get(row_index).copied() {
                let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
                if position.x >= control_start_x {
                    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
                        -1
                    } else {
                        1
                    };
                    return Some(SoundContentHit::Row { row, direction });
                }
            }
        }
    }

    let first_sound_line = usize::from(APPEARANCE_TOP_PADDING)
        + SoundRow::ALL.len() * usize::from(APPEARANCE_ROW_HEIGHT)
        + 4
        + discovery.directories.len();
    let sound_index = line_index.checked_sub(first_sound_line)?;
    (sound_index < discovery.sounds.len()).then_some(SoundContentHit::File(sound_index))
}

const KEYBOARD_ACTION_COLUMN_WIDTH: usize = 16;
const KEYBOARD_KEY_CAP_WIDTH: usize = 5;
const KEYBOARD_MNEMONIC_COLUMN_WIDTH: usize = 10;
const KEYBOARD_SUGGESTION_START_OFFSET: u16 = 37;
const KEYBOARD_PICKER_START_OFFSET: u16 = KEYBOARD_SUGGESTION_START_OFFSET + 12;

/// Keyboard-tab content: one live selector, explicit A/B presets, and the
/// specific recommendation or warning for the currently selected letter.
fn keyboard_lines(keyboard: &KeyboardSettings, bindings: &[KeyBinding]) -> Vec<Line<'static>> {
    let shortcut_base = keyboard.shortcut_base;
    let advice = keymap::shortcut_base_advice(shortcut_base);
    let advice_heading = if advice.is_recommended {
        "Recommended"
    } else {
        "Warning"
    };
    let advice_color = if advice.is_recommended {
        Color::Green
    } else {
        Color::Yellow
    };
    let label = "Shortcut base";
    let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
    let control = format!("\u{2039} {} \u{203a}", shortcut_base.label());

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled(
                label,
                Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            Span::raw(" ".repeat(padding)),
            Span::styled(
                control,
                theme::selected_style().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            "    Use Left/Right to browse A-Z, or type Shift+A-Z for an exact custom letter.",
            Style::new().add_modifier(Modifier::DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Complete presets",
            Style::new().add_modifier(Modifier::BOLD),
        )),
    ];
    for preset in SHORTCUT_BASE_PRESETS {
        let standard = if preset == ShortcutBase::A {
            "GNU Screen standard"
        } else {
            "tmux standard"
        };
        lines.push(Line::from(format!(
            "  [{} preset] {:<7}  {standard} complete key map",
            if preset == ShortcutBase::A {
                "GNU Screen"
            } else {
                "tmux"
            },
            preset.label(),
        )));
    }
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            format!("{advice_heading}: {}", shortcut_base.label()),
            Style::new().fg(advice_color).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  {}", advice.explanation)),
        Line::from(""),
        Line::from(Span::styled(
            "Each action is remapped live. Duplicate keys are refused; Help mirrors this table immediately.",
            Style::new().add_modifier(Modifier::DIM),
        )),
    ]);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Action             Key     Mnemonic   Available",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    let available = keymap::available_keys(bindings);
    for binding in bindings {
        let choices = available
            .iter()
            .take(3)
            .map(|key| format!("[{key}]"))
            .collect::<Vec<_>>()
            .join(" ");
        let mnemonic = keymap::mnemonic_for(binding.action, binding.letter).unwrap_or("—");
        lines.push(Line::from(format!(
            " {:<KEYBOARD_ACTION_COLUMN_WIDTH$.KEYBOARD_ACTION_COLUMN_WIDTH$} [{:^KEYBOARD_KEY_CAP_WIDTH$}] {:<KEYBOARD_MNEMONIC_COLUMN_WIDTH$.KEYBOARD_MNEMONIC_COLUMN_WIDTH$} {} [+]",
            keymap::action_label(binding.action),
            keymap::key_label(binding.letter),
            mnemonic,
            choices,
        )));
    }
    lines
}

/// A table click either applies one of the visible free-key suggestions or
/// opens the physical keyboard picker for the row's action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardTableAction {
    Assign(char),
    OpenPicker(Action),
}

pub const KEYBOARD_TABLE_FIRST_ROW: u16 = 14;

pub fn keyboard_table_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    bindings: &[KeyBinding],
) -> Option<KeyboardTableAction> {
    if !content_area.contains(position) {
        return None;
    }
    let line = position
        .y
        .saturating_sub(content_area.y)
        .saturating_add(scroll);
    let row_index = usize::from(line.checked_sub(KEYBOARD_TABLE_FIRST_ROW)?);
    let binding = *bindings.get(row_index)?;
    let available = keymap::available_keys(bindings);
    // Every rendered suggestion consumes four cells: `[x] `. Keep these
    // offsets tied to the fixed-width action, key-cap, and mnemonic columns
    // in `keyboard_lines`, so the visual addition cannot shift mouse hits.
    let suggestion_start = content_area
        .x
        .saturating_add(KEYBOARD_SUGGESTION_START_OFFSET);
    let picker_start = content_area.x.saturating_add(KEYBOARD_PICKER_START_OFFSET);
    if position.x >= picker_start {
        return Some(KeyboardTableAction::OpenPicker(binding.action));
    }
    if position.x >= suggestion_start {
        let index = usize::from(position.x.saturating_sub(suggestion_start) / 4);
        return available
            .get(index)
            .copied()
            .map(KeyboardTableAction::Assign);
    }
    None
}

/// Returns the complete keymap preset row whose rendered line was clicked.
pub fn keyboard_keymap_preset_at(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<KeymapPreset> {
    if !content_area.contains(position) {
        return None;
    }
    let line = position
        .y
        .saturating_sub(content_area.y)
        .saturating_add(scroll);
    let preset_index = usize::from(line.checked_sub(5)?);
    KeymapPreset::ALL.get(preset_index).copied()
}

const KEYBOARD_PICKER_ROWS: &[&str] = &[
    "`1234567890-=",
    "qwertyuiop[]\\",
    "asdfghjkl;'",
    "zxcvbnm,./",
    "~!@#$%^&*()_+",
    "QWERTYUIOP{}|",
    "ASDFGHJKL:\"",
    "ZXCVBNM<>?",
];

fn render_keyboard_picker(frame: &mut Frame, area: Rect, app: &App, action: Action) {
    let popup = centered_rect(78, 58, area);
    frame.render_widget(Clear, popup);
    let available = keymap::available_keys(&app.keybindings);
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "Choose key for {}",
                keymap::action_name(action).replace('_', " ")
            ),
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Line::from("Only unassigned keys are active. Esc closes without changing anything."),
        Line::from(""),
    ];
    for (row_index, row) in KEYBOARD_PICKER_ROWS.iter().enumerate() {
        let indent = " ".repeat(row_index * 2 + 2);
        let mut spans = vec![Span::raw(indent)];
        for key in row.chars() {
            let style = if available.contains(&key) {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else {
                Style::new().add_modifier(Modifier::DIM)
            };
            spans.push(Span::styled(format!("[{key}] "), style));
        }
        lines.push(Line::from(spans));
    }
    let space_style = if available.contains(&' ') {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    lines.push(Line::from(vec![
        Span::raw("                  "),
        Span::styled("[            Space            ]", space_style),
    ]));
    frame.render_widget(
        Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title("Keyboard"))),
        popup,
    );
}

/// Decodes a click on an *available* keycap in the staggered picker. Occupied
/// keys deliberately behave like inert keyboard plastic: they cannot close
/// the dialog or reach the assignment layer at all.
pub fn keyboard_picker_key_at(area: Rect, position: Position, available: &[char]) -> Option<char> {
    let popup = centered_rect(78, 58, area);
    for (row_index, row) in KEYBOARD_PICKER_ROWS.iter().enumerate() {
        let y = popup.y.saturating_add(4 + row_index as u16);
        if position.y != y {
            continue;
        }
        let start_x = popup.x.saturating_add(2 + (row_index * 2) as u16);
        let index = usize::from(position.x.saturating_sub(start_x) / 4);
        return row.chars().nth(index).filter(|key| available.contains(key));
    }
    let space_y = popup
        .y
        .saturating_add(4 + KEYBOARD_PICKER_ROWS.len() as u16);
    if position.y == space_y
        && position.x >= popup.x.saturating_add(18)
        && position.x < popup.x.saturating_add(49)
        && available.contains(&' ')
    {
        return Some(' ');
    }
    None
}

/// Returns the decrement/increment direction for the Keyboard tab's one
/// selector. Geometry mirrors the selector line in [`keyboard_lines`].
pub fn keyboard_content_hit(content_area: Rect, scroll: u16, position: Position) -> Option<i32> {
    if !content_area.contains(position) {
        return None;
    }
    // Map screen position back to a content line index (the same
    // scroll-aware direction `appearance_content_hit` uses) rather than
    // projecting the selector's fixed content line forward onto the screen
    // -- the forward direction underflows via `saturating_sub` once `scroll`
    // exceeds `content_area.y + APPEARANCE_TOP_PADDING`, clamping to 0 and
    // producing a false-positive hit on whatever content has scrolled into
    // the first on-screen row.
    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll);
    if line_index != i32::from(APPEARANCE_TOP_PADDING) {
        return None;
    }
    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    Some(if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    })
}

/// Returns the explicit A/B preset whose rendered row was clicked.
pub fn keyboard_preset_at(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<ShortcutBase> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index = position
        .y
        .saturating_sub(content_area.y)
        .saturating_add(scroll);
    // `keyboard_lines`: blank, selector, description, blank, heading, then
    // one row per preset.
    let preset_index = usize::from(line_index.checked_sub(5)?);
    SHORTCUT_BASE_PRESETS.get(preset_index).copied()
}

fn appearance_row_label(row: AppearanceRow) -> &'static str {
    match row {
        AppearanceRow::AutoResizeTree => "Auto-resize tree panel on focus",
        AppearanceRow::TreeWidth => "Tree panel width",
        AppearanceRow::TreeOrder => "Tree order",
        AppearanceRow::AgentIdentifierMode => "Agent identifier",
        AppearanceRow::ColorScheme => "Color theme",
        AppearanceRow::MotionLevel => "Motion level",
        AppearanceRow::SidebarDensity => "Sidebar density",
        AppearanceRow::UseStableGlyphs => "Use stable glyphs",
    }
}

fn appearance_row_description(row: AppearanceRow) -> &'static str {
    match row {
        AppearanceRow::AutoResizeTree => {
            "Widen the tree panel while it has mouse or keyboard focus, then ease back."
        }
        AppearanceRow::TreeWidth => "Collapsed width of the tree panel, in terminal columns.",
        AppearanceRow::TreeOrder => {
            "Order entries independently inside each group; split-view placement stays fixed."
        }
        AppearanceRow::AgentIdentifierMode => {
            "Show each agent type as its full name, one letter, a chosen icon, or nothing."
        }
        AppearanceRow::ColorScheme => "ilium's built-in color presets.",
        AppearanceRow::MotionLevel => {
            "Full keeps spatial transitions; reduced and off minimise motion."
        }
        AppearanceRow::SidebarDensity => "Controls the vertical spacing used by the tree sidebar.",
        AppearanceRow::UseStableGlyphs => {
            "Use plain one-cell symbols for hover actions on terminals that cannot render emoji; normal icons remain the default."
        }
    }
}

fn appearance_row_value(row: AppearanceRow, ui: &UiSettings) -> String {
    match row {
        AppearanceRow::AutoResizeTree => {
            if ui.auto_resize_tree_on_focus {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        AppearanceRow::TreeWidth => ui.tree_width.to_string(),
        AppearanceRow::TreeOrder => ui.tree_order.label().to_string(),
        AppearanceRow::AgentIdentifierMode => ui.agent_identifiers.mode.label().to_string(),
        AppearanceRow::ColorScheme => match ui.color_scheme {
            ColorScheme::Dark => "Dark".to_string(),
            ColorScheme::Light => "Light".to_string(),
        },
        AppearanceRow::MotionLevel => ui.motion_level.label().to_string(),
        AppearanceRow::SidebarDensity => ui.sidebar_density.label().to_string(),
        AppearanceRow::UseStableGlyphs => {
            if ui.use_stable_glyphs {
                "On".to_string()
            } else {
                "Off (normal icons)".to_string()
            }
        }
    }
}

fn setting_lines(rows: &[(&str, String, &str)], selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for (index, (label, value, description)) in rows.iter().enumerate() {
        let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
        let label_style = if index == selected_row {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new()
        };
        let control_style = if index == selected_row {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::accent_bg())
        };
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
            Span::styled((*label).to_string(), label_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(format!("‹ {value} ›"), control_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {description}"),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines
}
fn terminal_lines(settings: &TerminalSettings, selected: usize) -> Vec<Line<'static>> {
    setting_lines(
        &[
            (
                "Scrollback budget",
                format!("{} MiB", settings.scrollback_budget_mib),
                "Maximum retained terminal output per pane; takes effect for new terminal views.",
            ),
            (
                "New pane directory",
                settings.new_pane_directory.label().to_string(),
                "Choose the initial directory for a terminal pane.",
            ),
        ],
        selected,
    )
}
fn editor_lines(settings: &EditorSettings, selected: usize) -> Vec<Line<'static>> {
    setting_lines(
        &[
            (
                "Line numbers",
                on_off(settings.show_line_numbers),
                "Default visibility for local editor buffers.",
            ),
            (
                "Minimap",
                on_off(settings.show_minimap),
                "Default minimap visibility for local editor buffers.",
            ),
            (
                "Autosave",
                on_off(settings.autosave_enabled),
                "Save modified local files automatically.",
            ),
            (
                "Autosave delay",
                format!("{} ms", settings.autosave_delay_ms),
                "Delay after the latest edit before autosaving.",
            ),
            (
                "Markdown default",
                if settings.markdown_rendered_by_default {
                    "Rendered".into()
                } else {
                    "Source".into()
                },
                "Initial view for Markdown files opened in the editor.",
            ),
        ],
        selected,
    )
}
fn session_lines(settings: &SessionSettings, selected: usize) -> Vec<Line<'static>> {
    setting_lines(
        &[(
            "Recovery policy",
            settings.recovery_policy.label().to_string(),
            "Used by the detached server when it next starts this project session.",
        )],
        selected,
    )
}

fn voice_lines(app: &App, selected: usize) -> Vec<Line<'static>> {
    let settings = &app.voice_settings;
    let state = match &app.voice_connection_state {
        ilium_voice::VoiceConnectionState::Disabled => "Disabled".to_owned(),
        ilium_voice::VoiceConnectionState::Connecting => "Connecting…".to_owned(),
        ilium_voice::VoiceConnectionState::Listening => "Listening".to_owned(),
        ilium_voice::VoiceConnectionState::Recording => "Recording".to_owned(),
        ilium_voice::VoiceConnectionState::Thinking => "Thinking".to_owned(),
        ilium_voice::VoiceConnectionState::Speaking => "Speaking".to_owned(),
        ilium_voice::VoiceConnectionState::Failed(error) => format!("Failed: {error}"),
    };
    let input_device = settings
        .input_device_name
        .clone()
        .unwrap_or_else(|| "System default".to_owned());
    let output_device = settings
        .output_device_name
        .clone()
        .unwrap_or_else(|| "System default".to_owned());
    let prompt_summary = if settings.custom_prompt.trim().is_empty() {
        "Default only".to_owned()
    } else {
        format!("Custom · {} lines", settings.custom_prompt.lines().count())
    };
    setting_lines(
        &[
            (
                "Voice control",
                if settings.enabled { state } else { "Off".to_owned() },
                "Starts or stops the owned microphone, Realtime session, and speaker actor.",
            ),
            (
                "OpenAI API key",
                if settings.api_key.is_empty() {
                    if std::env::var_os("OPENAI_API_KEY").is_some() {
                        "Configured via OPENAI_API_KEY".to_owned()
                    } else {
                        "Not configured".to_owned()
                    }
                } else {
                    "Configured ••••••••".to_owned()
                },
                "Enter or click to replace the key. The value is masked and never exposed to tools.",
            ),
            (
                "Model",
                settings.model.label().to_owned(),
                "Use the most capable Realtime model or its faster, lower-cost mini variant.",
            ),
            (
                "Voice",
                settings.voice.label().to_owned(),
                "OpenAI voice for this session; reconnecting is required once audio has begun.",
            ),
            (
                "Reasoning effort",
                settings.reasoning_effort.label().to_owned(),
                "Higher effort can improve multi-step tool selection at the cost of latency.",
            ),
            (
                "Input mode",
                settings.input_mode.label().to_owned(),
                "Semantic VAD is hands-free; push to talk commits only while its key is held.",
            ),
            (
                "VAD eagerness",
                settings.vad_eagerness.label().to_owned(),
                "Controls how quickly semantic VAD decides that the user's turn is complete.",
            ),
            (
                "Input device",
                input_device,
                "Microphone captured by CPAL. System default follows the operating-system choice.",
            ),
            (
                "Output device",
                output_device,
                "Speaker used for streamed model audio.",
            ),
            (
                "Output volume",
                format!("{}%", settings.output_volume_percent),
                "Local playback gain; this does not change microphone input.",
            ),
            (
                "Custom prompt",
                prompt_summary,
                "Enter or click to edit additive instructions in a multiline text area.",
            ),
            (
                "Connection",
                "Reconnect now".to_owned(),
                "Restart audio and the WebSocket with the current persisted settings.",
            ),
        ],
        selected,
    )
}
fn on_off(value: bool) -> String {
    if value {
        "On".into()
    } else {
        "Off".into()
    }
}

/// One label/value line per `AppearanceRow`, each followed by a dim
/// description line and a blank spacer -- see [`appearance_content_hit`]
/// for the matching hit-test arithmetic, and the module doc comment for why
/// both read `AppearanceRow::ALL` instead of hardcoding a row count.
fn appearance_lines(ui: &UiSettings, selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(
        usize::from(APPEARANCE_TOP_PADDING)
            + AppearanceRow::ALL.len() * usize::from(APPEARANCE_ROW_HEIGHT),
    );
    lines.push(Line::from(""));
    for (index, row) in AppearanceRow::ALL.into_iter().enumerate() {
        let selected = index == selected_row;
        lines.push(appearance_row_line(row, ui, selected));
        lines.push(Line::from(Span::styled(
            format!("    {}", appearance_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines
}

fn appearance_row_line(row: AppearanceRow, ui: &UiSettings, selected: bool) -> Line<'static> {
    let label = appearance_row_label(row);
    let value = appearance_row_value(row, ui);
    let control = format!("\u{2039} {value} \u{203a}");

    let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
    let label_style = if selected {
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::new()
    };
    let control_style = if selected {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme::accent_bg())
    };

    Line::from(vec![
        Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
        Span::styled(label, label_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(control, control_style),
    ])
}

/// The `(row, direction)` a click at `position` should adjust, if any.
/// `direction` is `-1` for the `‹` hot zone, `+1` for anywhere else in the
/// control -- see `App::settings_adjust_row`. Reproduces
/// [`appearance_lines`]'s exact line arithmetic (top padding, three lines
/// per row) and [`appearance_row_line`]'s exact column arithmetic (left
/// inset, padded label column) rather than re-deriving either.
pub fn appearance_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<(AppearanceRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }

    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll);
    let line_index = line_index - i32::from(APPEARANCE_TOP_PADDING);
    if line_index < 0 || line_index % i32::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row_index = usize::try_from(line_index / i32::from(APPEARANCE_ROW_HEIGHT)).ok()?;
    let row = AppearanceRow::ALL.get(row_index).copied()?;

    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    };
    Some((row, direction))
}

/// Shared hit geometry for the small fixed-row settings tabs.
pub fn simple_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
    row_count: usize,
) -> Option<(usize, i32)> {
    if !content_area.contains(position) {
        return None;
    }
    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll)
        - i32::from(APPEARANCE_TOP_PADDING);
    if line_index < 0 || line_index % i32::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row = usize::try_from(line_index / i32::from(APPEARANCE_ROW_HEIGHT)).ok()?;
    if row >= row_count {
        return None;
    }
    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    (position.x >= control_start_x).then_some((
        row,
        if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
            -1
        } else {
            1
        },
    ))
}

/// Static "About" content: version plus a short description. No settings
/// live here, per the design brief -- see the module doc comment.
fn about_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "ilium",
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", env!("CARGO_PKG_VERSION"))),
        Line::from(""),
        Line::from("A terminal multiplexer with a tree-structured pane list and AI"),
        Line::from(
            "coding agent awareness (Claude Code, Codex CLI, Antigravity CLI, and similar tools).",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn compute_layout_leaves_no_border_gap_between_tabs_and_content() {
        let layout = compute_layout(Rect::new(0, 0, 100, 40));
        assert!(layout.content_area.x > layout.tab_list_area.right());
        assert_eq!(
            layout.content_area.x - layout.tab_list_area.right(),
            GAP_WIDTH
        );
    }

    #[test]
    fn tab_at_maps_each_row_to_its_tab() {
        let area = Rect::new(0, 0, 30, 22);
        assert_eq!(
            tab_at(area, Position::new(2, 1)),
            Some(SettingsTab::Appearance)
        );
        assert_eq!(tab_at(area, Position::new(2, 3)), Some(SettingsTab::Icons));
        assert_eq!(
            tab_at(area, Position::new(2, 5)),
            Some(SettingsTab::Keyboard)
        );
        assert_eq!(
            tab_at(area, Position::new(2, 7)),
            Some(SettingsTab::Terminal)
        );
        assert_eq!(tab_at(area, Position::new(2, 9)), Some(SettingsTab::Editor));
        assert_eq!(
            tab_at(area, Position::new(2, 11)),
            Some(SettingsTab::Session)
        );
        assert_eq!(
            tab_at(area, Position::new(2, 13)),
            Some(SettingsTab::KanbanBoard)
        );
        assert_eq!(tab_at(area, Position::new(2, 15)), Some(SettingsTab::Sound));
        assert_eq!(
            tab_at(area, Position::new(2, 17)),
            Some(SettingsTab::VoiceControl)
        );
        assert_eq!(
            tab_at(area, Position::new(2, 19)),
            Some(SettingsTab::Inference)
        );
        assert_eq!(tab_at(area, Position::new(2, 21)), Some(SettingsTab::About));
        // Row 0 is the top-padding blank line -- no tab there.
        assert_eq!(tab_at(area, Position::new(2, 0)), None);
        // Row 2 is the blank spacer after the first tab.
        assert_eq!(tab_at(area, Position::new(2, 2)), None);
    }

    #[test]
    fn icons_tab_renders_the_assignment_preview_and_chaptered_catalogue() {
        let app = App::new("test-session".to_string(), std::path::PathBuf::from("/tmp"));
        let state = SettingsState {
            tab: SettingsTab::Icons,
            ..SettingsState::default()
        };
        let backend = TestBackend::new(160, 45);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Icon assignments"));
        assert!(rendered.contains("Sidebar preview"));
        assert!(rendered.contains("Terminal"));
        assert!(rendered.contains("[+]"));

        let picker_state = SettingsState {
            tab: SettingsTab::Icons,
            icon_picker: Some(crate::app::IconPickerState::new(IconTarget::Terminal)),
            ..SettingsState::default()
        };
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &picker_state))
            .unwrap();
        let picker_rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(picker_rendered.contains("Icon catalogue"));
        assert!(picker_rendered.contains("Quick picks"));
        assert!(picker_rendered.contains("Official UTF-8 chapters first"));
        assert!(picker_rendered.contains("Multi-column"));
        assert!(picker_rendered.contains("folder"));
        assert!(picker_rendered.contains("Nerd Font"));
        assert!(
            !picker_rendered.contains("Sidebar preview"),
            "the catalogue must render as its own frame, without stale Icons-tab content"
        );
        assert!(
            !picker_rendered.contains("[+]"),
            "the previous assignment table must not leak through the catalogue"
        );

        let filtered_picker_state = SettingsState {
            tab: SettingsTab::Icons,
            icon_picker: Some(crate::app::IconPickerState {
                search_query: "rocket".to_string(),
                search_results: crate::icon_settings::semantic_picker_search_results(vec![
                    crate::icon_settings::IconSemanticSearchHit {
                        category_label: "Travel · Air",
                        family: IconCatalogFamily::OfficialUtf8,
                        entry: IconCatalogEntry {
                            glyph: "🚀",
                            name: "rocket",
                        },
                    },
                ]),
                ..crate::app::IconPickerState::new(IconTarget::Terminal)
            }),
            ..SettingsState::default()
        };
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &filtered_picker_state))
            .unwrap();
        let filtered_rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(filtered_rendered.contains("rocket"));
        assert!(!filtered_rendered.contains("Quick picks"));

        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();
        let returned_to_icons = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(returned_to_icons.contains("Icon assignments"));
        assert!(
            !returned_to_icons.contains("rocket"),
            "returning to icon assignments must not retain catalogue cells"
        );
    }

    #[test]
    fn icons_tab_keeps_the_catalogue_button_visible_at_compact_width() {
        let app = App::new("test-session".to_string(), std::path::PathBuf::from("/tmp"));
        let state = SettingsState {
            tab: SettingsTab::Icons,
            ..SettingsState::default()
        };
        let backend = TestBackend::new(120, 45);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("[+]"));
    }

    #[test]
    fn icon_table_hit_uses_the_rendered_data_row_and_exact_catalogue_button() {
        let area = Rect::new(0, 0, 120, 30);
        let (table, _) = icon_tab_columns(area);
        let inner = table.inner(ratatui::layout::Margin::new(1, 1));

        // The first inner line is the table heading. The first target starts
        // one row below it, and must not be interpreted as the second target.
        assert_eq!(
            icons_table_hit(area, 0, Position::new(inner.x + 1, inner.y + 1)),
            Some(IconTableHit {
                target: IconTarget::Group,
                action: IconTableAction::CycleSuggestion,
            })
        );
        assert_eq!(
            icons_table_hit(area, 0, Position::new(inner.x + 1, inner.y + 2)),
            None,
            "the spacer below an assignment is deliberately not interactive"
        );
        assert_eq!(
            icons_table_hit(area, 0, Position::new(inner.x + 1, inner.y + 3)),
            Some(IconTableHit {
                target: IconTarget::TopLevel,
                action: IconTableAction::CycleSuggestion,
            })
        );
        assert_eq!(
            icons_table_hit(
                area,
                0,
                Position::new(
                    inner.x + icon_table_picker_column(icon_table_column_gap(table.width)) as u16,
                    inner.y + 1,
                ),
            ),
            Some(IconTableHit {
                target: IconTarget::Group,
                action: IconTableAction::OpenCatalogue,
            })
        );
        assert_eq!(
            icons_table_hit(area, 0, Position::new(inner.x, inner.y)),
            None
        );
    }

    #[test]
    fn icon_choices_keep_fixed_terminal_cell_width_when_selected() {
        for target in IconTarget::ALL {
            for glyph in target.suggestions() {
                let unselected_slot = icon_choice_slot(glyph, false);
                let selected_slot = icon_choice_slot(glyph, true);
                assert_eq!(
                    UnicodeWidthStr::width(unselected_slot.as_str()),
                    ICON_TABLE_CHOICE_WIDTH
                );
                assert_eq!(
                    UnicodeWidthStr::width(selected_slot.as_str()),
                    ICON_TABLE_CHOICE_WIDTH
                );
            }
        }

        let choices = IconTarget::Folder
            .suggestions()
            .iter()
            .map(|glyph| icon_choice_slot(glyph, *glyph == "🗂️"))
            .collect::<Vec<_>>()
            .join(" ");
        let row = format_icon_assignment_row("Folder", "🗂️", &choices, ICON_TABLE_COMPACT_GAP);
        assert_eq!(
            UnicodeWidthStr::width(row.as_str()),
            ICON_TABLE_PICKER_COLUMN + ICON_TABLE_PICKER_LABEL.width()
        );
    }

    #[test]
    fn icon_table_uses_comfortable_column_gaps_only_when_the_table_is_wide_enough() {
        assert_eq!(
            icon_table_column_gap(ICON_TABLE_MIN_OUTER_WIDTH),
            ICON_TABLE_COMPACT_GAP
        );
        assert_eq!(
            icon_table_column_gap(ICON_TABLE_COMFORTABLE_OUTER_WIDTH),
            ICON_TABLE_COMFORTABLE_GAP
        );
    }

    #[test]
    fn inference_rows_hide_impossible_fields_and_warn_for_kilo() {
        let settings = ilium_inference::InferenceSettings::default();
        assert_eq!(
            inference_rows(&settings),
            vec![InferenceRow::Provider, InferenceRow::Test]
        );
        let text = inference_lines(
            &settings,
            None,
            &InferenceTestState::Idle,
            &OllamaModelDiscoveryState::Idle,
            &[],
            0,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(text.contains("Do not send secrets"));
        assert!(text.contains("LLM training"));

        let mut ollama = settings;
        ollama.selected_provider = ilium_inference::InferenceProviderKind::Ollama;
        assert!(inference_rows(&ollama).contains(&InferenceRow::RefreshOllamaModels));

        let discovered_models = vec!["qwen3.6:latest".to_string(), "gemma4:12b".to_string()];
        let loading_text = inference_lines(
            &ollama,
            None,
            &InferenceTestState::Idle,
            &OllamaModelDiscoveryState::Loading {
                endpoint: "http://127.0.0.1:11434".to_string(),
                started_at: std::time::Instant::now(),
            },
            &discovered_models,
            0,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(loading_text.contains("Discovering local models"));
        assert!(loading_text.contains("Ollama model discovery log"));
        assert!(loading_text.contains("GET http://127.0.0.1:11434/api/tags"));
        assert!(loading_text.contains("qwen3.6:latest"));
    }

    #[test]
    fn inference_test_lifecycle_renders_a_boxed_running_log_and_failure() {
        let settings = ilium_inference::InferenceSettings::default();
        let running = inference_lines(
            &settings,
            None,
            &InferenceTestState::Running {
                provider: ilium_inference::InferenceProviderKind::Ollama,
                started_at: std::time::Instant::now(),
            },
            &OllamaModelDiscoveryState::Idle,
            &[],
            0,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(running.contains("Provider test log"));
        assert!(running.contains("Testing Ollama (local)"));
        assert!(running.contains("Wait for and validate the required JSON response"));

        let failed = inference_test_log_lines(&InferenceTestState::Failed {
            provider: ilium_inference::InferenceProviderKind::Ollama,
            error: "connection refused".to_string(),
            elapsed: std::time::Duration::from_millis(250),
        })
        .into_iter()
        .map(|line| line.to_string())
        .collect::<String>();
        assert!(failed.contains("test failed after 0.25s"));
        assert!(failed.contains("Provider error: connection refused"));
        assert!(failed.contains("No settings were changed"));
    }

    #[test]
    fn running_inference_test_is_visibly_boxed_in_the_rendered_settings_screen() {
        let mut app = App::new("test-session".to_string(), std::path::PathBuf::from("/tmp"));
        app.settings_select_inference_provider(ilium_inference::InferenceProviderKind::Ollama);
        app.request_inference_test();
        let state = SettingsState {
            tab: SettingsTab::Inference,
            selected_row: 0,
            scroll: 0,
            ..SettingsState::default()
        };
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Provider test log"));
        assert!(rendered.contains("Testing Ollama (local)"));
        assert!(rendered.contains("Wait for and validate"));
        assert!(rendered.contains("┌"));
        assert!(rendered.contains("│"));
    }

    #[test]
    fn running_ollama_discovery_is_visibly_boxed_in_the_rendered_settings_screen() {
        let mut app = App::new("test-session".to_string(), std::path::PathBuf::from("/tmp"));
        app.settings_select_inference_provider(ilium_inference::InferenceProviderKind::Ollama);
        app.request_ollama_model_refresh();
        let state = SettingsState {
            tab: SettingsTab::Inference,
            selected_row: 1,
            scroll: 0,
            ..SettingsState::default()
        };
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Ollama model discovery log"));
        assert!(rendered.contains("Discovering local models"));
        assert!(rendered.contains("Request GET"));
        assert!(rendered.contains("┌"));
        assert!(rendered.contains("│"));
    }

    #[test]
    fn appearance_content_hit_finds_each_row_and_the_correct_zone() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;

        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some((AppearanceRow::AutoResizeTree, -1))
        );
        assert_eq!(
            appearance_content_hit(
                area,
                0,
                Position::new(control_start_x + DECREMENT_ZONE_WIDTH, 1)
            ),
            Some((AppearanceRow::AutoResizeTree, 1))
        );
        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 4)),
            Some((AppearanceRow::TreeWidth, -1))
        );
        // A click on the label (left of the control) is a no-op.
        assert_eq!(appearance_content_hit(area, 0, Position::new(2, 1)), None);
        // A click on the description line between rows is a no-op.
        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 2)),
            None
        );
    }

    #[test]
    fn appearance_content_hit_accounts_for_scroll_offset() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        // Scrolled down by 3 lines, row 1 on screen is TreeWidth's line
        // (originally at line 4).
        assert_eq!(
            appearance_content_hit(area, 3, Position::new(control_start_x, 1)),
            Some((AppearanceRow::TreeWidth, -1))
        );
    }

    #[test]
    fn appearance_lines_expose_agent_mode_and_stable_glyph_opt_in() {
        let ui = UiSettings {
            tree_order: crate::config::TreeOrder::AgeAscending,
            agent_identifiers: crate::config::AgentIdentifierSettings {
                mode: crate::config::AgentIdentifierMode::Icon,
                claude_icon: crate::config::ClaudeAgentIcon::MagicWand,
                codex_icon: crate::config::CodexAgentIcon::Tools,
                antigravity_icon: crate::config::AntigravityAgentIcon::Orbit,
            },
            ..UiSettings::default()
        };

        let rendered = appearance_lines(&ui, 3)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Tree order"));
        assert!(rendered.contains("Age up (newest first)"));
        assert!(rendered.contains("Agent identifier"));
        assert!(rendered.contains("Selected icon"));
        assert!(!rendered.contains("Claude icon"));
        assert!(!rendered.contains("Codex icon"));
        assert!(!rendered.contains("Antigravity icon"));
        assert!(rendered.contains("Use stable glyphs"));
        assert!(rendered.contains("Off (normal icons)"));
    }

    #[test]
    fn keyboard_content_hit_finds_both_stepper_directions() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        assert_eq!(
            keyboard_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some(-1)
        );
        assert_eq!(
            keyboard_content_hit(
                area,
                0,
                Position::new(control_start_x + DECREMENT_ZONE_WIDTH, 1)
            ),
            Some(1)
        );
        assert_eq!(keyboard_content_hit(area, 0, Position::new(2, 1)), None);
    }

    #[test]
    fn kanban_board_tab_renders_and_hits_both_layout_steppers() {
        let settings = KanbanBoardSettings {
            card_preview_lines: 3,
            minimum_column_width: 20,
        };
        let rendered = kanban_board_lines(&settings, 0)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let area = Rect::new(0, 0, 70, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;

        assert!(rendered.contains("Card preview lines"));
        assert!(rendered.contains("‹ 3 ›"));
        assert!(rendered.contains("Minimum column width"));
        assert!(rendered.contains("‹ 20 ›"));
        assert_eq!(
            kanban_board_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some((KanbanBoardRow::CardPreviewLines, -1))
        );
        assert_eq!(
            kanban_board_content_hit(area, 0, Position::new(control_start_x + 3, 1)),
            Some((KanbanBoardRow::CardPreviewLines, 1))
        );
        assert_eq!(
            kanban_board_content_hit(area, 0, Position::new(control_start_x + 3, 4)),
            Some((KanbanBoardRow::MinimumColumnWidth, 1))
        );
    }

    #[test]
    fn keyboard_preset_hit_testing_maps_the_two_rendered_rows() {
        let area = Rect::new(0, 0, 60, 20);
        assert_eq!(
            keyboard_preset_at(area, 0, Position::new(2, 5)),
            Some(ShortcutBase::A)
        );
        assert_eq!(
            keyboard_preset_at(area, 0, Position::new(2, 6)),
            Some(ShortcutBase::B)
        );
        assert_eq!(keyboard_preset_at(area, 0, Position::new(2, 4)), None);
    }

    #[test]
    fn keyboard_lines_show_presets_and_specific_current_advice() {
        let mut keyboard = KeyboardSettings::default();
        let recommended = keyboard_lines(&keyboard, keymap::LEADER_BINDINGS)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(recommended.contains("[GNU Screen preset] Ctrl+A"));
        assert!(recommended.contains("[tmux preset] Ctrl+B"));
        assert!(recommended.contains("Recommended: Ctrl+A"));

        keyboard.shortcut_base = ShortcutBase::parse("c").unwrap();
        let warned = keyboard_lines(&keyboard, keymap::LEADER_BINDINGS)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(warned.contains("Warning: Ctrl+C"));
        assert!(warned.contains("interrupt/SIGINT"));
    }

    #[test]
    fn keyboard_table_exposes_free_keys_and_its_picker_target() {
        let area = Rect::new(0, 0, 100, 40);
        let bindings = keymap::LEADER_BINDINGS.to_vec();
        let lines = keyboard_lines(&KeyboardSettings::default(), &bindings)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(lines.contains("Mnemonic"));
        assert!(lines.contains("[  c  ] create"));
        assert!(lines.contains("Available"));
        assert!(lines.contains("[+]"));
        assert_eq!(
            keyboard_table_hit(
                area,
                0,
                Position::new(37, KEYBOARD_TABLE_FIRST_ROW),
                &bindings,
            ),
            Some(KeyboardTableAction::Assign('i'))
        );
        assert_eq!(
            keyboard_table_hit(
                area,
                0,
                Position::new(70, KEYBOARD_TABLE_FIRST_ROW),
                &bindings,
            ),
            Some(KeyboardTableAction::OpenPicker(Action::NewTerminal))
        );
    }

    #[test]
    fn keyboard_table_renders_every_action_from_the_live_remapped_table() {
        let mut bindings = keymap::LEADER_BINDINGS.to_vec();
        keymap::assign_key(&mut bindings, Action::NewGroup, 'z')
            .expect("z is free in the default map");
        let rendered = keyboard_lines(&KeyboardSettings::default(), &bindings)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(
            rendered.matches("New group").count(),
            1,
            "the table must have one row for every action"
        );
        assert!(rendered.contains("New group        [  z  ] —"));
    }

    #[test]
    fn staggered_keyboard_maps_clicks_to_the_rendered_keycaps() {
        let area = Rect::new(0, 0, 120, 50);
        let popup = centered_rect(78, 58, area);
        let available = vec!['1', 'q', ' '];
        assert_eq!(
            keyboard_picker_key_at(area, Position::new(popup.x + 6, popup.y + 4), &available),
            Some('1')
        );
        assert_eq!(
            keyboard_picker_key_at(area, Position::new(popup.x + 4, popup.y + 5), &available),
            Some('q')
        );
        assert_eq!(
            keyboard_picker_key_at(area, Position::new(popup.x + 8, popup.y + 5), &available),
            None
        );
        assert_eq!(
            keyboard_picker_key_at(area, Position::new(popup.x + 18, popup.y + 12), &available),
            Some(' ')
        );
    }

    #[test]
    fn staggered_keyboard_represents_every_non_space_bindable_key_once() {
        let picker_keys = KEYBOARD_PICKER_ROWS
            .iter()
            .flat_map(|row| row.chars())
            .collect::<Vec<_>>();
        let expected_keys = keymap::BINDABLE_KEYS
            .iter()
            .copied()
            .filter(|key| *key != ' ')
            .collect::<Vec<_>>();

        assert_eq!(picker_keys.len(), expected_keys.len());
        for key in expected_keys {
            assert_eq!(
                picker_keys
                    .iter()
                    .filter(|candidate| **candidate == key)
                    .count(),
                1,
                "{key:?} must have one visual keycap"
            );
        }
    }

    #[test]
    fn max_scroll_is_zero_when_content_already_fits() {
        let app = App::new(
            "settings-test".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let area = Rect::new(0, 0, 60, 40);
        assert_eq!(max_scroll(SettingsTab::Appearance, &app, 0, area), 0);
    }

    #[test]
    fn max_scroll_is_positive_when_content_overflows_a_short_terminal() {
        let app = App::new(
            "settings-test".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let area = Rect::new(0, 0, 60, 3);
        assert!(max_scroll(SettingsTab::Appearance, &app, 0, area) > 0);
    }

    fn sound_discovery_fixture() -> ilium_sound::SoundDiscovery {
        ilium_sound::SoundDiscovery {
            platform: "Linux",
            directories: vec![ilium_sound::SoundDirectory {
                path: std::path::PathBuf::from("/usr/share/sounds"),
                origin: "XDG sound themes".to_string(),
            }],
            sounds: vec![ilium_sound::SystemSound {
                path: std::path::PathBuf::from("/usr/share/sounds/complete.oga"),
                display_name: "freedesktop / stereo / complete".to_string(),
                collection: "XDG sound themes".to_string(),
            }],
            was_truncated: false,
        }
    }

    #[test]
    fn icon_picker_hit_tracks_the_rendered_search_and_document_windows() {
        let area = Rect::new(0, 0, 160, 45);
        let picker = crate::app::IconPickerState::new(IconTarget::Terminal);
        let layout = icon_picker_layout(area);

        assert_eq!(
            icon_picker_hit(
                area,
                &picker,
                Position::new(layout.search_area.x, layout.search_area.y),
            ),
            Some(IconPickerHit::ActivateSearch),
        );
        assert_eq!(
            icon_picker_hit(
                area,
                &picker,
                Position::new(layout.view_switch_area.x, layout.view_switch_area.y),
            ),
            Some(IconPickerHit::ToggleColumnMode),
        );
        assert_eq!(
            icon_picker_hit(
                area,
                &picker,
                Position::new(layout.document_area.x, layout.document_area.y + 1),
            ),
            Some(IconPickerHit::Entry(0)),
        );
        assert_eq!(
            icon_picker_hit(area, &picker, Position::new(0, 0)),
            Some(IconPickerHit::Close),
        );
        assert!(matches!(
            icon_picker_hit(
                area,
                &picker,
                Position::new(
                    layout.document_area.right().saturating_sub(1),
                    layout.document_area.y + 4,
                ),
            ),
            Some(IconPickerHit::ScrollTo(_)),
        ));
    }

    #[test]
    fn icon_picker_defaults_to_multi_column_and_maps_cells_without_crossing_rows() {
        let area = Rect::new(0, 0, 160, 45);
        let picker = crate::app::IconPickerState::new(IconTarget::Terminal);
        let layout = icon_picker_layout(area);
        let multi_columns = picker_document_columns(layout.document_area, picker.column_mode);
        let single_columns =
            picker_document_columns(layout.document_area, IconPickerColumnMode::SingleColumn);

        assert_eq!(picker.column_mode, IconPickerColumnMode::MultiColumn);
        assert_eq!(multi_columns, 3);
        assert_eq!(single_columns, 1);
        assert_eq!(
            icon_picker_row_for_entry(&picker.search_results, multi_columns, 2),
            1,
            "the third icon belongs in the first multi-column entry row"
        );
        assert_eq!(
            icon_picker_row_for_entry(&picker.search_results, single_columns, 2),
            3,
            "the same icon remains independently addressable in one-column mode"
        );

        let cell_width = layout.document_area.width.div_ceil(multi_columns as u16);
        assert_eq!(
            icon_picker_hit(
                area,
                &picker,
                Position::new(
                    layout.document_area.x + cell_width * 2,
                    layout.document_area.y + 1
                ),
            ),
            Some(IconPickerHit::Entry(2)),
        );
    }

    #[test]
    fn icon_picker_density_modes_use_distinct_dark_canvases_for_safe_repaint() {
        let multi_canvas = icon_picker_canvas_style(IconPickerColumnMode::MultiColumn);
        let single_canvas = icon_picker_canvas_style(IconPickerColumnMode::SingleColumn);

        assert_ne!(multi_canvas, single_canvas);
        assert_eq!(multi_canvas.bg, Some(Color::Black));
        assert_eq!(single_canvas.bg, Some(Color::Rgb(1, 1, 1)));
    }

    #[test]
    fn icon_picker_never_splits_a_multi_codepoint_emoji_while_clipping_a_cell() {
        let glyph = "👩‍❤️‍💋‍👩";
        assert_eq!(
            truncate_icon_text(glyph, UnicodeWidthStr::width(glyph)),
            glyph,
            "the terminal must receive a complete emoji grapheme, never a dangling gender sign"
        );
    }

    #[test]
    fn icon_picker_keeps_a_far_keyboard_selection_inside_its_viewport() {
        let area = Rect::new(0, 0, 160, 45);
        let mut picker = crate::app::IconPickerState::new(IconTarget::Terminal);
        picker.selected_entry = 1_000;
        let scroll_row = icon_picker_scroll_for_entry(area, &picker);
        let layout = icon_picker_layout(area);
        let columns = picker_document_columns(layout.document_area, picker.column_mode);
        let selected_row = icon_picker_row_for_entry(&picker.search_results, columns, 1_000);
        let viewport_height = usize::from(layout.document_area.height);

        assert!(scroll_row > 0);
        assert!(selected_row >= scroll_row);
        assert!(selected_row < scroll_row + viewport_height);
    }

    #[test]
    fn sound_lines_show_source_events_and_real_discovery_evidence() {
        let settings = ilium_sound::SoundSettings::default();
        let rendered = sound_lines(&settings, &sound_discovery_fixture(), 0)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("System beep"));
        assert!(rendered.contains("[x] Enabled"));
        assert!(rendered.contains("Agent needs approval"));
        assert!(rendered.contains("/usr/share/sounds"));
        assert!(rendered.contains("freedesktop / stereo / complete"));
    }

    #[test]
    fn sound_hit_testing_handles_controls_catalog_and_scroll() {
        let discovery = sound_discovery_fixture();
        let area = Rect::new(0, 0, 100, 40);
        let control_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        assert_eq!(
            sound_content_hit(area, 0, Position::new(control_x, 1), &discovery),
            Some(SoundContentHit::Row {
                row: SoundRow::Source,
                direction: -1
            })
        );
        let first_sound_line = 1
            + SoundRow::ALL.len() as u16 * APPEARANCE_ROW_HEIGHT
            + 4
            + discovery.directories.len() as u16;
        assert_eq!(
            sound_content_hit(area, 0, Position::new(2, first_sound_line), &discovery),
            Some(SoundContentHit::File(0))
        );
        assert_eq!(
            sound_content_hit(area, 3, Position::new(control_x, 1), &discovery),
            Some(SoundContentHit::Row {
                row: SoundRow::File,
                direction: -1
            })
        );
    }

    #[test]
    fn close_button_hit_covers_the_right_aligned_label_only() {
        let header_area = Rect::new(0, 0, 40, 2);
        let label_width = CLOSE_LABEL.chars().count() as u16;

        // Inside the label, top row.
        assert!(close_button_hit(
            header_area,
            Position::new(header_area.right() - 1, 0)
        ));
        // Just left of the label -- still the title, not a hit.
        assert!(!close_button_hit(
            header_area,
            Position::new(header_area.right() - label_width - 1, 0)
        ));
        // Same column, but the hint line below the label -- not a hit.
        assert!(!close_button_hit(
            header_area,
            Position::new(header_area.right() - 1, 1)
        ));
    }

    #[test]
    fn close_button_hit_is_false_when_header_too_narrow_for_the_label() {
        let header_area = Rect::new(0, 0, 3, 2);
        assert!(!close_button_hit(header_area, Position::new(1, 0)));
    }
}
