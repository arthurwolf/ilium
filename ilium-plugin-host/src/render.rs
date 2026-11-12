//! UI tree caps, retained-tree diffing and contribution replication (reference-ui.md).

use ilium_plugin_api::generated::types::ui::UiTree;

/// What changed between two trees (reference-ui.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiDelta {
    /// JSON pointers of the changed nodes.
    pub changed_paths: Vec<String>,
}

/// Diffs a retained tree against the next one.
pub fn diff(previous: &UiTree, next: &UiTree) -> UiDelta {
    let longest = previous.nodes.len().max(next.nodes.len());
    let mut changed_paths: Vec<String> = (0..longest)
        .filter(|index| previous.nodes.get(*index) != next.nodes.get(*index))
        .map(|index| format!("/nodes/{index}"))
        .collect();
    if previous.root != next.root {
        changed_paths.push("/root".to_owned());
    }
    UiDelta { changed_paths }
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
mod tests {
    use super::{diff, sanitize_plugin_text, UiDelta};
    use ilium_plugin_api::generated::types::ui::{NodeFlags, UiNode, UiTree, Widget};

    /// Builds a node with the given key and children and no flags, tooltip or focus data.
    fn make_node(key: Option<&str>, children: Vec<u32>) -> UiNode {
        UiNode {
            key: key.map(str::to_owned),
            widget: Widget::Clear,
            children,
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }
    }

    /// Builds a tree whose root is node 0 and whose nodes are the given list.
    fn make_tree(nodes: Vec<UiNode>) -> UiTree {
        UiTree {
            version: 1,
            nodes,
            root: 0,
        }
    }

    /// Keeps only the alphabetic characters, so tests can check that visible letters survive
    /// whichever way the host removes or replaces a control character.
    fn letters_only(text: &str) -> String {
        text.chars()
            .filter(|character| character.is_alphabetic())
            .collect()
    }

    /// REQ: HST-269
    #[test]
    fn diff_of_identical_trees_reports_no_changed_paths() {
        let previous = make_tree(vec![
            make_node(Some("root"), vec![1]),
            make_node(Some("save"), vec![]),
        ]);
        let next = previous.clone();
        assert_eq!(
            diff(&previous, &next),
            UiDelta {
                changed_paths: vec![]
            }
        );
    }

    /// REQ: HST-269
    #[test]
    fn diff_of_two_empty_trees_reports_no_changed_paths() {
        let previous = UiTree::default();
        let next = UiTree::default();
        assert_eq!(
            diff(&previous, &next),
            UiDelta {
                changed_paths: vec![]
            }
        );
    }

    /// REQ: HST-269
    #[test]
    fn diff_reports_a_changed_leaf_as_a_json_pointer() {
        let previous = make_tree(vec![
            make_node(Some("root"), vec![1]),
            make_node(Some("save"), vec![]),
        ]);
        let mut changed_leaf = make_node(Some("save"), vec![]);
        changed_leaf.disabled_reason = Some("busy".to_owned());
        let next = make_tree(vec![make_node(Some("root"), vec![1]), changed_leaf]);
        assert_eq!(
            diff(&previous, &next),
            UiDelta {
                changed_paths: vec!["/nodes/1".to_owned()]
            }
        );
    }

    /// REQ: HST-269
    #[test]
    fn diff_reports_an_added_node_and_its_changed_parent() {
        let previous = make_tree(vec![make_node(Some("root"), vec![])]);
        let next = make_tree(vec![
            make_node(Some("root"), vec![1]),
            make_node(Some("new"), vec![]),
        ]);
        let delta = diff(&previous, &next);
        assert!(
            delta.changed_paths.contains(&"/nodes/1".to_owned()),
            "the added node must be reported, got {:?}",
            delta.changed_paths
        );
        assert!(
            delta.changed_paths.contains(&"/nodes/0".to_owned()),
            "the parent whose child list changed must be reported, got {:?}",
            delta.changed_paths
        );
    }

    /// REQ: HST-269
    #[test]
    fn diff_from_an_empty_tree_to_a_populated_tree_reports_every_node() {
        let previous = UiTree::default();
        let next = make_tree(vec![
            make_node(Some("root"), vec![1]),
            make_node(Some("child"), vec![]),
        ]);
        let delta = diff(&previous, &next);
        assert!(
            delta.changed_paths.contains(&"/nodes/0".to_owned()),
            "the root must be reported, got {:?}",
            delta.changed_paths
        );
        assert!(
            delta.changed_paths.contains(&"/nodes/1".to_owned()),
            "the new child must be reported, got {:?}",
            delta.changed_paths
        );
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_returns_empty_text_for_empty_input() {
        assert_eq!(sanitize_plugin_text("", 10), "");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_keeps_text_that_is_within_the_limit_unchanged() {
        assert_eq!(sanitize_plugin_text("Save file", 20), "Save file");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_keeps_text_whose_length_equals_the_limit_without_an_ellipsis() {
        assert_eq!(sanitize_plugin_text("hello", 5), "hello");
    }

    /// REQ: WID-015
    #[test]
    fn sanitize_cuts_text_longer_than_the_limit_and_ends_with_an_ellipsis() {
        let result = sanitize_plugin_text("abcdefghij", 5);
        assert_eq!(result.chars().count(), 5);
        assert_eq!(result, "abcd\u{2026}");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_counts_characters_not_bytes_when_cutting() {
        let result = sanitize_plugin_text("h\u{e9}llo w\u{f6}rld", 5);
        assert_eq!(result.chars().count(), 5);
        assert_eq!(result, "h\u{e9}ll\u{2026}");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_with_limit_one_returns_only_the_ellipsis_for_longer_text() {
        assert_eq!(sanitize_plugin_text("abc", 1), "\u{2026}");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_with_limit_zero_returns_empty_text() {
        assert_eq!(sanitize_plugin_text("abc", 0), "");
    }

    /// REQ: RND-019
    #[test]
    fn sanitize_removes_ansi_escape_sequences() {
        assert_eq!(sanitize_plugin_text("\u{1b}[31mred\u{1b}[0m", 50), "red");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_removes_c0_and_c1_control_characters() {
        let result = sanitize_plugin_text("a\u{7}b\u{7f}c\u{81}d", 50);
        assert_eq!(letters_only(&result), "abcd");
        assert!(!result.chars().any(|character| character.is_control()));
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_removes_bidi_override_and_isolate_characters() {
        let result = sanitize_plugin_text("abc\u{202e}def\u{2066}ghi\u{2069}", 50);
        assert_eq!(letters_only(&result), "abcdefghi");
        assert!(!result.contains('\u{202e}'));
        assert!(!result.contains('\u{2066}'));
        assert!(!result.contains('\u{2069}'));
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_removes_zero_width_characters() {
        let result = sanitize_plugin_text("a\u{200b}b\u{200d}c", 50);
        assert_eq!(letters_only(&result), "abc");
        assert!(!result.contains('\u{200b}'));
        assert!(!result.contains('\u{200d}'));
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_removes_a_run_of_three_box_drawing_characters() {
        let result = sanitize_plugin_text("a\u{2500}\u{2500}\u{2500}b", 50);
        assert_eq!(letters_only(&result), "ab");
        assert!(!result.contains("\u{2500}\u{2500}\u{2500}"));
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_keeps_a_single_box_drawing_character() {
        assert_eq!(sanitize_plugin_text("a\u{2500}b", 50), "a\u{2500}b");
    }

    /// REQ: RND-019
    #[test]
    fn sanitize_turns_crlf_into_one_space() {
        assert_eq!(sanitize_plugin_text("a\r\nb", 50), "a b");
    }

    /// REQ: RND-019
    #[test]
    fn sanitize_turns_a_lone_lf_or_cr_into_one_space_each() {
        assert_eq!(sanitize_plugin_text("a\nb", 50), "a b");
        assert_eq!(sanitize_plugin_text("a\rb", 50), "a b");
    }

    /// REQ: SEC-380
    #[test]
    fn sanitize_does_not_count_removed_control_characters_against_the_limit() {
        assert_eq!(sanitize_plugin_text("\u{1b}[31mabc", 3), "abc");
    }
}

/// Number of colours the outer terminal offers, for `nearest_terminal_colour` (HST-390).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalPalette {
    Colours16,
    Colours256,
}

/// Maps a plugin `rgb` colour to the nearest colour index the terminal offers. A 16-colour
/// terminal still gets a valid index, so the plugin call never fails on colour (HST-390).
pub fn nearest_terminal_colour(red: u8, green: u8, blue: u8, palette: TerminalPalette) -> u8 {
    const BASE16: [(u8, u8, u8); 16] = [
        (0, 0, 0), (128, 0, 0), (0, 128, 0), (128, 128, 0), (0, 0, 128), (128, 0, 128),
        (0, 128, 128), (192, 192, 192), (128, 128, 128), (255, 0, 0), (0, 255, 0),
        (255, 255, 0), (0, 0, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
    ];
    let level = |value: u8| -> u8 {
        const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let mut best = 0;
        for (index, step) in STEPS.iter().enumerate() {
            if value.abs_diff(*step) < value.abs_diff(STEPS[best]) {
                best = index;
            }
        }
        best as u8
    };
    let distance = |(r, g, b): (u8, u8, u8)| -> u32 {
        let square = |a: u8, b: u8| u32::from(a.abs_diff(b)).pow(2);
        square(r, red) + square(g, green) + square(b, blue)
    };
    let nearest_base = BASE16
        .iter()
        .enumerate()
        .min_by_key(|(_, colour)| distance(**colour))
        .map(|(index, _)| index as u8)
        .unwrap_or(0);
    match palette {
        TerminalPalette::Colours16 => nearest_base,
        TerminalPalette::Colours256 => {
            let cube = 16 + 36 * level(red) + 6 * level(green) + level(blue);
            let steps = [0u8, 95, 135, 175, 215, 255];
            let cube_rgb = (
                steps[usize::from(level(red))],
                steps[usize::from(level(green))],
                steps[usize::from(level(blue))],
            );
            if distance(cube_rgb) <= distance(BASE16[usize::from(nearest_base)]) {
                cube
            } else {
                nearest_base
            }
        }
    }
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
