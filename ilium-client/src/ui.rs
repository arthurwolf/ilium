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
use crate::icon_settings::IconTarget;
use crate::scheduled_input::{ScheduledInputDialogState, ScheduledInputFocus};
use crate::{
    editor_chrome, editor_highlight, editor_toolbar, explorer_overlay, help, markdown, minimap,
    modal, search_ui, terminal_view, theme, tree_ui,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let layout = app.layout;

    draw_base_layer(frame, area, app);
    draw_voice_control(frame, layout.voice_control_area, app);

    // Every suspended parent draws before its child. This makes stack depth
    // a rendering concern rather than forcing child modes to clone or embed
    // the state of the screen they temporarily cover.
    for mode in &app.modal_stack {
        draw_mode_overlay(frame, area, app, mode);
    }
    draw_mode_overlay(frame, area, app, &app.mode);
}

/// Draws the one full-screen root behind every stacked overlay. Settings and
/// Search replace the ordinary workspace only when they are the oldest layer.
fn draw_base_layer(frame: &mut Frame, area: Rect, app: &mut App) {
    let root_mode = app.modal_stack.first().unwrap_or(&app.mode);
    if let Mode::Search(state) = root_mode {
        search_ui::render(frame, area, state, &app.ui_settings.icons);
        return;
    }
    if let Mode::Settings(state) = root_mode {
        crate::settings_ui::render(frame, area, app, state);
        return;
    }

    let layout = app.layout;
    // The pane renders before the tree because `PseudoTerminal` clears its
    // area; the tree then restores the connected shared-border joints.
    draw_pane(frame, layout.pane_area, app);
    let tree_focused = matches!(app.focus, FocusTarget::Tree);
    let focused_pane_id = app.focused_pane_id();
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
            terminal_activity_elapsed_ms: app.started_at.elapsed().as_millis(),
            current_unix_millis: crate::scheduled_input::unix_millis_now(),
            project_name: app.project_name.as_deref(),
            project_icon: app.project_icon.as_deref(),
            is_project_name_loading: app.is_project_name_loading,
            titles_loading: &app.titles_loading,
            recently_created: &app.recently_created,
            terminal_activity: &app.terminal_activity,
            focused_pane_id,
            transitions: &app.tree_transitions,
            agent_identifiers: &app.ui_settings.agent_identifiers,
            icons: &app.ui_settings.icons,
            tree_order: app.ui_settings.tree_order,
            sidebar_density: app.ui_settings.sidebar_density,
            use_stable_glyphs: app.ui_settings.use_stable_glyphs,
            show_inferred_title_icons: app.ui_settings.show_inferred_title_icons,
            hover: tree_ui::TreeHoverState {
                node: app.hovered_tree_node,
                toolbar_hovered: app.tree_toolbar_hovered,
                toolbar_action: app.hovered_tree_toolbar_action,
                show_management_actions: app.ui_settings.show_tree_row_management_controls,
            },
            panes: &app.panes,
        },
    );
    draw_status_bar(frame, layout.status_area, app);
}

/// Draws one non-root layer without deciding which layer owns input. Input
/// dispatch remains top-only through `App::mode`; this function is read-only.
fn draw_mode_overlay(frame: &mut Frame, area: Rect, app: &App, mode: &Mode) {
    match mode {
        Mode::Explorer(overlay, _)
        | Mode::FolderExplorer(overlay, _)
        | Mode::ProjectFolderExplorer(overlay, _)
        | Mode::BoardPathPicker(overlay) => {
            explorer_overlay::render(frame, area, overlay, std::time::SystemTime::now());
        }
        Mode::ExplorerFileMenu(menu) => {
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
            Line::from(format!(
                "{} Create board from {label}",
                context_menu_icon(&app.ui_settings, IconTarget::Board)
            )),
                ])
                .block(theme::block(true).title(theme::chrome_title("File actions"))),
                menu.area,
            );
        }
        Mode::Help => help::render(
            frame,
            area,
            app.keyboard_settings.shortcut_base,
            app.keyboard_settings.navigation_shortcut_base,
            &app.keybindings,
        ),
        Mode::ContextMenu(menu) => {
            draw_context_menu(frame, menu, app.ui_settings.tree_order, &app.ui_settings);
        }
        Mode::TerminalPaneContextMenu(menu) => draw_terminal_pane_context_menu(frame, menu, &app.ui_settings),
        Mode::AgentDebugLog(_) => {}
        Mode::AgentDebugSavePath(_, state) => {
            modal::render_text_prompt(frame, area, "Save agent debug log", state, "Save");
        }
        Mode::SchedulePaneInput(state) => draw_scheduled_input_dialog(frame, area, app, state),
        Mode::QueuePrompt(state) => draw_prompt_queue_dialog(frame, area, app, state),
        Mode::EditorLineContextMenu(menu) => draw_editor_line_context_menu(frame, menu, &app.ui_settings),
        Mode::CreateAgentFromLine(state) => draw_create_agent_from_line(frame, area, state),
        Mode::CreateGroup(state) => draw_create_group(frame, app, state),
        Mode::CreateSplitOrientation(state) => {
            draw_create_split_orientation(frame, area, state, &app.ui_settings.icons);
        }
        Mode::CreateSplitMembers(state) => draw_create_split_members(frame, area, state),
        Mode::CreateBoard(state) => draw_create_board(frame, area, state),
        Mode::BoardCardPrompt(_, state) => {
            modal::render_text_prompt(frame, area, "New card", state, "Create card");
        }
        Mode::BoardColumnPrompt(_, state) => {
            modal::render_text_prompt(frame, area, "New column", state, "Create column");
        }
        Mode::BoardRenamePrompt(_, target, state) => {
            let title = match target {
                BoardRenameTarget::Card => "Rename card",
                BoardRenameTarget::Column => "Rename column",
            };
            modal::render_text_prompt(frame, area, title, state, "Rename");
        }
        Mode::BoardDeleteConfirm(_, target) => {
            let (title, message) = match target {
                BoardDeleteTarget::Card => ("Delete card?", "Delete the selected card?"),
                BoardDeleteTarget::Column => {
                    ("Delete column?", "Delete the empty selected column?")
                }
            };
            modal::render_confirm(
                frame,
                area,
                title,
                message,
                modal::DialogActions::confirmation(
                    "Cancel",
                    modal::DialogButtonTone::Neutral,
                    "Delete",
                    modal::DialogButtonTone::Danger,
                ),
            );
        }
        Mode::Rename(state) => {
            modal::render_text_prompt(frame, area, "Rename", state, "Rename");
        }
        Mode::CommandPrompt(state) => {
            modal::render_text_prompt(frame, area, "Run command", state, "Run");
        }
        Mode::InferenceSettingPrompt(field, state) => {
            modal::render_text_prompt(frame, area, field.label(), state, "Apply");
        }
        Mode::VoiceSettingPrompt(field, state) => {
            modal::render_masked_text_prompt(frame, area, field.label(), state, "Replace");
        }
        Mode::ApiSettingPrompt(state) => {
            modal::render_text_prompt(frame, area, "HTTP API port", state, "Apply");
        }
        Mode::VoicePromptEditor(state) => modal::render_multiline_prompt(
            frame,
            area,
            "Voice control custom prompt",
            &state.textarea,
            "Apply",
        ),
        Mode::SaveAs(_, state) => {
            modal::render_text_prompt(frame, area, "Save As", state, "Save");
        }
        Mode::ConfirmClose(target) => draw_confirm_close(frame, area, app, *target),
        Mode::ConfirmSessionRecovery { pane_count } => modal::render_confirm(
            frame,
            area,
            "Restore previous session?",
            &format!("A stored snapshot contains {pane_count} pane(s). Restore it, or discard it and start fresh?"),
            modal::DialogActions::confirmation(
                "Discard",
                modal::DialogButtonTone::Danger,
                "Restore",
                modal::DialogButtonTone::Primary,
            ),
        ),
        Mode::Normal
        | Mode::LeaderPending
        | Mode::NavigationLeaderPending
        | Mode::Move
        | Mode::Settings(_)
        | Mode::Search(_) => {}
    }
}

fn draw_create_board(frame: &mut Frame, area: Rect, state: &CreateBoardState) {
    let layout = modal::create_board_dialog_layout(area);
    frame.render_widget(Clear, layout.popup);
    let block = theme::block(true).title(theme::chrome_title("New board"));
    frame.render_widget(block, layout.popup);
    draw_scheduled_input_field(
        frame,
        layout.name_box,
        "Board name",
        &state.name,
        !state.editing_path,
    );

    let selected_storage_style = Style::new()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let unselected_storage_style = Style::new().fg(Color::Cyan);
    let (folder_style, markdown_style) = match state.storage_kind {
        BoardStorageKind::Folder => (selected_storage_style, unselected_storage_style),
        BoardStorageKind::MarkdownFile => (unselected_storage_style, selected_storage_style),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Folder columns + Markdown cards ", folder_style),
            Span::raw("  "),
            Span::styled(" One Markdown file ", markdown_style),
        ]))
        .alignment(Alignment::Center),
        layout.storage_row,
    );

    draw_scheduled_input_field(
        frame,
        layout.path_box,
        "Storage path",
        &state.path,
        state.editing_path,
    );
    frame.render_widget(
        Paragraph::new("[ Browse path… ]")
            .style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        layout.browse_button,
    );
    modal::render_dialog_actions(
        frame,
        layout.actions,
        modal::DialogActions::form("Cancel", "Create board"),
    );
    frame.render_widget(
        Paragraph::new("Click a field to edit · Tab switches fields · Ctrl+P browses")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint_row,
    );
}

fn draw_create_split_orientation(
    frame: &mut Frame,
    area: Rect,
    state: &CreateSplitOrientationState,
    icons: &crate::icon_settings::IconSettings,
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
            Line::from(format!(
                "{vertical_marker} {}  Vertical — side by side",
                icons.glyph(IconTarget::SplitVertical)
            )),
            Line::from(format!(
                "{horizontal_marker} {}  Horizontal — stacked",
                icons.glyph(IconTarget::SplitHorizontal)
            )),
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
    modal::render_confirm(
        frame,
        area,
        "Close this item?",
        &message,
        modal::DialogActions::confirmation(
            "Keep open",
            modal::DialogButtonTone::Neutral,
            "Close",
            modal::DialogButtonTone::Danger,
        ),
    );
}

/// Draws the actionable right-click popup. It is intentionally opaque and
/// rendered after panels so no terminal content leaks through its commands.
fn draw_context_menu(
    frame: &mut Frame,
    menu: &ContextMenu,
    current_tree_order: crate::config::TreeOrder,
    ui: &crate::config::UiSettings,
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
            Line::from(Span::styled(
                format!(
                    "{} {}",
                    context_menu_icon(ui, action.icon_target()),
                    action.label()
                ),
                style,
            ))
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
                format!(
                    " {check}{} {}",
                    context_menu_icon(ui, IconTarget::TopLevel),
                    tree_order.label()
                ),
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

fn draw_prompt_queue_dialog(
    frame: &mut Frame,
    screen_area: Rect,
    app: &App,
    state: &crate::prompt_queue::PromptQueueDialogState,
) {
    use crate::prompt_queue::PromptQueueFocus;
    use ilium_core::PromptQueueDelivery;

    let layout = crate::prompt_queue::dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Queue prompt after agent finishes")),
        layout.popup,
    );
    let pane_name = app
        .tree
        .get(state.pane_id)
        .map_or("terminal", |node| node.name.as_str());
    frame.render_widget(
        Paragraph::new(format!(
            "The next detected bell for {pane_name} sends this prompt and Enter."
        ))
        .style(Style::new().add_modifier(Modifier::DIM)),
        layout.subtitle,
    );
    frame.render_widget(
        Paragraph::new("PROMPT").style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.prompt_label,
    );
    let text_style = if state.focus == PromptQueueFocus::Text {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(state.text.buf.clone())
            .block(
                theme::block(state.focus == PromptQueueFocus::Text)
                    .title(theme::chrome_title("Multiline prompt")),
            )
            .style(text_style)
            .wrap(ratatui::widgets::Wrap { trim: false }),
        layout.text,
    );
    frame.render_widget(
        Paragraph::new("DELIVERY").style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        layout.delivery_label,
    );
    let delivery_text = match state.delivery_choice {
        PromptQueueDelivery::Once => "[x] Once    [ ] Run X times    [ ] Enqueue forever (DANGER)",
        PromptQueueDelivery::Times { .. } => {
            "[ ] Once    [x] Run X times    [ ] Enqueue forever (DANGER)"
        }
        PromptQueueDelivery::Forever => {
            "[ ] Once    [ ] Run X times    [x] Enqueue forever (DANGER)"
        }
    };
    let delivery_style = if state.focus == PromptQueueFocus::Delivery {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    frame.render_widget(
        Paragraph::new(delivery_text).style(delivery_style),
        layout.delivery,
    );
    let times_value = match state.delivery_choice {
        PromptQueueDelivery::Times { .. } => state.times.buf.as_str(),
        _ => "(only for Run X times)",
    };
    let times_style = if state.focus == PromptQueueFocus::Times {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(format!("Runs: {times_value}"))
            .block(
                theme::block(state.focus == PromptQueueFocus::Times)
                    .title(theme::chrome_title("Repeat count")),
            )
            .style(times_style),
        layout.times,
    );
    let warning = match state.delivery_choice {
        PromptQueueDelivery::Forever => "DANGER: this will re-send forever whenever the agent finishes. Do not run it unmonitored.",
        PromptQueueDelivery::Times { .. } => "The same prompt is sent once per future finish, until the selected count is exhausted.",
        PromptQueueDelivery::Once => "The prompt remains queued until the agent next finishes, then is sent once.",
    };
    frame.render_widget(
        Paragraph::new(warning).style(Style::new().fg(
            if matches!(state.delivery_choice, PromptQueueDelivery::Forever) {
                Color::Red
            } else {
                Color::Yellow
            },
        )),
        layout.warning,
    );
    let button_style = if state.focus == PromptQueueFocus::EnqueueButton {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new("[ Enqueue ]")
            .style(button_style)
            .alignment(Alignment::Center),
        layout.enqueue_button,
    );
    frame.render_widget(
        Paragraph::new("Tab fields · ←/→ select delivery · Ctrl+Enter enqueue · Esc cancel")
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
fn draw_editor_line_context_menu(
    frame: &mut Frame,
    menu: &EditorLineContextMenu,
    ui: &crate::config::UiSettings,
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
            Line::from(Span::styled(
                format!(
                    "{} {}",
                    context_menu_icon(ui, action.icon_target()),
                    action.label()
                ),
                style,
            ))
        })
        .collect();
    let widget =
        Paragraph::new(lines).block(theme::block(true).title(theme::chrome_title("Line actions")));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);
}

/// Draws terminal copy actions without implying that a plain shell is an agent.
fn draw_terminal_pane_context_menu(
    frame: &mut Frame,
    menu: &crate::terminal_context_menu::TerminalPaneContextMenu,
    ui: &crate::config::UiSettings,
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
            Line::from(Span::styled(
                format!(
                    "{} {}",
                    context_menu_icon(ui, action.icon_target()),
                    action.label()
                ),
                style,
            ))
        })
        .collect();
    let widget = Paragraph::new(lines)
        .block(theme::block(true).title(theme::chrome_title("Terminal actions")));
    frame.render_widget(Clear, menu.area);
    frame.render_widget(widget, menu.area);
}

/// Keeps menu renderers text-only when the accessibility preference is off,
/// while every enabled menu entry receives its current configurable glyph.
fn context_menu_icon(ui: &crate::config::UiSettings, target: IconTarget) -> String {
    if ui.show_context_menu_icons {
        format!(" {}", ui.icons.glyph(target))
    } else {
        String::new()
    }
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
    // `state.name.cursor` (see `TextPromptState`) is a *char* index, not a
    // display column -- measuring the prefix's `UnicodeWidthStr::width()`
    // (like `draw_scheduled_input_field` does) rather than the raw char
    // count is required so a wide (CJK/emoji) name doesn't land the cursor
    // to the left of where the character actually renders.
    if layout.name_row.width > 0 {
        let prefix: String = state.name.buf.chars().take(state.name.cursor).collect();
        // `saturating_add` throughout -- an unbounded prefix width could
        // otherwise overflow `u16` (panic in debug, wrap to a bogus column
        // in release) before the `.min()` clamp below ever got a chance to
        // run -- see the identical fix (and its rationale) in
        // `modal::render_text_prompt`.
        let cursor_x = layout
            .name_row
            .x
            .saturating_add(6)
            .saturating_add(u16::try_from(prefix.width()).unwrap_or(u16::MAX));
        // `Rect::right()` is exclusive (the first column *outside* the rect),
        // so clamping to it directly would let the cursor land one cell past
        // the row's real last cell -- see the identical fix in
        // `modal::render_text_prompt` for the same `Rect::right()` pitfall.
        frame.set_cursor_position(Position::new(
            cursor_x.min(layout.name_row.right().saturating_sub(1)),
            layout.name_row.y,
        ));
    }

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
                app.ui_settings.icons.glyph(IconTarget::TopLevel)
            } else {
                app.ui_settings.icons.glyph(IconTarget::Group)
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

/// Draws the focused pane's live content (terminal screen or editor
/// buffer), or a placeholder when nothing is focused.
fn draw_pane(frame: &mut Frame, area: Rect, app: &App) {
    let root_mode = app.modal_stack.first().unwrap_or(&app.mode);
    if let Mode::AgentDebugLog(state) = root_mode {
        crate::agent_debug_ui::render(frame, area, app, state);
        return;
    }
    if let RightPanelTarget::Chatroom { project_id } = &app.right_panel_target {
        crate::chatroom_ui::render(frame, area, app, *project_id);
        return;
    }
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
    draw_screen_transfer_controls(frame, app);
}

/// Draws the two directional actions in the middle of every eligible split
/// separator. They are rendered after pane borders so the configured glyphs
/// remain directly clickable instead of being overwritten by chrome.
fn draw_screen_transfer_controls(frame: &mut Frame, app: &App) {
    for transfer in app.screen_transfers() {
        let icon_target = match transfer.direction {
            crate::split_layout::PaneDirection::Left => IconTarget::ScreenTransferLeft,
            crate::split_layout::PaneDirection::Right => IconTarget::ScreenTransferRight,
            crate::split_layout::PaneDirection::Up => IconTarget::ScreenTransferUp,
            crate::split_layout::PaneDirection::Down => IconTarget::ScreenTransferDown,
        };
        let glyph = app.ui_settings.icons.glyph(icon_target);
        let control = Paragraph::new(Span::styled(
            glyph,
            theme::selected_style().add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(control, transfer.control_area);
    }
}

fn draw_pane_runtime(frame: &mut Frame, app: &App, viewport: crate::split_layout::PaneViewport) {
    let pane_focused =
        matches!(app.focus, FocusTarget::Pane) && app.active_pane_id() == Some(viewport.pane_id);
    let pane_title = pane_title(app, viewport.pane_id);
    let completed_agent_close_action = app.completed_agent_close_action(viewport);
    let Some(runtime) = app.panes.get(&viewport.pane_id) else {
        let placeholder = Paragraph::new("pane is loading")
            .block(theme::block(pane_focused).title(theme::chrome_title(&pane_title)));
        frame.render_widget(placeholder, viewport.outer_area);
        if let Some(action) = completed_agent_close_action {
            draw_completed_agent_close_action(frame, action.button_area);
        }
        return;
    };

    match runtime {
        PaneRuntime::Terminal(term) => {
            if let Some(action) = completed_agent_close_action {
                let block = theme::block(pane_focused).title(theme::chrome_title(&pane_title));
                frame.render_widget(block, viewport.outer_area);
                term.with_screen(|screen| {
                    frame.render_widget(PseudoTerminal::new(screen), action.terminal_area);
                });
                // `action.terminal_area` is already inset inside the block's
                // border (it's carved out of `viewport.content_area` to leave
                // room for the close-agent button row), so a scrollbar drawn
                // against it would land one column inside the border, on top
                // of real terminal text, instead of merging into the border
                // like the ordinary (non-completed) branch below does --
                // `viewport.outer_area` is the area that actually lines up
                // with the border column.
                draw_terminal_scrollbar(frame, viewport.outer_area, term.as_ref());
            } else {
                term.with_screen(|screen| {
                    let widget = PseudoTerminal::new(screen)
                        .block(theme::block(pane_focused).title(theme::chrome_title(&pane_title)));
                    frame.render_widget(widget, viewport.outer_area);
                });
                draw_terminal_scrollbar(frame, viewport.outer_area, term.as_ref());
            }
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

    if let Some(action) = completed_agent_close_action {
        draw_completed_agent_close_action(frame, action.button_area);
    }
}

/// Renders the completed-agent acknowledgement as a full-width destructive
/// action, visually distinct from ordinary terminal output and chrome.
fn draw_completed_agent_close_action(frame: &mut Frame, area: Rect) {
    let style = Style::new()
        .fg(Color::White)
        .bg(Color::Red)
        .add_modifier(Modifier::BOLD);
    let button = Paragraph::new(Span::styled(
        crate::completed_agent_action::CLOSE_COMPLETED_AGENT_LABEL,
        style,
    ))
    .alignment(Alignment::Center)
    .style(style);
    frame.render_widget(button, area);
}

/// Draws a vertical scrollbar merged into the terminal pane block's right
/// border, mirroring `tree_ui::draw_scrollbar` -- shown only once the pane
/// actually has scrollback history to navigate (`Ctrl+A` isn't involved:
/// see `App::handle_pane_key`/`handle_pane_mouse` for Shift+PageUp/
/// PageDown, Shift+End, and wheel navigation).
fn draw_terminal_scrollbar(frame: &mut Frame, area: Rect, term: &terminal_view::TerminalView) {
    let total = term.scrollback_total();
    if total == 0 {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(total.saturating_add(1))
        .position(total.saturating_sub(term.scrollback_position()))
        .viewport_content_length(usize::from(term.viewport_rows()));
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
            editor
                .update_source_scroll_mirror(chrome.content_area.height, chrome.content_area.width);
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
                    editor.line_display,
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
        let highlight_line = minimap_highlight_line(editor, lines.len(), chrome.content_area.width);
        minimap::render(
            frame,
            minimap_area,
            lines,
            highlight_line,
            chrome.content_area.width,
        );
    }
}

/// Draws a Source-mode scrollbar only when the buffer extends beyond the
/// available editor body. It uses Ratatui's own scrollbar widget and shares
/// the same authoritative viewport position as wheel navigation.
fn draw_source_scrollbar(frame: &mut Frame, area: Rect, editor: &EditorPane) {
    let total_lines = editor.source_visual_rows(area.width).len();
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
    let total_height = markdown::view::content_height(document, area.width, editor.line_display);
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
            let total_height =
                markdown::view::content_height(document, rendered_width, editor.line_display)
                    .max(1);
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
        NodeKind::Pane { status, .. } => {
            let logical_title = match status {
                PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _) => {
                    format!("{} — {}", node.name, agent_class_title(class))
                }
                _ => node.name.clone(),
            };
            crate::pane_title::decorate_pane_title(status, &logical_title)
        }
        NodeKind::Container(_) | NodeKind::Folder { .. } => node.name.clone(),
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
    if area.is_empty() {
        return;
    }

    // Every arm here is a compile-time-constant label -- borrowing a
    // `&'static str` instead of building a fresh owned `String` avoids a
    // needless heap allocation on every single render frame (this function
    // runs on the redraw hot path, potentially many times per second).
    let mode_label: &'static str = match &app.mode {
        Mode::Normal => "NORMAL",
        Mode::LeaderPending => "LEADER (press a letter — ? for help)",
        Mode::NavigationLeaderPending => "TREE NAVIGATION (↓ ↑ Pg↓ Pg↑)",
        Mode::Move => "MOVE",
        // The buffer itself is shown in the modal popup (see `draw`), not
        // here -- the status bar only names the mode while one is open.
        Mode::Rename(_) => "RENAME",
        Mode::CommandPrompt(_) => "RUN COMMAND",
        Mode::InferenceSettingPrompt(_, _) => "INFERENCE SETTING",
        Mode::VoiceSettingPrompt(_, _) => "VOICE SETTING",
        Mode::ApiSettingPrompt(_) => "HTTP API PORT",
        Mode::VoicePromptEditor(_) => "VOICE PROMPT",
        Mode::SaveAs(..) => "SAVE AS",
        Mode::Help => "HELP",
        Mode::Explorer(..) => "FILE PICKER",
        Mode::ExplorerFileMenu(_) => "FILE ACTIONS",
        Mode::FolderExplorer(..) => "FOLDER PICKER",
        Mode::ProjectFolderExplorer(..) => "PROJECT FOLDER PICKER",
        Mode::ContextMenu(..) => "TREE ACTIONS",
        Mode::TerminalPaneContextMenu(..) => "TERMINAL ACTIONS",
        Mode::AgentDebugLog(..) => "AGENT DEBUG LOG",
        Mode::AgentDebugSavePath(..) => "SAVE AGENT DEBUG LOG",
        Mode::SchedulePaneInput(..) => "SCHEDULE INPUT",
        Mode::QueuePrompt(..) => "QUEUE PROMPT",
        Mode::EditorLineContextMenu(..) => "LINE ACTIONS",
        Mode::CreateAgentFromLine(..) => "CREATE AGENT",
        Mode::CreateGroup(_) => "NEW GROUP",
        Mode::CreateSplitOrientation(_) => "NEW SPLIT",
        Mode::CreateSplitMembers(_) => "SELECT SPLIT PANES",
        Mode::CreateBoard(_) => "NEW BOARD",
        Mode::BoardPathPicker(_) => "BOARD PATH",
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
            app.restructure_status_text()
                .unwrap_or_else(|| format!("{spinner} Restructuring projects with AI…")),
            bar_style.add_modifier(Modifier::BOLD),
        ));
    } else if let Some(status) = app.restructure_status_text() {
        spans.push(Span::raw("  —  "));
        spans.push(Span::raw(status));
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

/// Draws the persistent voice affordance to the right of the purple status
/// bar. The dot represents the persisted on/off state; the adjacent label
/// exposes the live transport state without turning provider details into UI
/// dependencies.
fn draw_voice_control(frame: &mut Frame, area: Rect, app: &App) {
    if area.is_empty() {
        return;
    }

    let is_enabled = app.voice_settings.enabled;
    let is_recording = matches!(
        app.voice_connection_state,
        ilium_voice::VoiceConnectionState::Recording
    );
    let dot_style = if is_enabled {
        let style = Style::new().fg(Color::Red);
        if is_recording {
            style.add_modifier(Modifier::BOLD | Modifier::RAPID_BLINK)
        } else {
            style.add_modifier(Modifier::BOLD)
        }
    } else {
        Style::new().fg(Color::Black)
    };
    let state_label = if !is_enabled {
        "OFF"
    } else {
        match &app.voice_connection_state {
            ilium_voice::VoiceConnectionState::Disabled => "STARTING",
            ilium_voice::VoiceConnectionState::Connecting => "CONNECTING",
            ilium_voice::VoiceConnectionState::Listening => "LISTENING",
            ilium_voice::VoiceConnectionState::Recording => "RECORDING",
            ilium_voice::VoiceConnectionState::Thinking => "THINKING",
            ilium_voice::VoiceConnectionState::Speaking => "SPEAKING",
            ilium_voice::VoiceConnectionState::Failed(_) => "FAILED",
        }
    };
    let shortcut = if matches!(
        app.voice_settings.input_mode,
        ilium_voice::VoiceInputMode::PushToTalk
    ) {
        "F8 hold"
    } else {
        "F8"
    };
    let content = Line::from(vec![
        Span::styled(" ● ", dot_style),
        Span::styled(
            format!("VOICE {state_label}"),
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {shortcut}"), Style::new().fg(Color::DarkGray)),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .alignment(Alignment::Center)
            .style(Style::new().bg(Color::Rgb(24, 24, 28))),
        area,
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
    fn terminal_scrollbar_thumb_tracks_the_full_viewport_from_top_to_tail() {
        let mut view = TerminalView::new(4, 20);
        for line in 0..10 {
            view.feed(format!("line {line}\r\n").as_bytes());
        }
        let mut terminal = Terminal::new(TestBackend::new(3, 10)).unwrap();

        terminal
            .draw(|frame| draw_terminal_scrollbar(frame, frame.area(), &view))
            .unwrap();
        let tail_thumb_rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(3)
            .enumerate()
            .filter_map(|(row, cells)| (cells[2].symbol() == "█").then_some(row))
            .collect::<Vec<_>>();
        assert!(tail_thumb_rows.len() > 1);
        assert_eq!(tail_thumb_rows.last().copied(), Some(9));

        view.scroll_up(u16::MAX);
        terminal
            .draw(|frame| draw_terminal_scrollbar(frame, frame.area(), &view))
            .unwrap();
        let top_thumb_rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(3)
            .enumerate()
            .filter_map(|(row, cells)| (cells[2].symbol() == "█").then_some(row))
            .collect::<Vec<_>>();
        assert_eq!(top_thumb_rows.first().copied(), Some(0));
    }

    #[test]
    fn right_panel_title_prefixes_done_without_mutating_the_pane_name() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "Review authentication", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(AgentClass::Codex, ilium_core::AgentActivity::Done),
            )
            .unwrap();

        assert_eq!(
            pane_title(&app, pane_id),
            "« [done] » Review authentication — Codex"
        );
        assert_eq!(app.tree.get(pane_id).unwrap().name, "Review authentication");

        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(AgentClass::Codex, ilium_core::AgentActivity::Working),
            )
            .unwrap();
        assert_eq!(pane_title(&app, pane_id), "Review authentication — Codex");
    }

    #[test]
    fn completed_agent_renders_a_full_width_red_close_action_at_the_panel_bottom() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "Completed review", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(AgentClass::Codex, ilium_core::AgentActivity::Done),
            )
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let action = app
            .completed_agent_close_action(app.pane_viewport(pane_id).unwrap())
            .expect("done agent should expose its close action");
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();

        let button_cells = terminal
            .backend()
            .buffer()
            .content()
            .chunks(120)
            .nth(usize::from(action.button_area.y))
            .expect("button row is inside the test backend")
            .get(usize::from(action.button_area.x)..usize::from(action.button_area.right()))
            .expect("button spans the complete inner panel row");
        assert!(button_cells.iter().all(|cell| cell.bg == Color::Red));
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Close finished agent"));
    }

    #[test]
    fn rapid_terminal_switches_never_mix_the_previous_pane_into_the_right_panel() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let first = app
            .tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        let mut first_view = TerminalView::new(24, 80);
        first_view.apply_replay(b"FIRST-PANE-ONLY\r\n", 1, true);
        let mut second_view = TerminalView::new(24, 80);
        second_view.apply_replay(b"SECOND-PANE-ONLY\r\n", 1, true);
        app.panes
            .insert(first, PaneRuntime::Terminal(Box::new(first_view)));
        app.panes
            .insert(second, PaneRuntime::Terminal(Box::new(second_view)));
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        for (pane_id, expected, forbidden) in [
            (first, "FIRST-PANE-ONLY", "SECOND-PANE-ONLY"),
            (second, "SECOND-PANE-ONLY", "FIRST-PANE-ONLY"),
            (first, "FIRST-PANE-ONLY", "SECOND-PANE-ONLY"),
            (second, "SECOND-PANE-ONLY", "FIRST-PANE-ONLY"),
        ] {
            app.focus_pane(pane_id);
            terminal
                .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
                .unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();

            assert!(rendered.contains(expected));
            assert!(
                !rendered.contains(forbidden),
                "the prior pane must be fully cleared in the same frame"
            );
        }
    }

    #[test]
    fn voice_control_dot_is_black_when_disabled_and_red_when_enabled() {
        let backend = TestBackend::new(22, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new("test".to_owned(), std::env::temp_dir());

        terminal
            .draw(|frame| draw_voice_control(frame, frame.area(), &app))
            .expect("render disabled voice control");
        let disabled_dot = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .find(|cell| cell.symbol() == "●")
            .expect("disabled dot");
        assert_eq!(disabled_dot.fg, Color::Black);

        app.voice_settings.enabled = true;
        app.voice_connection_state = ilium_voice::VoiceConnectionState::Recording;
        terminal
            .draw(|frame| draw_voice_control(frame, frame.area(), &app))
            .expect("render enabled voice control");
        let enabled_dot = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .find(|cell| cell.symbol() == "●")
            .expect("enabled dot");
        assert_eq!(enabled_dot.fg, Color::Red);
        assert!(enabled_dot.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn stacked_voice_credential_prompt_renders_over_live_settings() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.mode = Mode::Settings(crate::app::SettingsState {
            tab: crate::app::SettingsTab::VoiceControl,
            selected_row: 1,
            ..crate::app::SettingsState::default()
        });
        app.settings_adjust_voice_row(crate::voice_settings::VoiceRow::ApiKey, 1);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("⚙ Settings"));
        assert!(rendered.contains("Voice control"));
        assert!(rendered.contains("OpenAI API key"));
        assert!(rendered.contains("Protected value"));
        assert!(rendered.contains("Keep existing"));
        assert!(rendered.contains("Replace"));
    }

    #[test]
    fn agent_debug_mode_replaces_the_complete_right_panel_with_aerated_history() {
        let mut app = App::new("test".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "codex", PaneContentKind::Terminal)
            .unwrap();
        let mut terminal_view = TerminalView::new(24, 80);
        terminal_view.feed(b"NORMAL TERMINAL MUST BE HIDDEN");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(terminal_view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.mode = Mode::AgentDebugLog(crate::app::AgentDebugLogViewState {
            pane_id,
            scroll_position: crate::app::AgentDebugLogScrollPosition::FromNewest(0),
        });
        app.agent_debug_logs.insert(
            pane_id,
            crate::app::AgentDebugLogCache {
                through_sequence: 1,
                log: ilium_ipc::PaneDebugLog {
                    entries: vec![ilium_ipc::AgentDebugEntry {
                        sequence: 1,
                        occurred_at_unix_millis: 1_700_000_000_000,
                        severity: ilium_ipc::AgentDebugSeverity::Success,
                        source: ilium_ipc::AgentDebugSource::SessionDiscovery,
                        kind: ilium_ipc::AgentDebugEventKind::SessionResolved,
                        summary: "Project-verified agent session resolved".to_string(),
                        fields: vec![ilium_ipc::AgentDebugField::sensitive(
                            "session ID",
                            "session-123",
                        )],
                        correlation_id: None,
                        context: ilium_ipc::AgentDebugContext::default(),
                        metadata: Default::default(),
                    }],
                    ..Default::default()
                },
                ..crate::app::AgentDebugLogCache::default()
            },
        );
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains('🐞'));
        assert!(rendered.contains("Agent debug log"));
        assert!(rendered.contains("SESSION"));
        assert!(rendered.contains("Project-verified agent session resolved"));
        assert!(rendered.contains('🔒'));
        assert!(rendered.contains("session ID: session-123"));
        assert!(rendered.contains("← Back to agent"));
        assert!(rendered.contains("Save log"));
        assert!(rendered.contains("Panel resizes hidden"));
        assert!(!rendered.contains("NORMAL TERMINAL MUST BE HIDDEN"));
    }

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
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.ui_settings.tree_order = crate::config::TreeOrder::AgeDescending;
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(mut menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("context menu should be open");
        };
        app.open_context_tree_order_submenu(&mut menu);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        terminal
            .draw(|frame| {
                draw_context_menu(
                    frame,
                    &menu,
                    crate::config::TreeOrder::AgeDescending,
                    &app.ui_settings,
                )
            })
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        // A configurable icon renders between the check mark and the label, so
        // the expected row is composed through the same helper the renderer
        // uses; pinning the literal pair would break whenever that icon
        // changes or the user turns context-menu icons off entirely.
        let active_row = format!(
            "✓{} Age down (oldest first)",
            context_menu_icon(&app.ui_settings, IconTarget::TopLevel)
        );
        assert!(rendered.contains(&active_row));
        assert_eq!(rendered.matches('✓').count(), 1);
    }

    #[test]
    fn context_menu_icons_follow_the_live_ui_preference() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.open_context_menu(ROOT_ID, 2, 2);
        let Mode::ContextMenu(menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("context menu should open");
        };
        let render = |ui: &crate::config::UiSettings| {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal
                .draw(|frame| draw_context_menu(frame, &menu, crate::config::TreeOrder::Manual, ui))
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };

        let with_icons = render(&app.ui_settings);
        assert!(with_icons.contains(app.ui_settings.icons.glyph(IconTarget::ToolbarSearch)));

        app.ui_settings.show_context_menu_icons = false;
        let text_only = render(&app.ui_settings);
        assert!(!text_only.contains(app.ui_settings.icons.glyph(IconTarget::ToolbarSearch)));
        assert!(text_only.contains("Search workspace…"));
    }

    #[test]
    fn scheduled_input_dialog_renders_all_aerated_controls() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
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
        let mut app = App::new("test".to_string(), std::env::temp_dir());
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

    #[test]
    fn split_view_renders_configured_directional_screen_transfer_controls() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let left = app
            .tree
            .add_pane(group, "left", PaneContentKind::Terminal)
            .unwrap();
        let right = app
            .tree
            .add_pane(group, "right", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[left, right],
            )
            .unwrap();
        app.panes.insert(
            left,
            PaneRuntime::Terminal(Box::new(TerminalView::new(20, 30))),
        );
        app.panes.insert(
            right,
            PaneRuntime::Terminal(Box::new(TerminalView::new(20, 30))),
        );
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split,
            active_pane_id: Some(left),
        };
        app.ui_settings
            .icons
            .set(IconTarget::ScreenTransferRight, "⇢".to_string());
        app.ui_settings
            .icons
            .set(IconTarget::ScreenTransferLeft, "⇠".to_string());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let rightward = app
            .screen_transfers()
            .into_iter()
            .find(|transfer| {
                transfer.source_pane_id == left
                    && transfer.destination_pane_id == right
                    && transfer.direction == crate::split_layout::PaneDirection::Right
            })
            .unwrap();
        let leftward = app
            .screen_transfers()
            .into_iter()
            .find(|transfer| {
                transfer.source_pane_id == right
                    && transfer.destination_pane_id == left
                    && transfer.direction == crate::split_layout::PaneDirection::Left
            })
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        terminal
            .draw(|frame| draw_pane(frame, app.layout.pane_area, &app))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(rightward.control_area.x, rightward.control_area.y)].symbol(),
            "⇢"
        );
        assert_eq!(
            buffer[(leftward.control_area.x, leftward.control_area.y)].symbol(),
            "⇠"
        );
    }
}
