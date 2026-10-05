//! Startup progress shared between the server and a starting client.
//!
//! A server restoring a large session is busy for a long time before it
//! accepts connections. It publishes what it is doing to a small file beside
//! its socket so the client can tell the user, without the server answering
//! IPC. Publishing is best effort: progress is a courtesy, never a gate.

use std::path::{Path, PathBuf};

/// One observation of server startup. `total == 0` means the size of the
/// work is not known yet, so the client shows an indeterminate bar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartupProgress {
    /// General category of work, for example "Restoring panes".
    pub category: String,
    /// The item being worked on, for example a pane title.
    pub item: String,
    pub completed: u64,
    pub total: u64,
}

impl StartupProgress {
    fn encode(&self) -> String {
        let line = |text: &str| text.replace(['\n', '\r'], " ");
        format!(
            "{}\n{}\n{}\n{}\n",
            line(&self.category),
            line(&self.item),
            self.completed,
            self.total
        )
    }

    fn decode(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        Some(Self {
            category: lines.next()?.to_owned(),
            item: lines.next()?.to_owned(),
            completed: lines.next()?.parse().ok()?,
            total: lines.next()?.parse().ok()?,
        })
    }

    /// Fraction in `0.0..=1.0`, or `None` while the total is unknown.
    pub fn fraction(&self) -> Option<f64> {
        (self.total > 0).then(|| (self.completed as f64 / self.total as f64).clamp(0.0, 1.0))
    }
}

/// The progress file that belongs to one session socket.
pub fn startup_progress_path(socket_path: &Path) -> PathBuf {
    let mut name = socket_path.file_name().unwrap_or_default().to_os_string();
    name.push(".startup");
    socket_path.with_file_name(name)
}

/// Atomically replaces the progress file so a reader never sees half a record.
pub fn publish_startup_progress(path: &Path, progress: &StartupProgress) -> std::io::Result<()> {
    let mut temporary_name = path.file_name().unwrap_or_default().to_os_string();
    temporary_name.push(".tmp");
    let temporary = path.with_file_name(temporary_name);
    std::fs::write(&temporary, progress.encode())?;
    std::fs::rename(&temporary, path)
}

/// Reads the latest record; `None` when no startup is in progress.
pub fn read_startup_progress(path: &Path) -> Option<StartupProgress> {
    use std::io::Read;
    // Progress is a courtesy display, not session data. Refuse oversized or
    // growing files before their contents can exceed the reader's job debit.
    const MAX_BYTES: u64 = 64 * 1024;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_BYTES {
        return None;
    }
    StartupProgress::decode(&text)
}

/// Removes the record once the server is ready. A missing file is success.
pub fn clear_startup_progress(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_round_trips_and_strips_line_breaks() {
        let directory = std::env::temp_dir().join(format!("ilium-startup-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = startup_progress_path(&directory.join("session.sock"));
        assert!(path.ends_with("session.sock.startup"));
        let progress = StartupProgress {
            category: "Restoring panes".into(),
            item: "two\nlines".into(),
            completed: 3,
            total: 12,
        };
        publish_startup_progress(&path, &progress).unwrap();
        let read = read_startup_progress(&path).unwrap();
        assert_eq!(read.item, "two lines");
        assert_eq!(read.fraction(), Some(0.25));
        clear_startup_progress(&path);
        assert_eq!(read_startup_progress(&path), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn unknown_total_has_no_fraction() {
        assert_eq!(StartupProgress::default().fraction(), None);
    }

    #[test]
    fn oversized_progress_record_is_refused_without_changing_original_file() {
        let directory =
            std::env::temp_dir().join(format!("ilium-startup-limit-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("session.startup");
        let original = format!("Restoring panes\neditor\n2\n5\n{}", "x".repeat(64 * 1024));
        std::fs::write(&path, &original).unwrap();
        assert!(read_startup_progress(&path).is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
