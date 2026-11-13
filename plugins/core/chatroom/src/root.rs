//! Project-root resolution for `ilium chat`, moved from `ilium-client/src/chatroom_root.rs`.
//!
//! `CHATROOM.md` is git-ignored, so it exists only in the main checkout. A working directory in a
//! linked Git worktree (or a pane whose shell moved elsewhere) is mapped back to the room that
//! owns it. The rules work over path strings and a [`RoomFileSystem`] the caller implements
//! (the `fs` host import, gap G10, or a map in tests), so no process or real file is needed.

/// Message shared by every command that needs an existing room.
pub const NO_CHATROOM_MESSAGE: &str = "this project does not have a chatroom";

/// What the resolver needs to know about the file system.
pub trait RoomFileSystem {
    /// True only for a regular `CHATROOM.md` file in `directory` (a symlink does not count).
    fn has_room(&self, directory: &str) -> bool;
    /// `Some(true)` when `<directory>/.git` is a regular file (a linked worktree), `Some(false)`
    /// when it is a directory, `None` when it does not exist.
    fn dot_git_is_file(&self, directory: &str) -> Option<bool>;
    /// The text of `<directory>/.git` (a file).
    fn read_dot_git_file(&self, directory: &str) -> Option<String>;
    /// The text of the file `<gitdir>/commondir`.
    fn read_commondir(&self, gitdir: &str) -> Option<String>;
    /// The canonical form of `path` (symlinks resolved, `..` folded), when it exists.
    fn canonicalize(&self, path: &str) -> Option<String>;
}

/// `start` and each parent, nearest first, for an absolute `/`-separated path.
pub fn ancestors(start: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = start.trim_end_matches('/').to_string();
    loop {
        if current.is_empty() {
            result.push("/".to_string());
            return result;
        }
        result.push(current.clone());
        match current.rfind('/') {
            Some(index) => current.truncate(index),
            None => return result,
        }
    }
}

/// Joins `base` and a relative or absolute `path`.
pub fn join(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else if base == "/" {
        format!("/{path}")
    } else {
        format!("{}/{path}", base.trim_end_matches('/'))
    }
}

/// The parent of an absolute path and its last component.
fn split_last(path: &str) -> Option<(String, String)> {
    let trimmed = path.trim_end_matches('/');
    let index = trimmed.rfind('/')?;
    let parent = if index == 0 { "/" } else { &trimmed[..index] };
    Some((parent.to_string(), trimmed[index + 1..].to_string()))
}

/// Picks the project root for a chat command, in this order:
/// 1. the nearest ancestor of `cwd` that holds a room;
/// 2. the main checkout owning the linked worktree that contains `cwd`;
/// 3. the same two lookups from the pane's own worktree (`pane_worktree`, the `ILIUM_WORKTREE`
///    variable), for a shell that left its worktree;
/// 4. `cwd` itself, where a missing room is then reported by the caller.
pub fn resolve_project_root(
    file_system: &dyn RoomFileSystem,
    cwd: &str,
    pane_worktree: Option<&str>,
) -> String {
    room_root_from(file_system, cwd)
        .or_else(|| pane_worktree.and_then(|start| room_root_from(file_system, start)))
        .unwrap_or_else(|| cwd.to_string())
}

fn room_root_from(file_system: &dyn RoomFileSystem, start: &str) -> Option<String> {
    ancestors(start)
        .into_iter()
        .find(|ancestor| file_system.has_room(ancestor))
        .or_else(|| {
            main_checkout_of_linked_worktree(file_system, start)
                .filter(|root| file_system.has_room(root))
        })
}

/// The main checkout root when `start` lies inside a linked worktree. The nearest `.git` decides:
/// a directory means `start` is already in a main checkout, a file means a linked worktree whose
/// `gitdir:` line points at `<main>/.git/worktrees/<name>`, and that folder's `commondir` file
/// names the shared `<main>/.git`.
pub fn main_checkout_of_linked_worktree(
    file_system: &dyn RoomFileSystem,
    start: &str,
) -> Option<String> {
    for ancestor in ancestors(start) {
        match file_system.dot_git_is_file(&ancestor) {
            None => continue,
            Some(false) => return None,
            Some(true) => {
                let contents = file_system.read_dot_git_file(&ancestor)?;
                return main_checkout_from_dot_git_file(file_system, &ancestor, &contents);
            }
        }
    }
    None
}

fn main_checkout_from_dot_git_file(
    file_system: &dyn RoomFileSystem,
    worktree_root: &str,
    dot_git_text: &str,
) -> Option<String> {
    let gitdir_text = dot_git_text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:").map(str::trim))?;
    let gitdir = join(worktree_root, gitdir_text);
    // A submodule's gitdir has no `commondir`, so it never maps to a room.
    let commondir = file_system.read_commondir(&gitdir)?;
    let common_git_dir = file_system.canonicalize(&join(&gitdir, commondir.trim()))?;
    let (parent, name) = split_last(&common_git_dir)?;
    if name != ".git" {
        return None;
    }
    Some(parent)
}
