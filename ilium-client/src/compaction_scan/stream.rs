//! Phase 2 of the scan: stream one transcript file into a `SessionTrace`.
//!
//! Files range from 1 KB to several GB and live sessions append to them while
//! they are read, so the reader is deliberately simple and bounded:
//!
//! * lines are cut at `MAX_LINE_BYTES` (the rest of an oversize line is
//!   drained, never buffered; the parser counts it as skipped),
//! * only the first `size` bytes seen at listing time are read (an appending
//!   session cannot make a scan chase its tail),
//! * a truncated last line is fed like any other (the parser counts an
//!   undecodable relevant line and moves on),
//! * progress is reported per read chunk, not per file, and the stop request
//!   is honoured between chunks.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use ilium_compaction_analysis::parse::{line_may_matter, TraceBuilder};
use ilium_compaction_analysis::{AgentKind, SessionTrace};

/// Longest line the parser sees. Larger lines are drained and counted by the
/// parser as oversize (the research capped parsing at about 30 MB).
pub(super) const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;
/// Read-ahead buffer; also the granularity of progress and stop checks.
const READ_CHUNK_BYTES: usize = 256 * 1024;
/// A line buffer that grew beyond this is replaced after use.
const BUFFER_SHRINK_BYTES: usize = 1024 * 1024;

/// How one file ended.
#[derive(Debug)]
pub(super) enum FileOutcome {
    /// The finished trace.
    Parsed { trace: Box<SessionTrace> },
    /// The stop request arrived mid-file.
    Stopped,
    /// The file could not be opened or read.
    Unreadable(io::Error),
}

/// How a bounded line read ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LineRead {
    /// End of input with nothing read.
    Eof,
    /// A line is in the buffer; `oversize` when it was cut at the limit.
    Line { oversize: bool },
    /// `on_chunk` asked to stop.
    Stopped,
}

/// Reads one `\n`-terminated line (or the final unterminated one) into
/// `buffer`, keeping at most `max_line_bytes + 1` bytes of it. `on_chunk` gets
/// the number of bytes consumed from the input after each chunk and returns
/// `false` to stop.
pub(super) fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
    max_line_bytes: usize,
    on_chunk: &mut dyn FnMut(usize) -> bool,
) -> io::Result<LineRead> {
    buffer.clear();
    let mut consumed_any = false;
    let mut oversize = false;
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            return Ok(if consumed_any {
                LineRead::Line { oversize }
            } else {
                LineRead::Eof
            });
        }
        let newline = chunk.iter().position(|&byte| byte == b'\n');
        let take = newline.map_or(chunk.len(), |index| index + 1);
        let room = (max_line_bytes + 1).saturating_sub(buffer.len());
        let kept = take.min(room);
        buffer.extend_from_slice(&chunk[..kept]);
        oversize |= kept < take;
        reader.consume(take);
        consumed_any = true;
        if !on_chunk(take) {
            return Ok(LineRead::Stopped);
        }
        if newline.is_some() {
            return Ok(LineRead::Line { oversize });
        }
    }
}

/// Streams the first `size` bytes of `path` into a trace.
///
/// `on_bytes` receives each chunk's byte count (progress) and returns `false`
/// to stop.
pub(super) fn parse_file(
    agent: AgentKind,
    path: &Path,
    size: u64,
    is_subagent: bool,
    on_bytes: &mut dyn FnMut(usize) -> bool,
) -> FileOutcome {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return FileOutcome::Unreadable(error),
    };
    let mut reader = BufReader::with_capacity(READ_CHUNK_BYTES, file.take(size));
    let mut builder = TraceBuilder::new(agent)
        .with_subagent(is_subagent)
        .with_max_line_bytes(MAX_LINE_BYTES);
    let mut buffer: Vec<u8> = Vec::with_capacity(64 * 1024);
    loop {
        match read_bounded_line(&mut reader, &mut buffer, MAX_LINE_BYTES, &mut *on_bytes) {
            Ok(LineRead::Eof) => break,
            Ok(LineRead::Stopped) => return FileOutcome::Stopped,
            Ok(LineRead::Line { oversize }) => {
                // An oversize line is fed (cut at limit + 1 byte) so the parser
                // counts it; the prefilter would hide it.
                if oversize || line_may_matter(agent, &buffer) {
                    builder.feed_line(&buffer);
                }
            }
            Err(error) => return FileOutcome::Unreadable(error),
        }
        if buffer.capacity() > BUFFER_SHRINK_BYTES {
            buffer = Vec::with_capacity(64 * 1024);
        }
    }
    FileOutcome::Parsed {
        trace: Box::new(builder.finish()),
    }
}
