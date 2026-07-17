//! Top-level layout: a left tree column, the focused pane's content on
//! the right, and a bottom status bar -- with the Explorer file-picker or
//! Help reference drawn as an overlay on top of everything else when
//! active. It consumes the shared animated `App::layout`; everything it
//! draws is delegated to `tree_ui`, `help`, or the pane runtimes themselves.

use ilium_core::{AgentClass, AgentProvider, NodeId, NodeKind, PaneStatus, ROOT_ID};
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;
use unicode_width::UnicodeWidthStr;

use crate::agent_from_line::{
    AgentLaunchType, CreateAgentFocus, CreateAgentFromLineState, EditorLineContextMenu,
};
use crate::app::{
    App, BoardDeleteTarget, BoardRenameTarget, BoardStorageKind, ContextMenu, CreateBoardState,
    CreateGroupState, CreateSplitMembersState, CreateSplitOrientationState, FocusTarget, Mode,
    PaneRuntime, RightPanelTarget,
};
use crate::editor_pane::{EditorPane, EditorViewMode};
use crate::scheduled_input::{ScheduledInputDialogState, ScheduledInputFocus};
use crate::{
    editor_chrome, editor_highlight, editor_toolbar, explorer_overlay, help, markdown, minimap,
    modal, search_ui, terminal_view, theme, tree_ui,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let layout = app.layout;

    if let Mode::Search(state) = &app.mode {
        search_ui::render(frame, area, state);
        return;
    }

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
    let editor_paths = editor_pane_paths(&app.panes);
    tree_ui::render(
        frame,
        layout.tree_area,
        &app.tree,
        &mut app.tree_state,
        tree_ui::TreeRenderOptions {
            focused: tree_focused,
            elapsed_ms: if matches!(
                app.ui_settings.motion_level,
                crate::config::MotionLevel::Off
            ) {
                0
            } else {
                app.started_at.elapsed().as_millis()
            },
            current_unix_millis: crate::scheduled_input::unix_millis_now(),
            project_name: app.project_name.as_deref(),
            is_project_name_loading: app.is_project_name_loading,
            titles_loading: &app.titles_loading,
            recently_created: &app.recently_created,
            transitions: &app.tree_transitions,
            agent_identifiers: &app.ui_settings.agent_identifiers,
            tree_order: app.ui_settings.tree_order,
            sidebar_density: app.ui_settings.sidebar_density,
            hover: tree_ui::TreeHoverState {
                node: app.hovered_tree_node,
                toolbar_hovered: app.tree_toolbar_hovered,
                toolbar_action: app.hovered_tree_toolbar_action,
            },
            editor_paths: &editor_paths,
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
        help::render(frame, area, app.keyboard_settings.shortcut_base);
    }
    if let Mode::ContextMenu(menu) = &app.mode {
        draw_context_menu(frame, menu, app.ui_settings.tree_order);
    }
    if let Mode::SchedulePaneInput(state) = &app.mode {
        draw_scheduled_input_dialog(frame, area, app, state);
    }
    if let Mode::EditorLineContextMenu(menu) = &app.mode {
        draw_editor_line_context_menu(frame, menu);
    }
    if let Mode::CreateAgentFromLine(state) = &app.mode {
        draw_create_agent_from_line(frame, area, state);
    }
    if let Mode::CreateGroup(state) = &app.mode {
        draw_create_group(frame, app, state);
    }
    if let Mode::CreateSplitOrientation(state) = &app.mode {
        draw_create_split_orientation(frame, area, state);
    }
    if let Mode::CreateSplitMembers(state) = &app.mode {
        draw_create_split_members(frame, area, state);
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
    if let Mode::InferenceSettingPrompt(field, state) = &app.mode {
        modal::render_text_prompt(frame, area, field.label(), state);
    }
    if let Mode::SaveAs(_, state) = &app.mode {
        modal::render_text_prompt(frame, area, "Save As", state);
    }
    if let Mode::ConfirmClose(target) = &app.mode {
        draw_confirm_close(frame, area, app, *target);
    }
    if let Mode::ConfirmSessionRecovery { pane_count } = &app.mode {
        modal::render_confirm(
            frame,
            area,
            "Restore previous session?",
            &format!("Restore {pane_count} pane(s) from the stored snapshot?  Enter/Y: restore · N/Esc: start fresh"),
        );
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
            "Tab field · Ctrl+P browse · Enter create · Esc cancel",
            Style::new().add_modifier(Modifier::DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_create_split_orientation(
    frame: &mut Frame,
    area: Rect,
    state: &CreateSplitOrientationState,
) {
    let popup = modal::create_split_orientation_dialog_area(area);
    frame.render_widget(Clear, popup);
    let block = theme::block(true).title(theme::chrome_title("New split view"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let vertical_marker = if state.orientation == ilium_core::SplitOrientation::Vertical {
        "›"
    } else {
        " "
    };
    let horizontal_marker = if state.orientation == ilium_core::SplitOrientation::Horizontal {
        "›"
    } else {
        " "
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Choose how two or three panes are arranged:"),
            Line::from(""),
            Line::from(format!("{vertical_marker} ▥  Vertical — side by side")),
            Line::from(format!("{horizontal_marker} ▤  Horizontal — stacked")),
            Line::from(""),
            Line::from(Span::styled(
                "E  Create empty with this orientation",
                Style::new().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "←/→ choose · Enter choose panes · E create empty · Esc cancel",
                Style::new().add_modifier(Modifier::DIM),
            )),
        ]),
        inner,
    );
}

fn draw_create_split_members(frame: &mut Frame, area: Rect, state: &CreateSplitMembersState) {
    let popup = modal::create_split_members_dialog_area(area, state.choices.len());
    frame.render_widget(Clear, popup);
    let block = theme::block(true).title(theme::chrome_title("Add panes to split"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let (start, end) =
        modal::create_split_member_visible_window(state.selected_index, state.choices.len());
    let selected_count = state
        .choices
        .iter()
        .filter(|choice| choice.selected)
        .count();
    let mut lines = vec![
        Line::from(format!(
            "Select up to four panes ({selected_count}/4). Selecting none creates an empty split."
        )),
        Line::from(""),
    ];
    if state.choices.is_empty() {
        lines.push(Line::from(Span::styled(
            "No eligible panes; all existing panes are already in split views.",
            Style::new().add_modifier(Modifier::DIM),
        )));
    } else {
        for (index, choice) in state.choices[start..end].iter().enumerate() {
            let absolute_index = start + index;
            let marker = if absolute_index == state.selected_index {
                "›"
            } else {
                " "
            };
            let checkbox = if choice.selected { "[x]" } else { "[ ]" };
            let style = if absolute_index == state.selected_index {
                theme::selected_style()
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {checkbox} {}", choice.label),
                style,
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ navigate · Space toggle · Enter create · Esc cancel",
        Style::new().add_modifier(Modifier::DIM),
    )));
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
fn draw_context_menu(
    frame: &mut Frame,
    menu: &ContextMenu,
    current_tree_order: crate::config::TreeOrder,
) {
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

    let Some(submenu) = &menu.tree_order_submenu else {
        return;
    };
    let lines: Vec<Line> = crate::config::TreeOrder::ALL
        .iter()
        .enumerate()
        .map(|(index, tree_order)| {
            let style = if index == submenu.selected_index {
                Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::new()
            };
            let check = if *tree_order == current_tree_order {
                "✓"
            } else {
                " "
            };
            Line::from(Span::styled(
                format!(" {check} {}", tree_order.label()),
                style,
            ))
        })
        .collect();
    let widget =
        Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title("Order by")));
    frame.render_widget(Clear, submenu.area);
    frame.render_widget(widget, submenu.area);
}

/// Renders a deliberately spacious form: duration first, then payload, then
/// the Enter policy and one explicit confirmation button. The same geometry
/// drives `crate::mouse`, so every visible control has an exact hit target.
fn draw_scheduled_input_dialog(
    frame: &mut Frame,
    screen_area: Rect,
    app: &App,
    state: &ScheduledInputDialogState,
) {
    let layout = crate::scheduled_input::dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Hit key(s) X time from now")),
        layout.popup,
    );
    let pane_name = app
        .tree
        .get(state.pane_id)
        .map_or("terminal", |node| node.name.as_str());
    frame.render_widget(
        Paragraph::new(format!("Schedule input for {pane_name}"))
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.subtitle,
    );
    frame.render_widget(
        Paragraph::new("WHEN").style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.duration_label,
    );
    draw_scheduled_input_field(
        frame,
        layout.hours,
        "Hours",
        &state.hours,
        state.focus == ScheduledInputFocus::Hours,
    );
    draw_scheduled_input_field(
        frame,
        layout.minutes,
        "Minutes",
        &state.minutes,
        state.focus == ScheduledInputFocus::Minutes,
    );
    draw_scheduled_input_field(
        frame,
        layout.seconds,
        "Seconds",
        &state.seconds,
        state.focus == ScheduledInputFocus::Seconds,
    );
    frame.render_widget(
        Paragraph::new("WHAT TO HIT")
            .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.payload_label,
    );
    draw_scheduled_input_field(
        frame,
        layout.text,
        "Text (optional)",
        &state.text,
        state.focus == ScheduledInputFocus::Text,
    );

    let checkbox_style = if state.focus == ScheduledInputFocus::SendEnter {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    let checkbox = if state.send_enter { "[x]" } else { "[ ]" };
    frame.render_widget(
        Paragraph::new(format!("{checkbox} Send Enter after the text")).style(checkbox_style),
        layout.send_enter,
    );

    let button_style = if state.focus == ScheduledInputFocus::ScheduleButton {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new("[ Schedule input ]")
            .style(button_style)
            .alignment(Alignment::Center),
        layout.schedule_button,
    );
    frame.render_widget(
        Paragraph::new("Tab field · Space toggle · Ctrl+Enter schedule · Esc cancel")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint,
    );
}

fn draw_scheduled_input_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    state: &crate::text_prompt::TextPromptState,
    focused: bool,
) {
    let border_style = if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let block = theme::block(focused)
        .title(theme::chrome_title(label))
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(state.buf.as_str()), inner);
    if !focused || inner.width == 0 || inner.height == 0 {
        return;
    }
    let prefix: String = state.buf.chars().take(state.cursor).collect();
    let cursor_offset = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(
        inner
            .x
            .saturating_add(cursor_offset)
            .min(inner.right().saturating_sub(1)),
        inner.y,
    ));
}

/// Draws the line-specific right-click action without implying that its file
/// target is the currently selected tree node.
fn draw_editor_line_context_menu(frame: &mut Frame, menu: &EditorLineContextMenu) {
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
    let widget =
        Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title("Line actions")));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);
}

/// Draws the agent selector, editable multi-line prompt, and explicit submit
/// button using geometry shared with `crate::mouse`.
fn draw_create_agent_from_line(
    frame: &mut Frame,
    screen_area: Rect,
    state: &CreateAgentFromLineState,
) {
    let layout = crate::agent_from_line::dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Create agent from line")),
        layout.popup,
    );

    let selector_style = if state.focus == CreateAgentFocus::AgentType {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    let agent_option = |agent_type: AgentLaunchType| {
        let marker = if state.agent_type == agent_type {
            "(●)"
        } else {
            "( )"
        };
        Span::styled(
            format!("{marker} {}", agent_type.label()),
            if state.agent_type == agent_type {
                selector_style.add_modifier(Modifier::BOLD)
            } else {
                selector_style
            },
        )
    };
    let mut agent_spans = vec![Span::styled(
        "Agent: ",
        Style::new().add_modifier(Modifier::DIM),
    )];
    for (index, agent_type) in AgentLaunchType::ALL.into_iter().enumerate() {
        if index > 0 {
            agent_spans.push(Span::raw("   "));
        }
        agent_spans.push(agent_option(agent_type));
    }
    frame.render_widget(Paragraph::new(Line::from(agent_spans)), layout.agent_row);

    let prompt_border_style = if state.focus == CreateAgentFocus::Prompt {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let prompt_block = theme::block(state.focus == CreateAgentFocus::Prompt)
        .title(theme::chrome_title("Task prompt"))
        .border_style(prompt_border_style);
    let prompt_inner = prompt_block.inner(layout.prompt_area);
    frame.render_widget(prompt_block, layout.prompt_area);
    frame.render_widget(&state.prompt, prompt_inner);

    let button_style = if state.focus == CreateAgentFocus::CreateButton {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new(Span::styled("[ Create agent ]", button_style)).alignment(Alignment::Center),
        layout.create_button,
    );
    frame.render_widget(
        Paragraph::new("Tab field · Enter newline · Ctrl+Enter create · Esc cancel")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint_row,
    );
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
            // Build name via format! to avoid cloning node.name — format! reads
            // the source and produces a new owned String only once needed.
            let name_str = if is_top_level {
                "Top level"
            } else {
                app.tree
                    .get(destination.id)
                    .map(|node| node.name.as_str())
                    .unwrap_or("group")
            };
            let row_style = if index == state.selected_index {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else if is_top_level {
                Style::new().fg(GROUP_ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            Line::from(Span::styled(
                format!(" {indent}{icon} {name_str}"),
                row_style,
            ))
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

/// Every currently-open editor pane's backing file path, keyed by pane id --
/// the tree panel's only view into `EditorPane.path`, which otherwise lives
/// entirely in client-runtime state rather than the (server-mirrored) tree.
/// See `tree_ui::TreeRenderOptions::editor_paths`.
fn editor_pane_paths(
    panes: &std::collections::HashMap<NodeId, PaneRuntime>,
) -> std::collections::HashMap<NodeId, std::path::PathBuf> {
    panes
        .iter()
        .filter_map(|(pane_id, runtime)| match runtime {
            PaneRuntime::Editor(editor) => editor.path.clone().map(|path| (*pane_id, path)),
            _ => None,
        })
        .collect()
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
            crate::board_ui::render(
                frame,
                inner,
                board.as_ref(),
                app.kanban_board_settings.card_preview_lines,
                app.kanban_board_settings.minimum_column_width,
            );
        }
    }
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
            status: PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _),
            ..
        } => format!("{} — {}", node.name, agent_class_title(class)),
        _ => node.name.clone(),
    }
}

/// Compact, stable class name for the selected-terminal title.
fn agent_class_title(class: &AgentClass) -> &str {
    class.label()
}

/// Draws the one-line status bar: the current mode, plus any pending
/// status message. Rendered as a rounded pill -- inset by one column on
/// each side, with a powerline round-cap glyph closing off each end --
/// rather than a bar that runs flush into the screen's edges.
fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    // Every arm here is a compile-time-constant label -- borrowing a
    // `&'static str` instead of building a fresh owned `String` avoids a
    // needless heap allocation on every single render frame (this function
    // runs on the redraw hot path, potentially many times per second).
    let mode_label: &'static str = match &app.mode {
        Mode::Normal => "NORMAL",
        Mode::LeaderPending => "LEADER (press a letter — ? for help)",
        Mode::Move => "MOVE",
        // The buffer itself is shown in the modal popup (see `draw`), not
        // here -- the status bar only names the mode while one is open.
        Mode::Rename(_) => "RENAME",
        Mode::CommandPrompt(_) => "RUN COMMAND",
        Mode::InferenceSettingPrompt(_, _) => "INFERENCE SETTING",
        Mode::SaveAs(..) => "SAVE AS",
        Mode::Help => "HELP",
        Mode::Explorer(..) => "FILE PICKER",
        Mode::ExplorerFileMenu(_) => "FILE ACTIONS",
        Mode::FolderExplorer(..) => "FOLDER PICKER",
        Mode::ContextMenu(..) => "TREE ACTIONS",
        Mode::SchedulePaneInput(..) => "SCHEDULE INPUT",
        Mode::EditorLineContextMenu(..) => "LINE ACTIONS",
        Mode::CreateAgentFromLine(..) => "CREATE AGENT",
        Mode::CreateGroup(_) => "NEW GROUP",
        Mode::CreateSplitOrientation(_) => "NEW SPLIT",
        Mode::CreateSplitMembers(_) => "SELECT SPLIT PANES",
        Mode::CreateBoard(_) => "NEW BOARD",
        Mode::BoardPathPicker(_, _) => "BOARD PATH",
        Mode::BoardCardPrompt(_, _) => "NEW CARD",
        Mode::BoardColumnPrompt(_, _) => "NEW COLUMN",
        Mode::BoardRenamePrompt(_, _, _) => "RENAME BOARD ITEM",
        Mode::BoardDeleteConfirm(_, _) => "DELETE BOARD ITEM",
        Mode::ConfirmClose(_) => "CONFIRM CLOSE",
        Mode::ConfirmSessionRecovery { .. } => "SESSION RECOVERY",
        Mode::Search(_) => "SEARCH",
        // Unreachable in practice -- `draw` returns before this ever runs
        // while `Mode::Settings` is active (the settings view replaces the
        // whole screen, status bar included). Kept as a real arm rather
        // than a wildcard so this stays exhaustive if that early return is
        // ever removed.
        Mode::Settings(_) => "SETTINGS",
    };

    let bar_style = theme::statusbar_style();
    let mut spans = vec![
        Span::raw("\u{2139} "),
        Span::styled(mode_label, bar_style.add_modifier(Modifier::BOLD)),
    ];
    if app.structure_loading {
        let elapsed_ms = app.started_at.elapsed().as_millis();
        let frame_index =
            (elapsed_ms / tree_ui::SPINNER_FRAME_MS) as usize % tree_ui::SPINNER_FRAMES.len();
        let spinner = tree_ui::SPINNER_FRAMES[frame_index];
        spans.push(Span::raw("  —  "));
        spans.push(Span::styled(
            format!("{spinner} Restructuring workspace with AI…"),
            bar_style.add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(message) = &app.status_message {
        spans.push(Span::raw("  —  "));
        // Borrow rather than `message.clone()` -- `app` outlives this
        // function's local `spans`, so there is no need to allocate a new
        // `String` copy of the status message on every render frame.
        spans.push(Span::raw(message.as_str()));
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
    use crate::app::{PaneRuntime, RightPanelTarget};
    use crate::terminal_view::TerminalView;
    use ilium_core::{PaneContentKind, SplitOrientation};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui_textarea::TextArea;
    use std::path::PathBuf;

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

    #[test]
    fn create_agent_dialog_renders_selector_editable_prompt_and_button() {
        let state = CreateAgentFromLineState::new(
            crate::agent_from_line::EditorSourceLine {
                pane_id: NodeId(2),
                path: PathBuf::from("/work/main.rs"),
                line_number: 9,
                text: "finish_feature();".to_string(),
            },
            NodeId(1),
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal
            .draw(|frame| draw_create_agent_from_line(frame, frame.area(), &state))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Create agent from line"));
        assert!(rendered.contains("Claude"));
        assert!(rendered.contains("Codex"));
        assert!(rendered.contains("/goal please do the following task"));
        assert!(rendered.contains("[ Create agent ]"));
    }

    #[test]
    fn tree_order_submenu_renders_one_check_before_the_active_option() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.ui_settings.tree_order = crate::config::TreeOrder::AgeDescending;
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("context menu should be open");
        };
        app.open_context_tree_order_submenu(&mut menu);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal
            .draw(|frame| draw_context_menu(frame, &menu, crate::config::TreeOrder::AgeDescending))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("✓ Age down (oldest first)"));
        assert_eq!(rendered.matches('✓').count(), 1);
    }

    #[test]
    fn scheduled_input_dialog_renders_all_aerated_controls() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "release shell", PaneContentKind::Terminal)
            .unwrap();
        app.mode = Mode::SchedulePaneInput(Box::new(
            crate::scheduled_input::ScheduledInputDialogState::new(pane_id),
        ));
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Hit key(s) X time from now"));
        assert!(rendered.contains("Schedule input for release shell"));
        assert!(rendered.contains("Hours"));
        assert!(rendered.contains("Minutes"));
        assert!(rendered.contains("Seconds"));
        assert!(rendered.contains("Text (optional)"));
        assert!(rendered.contains("[x] Send Enter after the text"));
        assert!(rendered.contains("[ Schedule input ]"));
    }

    #[test]
    fn split_view_renders_both_terminal_members_and_active_slot_chrome() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[first, second],
            )
            .unwrap();
        let mut first_view = TerminalView::new(20, 30);
        first_view.feed(b"LEFT-PANE");
        let mut second_view = TerminalView::new(20, 30);
        second_view.feed(b"RIGHT-PANE");
        app.panes
            .insert(first, PaneRuntime::Terminal(Box::new(first_view)));
        app.panes
            .insert(second, PaneRuntime::Terminal(Box::new(second_view)));
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(second),
        };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("LEFT-PANE"));
        assert!(rendered.contains("RIGHT-PANE"));
        assert!(rendered.contains("first"));
        assert!(rendered.contains("second"));
    }
}
