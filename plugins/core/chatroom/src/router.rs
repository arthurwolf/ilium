//! Reference routing of new `CHATROOM.md` records, moved from `ilium-server/src/chatroom_router.rs`.
//!
//! The server polls the file; this module holds the decisions that need no file or pane: how many
//! bytes and records one poll may read, where a poll resumes after a rotation, how a batch of
//! bytes splits into records, which `@name` references a record carries, and where the cursor
//! stands after a partial delivery. Delivery itself goes through the fleet `deliver` service
//! (gap G3) and the file read through the `fs` host import (gap G10).

/// Most bytes of one poll.
pub const MAX_BATCH_BYTES: usize = 64 * 1024;
/// Most records of one poll.
pub const MAX_BATCH_RECORDS: usize = 64;
/// The writer accepts 4,000 scalar values at up to four bytes; leave room for timestamp, author,
/// separators and escaping without letting one record monopolise a batch.
pub const MAX_RECORD_BYTES: usize = 20 * 1024;

/// One record as the router needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub author: String,
    pub content: String,
}

/// Where a poll stands in the file: the identity of the file and the next byte to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileCursor {
    /// Device and inode (or the platform's equivalent) of the file last read.
    pub generation: Option<(u64, u64)>,
    pub byte_offset: u64,
}

impl FileCursor {
    pub fn new(byte_offset: u64) -> Self {
        Self {
            generation: None,
            byte_offset,
        }
    }
}

/// One record of a batch with the cursor that follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRecord {
    pub author: String,
    pub content: String,
    pub next_cursor: FileCursor,
}

/// The records of one poll and the cursor after the whole batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordBatch {
    pub next_cursor: FileCursor,
    pub records: Vec<PendingRecord>,
}

/// Why a poll is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchError {
    /// One line is longer than [`MAX_RECORD_BYTES`]; the cursor does not advance.
    OversizedRecord,
    /// The byte offset would overflow.
    OffsetOverflow,
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OversizedRecord => formatter.write_str("chatroom record exceeds byte limit"),
            Self::OffsetOverflow => formatter.write_str("chatroom offset overflow"),
        }
    }
}

/// The offset a poll starts from: the cursor's, unless the file was replaced (another generation)
/// or truncated below it, in which case the poll restarts at the top.
pub fn start_offset(cursor: FileCursor, generation: (u64, u64), file_length: u64) -> u64 {
    if cursor.generation != Some(generation) || file_length < cursor.byte_offset {
        0
    } else {
        cursor.byte_offset
    }
}

/// Parses `- <timestamp> | <author> | <content>` as the router needs it: both fields unescaped, a
/// trailing lone backslash makes the record invalid.
pub fn parse_record(line: &str) -> Option<Record> {
    let mut fields = line.strip_prefix("- ")?.splitn(3, " | ");
    let _timestamp = fields.next()?;
    let author = unescape(fields.next()?)?;
    let content = unescape(fields.next()?)?;
    Some(Record { author, content })
}

/// Reverses the writer's escaping; `None` for a field that ends inside an escape.
pub fn unescape(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next()? {
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                '\\' => output.push('\\'),
                other => output.push(other),
            }
        } else {
            output.push(character);
        }
    }
    Some(output)
}

/// Splits the bytes read from `offset` (at most [`MAX_BATCH_BYTES`]) into records. Only complete
/// lines are consumed: an unterminated tail waits for the next poll. At most
/// [`MAX_BATCH_RECORDS`] lines are consumed per poll.
pub fn parse_batch(
    bytes: &[u8],
    offset: u64,
    generation: (u64, u64),
) -> Result<RecordBatch, BatchError> {
    let mut records = Vec::with_capacity(MAX_BATCH_RECORDS.min(bytes.len() / 8 + 1));
    let mut line_start = 0;
    let mut consumed = 0;
    let mut processed_lines = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let mut line = &bytes[line_start..index];
        if let Some(without_carriage_return) = line.strip_suffix(b"\r") {
            line = without_carriage_return;
        }
        if line.len() > MAX_RECORD_BYTES {
            return Err(BatchError::OversizedRecord);
        }
        let text = std::str::from_utf8(line).unwrap_or("");
        if let Some(record) = parse_record(text) {
            let byte_offset = offset
                .checked_add((index + 1) as u64)
                .ok_or(BatchError::OffsetOverflow)?;
            records.push(PendingRecord {
                author: record.author,
                content: record.content,
                next_cursor: FileCursor {
                    generation: Some(generation),
                    byte_offset,
                },
            });
        }
        consumed = index + 1;
        line_start = consumed;
        processed_lines += 1;
        if processed_lines == MAX_BATCH_RECORDS {
            break;
        }
    }
    let trailing_length = bytes.len().saturating_sub(line_start);
    if processed_lines < MAX_BATCH_RECORDS && trailing_length > MAX_RECORD_BYTES {
        return Err(BatchError::OversizedRecord);
    }
    let next_offset = offset
        .checked_add(consumed as u64)
        .ok_or(BatchError::OffsetOverflow)?;
    Ok(RecordBatch {
        next_cursor: FileCursor {
            generation: Some(generation),
            byte_offset: next_offset,
        },
        records,
    })
}

/// The cursor to keep after delivering `delivered_records` of `batch`: the whole batch moves it to
/// the batch end, a partial delivery to just after the last delivered record, none leaves it.
pub fn cursor_after_delivery(
    previous: Option<FileCursor>,
    batch: &RecordBatch,
    delivered_records: usize,
    batch_delivered: bool,
) -> Option<FileCursor> {
    if batch_delivered {
        return Some(batch.next_cursor);
    }
    if delivered_records == 0 {
        return previous;
    }
    batch
        .records
        .get(delivered_records - 1)
        .map(|record| record.next_cursor)
        .or(previous)
}

/// Each line of a notification, keeping a trailing empty line (one Enter per boundary).
pub fn notification_lines(message: &str) -> impl Iterator<Item = &str> {
    message.split('\n')
}

/// The `@name` references of a message, in order, with edge punctuation stripped.
pub fn referenced_names(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .map(|word| {
            word.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-' && character != '_'
            })
        })
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Compares a pane name and an author the way routing does: a leading `@` is ignored.
pub fn same_agent_name(pane_name: &str, author: &str) -> bool {
    pane_name.strip_prefix('@').unwrap_or(pane_name) == author.strip_prefix('@').unwrap_or(author)
}
