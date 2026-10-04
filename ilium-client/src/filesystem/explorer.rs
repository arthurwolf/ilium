//! Bounded finite directory preparation; no filesystem operation runs in
//! ExplorerOverlay's event or render methods.
use crate::explorer_overlay::ExplorerSelection;
use ilium_execution::{Job, JobContext, JobCost};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(crate) struct ExplorerEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// `None` for directories and the `..` entry -- only files show a size.
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}
#[derive(Clone)]
pub(crate) struct ExplorerRead {
    pub revision: u64,
    pub directory: PathBuf,
    pub show_hidden: bool,
    pub selection: ExplorerSelection,
    pub manual_input: Option<String>,
}
pub(crate) struct ExplorerListing {
    pub directory: PathBuf,
    pub entries: Vec<ExplorerEntry>,
}
impl ExplorerRead {
    pub const COST: JobCost = JobCost {
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 4 * 1024 * 1024,
    };
}
impl Job for ExplorerRead {
    type Output = ExplorerListing;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        if context.stop_requested() {
            return Err("Explorer preparation cancelled".into());
        }
        let directory = if self.manual_input.is_some() {
            let canonical = ilium_platform::paths::canonicalize(&self.directory)
                .map_err(|error| error.to_string())?;
            if !canonical.is_dir() {
                return Err("entered path is not a directory".into());
            }
            canonical
        } else {
            self.directory
        };
        if directory.capacity() > 64 * 1024 {
            return Err("Canonical explorer path exceeds retained limit".into());
        }
        let entries = list_entries(&directory, self.show_hidden, self.selection, || {
            context.stop_requested()
        })
        .map_err(|error| error.to_string())?;
        Ok(ExplorerListing { directory, entries })
    }
}

pub(crate) fn list_entries(
    dir: &Path,
    show_hidden: bool,
    selection: ExplorerSelection,
    stop: impl Fn() -> bool,
) -> anyhow::Result<Vec<ExplorerEntry>> {
    let mut entries = Vec::new();
    let mut retained_fields = 0_usize;
    if let Some(parent) = dir.parent() {
        entries.push(ExplorerEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            size: None,
            modified: None,
        });
    }

    for dir_entry in std::fs::read_dir(dir)? {
        if stop() {
            anyhow::bail!("Explorer preparation cancelled");
        }
        if entries.len() >= 8192 {
            anyhow::bail!("Explorer entry limit exceeded (8192)");
        }
        let Ok(dir_entry) = dir_entry else {
            continue;
        };
        let name = dir_entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = dir_entry.path();
        let is_symlink = dir_entry
            .file_type()
            .map(|file_type| file_type.is_symlink())
            .unwrap_or(false);
        // `metadata()` follows symlinks, so a symlink-to-directory is
        // still browsable; fall back to `symlink_metadata()` so a
        // broken link still shows up (as a zero-size, non-directory
        // entry) instead of silently vanishing from the listing.
        let Ok(metadata) = std::fs::metadata(&path).or_else(|_| path.symlink_metadata()) else {
            continue;
        };
        if selection == ExplorerSelection::Folder && !metadata.is_dir() {
            continue;
        }
        let retained = retained_fields
            + entries.capacity() * std::mem::size_of::<ExplorerEntry>()
            + name.capacity()
            + path.capacity();
        if retained > 4 * 1024 * 1024 {
            anyhow::bail!("Explorer listing exceeds 4 MiB retained limit");
        }
        retained_fields += name.capacity() + path.capacity();
        entries.push(ExplorerEntry {
            name,
            path,
            is_dir: metadata.is_dir(),
            is_symlink,
            size: (!metadata.is_dir()).then_some(metadata.len()),
            modified: metadata.modified().ok(),
        });
    }

    let parent_offset = usize::from(entries.first().is_some_and(|entry| entry.name == ".."));
    entries[parent_offset..].sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let final_bytes = dir.as_os_str().len()
        + entries.capacity() * std::mem::size_of::<ExplorerEntry>()
        + entries
            .iter()
            .map(|entry| entry.name.capacity() + entry.path.capacity())
            .sum::<usize>();
    if final_bytes > 4 * 1024 * 1024 {
        anyhow::bail!("Explorer listing exceeds 4 MiB retained limit");
    }
    Ok(entries)
}
