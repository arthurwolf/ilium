//! UI tree caps, retained-tree diffing and contribution replication (reference-ui.md).

use ilium_plugin_api::generated::types::ui::UiTree;

/// What changed between two trees (reference-ui.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiDelta {
    /// JSON pointers of the changed nodes.
    pub changed_paths: Vec<String>,
}

/// Diffs a retained tree against the next one.
pub fn diff(_previous: &UiTree, _next: &UiTree) -> UiDelta {
    unimplemented!("host::render::diff")
}

/// Strips control and bidi characters from plugin text and cuts it to `limit` characters
/// (reference-extension-security.md SEC-380).
pub fn sanitize_plugin_text(text: &str, limit: usize) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Ground,
        Escape,
        EscapeIntermediate,
        CsiParameters,
        CsiIntermediate,
        String { osc: bool, escaped: bool },
    }

    fn forbidden(c: char) -> bool {
        c.is_control()
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            || matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}')
    }

    fn emit(output: &mut String, count: &mut usize, limit: usize, c: char) -> bool {
        if *count == limit {
            output.pop();
            output.push('…');
            return false;
        }
        output.push(c);
        *count += 1;
        true
    }

    if limit == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut count = 0;
    let mut state = State::Ground;
    let mut pending = [None, None];
    let mut pending_len = 0;
    let mut long_run = false;
    for character in text.chars() {
        let mut reprocess = true;
        while reprocess {
            reprocess = false;
            match state {
                State::Ground => {
                    state = match character {
                        '\u{1b}' => State::Escape,
                        '\u{9b}' => State::CsiParameters,
                        '\u{9d}' => State::String {
                            osc: true,
                            escaped: false,
                        },
                        '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => State::String {
                            osc: false,
                            escaped: false,
                        },
                        _ if forbidden(character) => State::Ground,
                        _ => {
                            if ('\u{2500}'..='\u{257f}').contains(&character) {
                                if !long_run {
                                    if pending_len < 2 {
                                        pending[pending_len] = Some(character);
                                        pending_len += 1;
                                    } else {
                                        pending_len = 0;
                                        long_run = true;
                                        if !emit(&mut result, &mut count, limit, ' ') {
                                            return result;
                                        }
                                    }
                                }
                            } else {
                                for c in pending[..pending_len].iter().flatten() {
                                    if !emit(&mut result, &mut count, limit, *c) {
                                        return result;
                                    }
                                }
                                pending_len = 0;
                                long_run = false;
                                if !emit(&mut result, &mut count, limit, character) {
                                    return result;
                                }
                            }
                            State::Ground
                        }
                    };
                }
                State::Escape => {
                    state = match character {
                        '[' => State::CsiParameters,
                        ']' => State::String {
                            osc: true,
                            escaped: false,
                        },
                        'P' | 'X' | '^' | '_' => State::String {
                            osc: false,
                            escaped: false,
                        },
                        '\u{20}'..='\u{2f}' => State::EscapeIntermediate,
                        '\u{1b}' => State::Escape,
                        '7' | '8' | '=' | '>' | 'D' | 'E' | 'H' | 'M' | 'N' | 'O' | 'Z' | 'c' => {
                            State::Ground
                        }
                        _ => {
                            reprocess = true;
                            State::Ground
                        }
                    };
                }
                State::EscapeIntermediate => {
                    state = match character {
                        '\u{1b}' => State::Escape,
                        '\u{20}'..='\u{2f}' => State::EscapeIntermediate,
                        '\u{30}'..='\u{7e}' => State::Ground,
                        _ if forbidden(character) => State::EscapeIntermediate,
                        _ => {
                            reprocess = true;
                            State::Ground
                        }
                    };
                }
                State::CsiParameters | State::CsiIntermediate => {
                    state = match character {
                        '\u{1b}' => State::Escape,
                        '\u{20}'..='\u{2f}' => State::CsiIntermediate,
                        '\u{30}'..='\u{3f}' if matches!(state, State::CsiParameters) => {
                            State::CsiParameters
                        }
                        '\u{40}'..='\u{7e}' => State::Ground,
                        _ if forbidden(character) => state,
                        _ => {
                            reprocess = true;
                            State::Ground
                        }
                    };
                }
                State::String { osc, escaped } => {
                    state = if character == '\u{9c}'
                        || (escaped && character == '\\')
                        || (osc && character == '\u{7}')
                    {
                        State::Ground
                    } else {
                        State::String {
                            osc,
                            escaped: character == '\u{1b}',
                        }
                    };
                }
            }
        }
    }
    for c in pending[..pending_len].iter().flatten() {
        if !emit(&mut result, &mut count, limit, *c) {
            return result;
        }
    }
    result
}

#[cfg(test)]
mod text_tests {
    use super::sanitize_plugin_text;

    #[test]
    fn strips_sequences_before_detecting_box_runs() {
        for separator in ["\u{7}", "\u{200b}", "\u{1b}[0m"] {
            assert_eq!(
                sanitize_plugin_text(&format!("a──{separator}─b"), 120),
                "a b"
            );
        }
        assert_eq!(
            sanitize_plugin_text("\u{1b}[31mred\u{1b}[0m \u{1b}]0;title\u{7}done", 120),
            "red done"
        );
        assert_eq!(sanitize_plugin_text("a\u{1b}b", 120), "ab");
        assert_eq!(sanitize_plugin_text("a\u{1b}]secret", 120), "a");
        assert_eq!(
            sanitize_plugin_text("a\u{1b}Psecret\u{7}more\u{1b}\\b", 120),
            "ab"
        );
    }

    #[test]
    fn clips_only_after_sanitizing() {
        for (input, limit, expected) in [
            ("abc", 2, "a…"),
            ("ab\u{7}", 2, "ab"),
            ("───", 2, " "),
            ("ab", 1, "…"),
            ("a", 0, ""),
            ("é日本", 2, "é…"),
        ] {
            let clean = sanitize_plugin_text(input, limit);
            assert_eq!(clean, expected);
            assert_eq!(sanitize_plugin_text(&clean, limit), clean);
        }
        assert_eq!(sanitize_plugin_text("hello", usize::MAX), "hello");
    }
}
