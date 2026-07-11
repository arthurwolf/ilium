//! Translates `illium_ipc`'s wire-shaped mouse event types into
//! `crossterm::event::MouseEvent`, the type `illium_pty::PtySession::write_mouse_input`
//! expects. Kept separate from `illium-pty`'s own `mouse` module -- that
//! one encodes a `crossterm::event::MouseEvent` into the xterm wire bytes
//! a pty's negotiated mouse protocol expects; this one just converts
//! between two *structured* representations of the same event so the IPC
//! protocol doesn't need to depend on `crossterm` itself.

use illium_ipc::{
    MouseButton as IpcMouseButton, MouseEventKind as IpcMouseEventKind, MouseModifiers,
};

/// Builds the `crossterm::event::MouseEvent` for `PtySession::write_mouse_input`
/// from one `illium_ipc::ClientRequest::MouseInput`'s fields.
pub fn to_crossterm_event(
    kind: IpcMouseEventKind,
    column: u16,
    row: u16,
    modifiers: MouseModifiers,
) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind: to_crossterm_kind(kind),
        column,
        row,
        modifiers: to_crossterm_modifiers(modifiers),
    }
}

fn to_crossterm_kind(kind: IpcMouseEventKind) -> crossterm::event::MouseEventKind {
    use crossterm::event::MouseEventKind as Ct;
    match kind {
        IpcMouseEventKind::Down(button) => Ct::Down(to_crossterm_button(button)),
        IpcMouseEventKind::Up(button) => Ct::Up(to_crossterm_button(button)),
        IpcMouseEventKind::Drag(button) => Ct::Drag(to_crossterm_button(button)),
        IpcMouseEventKind::Moved => Ct::Moved,
        IpcMouseEventKind::ScrollUp => Ct::ScrollUp,
        IpcMouseEventKind::ScrollDown => Ct::ScrollDown,
        IpcMouseEventKind::ScrollLeft => Ct::ScrollLeft,
        IpcMouseEventKind::ScrollRight => Ct::ScrollRight,
    }
}

fn to_crossterm_button(button: IpcMouseButton) -> crossterm::event::MouseButton {
    match button {
        IpcMouseButton::Left => crossterm::event::MouseButton::Left,
        IpcMouseButton::Right => crossterm::event::MouseButton::Right,
        IpcMouseButton::Middle => crossterm::event::MouseButton::Middle,
    }
}

fn to_crossterm_modifiers(modifiers: MouseModifiers) -> crossterm::event::KeyModifiers {
    let mut result = crossterm::event::KeyModifiers::NONE;
    if modifiers.shift {
        result |= crossterm::event::KeyModifiers::SHIFT;
    }
    if modifiers.alt {
        result |= crossterm::event::KeyModifiers::ALT;
    }
    if modifiers.control {
        result |= crossterm::event::KeyModifiers::CONTROL;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_left_with_shift_converts_field_for_field() {
        let event = to_crossterm_event(
            IpcMouseEventKind::Down(IpcMouseButton::Left),
            10,
            5,
            MouseModifiers {
                shift: true,
                alt: false,
                control: false,
            },
        );
        assert_eq!(
            event.kind,
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
        );
        assert_eq!(event.column, 10);
        assert_eq!(event.row, 5);
        assert_eq!(event.modifiers, crossterm::event::KeyModifiers::SHIFT);
    }

    #[test]
    fn scroll_and_move_kinds_carry_no_button() {
        assert_eq!(
            to_crossterm_kind(IpcMouseEventKind::Moved),
            crossterm::event::MouseEventKind::Moved
        );
        assert_eq!(
            to_crossterm_kind(IpcMouseEventKind::ScrollUp),
            crossterm::event::MouseEventKind::ScrollUp
        );
        assert_eq!(
            to_crossterm_kind(IpcMouseEventKind::ScrollDown),
            crossterm::event::MouseEventKind::ScrollDown
        );
        assert_eq!(
            to_crossterm_kind(IpcMouseEventKind::ScrollLeft),
            crossterm::event::MouseEventKind::ScrollLeft
        );
        assert_eq!(
            to_crossterm_kind(IpcMouseEventKind::ScrollRight),
            crossterm::event::MouseEventKind::ScrollRight
        );
    }

    #[test]
    fn all_modifiers_combine_into_the_crossterm_bitflags() {
        let modifiers = to_crossterm_modifiers(MouseModifiers {
            shift: true,
            alt: true,
            control: true,
        });
        assert!(modifiers.contains(crossterm::event::KeyModifiers::SHIFT));
        assert!(modifiers.contains(crossterm::event::KeyModifiers::ALT));
        assert!(modifiers.contains(crossterm::event::KeyModifiers::CONTROL));
    }
}
