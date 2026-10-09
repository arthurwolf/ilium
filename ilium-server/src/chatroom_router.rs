//! Routes explicit `@name` references in newly appended project chat records
//! to the matching live agent pane.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ilium_execution::{Job, JobContext, JobCost, Lane};
use ilium_ipc::PromptSubmissionSource;

use crate::ipc::handlers::{submit_terminal_text, write_scheduled_key_input};
use crate::pane::PaneResource;
use crate::state::ServerState;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_BATCH_BYTES: usize = 64 * 1024;
const MAX_BATCH_RECORDS: usize = 64;
// The chatroom writer accepts 4,000 Unicode scalar values. At four UTF-8
// bytes per scalar, the payload alone may need 16,000 bytes; leave room for
// the timestamp, author, separators, and field escaping without making a
// single record large enough to monopolize the complete read batch.
const MAX_RECORD_BYTES: usize = 20 * 1024;
const JOB_OVERHEAD_BYTES: usize = 8 * 1024;
const JOB_INPUT_BYTES: usize = MAX_BATCH_BYTES + JOB_OVERHEAD_BYTES;
const RESULT_BYTES: usize = MAX_BATCH_BYTES * 3;

pub(crate) fn spawn(state: Arc<ServerState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(state))
}

async fn run(state: Arc<ServerState>) {
    let path = state.session_cwd.join("CHATROOM.md");
    let Some(execution) = state
        .execution
        .get()
        .map(|execution| execution.client.clone())
    else {
        tracing::warn!("chatroom reference router started before execution bank");
        return;
    };
    if path.as_os_str().as_encoded_bytes().len() > JOB_OVERHEAD_BYTES {
        tracing::error!("chatroom path exceeds the admitted input bound");
        return;
    }
    let mut cursor = None;
    loop {
        let reservation = match execution.foundation.try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: JOB_INPUT_BYTES,
                result_bytes: RESULT_BYTES,
            },
        ) {
            Ok(reservation) => reservation,
            Err(ilium_execution::RejectReason::Closed) => return,
            Err(reason) => {
                tracing::debug!(?reason, "chatroom reference scan deferred by I/O admission");
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
        };
        let job = ChatroomReadJob {
            path: path.clone(),
            cursor,
        };
        match execution.run_reserved(reservation, job).await {
            Ok(batch) => {
                let (batch, _retention) = batch.into_parts();
                let mut delivered = true;
                let mut delivered_records = 0;
                for record in &batch.records {
                    if !route_record(&state, &record.author, &record.content).await {
                        delivered = false;
                        break;
                    }
                    delivered_records += 1;
                }
                cursor = cursor_after_delivery(cursor, &batch, delivered_records, delivered);
            }
            Err(error) => tracing::warn!(%error, "chatroom reference scan failed"),
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    author: String,
    content: String,
}

struct RecordBatch {
    next_cursor: FileCursor,
    records: Vec<PendingRecord>,
}

#[derive(Debug)]
struct PendingRecord {
    author: String,
    content: String,
    next_cursor: FileCursor,
}

fn cursor_after_delivery(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileCursor {
    generation: Option<(u64, u64)>,
    byte_offset: u64,
}

impl FileCursor {
    fn new(byte_offset: u64) -> Self {
        Self {
            generation: None,
            byte_offset,
        }
    }
}

struct ChatroomReadJob {
    path: PathBuf,
    cursor: Option<FileCursor>,
}

impl Job for ChatroomReadJob {
    type Output = RecordBatch;
    type Error = io::Error;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"));
        }
        match self.cursor {
            Some(cursor) => {
                read_new_records(&self.path, cursor).map(|(next_cursor, records)| RecordBatch {
                    next_cursor,
                    records,
                })
            }
            None => match open_chatroom_log(&self.path) {
                Ok(file) => {
                    let metadata = file.metadata()?;
                    if !metadata.is_file() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "chatroom log is not a regular file",
                        ));
                    }
                    Ok(RecordBatch {
                        next_cursor: FileCursor {
                            generation: Some(ilium_platform::secure_fs::file_generation(&file)?),
                            byte_offset: metadata.len(),
                        },
                        records: Vec::new(),
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(RecordBatch {
                    next_cursor: FileCursor {
                        generation: None,
                        byte_offset: 0,
                    },
                    records: Vec::new(),
                }),
                Err(error) => Err(error),
            },
        }
    }
}

fn read_new_records(
    path: &Path,
    cursor: FileCursor,
) -> Result<(FileCursor, Vec<PendingRecord>), std::io::Error> {
    let mut file = match open_chatroom_log(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((cursor, Vec::new()));
        }
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chatroom log is not a regular file",
        ));
    }
    let generation = ilium_platform::secure_fs::file_generation(&file)?;
    let offset = if cursor.generation != Some(generation) || metadata.len() < cursor.byte_offset {
        0
    } else {
        cursor.byte_offset
    };
    if metadata.len() == offset {
        return Ok((
            FileCursor {
                generation: Some(generation),
                byte_offset: offset,
            },
            Vec::new(),
        ));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity(MAX_BATCH_BYTES);
    file.take(MAX_BATCH_BYTES as u64).read_to_end(&mut bytes)?;

    let mut records = Vec::with_capacity(MAX_BATCH_RECORDS);
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
            return Err(oversized_record());
        }
        let text = std::str::from_utf8(line).unwrap_or("");
        if let Some(record) = parse_record(text) {
            let byte_offset = offset.checked_add((index + 1) as u64).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "chatroom offset overflow")
            })?;
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
        return Err(oversized_record());
    }
    let next_offset = offset
        .checked_add(consumed as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "chatroom offset overflow"))?;
    Ok((
        FileCursor {
            generation: Some(generation),
            byte_offset: next_offset,
        },
        records,
    ))
}

fn open_chatroom_log(path: &Path) -> io::Result<File> {
    ilium_platform::secure_fs::open_regular_file(path)
}

fn oversized_record() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "chatroom record exceeds byte limit",
    )
}

fn parse_record(line: &str) -> Option<Record> {
    let mut fields = line.strip_prefix("- ")?.splitn(3, " | ");
    let _timestamp = fields.next()?;
    let author = unescape(fields.next()?)?;
    let content = unescape(fields.next()?)?;
    Some(Record { author, content })
}

fn unescape(value: &str) -> Option<String> {
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

async fn route_record(state: &Arc<ServerState>, author: &str, content: &str) -> bool {
    let references = referenced_names(content);
    if references.is_empty() {
        return true;
    }
    let panes = state.panes.read().await;
    let tree = state.tree.read().await;
    let targets = tree
        .panes()
        .filter_map(|node| {
            let name = node.name.strip_prefix('@').unwrap_or(&node.name);
            if !references.iter().any(|reference| reference == name) {
                return None;
            }
            let pane_id = node.id;
            if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(_))) {
                return None;
            }
            Some((pane_id, node.name.clone()))
        })
        .collect::<Vec<_>>();
    drop(tree);
    drop(panes);
    for (pane_id, target_name) in targets {
        let source_session = source_session_id(state, author).await;
        let called_name = target_name.strip_prefix('@').unwrap_or(&target_name);
        let message = format!(
            "new event: the agent {author} with session {} sent you (called @{called_name}) the following message in the chatroom:\n{content}\n",
            source_session.as_deref().unwrap_or("unknown")
        );
        for line in notification_lines(&message) {
            let result = if line.is_empty() {
                write_scheduled_key_input(
                    state,
                    pane_id,
                    b"\r",
                    Some(PromptSubmissionSource::ScheduledInput),
                )
                .await
            } else {
                submit_terminal_text(state, pane_id, line, PromptSubmissionSource::ScheduledInput)
                    .await
            };
            if let Err(error) = result {
                tracing::warn!(?pane_id, %error, "chatroom reference delivery failed");
                return false;
            }
        }
    }
    true
}

fn notification_lines(message: &str) -> impl Iterator<Item = &str> {
    message.split('\n')
}

async fn source_session_id(state: &Arc<ServerState>, author: &str) -> Option<String> {
    let expected = author.strip_prefix('@').unwrap_or(author);
    let tree = state.tree.read().await;
    let panes = state.panes.read().await;
    let source = tree.panes().find_map(|node| {
        let name = node.name.strip_prefix('@').unwrap_or(&node.name);
        if name != expected {
            return None;
        }
        match panes.get(&node.id) {
            Some(PaneResource::Terminal(runtime)) => runtime.session_id.clone(),
            _ => None,
        }
    });
    source
}

fn referenced_names(content: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_chat_record_and_multiple_references() {
        let record = parse_record("- t | sender | hello\\n@compiler @reviewer").unwrap();
        assert_eq!(record.content, "hello\n@compiler @reviewer");
        assert_eq!(
            referenced_names(&record.content),
            vec!["compiler", "reviewer"]
        );
    }

    #[test]
    fn notification_lines_preserve_each_enter_boundary() {
        let lines = notification_lines("header\n/goal\n").collect::<Vec<_>>();
        assert_eq!(lines, vec!["header", "/goal", ""]);
    }

    #[test]
    fn append_reader_returns_only_one_ordered_record_batch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let contents = (0..70)
            .map(|index| format!("- t | sender | record-{index}\n"))
            .collect::<String>();
        std::fs::write(&path, &contents).unwrap();

        let (next_cursor, records) = read_new_records(&path, FileCursor::new(0)).unwrap();

        assert_eq!(
            records.len(),
            64,
            "one poll must not materialize the whole backlog"
        );
        assert_eq!(records[0].content, "record-0");
        assert_eq!(records[63].content, "record-63");
        assert_eq!(
            next_cursor.byte_offset,
            contents
                .lines()
                .take(64)
                .map(|line| line.len() + 1)
                .sum::<usize>() as u64
        );
        let (final_cursor, remaining) = read_new_records(&path, next_cursor).unwrap();
        assert_eq!(remaining.len(), 6);
        assert_eq!(remaining[0].content, "record-64");
        assert_eq!(final_cursor.byte_offset, contents.len() as u64);
    }

    #[test]
    fn append_reader_can_resume_after_the_last_successfully_delivered_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let contents = "- t | sender | first\n- t | sender | second\nhand-edited note\n";
        std::fs::write(&path, contents).unwrap();

        let (batch_cursor, records) = read_new_records(&path, FileCursor::new(0)).unwrap();
        assert_eq!(records.len(), 2);
        let batch = RecordBatch {
            next_cursor: batch_cursor,
            records,
        };
        let original_cursor = Some(FileCursor::new(0));
        assert_eq!(
            cursor_after_delivery(original_cursor, &batch, 0, false),
            original_cursor,
            "a failed first delivery must leave the batch available for retry"
        );
        let acknowledged_cursor = cursor_after_delivery(original_cursor, &batch, 1, false).unwrap();
        assert!(acknowledged_cursor.byte_offset < batch_cursor.byte_offset);
        assert_eq!(
            cursor_after_delivery(original_cursor, &batch, 2, true),
            Some(batch_cursor),
            "a fully delivered batch commits ignored trailing rows too"
        );

        let (retry_cursor, retry_records) = read_new_records(&path, acknowledged_cursor).unwrap();
        assert_eq!(retry_records.len(), 1);
        assert_eq!(retry_records[0].content, "second");
        assert_eq!(retry_cursor.byte_offset, contents.len() as u64);
    }

    #[test]
    fn append_reader_restarts_when_equal_length_log_is_atomically_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let previous = directory.path().join("CHATROOM.previous");
        let replacement = directory.path().join("CHATROOM.replacement");
        let old_record = "- t | sender | old record\n";
        let new_record = "- t | sender | new record\n";
        assert_eq!(old_record.len(), new_record.len());
        std::fs::write(&path, old_record).unwrap();

        let (cursor, records) = read_new_records(&path, FileCursor::new(0)).unwrap();
        assert_eq!(records[0].content, "old record");
        assert_eq!(cursor.byte_offset, old_record.len() as u64);

        std::fs::rename(&path, &previous).unwrap();
        std::fs::write(&replacement, new_record).unwrap();
        std::fs::rename(&replacement, &path).unwrap();

        let (next_cursor, records) = read_new_records(&path, cursor).unwrap();
        assert_eq!(records.len(), 1, "replacement starts a new file generation");
        assert_eq!(records[0].content, "new record");
        assert_eq!(next_cursor.byte_offset, new_record.len() as u64);

        let larger = directory.path().join("CHATROOM.larger");
        let larger_record = "- t | sender | a longer replacement record\n";
        std::fs::rename(&path, &replacement).unwrap();
        std::fs::write(&larger, larger_record).unwrap();
        std::fs::rename(&larger, &path).unwrap();

        let (final_cursor, records) = read_new_records(&path, next_cursor).unwrap();
        assert_eq!(records.len(), 1, "larger replacement starts at byte zero");
        assert_eq!(records[0].content, "a longer replacement record");
        assert_eq!(final_cursor.byte_offset, larger_record.len() as u64);
    }

    #[test]
    fn append_reader_bounds_bytes_even_when_record_count_is_below_its_cap() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let line = format!("- t | sender | {}\n", "x".repeat(MAX_RECORD_BYTES - 20));
        let contents = line.repeat(20);
        std::fs::write(&path, &contents).unwrap();

        let (next_cursor, records) = read_new_records(&path, FileCursor::new(0)).unwrap();

        assert!(records.len() < MAX_BATCH_RECORDS);
        assert!(next_cursor.byte_offset <= MAX_BATCH_BYTES as u64);
        assert_eq!(next_cursor.byte_offset as usize, records.len() * line.len());
        assert!(next_cursor.byte_offset < contents.len() as u64);
    }

    #[test]
    fn append_reader_accepts_the_writer_limit_of_four_byte_unicode_messages() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let content = "🧭".repeat(4_000);
        let line = format!("- t | sender | {content}\n");
        assert!(line.len() <= MAX_RECORD_BYTES);
        std::fs::write(&path, &line).unwrap();

        let (cursor, records) = read_new_records(&path, FileCursor::new(0)).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].content, content);
        assert_eq!(cursor.byte_offset as usize, line.len());
    }

    #[test]
    fn append_reader_keeps_an_unterminated_record_for_the_next_batch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let complete = b"- t | sender | first\n";
        std::fs::write(
            &path,
            [complete.as_slice(), b"- t | sender | second".as_slice()].concat(),
        )
        .unwrap();

        let (cursor, records) = read_new_records(&path, FileCursor::new(0)).unwrap();
        assert_eq!(cursor.byte_offset, complete.len() as u64);
        assert_eq!(records.len(), 1);

        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        let (next_cursor, appended) = read_new_records(&path, cursor).unwrap();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].content, "second");
        assert_eq!(
            next_cursor.byte_offset,
            std::fs::metadata(&path).unwrap().len()
        );
    }

    #[test]
    fn append_reader_refuses_a_record_beyond_its_byte_limit_without_advancing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        let record = format!("- t | sender | {}\n", "x".repeat(MAX_RECORD_BYTES));
        std::fs::write(&path, &record).unwrap();

        let error = read_new_records(&path, FileCursor::new(0))
            .expect_err("oversized record must be explicit");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), record.len() as u64);
    }

    #[test]
    fn append_reader_rejects_a_non_regular_path_without_reading_it() {
        let directory = tempfile::tempdir().unwrap();

        let error = read_new_records(directory.path(), FileCursor::new(0))
            .expect_err("directory is not a log");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn chatroom_read_job_uses_the_finite_io_bank_and_returns_a_retained_batch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("CHATROOM.md");
        std::fs::write(&path, "- t | sender | bounded\n").unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();

        let result = execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: JOB_INPUT_BYTES,
                    result_bytes: RESULT_BYTES,
                },
                ChatroomReadJob {
                    path,
                    cursor: Some(FileCursor::new(0)),
                },
            )
            .await
            .unwrap();

        assert_eq!(result.view().records[0].content, "bounded");
        assert_eq!(result.view().next_cursor.byte_offset, 23);
    }
}
