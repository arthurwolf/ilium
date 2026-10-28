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
    let cleaned = strip_unsafe_characters(text);
    if cleaned.len() <= limit {
        return cleaned.into_iter().collect();
    }
    if limit == 0 {
        return String::new();
    }
    let mut cut: String = cleaned.into_iter().take(limit - 1).collect();
    cut.push('\u{2026}');
    cut
}

/// True for bidi controls and zero-width characters.
fn is_invisible_format_character(character: char) -> bool {
    matches!(character,
        '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{061c}'
        | '\u{feff}')
}

/// Removes ANSI escapes, control, bidi and zero-width characters and runs of three or more
/// box-drawing characters; newlines become one space each (CRLF counts once).
fn strip_unsafe_characters(text: &str) -> Vec<char> {
    let characters: Vec<char> = text.chars().collect();
    let mut kept: Vec<char> = Vec::with_capacity(characters.len());
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        index += 1;
        if character == '\u{1b}' {
            if characters.get(index) == Some(&'[') {
                index += 1;
                while index < characters.len() {
                    let final_byte = characters[index];
                    index += 1;
                    if ('\u{40}'..='\u{7e}').contains(&final_byte) {
                        break;
                    }
                }
            }
            continue;
        }
        if character == '\r' && characters.get(index) == Some(&'\n') {
            index += 1;
            kept.push(' ');
        } else if character == '\r' || character == '\n' {
            kept.push(' ');
        } else if character.is_control() || is_invisible_format_character(character) {
            continue;
        } else {
            kept.push(character);
        }
    }
    remove_box_drawing_runs(kept)
}

/// Drops every run of three or more box-drawing characters; shorter runs stay.
fn remove_box_drawing_runs(characters: Vec<char>) -> Vec<char> {
    let is_box = |character: char| ('\u{2500}'..='\u{257f}').contains(&character);
    let mut result = Vec::with_capacity(characters.len());
    let mut index = 0;
    while index < characters.len() {
        let run_end = characters[index..]
            .iter()
            .position(|character| is_box(*character) != is_box(characters[index]))
            .map_or(characters.len(), |offset| index + offset);
        let is_long_box_run = is_box(characters[index]) && run_end - index >= 3;
        if !is_long_box_run {
            result.extend_from_slice(&characters[index..run_end]);
        }
        index = run_end;
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
