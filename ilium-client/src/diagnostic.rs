//! A full-text escape hatch for diagnostics in compact UI surfaces. This
//! overlay retains the underlying mode and drafts and never truncates text.
use crate::app::{App, Mode};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::{
    layout::Rect,
    widgets::{Clear, Paragraph},
    Frame,
};

#[derive(Debug)]
pub(crate) struct DiagnosticView {
    text: String,
    scroll: usize,
    /// Screen cells of the "Copy" button from the last render, for hit-testing.
    copy_button: Option<Rect>,
}

const COPY_BUTTON_LABEL: &str = "[ Copy ]";

pub(crate) fn current_text(app: &App) -> Option<String> {
    use crate::value_dialog::ValueDialogState;
    let modal = match &app.mode {
        Mode::LocationPicker(state) => state.diagnostic_text().map(str::to_owned),
        Mode::Settings(state) => state
            .plugin_editor
            .as_ref()
            .and_then(|editor| editor.error.clone()),
        Mode::TextTriggerDialog(state) => state.save_error.clone(),
        Mode::Explorer(state, _)
        | Mode::FolderExplorer(state, _)
        | Mode::ProjectFolderExplorer(state, _)
        | Mode::BoardPathPicker(state) => state.diagnostic_text().map(str::to_owned),
        Mode::AgentMessageDialog(state) => state.error.clone(),
        Mode::ValueDialog(host) => match &host.dialog {
            ValueDialogState::Choice(state) => state.notice.clone(),
            ValueDialogState::Number(state) => state.error.clone(),
        },
        Mode::CreateAgentWorkspace(state) => Some(state.status_text()),
        Mode::WorktreeManager(state) => Some(state.detail_text()),
        Mode::AnimationTextPrompt(target, _) => target.error.clone(),
        _ => None,
    };
    if let Some(text) = app
        .plugin_permission_session
        .as_ref()
        .and_then(|state| state.diagnostic_text())
    {
        return Some(match &app.status_message {
            Some(status) => format!("{text}\n\n{status}"),
            None => text.to_owned(),
        });
    }
    if modal.is_some() {
        return modal;
    }
    if let Some(state) = &app.remote_compaction {
        if let crate::remote_compaction_dialog::RemoteCompactionPhase::Failed(error) = &state.phase
        {
            return Some(error.clone());
        }
    }
    if let Some(popover) = &app.agent_popover {
        if !popover.enabled {
            return popover.unavailable_reason.clone();
        }
    }
    if let Some((node, slot, _)) = app.hovered_status_slot {
        if let Some(content) = app.emitted_status_tooltip(node, slot) {
            return Some(format!(
                "{}\n{}\n{}",
                content.title,
                content.body,
                content.reason.as_deref().unwrap_or_default()
            ));
        }
    }
    if let Some((pane_id, anchor)) = app.hovered_progress {
        if let Some(area) = app
            .pane_viewports()
            .into_iter()
            .find(|viewport| viewport.pane_id == pane_id)
            .and_then(|viewport| viewport.progress_area)
        {
            if let Some(progress) = app.progress_monitor_at(pane_id, area, anchor) {
                let content = crate::progress_bar::details_tooltip(
                    progress,
                    app.progress_display_now_unix_millis(),
                    None,
                );
                return Some(format!(
                    "{}\n{}\n{}",
                    content.title,
                    content.body,
                    content.reason.unwrap_or_default()
                ));
            }
        }
    }
    let mut messages = Vec::new();
    if let Some(bridge) = app.animation_frame.permission_bridge() {
        if let Ok(status) = bridge.status() {
            if status.phase() == crate::animation_plugins::review_bridge::ReviewPhase::Failed {
                messages.push(status.message().to_owned());
            }
        }
    }
    if let Some(error) = app
        .focused_pane_id()
        .or_else(|| app.selected_node_id())
        .and_then(|node| app.editor_load_errors.get(&node))
    {
        messages.push(error.clone());
    }
    if let Some(crate::app::PaneRuntime::Editor(editor)) = app
        .focused_pane_id()
        .or_else(|| app.selected_node_id())
        .and_then(|node| app.panes.get(&node))
    {
        if let Some(error) = &editor.preparation_error {
            messages.push(error.clone());
        }
    }
    if let Some(error) = &app.statistics_diagnostic {
        messages.push(error.clone());
    }
    if matches!(app.mode, Mode::Settings(_)) {
        messages.extend(app.plugin_panel_model().issue_details);
        for error in [
            &app.reset_monitor_state.claude.last_error,
            &app.reset_monitor_state.codex.last_error,
        ]
        .into_iter()
        .flatten()
        {
            messages.push(error.clone());
        }
    }
    if let Some(message) = &app.status_message {
        messages.push(message.clone());
    }
    if let Some(error) = app.semantic_animation_error() {
        messages.push(error);
    }
    if let crate::app::InferenceTestState::Failed { error, .. } = &app.inference_test_state {
        messages.push(error.clone());
    }
    for error in [
        &app.git_settings_error,
        &app.agent_detection_settings_error,
        &app.inference_settings_save_error,
    ]
    .into_iter()
    .flatten()
    {
        messages.push(error.clone());
    }
    if let Some(content) = app
        .selected_node_id()
        .and_then(|node| app.blocking_error_tooltip(node))
    {
        messages.push(format!(
            "{}\n{}\n{}",
            content.title,
            content.body,
            content.reason.as_deref().unwrap_or_default()
        ));
    }
    messages.dedup();
    (!messages.is_empty()).then(|| messages.join("\n\n"))
}

pub(crate) fn intercept_event(app: &mut App, event: &Event) -> bool {
    if let Event::Key(key) = event {
        if key.kind == KeyEventKind::Release {
            return app.diagnostic_view.is_some();
        }
        if key.code == KeyCode::F(9) {
            if app.diagnostic_view.is_some() {
                app.diagnostic_view = None;
            } else if let Some(text) = current_text(app) {
                app.diagnostic_view = Some(DiagnosticView {
                    text,
                    scroll: 0,
                    copy_button: None,
                });
            } else {
                return false;
            }
            return true;
        }
    }
    let Some(view) = &mut app.diagnostic_view else {
        return false;
    };
    let mut is_copy_requested = false;
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => is_copy_requested = true,
            KeyCode::Esc => app.diagnostic_view = None,
            KeyCode::Up => view.scroll = view.scroll.saturating_sub(1),
            KeyCode::Down => view.scroll = view.scroll.saturating_add(1),
            KeyCode::PageUp => view.scroll = view.scroll.saturating_sub(10),
            KeyCode::PageDown => view.scroll = view.scroll.saturating_add(10),
            KeyCode::Home => view.scroll = 0,
            KeyCode::End => view.scroll = usize::MAX,
            _ => {}
        },
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => view.scroll = view.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => view.scroll = view.scroll.saturating_add(3),
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                is_copy_requested = view.copy_button.is_some_and(|button| {
                    button.contains(ratatui::layout::Position::new(mouse.column, mouse.row))
                });
            }
            _ => {}
        },
        _ => {}
    }
    if is_copy_requested {
        if let Some(text) = app.diagnostic_view.as_ref().map(|view| view.text.clone()) {
            app.copy_terminal_text_to_clipboard(
                text,
                "Copied the full diagnostic to the clipboard",
            );
        }
    }
    // Resize still reaches the client lifecycle; input cannot reach the draft.
    !matches!(event, Event::Resize(_, _))
}

pub(crate) fn render(frame: &mut Frame, view: &mut DiagnosticView) {
    let screen = frame.area();
    let width = screen.width.min(100);
    let rows = crate::value_dialog::wrap_cells(&view.text, width.saturating_sub(2).max(1));
    let height = (rows.len().saturating_add(3).min(usize::from(screen.height))) as u16;
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(crate::theme::block(true).title(" Full diagnostic "), area);
    let body = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        width.saturating_sub(2),
        height.saturating_sub(3),
    );
    view.scroll = view
        .scroll
        .min(rows.len().saturating_sub(usize::from(body.height)));
    let visible = rows
        .iter()
        .skip(view.scroll)
        .take(usize::from(body.height))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(Paragraph::new(visible), body);
    view.copy_button = None;
    if height >= 3 {
        let footer_y = area.bottom().saturating_sub(2);
        let button_width = COPY_BUTTON_LABEL.chars().count() as u16;
        let hint_width = body.width.saturating_sub(button_width + 1);
        frame.render_widget(
            Paragraph::new("↑↓ scroll · PgUp/PgDn · c copy · Esc close"),
            Rect::new(body.x, footer_y, hint_width, 1),
        );
        if body.width > button_width {
            let button = Rect::new(body.right() - button_width, footer_y, button_width, 1);
            frame.render_widget(
                Paragraph::new(COPY_BUTTON_LABEL).style(
                    ratatui::style::Style::default()
                        .fg(crate::theme::accent_fg())
                        .bg(crate::theme::accent_bg()),
                ),
                button,
            );
            view.copy_button = Some(button);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn unrelated_function_key_reaches_healthy_terminal() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        assert!(!intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE))
        ));
        assert!(app.diagnostic_view.is_none());
    }

    #[test]
    fn diagnostic_overlay_consumes_paste_without_editing_parent_draft() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        app.mode = Mode::Rename(crate::text_prompt::TextPromptState::new("retained draft"));
        app.status_message = Some("storage unavailable".into());
        intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE)),
        );
        assert!(intercept_event(
            &mut app,
            &Event::Paste("must not insert".into())
        ));
        assert!(!intercept_event(&mut app, &Event::Resize(30, 12)));
        intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        let Mode::Rename(prompt) = &app.mode else {
            panic!("draft mode replaced")
        };
        assert_eq!(prompt.buf, "retained draft");
    }

    #[test]
    fn full_diagnostic_reaches_tail_and_preserves_underlying_mode() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        app.status_message = Some(format!(
            "{}\nFINALDETAIL",
            "長い diagnostic\n".repeat(80_000)
        ));
        assert!(intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE))
        ));
        assert!(matches!(app.mode, Mode::Normal));
        intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
        );
        let mut terminal = Terminal::new(TestBackend::new(48, 16)).unwrap();
        terminal
            .draw(|frame| render(frame, app.diagnostic_view.as_mut().unwrap()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("FINALDETAIL"));
        intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(app.diagnostic_view.is_none());
        assert!(matches!(app.mode, Mode::Normal));
    }
    #[test]
    fn copy_button_and_key_both_request_the_clipboard() {
        let mut app = App::new("test".into(), std::env::temp_dir());
        app.status_message = Some("storage unavailable".into());
        intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE)),
        );
        let mut terminal = Terminal::new(TestBackend::new(48, 16)).unwrap();
        terminal
            .draw(|frame| render(frame, app.diagnostic_view.as_mut().unwrap()))
            .unwrap();
        let button = app.diagnostic_view.as_ref().unwrap().copy_button.unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("[ Copy ]"));
        // No clipboard service exists in tests, so a request shows up as the
        // "unavailable" status instead of the old message.
        app.status_message = None;
        intercept_event(
            &mut app,
            &Event::Mouse(crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: button.x,
                row: button.y,
                modifiers: KeyModifiers::NONE,
            }),
        );
        assert!(app.status_message.is_some(), "button click must copy");
        app.status_message = None;
        intercept_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
        );
        assert!(app.status_message.is_some(), "c key must copy");
    }
}
