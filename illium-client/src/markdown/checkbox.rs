//! Detects a GFM task-list checkbox (`- [ ] foo` / `- [x] foo`) inside a
//! raw source line, so a click in `EditorPane`'s Source view can be mapped
//! to "this is a checkbox, flip it" without a full markdown parse -- the
//! same "cheap text heuristic over the raw line" approach `minimap.rs`
//! already uses for its own line classification.

/// If `line` is a task-list item, returns the char index of its `[`
/// bracket plus whether it's currently checked. `None` for every other
/// line shape (not a list item, or a list item with no checkbox).
pub fn find_checkbox(line: &str) -> Option<(usize, bool)> {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() && chars[index] == ' ' {
        index += 1;
    }

    if index < chars.len() && matches!(chars[index], '-' | '*' | '+') {
        index += 1;
    } else {
        let marker_start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        if index == marker_start {
            return None;
        }
        if index < chars.len() && matches!(chars[index], '.' | ')') {
            index += 1;
        } else {
            return None;
        }
    }

    if chars.get(index) != Some(&' ') {
        return None;
    }
    index += 1;

    if chars.get(index) != Some(&'[') || chars.get(index + 2) != Some(&']') {
        return None;
    }
    let checked = match chars.get(index + 1) {
        Some(' ') => false,
        Some('x' | 'X') => true,
        _ => return None,
    };
    Some((index, checked))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchecked_dash_item() {
        assert_eq!(find_checkbox("- [ ] buy milk"), Some((2, false)));
    }

    #[test]
    fn checked_dash_item() {
        assert_eq!(find_checkbox("- [x] buy milk"), Some((2, true)));
    }

    #[test]
    fn checked_uppercase_x() {
        assert_eq!(find_checkbox("- [X] buy milk"), Some((2, true)));
    }

    #[test]
    fn indented_item() {
        assert_eq!(find_checkbox("  - [ ] nested"), Some((4, false)));
    }

    #[test]
    fn star_and_plus_markers() {
        assert_eq!(find_checkbox("* [ ] a"), Some((2, false)));
        assert_eq!(find_checkbox("+ [x] a"), Some((2, true)));
    }

    #[test]
    fn ordered_marker() {
        assert_eq!(find_checkbox("1. [ ] a"), Some((3, false)));
        assert_eq!(find_checkbox("12) [x] a"), Some((4, true)));
    }

    #[test]
    fn plain_list_item_has_no_checkbox() {
        assert_eq!(find_checkbox("- just text"), None);
    }

    #[test]
    fn non_list_line_is_not_a_checkbox() {
        assert_eq!(find_checkbox("plain paragraph [ ] not a checkbox"), None);
    }

    #[test]
    fn brackets_without_space_or_x_are_rejected() {
        assert_eq!(find_checkbox("- [y] a"), None);
        assert_eq!(find_checkbox("- [  ] a"), None);
    }
}
