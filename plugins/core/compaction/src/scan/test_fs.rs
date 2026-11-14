//! A [`TranscriptFs`] over a real temporary "home" directory, laid out like an agent user's home: the scope
//! `agent-sessions:claude` is `<home>/.claude/projects`, `agent-sessions:codex` is `<home>/.codex/sessions` and
//! `agent-config:codex` is `<home>/.codex`.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::source::{DirItem, EntryInfo, ScanPath, TranscriptFs};

pub(crate) struct StdFs {
    home: PathBuf,
}

impl StdFs {
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
        }
    }

    fn resolve(&self, path: &ScanPath) -> PathBuf {
        let base = match path.scope.as_str() {
            "agent-sessions:claude" => self.home.join(".claude").join("projects"),
            "agent-sessions:codex" => self.home.join(".codex").join("sessions"),
            "agent-config:codex" => self.home.join(".codex"),
            "agent-config:claude" => self.home.join(".claude"),
            other => panic!("unknown scope {other}"),
        };
        path.segments().fold(base, |joined, part| joined.join(part))
    }

    /// The scan path of a real file below the home directory.
    pub(crate) fn scan_path(&self, file: &Path) -> ScanPath {
        let claude = self.home.join(".claude").join("projects");
        let codex = self.home.join(".codex").join("sessions");
        let codex_home = self.home.join(".codex");
        let (scope, base) = if file.starts_with(&claude) {
            ("agent-sessions:claude", claude)
        } else if file.starts_with(&codex) {
            ("agent-sessions:codex", codex)
        } else {
            ("agent-config:codex", codex_home)
        };
        let relative = file
            .strip_prefix(base)
            .expect("the file is below its scope")
            .to_string_lossy()
            .replace('\\', "/");
        ScanPath::new(scope, &relative)
    }
}

fn info_of(metadata: &std::fs::Metadata) -> EntryInfo {
    EntryInfo {
        is_dir: metadata.is_dir(),
        is_file: metadata.is_file(),
        length: metadata.len(),
        modified_ms: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64),
    }
}

impl TranscriptFs for StdFs {
    fn list_dir(&self, path: &ScanPath) -> io::Result<Vec<DirItem>> {
        let mut items = Vec::new();
        for child in std::fs::read_dir(self.resolve(path))?.flatten() {
            let Ok(kind) = child.file_type() else {
                continue;
            };
            let Ok(metadata) = child.metadata() else {
                continue;
            };
            let mut info = info_of(&metadata);
            info.is_dir = kind.is_dir();
            info.is_file = kind.is_file();
            items.push(DirItem {
                name: child.file_name().to_string_lossy().into_owned(),
                info,
            });
        }
        Ok(items)
    }

    fn metadata(&self, path: &ScanPath) -> io::Result<EntryInfo> {
        std::fs::metadata(self.resolve(path)).map(|metadata| info_of(&metadata))
    }

    fn read_at(&self, path: &ScanPath, offset: u64, max: usize) -> io::Result<Vec<u8>> {
        let mut file = std::fs::File::open(self.resolve(path))?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = vec![0_u8; max];
        let mut filled = 0;
        while filled < max {
            let count = file.read(&mut buffer[filled..])?;
            if count == 0 {
                break;
            }
            filled += count;
        }
        buffer.truncate(filled);
        Ok(buffer)
    }
}
