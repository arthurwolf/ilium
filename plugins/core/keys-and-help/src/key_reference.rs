//! Pure text rules of the key reference (port of the width helpers of `ilium-client/src/help.rs`).
//!
//! The panel rows are cut to the cell width of the view. A cut text ends with `…`, and the width is
//! measured in terminal cells, so a wide character is never split and never overruns the column.

use ilium_plugin_sdk::generated::types::layout::Alignment;
use ilium_plugin_sdk::generated::types::text::{Modifiers, Span, Style, StyledText};
use ilium_plugin_sdk::generated::types::ui::{NodeFlags, ParagraphSpec, UiNode, UiTree, Widget};
use ilium_plugin_sdk::host::keymap::Binding;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The panel's heading line.
pub const TITLE: &str = "Key reference";

/// Columns the panel keeps free for the frame the host draws around it.
const FRAME_COLUMNS: u16 = 2;

/// A gap narrower than this stays blank instead of carrying dotted leaders.
const LEADER_MIN_GAP: usize = 5;

/// The ellipsis that marks an omission, one cell wide.
pub const ELLIPSIS: &str = "…";

/// Cuts `text` to at most `max_width` cells. A text that fits is returned unchanged; a longer one
/// keeps as many whole characters as fit before the ellipsis; a zero width gives an empty string.
pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    let content_width = max_width.saturating_sub(UnicodeWidthStr::width(ELLIPSIS));
    let mut result = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > content_width {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push_str(ELLIPSIS);
    result
}

/// The line of one key row: the chord padded to `chord_width` cells, two blanks, then the summary
/// cut to the rest of `total_width`. A chord wider than its column is cut like a summary.
pub fn row_line(chord: &str, summary: &str, chord_width: usize, total_width: usize) -> String {
    let chord = truncate_to_width(chord, chord_width);
    let padding = chord_width.saturating_sub(UnicodeWidthStr::width(chord.as_str()));
    let summary_width = total_width.saturating_sub(chord_width + 2);
    format!(
        "{chord}{}  {}",
        " ".repeat(padding),
        truncate_to_width(summary, summary_width)
    )
}

/// The label and the leader-filled gap before the chord label, `width` cells in all: one blank
/// cell on each side of the dots, and no dots in a gap under five cells. A label too long for
/// the row is cut so the chord label always shows.
pub fn leader_line(label: &str, chord: &str, width: usize) -> String {
    let chord_width = UnicodeWidthStr::width(chord);
    let label = truncate_to_width(label, width.saturating_sub(chord_width + 1));
    let gap = width
        .saturating_sub(UnicodeWidthStr::width(label.as_str()))
        .saturating_sub(chord_width)
        .max(1);
    let filler = if gap >= LEADER_MIN_GAP {
        format!(" {} ", "…".repeat(gap - 2))
    } else {
        " ".repeat(gap)
    };
    format!("{label}{filler}{chord}")
}

/// The heading a binding context is shown under: the kernel's own contexts get a readable
/// name, any other context (an area name) is shown as it is.
pub fn context_heading(context: &str) -> String {
    match context {
        "leader" | "prefix" => "Prefix keys".to_owned(),
        "global" => "Global keys".to_owned(),
        other => match other.strip_prefix("pane:") {
            Some(kind) => format!("In a {kind} pane"),
            None => other.to_owned(),
        },
    }
}

/// The label of a binding. Labels come from the keymap; a bare command id
/// (`ilium/split-vertical`, `ilium.core.keys-and-help/show-keys`) reads as words.
pub fn command_label(command: &str) -> String {
    if command.contains(' ') || !(command.contains('/') || command.contains('.')) {
        return command.to_owned();
    }
    let name = command.rsplit(['/', '.']).next().unwrap_or(command);
    let words = name.replace(['-', '_'], " ");
    let mut characters = words.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => command.to_owned(),
    }
}

/// The chord label of a binding: the host's display form, after `Prefix` for a key pressed
/// after the leader when the chord does not already name the prefix.
pub fn chord_text(context: &str, chord: &str, label: &str) -> String {
    let after_leader = matches!(context, "leader" | "prefix");
    if after_leader && !chord.starts_with("prefix+") {
        format!("Prefix  {label}")
    } else {
        label.to_owned()
    }
}

/// The display form of a prefix setting such as `ctrl+b` (or `ctrl-b`): `Ctrl+B`.
pub fn prefix_label(setting: &str) -> String {
    setting
        .split(['+', '-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.chars().count() == 1 {
                part.to_uppercase()
            } else {
                let mut characters = part.chars();
                characters
                    .next()
                    .map(|first| first.to_uppercase().chain(characters).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<String>>()
        .join("+")
}

/// The last line: the prefix and the active preset, from the Keyboard tab's JSON. `None` when
/// the tab names no prefix.
pub fn footer(keyboard_json: &str) -> Option<String> {
    let settings: serde_json::Value = serde_json::from_str(keyboard_json).ok()?;
    let prefix = prefix_label(settings.get("shortcut_base")?.as_str()?);
    if prefix.is_empty() {
        return None;
    }
    let preset = match settings.get("preset").and_then(serde_json::Value::as_str) {
        Some("screen") => Some("GNU Screen".to_owned()),
        Some("tmux") => Some("tmux".to_owned()),
        Some("custom") | None => None,
        Some(other) => Some(other.to_owned()),
    };
    Some(match preset {
        Some(preset) => format!("Prefix is {prefix}. Preset: {preset}."),
        None => format!("Prefix is {prefix}."),
    })
}

fn plain(text: impl Into<String>) -> StyledText {
    vec![Span {
        text: text.into(),
        style: Style::default(),
    }]
}

fn styled(text: impl Into<String>, add: Modifiers) -> StyledText {
    vec![Span {
        text: text.into(),
        style: Style {
            add,
            ..Style::default()
        },
    }]
}

/// A reference narrower than this many cells uses the short mouse and history lines.
const COMPACT_BELOW: usize = 60;

/// The two lines after the bindings: what the mouse does and how the terminal history is
/// reached. Neither is a key binding, so the keymap never lists them. A narrow reference gets the
/// short forms.
pub fn mouse_lines(width: usize) -> [&'static str; 2] {
    if width < COMPACT_BELOW {
        [
            "Mouse: focus · menus · tree reorder",
            "History: wheel / Shift+PgUp/PgDn",
        ]
    } else {
        [
            "Mouse: pane focus · tree expand/reorder · context menus · tree footer Settings",
            "Terminal history: wheel / Shift+PgUp/PgDn · Shift+End live · Ctrl+End app",
        ]
    }
}

/// The lines of the key reference for a view `columns` cells wide: the title, the bindings
/// grouped under their context's heading in first-seen order (one row per binding with a chord, its
/// first chord only), the mouse and history lines, then the footer. `label` gives a chord's
/// display form. Every line is at most `columns` less the frame wide.
pub fn reference_lines(
    bindings: &[Binding],
    mut label: impl FnMut(&str) -> String,
    footer: Option<String>,
    columns: u16,
) -> Vec<StyledText> {
    let width = usize::from(columns.saturating_sub(FRAME_COLUMNS));
    let mut lines = vec![
        styled(truncate_to_width(TITLE, width), Modifiers::BOLD),
        plain(""),
    ];
    // Contexts with the same heading (`leader` and `prefix`) share one group.
    let mut headings: Vec<String> = Vec::new();
    for binding in bindings {
        let heading = context_heading(&binding.context);
        if !binding.chords.is_empty() && !headings.contains(&heading) {
            headings.push(heading);
        }
    }
    for heading in headings {
        let members = bindings
            .iter()
            .filter(|binding| context_heading(&binding.context) == heading);
        lines.push(styled(truncate_to_width(&heading, width), Modifiers::BOLD));
        for binding in members {
            let Some(chord) = binding.chords.first() else {
                continue;
            };
            let chord = chord_text(&binding.context, chord, &label(chord));
            lines.push(plain(leader_line(
                &command_label(&binding.command),
                &chord,
                width,
            )));
        }
    }
    lines.push(plain(""));
    for line in mouse_lines(width) {
        lines.push(plain(truncate_to_width(line, width)));
    }
    if let Some(footer) = footer {
        lines.push(plain(""));
        lines.push(styled(truncate_to_width(&footer, width), Modifiers::DIM));
    }
    lines
}

/// The view tree of the `keys` panel: one scrollable paragraph, so a long key map scrolls
/// with a scrollbar in the host.
pub fn reference_tree(lines: Vec<StyledText>) -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![UiNode {
            key: Some("keys".to_owned()),
            widget: Widget::Paragraph(ParagraphSpec {
                lines,
                rich: None,
                wrap: Some(false),
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: true,
            }),
            children: Vec::new(),
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }],
        root: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: PORT:ilium-client/src/help.rs::help_summary_truncation_respects_terminal_cells_and_marks_omissions
    #[test]
    fn help_summary_truncation_respects_terminal_cells_and_marks_omissions() {
        assert_eq!(truncate_to_width("move workspace pane", 6), "move …");
        assert_eq!(truncate_to_width("界面設定", 5), "界面…");
        assert_eq!(truncate_to_width("ok", 2), "ok");
        assert_eq!(truncate_to_width("long", 0), "");
    }

    #[test]
    fn a_row_never_exceeds_its_width_and_aligns_the_summaries() {
        for total in [8, 20, 40] {
            let line = row_line("Ctrl+B c", "Create a new terminal pane", 10, total);
            assert!(
                UnicodeWidthStr::width(line.as_str()) <= total.max(12),
                "{line}"
            );
        }
        let line = row_line("a", "x", 4, 20);
        assert_eq!(line, "a     x");
    }

    fn binding(context: &str, command: &str, chord: &str) -> Binding {
        Binding {
            command: command.to_owned(),
            chords: vec![chord.to_owned()],
            context: context.to_owned(),
            is_default: true,
        }
    }

    fn texts(lines: &[StyledText]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn leaders_fill_a_wide_gap_and_a_narrow_one_stays_blank() {
        assert_eq!(leader_line("Help", "Prefix  ?", 20), "Help …………… Prefix  ?");
        assert_eq!(leader_line("Help", "Prefix  ?", 15), "Help  Prefix  ?");
        // Too long a label is cut; the chord label stays whole.
        let line = leader_line("Split the focused pane vertically", "Prefix  %", 20);
        assert!(line.ends_with(" Prefix  %"), "{line}");
        assert!(UnicodeWidthStr::width(line.as_str()) <= 20, "{line}");
    }

    #[test]
    fn kernel_bindings_read_as_words_under_readable_headings() {
        let bindings = vec![
            binding("leader", "ilium/help", "?"),
            binding("leader", "ilium/split-vertical", "%"),
            binding("prefix", "ilium.core.keys-and-help/show-keys", "f1"),
            binding("global", "acme/find", "ctrl+f"),
        ];
        let lines = reference_lines(
            &bindings,
            |chord| match chord {
                "f1" => "F1".to_owned(),
                "ctrl+f" => "Ctrl+F".to_owned(),
                other => other.to_owned(),
            },
            footer(r#"{"shortcut_base":"ctrl+b","preset":"tmux"}"#),
            40,
        );
        let texts = texts(&lines);
        assert_eq!(texts[0], TITLE);
        assert_eq!(texts[2], "Prefix keys");
        assert!(texts[3].starts_with("Help ") && texts[3].ends_with(" Prefix  ?"));
        assert!(texts[4].starts_with("Split vertical "));
        // `leader` and `prefix` bindings share the one Prefix keys group.
        assert!(texts[5].starts_with("Show keys ") && texts[5].ends_with(" Prefix  F1"));
        assert_eq!(texts[6], "Global keys");
        assert!(texts[7].starts_with("Find ") && texts[7].ends_with(" Ctrl+F"));
        assert_eq!(
            texts.last().map(String::as_str),
            Some("Prefix is Ctrl+B. Preset: tmux.")
        );
        for row in &texts {
            assert!(UnicodeWidthStr::width(row.as_str()) <= 38, "{row}");
        }
    }

    #[test]
    fn mouse_and_history_lines_follow_the_width_and_never_overrun_it() {
        let wide = reference_lines(&[], |chord| chord.to_owned(), None, 120);
        let wide = texts(&wide);
        assert!(wide.iter().any(|row| row.starts_with("Mouse: pane focus")));
        assert!(wide
            .iter()
            .any(|row| row.ends_with("Shift+End live · Ctrl+End app")));
        let narrow = reference_lines(&[], |chord| chord.to_owned(), None, 40);
        let narrow = texts(&narrow);
        assert!(narrow.contains(&"Mouse: focus · menus · tree reorder".to_owned()));
        assert!(narrow.contains(&"History: wheel / Shift+PgUp/PgDn".to_owned()));
        for columns in [10u16, 14, 24, 40, 62, 80] {
            let lines = reference_lines(&[], |chord| chord.to_owned(), None, columns);
            for row in texts(&lines) {
                assert!(
                    UnicodeWidthStr::width(row.as_str()) <= usize::from(columns - FRAME_COLUMNS),
                    "{columns}: {row}"
                );
            }
        }
    }

    #[test]
    fn the_footer_names_the_prefix_and_a_known_preset() {
        assert_eq!(
            footer(r#"{"shortcut_base":"ctrl+a","preset":"screen"}"#).as_deref(),
            Some("Prefix is Ctrl+A. Preset: GNU Screen.")
        );
        assert_eq!(
            footer(r#"{"shortcut_base":"ctrl-g","preset":"custom"}"#).as_deref(),
            Some("Prefix is Ctrl+G.")
        );
        assert_eq!(footer("{}"), None);
        assert_eq!(footer("not json"), None);
    }

    #[test]
    fn the_tree_is_one_scrollable_keyed_paragraph() {
        let tree = reference_tree(vec![plain("x")]);
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.nodes[0].key.as_deref(), Some("keys"));
        assert!(matches!(
            &tree.nodes[0].widget,
            Widget::Paragraph(spec) if spec.scrollable
        ));
    }
}
