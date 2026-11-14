//! How text reaches an agent's composer: the bytes of one initial prompt and, in
//! [`prompt_tracker`], the reconstruction of what a pane's composer holds. The single rule for
//! delivery lives here so every caller frames input the same way. The PTY write and the wait
//! for a visible prompt stay with the kernel (`send-input`, foreground detection).

pub mod prompt_tracker;

/// Encodes multiline editor content as one bracketed paste, leaving the
/// caller responsible for the one semantic submission key that follows it.
///
/// Two boundary hazards are normalized before framing: a bare or CRLF `\r`
/// (e.g. classic-Mac or Windows line endings surviving into an editor source
/// line) reads to a composer as a premature Enter if written raw, and literal
/// bracketed-paste markers embedded in the task text corrupt paste framing --
/// an end marker inside our paste would close it early and let the remainder
/// of the payload run as live keystrokes, while a start marker written raw on
/// the single-line path would open a phantom paste that swallows the
/// submission Enter (and everything typed after it). Both markers are
/// stripped before either framing branch runs.
pub fn initial_input_bytes(initial_input: &str) -> Vec<u8> {
    let normalized_input = initial_input.replace("\r\n", "\n").replace('\r', "\n");
    let sanitized_input = normalized_input
        .replace("\x1b[200~", "")
        .replace("\x1b[201~", "");

    if !sanitized_input.contains('\n') {
        return sanitized_input.into_bytes();
    }

    let mut bracketed_paste = Vec::with_capacity(sanitized_input.len() + 12);
    bracketed_paste.extend_from_slice(b"\x1b[200~");
    bracketed_paste.extend_from_slice(sanitized_input.as_bytes());
    bracketed_paste.extend_from_slice(b"\x1b[201~");
    bracketed_paste
}

#[cfg(test)]
mod tests {
    use super::initial_input_bytes;

    #[test]
    fn multiline_initial_prompt_is_one_bracketed_paste_body() {
        assert_eq!(
            initial_input_bytes("first\nsecond"),
            b"\x1b[200~first\nsecond\x1b[201~"
        );
    }

    #[test]
    fn single_line_initial_prompt_is_written_verbatim() {
        assert_eq!(initial_input_bytes("do_work();"), b"do_work();");
    }

    #[test]
    fn crlf_line_endings_are_normalized_before_bracketing() {
        assert_eq!(
            initial_input_bytes("first\r\nsecond"),
            b"\x1b[200~first\nsecond\x1b[201~"
        );
    }

    #[test]
    fn a_lone_carriage_return_becomes_a_line_break_not_a_premature_enter() {
        // Classic-Mac-style line endings have no `\n` at all; without
        // normalization this would take the unwrapped single-line branch and
        // send a raw `\r` the composer could read as an early submission.
        assert_eq!(
            initial_input_bytes("first\rsecond"),
            b"\x1b[200~first\nsecond\x1b[201~"
        );
    }

    #[test]
    fn an_embedded_paste_end_marker_is_stripped_so_it_cannot_end_the_paste_early() {
        assert_eq!(
            initial_input_bytes("before\x1b[201~after\nnext"),
            b"\x1b[200~beforeafter\nnext\x1b[201~"
        );
    }

    #[test]
    fn a_single_line_paste_start_marker_is_stripped_so_it_cannot_open_a_phantom_paste() {
        // Written raw, `ESC[200~` would put the composer into paste mode with
        // no closing marker, so the later Enter would be swallowed as paste
        // content.
        assert_eq!(initial_input_bytes("before\x1b[200~after"), b"beforeafter");
    }

    #[test]
    fn an_embedded_paste_start_marker_inside_a_multiline_paste_is_stripped() {
        assert_eq!(
            initial_input_bytes("before\x1b[200~after\nnext"),
            b"\x1b[200~beforeafter\nnext\x1b[201~"
        );
    }
}
