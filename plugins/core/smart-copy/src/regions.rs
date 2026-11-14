//! Structural region detection over the visible rows of a terminal: fenced and indented code, tables,
//! frames, trees, lists, quotes, headings and paragraphs, commands, diffs and diagnostics, plus the
//! fine-grained tokens of [`crate::recognise`] found inside them.
//!
//! The rows arrive as plain text (`terminal.read-screen`), so a span is a range of characters of
//! one row, not a range of terminal cells. Regions may nest; the caller decides what to offer.

use std::collections::HashMap;

use crate::recognise;

/// Most regions one scan reports. Half of the 2048 an offer may hold, the rest is for contributors.
pub const MAXIMUM_DETECTED_CANDIDATES: usize = 1024;
/// Most rows one region may span before it is skipped as too large to be a selection.
pub const MAXIMUM_PARTS_PER_CANDIDATE: usize = 128;

/// A range of characters of one row, both ends inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellSpan {
    pub row: u16,
    pub start_column: u16,
    pub end_column: u16,
}

/// One region the scan found.
#[derive(Debug, Clone)]
pub struct DetectedRegion {
    pub label: String,
    pub kind: String,
    pub spans: Vec<CellSpan>,
    pub text: String,
    pub cell_count: usize,
}

struct ScanLine {
    text: String,
    chars: Vec<char>,
    start_columns: Vec<u16>,
    end_columns: Vec<u16>,
}

impl ScanLine {
    fn new(text: &str) -> Self {
        let mut line = Self {
            text: String::new(),
            chars: Vec::new(),
            start_columns: Vec::new(),
            end_columns: Vec::new(),
        };
        // Columns are character indices of the row text: a plugin reads rows as text, not cells.
        for (column, character) in text.chars().enumerate() {
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            line.text.push(character);
            line.chars.push(character);
            line.start_columns.push(column);
            line.end_columns.push(column);
        }
        line
    }

    /// The characters a span covers, without trailing blanks.
    fn text_of(&self, span: CellSpan) -> String {
        let start = usize::from(span.start_column).min(self.chars.len());
        let end = (usize::from(span.end_column) + 1).min(self.chars.len());
        self.chars[start..end.max(start)]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string()
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
        format!("{} (visible excerpt)", label)
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
    ["$ ", "❯ ", "› ", "PS> ", "C:\\> "]
        .iter()
        .any(|prefix| text.starts_with(prefix))
        || text.split_once("$ ").is_some_and(|(prompt, _)| {
            prompt.contains('@') && prompt.len() <= 60 && !prompt.contains(' ')
        })
}

fn is_diff_start(text: &str) -> bool {
    text.starts_with("diff --git ")
        || text.starts_with("@@ ")
        || text.starts_with("--- a/")
        || text.starts_with("*** Begin Patch")
}

fn is_diagnostic_start(text: &str) -> bool {
    let text = text.trim_start();
    [
        "error:",
        "warning:",
        "error[",
        "Traceback ",
        "Exception:",
        "Caused by:",
        "thread '",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

fn is_tree_branch(text: &str) -> bool {
    ["├─", "└─", "+--", "|--", "\\--"]
        .iter()
        .any(|marker| text.contains(marker))
}

/// Every structure and token the visible rows offer, most specific first.
pub fn detect_regions(rows: &[String]) -> Vec<DetectedRegion> {
    let lines = rows
        .iter()
        .map(|row| ScanLine::new(row))
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
                format!("Visible fenced block L{}", row + 1),
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
                    format!("Code contents L{}", row + 1),
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
            format!("Visible table L{}", row + 1),
            "table",
            true,
        );
        for table_row in std::iter::once(row).chain((row + 2)..end) {
            for (column, (start, stop)) in table_cells(&lines[table_row]).into_iter().enumerate() {
                if let Some(span) = lines[table_row].content_span(table_row, start, stop) {
                    draft(
                        &mut details,
                        format!("Table cell L{} C{}", table_row + 1, column + 1),
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
            format!("Visible terminal frame L{}", start + 1),
            "box",
            true,
        );
        let interior = (start..row)
            .filter_map(|line_row| box_interior(&lines[line_row], line_row))
            .take(MAXIMUM_PARTS_PER_CANDIDATE)
            .collect();
        draft(
            &mut details,
            format!("Frame contents L{}", start + 1),
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
                    || text.starts_with("diff ")
                    || text.starts_with("index ")
                    || text.starts_with("new file ")
                    || text.starts_with("deleted file "))
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
            format!("Visible diff L{}", start + 1),
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
                || ["at ", "Caused by:", "--> "]
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
            format!("Visible diagnostic L{}", start + 1),
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
            format!("Visible tree L{}", start + 1),
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
                format!("Visible indented code L{}", start + 1),
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
                format!("Heading L{}", row + 1),
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
                    format!("Heading L{}", row + 1),
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
                format!("Visible section L{}", start + 1),
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
            format!("Prompt line L{}", row + 1),
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
            format!(
                "Visible {} L{}",
                if list { "list" } else { "quote" },
                start + 1
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
            format!("Visible paragraph L{}", start + 1),
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
                        format!("Inline code L{}", line_row + 1),
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
                    format!("URL L{}", line_row + 1),
                    "url",
                    vec![span],
                );
            }
        }
    }

    // Every non-empty line is selectable on its own, so clicking anywhere on a
    // line that is not covered by a smaller region (a URL, a path, ...) selects
    // the whole line.
    let mut line_regions = Vec::new();
    for (line_row, line) in lines.iter().enumerate() {
        if let Some(span) = line.full_span(line_row) {
            draft(
                &mut line_regions,
                format!("line {}", line_row + 1),
                "line",
                vec![span],
            );
        }
        if let Some(start) = is_command_prefix_end(&line.text) {
            if let Some(span) = line.content_span(line_row, start, line.chars.len()) {
                draft(
                    &mut line_regions,
                    format!("command text on line {}", line_row + 1),
                    "command-text",
                    vec![span],
                );
            }
        }
    }

    // Tokens: URLs, paths, qualified names, addresses, values and so on.
    let mut token_regions = Vec::new();
    for (line_row, line) in lines.iter().enumerate() {
        for token in recognise::scan_line(&line.text) {
            if let Some(span) = line.content_span(line_row, token.start, token.end) {
                draft(&mut token_regions, token.label, token.kind, vec![span]);
            }
        }
    }
    let row_texts = lines
        .iter()
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();
    for address in recognise::scan_postal_addresses(&row_texts) {
        let spans = address
            .rows
            .iter()
            .filter_map(|(row, start, end)| lines[*row].content_span(*row, *start, *end))
            .collect::<Vec<_>>();
        draft(&mut token_regions, address.label, address.kind, spans);
    }

    let mut regions: Vec<DetectedRegion> = Vec::new();
    let mut geometries: HashMap<Vec<CellSpan>, usize> = HashMap::new();
    for mut region in primary
        .into_iter()
        .chain(details)
        .chain(token_regions)
        .chain(line_regions)
    {
        if regions.len() >= MAXIMUM_DETECTED_CANDIDATES {
            break;
        }
        region
            .spans
            .sort_by_key(|span| (span.row, span.start_column, span.end_column));
        region.spans.dedup();
        if region.spans.len() > MAXIMUM_PARTS_PER_CANDIDATE {
            continue;
        }
        if let Some(&existing) = geometries.get(&region.spans) {
            // A specific kind (an address, a path) that covers exactly what a
            // generic line or paragraph covers names that region better.
            let is_generic = |kind: &str| matches!(kind, "line" | "paragraph");
            if is_generic(&regions[existing].kind) && !is_generic(region.kind) {
                regions[existing].kind = region.kind.to_string();
                regions[existing].label = region.label;
            }
            continue;
        }
        let text = region
            .spans
            .iter()
            .map(|span| lines[usize::from(span.row)].text_of(*span))
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
        geometries.insert(region.spans.clone(), regions.len());
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

/// Character index just after a shell or agent prompt marker, if the line
/// starts with one followed by a command.
fn is_command_prefix_end(text: &str) -> Option<usize> {
    let trimmed = text.trim_start();
    let indent = text.chars().count() - trimmed.chars().count();
    let marker = ["$ ", "❯ ", "› ", "> ", "# "]
        .iter()
        .find(|marker| trimmed.starts_with(**marker))?;
    let end = indent + marker.chars().count();
    (trimmed.chars().count() > marker.chars().count()).then_some(end)
}
