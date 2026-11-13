//! Finds the project root whose `CHATROOM.md` an `ilium chat` command must use.
//!
//! `CHATROOM.md` is git-ignored, so it exists only in the main checkout. An
//! agent running inside a linked Git worktree (or a pane whose shell moved
//! elsewhere) therefore has no room in any parent directory, yet the user sees
//! the project's room in the interface. This module maps such a working
//! directory back to the owning room. It only reads files: a linked worktree
//! records its main repository in its `.git` file, so no `git` process runs.

use std::fs;
use std::path::{Path, PathBuf};

use crate::chatroom;

/// Message shared by every command that needs an existing room.
pub const NO_CHATROOM_MESSAGE: &str = "this project does not have a chatroom";

/// Picks the project root for a chat command, in this order:
/// 1. the nearest ancestor of `cwd` that holds a room;
/// 2. the main checkout owning the linked worktree that contains `cwd`;
/// 3. the same two lookups from the pane's own worktree (`pane_worktree`,
///    the `ILIUM_WORKTREE` variable), for a shell that left its worktree;
/// 4. `cwd` itself, where a missing room is then reported by the caller.
pub fn resolve_project_root(cwd: &Path, pane_worktree: Option<&Path>) -> PathBuf {
    room_root_from(cwd)
        .or_else(|| pane_worktree.and_then(room_root_from))
        .unwrap_or_else(|| cwd.to_path_buf())
}

/// Room owner for one starting directory: a room in a parent, else the main
/// checkout of the linked worktree it sits in.
fn room_root_from(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|ancestor| chatroom::exists(ancestor))
        .map(Path::to_path_buf)
        .or_else(|| main_checkout_of_linked_worktree(start).filter(|root| chatroom::exists(root)))
}

/// Returns the main checkout root when `start` lies inside a linked Git
/// worktree. The nearest `.git` decides: a directory means `start` is already
/// in a main checkout, a file means a linked worktree whose `gitdir:` line
/// points at `<main>/.git/worktrees/<name>`, and that folder's `commondir`
/// file names the shared `<main>/.git`.
fn main_checkout_of_linked_worktree(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        let dot_git = ancestor.join(".git");
        let Ok(metadata) = fs::symlink_metadata(&dot_git) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            return None;
        }
        return main_checkout_from_dot_git_file(ancestor, &dot_git);
    }
    None
}

fn main_checkout_from_dot_git_file(worktree_root: &Path, dot_git_file: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(dot_git_file).ok()?;
    let gitdir = contents.lines().find_map(|line| {
        line.strip_prefix("gitdir:")
            .map(|path| PathBuf::from(path.trim()))
    })?;
    let gitdir = worktree_root.join(gitdir);
    // A submodule's gitdir has no `commondir`, so it never maps to a room.
    let commondir = fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common_git_dir = fs::canonicalize(gitdir.join(commondir.trim())).ok()?;
    if common_git_dir.file_name()? != ".git" {
        return None;
    }
    common_git_dir.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A main checkout with a room and one linked worktree, laid out the way
    /// `git worktree add` writes them.
    struct Repository {
        _directory: tempfile::TempDir,
        main: PathBuf,
        worktree: PathBuf,
    }

    fn repository_with_room() -> Repository {
        let directory = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(directory.path()).unwrap();
        let main = base.join("main");
        let worktree = base.join("agent-task");
        let gitdir = main.join(".git/worktrees/agent-task");
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        fs::write(main.join(chatroom::CHATROOM_FILE_NAME), "").unwrap();
        Repository {
            _directory: directory,
            main,
            worktree,
        }
    }

    #[test]
    fn nearest_parent_room_wins() {
        let repository = repository_with_room();
        let nested = repository.main.join("src/deep");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(resolve_project_root(&nested, None), repository.main);
    }

    #[test]
    fn linked_worktree_resolves_to_the_main_checkout_room() {
        let repository = repository_with_room();
        let nested = repository.worktree.join("src/deep");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(resolve_project_root(&nested, None), repository.main);
        assert_eq!(
            resolve_project_root(&repository.worktree, None),
            repository.main
        );
    }

    #[test]
    fn a_room_in_the_worktree_itself_takes_precedence() {
        let repository = repository_with_room();
        fs::write(repository.worktree.join(chatroom::CHATROOM_FILE_NAME), "").unwrap();

        assert_eq!(
            resolve_project_root(&repository.worktree, None),
            repository.worktree
        );
    }

    #[test]
    fn pane_worktree_covers_a_shell_that_left_its_worktree() {
        let repository = repository_with_room();
        let scratch = tempfile::tempdir().unwrap();
        let scratch_path = fs::canonicalize(scratch.path()).unwrap();

        assert_eq!(
            resolve_project_root(&scratch_path, Some(&repository.worktree)),
            repository.main
        );
        assert_eq!(resolve_project_root(&scratch_path, None), scratch_path);
    }

    #[test]
    fn worktree_of_a_repository_without_a_room_falls_back_to_the_cwd() {
        let repository = repository_with_room();
        fs::remove_file(repository.main.join(chatroom::CHATROOM_FILE_NAME)).unwrap();

        assert_eq!(
            resolve_project_root(&repository.worktree, None),
            repository.worktree
        );
    }

    #[test]
    fn malformed_or_submodule_dot_git_files_fall_back_to_the_cwd() {
        let directory = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        fs::write(root.join(".git"), "not a gitdir pointer\n").unwrap();
        assert_eq!(resolve_project_root(&root, None), root);

        let submodule_gitdir = root.join("modules/lib");
        fs::create_dir_all(&submodule_gitdir).unwrap();
        fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", submodule_gitdir.display()),
        )
        .unwrap();
        assert_eq!(resolve_project_root(&root, None), root);
    }

    #[test]
    fn a_parent_room_beats_the_worktree_mapping() {
        let repository = repository_with_room();
        let outer_room = repository.main.parent().unwrap().join("CHATROOM.md");
        // The ancestor walk (step 1) must run before any worktree mapping.
        fs::write(&outer_room, "").unwrap();
        fs::remove_file(repository.main.join(chatroom::CHATROOM_FILE_NAME)).unwrap();

        assert_eq!(
            resolve_project_root(&repository.worktree, None),
            repository.main.parent().unwrap()
        );
    }
}
