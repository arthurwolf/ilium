//! Streams transcript lines into `<directory>/<session_id>.jsonl` through a
//! temporary file in the same directory and a final rename, so a
//! half-written transcript never appears under its final name. Dropping the
//! writer before [`TranscriptFileWriter::finish`] succeeded removes the
//! temporary file (and the directory, if this writer created it), which
//! covers every error and cancellation path of the caller.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::LineSink;
use crate::error::ConvertError;

#[derive(Debug)]
pub(crate) struct TranscriptFileWriter {
    directory: PathBuf,
    /// The directory did not exist before this writer created it, so an
    /// abandoned write removes it again (non-recursively: only when empty).
    remove_directory_on_failure: bool,
    temporary_path: PathBuf,
    final_path: PathBuf,
    writer: std::io::BufWriter<std::fs::File>,
    bytes_written: u64,
    /// Ceiling for the finished file, enforced per line as it is written.
    limit: u64,
    finished: bool,
}

impl TranscriptFileWriter {
    /// Creates the directory (owner-only) and the temporary file. `limit`
    /// is the largest file this writer agrees to produce.
    pub(crate) fn create(
        directory: &Path,
        session_id: &str,
        limit: u64,
    ) -> Result<Self, ConvertError> {
        let write_error = |path: &Path, error: std::io::Error| ConvertError::TargetWrite {
            path: path.to_path_buf(),
            error,
        };
        let final_path = directory.join(format!("{session_id}.jsonl"));
        // Never matches `*.jsonl`, so Claude Code's own scan cannot pick it up.
        let temporary_path = directory.join(format!(".{session_id}.jsonl.tmp"));

        let directory_existed = directory.is_dir();
        ilium_platform::secure_fs::create_private_directory(directory)
            .map_err(|error| write_error(directory, error))?;
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .and_then(|file| {
                ilium_platform::secure_fs::restrict_open_file_to_owner(&file)?;
                Ok(file)
            });
        let file = match opened {
            Ok(file) => file,
            Err(error) => {
                if !directory_existed {
                    let _ = std::fs::remove_dir(directory);
                }
                return Err(write_error(&temporary_path, error));
            }
        };
        Ok(Self {
            directory: directory.to_path_buf(),
            remove_directory_on_failure: !directory_existed,
            temporary_path,
            final_path,
            writer: std::io::BufWriter::new(file),
            bytes_written: 0,
            limit,
            finished: false,
        })
    }

    /// Bytes of the file so far, newlines included.
    pub(crate) fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Flushes, consults `should_cancel` right before the rename, syncs and
    /// renames the temporary file into place. Returns the final path.
    pub(crate) fn finish(
        mut self,
        should_cancel: &dyn Fn() -> bool,
    ) -> Result<PathBuf, ConvertError> {
        let temporary_error = |error: std::io::Error| ConvertError::TargetWrite {
            path: self.temporary_path.clone(),
            error,
        };
        self.writer.flush().map_err(temporary_error)?;
        if should_cancel() {
            return Err(ConvertError::Cancelled);
        }
        self.writer.get_ref().sync_all().map_err(temporary_error)?;
        std::fs::rename(&self.temporary_path, &self.final_path).map_err(|error| {
            ConvertError::TargetWrite {
                path: self.final_path.clone(),
                error,
            }
        })?;
        self.finished = true;
        Ok(self.final_path.clone())
    }
}

impl LineSink for TranscriptFileWriter {
    fn write_line(&mut self, line: &str) -> Result<(), ConvertError> {
        let line_bytes = line.len() as u64 + 1;
        let after = self.bytes_written.saturating_add(line_bytes);
        if after > self.limit {
            return Err(ConvertError::FileTooLarge {
                path: self.final_path.clone(),
                bytes: after,
                maximum: self.limit,
            });
        }
        let write = self
            .writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"));
        write.map_err(|error| ConvertError::TargetWrite {
            path: self.temporary_path.clone(),
            error,
        })?;
        self.bytes_written = after;
        Ok(())
    }
}

impl Drop for TranscriptFileWriter {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Best effort: the temp file is ours, and the directory is only
        // removed when it is still empty.
        let _ = std::fs::remove_file(&self.temporary_path);
        if self.remove_directory_on_failure {
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_ID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn lines_land_in_the_final_file_only_after_finish() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("projects").join("slug");
        let mut writer = TranscriptFileWriter::create(&directory, SESSION_ID, 1_000).unwrap();
        writer.write_line("{\"a\":1}").unwrap();
        writer.write_line("{\"b\":2}").unwrap();
        assert_eq!(writer.bytes_written(), 16);
        assert_eq!(
            entries(&directory),
            vec![format!(".{SESSION_ID}.jsonl.tmp")]
        );
        let path = writer.finish(&|| false).unwrap();
        assert_eq!(path, directory.join(format!("{SESSION_ID}.jsonl")));
        assert_eq!(entries(&directory), vec![format!("{SESSION_ID}.jsonl")]);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"a\":1}\n{\"b\":2}\n"
        );
    }

    #[test]
    fn cancelling_at_finish_removes_the_temp_file_and_a_directory_it_created() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("fresh");
        let mut writer = TranscriptFileWriter::create(&directory, SESSION_ID, 1_000).unwrap();
        writer.write_line("{}").unwrap();
        let error = writer.finish(&|| true).unwrap_err();
        assert!(matches!(error, ConvertError::Cancelled));
        assert!(
            !directory.exists(),
            "the directory this writer created is gone"
        );
    }

    #[test]
    fn a_pre_existing_directory_survives_an_abandoned_write() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().to_path_buf();
        std::fs::write(directory.join("other.jsonl"), "{}\n").unwrap();
        {
            let mut writer = TranscriptFileWriter::create(&directory, SESSION_ID, 1_000).unwrap();
            writer.write_line("{}").unwrap();
            // Dropped without finish: an error path in the caller.
        }
        assert_eq!(entries(&directory), vec!["other.jsonl".to_string()]);
    }

    #[test]
    fn the_output_limit_is_enforced_while_writing_and_cleans_up() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("capped");
        let mut writer = TranscriptFileWriter::create(&directory, SESSION_ID, 10).unwrap();
        writer.write_line("123456789").unwrap();
        let error = writer.write_line("x").unwrap_err();
        match error {
            ConvertError::FileTooLarge { bytes, maximum, .. } => {
                assert_eq!((bytes, maximum), (12, 10));
            }
            other => panic!("unexpected error {other}"),
        }
        drop(writer);
        assert!(!directory.exists());
    }

    #[test]
    fn a_leftover_temp_file_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().to_path_buf();
        std::fs::write(directory.join(format!(".{SESSION_ID}.jsonl.tmp")), "old").unwrap();
        let error = TranscriptFileWriter::create(&directory, SESSION_ID, 1_000).unwrap_err();
        assert!(matches!(error, ConvertError::TargetWrite { .. }), "{error}");
        assert_eq!(
            std::fs::read_to_string(directory.join(format!(".{SESSION_ID}.jsonl.tmp"))).unwrap(),
            "old"
        );
    }
}
