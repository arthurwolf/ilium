//! Reading transcripts as JSON lines, and the file snapshot the writers use to
//! prove the agent has not appended anything since the transcript was read.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::error::CompactionError;

/// Lines longer than this are skipped rather than parsed.
pub(crate) const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Identity of a transcript file at the moment it was read: its byte length
/// and a hash of its final line. In-process only, so the hasher need not be
/// stable across runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileSnapshot {
    pub(crate) len: u64,
    pub(crate) last_line_hash: u64,
}

impl FileSnapshot {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        hasher.write(last_line(bytes));
        Self {
            len: bytes.len() as u64,
            last_line_hash: hasher.finish(),
        }
    }
}

/// The final line of `bytes`: the fragment after the last newline when there
/// is one (a torn write), otherwise the last complete line.
fn last_line(bytes: &[u8]) -> &[u8] {
    let trimmed = match bytes.last() {
        Some(b'\n') => &bytes[..bytes.len() - 1],
        _ => bytes,
    };
    match trimmed.iter().rposition(|byte| *byte == b'\n') {
        Some(position) => &trimmed[position + 1..],
        None => trimmed,
    }
}

/// How much of a file a writer keeps before appending its records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TailShape {
    /// Bytes of the original kept verbatim.
    pub(crate) keep_len: usize,
    /// True when the kept bytes do not end in a newline (a complete last
    /// record that was written without one).
    pub(crate) needs_newline: bool,
    /// True when a torn, unparseable final fragment is dropped from the copy.
    pub(crate) dropped_torn_fragment: bool,
}

pub(crate) fn tail_shape(bytes: &[u8]) -> TailShape {
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return TailShape {
            keep_len: bytes.len(),
            needs_newline: false,
            dropped_torn_fragment: false,
        };
    }
    let fragment_start = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    if serde_json::from_slice::<Value>(&bytes[fragment_start..]).is_ok() {
        return TailShape {
            keep_len: bytes.len(),
            needs_newline: true,
            dropped_torn_fragment: false,
        };
    }
    TailShape {
        keep_len: fragment_start,
        needs_newline: false,
        dropped_torn_fragment: true,
    }
}

/// A whole transcript parsed line by line. Unparseable and oversized lines are
/// skipped and counted; a torn last line never fails the read.
pub(crate) struct LoadedTranscript {
    pub(crate) values: Vec<Value>,
    pub(crate) snapshot: FileSnapshot,
    pub(crate) skipped_lines: usize,
    pub(crate) shape: TailShape,
}

pub(crate) fn read_transcript_bytes(path: &Path) -> Result<Vec<u8>, CompactionError> {
    std::fs::read(path).map_err(|error| CompactionError::Unreadable {
        path: path.to_path_buf(),
        error,
    })
}

pub(crate) fn load_transcript(path: &Path) -> Result<LoadedTranscript, CompactionError> {
    let bytes = read_transcript_bytes(path)?;
    let (values, skipped_lines) = parse_lines(&bytes);
    Ok(LoadedTranscript {
        values,
        snapshot: FileSnapshot::of(&bytes),
        skipped_lines,
        shape: tail_shape(&bytes),
    })
}

pub(crate) fn parse_lines(bytes: &[u8]) -> (Vec<Value>, usize) {
    let mut values = Vec::new();
    let mut skipped = 0;
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            skipped += 1;
            continue;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(value) => values.push(value),
            Err(_) => skipped += 1,
        }
    }
    (values, skipped)
}

/// Parses the last `max_bytes` of a file. When the window starts inside a
/// line, that partial line is dropped.
pub(crate) fn read_tail_values(path: &Path, max_bytes: u64) -> std::io::Result<Vec<Value>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes)?;
    let window = if start > 0 {
        match bytes.iter().position(|byte| *byte == b'\n') {
            Some(position) => &bytes[position + 1..],
            None => &[][..],
        }
    } else {
        &bytes[..]
    };
    Ok(parse_lines(window).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torn_fragment_is_dropped_and_complete_unterminated_line_is_kept() {
        let torn = b"{\"a\":1}\n{\"b\":";
        let shape = tail_shape(torn);
        assert_eq!(shape.keep_len, 8);
        assert!(shape.dropped_torn_fragment);

        let unterminated = b"{\"a\":1}\n{\"b\":2}";
        let shape = tail_shape(unterminated);
        assert_eq!(shape.keep_len, unterminated.len());
        assert!(shape.needs_newline);

        let clean = b"{\"a\":1}\n";
        assert_eq!(tail_shape(clean).keep_len, clean.len());
    }

    #[test]
    fn snapshot_changes_when_the_last_line_or_length_changes() {
        let base = FileSnapshot::of(b"{\"a\":1}\n");
        assert_eq!(base, FileSnapshot::of(b"{\"a\":1}\n"));
        assert_ne!(base, FileSnapshot::of(b"{\"a\":2}\n"));
        assert_ne!(base, FileSnapshot::of(b"{\"a\":1}\n{\"b\":1}\n"));
    }

    #[test]
    fn bad_lines_are_skipped_and_counted() {
        let (values, skipped) = parse_lines(b"{\"a\":1}\nnot json\n\n{\"b\":2}\n{\"c\"");
        assert_eq!(values.len(), 2);
        assert_eq!(skipped, 2);
    }
}
