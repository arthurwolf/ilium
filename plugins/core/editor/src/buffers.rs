//! Buffer text the plugin owns while it runs, and the edits applied to it (core-plugin-editor.md
//! "Be the editor provider", the `ilium.core.editor/buffers` service).
//!
//! Pure: no host call. A buffer is a list of lines without their `\n`; columns count Unicode
//! scalar values. [`Buffer::replace_ranges`] applies a list of edits atomically against the
//! revision the caller read: a stale revision, an inverted range or overlapping edits change
//! nothing.

use std::collections::BTreeMap;

use ilium_plugin_sdk::generated::types::{BufferRef, TextEdit, TextPosition};

/// Lines one `read` returns at most (the `editor.read-lines` limit).
pub const MAX_READ_LINES: u32 = 10_000;

/// Largest file text the plugin keeps in a buffer, in bytes.
pub const MAX_BUFFER_BYTES: usize = 32 * 1024 * 1024;

/// Why an edit list was refused; nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The caller read an older revision than the buffer has now.
    StaleRevision { current: u64 },
    /// An edit's end lies before its start.
    InvertedRange,
    /// Two edits touch the same text.
    Overlap,
    /// The edits would grow the buffer past [`MAX_BUFFER_BYTES`].
    TooLarge,
}

impl EditError {
    /// The error code of the service reply.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::StaleRevision { .. } => "conflict",
            Self::InvertedRange | Self::Overlap => "invalid-argument",
            Self::TooLarge => "quota-exceeded",
        }
    }

    /// Plain text for logs and the reply.
    pub fn message(&self) -> String {
        match self {
            Self::StaleRevision { current } => {
                format!("the buffer is at revision {current}, not the revision the edit was made against")
            }
            Self::InvertedRange => "an edit range ends before it starts".to_string(),
            Self::Overlap => "two edits overlap".to_string(),
            Self::TooLarge => "the edit would make the buffer too large".to_string(),
        }
    }
}

/// One open buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct Buffer {
    pub reference: BufferRef,
    lines: Vec<String>,
}

impl Buffer {
    /// A buffer for an `editor-opened` reference with the text the file held (`None`: no file or
    /// an unreadable one, so the buffer starts as one empty line).
    pub fn opened(reference: BufferRef, text: Option<&str>) -> Self {
        let lines = text.map(split_lines).unwrap_or_else(|| vec![String::new()]);
        Self { reference, lines }
    }

    /// Number of lines the buffer holds now.
    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }

    /// Up to `count` lines from `first_line`; fewer when the buffer ends sooner.
    pub fn read(&self, first_line: u32, count: u32) -> Vec<String> {
        self.lines
            .iter()
            .skip(first_line as usize)
            .take(count.min(MAX_READ_LINES) as usize)
            .cloned()
            .collect()
    }

    /// One line, or `None` past the end.
    pub fn line(&self, line: u32) -> Option<&str> {
        self.lines.get(line as usize).map(String::as_str)
    }

    /// The whole text, lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn byte_size(&self) -> usize {
        self.lines.iter().map(|line| line.len() + 1).sum()
    }

    /// Applies `edits` atomically against `base_revision`. On success the revision rises by one,
    /// the buffer is dirty and the new revision is returned.
    pub fn replace_ranges(
        &mut self,
        base_revision: u64,
        edits: &[TextEdit],
    ) -> Result<u64, EditError> {
        if base_revision != self.reference.revision {
            return Err(EditError::StaleRevision {
                current: self.reference.revision,
            });
        }
        let mut ordered: Vec<(Place, Place, &str)> = Vec::with_capacity(edits.len());
        for edit in edits {
            let start = self.clamp(&edit.range.start);
            let end = self.clamp(&edit.range.end);
            if end < start {
                return Err(EditError::InvertedRange);
            }
            ordered.push((start, end, edit.replacement.as_str()));
        }
        ordered.sort_by_key(|entry| (entry.0, entry.1));
        for pair in ordered.windows(2) {
            if pair[1].0 < pair[0].1 {
                return Err(EditError::Overlap);
            }
        }
        let growth: usize = ordered.iter().map(|entry| entry.2.len()).sum();
        if self.byte_size().saturating_add(growth) > MAX_BUFFER_BYTES {
            return Err(EditError::TooLarge);
        }
        // Back to front, so the places of the edits still to apply stay valid.
        for (start, end, replacement) in ordered.into_iter().rev() {
            self.splice(start, end, replacement);
        }
        self.reference.revision += 1;
        self.reference.dirty = true;
        self.reference.line_count = self.line_count();
        Ok(self.reference.revision)
    }

    /// The position as a place inside the buffer: a line past the end means the end of the last
    /// line, a column past the end of a line means that line's end.
    fn clamp(&self, position: &TextPosition) -> Place {
        let last = self.lines.len().saturating_sub(1);
        let line = (position.line as usize).min(last);
        if position.line as usize > last {
            let width = self.lines.get(last).map_or(0, |text| text.chars().count());
            return Place {
                line: last,
                column: width,
            };
        }
        let width = self.lines.get(line).map_or(0, |text| text.chars().count());
        Place {
            line,
            column: (position.column as usize).min(width),
        }
    }

    fn splice(&mut self, start: Place, end: Place, replacement: &str) {
        let head: String = self.lines[start.line].chars().take(start.column).collect();
        let tail: String = self.lines[end.line].chars().skip(end.column).collect();
        let joined = format!("{head}{replacement}{tail}");
        let replaced = split_lines(&joined);
        self.lines.splice(start.line..=end.line, replaced);
    }
}

/// A line and a character column inside a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    line: usize,
    column: usize,
}

/// Splits file text into lines. A trailing `\n` ends the last line instead of opening an empty
/// one, a `\r` before `\n` is dropped, and an empty text is one empty line.
pub fn split_lines(text: &str) -> Vec<String> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<String> = body
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

/// The buffers of one plugin instance, by buffer id.
#[derive(Debug, Default)]
pub struct Buffers {
    open: BTreeMap<u64, Buffer>,
}

impl Buffers {
    /// Opens (or reopens) a buffer.
    pub fn open(&mut self, buffer: Buffer) {
        self.open.insert(buffer.reference.buffer.0, buffer);
    }

    /// Closes a buffer; unknown ids are ignored.
    pub fn close(&mut self, id: u64) {
        self.open.remove(&id);
    }

    pub fn get(&self, id: u64) -> Option<&Buffer> {
        self.open.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Buffer> {
        self.open.get_mut(&id)
    }

    /// Open buffers in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Buffer> {
        self.open.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::generated::types::{BufferId, TextRange};

    fn buffer(text: &str) -> Buffer {
        Buffer::opened(
            BufferRef {
                buffer: BufferId(1),
                revision: 1,
                line_count: 1,
                ..BufferRef::default()
            },
            Some(text),
        )
    }

    fn edit(start: (u32, u32), end: (u32, u32), replacement: &str) -> TextEdit {
        TextEdit {
            range: TextRange {
                start: TextPosition {
                    line: start.0,
                    column: start.1,
                },
                end: TextPosition {
                    line: end.0,
                    column: end.1,
                },
            },
            replacement: replacement.to_string(),
        }
    }

    /// REQ: F-019
    #[test]
    fn split_lines_follows_the_kernel_line_count() {
        assert_eq!(split_lines(""), vec![""]);
        assert_eq!(split_lines("a\n"), vec!["a"]);
        assert_eq!(split_lines("a\n\n"), vec!["a", ""]);
        assert_eq!(split_lines("a\r\nb"), vec!["a", "b"]);
    }

    /// REQ: F-019
    #[test]
    fn two_edits_apply_against_the_original_places() {
        let mut target = buffer("abc\ndef");
        let result =
            target.replace_ranges(1, &[edit((0, 0), (0, 1), "XY"), edit((1, 2), (1, 3), "!")]);
        assert_eq!(result, Ok(2));
        assert_eq!(target.text(), "XYbc\nde!");
        assert!(target.reference.dirty);
    }

    /// REQ: F-019
    #[test]
    fn an_edit_past_the_end_appends() {
        let mut target = buffer("abc");
        assert_eq!(
            target.replace_ranges(1, &[edit((9, 9), (9, 9), "\nz")]),
            Ok(2)
        );
        assert_eq!(target.text(), "abc\nz");
    }

    /// REQ: F-019
    #[test]
    fn an_inverted_range_is_refused_and_changes_nothing() {
        let mut target = buffer("abc");
        let result = target.replace_ranges(1, &[edit((0, 2), (0, 1), "x")]);
        assert_eq!(result, Err(EditError::InvertedRange));
        assert_eq!(target.text(), "abc");
        assert_eq!(target.reference.revision, 1);
        assert!(!target.reference.dirty);
    }

    /// REQ: F-019
    #[test]
    fn touching_edits_do_not_overlap() {
        let mut target = buffer("abcd");
        let result =
            target.replace_ranges(1, &[edit((0, 0), (0, 2), "X"), edit((0, 2), (0, 4), "Y")]);
        assert_eq!(result, Ok(2));
        assert_eq!(target.text(), "XY");
    }

    /// REQ: F-019
    #[test]
    fn read_is_bounded_by_the_buffer_and_the_limit() {
        let target = buffer("a\nb\nc");
        assert_eq!(target.read(1, 10), vec!["b", "c"]);
        assert!(target.read(7, 3).is_empty());
        assert_eq!(target.read(0, MAX_READ_LINES + 5).len(), 3);
    }
}
