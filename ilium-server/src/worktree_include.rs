//! Bounded, rollback-aware copying of files selected by `.worktreeinclude`.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;
use ilium_platform::secure_fs::NoFollowDirectory;
use walkdir::WalkDir;

const MAX_INCLUDE_FILE_BYTES: u64 = 64 * 1024;
const MAX_COPIED_FILES: usize = 1_000;
const MAX_COPIED_BYTES: u64 = 64 * 1024 * 1024;

/// Paths created by the copy, in creation order. Remove them in reverse order
/// if the enclosing worktree operation fails. Paths that existed beforehand are
/// never included.
#[derive(Debug, Default)]
pub(crate) struct IncludeCopyReport {
    pub(crate) created_paths: Vec<PathBuf>,
    created_files: Vec<(PathBuf, fs::File)>,
}

#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub(crate) struct IncludeCopyError {
    pub(crate) reason: String,
    /// Includes a partially written destination file, if copying failed.
    pub(crate) created_paths: Vec<PathBuf>,
    created_files: Vec<(PathBuf, fs::File)>,
}

fn failure(reason: impl Into<String>, report: IncludeCopyReport) -> IncludeCopyError {
    IncludeCopyError {
        reason: reason.into(),
        created_paths: report.created_paths,
        created_files: report.created_files,
    }
}

/// Undo only files whose original open handles still identify the destination
/// entries. Newly made empty directories may remain until Git removes the
/// worktree; this function never traverses a substituted symlink.
pub(crate) fn remove_created_files(target_root: &Path, error: &IncludeCopyError) -> io::Result<()> {
    remove_created_entries(target_root, &error.created_paths, &error.created_files)
}

/// Use the same exact-file rollback after a successful copy if the requester
/// disconnects before the pane is committed to the session.
pub(crate) fn remove_created_report(
    target_root: &Path,
    report: &IncludeCopyReport,
) -> io::Result<()> {
    remove_created_entries(target_root, &report.created_paths, &report.created_files)
}

fn remove_created_entries(
    target_root: &Path,
    created_paths: &[PathBuf],
    created_files: &[(PathBuf, fs::File)],
) -> io::Result<()> {
    let root = NoFollowDirectory::open_root(target_root)?;
    for path in created_paths {
        if created_files.iter().any(|(owned, _)| owned == path) {
            continue;
        }
        if fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_dir()) {
            return Err(io::Error::other(format!(
                "created file has no retained identity: {}",
                path.display()
            )));
        }
    }
    for (path, expected_file) in created_files.iter().rev() {
        let relative = path
            .strip_prefix(target_root)
            .map_err(|_| io::Error::other("created file escaped worktree"))?;
        let mut parent = None;
        let relative_parent = relative.parent().unwrap_or_else(|| Path::new(""));
        for component in relative_parent.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::other("invalid created file path"));
            };
            parent = Some(parent.as_ref().unwrap_or(&root).open_directory(name)?);
        }
        let name = relative
            .file_name()
            .ok_or_else(|| io::Error::other("created file has no name"))?;
        parent
            .as_ref()
            .unwrap_or(&root)
            .remove_regular(name, expected_file)?;
    }
    Ok(())
}

fn regular_file_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not a regular file: {}", path.display()),
        ));
    }
    Ok(metadata)
}

/// Copy selected regular files from an existing project checkout into a new
/// worktree. A missing `.worktreeinclude` is an empty selection. The destination
/// root must already exist; neither existing files nor symbolic links are
/// followed or replaced. Run this blocking function on a blocking worker.
pub(crate) fn copy_worktree_includes(
    source_root: &Path,
    target_root: &Path,
) -> Result<IncludeCopyReport, IncludeCopyError> {
    let mut report = IncludeCopyReport::default();
    let source = fs::canonicalize(source_root).map_err(|error| {
        failure(
            format!("source root unavailable: {error}"),
            IncludeCopyReport::default(),
        )
    })?;
    let target = fs::canonicalize(target_root).map_err(|error| {
        failure(
            format!("target root unavailable: {error}"),
            IncludeCopyReport::default(),
        )
    })?;
    if source == target || source.starts_with(&target) || target.starts_with(&source) {
        return Err(failure("source and target roots must be separate", report));
    }
    for root in [source_root, target_root] {
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            failure(
                format!("root {}: {error}", root.display()),
                IncludeCopyReport::default(),
            )
        })?;
        if !metadata.file_type().is_dir() {
            return Err(failure(
                format!("root is not a real directory: {}", root.display()),
                report,
            ));
        }
    }
    let source_directory = NoFollowDirectory::open_root(&source).map_err(|error| {
        failure(
            format!("source root: {error}"),
            IncludeCopyReport::default(),
        )
    })?;
    let target_directory = NoFollowDirectory::open_root(&target).map_err(|error| {
        failure(
            format!("target root: {error}"),
            IncludeCopyReport::default(),
        )
    })?;

    let include_path = source.join(".worktreeinclude");
    let include_file = match source_directory.open_regular(".worktreeinclude".as_ref()) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(report),
        Err(error) => {
            return Err(failure(
                format!("{}: {error}", include_path.display()),
                report,
            ))
        }
    };
    if include_file
        .metadata()
        .map_err(|error| {
            failure(
                format!("{}: {error}", include_path.display()),
                IncludeCopyReport::default(),
            )
        })?
        .len()
        > MAX_INCLUDE_FILE_BYTES
    {
        return Err(failure(
            format!(
                "invalid or oversized include file: {}",
                include_path.display()
            ),
            report,
        ));
    }
    let mut contents = Vec::new();
    include_file
        .take(MAX_INCLUDE_FILE_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|error| {
            failure(
                format!("{}: {error}", include_path.display()),
                IncludeCopyReport::default(),
            )
        })?;
    if contents.len() as u64 > MAX_INCLUDE_FILE_BYTES {
        return Err(failure(
            format!("oversized include file: {}", include_path.display()),
            report,
        ));
    }
    let contents = String::from_utf8(contents).map_err(|error| {
        failure(
            format!("{}: {error}", include_path.display()),
            IncludeCopyReport::default(),
        )
    })?;
    let mut builder = GitignoreBuilder::new(&source);
    for (index, line) in contents.lines().enumerate() {
        let line = if index == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            line
        };
        builder
            .add_line(Some(include_path.clone()), line)
            .map_err(|error| {
                failure(
                    format!("{}:{}: {error}", include_path.display(), index + 1),
                    IncludeCopyReport::default(),
                )
            })?;
    }
    let matcher = builder.build().map_err(|error| {
        failure(
            format!("{}: {error}", include_path.display()),
            IncludeCopyReport::default(),
        )
    })?;

    // Discover the complete selection before writing. This catches size and
    // path errors without leaving a half-populated worktree in common cases.
    let mut selected = Vec::new();
    let mut total_bytes = 0_u64;
    for entry in WalkDir::new(&source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || entry.file_name() != ".git")
    {
        let entry = entry.map_err(|error| {
            failure(
                format!("source walk: {error}"),
                IncludeCopyReport::default(),
            )
        })?;
        let path = entry.path();
        let relative = path
            .strip_prefix(&source)
            .map_err(|error| failure(error.to_string(), IncludeCopyReport::default()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        if !matcher
            .matched_path_or_any_parents(path, entry.file_type().is_dir())
            .is_ignore()
        {
            continue;
        }
        if entry.file_type().is_dir() {
            continue;
        }
        let metadata = regular_file_metadata(path).map_err(|error| {
            failure(
                format!("{}: {error}", path.display()),
                IncludeCopyReport::default(),
            )
        })?;
        total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
            failure(
                "included files exceed byte limit",
                IncludeCopyReport::default(),
            )
        })?;
        if selected.len() >= MAX_COPIED_FILES || total_bytes > MAX_COPIED_BYTES {
            return Err(failure("included files exceed count or byte limit", report));
        }
        selected.push((relative.to_path_buf(), metadata.len()));
    }
    selected.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    for (relative, expected_bytes) in selected {
        if !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(failure("included path escapes the source root", report));
        }
        let source_path = source.join(&relative);
        let destination = target.join(&relative);
        let relative_parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let mut source_parent = None;
        let mut target_parent = None;
        let mut created_parent = target.clone();
        for component in relative_parent.components() {
            let Component::Normal(name) = component else {
                return Err(failure("invalid included path component", report));
            };
            let source_next = source_parent
                .as_ref()
                .unwrap_or(&source_directory)
                .open_directory(name)
                .map_err(|error| {
                    failure(
                        format!("{}: {error}", source_path.display()),
                        std::mem::take(&mut report),
                    )
                })?;
            let target_parent_handle = target_parent.as_ref().unwrap_or(&target_directory);
            let was_created = target_parent_handle
                .create_directory_if_missing(name)
                .map_err(|error| {
                    failure(
                        format!("{}: {error}", destination.display()),
                        std::mem::take(&mut report),
                    )
                })?;
            created_parent.push(name);
            if was_created {
                report.created_paths.push(created_parent.clone());
            }
            let target_next = target_parent_handle.open_directory(name).map_err(|error| {
                failure(
                    format!("{}: {error}", created_parent.display()),
                    std::mem::take(&mut report),
                )
            })?;
            source_parent = Some(source_next);
            target_parent = Some(target_next);
        }
        let file_name = relative
            .file_name()
            .ok_or_else(|| failure("included path has no filename", std::mem::take(&mut report)))?;
        let input = source_parent
            .as_ref()
            .unwrap_or(&source_directory)
            .open_regular(file_name)
            .map_err(|error| {
                failure(
                    format!("{}: {error}", source_path.display()),
                    std::mem::take(&mut report),
                )
            })?;
        let metadata = input.metadata().map_err(|error| {
            failure(
                format!("{}: {error}", source_path.display()),
                std::mem::take(&mut report),
            )
        })?;
        if !metadata.file_type().is_file() || metadata.len() != expected_bytes {
            return Err(failure(
                format!("source changed while copying: {}", source_path.display()),
                report,
            ));
        }
        let mut output = target_parent
            .as_ref()
            .unwrap_or(&target_directory)
            .create_regular(file_name)
            .map_err(|error| {
                failure(
                    format!("{}: {error}", destination.display()),
                    std::mem::take(&mut report),
                )
            })?;
        report.created_paths.push(destination.clone());
        let rollback_handle = output.try_clone().map_err(|error| {
            failure(
                format!(
                    "{}: could not retain file identity: {error}",
                    destination.display()
                ),
                std::mem::take(&mut report),
            )
        })?;
        report
            .created_files
            .push((destination.clone(), rollback_handle));
        output
            .set_permissions(metadata.permissions())
            .map_err(|error| {
                failure(
                    format!("{}: {error}", destination.display()),
                    std::mem::take(&mut report),
                )
            })?;
        let copied =
            io::copy(&mut input.take(expected_bytes + 1), &mut output).map_err(|error| {
                failure(
                    format!("{}: {error}", destination.display()),
                    std::mem::take(&mut report),
                )
            })?;
        if copied != expected_bytes {
            return Err(failure(
                format!("source changed while copying: {}", source_path.display()),
                report,
            ));
        }
        output.flush().map_err(|error| {
            failure(
                format!("{}: {error}", destination.display()),
                std::mem::take(&mut report),
            )
        })?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&target).expect("target");
        let source = ilium_platform::paths::canonicalize(&source).expect("canonical source");
        let target = ilium_platform::paths::canonicalize(&target).expect("canonical target");
        (temp, source, target)
    }

    #[test]
    fn copies_selected_files_and_tracks_created_paths() {
        let (_temp, source, target) = roots();
        fs::write(
            source.join(".worktreeinclude"),
            ".env\nconfig/**\n!config/secret\n",
        )
        .expect("include");
        fs::write(source.join(".env"), "TOKEN=sample\n").expect("env");
        fs::create_dir(source.join("config")).expect("config dir");
        fs::write(source.join("config/app"), "app").expect("app");
        fs::write(source.join("config/secret"), "secret").expect("secret");
        let report = copy_worktree_includes(&source, &target).expect("copy");
        assert_eq!(
            fs::read_to_string(target.join(".env")).expect("env"),
            "TOKEN=sample\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("config/app")).expect("app"),
            "app"
        );
        assert!(!target.join("config/secret").exists());
        assert_eq!(report.created_paths.len(), 3);
        assert!(report.created_paths.contains(&target.join("config")));
    }

    #[test]
    fn existing_destination_is_preserved_and_error_lists_prior_writes() {
        let (_temp, source, target) = roots();
        fs::write(source.join(".worktreeinclude"), "a\nb\n").expect("include");
        fs::write(source.join("a"), "new a").expect("a");
        fs::write(source.join("b"), "new b").expect("b");
        fs::write(target.join("b"), "old b").expect("existing b");
        let error = copy_worktree_includes(&source, &target).expect_err("collision");
        assert_eq!(fs::read_to_string(target.join("b")).expect("b"), "old b");
        assert_eq!(error.created_paths, vec![target.join("a")]);
        remove_created_files(&target, &error).expect("remove only the copied file");
        assert!(!target.join("a").exists());
        assert_eq!(fs::read_to_string(target.join("b")).expect("b"), "old b");
    }

    #[test]
    fn rollback_refuses_a_replaced_copied_file() {
        let (_temp, source, target) = roots();
        fs::write(source.join(".worktreeinclude"), "a\nb\n").expect("include");
        fs::write(source.join("a"), "new a").expect("a");
        fs::write(source.join("b"), "new b").expect("b");
        fs::write(target.join("b"), "old b").expect("existing b");
        let error = copy_worktree_includes(&source, &target).expect_err("collision");
        fs::remove_file(target.join("a")).expect("replace a");
        fs::write(target.join("a"), "someone else's a").expect("replacement");
        assert!(remove_created_files(&target, &error).is_err());
        assert_eq!(
            fs::read_to_string(target.join("a")).expect("replacement"),
            "someone else's a"
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_symlink_and_symlinked_destination_parent_are_rejected() {
        use std::os::unix::fs::symlink;

        let (_temp, source, target) = roots();
        fs::write(source.join(".worktreeinclude"), "linked\n").expect("include");
        symlink("outside", source.join("linked")).expect("link");
        assert!(copy_worktree_includes(&source, &target).is_err());

        fs::write(source.join(".worktreeinclude"), "config/app\n").expect("include");
        fs::create_dir(source.join("config")).expect("source config");
        fs::write(source.join("config/app"), "app").expect("app");
        symlink(&source, target.join("config")).expect("target link");
        let error = copy_worktree_includes(&source, &target).expect_err("target symlink");
        assert!(error.created_paths.is_empty());
        assert!(!source.join("app").exists());
    }

    #[test]
    fn missing_include_file_copies_nothing() {
        let (_temp, source, target) = roots();
        fs::write(source.join("a"), "a").expect("a");
        let report = copy_worktree_includes(&source, &target).expect("empty selection");
        assert!(report.created_paths.is_empty());
        assert!(!target.join("a").exists());
    }

    #[test]
    fn byte_limit_rejects_selection_before_writing() {
        let (_temp, source, target) = roots();
        fs::write(source.join(".worktreeinclude"), "huge\n").expect("include");
        fs::File::create(source.join("huge"))
            .expect("huge")
            .set_len(MAX_COPIED_BYTES + 1)
            .expect("sparse file");
        let error = copy_worktree_includes(&source, &target).expect_err("limit");
        assert!(error.created_paths.is_empty());
        assert!(!target.join("huge").exists());
    }
}
