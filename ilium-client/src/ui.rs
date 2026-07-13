//! Top-level layout: a left tree column, the focused pane's content on
//! the right, and a bottom status bar -- with the Explorer file-picker or
//! Help reference drawn as an overlay on top of everything else when
//! active. It consumes the shared animated `App::layout`; everything it
//! draws is delegated to `tree_ui`, `help`, or the pane runtimes themselves.

use ilium_core::{AgentClass, NodeId, NodeKind, PaneStatus, ROOT_ID};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;

use crate::app::{
    App, BoardDeleteTarget, BoardRenameTarget, BoardStorageKind, ContextMenu, CreateBoardState,
    CreateGroupState, FocusTarget, Mode, PaneRuntime, RightPanelTarget,
};
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::{
    editor_chrome, editor_highlight, editor_toolbar, explorer_overlay, help, markdown, minimap,
    modal, terminal_view, theme, tree_ui,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let layout = app.layout;

    // The full-screen settings view replaces everything else this frame --
    // see `crate::settings_ui`'s module doc comment ("the **entire** screen
    // is replaced") -- so it returns before any of the ordinary tree/pane/
    // status-bar drawing below ever runs, rather than layering on top of it
    // like the popup overlays further down do.
    if let Mode::Settings(state) = &app.mode {
        crate::settings_ui::render(frame, area, app, state);
        return;
    }

    // The pane renders first: `PseudoTerminal` clears its whole area before
    // drawing (so a shrunk PTY screen never leaves stale content behind),
    // which would wipe out the tree's border character on the one column
    // they share. Drawing the tree second means its border merge (which
    // never clears) has the last word and actually fuses the two into a
    // connected `┬`/`┴` joint instead of losing to the pane's plain corner.
    draw_pane(frame, layout.pane_area, app);
    let tree_focused = matches!(app.focus, FocusTarget::Tree);
    tree_ui::render(
        frame,
        layout.tree_area,
        &app.tree,
        &mut app.tree_state,
        tree_ui::TreeRenderOptions {
            focused: tree_focused,
            elapsed_ms: app.started_at.elapsed().as_millis(),
            project_name: app.project_name.as_deref(),
            is_project_name_loading: app.is_project_name_loading,
            titles_loading: &app.titles_loading,
            recently_created: &app.recently_created,
            hover: tree_ui::TreeHoverState {
                node: app.hovered_tree_node,
                toolbar_hovered: app.tree_toolbar_hovered,
                toolbar_action: app.hovered_tree_toolbar_action,
            },
        },
    );

    draw_status_bar(frame, layout.status_area, app);

    // Overlays render last, on top of the layout above.
    if let Mode::Explorer(overlay, _)
    | Mode::FolderExplorer(overlay, _)
    | Mode::BoardPathPicker(overlay, _) = &app.mode
    {
        explorer_overlay::render(frame, area, overlay, std::time::SystemTime::now());
    }
    if let Mode::ExplorerFileMenu(menu) = &app.mode {
        explorer_overlay::render(frame, area, &menu.overlay, std::time::SystemTime::now());
        frame.render_widget(Clear, menu.area);
        let label = menu
            .file_path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "Markdown file",
                    Style::new().add_modifier(Modifier::DIM),
                )),
                Line::from(format!("▦ Create board from {label}")),
            ])
            .block(theme::block(true).title(theme::chrome_title("File actions"))),
            menu.area,
        );
    }
    if matches!(app.mode, Mode::Help) {
        help::render(frame, area);
    }
    if let Mode::ContextMenu(menu) = &app.mode {
        draw_context_menu(frame, menu);
    }
    if let Mode::CreateGroup(state) = &app.mode {
        draw_create_group(frame, app, state);
    }
    if let Mode::CreateBoard(state) = &app.mode {
        draw_create_board(frame, area, state);
    }
    if let Mode::BoardCardPrompt(_, state) = &app.mode {
        modal::render_text_prompt(frame, area, "New card", state);
    }
    if let Mode::BoardColumnPrompt(_, state) = &app.mode {
        modal::render_text_prompt(frame, area, "New column", state);
    }
    if let Mode::BoardRenamePrompt(_, target, state) = &app.mode {
        let title = match target {
            BoardRenameTarget::Card => "Rename card",
            BoardRenameTarget::Column => "Rename column",
        };
        modal::render_text_prompt(frame, area, title, state);
    }
    if let Mode::BoardDeleteConfirm(_, target) = &app.mode {
        let (title, message) = match target {
            BoardDeleteTarget::Card => ("Delete card?", "Delete the selected card?"),
            BoardDeleteTarget::Column => ("Delete column?", "Delete the empty selected column?"),
        };
        modal::render_confirm(frame, area, title, message);
    }
    if let Mode::Rename(state) = &app.mode {
        modal::render_text_prompt(frame, area, "Rename", state);
    }
    if let Mode::CommandPrompt(state) = &app.mode {
        modal::render_text_prompt(frame, area, "Run command", state);
    }
    if let Mode::SaveAs(_, state) = &app.mode {
        modal::render_text_prompt(frame, area, "Save As", state);
    }
    if let Mode::ConfirmClose(target) = &app.mode {
        draw_confirm_close(frame, area, app, *target);
    }
}

fn draw_create_board(frame: &mut Frame, area: Rect, state: &CreateBoardState) {
    let popup = modal::centered_fixed_rect(68, 11, area);
    frame.render_widget(Clear, popup);
    let block = theme::block(true).title(theme::chrome_title("New board"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let storage = match state.storage_kind {
        BoardStorageKind::Folder => "Folder columns + Markdown cards",
        BoardStorageKind::MarkdownFile => "One Markdown file (headings + bullets)",
    };
    let active_name = if state.editing_path { "" } else { " ›" };
    let active_path = if state.editing_path { " ›" } else { "" };
    let lines = vec![
        Line::from(format!("Name{active_name}: {}", state.name.buf)),
        Line::from(format!("Storage: {storage}  (←/→ to switch)")),
        Line::from(format!("Path{active_path}: {}", state.path.buf)),
        Line::from(Span::styled("[ Browse… ]", Style::new().fg(Color::Cyan))),
        Line::from(""),
        Line::from(Span::styled(
            "Tab field · p browse · Enter create · Esc cancel",
            Style::new().add_modifier(Modifier::DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draws the close-confirmation popup, falling back to a generic message
/// if the target somehow no longer exists (e.g. removed by another path
/// the same tick) rather than panicking mid-render.
fn draw_confirm_close(frame: &mut Frame, area: Rect, app: &App, target: NodeId) {
    let message = app
        .close_confirmation_message(target)
        .unwrap_or_else(|| "Close this item?".to_string());
    modal::render_confirm(frame, area, "Close?", &message);
}

/// Draws the actionable right-click popup. It is intentionally opaque and
/// rendered after panels so no terminal content leaks through its commands.
fn draw_context_menu(frame: &mut Frame, menu: &ContextMenu) {
    let lines: Vec<Line> = menu
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let style = if index == menu.selected_index {
                Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::new()
            };
            Line::from(Span::styled(format!(" {}", action.label()), style))
        })
        .collect();
    let title = app_menu_title(menu);
    let widget = Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title(title)));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);
}

/// Uses the target identifier only as context; the menu labels make the
/// command's effect clear without duplicating potentially long tree names.
fn app_menu_title(_menu: &ContextMenu) -> &'static str {
    "Tree actions"
}

/// Group icon reused from `tree_ui`'s own folder glyph, so the picker's
/// destination list reads as literally the same visual language as the
/// tree panel it mirrors.
const GROUP_ICON: &str = "\u{1F4C1}";
/// Distinct icon for the "top level" entry -- it isn't a real group node in
/// the rendered tree, so it earns a glyph of its own rather than borrowing
/// the folder icon for something that isn't quite a folder.
const TOP_LEVEL_ICON: &str = "\u{2302}";
const GROUP_ACCENT: Color = Color::Rgb(0x7a, 0xa2, 0xf7);

/// Draws the "New group" destination picker: an always-editable name field
/// (optional -- left blank it defaults to "group") plus the flattened list
/// of every existing group, top level first, with the current selection
/// highlighted in the same accent used for the real tree's selected row.
fn draw_create_group(frame: &mut Frame, app: &App, state: &CreateGroupState) {
    frame.render_widget(Clear, state.area);
    let block = theme::block(true).title(theme::chrome_title("New group"));
    let layout = modal::create_group_layout(state.area);
    frame.render_widget(block, state.area);

    let name_label = Span::styled("Name  ", Style::new().add_modifier(Modifier::DIM));
    let name_value = if state.name.buf.is_empty() {
        Span::styled(
            "group",
            Style::new().add_modifier(Modifier::DIM | Modifier::ITALIC),
        )
    } else {
        Span::raw(state.name.buf.as_str())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![name_label, name_value])),
        layout.name_row,
    );
    // `saturating_add` throughout -- `state.name.cursor` grows with every
    // typed/pasted character and is otherwise unbounded, so a plain `+`
    // here could overflow `u16` (panic in debug, wrap to a bogus column in
    // release) before the `.min()` clamp below ever got a chance to run --
    // see the identical fix (and its rationale) in
    // `modal::render_text_prompt`.
    let cursor_x = layout
        .name_row
        .x
        .saturating_add(6)
        .saturating_add(u16::try_from(state.name.cursor).unwrap_or(u16::MAX));
    // `Rect::right()` is exclusive (the first column *outside* the rect), so
    // clamping to it directly would let the cursor land one cell past the
    // row's real last cell -- see the identical fix in
    // `modal::render_text_prompt` for the same `Rect::right()` pitfall.
    frame.set_cursor_position(Position::new(
        cursor_x.min(layout.name_row.right().saturating_sub(1)),
        layout.name_row.y,
    ));

    frame.render_widget(
        Paragraph::new(Span::styled(
            "Create under:",
            Style::new().add_modifier(Modifier::DIM | Modifier::UNDERLINED),
        )),
        layout.label_row,
    );

    let (start, end) = modal::create_group_visible_window(
        state.selected_index,
        state.destinations.len(),
        modal::CREATE_GROUP_MAX_VISIBLE,
    );
    let rows: Vec<Line> = state.destinations[start..end]
        .iter()
        .enumerate()
        .map(|(offset, destination)| {
            let index = start + offset;
            let is_top_level = destination.id == ROOT_ID;
            let indent = "  ".repeat(destination.depth.saturating_sub(1));
            let icon = if is_top_level {
                TOP_LEVEL_ICON
            } else {
                GROUP_ICON
            };
            let name = if is_top_level {
                "Top level".to_string()
            } else {
                app.tree
                    .get(destination.id)
                    .map(|node| node.name.clone())
                    .unwrap_or_else(|| "group".to_string())
            };
            let row_style = if index == state.selected_index {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else if is_top_level {
                Style::new().fg(GROUP_ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Line::from(Span::styled(format!(" {indent}{icon} {name}"), row_style))
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), layout.list_area);

    let scroll_hint = if state.destinations.len() > modal::CREATE_GROUP_MAX_VISIBLE {
        " · more above/below"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("\u{2191}\u{2193} choose · Enter/click create · Esc cancel{scroll_hint}"),
            Style::new().add_modifier(Modifier::DIM),
        )),
        layout.hint_row,
    );
}

/// Draws the focused pane's live content (terminal screen or editor
/// buffer), or a placeholder when nothing is focused.
fn draw_pane(frame: &mut Frame, area: Rect, app: &App) {
    let viewports = app.pane_viewports();
    if viewports.is_empty() {
        let (title, message) = match app.right_panel_target {
            RightPanelTarget::SplitView { split_id, .. } => (
                app.tree
                    .get(split_id)
                    .map(|node| node.name.as_str())
                    .unwrap_or("Split view"),
                "Split view is empty\nAdd up to four panes from the tree",
            ),
            _ => ("Terminal", "no pane selected"),
        };
        let placeholder =
            Paragraph::new(message).block(theme::block(false).title(theme::chrome_title(title)));
        frame.render_widget(placeholder, area);
        return;
    }

    for viewport in viewports {
        draw_pane_runtime(frame, app, viewport);
    }
}

fn draw_pane_runtime(frame: &mut Frame, app: &App, viewport: crate::split_layout::PaneViewport) {
    let pane_focused =
        matches!(app.focus, FocusTarget::Pane) && app.active_pane_id() == Some(viewport.pane_id);
    let pane_title = pane_title(app, viewport.pane_id);
    let Some(runtime) = app.panes.get(&viewport.pane_id) else {
        let placeholder = Paragraph::new("pane is loading")
            .block(theme::block(pane_focused).title(theme::chrome_title(&pane_title)));
        frame.render_widget(placeholder, viewport.outer_area);
        return;
    };

    match runtime {
        PaneRuntime::Terminal(term) => {
            term.with_screen(|screen| {
                let widget = PseudoTerminal::new(screen)
                    .block(theme::block(pane_focused).title(theme::chrome_title(&pane_title)));
                frame.render_widget(widget, viewport.outer_area);
            });
            draw_terminal_scrollbar(frame, viewport.outer_area, term.as_ref());
        }
        PaneRuntime::Editor(editor) => {
            let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
            let inner = block.inner(viewport.outer_area);
            frame.render_widget(block, viewport.outer_area);
            draw_editor(frame, inner, editor.as_ref());
        }
        PaneRuntime::Board(board) => {
            let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
            let inner = block.inner(viewport.outer_area);
            frame.render_widget(block, viewport.outer_area);
            draw_board(frame, inner, board.as_ref());
        }
    }
}

fn draw_board(frame: &mut Frame, area: Rect, board: &crate::board::BoardPane) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Block;
    if board.columns.is_empty() {
        frame.render_widget(
            Paragraph::new("No columns yet. Press c to create one."),
            area,
        );
        return;
    }
    let constraints = vec![Constraint::Ratio(1, board.columns.len() as u32); board.columns.len()];
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);
    for (index, (column, column_area)) in board.columns.iter().zip(columns.iter()).enumerate() {
        let is_selected_column = index == board.selected_column;
        let title_style = if is_selected_column {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        };
        let block = Block::bordered()
            .border_style(if is_selected_column {
                theme::border_style(true)
            } else {
                theme::border_style(false)
            })
            .title(Line::from(vec![
                Span::styled(format!(" {} ", column.title), title_style),
                Span::styled(
                    format!("{}", column.cards.len()),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            ]));
        let inner = block.inner(*column_area);
        frame.render_widget(block, *column_area);
        if column.cards.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "drop a card here",
                    Style::new().add_modifier(Modifier::DIM | Modifier::ITALIC),
                )),
                inner,
            );
            continue;
        }

        // Each card gets a compact, independent block instead of reading as
        // one uninterrupted paragraph. The blank row between cards gives the
        // dense terminal grid enough air without wasting a full column.
        let mut constraints = Vec::with_capacity(column.cards.len() * 2);
        for _ in &column.cards {
            constraints.push(Constraint::Length(3));
            constraints.push(Constraint::Length(1));
        }
        let card_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);
        for (card_index, card) in column.cards.iter().enumerate() {
            let is_selected_card = is_selected_column && card_index == board.selected_card;
            let card_style = if is_selected_card {
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            let card_block = Block::bordered()
                .border_style(if is_selected_card {
                    theme::border_style(true)
                } else {
                    theme::border_style(false)
                })
                .title(Span::styled(
                    if is_selected_card {
                        " selected "
                    } else {
                        " card "
                    },
                    card_style,
                ));
            frame.render_widget(
                Paragraph::new(card.title.as_str())
                    .block(card_block)
                    .wrap(ratatui::widgets::Wrap { trim: true }),
                card_areas[card_index * 2],
            );
        }
    }
    let hint = Rect::new(
        area.x.saturating_add(1),
        area.bottom().saturating_sub(1),
        area.width.saturating_sub(2),
        1,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "←/→ column · ↑/↓ card · n card · c column · e rename · d delete · Shift+←/→ move · drag cards",
            Style::new().add_modifier(Modifier::DIM),
        )),
        hint,
    );
}

/// Draws a vertical scrollbar merged into the terminal pane block's right
/// border, mirroring `tree_ui::draw_scrollbar` -- shown only once the pane
/// actually has scrollback history to navigate (`Ctrl+A` isn't involved:
/// see `App::handle_pane_key`/`handle_pane_mouse` for Shift+PageUp/
/// PageDown and wheel navigation).
fn draw_terminal_scrollbar(frame: &mut Frame, area: Rect, term: &terminal_view::TerminalView) {
    let total = term.scrollback_total();
    if total == 0 {
        return;
    }
    let mut scrollbar_state =
        ScrollbarState::new(total).position(total.saturating_sub(term.scrollback_position()));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(" "))
        .style(theme::border_style(false));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// Draws one editor pane's chrome (always-visible toolbar, main content,
/// optional minimap) into its content rect.
fn draw_editor(frame: &mut Frame, area: Rect, editor: &EditorPane) {
    let chrome = editor_chrome::compute(area, editor.show_minimap);
    editor_toolbar::render(frame, chrome.toolbar_area, editor);

    match editor.view_mode {
        EditorViewMode::Source => {
            editor.update_source_scroll_mirror(chrome.content_area.height);
            match editor.highlighted_lines() {
                Some(tokens) => {
                    editor.update_source_scroll_col_mirror(chrome.content_area.width);
                    editor_highlight::render(frame, chrome.content_area, editor, &tokens);
                }
                None => {
                    frame.render_widget(&editor.textarea, chrome.content_area);
                }
            }
            draw_source_scrollbar(frame, chrome.content_area, editor);
        }
        EditorViewMode::Rendered => match &editor.rendered {
            Some(document) => {
                markdown::view::render(
                    frame,
                    chrome.content_area,
                    document,
                    editor.rendered_scroll,
                );
                draw_rendered_scrollbar(frame, chrome.content_area, document, editor);
            }
            None => {
                frame.render_widget(Paragraph::new("Rendering…"), chrome.content_area);
            }
        },
    }

    if let Some(minimap_area) = chrome.minimap_area {
        let lines = editor.textarea.lines();
        let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
        let highlight_line = minimap_highlight_line(editor, lines.len(), chrome.content_area.width);
        minimap::render(
            frame,
            minimap_area,
            &borrowed,
            highlight_line,
            chrome.content_area.width,
        );
    }
}

/// Draws a Source-mode scrollbar only when the buffer extends beyond the
/// available editor body. It uses Ratatui's own scrollbar widget and shares
/// the same authoritative viewport position as wheel navigation.
fn draw_source_scrollbar(frame: &mut Frame, area: Rect, editor: &EditorPane) {
    let total_lines = editor.textarea.lines().len();
    if total_lines <= usize::from(area.height) {
        return;
    }
    let mut scrollbar_state =
        ScrollbarState::new(total_lines).position(usize::from(editor.source_scroll_row()));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(" "));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// Draws a vertical scrollbar for a Rendered-mode markdown document, only
/// once its content is actually taller than the visible area -- matches
/// `tree_ui::draw_scrollbar`'s same "don't show a full track for content
/// that already fits" rule.
fn draw_rendered_scrollbar(
    frame: &mut Frame,
    area: Rect,
    document: &markdown::render::RenderedDocument,
    editor: &EditorPane,
) {
    let total_height = markdown::view::content_height(document, area.width);
    if total_height <= area.height {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(usize::from(total_height))
        .position(usize::from(editor.rendered_scroll));
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(" "));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// The source line the minimap should mark as "you are here": the cursor
/// row in Source mode (known exactly), or the scroll-proportional line in
/// Rendered mode (the rendered view's scroll is measured in rendered
/// rows, not source lines, so this is an approximation across headers/
/// images whose rendered height differs from their one source line).
fn minimap_highlight_line(editor: &EditorPane, total_lines: usize, rendered_width: u16) -> usize {
    match editor.view_mode {
        EditorViewMode::Source => editor.textarea.cursor().0,
        EditorViewMode::Rendered => {
            let Some(document) = &editor.rendered else {
                return 0;
            };
            let total_height = markdown::view::content_height(document, rendered_width).max(1);
            let fraction = f64::from(editor.rendered_scroll) / f64::from(total_height);
            ((fraction * total_lines as f64).round() as usize).min(total_lines.saturating_sub(1))
        }
    }
}

/// Builds the selected right-panel title from logical pane naming plus
/// whatever agent class the render-cache tree currently knows for it.
///
/// Unlike the pre-client/server design, this never shows a real PID or
/// session ID: those are volatile OS facts the server discovers by
/// walking the pane's actual process tree, and `ilium_ipc::PaneStatus`
/// (the only agent information carried over the wire, via
/// `ServerEvent::PaneStatusChanged`) only carries `AgentClass` +
/// `AgentActivity` -- see `crate::naming_workers`'s module docs for the
/// matching gap on the session-title-inference side. Extending the wire
/// protocol to carry PID/session-id for display is a reasonable future
/// addition, not something this stage's scope covers.
fn pane_title(app: &App, id: NodeId) -> String {
    let Some(node) = app.tree.get(id) else {
        return "Terminal".to_string();
    };
    match &node.kind {
        NodeKind::Pane {
            status: PaneStatus::Agent(class, _),
            ..
        } => format!("{} — {}", node.name, agent_class_title(class)),
        _ => node.name.clone(),
    }
}

/// Compact, stable class name for the selected-terminal title.
fn agent_class_title(class: &AgentClass) -> &str {
    match class {
        AgentClass::Claude => "Claude",
        AgentClass::Codex => "Codex",
        AgentClass::Other(name) => name,
    }
}

/// Draws the one-line status bar: the current mode, plus any pending
/// status message. Rendered as a rounded pill -- inset by one column on
/// each side, with a powerline round-cap glyph closing off each end --
/// rather than a bar that runs flush into the screen's edges.
fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let mode_label = match &app.mode {
        Mode::Normal => "NORMAL".to_string(),
        Mode::LeaderPending => "LEADER (press a letter — ? for help)".to_string(),
        Mode::Move => "MOVE".to_string(),
        // The buffer itself is shown in the modal popup (see `draw`), not
        // here -- the status bar only names the mode while one is open.
        Mode::Rename(_) => "RENAME".to_string(),
        Mode::CommandPrompt(_) => "RUN COMMAND".to_string(),
        Mode::SaveAs(..) => "SAVE AS".to_string(),
        Mode::Help => "HELP".to_string(),
        Mode::Explorer(..) => "FILE PICKER".to_string(),
        Mode::ExplorerFileMenu(_) => "FILE ACTIONS".to_string(),
        Mode::FolderExplorer(..) => "FOLDER PICKER".to_string(),
        Mode::ContextMenu(..) => "TREE ACTIONS".to_string(),
        Mode::CreateGroup(_) => "NEW GROUP".to_string(),
        Mode::CreateBoard(_) => "NEW BOARD".to_string(),
        Mode::BoardPathPicker(_, _) => "BOARD PATH".to_string(),
        Mode::BoardCardPrompt(_, _) => "NEW CARD".to_string(),
        Mode::BoardColumnPrompt(_, _) => "NEW COLUMN".to_string(),
        Mode::BoardRenamePrompt(_, _, _) => "RENAME BOARD ITEM".to_string(),
        Mode::BoardDeleteConfirm(_, _) => "DELETE BOARD ITEM".to_string(),
        Mode::ConfirmClose(_) => "CONFIRM CLOSE".to_string(),
        // Unreachable in practice -- `draw` returns before this ever runs
        // while `Mode::Settings` is active (the settings view replaces the
        // whole screen, status bar included). Kept as a real arm rather
        // than a wildcard so this stays exhaustive if that early return is
        // ever removed.
        Mode::Settings(_) => "SETTINGS".to_string(),
    };

    let bar_style = theme::statusbar_style();
    let mut spans = vec![
        Span::raw("\u{2139} "),
        Span::styled(mode_label, bar_style.add_modifier(Modifier::BOLD)),
    ];
    if let Some(message) = &app.status_message {
        spans.push(Span::raw("  —  "));
        spans.push(Span::raw(message.clone()));
    }

    let cap_style = theme::statusbar_cap_style();
    let left_cap = Rect::new(area.x, area.y, 1, area.height);
    let right_cap = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );

    frame.render_widget(
        Paragraph::new(theme::STATUSBAR_CAP_LEFT).style(cap_style),
        left_cap,
    );
    frame.render_widget(Paragraph::new(Line::from(spans)).style(bar_style), inner);
    frame.render_widget(
        Paragraph::new(theme::STATUSBAR_CAP_RIGHT).style(cap_style),
        right_cap,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui_textarea::TextArea;

    #[test]
    fn source_scrollbar_renders_only_for_overflowing_buffers() {
        let mut editor = EditorPane::empty();
        editor.textarea = TextArea::from((0..10).map(|row| format!("line {row}")));

        let backend = TestBackend::new(12, 4);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| draw_source_scrollbar(frame, Rect::new(0, 0, 12, 4), &editor))
            .expect("render overflowing source scrollbar");
        let buffer = terminal.backend().buffer();
        assert!(
            (0..4).any(|row| buffer[(11, row)].symbol() != " "),
            "overflowing source should render a scrollbar thumb"
        );

        editor.textarea = TextArea::from(["short"]);
        terminal
            .draw(|frame| draw_source_scrollbar(frame, Rect::new(0, 0, 12, 4), &editor))
            .expect("render fitting source without scrollbar");
        let buffer = terminal.backend().buffer();
        assert!(
            (0..4).all(|row| buffer[(11, row)].symbol() == " "),
            "fitting source should not render a scrollbar"
        );
    }
}
