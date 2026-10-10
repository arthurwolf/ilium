//! Client-owned recipient selection and multiline message editing.
//! The caller revalidates recipients and owns terminal delivery.

use crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ilium_core::NodeId;
use ilium_core::{NodeKind, PaneContentKind, PaneStatus, Tree};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    widgets::{Clear, Paragraph},
    Frame,
};
use ratatui_textarea::TextArea;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Structural {
        root: NodeId,
        project: bool,
    },
    Filesystem {
        target: NodeId,
        boundary: NodeId,
        path: PathBuf,
    },
}

fn absolute_path(path: &Path, base: Option<&Path>) -> Option<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_owned()
    } else {
        base?.join(path)
    };
    if !joined.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    Some(normalized)
}

impl Scope {
    pub fn capture(tree: &Tree, target: NodeId) -> Result<Self, String> {
        let node = tree
            .get(target)
            .ok_or("The selected project or folder no longer exists")?;
        if node.is_project() || node.is_group() {
            return Ok(Self::Structural {
                root: target,
                project: node.is_project(),
            });
        }
        let NodeKind::Folder { path, .. } = &node.kind else {
            return Err("Select a project or folder".into());
        };
        let boundary = tree
            .project_ancestor(target)
            .or_else(|| {
                let mut parent = node.parent;
                while let Some(id) = parent {
                    let ancestor = tree.get(id)?;
                    if ancestor.is_group() {
                        return Some(id);
                    }
                    parent = ancestor.parent;
                }
                None
            })
            .ok_or("The folder has no containing project or group")?;
        let path = absolute_path(path, tree.project_path_for(target))
            .ok_or("The folder path cannot be resolved; reopen from its project")?;
        Ok(Self::Filesystem {
            target,
            boundary,
            path,
        })
    }

    pub fn recipients(&self, tree: &Tree) -> Vec<NodeId> {
        let root = match self {
            Self::Structural { root, .. } => *root,
            Self::Filesystem { boundary, .. } => *boundary,
        };
        tree.pane_ids_in_subtree(root)
            .into_iter()
            .filter(|id| {
                let is_agent = matches!(
                    tree.get(*id).map(|node| &node.kind),
                    Some(NodeKind::Pane {
                        content: PaneContentKind::Terminal,
                        status: PaneStatus::Agent(_),
                        ..
                    })
                );
                if !is_agent {
                    return false;
                }
                match self {
                    Self::Structural { .. } => true,
                    Self::Filesystem { boundary, path, .. } => {
                        if tree.get(*boundary).is_some_and(|node| node.is_project())
                            && tree.project_ancestor(*id) != Some(*boundary)
                        {
                            return false;
                        }
                        tree.pane_cwd(*id)
                            .and_then(|cwd| absolute_path(cwd, tree.project_path_for(*id)))
                            .is_some_and(|cwd| cwd.starts_with(path))
                    }
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipientState {
    Pending,
    Queued,
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct Recipient {
    pub pane_id: NodeId,
    pub label: String,
    pub checked: bool,
    pub state: RecipientState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Recipients,
    Message,
    Enter,
    Send,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Send,
    Cancel,
}

pub struct AgentMessageDialog {
    pub target: NodeId,
    pub title: String,
    pub recipients: Vec<Recipient>,
    pub message: TextArea<'static>,
    pub press_enter: bool,
    pub focus: Focus,
    pub selected: usize,
    pub offset: usize,
    pub error: Option<String>,
    pub scope: Option<Scope>,
}

impl AgentMessageDialog {
    pub fn new(target: NodeId, title: String, mut recipients: Vec<Recipient>) -> Self {
        for recipient in &mut recipients {
            recipient.checked = true;
        }
        let mut message = TextArea::default();
        message.set_cursor_line_style(Style::default());
        Self {
            target,
            title,
            recipients,
            message,
            press_enter: true,
            focus: Focus::Message,
            selected: 0,
            offset: 0,
            error: None,
            scope: None,
        }
    }

    pub fn text(&self) -> String {
        self.message.lines().join("\n")
    }

    pub fn selected_ids(&self) -> Vec<NodeId> {
        self.recipients
            .iter()
            .filter(|recipient| recipient.checked && recipient.state == RecipientState::Pending)
            .map(|recipient| recipient.pane_id)
            .collect()
    }

    fn toggle_recipient(&mut self) {
        if let Some(recipient) = self.recipients.get_mut(self.selected) {
            if recipient.state == RecipientState::Pending {
                recipient.checked = !recipient.checked;
            }
        }
    }

    fn advance_focus(&mut self, backwards: bool) {
        let order = [
            Focus::Recipients,
            Focus::Message,
            Focus::Enter,
            Focus::Send,
            Focus::Cancel,
        ];
        let index = order
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        self.focus = order[(index + if backwards { order.len() - 1 } else { 1 }) % order.len()];
    }

    pub fn handle_event(&mut self, event: &Event) -> Outcome {
        match event {
            Event::Paste(text) if self.focus == Focus::Message => {
                self.message
                    .insert_str(text.replace("\r\n", "\n").replace('\r', "\n"));
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc => return Outcome::Cancel,
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Outcome::Send;
                }
                KeyCode::Tab => self.advance_focus(key.modifiers.contains(KeyModifiers::SHIFT)),
                KeyCode::BackTab => self.advance_focus(true),
                KeyCode::Up if self.focus == Focus::Recipients => {
                    self.selected = self.selected.saturating_sub(1)
                }
                KeyCode::Down if self.focus == Focus::Recipients => {
                    self.selected = (self.selected + 1).min(self.recipients.len().saturating_sub(1))
                }
                KeyCode::Char(' ') | KeyCode::Enter if self.focus == Focus::Recipients => {
                    self.toggle_recipient()
                }
                KeyCode::Char(' ') | KeyCode::Enter if self.focus == Focus::Enter => {
                    self.press_enter = !self.press_enter
                }
                KeyCode::Enter if self.focus == Focus::Send => return Outcome::Send,
                KeyCode::Enter if self.focus == Focus::Cancel => return Outcome::Cancel,
                _ if self.focus == Focus::Message => {
                    self.message.input(event.clone());
                }
                _ => {}
            },
            _ => {}
        }
        Outcome::Continue
    }

    pub fn handle_mouse(&mut self, screen: Rect, mouse: MouseEvent) -> Outcome {
        let geometry = Geometry::new(screen);
        let recipients_block = crate::theme::block(self.focus == Focus::Recipients);
        let inner = recipients_block.inner(geometry.recipients);
        let rows = inner.height as usize;
        self.offset = self.visible_offset(rows);
        let point = Position::new(mouse.column, mouse.row);
        if geometry.recipients.contains(point) {
            match mouse.kind {
                MouseEventKind::ScrollDown => {
                    self.offset = (self.offset + 1).min(self.recipients.len().saturating_sub(rows));
                    self.selected = self.offset;
                }
                MouseEventKind::ScrollUp => {
                    self.offset = self.offset.saturating_sub(1);
                    self.selected = self.offset;
                }
                MouseEventKind::Down(MouseButton::Left)
                    if point.x > geometry.recipients.x
                        && point.x < geometry.recipients.right().saturating_sub(1)
                        && point.y >= inner.y
                        && point.y < inner.bottom() =>
                {
                    let has_overflow = self.recipients.len() > rows && inner.width > 1;
                    let track_x = inner.right().saturating_sub(1);
                    if has_overflow && point.x >= track_x {
                        return Outcome::Continue;
                    }
                    self.focus = Focus::Recipients;
                    let recipient_index = self.offset + (point.y - inner.y) as usize;
                    if recipient_index < self.recipients.len() {
                        self.selected = recipient_index;
                        self.toggle_recipient();
                    }
                }
                _ => {}
            }
            return Outcome::Continue;
        }
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return Outcome::Continue;
        }
        if geometry.message.contains(point) {
            self.focus = Focus::Message;
        } else if geometry.enter.contains(point) {
            self.focus = Focus::Enter;
            self.press_enter = !self.press_enter;
        } else if geometry.send.contains(point) {
            return Outcome::Send;
        } else if geometry.cancel.contains(point) {
            return Outcome::Cancel;
        }
        Outcome::Continue
    }

    fn visible_offset(&self, rows: usize) -> usize {
        let mut offset = self.offset.min(self.recipients.len().saturating_sub(rows));
        if self.focus == Focus::Recipients && rows > 0 {
            offset = offset.min(self.selected);
            if self.selected >= offset + rows {
                offset = self.selected + 1 - rows;
            }
        }
        offset
    }
}

struct Geometry {
    outer: Rect,
    recipients: Rect,
    message: Rect,
    enter: Rect,
    send: Rect,
    cancel: Rect,
    footer: Rect,
}

impl Geometry {
    fn new(screen: Rect) -> Self {
        let outer = crate::modal::centered_fixed_rect(100, 24, screen);
        let inner = crate::theme::block(true).inner(outer);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(2)])
            .split(inner);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(rows[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(columns[1]);
        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Min(0),
            ])
            .split(right[2]);
        Self {
            outer,
            recipients: columns[0],
            message: right[0],
            enter: right[1],
            send: buttons[0],
            cancel: buttons[1],
            footer: rows[1],
        }
    }
}

pub fn draw(frame: &mut Frame<'_>, screen: Rect, state: &AgentMessageDialog) {
    let geometry = Geometry::new(screen);
    frame.render_widget(Clear, geometry.outer);
    frame.render_widget(
        crate::theme::block(true).title(crate::theme::chrome_title(&format!(
            "Send message to all — {}",
            state.title
        ))),
        geometry.outer,
    );
    // Retain the field name on compact dialogs instead of spending the
    // entire title width on decoration and the selection count.
    let recipient_label = if geometry.recipients.width >= 28 {
        format!(
            "Agents ({}/{})",
            state.selected_ids().len(),
            state.recipients.len()
        )
    } else {
        "Agents".to_owned()
    };
    let recipient_title = if geometry.recipients.width >= 16 {
        crate::theme::chrome_title(&recipient_label)
    } else {
        ratatui::text::Line::from(" Agents ")
    };
    let recipient_block =
        crate::theme::block(state.focus == Focus::Recipients).title(recipient_title);
    let inner = recipient_block.inner(geometry.recipients);
    let offset = state.visible_offset(inner.height as usize);
    let has_overflow = state.recipients.len() > inner.height as usize && inner.width > 1;
    let content_width = if has_overflow {
        inner.width.saturating_sub(1)
    } else {
        inner.width
    };
    frame.render_widget(recipient_block, geometry.recipients);
    for (row, recipient) in state
        .recipients
        .iter()
        .enumerate()
        .skip(offset)
        .take(inner.height as usize)
    {
        let style = if state.focus == Focus::Recipients && row == state.selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let suffix = match &recipient.state {
            RecipientState::Pending => "",
            RecipientState::Queued => " (queued)",
            RecipientState::Unavailable(_) => " (unavailable)",
        };
        frame.render_widget(
            Paragraph::new(format!(
                "[{}] {}{}",
                if recipient.checked { 'x' } else { ' ' },
                recipient.label,
                suffix
            ))
            .style(style),
            Rect::new(inner.x, inner.y + (row - offset) as u16, content_width, 1),
        );
    }
    if has_overflow {
        let visible_rows = inner.height as usize;
        let thumb_length = (visible_rows * visible_rows / state.recipients.len()).max(1);
        let max_thumb_top = visible_rows.saturating_sub(thumb_length);
        let max_offset = state.recipients.len().saturating_sub(visible_rows);
        let thumb_top = (offset * max_thumb_top)
            .checked_div(max_offset)
            .unwrap_or(0);
        let track = (0..visible_rows)
            .map(|row| {
                if (thumb_top..thumb_top + thumb_length).contains(&row) {
                    "█"
                } else {
                    "│"
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        frame.render_widget(
            Paragraph::new(track),
            Rect::new(inner.right() - 1, inner.y, 1, inner.height),
        );
    }
    if state.recipients.is_empty() {
        frame.render_widget(Paragraph::new("No agents in the selected scope"), inner);
    }
    let message_block = crate::theme::block(state.focus == Focus::Message)
        .title(crate::theme::chrome_title("Message"));
    let message_inner = message_block.inner(geometry.message);
    frame.render_widget(message_block, geometry.message);
    frame.render_widget(&state.message, message_inner);
    for (area, focus, text) in [
        (
            geometry.enter,
            Focus::Enter,
            format!(
                "[{}] Press Enter after sending",
                if state.press_enter { 'x' } else { ' ' }
            ),
        ),
        (geometry.send, Focus::Send, "[ Send ]".to_string()),
        (geometry.cancel, Focus::Cancel, "[ Cancel ]".to_string()),
    ] {
        let style = if state.focus == focus {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        frame.render_widget(Paragraph::new(text).style(style), area);
    }
    frame.render_widget(
        Paragraph::new(
            state.error.as_deref().unwrap_or(
                "Tab: change field · Space: toggle agent · Ctrl+Enter: send · Esc: cancel",
            ),
        ),
        geometry.footer,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn scope_is_recursive_and_filesystem_membership_is_component_safe_and_project_local() {
        use ilium_core::{AgentActivity, AgentClass};
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/work/app")).unwrap();
        let group = tree.add_group(project, "nested").unwrap();
        let nested = tree.add_group(group, "deeper").unwrap();
        let folder = tree
            .add_folder(group, PathBuf::from("/work/app/src/./"))
            .unwrap();
        let other_project = tree.add_project(PathBuf::from("/work/other")).unwrap();
        let agent = |tree: &mut Tree, parent, cwd: &str| {
            let id = tree
                .add_pane(parent, "agent", PaneContentKind::Terminal)
                .unwrap();
            tree.set_pane_status(
                id,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
            )
            .unwrap();
            tree.set_pane_launch_cwd(id, PathBuf::from(cwd)).unwrap();
            id
        };
        let exact = agent(&mut tree, project, "/work/app/src");
        let descendant = agent(&mut tree, nested, "src/../src/deep");
        let prefix = agent(&mut tree, nested, "/work/app/src-extra");
        let outside = agent(&mut tree, other_project, "/work/app/src");
        tree.add_pane(nested, "shell", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(nested, "editor", PaneContentKind::Editor)
            .unwrap();
        assert_eq!(
            Scope::capture(&tree, project).unwrap().recipients(&tree),
            vec![descendant, prefix, exact]
        );
        assert_eq!(
            Scope::capture(&tree, group).unwrap().recipients(&tree),
            vec![descendant, prefix]
        );
        assert_eq!(
            Scope::capture(&tree, folder).unwrap().recipients(&tree),
            vec![descendant, exact]
        );
        assert!(!Scope::capture(&tree, folder)
            .unwrap()
            .recipients(&tree)
            .contains(&outside));
    }

    #[test]
    fn queued_and_unavailable_rows_cannot_be_reselected() {
        let mut state = AgentMessageDialog::new(
            NodeId(1),
            "Project".into(),
            vec![Recipient {
                pane_id: NodeId(2),
                label: "First".into(),
                checked: true,
                state: RecipientState::Pending,
            }],
        );
        state.recipients[0].state = RecipientState::Queued;
        state.recipients[0].checked = false;
        state.toggle_recipient();
        assert!(state.selected_ids().is_empty());
        state.recipients[0].state = RecipientState::Unavailable("removed".into());
        state.toggle_recipient();
        assert!(state.selected_ids().is_empty());
    }

    #[test]
    fn all_recipients_start_selected_and_plain_enter_edits_message() {
        let mut state = AgentMessageDialog::new(
            NodeId(1),
            "Project".into(),
            vec![
                Recipient {
                    pane_id: NodeId(2),
                    label: "First".into(),
                    checked: false,
                    state: RecipientState::Pending,
                },
                Recipient {
                    pane_id: NodeId(3),
                    label: "Second".into(),
                    checked: false,
                    state: RecipientState::Pending,
                },
            ],
        );
        assert_eq!(state.selected_ids(), vec![NodeId(2), NodeId(3)]);
        assert!(state.press_enter);
        state.handle_event(&Event::Paste("first\r\nsecond".into()));
        assert_eq!(state.text(), "first\nsecond");
        assert_eq!(
            state.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE
            ))),
            Outcome::Continue
        );
        assert_eq!(state.text(), "first\nsecond\n");
        assert_eq!(
            state.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::CONTROL
            ))),
            Outcome::Send
        );
        state.focus = Focus::Recipients;
        state.handle_event(&Event::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
        )));
        assert_eq!(state.selected_ids(), vec![NodeId(3)]);
        state.focus = Focus::Enter;
        state.handle_event(&Event::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
        )));
        assert!(!state.press_enter);
    }

    #[test]
    fn enter_toggles_focused_recipient_and_submit_option() {
        let mut state = AgentMessageDialog::new(
            NodeId(1),
            "Project".into(),
            vec![
                Recipient {
                    pane_id: NodeId(2),
                    label: "First".into(),
                    checked: true,
                    state: RecipientState::Pending,
                },
                Recipient {
                    pane_id: NodeId(3),
                    label: "Second".into(),
                    checked: true,
                    state: RecipientState::Pending,
                },
            ],
        );

        state.focus = Focus::Recipients;
        assert_eq!(
            state.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            ))),
            Outcome::Continue,
        );
        assert_eq!(state.selected_ids(), vec![NodeId(3)]);

        state.focus = Focus::Enter;
        assert!(state.press_enter);
        assert_eq!(
            state.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            ))),
            Outcome::Continue,
        );
        assert!(!state.press_enter);
    }

    #[test]
    fn mouse_toggles_exact_painted_recipient_and_submit_control() {
        let screen = Rect::new(0, 0, 120, 40);
        let mut state = AgentMessageDialog::new(
            NodeId(1),
            "Project".into(),
            vec![Recipient {
                pane_id: NodeId(2),
                label: "First".into(),
                checked: true,
                state: RecipientState::Pending,
            }],
        );
        let geometry = Geometry::new(screen);
        let click = |area: Rect| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1,
            row: area.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        state.handle_mouse(screen, click(geometry.recipients));
        assert!(state.selected_ids().is_empty());
        let mut send = click(geometry.send);
        send.row = geometry.send.y;
        assert_eq!(state.handle_mouse(screen, send), Outcome::Send);
    }

    #[test]
    fn top_level_mouse_dispatch_preserves_agent_message_recipient_selection() {
        use crate::app::App;
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus};
        use std::path::PathBuf;

        let mut app = App::new("agent-message-mouse-test".to_owned(), std::env::temp_dir());
        let project = app
            .tree
            .add_project(PathBuf::from("/tmp/agent-message-mouse-test"))
            .unwrap();
        let pane = app
            .tree
            .add_pane(project, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.open_agent_message_dialog(project);

        let geometry = Geometry::new(app.layout.screen_area);
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: geometry.recipients.x + 1,
                row: geometry.recipients.y + 1,
                modifiers: KeyModifiers::NONE,
            },
        );

        let crate::app::Mode::AgentMessageDialog(state) = &app.mode else {
            panic!("mouse dispatch must keep the message dialog open");
        };
        assert!(
            state.selected_ids().is_empty(),
            "the integrated mouse dispatcher must apply the recipient click"
        );
    }
}

#[cfg(test)]
mod recipient_overflow_visual_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn agent_message_overflow_track_is_visible_and_does_not_toggle_a_recipient() {
        for (width, height) in [(120, 40), (80, 24), (40, 12)] {
            let screen = Rect::new(0, 0, width, height);
            let recipients = (0..40)
                .map(|index| Recipient {
                    pane_id: NodeId(index + 1),
                    label: format!("R{index:02}"),
                    checked: true,
                    state: RecipientState::Pending,
                })
                .collect();
            let mut state =
                AgentMessageDialog::new(NodeId(100), "Synthetic scope".to_owned(), recipients);
            state.focus = Focus::Recipients;
            state.selected = 39;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, screen, &state)).unwrap();
            let geometry = Geometry::new(screen);
            let inner = crate::theme::block(true).inner(geometry.recipients);
            assert!(
                inner.width >= 8 && inner.height > 0,
                "fixture needs room for a checkbox row and track"
            );
            let x = inner.right() - 1;
            assert!(
                (inner.y..inner.bottom())
                    .any(|y| matches!(terminal.backend().buffer()[(x, y)].symbol(), "│" | "█")),
                "recipient overflow has no track at {width}x{height}"
            );
            let text = (inner.y..inner.bottom())
                .map(|y| {
                    (inner.x..inner.right())
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                text.contains("R39"),
                "last selected recipient is hidden at {width}x{height}"
            );
            let before = state.selected_ids();
            let result = state.handle_mouse(
                screen,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: inner.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
            assert_eq!(result, Outcome::Continue);
            assert_eq!(
                state.selected_ids(),
                before,
                "track click toggled a checkbox"
            );
            for border_x in [geometry.recipients.x, geometry.recipients.right() - 1] {
                state.handle_mouse(
                    screen,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: border_x,
                        row: inner.y,
                        modifiers: KeyModifiers::NONE,
                    },
                );
                assert_eq!(
                    state.selected_ids(),
                    before,
                    "side border click toggled a checkbox"
                );
            }
        }
    }
}
