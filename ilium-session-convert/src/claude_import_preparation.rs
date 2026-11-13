//! Produces a private, bounded Claude transcript for Codex's importer.
//!
//! The stable path preserves Codex's path-and-content import deduplication.
//! Exclusive creation prevents concurrent conversions from replacing each
//! other's input; the owner removes only the file it created.

use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::bounded_lines::{BoundedLine, BoundedLineReader, LineReadError};
use crate::error::ConvertError;
use crate::report::Reporter;

const MAX_PREPARATION_LINE_BYTES: usize = 64 * 1024 * 1024;
// Codex's importer has its own short tool-result preview. Keep the notice
// and the retained text within that preview, including UTF-8 multibyte text.
const MAX_CODEX_TOOL_RESULT_BYTES: usize = 3000;

pub(crate) struct PreparedImport {
    path: PathBuf,
    generation: (u64, u64),
}

impl PreparedImport {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PreparedImport {
    fn drop(&mut self) {
        if ilium_platform::secure_fs::open_regular_file(&self.path)
            .and_then(|file| ilium_platform::secure_fs::file_generation(&file))
            .is_ok_and(|generation| generation == self.generation)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn prepare(
    source: &Path,
    home: &Path,
    session_id: &str,
    reporter: &mut Reporter<'_>,
) -> Result<PreparedImport, ConvertError> {
    let source_error = |error| ConvertError::SourceUnreadable {
        path: source.to_path_buf(),
        error,
    };
    let input = ilium_platform::secure_fs::open_regular_file(source).map_err(source_error)?;
    let initial = input.metadata().map_err(source_error)?;
    let initial_generation =
        ilium_platform::secure_fs::file_generation(&input).map_err(source_error)?;
    // Codex canonicalizes the source and accepts it only under Claude's
    // projects root. A dedicated private directory keeps the original intact.
    let directory = home.join(".claude/projects/.ilium-conversion");
    ilium_platform::secure_fs::create_private_directory(&directory).map_err(|error| {
        ConvertError::TargetWrite {
            path: directory.clone(),
            error,
        }
    })?;
    let path = directory.join(format!("{session_id}.jsonl"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| ConvertError::TargetWrite {
            path: path.clone(),
            error,
        })?;
    let generation = ilium_platform::secure_fs::file_generation(&file).map_err(|error| {
        ConvertError::TargetWrite {
            path: path.clone(),
            error,
        }
    })?;
    let prepared = PreparedImport { path, generation };
    ilium_platform::secure_fs::restrict_open_file_to_owner(&file).map_err(|error| {
        ConvertError::TargetWrite {
            path: prepared.path.clone(),
            error,
        }
    })?;
    let mut output = BufWriter::new(file);
    let mut lines = BoundedLineReader::new(BufReader::new(input), MAX_PREPARATION_LINE_BYTES);
    let mut omitted = 0usize;
    let mut truncated = 0usize;
    let mut written = 0u64;
    loop {
        reporter.check_cancel()?;
        let bytes = match lines
            .next_line_interruptible(&mut |_| !reporter.is_cancelled())
            .map_err(|error| match error {
                LineReadError::Io(error) => source_error(error),
                LineReadError::Cancelled => ConvertError::Cancelled,
            })? {
            None => break,
            Some(BoundedLine::Oversized { length, .. }) => {
                return Err(ConvertError::TargetVerification(format!(
                    "a source line is {length} bytes; preprocessing refuses lines over {MAX_PREPARATION_LINE_BYTES} bytes to preserve conversation content"
                )));
            }
            Some(BoundedLine::Line { bytes, .. }) => bytes,
        };
        let mut value = match serde_json::from_slice::<Value>(bytes) {
            Ok(value) => value,
            // Preserve unknown or malformed records for the importer to decide.
            Err(_) => {
                write_line(&mut output, &prepared.path, bytes, &mut written)?;
                continue;
            }
        };
        if matches!(
            value.get("type").and_then(Value::as_str),
            Some("progress" | "file-history-snapshot" | "queue-operation")
        ) {
            omitted += 1;
            continue;
        }
        truncate_tool_results(&mut value, &mut truncated);
        let serialized = serde_json::to_vec(&value).map_err(|error| {
            ConvertError::TargetVerification(format!(
                "cannot serialize prepared transcript: {error}"
            ))
        })?;
        write_line(&mut output, &prepared.path, &serialized, &mut written)?;
    }
    output.flush().map_err(|error| ConvertError::TargetWrite {
        path: prepared.path.clone(),
        error,
    })?;
    let current_file =
        ilium_platform::secure_fs::open_regular_file(source).map_err(source_error)?;
    let current = current_file.metadata().map_err(source_error)?;
    if initial_generation
        != ilium_platform::secure_fs::file_generation(&current_file).map_err(source_error)?
        || initial.len() != current.len()
        || initial.modified().ok() != current.modified().ok()
    {
        return Err(ConvertError::SourceChanged {
            path: source.to_path_buf(),
        });
    }
    reporter.log(format!("Prepared importer input: omitted {omitted} bookkeeping records and truncated {truncated} tool results; the original transcript is unchanged"));
    Ok(prepared)
}

fn write_line(
    output: &mut BufWriter<std::fs::File>,
    path: &Path,
    bytes: &[u8],
    written: &mut u64,
) -> Result<(), ConvertError> {
    *written = written.saturating_add(bytes.len() as u64 + 1);
    if *written > crate::MAX_OUTPUT_TRANSCRIPT_BYTES {
        return Err(ConvertError::FileTooLarge {
            path: path.to_path_buf(),
            bytes: *written,
            maximum: crate::MAX_OUTPUT_TRANSCRIPT_BYTES,
        });
    }
    output
        .write_all(bytes)
        .and_then(|()| output.write_all(b"\n"))
        .map_err(|error| ConvertError::TargetWrite {
            path: path.to_path_buf(),
            error,
        })
}

fn truncate_tool_results(value: &mut Value, truncated: &mut usize) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("tool_result") {
                if let Some(content) = object.get_mut("content") {
                    let mut remaining = MAX_CODEX_TOOL_RESULT_BYTES;
                    let mut shortened = false;
                    truncate_tool_content(content, &mut remaining, &mut shortened);
                    *truncated += usize::from(shortened);
                }
            } else {
                for child in object.values_mut() {
                    truncate_tool_results(child, truncated);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                truncate_tool_results(item, truncated);
            }
        }
        _ => {}
    }
}

fn truncate_tool_content(value: &mut Value, remaining: &mut usize, shortened: &mut bool) {
    match value {
        Value::String(text) => {
            if text.len() > *remaining {
                let mut boundary = *remaining;
                while !text.is_char_boundary(boundary) {
                    boundary -= 1;
                }
                text.truncate(boundary);
                if !*shortened {
                    text.insert_str(0, "[tool result truncated by Ilium during conversion]\n");
                }
                *shortened = true;
                *remaining = 0;
            } else {
                *remaining -= text.len();
            }
        }
        Value::Array(items) => {
            for item in items {
                truncate_tool_content(item, remaining, shortened);
            }
        }
        Value::Object(object) => {
            if let Some(text) = object.get_mut("text") {
                truncate_tool_content(text, remaining, shortened);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_during_a_large_line_cleans_up_the_prepared_input() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.jsonl");
        std::fs::File::create(&source)
            .unwrap()
            .set_len(MAX_PREPARATION_LINE_BYTES as u64 + 1)
            .unwrap();
        let checks = std::cell::Cell::new(0);
        let cancelled = || {
            checks.set(checks.get() + 1);
            checks.get() >= 3
        };
        let mut sink = |_| {};
        let mut reporter = Reporter::new(&mut sink, &cancelled, 6);
        let result = prepare(&source, directory.path(), "fixture", &mut reporter);
        assert!(matches!(result, Err(ConvertError::Cancelled)));
        assert!(!directory
            .path()
            .join(".claude/projects/.ilium-conversion/fixture.jsonl")
            .exists());
        assert_eq!(
            std::fs::metadata(source).unwrap().len(),
            MAX_PREPARATION_LINE_BYTES as u64 + 1
        );
    }

    #[test]
    fn oversized_input_line_is_refused_without_touching_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.jsonl");
        let input = std::fs::File::create(&source).unwrap();
        input
            .set_len(MAX_PREPARATION_LINE_BYTES as u64 + 1)
            .unwrap();
        let mut sink = |_| {};
        let cancelled = || false;
        let mut reporter = Reporter::new(&mut sink, &cancelled, 6);
        let result = prepare(&source, directory.path(), "fixture", &mut reporter);
        assert!(matches!(result, Err(ConvertError::TargetVerification(_))));
        assert_eq!(
            std::fs::metadata(source).unwrap().len(),
            MAX_PREPARATION_LINE_BYTES as u64 + 1
        );
        assert!(!directory
            .path()
            .join(".claude/projects/.ilium-conversion/fixture.jsonl")
            .exists());
    }

    #[test]
    fn cleanup_preserves_a_replacement_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input.jsonl");
        std::fs::write(&path, "owned").unwrap();
        let original = std::fs::File::open(&path).unwrap();
        let generation = ilium_platform::secure_fs::file_generation(&original).unwrap();
        let prepared = PreparedImport {
            path: path.clone(),
            generation,
        };
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "replacement").unwrap();
        drop(prepared);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
    }

    #[test]
    fn tool_result_limit_applies_across_all_text_blocks() {
        let mut record = serde_json::json!({"type":"tool_result", "content":[
            {"type":"text", "text":"a".repeat(20_000)},
            {"type":"text", "text":"b".repeat(20_000)}
        ]});
        let mut truncated = 0;
        truncate_tool_results(&mut record, &mut truncated);
        let content = record["content"].to_string();
        assert!(
            content.len() < 35_000,
            "each result has one shared text budget"
        );
        assert!(content.contains("truncated"));
        assert_eq!(truncated, 1);
    }

    #[test]
    fn unicode_tool_result_starts_with_notice_and_has_a_bounded_utf8_excerpt() {
        let mut record = serde_json::json!({"type":"tool_result", "content":"🦀".repeat(2000)});
        let mut truncated = 0;
        truncate_tool_results(&mut record, &mut truncated);
        let content = record["content"].as_str().unwrap();
        assert!(content.starts_with("[tool result truncated by Ilium during conversion]"));
        assert!(content.len() <= MAX_CODEX_TOOL_RESULT_BYTES + 64);
        assert_eq!(truncated, 1);
    }
}
