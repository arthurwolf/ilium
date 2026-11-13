//! Pure rules of `CHATROOM.md` records and hook context, moved from `ilium-client/src/chatroom.rs`.
//!
//! No I/O: callers hand in file text and the clock string, and get text and decisions back. The
//! file access (advisory lock, secure open, cursor files) is the `fs` host import (gap G10) and
//! stays outside this module.
//!
//! The record format here is the one the kernel CLI writes: `- <timestamp> | <author> | <content>`
//! where a field escapes backslash, `|`, newline and tab, and drops every other control character.

/// Longest message the writer accepts, in Unicode scalar values.
pub const MAX_CONTENT_CHARS: usize = 4_000;
/// Unread tracking scans at most this many recent records for the reader's cursor.
pub const UNREAD_SCAN_RECORDS: usize = 2_000;
/// A session start or compaction gets this many times the per-turn byte cap.
pub const SESSION_START_BYTE_FACTOR: usize = 4;
/// Bytes kept free for the omitted-records note.
pub const OMITTED_NOTE_RESERVE: usize = 160;
/// The command a lifecycle hook runs; only unread records are printed.
pub const HOOK_COMMAND: &str = "ilium chat context --limit 40 --since-last-read --max-bytes 2048";
/// Any earlier spelling of the hook command starts with this and is upgraded in place.
pub const HOOK_COMMAND_PREFIX: &str = "ilium chat context";
/// Name of the room file in the project root.
pub const CHATROOM_FILE_NAME: &str = "CHATROOM.md";
/// Marker line of a new room file.
pub const CHATROOM_MARKER: &str = "<!-- ilium-chatroom: v1 -->";
/// Archive written when an oversized room is rotated.
pub const CHATROOM_ARCHIVE_IGNORE: &str = "/CHATROOM.archive.md";

/// One parsed record of the room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub timestamp: String,
    pub author: String,
    pub content: String,
}

/// Why a post is refused before any file is touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostError {
    /// Author or content is empty after sanitising.
    Empty,
    /// The content is longer than [`MAX_CONTENT_CHARS`].
    TooLong,
}

impl std::fmt::Display for PostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("chatroom author and message must not be empty"),
            Self::TooLong => {
                formatter.write_str("chatroom messages are limited to 4000 characters")
            }
        }
    }
}

/// Escapes one field for the one-line format.
pub fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// Reverses [`sanitize_field`]; an unknown escape keeps the escaped character.
pub fn unescape_field(value: &str) -> String {
    let mut result = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some(other) => result.push(other),
            None => result.push('\\'),
        }
    }
    result
}

/// Drops raw control bytes from a field as read from the file. The file is human-editable, so a
/// well-formed hand-edited line can carry terminal escapes; a raw tab stays as benign whitespace.
pub fn strip_raw_control_bytes(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect()
}

/// Parses one `- <timestamp> | <author> | <content>` line.
pub fn parse_message_line(line: &str) -> Option<ChatMessage> {
    let line = line.strip_prefix("- ")?;
    let mut fields = line.splitn(3, " | ");
    let mut parse_field =
        || -> Option<String> { Some(unescape_field(&strip_raw_control_bytes(fields.next()?))) };
    Some(ChatMessage {
        timestamp: parse_field()?,
        author: parse_field()?,
        content: parse_field()?,
    })
}

/// All records of a room file, oldest first. Lines outside the record shape are ignored.
pub fn parse_messages(contents: &str) -> Vec<ChatMessage> {
    contents.lines().filter_map(parse_message_line).collect()
}

/// The newest `limit` records of a room file (none for a zero limit).
pub fn tail_messages(contents: &str, limit: usize) -> Vec<ChatMessage> {
    if limit == 0 {
        return Vec::new();
    }
    let mut messages = parse_messages(contents);
    let keep_from = messages.len().saturating_sub(limit);
    messages.split_off(keep_from)
}

/// Validates a post and returns the exact record line to append, with the line end, preceded by a
/// line end when the file does not end in one (a human edit may have dropped it).
pub fn format_post(
    existing_contents: &str,
    timestamp: &str,
    author: &str,
    content: &str,
) -> Result<String, PostError> {
    let author = sanitize_field(author);
    let content = sanitize_field(content);
    if author.is_empty() || content.is_empty() {
        return Err(PostError::Empty);
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(PostError::TooLong);
    }
    let mut text = String::new();
    if !existing_contents.is_empty() && !existing_contents.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("- {timestamp} | {author} | {content}\n"));
    Ok(text)
}

/// Replaces control characters of a field for hook output: line breaks and tabs become their
/// escaped spelling, other controls vanish, so the model context never carries terminal codes.
pub fn flatten_for_hook_output(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            other if other.is_control() => String::new(),
            other => other.to_string(),
        })
        .collect()
}

/// Substitutes `{{v0}}`, `{{v1}}`, ... of a catalogue template; no escaping, like the kernel.
pub fn render_template(template: &str, values: &[&str]) -> String {
    let mut rendered = template.to_string();
    for (index, value) in values.iter().enumerate() {
        rendered = rendered.replace(&format!("{{{{v{index}}}}}"), value);
    }
    rendered
}

const CONTEXT_ROW_TEMPLATE: &str = include_str!("../templates/chatroom-context-row.hbs");
const OMITTED_TEMPLATE: &str = include_str!("../templates/chatroom-omitted.hbs");
const CONTEXT_TEMPLATE: &str = include_str!("../templates/chatroom-context.hbs");
const EMPTY_CONTEXT_TEMPLATE: &str = include_str!("../templates/chatroom-empty-context.hbs");
const UNREAD_CONTEXT_TEMPLATE: &str = include_str!("../templates/chatroom-unread-context.hbs");
const HEADER_GUIDANCE_TEMPLATE: &str = include_str!("../templates/chatroom-header-guidance.hbs");
const TITLE_TEMPLATE: &str = include_str!("../templates/chatroom-title.hbs");
const MESSAGES_HEADING_TEMPLATE: &str = include_str!("../templates/chatroom-messages-heading.hbs");
/// The posting threshold every header repeats.
pub const COORDINATION_GUIDANCE: &str = include_str!("../templates/coordination-guidance.hbs");

/// The header of a full (non-unread) context print.
pub fn full_context_header(messages: &[ChatMessage]) -> String {
    let template = if messages.is_empty() {
        EMPTY_CONTEXT_TEMPLATE
    } else {
        CONTEXT_TEMPLATE
    };
    render_template(template, &[COORDINATION_GUIDANCE])
}

/// One context row, with every field flattened.
pub fn render_context_row(message: &ChatMessage) -> String {
    render_template(
        CONTEXT_ROW_TEMPLATE,
        &[
            &flatten_for_hook_output(&message.timestamp),
            &flatten_for_hook_output(&message.author),
            &flatten_for_hook_output(&message.content),
        ],
    )
}

/// Cuts `row` to `budget` bytes at a character boundary and ends it with an ellipsis line.
pub fn truncate_row(row: &str, budget: usize) -> String {
    let marker = "…\n";
    let keep = budget.saturating_sub(marker.len());
    let mut end = keep.min(row.len());
    while !row.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{marker}", &row[..end])
}

/// Renders `header` plus the newest of `messages` (at most `limit`) that fit in `max_bytes`.
/// Newer records win; a single oversized newest record is cut, never dropped; the records that do
/// not fit are counted in a note.
pub fn render_capped(
    header: String,
    messages: &[ChatMessage],
    limit: usize,
    max_bytes: usize,
) -> String {
    let candidates = &messages[messages.len().saturating_sub(limit)..];
    let mut budget = max_bytes
        .saturating_sub(header.len())
        .saturating_sub(OMITTED_NOTE_RESERVE);
    let mut rows = Vec::new();
    for message in candidates.iter().rev() {
        let row = render_context_row(message);
        if row.len() <= budget {
            budget -= row.len();
            rows.push(row);
            continue;
        }
        if rows.is_empty() {
            rows.push(truncate_row(&row, budget));
        }
        break;
    }
    let omitted = messages.len() - rows.len();
    let mut output = header;
    if omitted > 0 {
        output.push_str(&render_template(
            OMITTED_TEMPLATE,
            &[&omitted.to_string(), &messages.len().to_string()],
        ));
    }
    for row in rows.into_iter().rev() {
        output.push_str(&row);
    }
    output
}

/// Context without unread tracking: the newest `limit` records, capped.
pub fn capped_context(contents: &str, limit: usize, max_bytes: usize) -> String {
    let messages = tail_messages(contents, limit);
    render_capped(full_context_header(&messages), &messages, limit, max_bytes)
}

/// Context holding every record, uncapped.
pub fn context(contents: &str, limit: usize) -> String {
    let messages = tail_messages(contents, limit);
    let mut output = full_context_header(&messages);
    for message in &messages {
        output.push_str(&render_context_row(message));
    }
    output
}

/// A stable opaque identity of one record, stored as the reader's cursor (128-bit FNV-1a over the
/// three fields, each ended by a zero byte).
pub fn message_fingerprint(message: &ChatMessage) -> String {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET;
    for field in [&message.timestamp, &message.author, &message.content] {
        for byte in field.bytes().chain(std::iter::once(0)) {
            hash ^= u128::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    format!("{hash:032x}")
}

/// The file name of one reader's cursor: only ASCII letters, digits, `-` and `_` of the key.
pub fn reader_cursor_file_name(reader_key: &str) -> String {
    let safe: String = reader_key
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .take(96)
        .collect();
    format!("{safe}.cursor")
}

/// Who is reading the room through a lifecycle hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadReader<'a> {
    /// Stable reader identity, such as an Ilium pane id or an agent session id.
    pub key: &'a str,
    /// A session start or compaction: the reader holds no earlier chatroom context.
    pub is_session_start: bool,
}

/// What an unread read decides, plus the cursor to store afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadOutcome {
    /// The text to print (empty when nothing is new).
    pub output: String,
    /// The fingerprint to store as the reader's cursor, when the room has any record.
    pub new_cursor: Option<String>,
}

/// Hook context holding only the records this reader has not seen, newest last, within
/// `max_bytes`. `stored_cursor` is the fingerprint read from the reader's cursor file. A session
/// start, a first read or a cursor that rotated out of the room gets the full header and
/// [`SESSION_START_BYTE_FACTOR`] times the budget.
pub fn unread_context(
    contents: &str,
    limit: usize,
    max_bytes: usize,
    reader: &UnreadReader<'_>,
    stored_cursor: Option<&str>,
) -> UnreadOutcome {
    let all = parse_messages(contents);
    let messages = &all[all.len().saturating_sub(UNREAD_SCAN_RECORDS)..];
    let unread_start = if reader.is_session_start {
        None
    } else {
        stored_cursor.and_then(|fingerprint| {
            messages
                .iter()
                .rposition(|message| message_fingerprint(message) == fingerprint)
                .map(|index| index + 1)
        })
    };
    let new_cursor = messages.last().map(message_fingerprint);
    let output = match unread_start {
        Some(start) if start == messages.len() => String::new(),
        Some(start) => render_capped(
            UNREAD_CONTEXT_TEMPLATE.to_string(),
            &messages[start..],
            limit,
            max_bytes,
        ),
        None => {
            let recent = &messages[messages.len().saturating_sub(limit)..];
            render_capped(
                full_context_header(recent),
                recent,
                limit,
                max_bytes.saturating_mul(SESSION_START_BYTE_FACTOR),
            )
        }
    };
    UnreadOutcome { output, new_cursor }
}

/// The text of a new room file: title, marker, guidance, messages heading.
pub fn new_room_text() -> String {
    let guidance = render_template(HEADER_GUIDANCE_TEMPLATE, &[COORDINATION_GUIDANCE]);
    format!(
        "{}\n{CHATROOM_MARKER}\n\n{}\n\n{}\n",
        TITLE_TEMPLATE.trim_end_matches('\n'),
        guidance.trim_end_matches('\n'),
        MESSAGES_HEADING_TEMPLATE.trim_end_matches('\n'),
    )
}

/// Returns `gitignore` with the room entries added, or `None` when both are already present.
pub fn gitignore_with_room(gitignore: &str) -> Option<String> {
    let missing: Vec<&str> = ["/CHATROOM.md", CHATROOM_ARCHIVE_IGNORE]
        .into_iter()
        .filter(|entry| !gitignore.lines().any(|line| line.trim() == *entry))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut text = gitignore.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for entry in missing {
        text.push_str(entry);
        text.push('\n');
    }
    Some(text)
}
