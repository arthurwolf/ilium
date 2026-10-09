//! Reading transcripts as JSON lines, and the file snapshot the writers use to
//! prove the agent has not appended anything since the transcript was read.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::de::{
    DeserializeSeed, Deserializer, Error as DeError, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
use serde_json::Value;

use crate::error::CompactionError;

/// Lines longer than this are skipped rather than parsed.
pub(crate) const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;
/// Bounds whole-transcript capture before parsing and transactional rewrite.
pub const MAX_TRANSCRIPT_BYTES: u64 = 128 * 1024 * 1024;
/// Parsing reserves at most half of the compaction job's 384 MiB working
/// declaration for owned JSON values and normalized conversation copies.
pub(crate) const MAX_PARSED_TRANSCRIPT_BYTES: usize = 192 * 1024 * 1024;
/// The pause worker reserves 64 MiB, including its 4 MiB tail buffer.
pub(crate) const MAX_PARSED_PAUSE_TAIL_BYTES: usize = 48 * 1024 * 1024;
/// Conservative per-node allowance for serde values, collection slots and
/// per-record indexing. The line charge also covers owned strings and one
/// normalized copy; this is an admission estimate, not allocator telemetry.
const JSON_NODE_RESERVE_BYTES: usize = 128;

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
    // IgnoredAny validates the fragment without constructing an owned Value.
    if serde_json::from_slice::<IgnoredAny>(&bytes[fragment_start..]).is_ok() {
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
    let file = std::fs::File::open(path).map_err(|error| CompactionError::Unreadable {
        path: path.to_path_buf(),
        error,
    })?;
    let initial_length = file
        .metadata()
        .map_err(|error| CompactionError::Unreadable {
            path: path.to_path_buf(),
            error,
        })?
        .len();
    if initial_length > MAX_TRANSCRIPT_BYTES {
        return Err(CompactionError::TooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_TRANSCRIPT_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(initial_length as usize);
    file.take(MAX_TRANSCRIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CompactionError::Unreadable {
            path: path.to_path_buf(),
            error,
        })?;
    if bytes.len() as u64 > MAX_TRANSCRIPT_BYTES {
        return Err(CompactionError::TooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_TRANSCRIPT_BYTES,
        });
    }
    Ok(bytes)
}

pub(crate) fn load_transcript(path: &Path) -> Result<LoadedTranscript, CompactionError> {
    let bytes = read_transcript_bytes(path)?;
    let (values, skipped_lines) = parse_lines(&bytes, MAX_PARSED_TRANSCRIPT_BYTES, path)?;
    Ok(LoadedTranscript {
        values,
        snapshot: FileSnapshot::of(&bytes),
        skipped_lines,
        shape: tail_shape(&bytes),
    })
}

pub(crate) fn parse_lines(
    bytes: &[u8],
    max_parsed_bytes: usize,
    path: &Path,
) -> Result<(Vec<Value>, usize), CompactionError> {
    let mut values = Vec::new();
    let mut skipped = 0;
    let mut parsed_bytes = 0usize;
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            skipped += 1;
            continue;
        }
        let Ok(node_count) = count_json_nodes(line) else {
            skipped += 1;
            continue;
        };
        let line_cost = line.len().checked_mul(2).and_then(|bytes| {
            node_count
                .checked_mul(JSON_NODE_RESERVE_BYTES)
                .and_then(|nodes| bytes.checked_add(nodes))
        });
        let Some(line_cost) = line_cost else {
            return Err(CompactionError::ParseMemoryLimit {
                path: path.to_path_buf(),
                max_bytes: max_parsed_bytes,
            });
        };
        let next_parsed_bytes = parsed_bytes.checked_add(line_cost).ok_or_else(|| {
            CompactionError::ParseMemoryLimit {
                path: path.to_path_buf(),
                max_bytes: max_parsed_bytes,
            }
        })?;
        if next_parsed_bytes > max_parsed_bytes {
            return Err(CompactionError::ParseMemoryLimit {
                path: path.to_path_buf(),
                max_bytes: max_parsed_bytes,
            });
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(value) => {
                values.push(value);
                parsed_bytes = next_parsed_bytes;
            }
            Err(_) => skipped += 1,
        }
    }
    Ok((values, skipped))
}

/// Counts JSON values and object keys without constructing a `Value` tree.
fn count_json_nodes(line: &[u8]) -> serde_json::Result<usize> {
    let mut nodes = 0usize;
    let mut deserializer = serde_json::Deserializer::from_slice(line);
    JsonNodeCounter { nodes: &mut nodes }.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(nodes)
}

struct JsonNodeCounter<'a> {
    nodes: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for JsonNodeCounter<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        *self.nodes = (*self.nodes).saturating_add(1);
        deserializer.deserialize_any(JsonNodeVisitor { nodes: self.nodes })
    }
}

struct JsonNodeVisitor<'a> {
    nodes: &'a mut usize,
}

impl<'de> Visitor<'de> for JsonNodeVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: DeError>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: DeError>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: DeError>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: DeError>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: DeError>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_string<E: DeError>(self, _: String) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: DeError>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence
            .next_element_seed(JsonNodeCounter { nodes: self.nodes })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        while map
            .next_key_seed(JsonNodeCounter { nodes: self.nodes })?
            .is_some()
        {
            map.next_value_seed(JsonNodeCounter { nodes: self.nodes })?;
        }
        Ok(())
    }
}

/// Parses the last `max_bytes` of a file. When the window starts inside a
/// line, that partial line is dropped.
pub(crate) fn read_tail_values(
    path: &Path,
    max_bytes: u64,
    max_parsed_bytes: usize,
) -> std::io::Result<Vec<Value>> {
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
    parse_lines(window, max_parsed_bytes, path)
        .map(|(values, _)| values)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_allocation_heavy_json(path: &Path, object_count: usize) {
        use std::io::Write;

        let mut writer = std::io::BufWriter::new(
            std::fs::File::create(path).expect("create allocation-heavy transcript"),
        );
        writer
            .write_all(
                b"{\"type\":\"user\",\"uuid\":\"allocation-heavy\",\"message\":{\"content\":[",
            )
            .expect("write Claude user record start");
        for start in (0..object_count).step_by(1024) {
            if start > 0 {
                writer.write_all(b",").expect("write object separator");
            }
            let count = (object_count - start).min(1024);
            let mut chunk = String::with_capacity(count * 27);
            for index in 0..count {
                if index > 0 {
                    chunk.push(',');
                }
                chunk.push_str("{\"type\":\"text\",\"text\":\"x\"}");
            }
            writer
                .write_all(chunk.as_bytes())
                .expect("write object chunk");
        }
        writer
            .write_all(b"]}}")
            .expect("write Claude user record end");
        writer.flush().expect("flush allocation-heavy transcript");
    }

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
        let (values, skipped) = parse_lines(
            b"{\"a\":1}\nnot json\n\n{\"b\":2}\n{\"c\"",
            1024,
            Path::new("test"),
        )
        .expect("small transcript fits its parse budget");
        assert_eq!(values.len(), 2);
        assert_eq!(skipped, 2);
    }

    #[test]
    fn oversized_transcript_is_refused_before_allocating_its_contents() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("oversized.jsonl");
        let file = std::fs::File::create(&path).expect("create sparse transcript");
        file.set_len(MAX_TRANSCRIPT_BYTES + 1)
            .expect("size sparse transcript");

        let error = read_transcript_bytes(&path).expect_err("oversized transcript must be refused");
        assert!(matches!(
            error,
            CompactionError::TooLarge {
                max_bytes: MAX_TRANSCRIPT_BYTES,
                ..
            }
        ));
    }

    #[test]
    fn high_allocation_json_is_refused_by_the_transcript_memory_budget() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("allocation-heavy.jsonl");
        write_allocation_heavy_json(&path, 700_000);
        let transcript_bytes = std::fs::metadata(&path)
            .expect("inspect allocation-heavy transcript")
            .len();
        assert!(transcript_bytes <= MAX_TRANSCRIPT_BYTES);
        assert!(transcript_bytes as usize <= MAX_LINE_BYTES);

        let error = match load_transcript(&path) {
            Ok(_) => panic!("allocation-heavy JSON must be refused before materialization"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("memory"),
            "resource refusal should report the bounded transcript memory budget: {error}"
        );
    }

    #[test]
    fn pause_tail_refuses_allocation_heavy_json_under_its_smaller_budget() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("allocation-heavy-tail.jsonl");
        write_allocation_heavy_json(&path, 100_000);
        let transcript_bytes = std::fs::metadata(&path)
            .expect("inspect allocation-heavy transcript")
            .len();
        assert!(transcript_bytes < 4 * 1024 * 1024);

        let error = read_tail_values(&path, 4 * 1024 * 1024, 48 * 1024 * 1024)
            .expect_err("pause tail must refuse allocation-heavy JSON");
        assert!(
            error.to_string().contains("memory"),
            "pause probe should report its bounded memory refusal: {error}"
        );
    }
}
