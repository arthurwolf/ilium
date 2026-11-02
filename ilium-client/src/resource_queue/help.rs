//! Contextual resource guidance; no agent or hardware commands are executed here.
use crate::app::App;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};
const TEXT:&str="What is a shared resource?\nA piece of hardware (a GPU, a USB board) that only one agent may use at a time. Agents line up in a queue, first in first out. Only the agent that holds the hardware (Using now) may touch it. Waiting agents may keep working on other things.\n\nReading the panel\nStatus shows whether the hardware is free, in use or needs attention. The queue table lists who holds it and who waits, with time estimates. Estimates are only advice: a countdown never releases hardware, and an overdue or uncertain owner makes the wait time unknown. The activity log lists everything that happened to the resource, newest first.\n\nYour own agents\nSelect a row with the mouse or Up/Down. Update my time estimate tells the queue how long you need. Cancel mine removes your waiting agent from the line. Release mine gives the hardware back; stop your device work first. These work only for agents running in this Ilium session.\n\nWhen something goes wrong\nTake back ownership forcibly removes a stuck or missing owner and pauses the queue. Afterwards Confirm safe says the hardware is idle and safe so the next agent may start. Type CONFIRM and give a reason. Ilium never stops programs or resets hardware itself, and cannot stop other programs of the same user from touching it.\n\nPause and offline\nPause queue stops handing out the hardware; the current holder keeps it. Mark offline means the hardware is unplugged or broken. Resume queue reopens the line.\n\nEdit, remove and delete\nEdit changes the name, rules and identity for every project. Remove here makes this project stop using the resource. Delete removes it from this project and then deletes it, only when it is unused and no other project uses it. Repair setup rewrites a project's agent instructions when they are out of date. Retry sends the last failed request again; Refresh reloads the state. Group view shows agents under their resource in the tree.\n\nKeyboard and mouse\nEvery button has a key shown in its tooltip: ? e g v t p u x d z o r c i l a. Hover a button for an explanation. Wheel or PageUp/PageDown scrolls the log. F1 or ? opens this help; Escape goes back.";
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
