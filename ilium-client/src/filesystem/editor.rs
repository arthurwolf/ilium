//! Finite editor file preparation and atomic write jobs. Editor state and
//! acknowledgement reconciliation remain on the client coordinator.
use ilium_execution::{Job, JobContext, JobCost};
use ilium_platform::file_lock::ExclusiveFileLock;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MAX_EDITOR_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_EDITOR_LINES: usize = 262_144;
pub(crate) const MAX_EDITOR_RETAINED_BYTES: usize = 64 * 1024 * 1024;

pub struct EditorRead {
    pub path: PathBuf,
    pub source_hold: Arc<ilium_execution::StorageAdmission>,
}
pub struct EditorSource {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub(crate) retention: Option<Arc<ilium_execution::StorageAdmission>>,
}
impl EditorRead {
    pub fn cost(&self) -> JobCost {
        JobCost {
            input_bytes: 128 * 1024 * 1024 + self.path.capacity(),
            result_bytes: MAX_EDITOR_RETAINED_BYTES,
        }
    }
}
impl Job for EditorRead {
    type Output = EditorSource;
    type Error = String;
    fn run(self, context: JobContext) -> Result<EditorSource, String> {
        if self.path.capacity() > 64 * 1024 {
            return Err("Editor path exceeds retained limit".into());
        }
        if context.stop_requested() {
            return Err("Editor load cancelled before reading".into());
        }
        let mut lines = Vec::new();
        match fs::metadata(&self.path) {
            Ok(metadata) if metadata.is_file() => {
                let file = ilium_platform::secure_fs::open_regular_file(&self.path)
                    .map_err(|error| error.to_string())?;
                let mut bytes = Vec::new();
                file.take((MAX_EDITOR_SOURCE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if bytes.len() > MAX_EDITOR_SOURCE_BYTES {
                    return Err("Editor file exceeds 32 MiB source limit".into());
                }
                let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
                let mut retained = 0_usize;
                for line in text.lines() {
                    if context.stop_requested() {
                        return Err("Editor load cancelled while preparing lines".into());
                    }
                    retained = retained
                        .checked_add(line.len())
                        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<String>()))
                        .ok_or_else(|| "Editor retained byte overflow".to_string())?;
                    if lines.len() >= MAX_EDITOR_LINES || retained > MAX_EDITOR_RETAINED_BYTES {
                        return Err("Editor file exceeds retained line limit".into());
                    }
                    lines.push(line.to_owned());
                }
                lines.shrink_to_fit();
            }
            Ok(_) => return Err(format!("{} is not a regular file", self.path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        Ok(EditorSource {
            path: self.path,
            lines,
            retention: Some(self.source_hold),
        })
    }
}

pub struct EditorWrite {
    pub path: PathBuf,
    pub source_revision: u64,
    pub lines: Arc<[String]>,
}
pub struct EditorWritten {
    pub path: PathBuf,
    pub source_revision: u64,
}
impl EditorWrite {
    pub fn cost(&self) -> Result<JobCost, String> {
        if self.lines.len() > MAX_EDITOR_LINES {
            return Err("Editor save exceeds line limit".into());
        }
        let mut bytes = std::mem::size_of::<Self>() + self.path.capacity();
        let mut source = 0_usize;
        for line in self.lines.iter() {
            bytes = bytes
                .checked_add(line.capacity())
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<String>()))
                .ok_or_else(|| "Editor retained byte overflow".to_string())?;
            source = source
                .checked_add(line.len() + 1)
                .ok_or_else(|| "Editor source byte overflow".to_string())?;
            if bytes > MAX_EDITOR_RETAINED_BYTES || source > MAX_EDITOR_SOURCE_BYTES {
                return Err("Editor save exceeds retained/source byte limit".into());
            }
        }
        Ok(JobCost {
            input_bytes: bytes + 256 * 1024,
            result_bytes: self.path.capacity() + 256 * 1024,
        })
    }
}
impl Job for EditorWrite {
    type Output = EditorWritten;
    type Error = String;
    fn run(self, _context: JobContext) -> Result<EditorWritten, String> {
        // Once admitted, authored writes are never cancelled or coalesced.
        self.cost()?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self.path.file_name().unwrap_or_default().to_string_lossy();
        let _lock = ExclusiveFileLock::acquire(&parent.join(format!(".{name}.ilium-write.lock")))
            .map_err(|error| error.to_string())?;
        let temporary = parent.join(format!(".{name}.ilium-tmp-{}", uuid::Uuid::new_v4()));
        let result = (|| -> io::Result<()> {
            let file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            let mut writer = io::BufWriter::new(file);
            for line in self.lines.iter() {
                writer.write_all(line.as_bytes())?;
                writer.write_all(b"\n")?;
            }
            writer.flush()?;
            writer.into_inner().map_err(io::Error::from)?.sync_all()?;
            ilium_platform::secure_fs::replace_file_durably(&temporary, &self.path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        Ok(EditorWritten {
            path: self.path,
            source_revision: self.source_revision,
        })
    }
}
