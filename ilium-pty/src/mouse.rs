//! Encodes host-terminal mouse events into the xterm mouse-report protocols
//! a pty's child process can request (`DECSET` mouse modes + SGR/UTF-8/X10
//! encodings). This is a pty/terminal-protocol concern -- the encoding
//! depends only on what the child *inside* the pty has negotiated
//! (`vt100::Screen::mouse_protocol_mode`/`mouse_protocol_encoding`), never
//! on the tree or agent-detection state above this crate.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// Encodes a crossterm mouse event using the mode and encoding requested by
/// the process inside a terminal. Returning `None` deliberately leaves
/// ordinary shell clicks alone; terminal applications opt in with xterm's
/// DECSET mouse modes before receiving any mouse input.
pub(crate) fn encode_mouse_event(
    event: MouseEvent,
    column: u16,
    row: u16,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    if mode == MouseProtocolMode::None || !event_is_enabled(event.kind, mode) {
        return None;
    }

    let (button_code, release) = mouse_button_code(event.kind)?;
    let modifiers = mouse_modifier_bits(event.modifiers);
    let column = u32::from(column) + 1;
    let row = u32::from(row) + 1;

    match encoding {
        MouseProtocolEncoding::Sgr => {
            // SGR conveys press vs release with the trailing M/m byte, so Cb
            // can (and must) keep the actual released button.
            let code = button_code | modifiers;
            let suffix = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{column};{row}{suffix}").into_bytes())
        }
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
            // Legacy X10/UTF-8 encoding has no press/release suffix -- the
            // only release signal is Cb's low bits reading 3, regardless of
            // which button was released. Reusing the pressed button's code
            // here would make a release indistinguishable from a press.
            let legacy_button_code = if release { 3 } else { button_code };
            let code = legacy_button_code | modifiers;
            let utf8 = matches!(encoding, MouseProtocolEncoding::Utf8);
            encode_legacy_mouse(code, column, row, utf8)
        }
    }
}

/// Whether this event class is part of the terminal's negotiated protocol.
fn event_is_enabled(kind: MouseEventKind, mode: MouseProtocolMode) -> bool {
    match kind {
        MouseEventKind::Down(_)
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => true,
        MouseEventKind::Up(_) => mode != MouseProtocolMode::Press,
        MouseEventKind::Drag(_) => matches!(
            mode,
            MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
        ),
        MouseEventKind::Moved => mode == MouseProtocolMode::AnyMotion,
    }
}

/// Maps a host event to xterm's Cb value before modifier bits are applied.
fn mouse_button_code(kind: MouseEventKind) -> Option<(u8, bool)> {
    match kind {
        MouseEventKind::Down(button) => Some((button_code(button)?, false)),
        // Report the actual released button here; encode_mouse_event's SGR
        // arm uses it as-is, while its legacy arm substitutes button 3 per
        // that encoding's release convention.
        MouseEventKind::Up(button) => Some((button_code(button)?, true)),
        MouseEventKind::Drag(button) => Some((32 | button_code(button)?, false)),
        // Motion with no held button uses legacy button 3 plus bit 5.
        MouseEventKind::Moved => Some((32 | 3, false)),
        MouseEventKind::ScrollUp => Some((64, false)),
        MouseEventKind::ScrollDown => Some((65, false)),
        MouseEventKind::ScrollLeft => Some((66, false)),
        MouseEventKind::ScrollRight => Some((67, false)),
    }
}

/// Maps crossterm's physical buttons to xterm's base button codes.
fn button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
    }
}

/// Applies the xterm modifier-bit convention to a mouse report.
fn mouse_modifier_bits(modifiers: KeyModifiers) -> u8 {
    let mut bits = 0;
    if modifiers.contains(KeyModifiers::SHIFT) {
        bits |= 4;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        bits |= 8;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        bits |= 16;
    }
    bits
}

/// Encodes classic X10/UTF-8 mouse reports. Classic encoding cannot express
/// cells outside 223; SGR-capable applications receive unrestricted values.
fn encode_legacy_mouse(code: u8, column: u32, row: u32, utf8: bool) -> Option<Vec<u8>> {
    let values = [u32::from(code) + 32, column + 32, row + 32];
    if !utf8 && values.iter().any(|value| *value > 255) {
        return None;
    }

    let mut bytes = b"\x1b[M".to_vec();
    for value in values {
        if utf8 {
            let character = char::from_u32(value)?;
            let mut buffer = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        } else {
            bytes.push(value as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn sgr_press_and_release_preserve_coordinates_and_button() {
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::Down(MouseButton::Left)),
                4,
                2,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::Up(MouseButton::Left)),
                4,
                2,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
    }

    #[test]
    fn motion_and_modifiers_follow_xterm_bits() {
        let mut event = mouse(MouseEventKind::Drag(MouseButton::Right));
        event.modifiers = KeyModifiers::SHIFT | KeyModifiers::CONTROL;
        assert_eq!(
            encode_mouse_event(
                event,
                0,
                0,
                MouseProtocolMode::ButtonMotion,
                MouseProtocolEncoding::Sgr
            ),
            Some(b"\x1b[<54;1;1M".to_vec())
        );
    }

    #[test]
    fn press_only_mode_drops_release_and_motion() {
        assert!(encode_mouse_event(
            mouse(MouseEventKind::Up(MouseButton::Left)),
            0,
            0,
            MouseProtocolMode::Press,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
        assert!(encode_mouse_event(
            mouse(MouseEventKind::Drag(MouseButton::Left)),
            0,
            0,
            MouseProtocolMode::Press,
            MouseProtocolEncoding::Sgr
        )
        .is_none());
    }

    #[test]
    fn legacy_encoding_reports_release_as_button_three() {
        // Legacy X10/UTF-8 encoding has no press/release suffix byte, so a
        // release must use Cb's reserved "button 3" value; reusing the
        // pressed button's code would make it indistinguishable from a press.
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::Up(MouseButton::Left)),
                0,
                0,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Default
            ),
            Some(vec![0x1b, b'[', b'M', 3 + 32, 32 + 1, 32 + 1])
        );
    }

    #[test]
    fn legacy_encoding_uses_x10_prefix() {
        assert_eq!(
            encode_mouse_event(
                mouse(MouseEventKind::ScrollDown),
                1,
                3,
                MouseProtocolMode::Press,
                MouseProtocolEncoding::Default
            ),
            Some(vec![0x1b, b'[', b'M', 97, 34, 36])
        );
    }
}
