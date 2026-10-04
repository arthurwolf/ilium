//! Constant-space exact line classification and checkbox parsing on CPU pages.
use crate::minimap::LineKind;
#[derive(Clone, Copy, Debug)]
enum Ordered {
    Initial,
    Digits,
    Separator,
    List,
    Other,
}
#[derive(Clone, Copy, Debug)]
enum Checkbox {
    Spaces,
    Digits,
    Space,
    Bracket,
    Value,
    Close(bool),
    Found(usize, bool),
    Other,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct LineFacts {
    pub chars: usize,
    pub leading: usize,
    first: [char; 3],
    prefix: usize,
    started: bool,
    ordered: Ordered,
    checkbox: Checkbox,
    bracket: usize,
}
impl Default for LineFacts {
    fn default() -> Self {
        Self {
            chars: 0,
            leading: 0,
            first: ['\0'; 3],
            prefix: 0,
            started: false,
            ordered: Ordered::Initial,
            checkbox: Checkbox::Spaces,
            bracket: 0,
        }
    }
}
impl LineFacts {
    pub fn accept(&mut self, ch: char) {
        if !self.started && ch.is_whitespace() {
            self.leading += 1;
        } else {
            if !self.started {
                self.started = true;
            }
            if self.prefix < 3 {
                self.first[self.prefix] = ch;
                self.prefix += 1;
            }
            self.ordered = match (self.ordered, ch) {
                (Ordered::Initial, ch) if ch.is_ascii_digit() => Ordered::Digits,
                (Ordered::Initial, '-' | '*' | '+') => Ordered::Separator,
                (Ordered::Digits, ch) if ch.is_ascii_digit() => Ordered::Digits,
                (Ordered::Digits, '.' | ')') => Ordered::Separator,
                (Ordered::Separator, ' ') => Ordered::List,
                (Ordered::List, _) => Ordered::List,
                _ => Ordered::Other,
            };
        }
        self.checkbox = match (self.checkbox, ch) {
            (Checkbox::Spaces, ' ') => Checkbox::Spaces,
            (Checkbox::Spaces, '-' | '*' | '+') => Checkbox::Space,
            (Checkbox::Spaces, ch) if ch.is_ascii_digit() => Checkbox::Digits,
            (Checkbox::Digits, ch) if ch.is_ascii_digit() => Checkbox::Digits,
            (Checkbox::Digits, '.' | ')') => Checkbox::Space,
            (Checkbox::Space, ' ') => Checkbox::Bracket,
            (Checkbox::Bracket, '[') => {
                self.bracket = self.chars;
                Checkbox::Value
            }
            (Checkbox::Value, ' ') => Checkbox::Close(false),
            (Checkbox::Value, 'x' | 'X') => Checkbox::Close(true),
            (Checkbox::Close(value), ']') => Checkbox::Found(self.bracket, value),
            (Checkbox::Found(column, value), _) => Checkbox::Found(column, value),
            _ => Checkbox::Other,
        };
        self.chars += 1;
    }
    pub fn kind(&self) -> LineKind {
        if !self.started {
            return LineKind::Blank;
        }
        if self.first[0] == '#' {
            return LineKind::Heading;
        }
        if self.prefix == 3 && (self.first == ['`'; 3] || self.first == ['~'; 3]) {
            return LineKind::Code;
        }
        if self.first[0] == '>' {
            return LineKind::Quote;
        }
        if matches!(self.ordered, Ordered::List | Ordered::Separator) {
            return LineKind::ListItem;
        }
        LineKind::Text
    }
    pub fn checkbox(&self) -> Option<(usize, bool)> {
        match self.checkbox {
            Checkbox::Found(column, value) => Some((column, value)),
            _ => None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_facts_match_real_classification_and_checkbox_oracles() {
        for line in [
            "",
            "   ",
            "\t  # heading",
            " ```rust",
            "~~~",
            " > quote",
            "*emphasis*",
            "*",
            "2024 prose",
            "12)",
            "12) text",
            "- [ ] body",
            "  12) [X] body",
            "\t- [x] not checkbox",
        ] {
            let mut facts = LineFacts::default();
            for ch in line.chars() {
                facts.accept(ch);
            }
            assert_eq!(facts.kind(), crate::minimap::classify_line(line), "{line}");
            assert_eq!(
                facts.checkbox(),
                crate::markdown::checkbox::find_checkbox(line),
                "{line}"
            );
            assert_eq!(facts.chars, line.chars().count());
        }
        let line = format!("{}1) [x] body", " ".repeat(70000));
        let mut facts = LineFacts::default();
        for ch in line.chars() {
            facts.accept(ch);
        }
        assert_eq!(facts.kind(), crate::minimap::classify_line(&line));
        assert_eq!(
            facts.checkbox(),
            crate::markdown::checkbox::find_checkbox(&line)
        );
    }
}
