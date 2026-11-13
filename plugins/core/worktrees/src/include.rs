//! `.worktreeinclude` selection planning, moved from `ilium-server/src/worktree_include.rs`.
//!
//! The kernel walked the filesystem, matched gitignore rules and copied files in one function.
//! The plugin splits it: this module is the pure planner that applies the limits and the path
//! rules to a list of source entries a host `fs` walk returned (gap G10), with the pattern match
//! supplied by the caller. Copying and rollback stay behind the `fs` import.

/// Largest `.worktreeinclude` accepted.
pub const MAX_INCLUDE_FILE_BYTES: u64 = 64 * 1024;
/// Most files one worktree creation copies.
pub const MAX_COPIED_FILES: usize = 1_000;
/// Most bytes one worktree creation copies.
pub const MAX_COPIED_BYTES: u64 = 64 * 1024 * 1024;
/// Most source entries visited while discovering the selection.
pub const MAX_VISITED_SOURCE_ENTRIES: usize = 10_000;
/// Name of the include file at the project root.
pub const INCLUDE_FILE_NAME: &str = ".worktreeinclude";

/// One entry of the source walk, relative to the source root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub relative_path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub bytes: u64,
}

/// A failed selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeError(pub String);

impl std::fmt::Display for IncludeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IncludeError {}

fn error(reason: &str) -> IncludeError {
    IncludeError(reason.to_owned())
}

/// Checks the include file's size and text before patterns are built from it.
pub fn validate_include_file(contents: &[u8]) -> Result<&str, IncludeError> {
    if contents.len() as u64 > MAX_INCLUDE_FILE_BYTES {
        return Err(error("oversized include file"));
    }
    let text = std::str::from_utf8(contents).map_err(|reason| IncludeError(reason.to_string()))?;
    Ok(text.trim_start_matches('\u{feff}'))
}

/// True for a relative path made only of normal components (no `.`, `..`, empty or absolute).
pub fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// True when two roots are the same or nested, which would copy a tree into itself.
pub fn roots_overlap(source: &str, target: &str) -> bool {
    use crate::location::is_within;
    source == target || is_within(source, target) || is_within(target, source)
}

/// Applies the walk limits, the match and the file rules, and returns the files to copy sorted
/// by path. `entries` must be in walk order; `.git` directories are skipped (not descended
/// into, not counted), exactly as the kernel's `filter_entry` did.
pub fn plan_selection(
    entries: &[SourceEntry],
    matches: impl Fn(&str, bool) -> bool,
    mut stop_requested: impl FnMut() -> bool,
) -> Result<Vec<SourceEntry>, IncludeError> {
    let mut selected: Vec<SourceEntry> = Vec::new();
    let mut total_bytes = 0_u64;
    let mut visited = 0_usize;
    for entry in entries {
        if stop_requested() {
            return Err(error("include preparation cancelled"));
        }
        let in_git_directory = entry
            .relative_path
            .split('/')
            .any(|component| component == ".git");
        if in_git_directory {
            continue;
        }
        visited += 1;
        if visited > MAX_VISITED_SOURCE_ENTRIES {
            return Err(error("source entry limit exceeded"));
        }
        if entry.relative_path.is_empty() || !matches(&entry.relative_path, entry.is_dir) {
            continue;
        }
        if entry.is_dir {
            continue;
        }
        if entry.is_symlink {
            return Err(IncludeError(format!(
                "{}: not a regular file",
                entry.relative_path
            )));
        }
        if !is_safe_relative_path(&entry.relative_path) {
            return Err(error("included path escapes the source root"));
        }
        total_bytes = total_bytes
            .checked_add(entry.bytes)
            .ok_or_else(|| error("included files exceed byte limit"))?;
        if selected.len() >= MAX_COPIED_FILES || total_bytes > MAX_COPIED_BYTES {
            return Err(error("included files exceed count or byte limit"));
        }
        selected.push(entry.clone());
    }
    selected.sort_unstable_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(selected)
}

/// Parent directories of `relative_path`, shallowest first, that must exist in the target.
pub fn parent_directories(relative_path: &str) -> Vec<String> {
    let mut parents = Vec::new();
    let mut prefix = String::new();
    let mut components = relative_path.split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
        parents.push(prefix.clone());
    }
    parents
}
