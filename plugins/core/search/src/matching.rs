//! Query matching over immutable source snapshots: terminal histories (control sequences
//! stripped, raw jump offsets kept) and local text fragments. Pure: no host call, no clock.
//! Rules ported from the built-in workspace search (case-insensitive ASCII fold, 72 characters
//! of context on each side, at most 800 results, newlines collapsed in context text).

/// Most results one scan reports; keeps a giant generated file from flooding the list.
pub const MAX_RESULTS: usize = 800;
/// Characters of surrounding context kept on each side of a match.
pub const CONTEXT_CHARS: usize = 72;
/// Longest "last command" shown for a terminal source.
pub const LAST_COMMAND_CHARS: usize = 72;

/// The kind of object a result belongs to; decides the label and how the result opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Agent,
    Shell,
    File,
    Board,
}

impl ObjectKind {
    /// The upper-case label drawn beside a result.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Agent => "AGENT",
            Self::Shell => "SHELL",
            Self::File => "FILE",
            Self::Board => "BOARD",
        }
    }

    /// The wire name used in plugin messages.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
            Self::File => "file",
            Self::Board => "board",
        }
    }

    /// Parses [`ObjectKind::wire_name`]; anything else is refused.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "agent" => Some(Self::Agent),
            "shell" => Some(Self::Shell),
            "file" => Some(Self::File),
            "board" => Some(Self::Board),
            _ => None,
        }
    }
}

/// A direct navigation point in a local pane buffer or a terminal's raw retained output.
/// Terminal offsets are exclusive byte ends so the matching text has already been fed when
/// the view rebuilds around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Terminal { history_end_byte: usize },
    Editor { line: usize },
    Board,
}

/// A compact, display-ready hit. Splitting the context around the match lets a renderer
/// highlight only the matched term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub pane_id: u64,
    pub kind: ObjectKind,
    pub object_name: String,
    pub automatic_title: Option<String>,
    pub path: Option<String>,
    pub last_command: Option<String>,
    pub before: String,
    pub matched: String,
    pub after: String,
    pub location: Location,
}

/// Searchable content of one source snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// The raw retained terminal output, escape sequences included.
    Terminal(Vec<u8>),
    /// Local text fragments, each with the location to open once it matches.
    Text(Vec<TextFragment>),
}

/// A local text fragment and the location to open when it matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFragment {
    pub text: String,
    pub location: Location,
}

/// One immutable source handed to a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub pane_id: u64,
    pub kind: ObjectKind,
    pub object_name: String,
    pub automatic_title: Option<String>,
    pub path: Option<String>,
    pub last_command: Option<String>,
    pub content: Content,
}

/// One scan request. The revision makes late results harmless after the user kept typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub revision: u64,
    pub query: String,
    pub sources: Vec<Source>,
}

/// Scans an immutable snapshot. `should_cancel` is polled between matches and every 4096
/// bytes of terminal history; a cancelled terminal strip publishes no partial matches.
pub fn search_workspace(request: &Request, should_cancel: impl Fn() -> bool) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for source in &request.sources {
        if should_cancel() || results.len() >= MAX_RESULTS {
            break;
        }
        match &source.content {
            Content::Terminal(history) => {
                let Some(searchable) = strip_terminal_controls_bounded(history, &should_cancel)
                else {
                    return Vec::new();
                };
                let last_command = last_command_from_text(&searchable.text);
                find_terminal_results(
                    &searchable,
                    history.len(),
                    &request.query,
                    |before, matched, after, history_end_byte| {
                        result_from_source(
                            source,
                            before,
                            matched,
                            after,
                            Location::Terminal { history_end_byte },
                            last_command.clone(),
                        )
                    },
                    &mut results,
                    &should_cancel,
                );
            }
            Content::Text(fragments) => {
                for fragment in fragments {
                    if should_cancel() || results.len() >= MAX_RESULTS {
                        break;
                    }
                    let location = fragment.location;
                    let line = match location {
                        Location::Editor { line } => line,
                        Location::Terminal { .. } | Location::Board => 0,
                    };
                    find_text_results_bounded(
                        &fragment.text,
                        &request.query,
                        |before, matched, after, _| {
                            result_from_source(source, before, matched, after, location, None)
                        },
                        line,
                        &mut results,
                        &should_cancel,
                    );
                }
            }
        }
    }
    results
}

/// Builds one result from a source and the three context slices. `last_command` of the
/// scan wins over the source's own.
pub fn result_from_source(
    source: &Source,
    before: &str,
    matched: &str,
    after: &str,
    location: Location,
    last_command: Option<String>,
) -> SearchResult {
    SearchResult {
        pane_id: source.pane_id,
        kind: source.kind,
        object_name: source.object_name.clone(),
        automatic_title: source.automatic_title.clone(),
        path: source.path.clone(),
        last_command: last_command.or_else(|| source.last_command.clone()),
        before: single_line(before),
        matched: single_line(matched),
        after: single_line(after),
        location,
    }
}

/// Collapses embedded newlines into spaces: context slices are cut without regard to line
/// boundaries and are drawn in one row.
pub fn single_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character == '\n' || character == '\r' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// Finds all bounded, case-insensitive matches in a plain local document. `line` is the
/// editor line the fragment starts on (zero for a board card).
pub fn find_text_results(
    text: &str,
    query: &str,
    make_result: impl Fn(&str, &str, &str, usize) -> SearchResult,
    line: usize,
    results: &mut Vec<SearchResult>,
) {
    find_text_results_bounded(text, query, make_result, line, results, &|| false);
}

fn find_text_results_bounded(
    text: &str,
    query: &str,
    make_result: impl Fn(&str, &str, &str, usize) -> SearchResult,
    line: usize,
    results: &mut Vec<SearchResult>,
    should_cancel: &impl Fn() -> bool,
) {
    if query.is_empty() || results.len() >= MAX_RESULTS {
        return;
    }
    let folded_text = text.to_ascii_lowercase();
    let folded_query = query.to_ascii_lowercase();
    let mut start = 0;
    while let Some(found) = folded_text[start..].find(&folded_query) {
        if should_cancel() {
            return;
        }
        let match_start = start + found;
        let match_end = match_start + folded_query.len();
        if !text.is_char_boundary(match_start) || !text.is_char_boundary(match_end) {
            start = match_end;
            continue;
        }
        let (before, matched, after) = context_parts(text, match_start, match_end);
        results.push(make_result(before, matched, after, line));
        if results.len() >= MAX_RESULTS {
            return;
        }
        start = match_end.max(start + 1);
        if start >= folded_text.len() {
            return;
        }
    }
}

/// Searches text already stripped of terminal control sequences. `raw_len` is the fallback
/// jump offset when a match's offset-table lookup misses.
pub fn find_terminal_results(
    searchable: &SearchableTerminalText,
    raw_len: usize,
    query: &str,
    make_result: impl Fn(&str, &str, &str, usize) -> SearchResult,
    results: &mut Vec<SearchResult>,
    should_cancel: &impl Fn() -> bool,
) {
    if query.is_empty() || results.len() >= MAX_RESULTS {
        return;
    }
    let folded_text = searchable.text.to_ascii_lowercase();
    let folded_query = query.to_ascii_lowercase();
    let mut start = 0;
    while let Some(found) = folded_text[start..].find(&folded_query) {
        if should_cancel() {
            return;
        }
        let match_start = start + found;
        let match_end = match_start + folded_query.len();
        if !searchable.text.is_char_boundary(match_start)
            || !searchable.text.is_char_boundary(match_end)
        {
            start = match_end;
            continue;
        }
        let (before, matched, after) = context_parts(&searchable.text, match_start, match_end);
        let history_end_byte = searchable
            .raw_ends
            .get(match_end.saturating_sub(1))
            .copied()
            .map(|raw_end| raw_end as usize)
            .unwrap_or(raw_len);
        results.push(make_result(before, matched, after, history_end_byte));
        if results.len() >= MAX_RESULTS {
            return;
        }
        start = match_end.max(start + 1);
        if start >= folded_text.len() {
            return;
        }
    }
}

/// The most recent shell prompt line of already-stripped terminal text, without its prompt.
pub fn last_command_from_text(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        ["$ ", "❯ ", "> "]
            .iter()
            .find_map(|prompt| line.strip_prefix(prompt))
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(|command| truncate(command, LAST_COMMAND_CHARS))
    })
}

/// Splits `text` into the context before, the match, and the context after.
pub fn context_parts(text: &str, match_start: usize, match_end: usize) -> (&str, &str, &str) {
    let before_start = text[..match_start]
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARS - 1)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let after_end = text[match_end..]
        .char_indices()
        .nth(CONTEXT_CHARS)
        .map(|(index, _)| match_end + index)
        .unwrap_or(text.len());
    (
        &text[before_start..match_start],
        &text[match_start..match_end],
        &text[match_end..after_end],
    )
}

/// Cuts `value` to `max_chars` characters, appending an ellipsis when something was cut.
pub fn truncate(value: &str, max_chars: usize) -> String {
    let end = value
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    let suffix = if end < value.len() { "…" } else { "" };
    format!("{}{}", &value[..end], suffix)
}

/// Terminal history with escape sequences removed, plus for every text byte the exclusive
/// end offset of the raw byte it came from (the jump target of a match ending there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchableTerminalText {
    pub text: String,
    pub raw_ends: Vec<u32>,
}

/// Strips control sequences from `bytes`, or returns `None` when `should_cancel` fired.
pub fn strip_terminal_controls_bounded(
    bytes: &[u8],
    should_cancel: &impl Fn() -> bool,
) -> Option<SearchableTerminalText> {
    let mut text = String::with_capacity(bytes.len());
    let mut raw_ends = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut cancellation_boundary = 0;
    while index < bytes.len() {
        if index >= cancellation_boundary && should_cancel() {
            return None;
        }
        if index >= cancellation_boundary {
            cancellation_boundary = index.saturating_add(4096);
        }
        if bytes[index] == 0x1b {
            index = skip_escape(bytes, index);
            continue;
        }
        let byte = bytes[index];
        if byte == b'\r' {
            // A PTY line ending is `\r\n`; one `\n` per pair keeps the text free of blank lines.
            if bytes.get(index + 1) == Some(&b'\n') {
                push_mapped_char(&mut text, &mut raw_ends, '\n', index + 2);
                index += 2;
            } else {
                push_mapped_char(&mut text, &mut raw_ends, '\n', index + 1);
                index += 1;
            }
            continue;
        }
        if byte == b'\n' {
            push_mapped_char(&mut text, &mut raw_ends, '\n', index + 1);
            index += 1;
            continue;
        }
        if byte < 0x20 || byte == 0x7f {
            index += 1;
            continue;
        }
        if let Some(character) = decode_next_char(&bytes[index..]) {
            let end = index + character.len_utf8();
            push_mapped_char(&mut text, &mut raw_ends, character, end);
            index = end;
        } else {
            index += 1;
        }
    }
    Some(SearchableTerminalText { text, raw_ends })
}

/// Strips control sequences without cancellation.
pub fn strip_terminal_controls(bytes: &[u8]) -> SearchableTerminalText {
    strip_terminal_controls_bounded(bytes, &|| false).unwrap_or(SearchableTerminalText {
        text: String::new(),
        raw_ends: Vec::new(),
    })
}

/// Decodes the single UTF-8 character at the front of `remaining`, tolerating garbage after
/// it (the probe is bounded to four bytes, so stripping stays linear).
fn decode_next_char(remaining: &[u8]) -> Option<char> {
    let probe = &remaining[..remaining.len().min(4)];
    match std::str::from_utf8(probe) {
        Ok(valid) => valid.chars().next(),
        Err(error) if error.valid_up_to() > 0 => std::str::from_utf8(&probe[..error.valid_up_to()])
            .ok()
            .and_then(|valid| valid.chars().next()),
        Err(_) => None,
    }
}

fn skip_escape(bytes: &[u8], index: usize) -> usize {
    let Some(next) = bytes.get(index + 1).copied() else {
        return bytes.len();
    };
    if next == b'[' {
        let mut cursor = index + 2;
        while cursor < bytes.len() {
            if (0x40..=0x7e).contains(&bytes[cursor]) {
                return cursor + 1;
            }
            cursor += 1;
        }
        return bytes.len();
    }
    // OSC, DCS, SOS, PM and APC strings end at BEL or ST (`ESC \`).
    if matches!(next, b']' | b'P' | b'X' | b'^' | b'_') {
        let mut cursor = index + 2;
        while cursor < bytes.len() {
            if bytes[cursor] == 0x07 {
                return cursor + 1;
            }
            if bytes[cursor] == 0x1b && bytes.get(cursor + 1) == Some(&b'\\') {
                return cursor + 2;
            }
            cursor += 1;
        }
        return bytes.len();
    }
    (index + 2).min(bytes.len())
}

fn push_mapped_char(text: &mut String, raw_ends: &mut Vec<u32>, character: char, raw_end: usize) {
    let raw_end = u32::try_from(raw_end).unwrap_or(u32::MAX);
    text.push(character);
    raw_ends.extend(std::iter::repeat_n(raw_end, character.len_utf8()));
}
