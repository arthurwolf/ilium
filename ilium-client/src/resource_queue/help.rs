//! Contextual resource guidance; no agent or hardware commands are executed here.
use crate::app::App;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};
const TEXT:&str="Create and link\nNew resource creates one shared capacity-one FIFO queue. Link existing resource shares that authority; its local slug may differ. Search by title, kind, global ID or project origin. Device identity is an optional typed GPU/USB descriptor, never an automatic hardware probe.\n\nOwned and queued\nOnly an Owned reservation permits device work. Queued agents may keep working independently. A parked marker means the blocking wait command is attached; queue membership alone does not imply the agent stopped working. Split participants are navigation references; selecting one focuses the original split child.\n\nWaiting and estimates\nWait on the existing reservation ID. Interrupting a wait detaches observation and does not cancel the reservation. Estimates are advisory; overdue, paused, unavailable or uncertain ownership makes ETA unknown. A countdown never releases hardware.\n\nOwn reservation controls\nSelect a reservation with Up/Down. d updates its duration estimate, z cancels an own queued claim, o explicitly releases own active ownership. These require a server-verified local invocation. Release only after stopping and cleaning up your own device work.\n\nRecovery\nr revokes the captured owner and pauses the queue; c is a separate confirm-clear action after physical cleanup. x cancels a selected queue entry as user recovery. Recovery captures a fresh catalog revision and challenge; type CONFIRM and an authored reason. Changed ownership/revision rejects the action. Ilium never stops workloads or resets hardware and cannot enforce physical exclusion against same-user processes.\n\nManagement and retry\ne edits global metadata; p pauses/reopens; u marks unavailable; l unlinks this project; a archives. i repairs this project's instruction/settings sync debt. t retries the exact unresolved request; Ctrl+R does the same inside a form. v refreshes authority state. g toggles display-only grouping. Grouping, progress visibility and icons are independent settings.\n\nNavigation\nUp/Down selects a queue reservation; PageUp/PageDown scrolls detail. F1 or ? opens this help; PageUp/PageDown scrolls help, Escape returns. Escape from detail restores the previous terminal or split. All references preserve real pane parents, terminal input routing and per-agent view state.";
pub(super) fn handle_event(app: &mut App, event: &Event) -> bool {
    if !app.resources.help_open {
        return false;
    }
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            match key.code {
                KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('?') => {
                    app.resources.help_open = false
                }
                KeyCode::PageDown | KeyCode::Down => {
                    app.resources.help_scroll = app.resources.help_scroll.saturating_add(5)
                }
                KeyCode::PageUp | KeyCode::Up => {
                    app.resources.help_scroll = app.resources.help_scroll.saturating_sub(5)
                }
                KeyCode::Home => app.resources.help_scroll = 0,
                _ => {}
            }
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.resources.help_scroll = app.resources.help_scroll.saturating_add(3)
            }
            MouseEventKind::ScrollUp => {
                app.resources.help_scroll = app.resources.help_scroll.saturating_sub(3)
            }
            _ => {}
        },
        _ => {}
    }
    true
}
pub(super) fn render(app: &App, frame: &mut Frame) {
    if !app.resources.help_open {
        return;
    }
    let screen = frame.area();
    let width = screen.width.min(96);
    let height = screen.height.min(36);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(TEXT)
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false })
            .scroll((app.resources.help_scroll, 0))
            .block(crate::theme::block(true).title("Resource help · PgUp/PgDown · Esc back")),
        area,
    );
}
