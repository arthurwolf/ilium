//! The open-externally policy for a clicked URL or path: which URL schemes are allowed and which
//! paths may even be looked at. Pure: the plugin never touches the disk here; it asks the host
//! (`fs` import, G10) whether a confined path is a file or a directory and passes the answer to
//! [`classify_path`].
//!
//! Moved from `ilium-client/src/open_target.rs`; the token detection of
//! `terminal_links::link_at` stays in the terminal plugin and arrives as an already extracted
//! URL or path string.

use std::path::{Component, Path, PathBuf};

/// A target the OS's own "open externally" handler can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTarget {
    Url(String),
    File(PathBuf),
    Directory(PathBuf),
}

impl OpenTarget {
    /// The menu label for this target's action.
    pub const fn menu_label(&self) -> &'static str {
        match self {
            Self::Url(_) => "Open in browser",
            Self::File(_) => "Open file",
            Self::Directory(_) => "Open folder",
        }
    }

    /// The verified regular file behind this target.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            Self::Url(_) | Self::Directory(_) => None,
        }
    }
}

/// Validates a URL: only `http` and `https` are offered. An OSC-8 label can be any string the
/// foreground program chose to emit, so this is the single gate.
pub fn resolve_url(url: &str) -> Option<OpenTarget> {
    (url.starts_with("https://") || url.starts_with("http://"))
        .then(|| OpenTarget::Url(url.to_string()))
}

/// Collapses `.` and `..` without touching the disk.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.last() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => normalized.push(component),
            },
            other => normalized.push(other),
        }
    }
    normalized.into_iter().collect()
}

/// The normalized path when it lies inside `cwd` or `home_dir`, otherwise `None`. A path outside
/// both must never reach `stat`: it may sit on an unresponsive mount. Both roots are normalized
/// like the candidate so a `cwd` such as `/a/b/../c` still matches.
pub fn confine(path: &Path, cwd: &Path, home_dir: Option<&Path>) -> Option<PathBuf> {
    let normalized = normalize_lexically(path);
    let normalized_cwd = normalize_lexically(cwd);
    let is_confined = normalized.starts_with(&normalized_cwd)
        || home_dir.is_some_and(|home_dir| normalized.starts_with(normalize_lexically(home_dir)));
    is_confined.then_some(normalized)
}

/// Classifies a confined path from the host's answer about what exists there. A directory wins
/// over a file; a missing path is never offered.
pub fn classify_path(confined: PathBuf, is_dir: bool, is_file: bool) -> Option<OpenTarget> {
    if is_dir {
        Some(OpenTarget::Directory(confined))
    } else if is_file {
        Some(OpenTarget::File(confined))
    } else {
        None
    }
}
