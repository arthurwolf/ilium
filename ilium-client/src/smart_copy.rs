//! Immutable terminal-screen extraction and model-referenced semantic regions.
//!
//! The model never supplies clipboard text or terminal coordinates. It sees
//! numbered source lines plus stable word IDs, then returns JSONL references;
//! this module resolves every reference back into the frozen `vt100::Screen`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use ilium_core::NodeId;
use ratatui::layout::Position;
use serde::{Deserialize, Serialize};

pub const ARRIVAL_FLASH_DURATION: Duration = Duration::from_millis(500);
pub const MAXIMUM_CANDIDATES: usize = 512;
const MAXIMUM_DETECTED_CANDIDATES: usize = MAXIMUM_CANDIDATES / 2;
pub const MAXIMUM_PARTS_PER_CANDIDATE: usize = 128;
pub const MAXIMUM_JSONL_LINE_BYTES: usize = 64 * 1024;
const MAXIMUM_CANDIDATE_LABEL_CHARACTERS: usize = 80;
const MAXIMUM_CANDIDATE_KIND_CHARACTERS: usize = 40;

pub fn exit_button_rect(toolbar_area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    const WIDTH: u16 = 8;
    let width = WIDTH.min(toolbar_area.width);
    ratatui::layout::Rect::new(
        toolbar_area.right().saturating_sub(width),
        toolbar_area.y,
        width,
        toolbar_area.height.min(1),
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptLine {
    pub id: u16,
    pub text: String,
    pub words: Vec<PromptWord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptWord {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
struct SnapshotWord {
    id: String,
    start_column: u16,
    end_column: u16,
}

#[derive(Clone)]
pub struct SmartCopySnapshot {
    pub screen: vt100::Screen,
    pub lines: Vec<PromptLine>,
    words: Vec<Vec<SnapshotWord>>,
    detected: Vec<DetectedRegion>,
}

impl SmartCopySnapshot {
    pub fn capture(screen: &vt100::Screen) -> Self {
        let (rows, columns) = screen.size();
        let mut lines = Vec::with_capacity(usize::from(rows));
        let mut words = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut text = String::new();
            let mut row_words = Vec::new();
            let mut active_start = None;
            let mut active_text = String::new();
            for column in 0..columns {
                let cell = screen.cell(row, column);
                if cell.is_some_and(vt100::Cell::is_wide_continuation) {
                    continue;
                }
                let contents = cell.map(vt100::Cell::contents).unwrap_or("");
                let rendered = if contents.is_empty() { " " } else { contents };
                text.push_str(rendered);
                let is_whitespace = rendered.chars().all(char::is_whitespace);
                match (active_start, is_whitespace) {
                    (None, false) => {
                        active_start = Some(column);
                        active_text.push_str(rendered);
                    }
                    (Some(_), false) => active_text.push_str(rendered),
                    (Some(start_column), true) => {
                        push_word(&mut row_words, start_column, column - 1, &mut active_text);
                        active_start = None;
                    }
                    (None, true) => {}
                }
            }
            if let Some(start_column) = active_start {
                push_word(
                    &mut row_words,
                    start_column,
                    columns.saturating_sub(1),
                    &mut active_text,
                );
            }
            let prompt_words = row_words
                .iter()
                .map(|word| PromptWord {
                    id: word.id.clone(),
                    text: cell_text(screen, row, word.start_column, word.end_column),
                })
                .collect();
            lines.push(PromptLine {
                id: row + 1,
                text: text.trim_end().to_string(),
                words: prompt_words,
            });
            words.push(row_words);
        }
        let mut snapshot = Self {
            screen: screen.clone(),
            lines,
            words,
            detected: Vec::new(),
        };
        snapshot.detected = detect_regions(&snapshot);
        snapshot
    }

    pub fn prompt_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.lines)
    }

    fn resolve_candidate(&self, spec: CandidateSpec) -> Result<SmartCopyCandidate, String> {
        if spec.label.trim().is_empty()
            || spec.label.chars().count() > MAXIMUM_CANDIDATE_LABEL_CHARACTERS
            || unsafe_display_text(&spec.label)
        {
            return Err(
                ilium_prompts::naming::NAMING_SMART_COPY_CANDIDATE_LABEL_IS_INVALID.to_string(),
            );
        }
        if spec.kind.trim().is_empty()
            || spec.kind.chars().count() > MAXIMUM_CANDIDATE_KIND_CHARACTERS
            || unsafe_display_text(&spec.kind)
        {
            return Err(
                ilium_prompts::naming::NAMING_SMART_COPY_CANDIDATE_KIND_IS_INVALID.to_string(),
            );
        }
        if spec.parts.is_empty() || spec.parts.len() > MAXIMUM_PARTS_PER_CANDIDATE {
            return Err(
                ilium_prompts::naming::NAMING_SMART_COPY_CANDIDATE_HAS_AN_INVALID_PART_COUNT
                    .to_string(),
            );
        }
        let mut spans = Vec::new();
        for part in spec.parts {
            match part {
                CandidatePartSpec::Lines { lines } => {
                    if lines.is_empty() {
                        return Err(
                            ilium_prompts::naming::NAMING_SMART_COPY_LINE_LIST_IS_EMPTY.to_string()
                        );
                    }
                    for line_id in lines {
                        spans.push(self.whole_line_span(line_id)?);
                    }
                }
                CandidatePartSpec::Range {
                    line,
                    from,
                    through,
                } => {
                    spans.push(self.range_span(line, from.as_deref(), through.as_deref())?);
                }
            }
        }
        spans.sort_by_key(|span| (span.row, span.start_column, span.end_column));
        spans.dedup();
        let text = spans
            .iter()
            .map(|span| cell_text(&self.screen, span.row, span.start_column, span.end_column))
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            return Err(
                ilium_prompts::naming::NAMING_SMART_COPY_CANDIDATE_RESOLVES_ONLY_TO_WHITESPACE
                    .to_string(),
            );
        }
        let cell_count = spans
            .iter()
            .map(|span| usize::from(span.end_column - span.start_column + 1))
            .sum();
        Ok(SmartCopyCandidate {
            label: spec.label,
            kind: spec.kind,
            spans,
            text,
            cell_count,
            arrived_at: Instant::now(),
        })
    }

    fn whole_line_span(&self, line_id: u16) -> Result<CellSpan, String> {
        let row = line_id.checked_sub(1).ok_or_else(|| {
            ilium_prompts::naming::NAMING_SMART_COPY_LINE_IDS_START_AT_1.to_string()
        })?;
        self.lines.get(usize::from(row)).ok_or_else(|| {
            ilium_prompts::render_value(
                "naming/smart_copy/line-v0-is-outside-the-snapshot",
                &serde_json::json!({"v0": (line_id).to_string()}),
            )
        })?;
        let end_column = line_end_column(&self.screen, row).ok_or_else(|| {
            ilium_prompts::render_value(
                "naming/smart_copy/line-v0-is-blank",
                &serde_json::json!({"v0": (line_id).to_string()}),
            )
        })?;
        Ok(CellSpan {
            row,
            start_column: 0,
            end_column,
        })
    }

    fn range_span(
        &self,
        line_id: u16,
        from: Option<&str>,
        through: Option<&str>,
    ) -> Result<CellSpan, String> {
        if from.is_none() && through.is_none() {
            return self.whole_line_span(line_id);
        }
        let row = line_id.checked_sub(1).ok_or_else(|| {
            ilium_prompts::naming::NAMING_SMART_COPY_LINE_IDS_START_AT_1.to_string()
        })?;
        let words = self.words.get(usize::from(row)).ok_or_else(|| {
            ilium_prompts::render_value(
                "naming/smart_copy/line-v0-is-outside-the-snapshot",
                &serde_json::json!({"v0": (line_id).to_string()}),
            )
        })?;
        let first = from
            .and_then(|id| words.iter().position(|word| word.id == id))
            .unwrap_or(0);
        let last = through
            .and_then(|id| words.iter().position(|word| word.id == id))
            .unwrap_or_else(|| words.len().saturating_sub(1));
        if words.is_empty() || first > last {
            return Err(ilium_prompts::render_value(
                "naming/smart_copy/invalid-word-range-on-line",
                &serde_json::json!({"v0": (line_id).to_string()}),
            ));
        }
        if from.is_some() && !words.iter().any(|word| Some(word.id.as_str()) == from) {
            return Err(ilium_prompts::render_value(
                "naming/smart_copy/unknown-starting-word-on-line",
                &serde_json::json!({"v0": (line_id).to_string()}),
            ));
        }
        if through.is_some() && !words.iter().any(|word| Some(word.id.as_str()) == through) {
            return Err(ilium_prompts::render_value(
                "naming/smart_copy/unknown-ending-word-on-line",
                &serde_json::json!({"v0": (line_id).to_string()}),
            ));
        }
        Ok(CellSpan {
            row,
            start_column: words[first].start_column,
            end_column: words[last].end_column,
        })
    }
}

fn unsafe_display_text(text: &str) -> bool {
    text.chars().any(|character| {
        character.is_control()
            || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
}

fn push_word(words: &mut Vec<SnapshotWord>, start_column: u16, end_column: u16, text: &mut String) {
    let id = format!("w{}", words.len() + 1);
    words.push(SnapshotWord {
        id,
        start_column,
        end_column,
    });
    text.clear();
}

fn line_end_column(screen: &vt100::Screen, row: u16) -> Option<u16> {
    let (_, columns) = screen.size();
    let last = (0..columns).rev().find(|column| {
        screen
            .cell(row, *column)
            .is_some_and(|cell| cell.has_contents() && !cell.contents().trim().is_empty())
    })?;
    Some(
        if last.saturating_add(1) < columns
            && screen
                .cell(row, last + 1)
                .is_some_and(vt100::Cell::is_wide_continuation)
        {
            last + 1
        } else {
            last
        },
    )
}

fn cell_text(screen: &vt100::Screen, row: u16, start_column: u16, end_column: u16) -> String {
    let mut text = String::new();
    for column in start_column..=end_column {
        let Some(cell) = screen.cell(row, column) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        if cell.contents().is_empty() {
            text.push(' ');
        } else {
            text.push_str(cell.contents());
        }
    }
    text.trim_end().to_string()
}

// Parser offsets are character positions; selections are terminal cell columns.
// Keep this mapping while scanning so wide and combining characters cannot shift
// a table cell or inline-code selection into its neighbour.
struct ScanLine {
    text: String,
    chars: Vec<char>,
    start_columns: Vec<u16>,
    end_columns: Vec<u16>,
}

impl ScanLine {
    fn new(screen: &vt100::Screen, row: u16, width: u16) -> Self {
        let mut line = Self {
            text: String::new(),
            chars: Vec::new(),
            start_columns: Vec::new(),
            end_columns: Vec::new(),
        };
        for column in 0..width {
            let cell = screen.cell(row, column);
            if cell.is_some_and(vt100::Cell::is_wide_continuation) {
                continue;
            }
            let contents = cell.map(vt100::Cell::contents).unwrap_or("");
            let rendered = if contents.is_empty() { " " } else { contents };
            let end_column = if column.saturating_add(1) < width
                && screen
                    .cell(row, column + 1)
                    .is_some_and(vt100::Cell::is_wide_continuation)
            {
                column + 1
            } else {
                column
            };
            for character in rendered.chars() {
                line.text.push(character);
                line.chars.push(character);
                line.start_columns.push(column);
                line.end_columns.push(end_column);
            }
        }
        line
    }

    fn trimmed(&self) -> &str {
        self.text.trim()
    }

    fn full_span(&self, row: usize) -> Option<CellSpan> {
        let last = self
            .chars
            .iter()
            .rposition(|character| !character.is_whitespace())?;
        Some(CellSpan {
            row: row as u16,
            start_column: 0,
            end_column: self.end_columns[last],
        })
    }

    fn content_span(&self, row: usize, mut start: usize, mut end: usize) -> Option<CellSpan> {
        end = end.min(self.chars.len());
        while start < end && self.chars[start].is_whitespace() {
            start += 1;
        }
        while start < end && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        (start < end).then(|| CellSpan {
            row: row as u16,
            start_column: self.start_columns[start],
            end_column: self.end_columns[end - 1],
        })
    }
}

struct RegionDraft {
    label: String,
    kind: &'static str,
    spans: Vec<CellSpan>,
}

fn draft(primary: &mut Vec<RegionDraft>, label: String, kind: &'static str, spans: Vec<CellSpan>) {
    if !spans.is_empty() && primary.len() < MAXIMUM_DETECTED_CANDIDATES {
        primary.push(RegionDraft { label, kind, spans });
    }
}

fn row_draft(
    drafts: &mut Vec<RegionDraft>,
    lines: &[ScanLine],
    start: usize,
    end: usize,
    label: String,
    kind: &'static str,
    preserve_blank_rows: bool,
) {
    let clipped_end = end.min(start.saturating_add(MAXIMUM_PARTS_PER_CANDIDATE));
    let spans = (start..clipped_end)
        .filter_map(|row| {
            lines[row].full_span(row).or_else(|| {
                (preserve_blank_rows && !lines[row].chars.is_empty()).then_some(CellSpan {
                    row: row as u16,
                    start_column: 0,
                    end_column: 0,
                })
            })
        })
        .collect();
    let label = if clipped_end < end {
        ilium_prompts::render_value(
            "naming/smart_copy/v0-visible-excerpt",
            &serde_json::json!({"v0": (label).to_string()}),
        )
    } else {
        label
    };
    draft(drafts, label, kind, spans);
}

fn fence_start(text: &str) -> Option<(char, usize)> {
    let trimmed = text.trim_start_matches(' ');
    if text.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let width = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (width >= 3).then_some((marker, width))
}

fn fence_closes(text: &str, marker: char, width: usize) -> bool {
    let trimmed = text.trim_start_matches(' ');
    if text.len() - trimmed.len() > 3 {
        return false;
    }
    let count = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    count >= width && trimmed.chars().skip(count).all(char::is_whitespace)
}

fn is_escaped(chars: &[char], index: usize) -> bool {
    chars[..index]
        .iter()
        .rev()
        .take_while(|character| **character == '\\')
        .count()
        % 2
        == 1
}

fn closing_backtick_run(chars: &[char], start: usize, width: usize) -> Option<usize> {
    let mut index = start;
    while index < chars.len() {
        let run_width = chars[index..]
            .iter()
            .take_while(|character| **character == '`')
            .count();
        if run_width == width {
            return Some(index);
        }
        // Do not treat a suffix of a longer delimiter run as an exact match.
        index += run_width.max(1);
    }
    None
}

fn table_cells(line: &ScanLine) -> Vec<(usize, usize)> {
    let mut pipes = Vec::new();
    let mut index = 0;
    while index < line.chars.len() {
        if line.chars[index] == '`' && !is_escaped(&line.chars, index) {
            let width = line.chars[index..]
                .iter()
                .take_while(|character| **character == '`')
                .count();
            if let Some(close) = closing_backtick_run(&line.chars, index + width, width) {
                index = close + width;
                continue;
            }
            index += width;
            continue;
        }
        if line.chars[index] == '|' && !is_escaped(&line.chars, index) {
            pipes.push(index);
        }
        index += 1;
    }
    if pipes.is_empty() {
        return Vec::new();
    }
    let mut cells = Vec::new();
    let mut previous = 0;
    for pipe in pipes {
        cells.push((previous, pipe));
        previous = pipe + 1;
    }
    cells.push((previous, line.chars.len()));
    if cells.first().is_some_and(|(start, end)| {
        line.chars[*start..*end]
            .iter()
            .all(|character| character.is_whitespace())
    }) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|(start, end)| {
        line.chars[*start..*end]
            .iter()
            .all(|character| character.is_whitespace())
    }) {
        cells.pop();
    }
    cells
}

fn is_table_separator(line: &ScanLine, cells: &[(usize, usize)]) -> bool {
    cells.len() >= 2
        && cells.iter().all(|(start, end)| {
            let segment = line.chars[*start..*end].iter().collect::<String>();
            let segment = segment.trim();
            let segment = segment.strip_prefix(':').unwrap_or(segment);
            let segment = segment.strip_suffix(':').unwrap_or(segment);
            segment.len() >= 3 && segment.chars().all(|character| character == '-')
        })
}

fn box_edge(text: &str) -> bool {
    let text = text.trim();
    let unicode = ['╭', '╰', '┌', '└', '┏', '┗', '╔', '╚'];
    let ascii = text.starts_with('+') && text.ends_with('+') && text.matches('-').count() >= 3;
    ascii
        || (text.chars().next().is_some_and(|ch| unicode.contains(&ch))
            && (text.contains('─') || text.contains('━') || text.contains('═')))
}

fn box_body(text: &str) -> bool {
    let text = text.trim();
    text.chars()
        .next()
        .is_some_and(|character| matches!(character, '│' | '║' | '┃'))
        || (text.starts_with('|') && text.ends_with('|') && text.len() > 2)
}

fn box_interior(line: &ScanLine, row: usize) -> Option<CellSpan> {
    let first = line
        .chars
        .iter()
        .position(|character| !character.is_whitespace())?;
    let last = line
        .chars
        .iter()
        .rposition(|character| !character.is_whitespace())?;
    let left = line.chars[first];
    if !['│', '║', '┃', '|'].contains(&left) {
        return None;
    }
    let right = if last > first && line.chars[last] == left {
        last
    } else {
        last + 1
    };
    line.content_span(row, first + 1, right)
}

fn heading_level(text: &str) -> Option<u8> {
    let trimmed = text.trim_start_matches(' ');
    if text.len() - trimmed.len() > 3 {
        return None;
    }
    let width = trimmed
        .chars()
        .take_while(|character| *character == '#')
        .count();
    (1..=6).contains(&width).then_some(())?;
    trimmed
        .chars()
        .nth(width)
        .filter(|character| character.is_whitespace())?;
    Some(width as u8)
}

fn setext_level(text: &str) -> Option<u8> {
    let text = text.trim();
    if text.len() < 3 {
        return None;
    }
    if text.chars().all(|character| character == '=') {
        Some(1)
    } else if text.chars().all(|character| character == '-') {
        Some(2)
    } else {
        None
    }
}

fn is_list(text: &str) -> bool {
    let text = text.trim_start();
    if ["- ", "* ", "+ ", "• ", "● ", "⏺ "]
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return true;
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && digits <= 3
        && matches!(text.chars().nth(digits), Some('.' | ')'))
        && text
            .chars()
            .nth(digits + 1)
            .is_some_and(char::is_whitespace)
}

fn is_command(text: &str) -> bool {
    let text = text.trim_start();
    [
        "$ ",
        "❯ ",
        "› ",
        ilium_prompts::naming::NAMING_SMART_COPY_PS,
        ilium_prompts::naming::NAMING_SMART_COPY_C,
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
        || text.split_once("$ ").is_some_and(|(prompt, _)| {
            prompt.contains('@') && prompt.len() <= 60 && !prompt.contains(' ')
        })
}

fn is_diff_start(text: &str) -> bool {
    text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_DIFF_GIT)
        || text.starts_with("@@ ")
        || text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_A)
        || text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_BEGIN_PATCH)
}

fn is_diagnostic_start(text: &str) -> bool {
    let text = text.trim_start();
    [
        "error:",
        "warning:",
        "error[",
        ilium_prompts::naming::NAMING_SMART_COPY_TRACEBACK,
        "Exception:",
        ilium_prompts::naming::NAMING_SMART_COPY_CAUSED_BY,
        ilium_prompts::naming::NAMING_SMART_COPY_THREAD,
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

fn is_tree_branch(text: &str) -> bool {
    ["├─", "└─", "+--", "|--", "\\--"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn detect_regions(snapshot: &SmartCopySnapshot) -> Vec<DetectedRegion> {
    let (height, width) = snapshot.screen.size();
    let lines = (0..height)
        .map(|row| ScanLine::new(&snapshot.screen, row, width))
        .collect::<Vec<_>>();
    let mut structural = vec![false; lines.len()];
    let mut fenced = vec![false; lines.len()];
    let mut primary = Vec::new();
    let mut details = Vec::new();

    // Whole structures are queued before cells and short inline selections.
    let mut row = 0;
    while row < lines.len() {
        if let Some((marker, marker_width)) = fence_start(&lines[row].text) {
            let close = ((row + 1)..lines.len())
                .find(|next| fence_closes(&lines[*next].text, marker, marker_width));
            // A bare fence inside a clipped viewport may be a closing fence.
            // Do not consume the rest of the screen on that ambiguous signal.
            if close.is_none()
                && row > 0
                && !lines[row - 1].trimmed().is_empty()
                && heading_level(&lines[row - 1].text).is_none()
                && !(row >= 2
                    && setext_level(&lines[row - 1].text).is_some()
                    && !lines[row - 2].trimmed().is_empty())
                && lines[row]
                    .text
                    .trim_start()
                    .chars()
                    .skip(marker_width)
                    .all(char::is_whitespace)
            {
                row += 1;
                continue;
            }
            let end = close.map_or(lines.len(), |last| last + 1);
            structural[row..end].fill(true);
            fenced[row..end].fill(true);
            row_draft(
                &mut primary,
                &lines,
                row,
                end,
                ilium_prompts::render_value(
                    "naming/smart_copy/visible-fenced-block-l",
                    &serde_json::json!({"v0": (row + 1).to_string()}),
                ),
                "fenced-code",
                true,
            );
            let content_end = close.unwrap_or(end);
            if row + 1 < content_end {
                row_draft(
                    &mut details,
                    &lines,
                    row + 1,
                    content_end,
                    ilium_prompts::render_value(
                        "naming/smart_copy/code-contents-l",
                        &serde_json::json!({"v0": (row + 1).to_string()}),
                    ),
                    "code",
                    true,
                );
            }
            row = end;
        } else {
            row += 1;
        }
    }

    row = 0;
    while row + 1 < lines.len() {
        if structural[row] || structural[row + 1] {
            row += 1;
            continue;
        }
        let header = table_cells(&lines[row]);
        let separator = table_cells(&lines[row + 1]);
        if header.len() < 2
            || header.len() != separator.len()
            || !is_table_separator(&lines[row + 1], &separator)
        {
            row += 1;
            continue;
        }
        let mut end = row + 2;
        while end < lines.len()
            && !structural[end]
            && table_cells(&lines[end]).len() == header.len()
        {
            end += 1;
        }
        structural[row..end].fill(true);
        row_draft(
            &mut primary,
            &lines,
            row,
            end,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-table-l",
                &serde_json::json!({"v0": (row + 1).to_string()}),
            ),
            "table",
            true,
        );
        for table_row in std::iter::once(row).chain((row + 2)..end) {
            for (column, (start, stop)) in table_cells(&lines[table_row]).into_iter().enumerate() {
                if let Some(span) = lines[table_row].content_span(table_row, start, stop) {
                    draft(
                        &mut details,
                        ilium_prompts::render_value(
                            "naming/smart_copy/table-cell-l-v0-c",
                            &serde_json::json!({"v0": (table_row + 1).to_string(), "v1": (column + 1).to_string()}),
                        ),
                        "table-cell",
                        vec![span],
                    );
                }
            }
        }
        row = end;
    }

    row = 0;
    while row < lines.len() {
        if structural[row]
            || !(box_edge(&lines[row].text)
                || (row == 0 && box_body(&lines[row].text) && !is_tree_branch(&lines[row].text)))
        {
            row += 1;
            continue;
        }
        let start = row;
        let mut body_count = usize::from(box_body(&lines[row].text));
        row += 1;
        while row < lines.len()
            && !structural[row]
            && (box_body(&lines[row].text) || box_edge(&lines[row].text))
        {
            if box_body(&lines[row].text) {
                body_count += 1;
            }
            row += 1;
            if box_edge(&lines[row - 1].text) {
                break;
            }
        }
        if body_count == 0 {
            continue;
        }
        structural[start..row].fill(true);
        row_draft(
            &mut primary,
            &lines,
            start,
            row,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-terminal-frame-l",
                &serde_json::json!({"v0": (start + 1).to_string()}),
            ),
            "box",
            true,
        );
        let interior = (start..row)
            .filter_map(|line_row| box_interior(&lines[line_row], line_row))
            .take(MAXIMUM_PARTS_PER_CANDIDATE)
            .collect();
        draft(
            &mut details,
            ilium_prompts::render_value(
                "naming/smart_copy/frame-contents-l",
                &serde_json::json!({"v0": (start + 1).to_string()}),
            ),
            "box-contents",
            interior,
        );
    }

    row = 0;
    while row < lines.len() {
        if structural[row] || !is_diff_start(lines[row].trimmed()) {
            row += 1;
            continue;
        }
        let start = row;
        row += 1;
        while row < lines.len() && !structural[row] {
            let text = &lines[row].text;
            if text.is_empty()
                || !(text
                    .chars()
                    .next()
                    .is_some_and(|character| matches!(character, ' ' | '+' | '-' | '@' | '\\'))
                    || text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_DIFF)
                    || text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_INDEX)
                    || text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_NEW_FILE)
                    || text.starts_with(ilium_prompts::naming::NAMING_SMART_COPY_DELETED_FILE))
            {
                break;
            }
            row += 1;
        }
        structural[start..row].fill(true);
        row_draft(
            &mut primary,
            &lines,
            start,
            row,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-diff-l",
                &serde_json::json!({"v0": (start + 1).to_string()}),
            ),
            "diff",
            false,
        );
    }

    row = 0;
    while row < lines.len() {
        if structural[row] || !is_diagnostic_start(lines[row].trimmed()) {
            row += 1;
            continue;
        }
        let start = row;
        row += 1;
        while row < lines.len() && !structural[row] && !lines[row].trimmed().is_empty() {
            let text = &lines[row].text;
            if !(text
                .chars()
                .next()
                .is_some_and(|character| matches!(character, ' ' | '\t' | '|' | '^'))
                || [
                    ilium_prompts::naming::NAMING_SMART_COPY_AT,
                    ilium_prompts::naming::NAMING_SMART_COPY_CAUSED_BY,
                    "--> ",
                ]
                .iter()
                .any(|prefix| text.trim_start().starts_with(prefix)))
            {
                break;
            }
            row += 1;
        }
        structural[start..row].fill(true);
        row_draft(
            &mut primary,
            &lines,
            start,
            row,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-diagnostic-l",
                &serde_json::json!({"v0": (start + 1).to_string()}),
            ),
            "diagnostic",
            false,
        );
    }

    row = 0;
    while row < lines.len() {
        if structural[row] || !is_tree_branch(&lines[row].text) {
            row += 1;
            continue;
        }
        let start = if row > 0
            && !structural[row - 1]
            && !lines[row - 1].trimmed().is_empty()
            && lines[row - 1].trimmed().len() <= 80
        {
            row - 1
        } else {
            row
        };
        let mut end = row + 1;
        while end < lines.len()
            && !structural[end]
            && (is_tree_branch(&lines[end].text)
                || lines[end]
                    .trimmed()
                    .chars()
                    .next()
                    .is_some_and(|character| matches!(character, '│' | '|')))
        {
            end += 1;
        }
        structural[start..end].fill(true);
        row_draft(
            &mut primary,
            &lines,
            start,
            end,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-tree-l",
                &serde_json::json!({"v0": (start + 1).to_string()}),
            ),
            "tree",
            false,
        );
        row = end;
    }

    // A sequence of indented lines is useful even without a visible fence.
    row = 0;
    while row < lines.len() {
        if structural[row]
            || !(lines[row].text.starts_with("    ") || lines[row].text.starts_with('\t'))
            || lines[row].trimmed().is_empty()
        {
            row += 1;
            continue;
        }
        let start = row;
        while row < lines.len()
            && !structural[row]
            && !lines[row].trimmed().is_empty()
            && (lines[row].text.starts_with("    ") || lines[row].text.starts_with('\t'))
        {
            row += 1;
        }
        if row - start >= 2 {
            structural[start..row].fill(true);
            row_draft(
                &mut primary,
                &lines,
                start,
                row,
                ilium_prompts::render_value(
                    "naming/smart_copy/visible-indented-code-l",
                    &serde_json::json!({"v0": (start + 1).to_string()}),
                ),
                "code",
                false,
            );
        }
    }

    let mut headings = Vec::<(usize, u8)>::new();
    row = 0;
    while row < lines.len() {
        if structural[row] {
            row += 1;
            continue;
        }
        if let Some(level) = heading_level(&lines[row].text) {
            structural[row] = true;
            headings.push((row, level));
            row_draft(
                &mut primary,
                &lines,
                row,
                row + 1,
                ilium_prompts::render_value(
                    "naming/smart_copy/heading-l",
                    &serde_json::json!({"v0": (row + 1).to_string()}),
                ),
                "heading",
                false,
            );
        } else if row + 1 < lines.len() && !structural[row + 1] && !lines[row].trimmed().is_empty()
        {
            if let Some(level) = setext_level(&lines[row + 1].text) {
                structural[row..row + 2].fill(true);
                headings.push((row, level));
                row_draft(
                    &mut primary,
                    &lines,
                    row,
                    row + 2,
                    ilium_prompts::render_value(
                        "naming/smart_copy/heading-l",
                        &serde_json::json!({"v0": (row + 1).to_string()}),
                    ),
                    "heading",
                    false,
                );
                row += 1;
            }
        }
        row += 1;
    }
    let visible_end = lines
        .iter()
        .rposition(|line| !line.trimmed().is_empty())
        .map_or(0, |last| last + 1);
    for (position, (start, level)) in headings.iter().enumerate() {
        let end = headings[(position + 1)..]
            .iter()
            .find(|(_, next_level)| next_level <= level)
            .map_or(visible_end, |(next_row, _)| *next_row);
        if end > start + 1 {
            row_draft(
                &mut primary,
                &lines,
                *start,
                end,
                ilium_prompts::render_value(
                    "naming/smart_copy/visible-section-l",
                    &serde_json::json!({"v0": (start + 1).to_string()}),
                ),
                "section",
                true,
            );
        }
    }

    row = 0;
    while row < lines.len() {
        if structural[row] || !is_command(&lines[row].text) {
            row += 1;
            continue;
        }
        structural[row] = true;
        row_draft(
            &mut primary,
            &lines,
            row,
            row + 1,
            ilium_prompts::render_value(
                "naming/smart_copy/prompt-line-l",
                &serde_json::json!({"v0": (row + 1).to_string()}),
            ),
            "command",
            false,
        );
        row += 1;
    }

    row = 0;
    while row < lines.len() {
        if structural[row] || !(is_list(&lines[row].text) || lines[row].trimmed().starts_with("> "))
        {
            row += 1;
            continue;
        }
        let list = is_list(&lines[row].text);
        let start = row;
        row += 1;
        while row < lines.len()
            && !structural[row]
            && !lines[row].trimmed().is_empty()
            && (if list {
                is_list(&lines[row].text) || lines[row].text.starts_with("  ")
            } else {
                lines[row].trimmed().starts_with("> ")
            })
        {
            row += 1;
        }
        structural[start..row].fill(true);
        row_draft(
            &mut primary,
            &lines,
            start,
            row,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-v0-l",
                &serde_json::json!({"v0": (if list { "list" } else { "quote" }).to_string(), "v1": (start + 1).to_string()}),
            ),
            if list { "list" } else { "quote" },
            false,
        );
    }

    row = 0;
    while row < lines.len() {
        if structural[row] || lines[row].trimmed().is_empty() {
            row += 1;
            continue;
        }
        let start = row;
        while row < lines.len() && !structural[row] && !lines[row].trimmed().is_empty() {
            row += 1;
        }
        row_draft(
            &mut primary,
            &lines,
            start,
            row,
            ilium_prompts::render_value(
                "naming/smart_copy/visible-paragraph-l",
                &serde_json::json!({"v0": (start + 1).to_string()}),
            ),
            "paragraph",
            false,
        );
    }

    for (line_row, line) in lines.iter().enumerate() {
        if fenced[line_row] {
            continue;
        }
        let mut index = 0;
        while index < line.chars.len() {
            if line.chars[index] != '`' || is_escaped(&line.chars, index) {
                index += 1;
                continue;
            }
            let width = line.chars[index..]
                .iter()
                .take_while(|ch| **ch == '`')
                .count();
            if let Some(close) = closing_backtick_run(&line.chars, index + width, width) {
                if let Some(span) = line.content_span(line_row, index + width, close) {
                    draft(
                        &mut details,
                        ilium_prompts::render_value(
                            "naming/smart_copy/inline-code-l",
                            &serde_json::json!({"v0": (line_row + 1).to_string()}),
                        ),
                        "inline-code",
                        vec![span],
                    );
                }
                index = close + width;
            } else {
                index += width;
            }
        }
        let mut url_starts = line
            .text
            .match_indices("https://")
            .chain(line.text.match_indices("http://"))
            .map(|(byte_index, _)| line.text[..byte_index].chars().count())
            .collect::<Vec<_>>();
        url_starts.sort_unstable();
        for start in url_starts {
            let mut end = start;
            while end < line.chars.len()
                && !line.chars[end].is_whitespace()
                && !['<', '>', '"', '\'', '`'].contains(&line.chars[end])
            {
                end += 1;
            }
            while end > start && ['.', ',', ';', ':', ')', ']', '}'].contains(&line.chars[end - 1])
            {
                end -= 1;
            }
            if let Some(span) = line.content_span(line_row, start, end) {
                draft(
                    &mut details,
                    ilium_prompts::render_value(
                        "naming/smart_copy/url-l",
                        &serde_json::json!({"v0": (line_row + 1).to_string()}),
                    ),
                    "url",
                    vec![span],
                );
            }
        }
    }

    let mut regions = Vec::new();
    let mut geometries = HashSet::new();
    for mut region in primary.into_iter().chain(details) {
        if regions.len() >= MAXIMUM_DETECTED_CANDIDATES {
            break;
        }
        region
            .spans
            .sort_by_key(|span| (span.row, span.start_column, span.end_column));
        region.spans.dedup();
        if region.spans.len() > MAXIMUM_PARTS_PER_CANDIDATE || geometries.contains(&region.spans) {
            continue;
        }
        let text = region
            .spans
            .iter()
            .map(|span| {
                cell_text(
                    &snapshot.screen,
                    span.row,
                    span.start_column,
                    span.end_column,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            continue;
        }
        let cell_count = region
            .spans
            .iter()
            .map(|span| usize::from(span.end_column - span.start_column + 1))
            .sum();
        geometries.insert(region.spans.clone());
        regions.push(DetectedRegion {
            label: region.label,
            kind: region.kind.to_string(),
            spans: region.spans,
            text,
            cell_count,
        });
    }
    regions
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateSpec {
    label: String,
    kind: String,
    parts: Vec<CandidatePartSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CandidatePartSpec {
    Lines {
        lines: Vec<u16>,
    },
    Range {
        line: u16,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        through: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellSpan {
    pub row: u16,
    pub start_column: u16,
    pub end_column: u16,
}

impl CellSpan {
    pub fn contains(self, row: u16, column: u16) -> bool {
        self.row == row && (self.start_column..=self.end_column).contains(&column)
    }
}

pub struct SmartCopyCandidate {
    pub label: String,
    pub kind: String,
    pub spans: Vec<CellSpan>,
    pub text: String,
    pub cell_count: usize,
    pub arrived_at: Instant,
}

#[derive(Clone)]
struct DetectedRegion {
    label: String,
    kind: String,
    spans: Vec<CellSpan>,
    text: String,
    cell_count: usize,
}

impl DetectedRegion {
    fn candidate(&self) -> SmartCopyCandidate {
        SmartCopyCandidate {
            label: self.label.clone(),
            kind: self.kind.clone(),
            spans: self.spans.clone(),
            text: self.text.clone(),
            cell_count: self.cell_count,
            arrived_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartCopyPhase {
    Connecting,
    Streaming,
    Complete,
    Failed(String),
}

pub struct SmartCopySession {
    pub generation: u64,
    pub pane_id: NodeId,
    pub snapshot: SmartCopySnapshot,
    pub phase: SmartCopyPhase,
    pub candidates: Vec<SmartCopyCandidate>,
    pub received_characters: usize,
    pub exact_output_tokens: Option<u64>,
    pub invalid_lines: usize,
    pub started_at: Instant,
    hovered_cell: Option<(u16, u16)>,
    overlap_index: usize,
    geometries: HashSet<Vec<CellSpan>>,
    /// Indices into `candidates` the user clicked, in click order. Candidates
    /// are only ever appended, so an index stays valid for the whole session.
    selected: Vec<usize>,
}

impl SmartCopySession {
    pub fn new(generation: u64, pane_id: NodeId, snapshot: SmartCopySnapshot) -> Self {
        let geometries = snapshot
            .detected
            .iter()
            .map(|region| region.spans.clone())
            .collect();
        let candidates = snapshot
            .detected
            .iter()
            .map(DetectedRegion::candidate)
            .collect();
        Self {
            generation,
            pane_id,
            snapshot,
            phase: SmartCopyPhase::Connecting,
            candidates,
            received_characters: 0,
            exact_output_tokens: None,
            invalid_lines: 0,
            started_at: Instant::now(),
            hovered_cell: None,
            overlap_index: 0,
            geometries,
            selected: Vec::new(),
        }
    }

    pub fn apply_json_line(&mut self, line: &str) -> Result<bool, String> {
        if self.candidates.len() >= MAXIMUM_CANDIDATES {
            return Ok(false);
        }
        if line.len() > MAXIMUM_JSONL_LINE_BYTES {
            self.invalid_lines += 1;
            return Err(
                ilium_prompts::naming::NAMING_SMART_COPY_JSONL_RECORD_EXCEEDS_64_KIB.to_string(),
            );
        }
        let spec: CandidateSpec = serde_json::from_str(line).map_err(|error| {
            self.invalid_lines += 1;
            ilium_prompts::render_value(
                "naming/smart_copy/invalid-jsonl-candidate",
                &serde_json::json!({"v0": (error).to_string()}),
            )
        })?;
        let candidate = self.snapshot.resolve_candidate(spec).inspect_err(|_| {
            self.invalid_lines += 1;
        })?;
        if !self.geometries.insert(candidate.spans.clone()) {
            return Ok(false);
        }
        self.candidates.push(candidate);
        Ok(true)
    }

    pub fn set_hover(&mut self, content_area: ratatui::layout::Rect, position: Position) {
        let hovered_cell = content_area.contains(position).then_some((
            position.y.saturating_sub(content_area.y),
            position.x.saturating_sub(content_area.x),
        ));
        if self.hovered_cell != hovered_cell {
            self.hovered_cell = hovered_cell;
            self.overlap_index = 0;
        }
    }

    pub fn clear_hover(&mut self) {
        self.hovered_cell = None;
        self.overlap_index = 0;
    }

    pub fn cycle_overlap(&mut self, direction: i32) {
        let count = self.matching_indices().len();
        if count < 2 {
            return;
        }
        self.overlap_index = if direction < 0 {
            self.overlap_index.checked_sub(1).unwrap_or(count - 1)
        } else {
            (self.overlap_index + 1) % count
        };
    }

    pub fn current_candidate(&self) -> Option<&SmartCopyCandidate> {
        let matches = self.matching_indices();
        let index = matches.get(self.overlap_index % matches.len().max(1))?;
        self.candidates.get(*index)
    }

    /// Toggles the hovered candidate in the persistent selection. Returns the
    /// toggled candidate's label and whether it is now selected, or `None`
    /// when nothing is hovered.
    pub fn toggle_current_selection(&mut self) -> Option<(String, bool)> {
        let matches = self.matching_indices();
        let index = *matches.get(self.overlap_index % matches.len().max(1))?;
        let is_selected = if let Some(position) = self.selected.iter().position(|i| *i == index) {
            self.selected.remove(position);
            false
        } else {
            self.selected.push(index);
            true
        };
        Some((self.candidates[index].label.clone(), is_selected))
    }

    pub fn is_selected(&self, candidate_index: usize) -> bool {
        self.selected.contains(&candidate_index)
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// Clipboard text for the whole selection: each selected region's text in
    /// click order, one blank line between regions.
    pub fn selected_text(&self) -> String {
        self.selected
            .iter()
            .filter_map(|index| self.candidates.get(*index))
            .map(|candidate| candidate.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn overlap_position(&self) -> Option<(usize, usize)> {
        let matches = self.matching_indices();
        (!matches.is_empty()).then_some((self.overlap_index % matches.len() + 1, matches.len()))
    }

    fn matching_indices(&self) -> Vec<usize> {
        let Some((row, column)) = self.hovered_cell else {
            return Vec::new();
        };
        let mut matches = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                candidate
                    .spans
                    .iter()
                    .any(|span| span.contains(row, column))
            })
            .map(|(index, candidate)| (index, candidate.cell_count))
            .collect::<Vec<_>>();
        matches.sort_by_key(|(index, cell_count)| (*cell_count, *index));
        matches.into_iter().map(|(index, _)| index).collect()
    }

    pub fn estimated_output_tokens(&self) -> usize {
        self.received_characters.div_ceil(4)
    }
}

#[cfg(test)]
pub fn system_prompt() -> String {
    system_prompt_with_instructions(&ilium_inference::PromptInstructions::default())
}

/// Builds the system request with the current saved selection preferences.
pub fn system_prompt_with_instructions(
    instructions: &ilium_inference::PromptInstructions,
) -> String {
    ilium_prompts::render_value(
        "naming/smart-copy-system",
        &serde_json::json!({
            "smart_copy": instructions.smart_copy.trim(),
        }),
    )
}

pub fn user_prompt(snapshot: &SmartCopySnapshot) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct PromptSpan {
        line: u16,
        from_column: u16,
        through_column: u16,
    }
    #[derive(Serialize)]
    struct PromptRegion<'a> {
        label: &'a str,
        kind: &'a str,
        cells: Vec<PromptSpan>,
    }
    #[derive(Serialize)]
    struct PromptInput<'a> {
        screen: &'a [PromptLine],
        already_detected: Vec<PromptRegion<'a>>,
    }
    let already_detected = snapshot
        .detected
        .iter()
        .map(|region| PromptRegion {
            label: &region.label,
            kind: &region.kind,
            cells: region
                .spans
                .iter()
                .map(|span| PromptSpan {
                    line: span.row + 1,
                    from_column: span.start_column,
                    through_column: span.end_column,
                })
                .collect(),
        })
        .collect();
    let input = PromptInput {
        screen: &snapshot.lines,
        already_detected,
    };
    Ok(ilium_prompts::render_value("naming/smart_copy/frozen-terminal-and-program-detected-selections-follow-as-json-data-cell-columns-in-a", &serde_json::json!({"v0": (serde_json::to_string(&input)?).to_string()})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(lines: &[&str]) -> SmartCopySnapshot {
        let mut parser = vt100::Parser::new(lines.len() as u16, 40, 0);
        parser.process(lines.join("\r\n").as_bytes());
        SmartCopySnapshot::capture(parser.screen())
    }

    #[test]
    fn resolves_word_ranges_to_frozen_source_text() {
        let snapshot = snapshot(&["run cargo test now"]);
        let mut session = SmartCopySession::new(1, NodeId(1), snapshot);
        assert!(session
            .apply_json_line(
                r#"{"label":"command","kind":"command","parts":[{"line":1,"from":"w2","through":"w3"}]}"#,
            )
            .unwrap());
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.text == "cargo test"));
    }

    #[test]
    fn smallest_overlapping_candidate_is_selected_first() {
        let snapshot = snapshot(&["alpha beta gamma"]);
        let mut session = SmartCopySession::new(1, NodeId(1), snapshot);
        assert!(!session
            .apply_json_line(r#"{"label":"line","kind":"line","parts":[{"line":1}]}"#)
            .unwrap());
        session
            .apply_json_line(
                r#"{"label":"word","kind":"word","parts":[{"line":1,"from":"w2","through":"w2"}]}"#,
            )
            .unwrap();
        session.set_hover(ratatui::layout::Rect::new(0, 0, 40, 1), Position::new(7, 0));
        assert_eq!(session.current_candidate().unwrap().label, "word");
        session.cycle_overlap(1);
        assert_eq!(session.current_candidate().unwrap().kind, "paragraph");
    }

    #[test]
    fn clicks_accumulate_a_persistent_selection_and_toggle_off() {
        let mut session = SmartCopySession::new(1, NodeId(1), snapshot(&["alpha beta gamma"]));
        session
            .apply_json_line(
                r#"{"label":"word","kind":"word","parts":[{"line":1,"from":"w2","through":"w2"}]}"#,
            )
            .unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 40, 1);
        session.set_hover(area, Position::new(7, 0));
        assert_eq!(
            session.toggle_current_selection(),
            Some(("word".to_string(), true))
        );
        session.set_hover(area, Position::new(0, 0));
        let (_, is_selected) = session.toggle_current_selection().unwrap();
        assert!(is_selected);
        assert_eq!(session.selected_count(), 2);
        assert!(session.selected_text().contains("beta"));
        assert!(session.selected_text().contains("alpha beta gamma"));
        session.set_hover(area, Position::new(7, 0));
        assert_eq!(
            session.toggle_current_selection(),
            Some(("word".to_string(), false))
        );
        assert_eq!(session.selected_count(), 1);
        session.set_hover(area, Position::new(50, 5));
        assert_eq!(session.toggle_current_selection(), None);
    }

    #[test]
    fn rejects_model_authored_or_out_of_bounds_references() {
        let snapshot = snapshot(&["safe source"]);
        let mut session = SmartCopySession::new(1, NodeId(1), snapshot);
        let initial = session.candidates.len();
        assert!(session
            .apply_json_line(r#"{"label":"fake","kind":"word","parts":[{"line":99,"from":"w1"}]}"#,)
            .is_err());
        assert_eq!(session.candidates.len(), initial);
    }

    #[test]
    fn pre_scan_seeds_sections_paragraphs_code_and_table_cells() {
        let snapshot = snapshot(&[
            "# Chapter",
            "first prose line",
            "second prose line",
            "",
            "## Section",
            "```rust",
            "let name = 1;",
            "```",
            "| Name | Value |",
            "| --- | --- |",
            "| left | right |",
        ]);
        let session = SmartCopySession::new(1, NodeId(1), snapshot.clone());
        for kind in [
            "heading",
            "section",
            "paragraph",
            "fenced-code",
            "code",
            "table",
            "table-cell",
        ] {
            assert!(
                session
                    .candidates
                    .iter()
                    .any(|candidate| candidate.kind == kind),
                "missing {kind}"
            );
        }
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "table-cell" && candidate.text == "right"));
        let prompt = user_prompt(&snapshot).unwrap();
        assert!(prompt.contains("already_detected"));
        assert!(prompt.contains("Visible table L9"));
    }

    #[test]
    fn cell_geometry_survives_wide_and_combining_characters() {
        let snapshot = snapshot(&["| 名 | Val |", "| --- | --- |", "| 漢 | e\u{301} |"]);
        let session = SmartCopySession::new(1, NodeId(1), snapshot);
        let wide = session
            .candidates
            .iter()
            .find(|candidate| candidate.kind == "table-cell" && candidate.text == "漢")
            .unwrap();
        assert_eq!(wide.spans[0].start_column, 2);
        assert_eq!(wide.spans[0].end_column, 3);
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "table-cell" && candidate.text == "e\u{301}"));
    }

    #[test]
    fn pre_scan_finds_terminal_frames_trees_lists_quotes_commands_and_diagnostics() {
        let snapshot = snapshot(&[
            "╭──────╮",
            "│ hello│",
            "╰──────╯",
            "",
            "root/",
            "├── src",
            "└── docs",
            "",
            "- first",
            "- second",
            "",
            "> quoted",
            "",
            "$ cargo test",
            "",
            "error: broken",
            "  --> src/lib.rs:2:3",
            "",
            "see `inline` here",
            "",
            "see https://example.test/x.",
            "",
            "diff --git a/a b/a",
            "--- a/a",
            "+++ b/a",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ]);
        let session = SmartCopySession::new(1, NodeId(1), snapshot);
        for kind in [
            "box",
            "box-contents",
            "tree",
            "list",
            "quote",
            "command",
            "diagnostic",
            "inline-code",
            "url",
            "diff",
        ] {
            assert!(
                session
                    .candidates
                    .iter()
                    .any(|candidate| candidate.kind == kind),
                "missing {kind}"
            );
        }
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "box-contents" && candidate.text == "hello"));
        assert!(
            session
                .candidates
                .iter()
                .any(|candidate| candidate.kind == "url"
                    && candidate.text == "https://example.test/x")
        );
    }

    #[test]
    fn exact_model_repeats_are_dropped_but_distinct_overlaps_remain() {
        let snapshot = snapshot(&["# Alpha beta"]);
        let mut session = SmartCopySession::new(1, NodeId(1), snapshot);
        let initial = session.candidates.len();
        assert!(!session
            .apply_json_line(r#"{"label":"repeat","kind":"line","parts":[{"line":1}]}"#)
            .unwrap());
        assert_eq!(session.candidates.len(), initial);
        assert!(session
            .apply_json_line(
                r#"{"label":"alpha","kind":"word","parts":[{"line":1,"from":"w2","through":"w2"}]}"#
            )
            .unwrap());
        session.set_hover(ratatui::layout::Rect::new(0, 0, 40, 1), Position::new(3, 0));
        let first = session.current_candidate().unwrap().label.clone();
        session.cycle_overlap(1);
        assert_ne!(session.current_candidate().unwrap().label, first);
    }

    #[test]
    fn wide_line_repeat_is_deduplicated_and_model_labels_are_bounded() {
        let snapshot = snapshot(&["漢"]);
        let mut session = SmartCopySession::new(1, NodeId(1), snapshot);
        assert!(!session
            .apply_json_line(r#"{"label":"repeat","kind":"line","parts":[{"line":1}]}"#)
            .unwrap());
        assert!(session
            .apply_json_line(
                "{\"label\":\"bad\\u001b[31m\",\"kind\":\"word\",\"parts\":[{\"line\":1}]}",
            )
            .is_err());
    }

    #[test]
    fn double_line_frames_and_short_tree_branches_are_detected() {
        let snapshot = snapshot(&["╔════╗", "║ one║", "╚════╝", "", "root/", "├─ a", "└─ b"]);
        let session = SmartCopySession::new(1, NodeId(1), snapshot);
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "box" && candidate.text.contains("╔════╗")));
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "box-contents" && candidate.text == "one"));
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "tree" && candidate.text.contains("└─ b")));
    }

    #[test]
    fn ascii_frames_and_codex_output_rows_are_detected() {
        let snapshot = snapshot(&[
            "+--------+",
            "| ready  |",
            "+--------+",
            "",
            "⏺ Read src/main.rs",
            "❯ /help",
        ]);
        let session = SmartCopySession::new(1, NodeId(1), snapshot);
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "box" && candidate.text.contains("+--------+")));
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "box-contents" && candidate.text == "ready"));
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "list" && candidate.text.contains("⏺ Read")));
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "command" && candidate.text == "❯ /help"));
    }

    #[test]
    fn bare_fences_after_markdown_headings_keep_visible_code() {
        for lines in [
            &["# Heading", "```", "body"][..],
            &["Heading", "---", "~~~", "body"][..],
        ] {
            let session = SmartCopySession::new(1, NodeId(1), snapshot(lines));
            assert!(session
                .candidates
                .iter()
                .any(|candidate| candidate.kind == "code" && candidate.text == "body"));
            assert!(session.candidates.iter().any(|candidate| {
                candidate.kind == "fenced-code" && !candidate.text.contains("Heading")
            }));
        }
    }

    #[test]
    fn inline_code_requires_an_exact_backtick_run() {
        let session = SmartCopySession::new(1, NodeId(1), snapshot(&["see ``a```b`` here"]));
        let code = session
            .candidates
            .iter()
            .find(|candidate| candidate.kind == "inline-code")
            .unwrap();
        assert_eq!(code.text, "a```b");
        assert_eq!(code.spans[0].start_column, 6);
        assert_eq!(code.spans[0].end_column, 10);
    }

    #[test]
    fn table_cells_respect_backtick_runs_and_backslash_parity() {
        let session = SmartCopySession::new(
            1,
            NodeId(1),
            snapshot(&[
                "| Name | Value |",
                "| --- | --- |",
                "| ``a`b|c`` | right |",
                r"| left\\| right |",
                r"| left\|inside | right |",
                "| unmatched ` code | right |",
            ]),
        );
        for text in ["``a`b|c``", r"left\\", r"left\|inside"] {
            assert!(session
                .candidates
                .iter()
                .any(|candidate| { candidate.kind == "table-cell" && candidate.text == text }));
        }
        assert_eq!(
            session
                .candidates
                .iter()
                .filter(|candidate| candidate.kind == "table-cell" && candidate.text == "right")
                .count(),
            4
        );
    }

    #[test]
    fn claude_prompt_rows_are_isolated_from_following_prose() {
        let session = SmartCopySession::new(
            1,
            NodeId(1),
            snapshot(&["› explain this code", "model status footer"]),
        );
        assert!(session.candidates.iter().any(|candidate| {
            candidate.kind == "command" && candidate.text == "› explain this code"
        }));
    }

    #[test]
    fn partial_fences_and_candidate_volume_are_bounded() {
        let unfinished_snapshot = snapshot(&["```", "unfinished body"]);
        let session = SmartCopySession::new(1, NodeId(1), unfinished_snapshot);
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "fenced-code"));
        assert!(session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "code"));
        let clipped_close = snapshot(&["code from above", "```", "normal prose"]);
        let session = SmartCopySession::new(3, NodeId(1), clipped_close);
        assert!(!session
            .candidates
            .iter()
            .any(|candidate| candidate.kind == "fenced-code"));
        let mut many_lines = Vec::new();
        for index in 0..600 {
            many_lines.push(format!("$ item {index}"));
            many_lines.push(String::new());
        }
        let borrowed = many_lines.iter().map(String::as_str).collect::<Vec<_>>();
        let many_line_snapshot = snapshot(&borrowed);
        let mut session = SmartCopySession::new(2, NodeId(1), many_line_snapshot);
        assert_eq!(session.candidates.len(), MAXIMUM_DETECTED_CANDIDATES);
        for item in 0..MAXIMUM_DETECTED_CANDIDATES {
            let line = item * 2 + 1;
            let record = serde_json::json!({
                "label": "additional word",
                "kind": "word",
                "parts": [{"line": line, "from": "w2", "through": "w2"}],
            });
            assert!(session.apply_json_line(&record.to_string()).unwrap());
        }
        assert_eq!(session.candidates.len(), MAXIMUM_CANDIDATES);
        assert!(!session
            .apply_json_line(
                r#"{"label":"overflow","kind":"word","parts":[{"line":1,"from":"w2","through":"w2"}]}"#
            )
            .unwrap());
    }

    #[test]
    fn custom_instructions_keep_smart_copy_source_contract_and_literal_text() {
        let instructions = ilium_inference::PromptInstructions {
            smart_copy: "Prefer commands {{> absent}} <x>&".into(),
            ..Default::default()
        };
        let prompt = system_prompt_with_instructions(&instructions);
        assert!(prompt.contains("Prefer commands {{> absent}} <x>&"));
        assert!(prompt.contains("source"));
        assert_eq!(
            system_prompt_with_instructions(&Default::default()),
            system_prompt()
        );
    }
}
