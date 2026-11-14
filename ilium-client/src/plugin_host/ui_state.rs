//! Host-owned state of the editable widgets of a plugin view (gap G8).
//!
//! After the first render the host owns the contents and the caret of a `text-input`, the typed
//! entry of a `number`, the first visible row of a `scroll-view` and the divider of a `split`
//! (widgets-input and widgets-layout-and-containers pages). This module is pure: it holds the
//! state and the editing rules, and `view` draws it and turns keys and clicks into `ui-action`s.
//!
//! The `ui-host` import of the plugin host keeps its own copy of the widget state for
//! `get-widget-state` and `set-widget-state`; the client side of that exchange is not wired yet.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ilium_plugin_api::generated::types::ui::{NumberSpec, TextInputSpec};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Longest text of one input, in characters, when the plugin sets no `max-length` (64 KiB cap).
pub(crate) const TEXT_CAP_CHARS: usize = 65_536;
/// A bracketed paste is cut to this many bytes (guide-text-input-and-composition, PDT rules).
pub(crate) const PASTE_CAP_BYTES: usize = 1_048_576;
/// Longest debounce a widget may ask for.
pub(crate) const DEBOUNCE_CAP_MS: u32 = 2_000;
/// Most trailing characters a secret input may keep visible.
const REVEAL_LAST_CAP: u8 = 8;
/// What a hidden character of a secret input shows.
const SECRET_GLYPH: &str = "•";

fn debounce(milliseconds: u32) -> Duration {
    Duration::from_millis(u64::from(milliseconds.min(DEBOUNCE_CAP_MS)))
}

/// Bidirectional controls a paste must not smuggle into a value (the bidi fixture of the manual).
pub(crate) fn is_bidi_control(character: char) -> bool {
    matches!(character, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

pub(crate) fn previous_boundary(text: &str, caret: usize) -> usize {
    text[..caret]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(start, _)| start)
}

pub(crate) fn next_boundary(text: &str, caret: usize) -> usize {
    text[caret..]
        .graphemes(true)
        .next()
        .map_or(text.len(), |cluster| caret + cluster.len())
}

/// Start of the word before the caret: trailing blanks first, then the word itself.
pub(crate) fn word_start(text: &str, caret: usize) -> usize {
    let mut start = caret;
    let mut seen_word = false;
    for (index, character) in text[..caret].char_indices().rev() {
        if character.is_whitespace() {
            if seen_word {
                break;
            }
        } else {
            seen_word = true;
        }
        start = index;
    }
    start
}

/// What one key did to a text input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditOutcome {
    /// The key means nothing to a text input; the view may use it.
    Ignored,
    /// The caret, the selection or the reveal switch moved; the text is the same.
    Moved,
    /// The text changed.
    Edited,
    /// Enter, with `submit-on-enter`.
    Submitted,
}

/// One visible character cluster of an input, as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputCell {
    pub text: String,
    pub selected: bool,
}

/// An input laid out in a window of cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InputLayout {
    pub cells: Vec<InputCell>,
    /// Column of the caret inside the window.
    pub caret_column: u16,
    /// First column and byte offset of every visible cluster.
    pub starts: Vec<(u16, usize)>,
    /// Column after the last visible cluster.
    pub end_column: u16,
    /// Whether the window shows the end of the text.
    pub reaches_end: bool,
}

/// The editable state of one `text-input`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TextEdit {
    text: String,
    /// Byte offset of the caret, always on a cluster boundary.
    caret: usize,
    /// The other end of the selection, when there is one.
    anchor: Option<usize>,
    revealed: bool,
    /// When the plugin is due to hear `changed`.
    due: Option<Instant>,
}

impl TextEdit {
    /// The state of an input the user has not touched: `initial` with the caret at its end.
    pub(crate) fn seeded(initial: &str) -> TextEdit {
        let text: String = initial
            .chars()
            .filter(|character| !character.is_control() && !is_bidi_control(*character))
            .take(TEXT_CAP_CHARS)
            .collect();
        TextEdit {
            caret: text.len(),
            text,
            ..TextEdit::default()
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the whole text, as `set-widget-state` does; the caret goes to the end.
    pub(crate) fn set_text(&mut self, text: &str) {
        let revealed = self.revealed;
        *self = TextEdit::seeded(text);
        self.revealed = revealed;
    }

    /// The selected byte range, ordered.
    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .map(|anchor| (anchor.min(self.caret), anchor.max(self.caret)))
            .filter(|(start, end)| start != end)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        true
    }

    fn move_caret(&mut self, to: usize, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }

    /// Inserts committed text at the caret, replacing the selection. `whole` refuses a commit
    /// that does not fit entirely; otherwise the longest prefix that fits goes in.
    fn insert(&mut self, incoming: &str, max_length: u32, whole: bool) -> bool {
        let clean: String = incoming
            .chars()
            .filter(|character| !character.is_control() && !is_bidi_control(*character))
            .collect();
        if clean.is_empty() {
            return false;
        }
        let limit = if max_length == 0 {
            TEXT_CAP_CHARS
        } else {
            usize::try_from(max_length)
                .unwrap_or(TEXT_CAP_CHARS)
                .min(TEXT_CAP_CHARS)
        };
        let selected_chars = self
            .selection()
            .map_or(0, |(start, end)| self.text[start..end].chars().count());
        let room = limit.saturating_sub(self.text.chars().count().saturating_sub(selected_chars));
        let mut piece = String::new();
        let mut used = 0usize;
        for cluster in clean.graphemes(true) {
            let count = cluster.chars().count();
            if used + count > room {
                break;
            }
            piece.push_str(cluster);
            used += count;
        }
        if piece.is_empty() || (whole && used < clean.chars().count()) {
            return false;
        }
        self.delete_selection();
        self.text.insert_str(self.caret, &piece);
        self.caret += piece.len();
        true
    }

    /// Applies one key. `now` starts the debounce of an edit.
    pub(crate) fn apply_key(
        &mut self,
        key: KeyEvent,
        spec: &TextInputSpec,
        now: Instant,
    ) -> EditOutcome {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let outcome = match key.code {
            KeyCode::Char(character) if control => match character.to_ascii_lowercase() {
                'a' => {
                    self.move_caret(0, shift);
                    EditOutcome::Moved
                }
                'e' => {
                    let end = self.text.len();
                    self.move_caret(end, shift);
                    EditOutcome::Moved
                }
                'u' => {
                    let caret = self.caret;
                    if caret == 0 {
                        EditOutcome::Ignored
                    } else {
                        self.text.replace_range(..caret, "");
                        self.caret = 0;
                        self.anchor = None;
                        EditOutcome::Edited
                    }
                }
                'w' => {
                    if self.delete_selection() {
                        EditOutcome::Edited
                    } else {
                        let start = word_start(&self.text, self.caret);
                        if start == self.caret {
                            EditOutcome::Ignored
                        } else {
                            let caret = self.caret;
                            self.text.replace_range(start..caret, "");
                            self.caret = start;
                            EditOutcome::Edited
                        }
                    }
                }
                'r' if spec.reveal => {
                    self.revealed = !self.revealed;
                    EditOutcome::Moved
                }
                _ => EditOutcome::Ignored,
            },
            KeyCode::Char(_) if alt => EditOutcome::Ignored,
            KeyCode::Char(character) => {
                let mut buffer = [0u8; 4];
                if self.insert(character.encode_utf8(&mut buffer), spec.max_length, true) {
                    EditOutcome::Edited
                } else {
                    EditOutcome::Ignored
                }
            }
            KeyCode::Backspace => {
                if self.delete_selection() {
                    EditOutcome::Edited
                } else if self.caret > 0 {
                    let start = previous_boundary(&self.text, self.caret);
                    let caret = self.caret;
                    self.text.replace_range(start..caret, "");
                    self.caret = start;
                    EditOutcome::Edited
                } else {
                    EditOutcome::Ignored
                }
            }
            KeyCode::Delete => {
                if self.delete_selection() {
                    EditOutcome::Edited
                } else if self.caret < self.text.len() {
                    let end = next_boundary(&self.text, self.caret);
                    let caret = self.caret;
                    self.text.replace_range(caret..end, "");
                    EditOutcome::Edited
                } else {
                    EditOutcome::Ignored
                }
            }
            KeyCode::Left => {
                match (self.selection(), shift) {
                    (Some((start, _)), false) => {
                        self.anchor = None;
                        self.caret = start;
                    }
                    _ => {
                        let to = previous_boundary(&self.text, self.caret);
                        self.move_caret(to, shift);
                    }
                }
                EditOutcome::Moved
            }
            KeyCode::Right => {
                match (self.selection(), shift) {
                    (Some((_, end)), false) => {
                        self.anchor = None;
                        self.caret = end;
                    }
                    _ => {
                        let to = next_boundary(&self.text, self.caret);
                        self.move_caret(to, shift);
                    }
                }
                EditOutcome::Moved
            }
            KeyCode::Home => {
                self.move_caret(0, shift);
                EditOutcome::Moved
            }
            KeyCode::End => {
                let end = self.text.len();
                self.move_caret(end, shift);
                EditOutcome::Moved
            }
            KeyCode::Enter if spec.submit_on_enter => EditOutcome::Submitted,
            _ => EditOutcome::Ignored,
        };
        if outcome == EditOutcome::Edited {
            self.due = Some(now + debounce(spec.debounce_ms));
        }
        outcome
    }

    /// A bracketed paste: one change, cut to 1 MiB and to the room the field has left.
    pub(crate) fn paste(&mut self, pasted: &str, spec: &TextInputSpec, now: Instant) -> bool {
        let mut end = pasted.len().min(PASTE_CAP_BYTES);
        while !pasted.is_char_boundary(end) {
            end -= 1;
        }
        let edited = self.insert(&pasted[..end], spec.max_length, false);
        if edited {
            self.due = Some(now + debounce(spec.debounce_ms));
        }
        edited
    }

    /// Puts the caret at the cluster under `column` of the window the input is drawn in.
    pub(crate) fn place_caret(&mut self, spec: &TextInputSpec, width: usize, column: u16) {
        let layout = self.layout(spec, width);
        let mut target = None;
        for (start, byte) in &layout.starts {
            if *start <= column {
                target = Some(*byte);
            }
        }
        let byte = match target {
            Some(_) if column >= layout.end_column && layout.reaches_end => self.text.len(),
            Some(byte) => byte,
            None => 0,
        };
        self.anchor = None;
        self.caret = byte.min(self.text.len());
    }

    /// The text to send once the debounce has run out, if an edit is waiting.
    pub(crate) fn take_due(&mut self, now: Instant) -> Option<String> {
        match self.due {
            Some(due) if due <= now => {
                self.due = None;
                Some(self.text.clone())
            }
            _ => None,
        }
    }

    /// The text of an edit that has not been sent yet, whatever its debounce says.
    pub(crate) fn flush(&mut self) -> Option<String> {
        self.due.take().map(|_| self.text.clone())
    }

    /// When the next unsent change falls due.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.due
    }

    /// Whether `value` matches the plugin's `validation` expression. No expression, or one
    /// that does not compile, accepts everything.
    pub(crate) fn is_valid(&self, spec: &TextInputSpec) -> bool {
        let Some(pattern) = spec.validation.as_deref() else {
            return true;
        };
        match regex::RegexBuilder::new(pattern)
            .size_limit(1 << 20)
            .build()
        {
            Ok(expression) => expression.is_match(&self.text),
            Err(_) => true,
        }
    }

    /// The input laid out in `width` cells, scrolled so the caret is visible. A secret shows
    /// one `•` per hidden cluster and the last `reveal-last` of them, unless revealed.
    pub(crate) fn layout(&self, spec: &TextInputSpec, width: usize) -> InputLayout {
        let clusters: Vec<(usize, &str)> = self.text.grapheme_indices(true).collect();
        let count = clusters.len();
        let keep = usize::from(spec.reveal_last.min(REVEAL_LAST_CAP));
        let hide = spec.secret && !self.revealed;
        let shown: Vec<String> = clusters
            .iter()
            .enumerate()
            .map(|(index, (_, cluster))| {
                if hide && index + keep < count {
                    SECRET_GLYPH.to_owned()
                } else {
                    (*cluster).to_owned()
                }
            })
            .collect();
        let widths: Vec<usize> = shown
            .iter()
            .map(|cluster| UnicodeWidthStr::width(cluster.as_str()))
            .collect();
        let caret_index = clusters
            .iter()
            .take_while(|(byte, _)| *byte < self.caret)
            .count();
        let room = width.max(1);
        let mut first = caret_index;
        let mut used = 0usize;
        while first > 0 && used + widths[first - 1] < room {
            first -= 1;
            used += widths[first];
        }
        let selection = self.selection();
        let mut layout = InputLayout {
            caret_column: u16::try_from(used).unwrap_or(u16::MAX),
            reaches_end: true,
            ..InputLayout::default()
        };
        let mut column = 0usize;
        for (((byte, _), width_of), text) in clusters.iter().zip(&widths).zip(&shown).skip(first) {
            if column + width_of > room {
                layout.reaches_end = false;
                break;
            }
            layout
                .starts
                .push((u16::try_from(column).unwrap_or(u16::MAX), *byte));
            layout.cells.push(InputCell {
                text: text.clone(),
                selected: selection.is_some_and(|(start, end)| *byte >= start && *byte < end),
            });
            column += width_of;
        }
        layout.end_column = u16::try_from(column).unwrap_or(u16::MAX);
        layout
    }
}

/// What one key did to a number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NumberOutcome {
    /// The key means nothing to a number.
    Ignored,
    /// The typed entry changed or opened; nothing is sent yet.
    Typing,
    /// The value changed and the plugin hears it now.
    Changed(f64),
}

/// The editable state of one `number`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NumberEdit {
    pub value: f64,
    /// The digits being typed, when the entry is open.
    pub typed: Option<String>,
    due: Option<Instant>,
}

/// Divisor of the displayed value: a `scale` of 0 counts as 1.
fn scale_of(spec: &NumberSpec) -> f64 {
    if spec.scale == 0 {
        1.0
    } else {
        f64::from(spec.scale)
    }
}

pub(crate) fn bounds(spec: &NumberSpec) -> (f64, f64) {
    let low = if spec.minimum.is_finite() {
        spec.minimum
    } else {
        0.0
    };
    let high = if spec.maximum.is_finite() {
        spec.maximum
    } else {
        100.0
    };
    (low.min(high), low.max(high))
}

/// Keeps a value inside the bounds of the spec; a value that is not a number becomes the minimum.
pub(crate) fn clamp_number(spec: &NumberSpec, value: f64) -> f64 {
    let (low, high) = bounds(spec);
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        low
    }
}

/// The value one step away: the next rung of `steps`, or `step` added; it never wraps.
pub(crate) fn step_number(spec: &NumberSpec, value: f64, direction: i32) -> f64 {
    let current = clamp_number(spec, value);
    let epsilon = 1e-9;
    if !spec.steps.is_empty() {
        let mut ladder: Vec<f64> = spec
            .steps
            .iter()
            .copied()
            .filter(|rung| rung.is_finite())
            .collect();
        ladder.sort_by(f64::total_cmp);
        let next = if direction >= 0 {
            ladder
                .iter()
                .copied()
                .find(|rung| *rung > current + epsilon)
        } else {
            ladder
                .iter()
                .rev()
                .copied()
                .find(|rung| *rung < current - epsilon)
        };
        return clamp_number(spec, next.unwrap_or(current));
    }
    let step = if spec.step.is_finite() && spec.step > 0.0 {
        spec.step
    } else {
        1.0
    };
    let moved = if direction >= 0 {
        current + step
    } else {
        current - step
    };
    // Round away the error that repeated float steps pile up.
    let rounded = (moved * 1e9).round() / 1e9;
    clamp_number(spec, rounded)
}

/// The value at cell `column` of a slider track `width` cells wide, snapped to the nearest step.
pub(crate) fn slider_number(spec: &NumberSpec, column: u16, width: u16) -> f64 {
    let (low, high) = bounds(spec);
    if width < 2 {
        return low;
    }
    let ratio = f64::from(column.min(width - 1)) / f64::from(width - 1);
    let raw = low + ratio * (high - low);
    if !spec.steps.is_empty() {
        let nearest = spec
            .steps
            .iter()
            .copied()
            .filter(|rung| rung.is_finite())
            .min_by(|a, b| (a - raw).abs().total_cmp(&(b - raw).abs()));
        return clamp_number(spec, nearest.unwrap_or(raw));
    }
    let step = if spec.step.is_finite() && spec.step > 0.0 {
        spec.step
    } else {
        1.0
    };
    let snapped = low + ((raw - low) / step).round() * step;
    clamp_number(spec, (snapped * 1e9).round() / 1e9)
}

/// The value as drawn: a special word, or the number divided by `scale` with its unit.
pub(crate) fn format_number(spec: &NumberSpec, value: f64) -> String {
    if let Some((_, word)) = spec
        .special_values
        .iter()
        .find(|(special, _)| (special - value).abs() < 1e-9)
    {
        return word.clone();
    }
    let shown = value / scale_of(spec);
    let mut text = if shown.is_finite() {
        format!("{shown:.6}")
    } else {
        "0".to_owned()
    };
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_owned();
    }
    match spec.unit.as_deref() {
        Some(unit) if !unit.is_empty() => format!("{text} {unit}"),
        _ => text,
    }
}

/// The stored value of typed digits: typed `1.7` with a `scale` of 10 is 17, clamped to the bounds.
pub(crate) fn parse_typed(spec: &NumberSpec, typed: &str) -> Option<f64> {
    let parsed: f64 = typed.trim().parse().ok()?;
    parsed
        .is_finite()
        .then(|| clamp_number(spec, parsed * scale_of(spec)))
}

impl NumberEdit {
    pub(crate) fn seeded(spec: &NumberSpec) -> NumberEdit {
        NumberEdit {
            value: clamp_number(spec, spec.value),
            typed: None,
            due: None,
        }
    }

    fn type_in(&mut self, character: char, spec: &NumberSpec, now: Instant) -> NumberOutcome {
        let buffer = self.typed.get_or_insert_with(String::new);
        let accepted = match character {
            '0'..='9' => true,
            '.' => !buffer.contains('.'),
            '-' => buffer.is_empty(),
            _ => false,
        };
        if !accepted || buffer.len() >= 24 {
            return NumberOutcome::Ignored;
        }
        buffer.push(character);
        self.due = Some(now + debounce(spec.debounce_ms));
        NumberOutcome::Typing
    }

    /// Applies one key; Enter or `*` opens the entry, a second Enter commits it.
    pub(crate) fn apply_key(
        &mut self,
        key: KeyEvent,
        spec: &NumberSpec,
        now: Instant,
    ) -> NumberOutcome {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return NumberOutcome::Ignored;
        }
        match key.code {
            KeyCode::Enter => match self.typed.take() {
                None => {
                    self.typed = Some(String::new());
                    NumberOutcome::Typing
                }
                Some(buffer) => {
                    self.due = None;
                    match parse_typed(spec, &buffer) {
                        Some(value) => {
                            self.value = value;
                            NumberOutcome::Changed(value)
                        }
                        None => NumberOutcome::Typing,
                    }
                }
            },
            KeyCode::Esc if self.typed.is_some() => {
                self.typed = None;
                self.due = None;
                NumberOutcome::Typing
            }
            KeyCode::Backspace if self.typed.is_some() => {
                if let Some(buffer) = self.typed.as_mut() {
                    buffer.pop();
                }
                self.due = Some(now + debounce(spec.debounce_ms));
                NumberOutcome::Typing
            }
            KeyCode::Char('*') if self.typed.is_none() => {
                self.typed = Some(String::new());
                NumberOutcome::Typing
            }
            KeyCode::Char(character) if self.typed.is_some() || character.is_ascii_digit() => {
                self.type_in(character, spec, now)
            }
            KeyCode::Left | KeyCode::Char('-') => self.step(spec, -1),
            KeyCode::Right | KeyCode::Char('+') => self.step(spec, 1),
            KeyCode::Home => {
                self.typed = None;
                self.set(spec, bounds(spec).0)
            }
            KeyCode::End => {
                self.typed = None;
                self.set(spec, bounds(spec).1)
            }
            _ => NumberOutcome::Ignored,
        }
    }

    fn set(&mut self, spec: &NumberSpec, value: f64) -> NumberOutcome {
        self.due = None;
        let value = clamp_number(spec, value);
        self.value = value;
        NumberOutcome::Changed(value)
    }

    /// One step up or down; the entry closes.
    pub(crate) fn step(&mut self, spec: &NumberSpec, direction: i32) -> NumberOutcome {
        self.typed = None;
        let value = step_number(spec, self.value, direction);
        self.set(spec, value)
    }

    /// Sets the value from the slider track.
    pub(crate) fn set_from_track(
        &mut self,
        spec: &NumberSpec,
        column: u16,
        width: u16,
    ) -> NumberOutcome {
        self.typed = None;
        let value = slider_number(spec, column, width);
        self.set(spec, value)
    }

    /// The typed digits as a value, once the debounce has run out.
    pub(crate) fn take_due(&mut self, spec: &NumberSpec, now: Instant) -> Option<f64> {
        match self.due {
            Some(due) if due <= now => {
                self.due = None;
                let value = parse_typed(spec, self.typed.as_deref()?)?;
                self.value = value;
                Some(value)
            }
            _ => None,
        }
    }

    /// A typed entry that has not been sent yet, committed now.
    pub(crate) fn flush(&mut self, spec: &NumberSpec) -> Option<f64> {
        self.due.take()?;
        let value = parse_typed(spec, self.typed.as_deref()?)?;
        self.value = value;
        Some(value)
    }

    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.due
    }
}

/// The first visible row of a `scroll-view`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ScrollState {
    pub offset: u16,
    /// The user scrolled away from the bottom, so `follow-tail` stops following.
    pub detached: bool,
}

/// The offset a scroll view is drawn at: the state, kept inside the content, or the last row
/// while `follow-tail` is following.
pub(crate) fn scroll_offset(
    state: Option<&ScrollState>,
    follow_tail: bool,
    max_offset: u16,
) -> u16 {
    match state {
        None if follow_tail => max_offset,
        None => 0,
        Some(state) if follow_tail && !state.detached => max_offset,
        Some(state) => state.offset.min(max_offset),
    }
}

/// A change the debounce has released: the widget key and what the plugin hears.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DueChange {
    Text(String, String),
    Number(String, f64),
}

/// The editable state of every widget of one plugin view, by widget key.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WidgetStates {
    pub texts: BTreeMap<String, TextEdit>,
    /// Multi-line `text-area`s; their changes reach the plugin as `DueChange::Text`.
    pub areas: BTreeMap<String, super::text_area::AreaEdit>,
    pub numbers: BTreeMap<String, NumberEdit>,
    pub scrolls: BTreeMap<String, ScrollState>,
    /// Divider positions in per mille.
    pub splits: BTreeMap<String, u16>,
    /// Number specs of the widgets the last composition drew, so a due entry can be parsed.
    pub number_specs: BTreeMap<String, NumberSpec>,
}

impl WidgetStates {
    /// The changes whose debounce has run out at `now`.
    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<DueChange> {
        let mut released = Vec::new();
        for (key, edit) in &mut self.texts {
            if let Some(text) = edit.take_due(now) {
                released.push(DueChange::Text(key.clone(), text));
            }
        }
        for (key, edit) in &mut self.areas {
            if let Some(text) = edit.take_due(now) {
                released.push(DueChange::Text(key.clone(), text));
            }
        }
        for (key, edit) in &mut self.numbers {
            if let Some(spec) = self.number_specs.get(key) {
                if let Some(value) = edit.take_due(spec, now) {
                    released.push(DueChange::Number(key.clone(), value));
                }
            }
        }
        released
    }

    /// Releases the unsent change of one widget now, as when the focus leaves it.
    pub(crate) fn flush(&mut self, key: &str) -> Option<DueChange> {
        if let Some(text) = self.texts.get_mut(key).and_then(TextEdit::flush) {
            return Some(DueChange::Text(key.to_owned(), text));
        }
        if let Some(text) = self
            .areas
            .get_mut(key)
            .and_then(super::text_area::AreaEdit::flush)
        {
            return Some(DueChange::Text(key.to_owned(), text));
        }
        let spec = self.number_specs.get(key)?;
        let value = self.numbers.get_mut(key)?.flush(spec)?;
        Some(DueChange::Number(key.to_owned(), value))
    }

    /// The soonest moment a change falls due, so the event loop can wake for it.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.texts
            .values()
            .filter_map(TextEdit::next_due)
            .chain(
                self.areas
                    .values()
                    .filter_map(super::text_area::AreaEdit::next_due),
            )
            .chain(self.numbers.values().filter_map(NumberEdit::next_due))
            .min()
    }

    /// Whether a number entry is open, which makes Escape cancel it instead of leaving the view.
    pub(crate) fn entry_open(&self) -> bool {
        self.numbers.values().any(|edit| edit.typed.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TextInputSpec {
        TextInputSpec {
            label: "Name".to_owned(),
            submit_on_enter: true,
            ..TextInputSpec::default()
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    fn typed(edit: &mut TextEdit, spec: &TextInputSpec, text: &str) {
        for character in text.chars() {
            edit.apply_key(press(KeyCode::Char(character)), spec, Instant::now());
        }
    }

    fn number_spec() -> NumberSpec {
        NumberSpec {
            label: "Width".to_owned(),
            value: 42.0,
            minimum: 0.0,
            maximum: 100.0,
            step: 5.0,
            unit: Some("cm".to_owned()),
            style: ilium_plugin_api::generated::types::ui::NumberStyle::Stepper,
            steps: Vec::new(),
            special_values: vec![(0.0, "Off".to_owned())],
            scale: 1,
            debounce_ms: 0,
        }
    }

    #[test]
    fn typing_inserts_at_the_caret_and_backspace_removes_one_cluster() {
        let spec = spec();
        let mut edit = TextEdit::seeded("ab");
        edit.apply_key(press(KeyCode::Left), &spec, Instant::now());
        typed(&mut edit, &spec, "X");
        assert_eq!(edit.text(), "aXb");
        edit.apply_key(press(KeyCode::Backspace), &spec, Instant::now());
        assert_eq!(edit.text(), "ab");
        edit.apply_key(press(KeyCode::Delete), &spec, Instant::now());
        assert_eq!(edit.text(), "a");
    }

    #[test]
    fn a_decomposed_letter_moves_and_deletes_as_one_cluster() {
        let spec = spec();
        let mut edit = TextEdit::seeded("e\u{301}x");
        edit.apply_key(press(KeyCode::Left), &spec, Instant::now());
        edit.apply_key(press(KeyCode::Backspace), &spec, Instant::now());
        assert_eq!(edit.text(), "x");
    }

    #[test]
    fn control_keys_move_and_cut() {
        let spec = spec();
        let mut edit = TextEdit::seeded("one two");
        edit.apply_key(ctrl('w'), &spec, Instant::now());
        assert_eq!(edit.text(), "one ");
        edit.apply_key(ctrl('a'), &spec, Instant::now());
        typed(&mut edit, &spec, "Z");
        assert_eq!(edit.text(), "Zone ");
        edit.apply_key(ctrl('e'), &spec, Instant::now());
        edit.apply_key(ctrl('u'), &spec, Instant::now());
        assert_eq!(edit.text(), "");
    }

    #[test]
    fn home_and_end_with_shift_select_and_typing_replaces_the_selection() {
        let spec = spec();
        let mut edit = TextEdit::seeded("hello");
        edit.apply_key(
            KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT),
            &spec,
            Instant::now(),
        );
        assert_eq!(edit.selection(), Some((0, 5)));
        typed(&mut edit, &spec, "x");
        assert_eq!(edit.text(), "x");
    }

    #[test]
    fn max_length_counts_characters_and_refuses_a_commit_that_does_not_fit() {
        let mut spec = spec();
        spec.max_length = 3;
        let mut edit = TextEdit::seeded("");
        typed(&mut edit, &spec, "abcd");
        assert_eq!(edit.text(), "abc");
        let mut decomposed = TextEdit::seeded("ab");
        assert!(!decomposed.paste("e\u{301}", &spec, Instant::now()));
        assert_eq!(decomposed.text(), "ab");
    }

    #[test]
    fn a_paste_drops_control_and_bidi_override_characters_and_is_cut_to_the_room() {
        let mut spec = spec();
        spec.max_length = 5;
        let mut edit = TextEdit::seeded("");
        assert!(edit.paste("\u{202E}abc\ndefgh", &spec, Instant::now()));
        assert_eq!(edit.text(), "abcde");
    }

    #[test]
    fn enter_submits_only_when_the_spec_says_so() {
        let mut spec = spec();
        let mut edit = TextEdit::seeded("a");
        assert_eq!(
            edit.apply_key(press(KeyCode::Enter), &spec, Instant::now()),
            EditOutcome::Submitted
        );
        spec.submit_on_enter = false;
        assert_eq!(
            edit.apply_key(press(KeyCode::Enter), &spec, Instant::now()),
            EditOutcome::Ignored
        );
    }

    #[test]
    fn a_change_is_released_after_its_debounce_and_flush_releases_it_early() {
        let mut spec = spec();
        spec.debounce_ms = 600;
        let start = Instant::now();
        let mut edit = TextEdit::seeded("");
        edit.apply_key(press(KeyCode::Char('a')), &spec, start);
        assert_eq!(edit.take_due(start), None);
        assert_eq!(
            edit.take_due(start + Duration::from_millis(600)),
            Some("a".to_owned())
        );
        edit.apply_key(press(KeyCode::Char('b')), &spec, start);
        assert_eq!(edit.flush(), Some("ab".to_owned()));
        assert_eq!(edit.flush(), None);
    }

    #[test]
    fn a_secret_hides_all_but_the_last_reveal_last_clusters_until_revealed() {
        let mut spec = spec();
        spec.secret = true;
        spec.reveal_last = 2;
        spec.reveal = true;
        let mut edit = TextEdit::seeded("abcd");
        let shown = |edit: &TextEdit, spec: &TextInputSpec| -> String {
            edit.layout(spec, 20)
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect()
        };
        assert_eq!(shown(&edit, &spec), "••cd");
        edit.apply_key(ctrl('r'), &spec, Instant::now());
        assert_eq!(shown(&edit, &spec), "abcd");
    }

    #[test]
    fn a_long_text_scrolls_so_the_caret_stays_inside_the_window() {
        let spec = spec();
        let edit = TextEdit::seeded("0123456789");
        let layout = edit.layout(&spec, 5);
        assert!(layout.caret_column < 5);
        assert_eq!(layout.cells.len(), 4);
        assert!(layout.reaches_end);
    }

    #[test]
    fn a_click_places_the_caret_on_the_cluster_under_it() {
        let spec = spec();
        let mut edit = TextEdit::seeded("abcdef");
        edit.place_caret(&spec, 20, 2);
        typed(&mut edit, &spec, "-");
        assert_eq!(edit.text(), "ab-cdef");
        edit.place_caret(&spec, 20, 19);
        typed(&mut edit, &spec, "!");
        assert_eq!(edit.text(), "ab-cdef!");
    }

    #[test]
    fn a_validation_expression_marks_a_value_that_does_not_match() {
        let mut spec = spec();
        spec.validation = Some("^[a-z]+$".to_owned());
        assert!(TextEdit::seeded("abc").is_valid(&spec));
        assert!(!TextEdit::seeded("ab1").is_valid(&spec));
        spec.validation = Some("(".to_owned());
        assert!(TextEdit::seeded("ab1").is_valid(&spec));
    }

    #[test]
    fn a_number_steps_by_step_or_rung_and_never_wraps() {
        let mut spec = number_spec();
        assert_eq!(step_number(&spec, 42.0, 1), 47.0);
        assert_eq!(step_number(&spec, 98.0, 1), 100.0);
        assert_eq!(step_number(&spec, 100.0, 1), 100.0);
        assert_eq!(step_number(&spec, 2.0, -1), 0.0);
        spec.steps = vec![10.0, 50.0, 100.0];
        assert_eq!(step_number(&spec, 42.0, 1), 50.0);
        assert_eq!(step_number(&spec, 42.0, -1), 10.0);
        assert_eq!(step_number(&spec, 10.0, -1), 10.0);
    }

    #[test]
    fn a_number_draws_special_words_units_and_scale() {
        let mut spec = number_spec();
        assert_eq!(format_number(&spec, 42.0), "42 cm");
        assert_eq!(format_number(&spec, 0.0), "Off");
        spec.scale = 10;
        assert_eq!(format_number(&spec, 17.0), "1.7 cm");
        assert_eq!(parse_typed(&spec, "1.7"), Some(17.0));
        assert_eq!(parse_typed(&spec, "99"), Some(100.0));
        assert_eq!(parse_typed(&spec, "x"), None);
    }

    #[test]
    fn typed_digits_apply_after_enter_or_the_debounce() {
        let spec = number_spec();
        let now = Instant::now();
        let mut edit = NumberEdit::seeded(&spec);
        assert_eq!(
            edit.apply_key(press(KeyCode::Char('7')), &spec, now),
            NumberOutcome::Typing
        );
        assert_eq!(edit.take_due(&spec, now), Some(7.0));
        edit.apply_key(press(KeyCode::Char('5')), &spec, now);
        assert_eq!(
            edit.apply_key(press(KeyCode::Enter), &spec, now),
            NumberOutcome::Changed(75.0)
        );
        assert_eq!(edit.typed, None);
    }

    #[test]
    fn number_keys_step_jump_and_cancel_an_entry() {
        let spec = number_spec();
        let now = Instant::now();
        let mut edit = NumberEdit::seeded(&spec);
        assert_eq!(
            edit.apply_key(press(KeyCode::Right), &spec, now),
            NumberOutcome::Changed(47.0)
        );
        assert_eq!(
            edit.apply_key(press(KeyCode::End), &spec, now),
            NumberOutcome::Changed(100.0)
        );
        assert_eq!(
            edit.apply_key(press(KeyCode::Home), &spec, now),
            NumberOutcome::Changed(0.0)
        );
        edit.apply_key(press(KeyCode::Char('*')), &spec, now);
        assert!(edit.typed.is_some());
        edit.apply_key(press(KeyCode::Esc), &spec, now);
        assert!(edit.typed.is_none());
    }

    #[test]
    fn the_slider_value_snaps_to_the_nearest_step() {
        let spec = number_spec();
        assert_eq!(slider_number(&spec, 0, 21), 0.0);
        assert_eq!(slider_number(&spec, 20, 21), 100.0);
        assert_eq!(slider_number(&spec, 10, 21), 50.0);
    }

    #[test]
    fn a_follow_tail_scroll_view_stays_at_the_bottom_until_the_user_scrolls_up() {
        assert_eq!(scroll_offset(None, true, 6), 6);
        assert_eq!(scroll_offset(None, false, 6), 0);
        let detached = ScrollState {
            offset: 2,
            detached: true,
        };
        assert_eq!(scroll_offset(Some(&detached), true, 6), 2);
        let following = ScrollState {
            offset: 2,
            detached: false,
        };
        assert_eq!(scroll_offset(Some(&following), true, 6), 6);
        assert_eq!(scroll_offset(Some(&detached), false, 1), 1);
    }

    #[test]
    fn widget_states_release_due_changes_by_key() {
        let mut states = WidgetStates::default();
        let mut spec = spec();
        spec.debounce_ms = 0;
        let mut edit = TextEdit::seeded("");
        let now = Instant::now();
        edit.apply_key(press(KeyCode::Char('q')), &spec, now);
        states.texts.insert("name".to_owned(), edit);
        assert_eq!(
            states.take_due(now),
            vec![DueChange::Text("name".to_owned(), "q".to_owned())]
        );
        assert!(states.take_due(now).is_empty());
    }
}
