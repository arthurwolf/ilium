//! Branch names, slugs and worktree locations (moved from `ilium-core` and `worktree_dialog.rs`).
//!
//! Paths are plain `/`-separated strings: the plugin receives and returns paths as text, and the
//! host decides the platform separator. Everything here is pure.

/// Why a branch name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchNameError {
    Empty,
    ForbiddenCharacter,
    InvalidSequence,
    InvalidComponent,
}

impl std::fmt::Display for BranchNameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "branch name is empty",
            Self::ForbiddenCharacter => "branch name contains a forbidden Git ref character",
            Self::InvalidSequence => "branch name contains an invalid Git ref sequence",
            Self::InvalidComponent => "branch name contains an invalid path component",
        })
    }
}

impl std::error::Error for BranchNameError {}

/// Fast branch validation for an editable form; Git's own `check-ref-format` still runs before
/// anything is created.
pub fn validate_branch_name(branch: &str) -> Result<(), BranchNameError> {
    if branch.is_empty() {
        return Err(BranchNameError::Empty);
    }
    if branch == "@" || branch.contains("..") || branch.contains("@{") || branch.contains("//") {
        return Err(BranchNameError::InvalidSequence);
    }
    if branch.chars().any(|character| {
        character.is_control()
            || matches!(character, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return Err(BranchNameError::ForbiddenCharacter);
    }
    if branch.starts_with('-')
        || branch.ends_with('/')
        || branch.ends_with('.')
        || branch.split('/').any(|component| {
            component.is_empty() || component.starts_with('.') || component.ends_with(".lock")
        })
    {
        return Err(BranchNameError::InvalidComponent);
    }
    Ok(())
}

/// A conservative, readable component for an automatically suggested branch (at most 48 bytes).
pub fn slugify_branch(text: &str) -> String {
    let mut slug = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "task".to_owned()
    } else {
        slug.to_owned()
    }
}

/// Expands `{repo_parent}`, `{repo_name}`, `{project}` and `{branch_slug}` in one left-to-right
/// pass, so a placeholder spelled inside a substituted value stays literal.
pub fn expand_worktree_path_template(
    template: &str,
    project_path: &str,
    repo_parent: &str,
    repo_name: &str,
    branch_slug: &str,
) -> String {
    let mut expanded = String::with_capacity(template.len());
    let mut remaining = template;
    while !remaining.is_empty() {
        if let Some(tail) = remaining.strip_prefix("{repo_parent}") {
            expanded.push_str(repo_parent);
            remaining = tail;
        } else if let Some(tail) = remaining.strip_prefix("{repo_name}") {
            expanded.push_str(repo_name);
            remaining = tail;
        } else if let Some(tail) = remaining.strip_prefix("{project}") {
            expanded.push_str(project_path);
            remaining = tail;
        } else if let Some(tail) = remaining.strip_prefix("{branch_slug}") {
            expanded.push_str(branch_slug);
            remaining = tail;
        } else if let Some(character) = remaining.chars().next() {
            expanded.push(character);
            remaining = &remaining[character.len_utf8()..];
        }
    }
    expanded
}

/// Rewrites the template's `/` to the platform separator.
pub fn native_separators(rendered: &str, separator: char) -> String {
    if separator == '/' {
        rendered.to_owned()
    } else {
        rendered.replace('/', &separator.to_string())
    }
}

/// Parent directory and final component of a path, ignoring trailing separators.
pub fn split_parent(path: &str) -> (&str, &str) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => ("/", &trimmed[1..]),
        Some(index) => (&trimmed[..index], &trimmed[index + 1..]),
        None => ("", trimmed),
    }
}

/// The default location of a new worktree, derived from the main registered worktree even when
/// the project was opened inside a linked one. A relative expansion falls back to
/// `<main>/.ilium/worktrees/<slug>`.
pub fn default_worktree_path(
    template: &str,
    main_worktree: &str,
    branch: &str,
    separator: char,
) -> String {
    let slug = slugify_branch(branch);
    let (parent, name) = split_parent(main_worktree);
    let name = if name.is_empty() { "project" } else { name };
    let rendered = expand_worktree_path_template(template, main_worktree, parent, name, &slug);
    let path = native_separators(&rendered, separator);
    if path.starts_with('/') || is_windows_absolute(&path) {
        path
    } else {
        format!(
            "{}/.ilium/worktrees/{slug}",
            main_worktree.trim_end_matches('/')
        )
    }
}

fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// The preferred branch, or `<preferred>-N` for the first free N in 2..=9999.
pub fn unique_branch(preferred: &str, local_branches: &[String]) -> String {
    let is_taken = |branch: &str| local_branches.iter().any(|name| name == branch);
    if !is_taken(preferred) {
        return preferred.to_owned();
    }
    for suffix in 2..=9999 {
        let candidate = format!("{preferred}-{suffix}");
        if !is_taken(&candidate) {
            return candidate;
        }
    }
    preferred.to_owned()
}

/// True when `path` equals `ancestor` or lies below it (component-wise).
pub fn is_within(path: &str, ancestor: &str) -> bool {
    let ancestor = ancestor.trim_end_matches('/');
    path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Why a proposed new worktree path was refused.
pub fn validate_new_path(
    path: &str,
    repo_common_dir: &str,
    existing_worktrees: &[&str],
) -> Result<(), String> {
    let normalized = path.starts_with('/')
        && !path.trim_end_matches('/').is_empty()
        && path
            .split('/')
            .all(|component| component != "." && component != "..");
    if !normalized {
        return Err("Worktree path must be absolute and normalized".into());
    }
    if is_within(path, repo_common_dir)
        || existing_worktrees
            .iter()
            .any(|entry| is_within(path, entry) || is_within(entry, path))
    {
        return Err("Worktree path overlaps an existing checkout or Git metadata".into());
    }
    Ok(())
}
