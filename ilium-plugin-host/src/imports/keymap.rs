//! Host implementation of `ilium:host/keymap`.

use ilium_plugin_api::generated::host::keymap::{Binding, HostKeymap};

/// Default kernel bindings: command, chords, context.
const DEFAULT_BINDINGS: [(&str, &[&str], &str); 6] = [
    ("pane.new", &["ctrl+shift+n"], "global"),
    ("pane.close", &["ctrl+shift+w"], "global"),
    ("pane.focus-next", &["alt+right", "alt+down"], "global"),
    ("pane.focus-previous", &["alt+left", "alt+up"], "global"),
    ("help.open", &["f1"], "global"),
    ("settings.open", &["ctrl+,"], "global"),
];

/// Host side of the `keymap` interface.
#[derive(Debug)]
pub struct KeymapImport;

impl HostKeymap for KeymapImport {
    fn bindings(&mut self) -> Vec<Binding> {
        DEFAULT_BINDINGS
            .iter()
            .map(|(command, chords, context)| Binding {
                command: (*command).to_string(),
                chords: chords.iter().map(|chord| (*chord).to_string()).collect(),
                context: (*context).to_string(),
                is_default: true,
            })
            .collect()
    }

    fn chord_label(&mut self, chord: String) -> String {
        chord
            .split('+')
            .map(label_part)
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// Display text of one chord part (modifier or key).
fn label_part(part: &str) -> String {
    match part {
        "up" => "\u{2191}".to_string(),
        "down" => "\u{2193}".to_string(),
        "left" => "\u{2190}".to_string(),
        "right" => "\u{2192}".to_string(),
        "page-up" => "Pg\u{2191}".to_string(),
        "page-down" => "Pg\u{2193}".to_string(),
        "plus" => "+".to_string(),
        _ => {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect(),
                None => String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeymapImport;
    use ilium_plugin_api::generated::host::keymap::{Binding, HostKeymap};

    /// Modifiers allowed in front of the key, per the chord grammar `chord = *(modifier "+") key`.
    const CHORD_MODIFIERS: [&str; 4] = ["ctrl", "alt", "shift", "super"];

    /// Named keys from the chord grammar; `f1` to `f24` are checked separately.
    const CHORD_NAMED_KEYS: [&str; 16] = [
        "enter",
        "escape",
        "tab",
        "backspace",
        "delete",
        "insert",
        "home",
        "end",
        "page-up",
        "page-down",
        "up",
        "down",
        "left",
        "right",
        "space",
        "plus",
    ];

    /// Returns true when `chord` matches the chord syntax of the keys page: lower case,
    /// zero or more modifiers joined by `+`, then exactly one key.
    fn is_valid_chord(chord: &str) -> bool {
        let parts: Vec<&str> = chord.split('+').collect();
        let Some((key, modifiers)) = parts.split_last() else {
            return false;
        };
        let all_modifiers_known = modifiers
            .iter()
            .all(|modifier| CHORD_MODIFIERS.contains(modifier));
        all_modifiers_known && is_valid_key(key)
    }

    fn is_valid_key(key: &str) -> bool {
        if CHORD_NAMED_KEYS.contains(&key) {
            return true;
        }
        if let Some(number) = key.strip_prefix('f') {
            if let Ok(function_number) = number.parse::<u32>() {
                return (1..=24).contains(&function_number)
                    && number == function_number.to_string();
            }
        }
        let mut characters = key.chars();
        match (characters.next(), characters.next()) {
            (Some(character), None) => {
                !character.is_whitespace() && !character.is_control() && !character.is_uppercase()
            }
            _ => false,
        }
    }

    /// The keymap is a kernel part; `bindings` returns the kernel's live bindings, each naming
    /// a command with at least one chord (manual: api-kernel-settings-and-keymap, `binding` record).
    /// REQ: COR-010
    #[test]
    fn bindings_each_entry_names_a_command_and_at_least_one_chord() {
        let mut import = KeymapImport;
        let bindings: Vec<Binding> = import.bindings();
        for binding in &bindings {
            assert!(
                !binding.command.is_empty(),
                "binding with empty command: {binding:?}"
            );
            assert!(
                !binding.chords.is_empty(),
                "binding without chords: {binding:?}"
            );
        }
    }

    /// Every chord in `binding.chords` uses the chord syntax of the keys page (api-keys, "Chord syntax").
    /// REQ: COR-010
    #[test]
    fn bindings_chords_follow_the_chord_grammar() {
        let mut import = KeymapImport;
        let bindings: Vec<Binding> = import.bindings();
        for binding in &bindings {
            for chord in &binding.chords {
                assert!(
                    is_valid_chord(chord),
                    "chord {chord:?} of {} breaks the chord grammar",
                    binding.command
                );
            }
        }
    }

    /// Documented example: the chord `ctrl+shift+k` is shown to the user as `Ctrl+Shift+K`
    /// (api-kernel-settings-and-keymap, `chord-label`; api-keys, "Chord syntax").
    /// REQ: COR-010
    #[test]
    fn chord_label_formats_modifiers_and_key_with_capitals() {
        let mut import = KeymapImport;
        assert_eq!(
            import.chord_label("ctrl+shift+k".to_string()),
            "Ctrl+Shift+K"
        );
    }

    /// Arrow keys show as the arrow glyphs in the client's help (api-keys, "Chord syntax").
    /// REQ: COR-010
    #[test]
    fn chord_label_shows_arrow_keys_as_glyphs() {
        let mut import = KeymapImport;
        let cases = [("up", "↑"), ("down", "↓"), ("left", "←"), ("right", "→")];
        for (chord, expected_label) in cases {
            assert_eq!(
                import.chord_label(chord.to_string()),
                expected_label,
                "chord {chord}"
            );
        }
    }

    /// Page keys show as `Pg↑` and `Pg↓` in the client's help (api-keys, "Chord syntax").
    /// REQ: COR-010
    #[test]
    fn chord_label_shows_page_keys_with_pg_prefix() {
        let mut import = KeymapImport;
        assert_eq!(import.chord_label("page-up".to_string()), "Pg↑");
        assert_eq!(import.chord_label("page-down".to_string()), "Pg↓");
    }

    /// The label of a chord with one modifier keeps the same capitals as the documented example
    /// (`alt+c` is written in lower case in the grammar; its label starts with `Alt+`).
    /// REQ: COR-010
    #[test]
    fn chord_label_capitalises_a_single_modifier() {
        let mut import = KeymapImport;
        let label = import.chord_label("alt+c".to_string());
        assert!(label.starts_with("Alt+"), "label was {label:?}");
    }
}
