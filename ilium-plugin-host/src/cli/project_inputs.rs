//! Deterministic file-input hashing for build freshness (DEV-095/305).
use std::{collections::BTreeMap, io::Read, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};

/// Hash project files, including manifest, lockfiles and toolchain configuration.
/// Generated root directories `dist`, `target`, `node_modules`, `.ilium` and
/// `.git` are excluded. Other files are conservatively inputs: `.gitignore` is
/// a watcher rule (DEV-185), not evidence that a file cannot affect compilation.
/// Symlinks and nonregular entries are refused rather than followed. No guest
/// or compiler is executed. Full DEV-305 build identity additionally requires
/// actual toolchain versions through `project_input_hash_with_toolchain`.
pub fn project_input_hash(project: &Path) -> Result<String, String> {
    project_input_hash_with_toolchain(project, &BTreeMap::new())
}

/// Include actual compiler/runtime and SDK versions supplied by the build
/// adapter. Keys and values are framed and sorted; this never guesses versions
/// from the host environment or from a previously generated build record.
pub fn project_input_hash_with_toolchain(
    project: &Path,
    versions: &BTreeMap<String, String>,
) -> Result<String, String> {
    let root = std::fs::symlink_metadata(project).map_err(|error| format!("project input root: {error}"))?;
    if !root.is_dir() || root.file_type().is_symlink() {
        return Err("project input root must be a real directory".into());
    }
    let mut pending = vec![PathBuf::new()];
    let mut files = Vec::new();
    while let Some(relative) = pending.pop() {
        let entries = std::fs::read_dir(project.join(&relative))
            .map_err(|error| format!("project input directory {}: {error}", relative.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("project input entry: {error}"))?;
            let name = entry.file_name();
            let name = name.to_str().ok_or("project input path is not UTF-8")?;
            if relative.as_os_str().is_empty()
                && matches!(name, "dist" | "target" | "node_modules" | ".ilium" | ".git") {
                continue;
            }
            let path = relative.join(name);
            let kind = entry.file_type().map_err(|error| format!("project input type: {error}"))?;
            if kind.is_symlink() {
                return Err(format!("project input symlink refused: {}", path.display()));
            }
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                files.push(path);
            } else {
                return Err(format!("project input is not a regular file: {}", path.display()));
            }
        }
    }
    // UTF-8 byte order, with portable slash separators, independent of read_dir.
    let mut files = files.into_iter().map(|path| {
        let portable = path.components().map(|part| part.as_os_str().to_str()
            .ok_or_else(|| "project input path is not UTF-8".to_owned()))
            .collect::<Result<Vec<_>, _>>()?.join("/");
        Ok((portable, path))
    }).collect::<Result<Vec<_>, String>>()?;
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hash = Sha256::new();
    hash.update(b"ilium-project-inputs\0\x01");
    hash.update((files.len() as u64).to_le_bytes());
    for (portable, path) in files {
        let absolute = project.join(path);
        let before = std::fs::symlink_metadata(&absolute).map_err(|error| format!("project input {portable}: {error}"))?;
        if !before.is_file() || before.file_type().is_symlink() {
            return Err(format!("project input changed type: {portable}"));
        }
        let mut directory = ilium_platform::secure_fs::NoFollowDirectory::open_root(project)
            .map_err(|error| format!("project input root: {error}"))?;
        let components = Path::new(&portable).components().collect::<Vec<_>>();
        for component in &components[..components.len() - 1] {
            directory = directory.open_directory(component.as_os_str())
                .map_err(|error| format!("project input directory: {error}"))?;
        }
        let mut file = directory.open_regular(components[components.len() - 1].as_os_str())
            .map_err(|error| format!("project input {portable}: {error}"))?;
        let opened = file.metadata().map_err(|error| format!("project input metadata: {error}"))?;
        field(&mut hash, portable.as_bytes());
        hash.update(opened.len().to_le_bytes());
        let mut read = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| format!("project input {portable}: {error}"))?;
            if count == 0 { break; }
            read = read.checked_add(count as u64).ok_or("project input byte count overflow")?;
            hash.update(&buffer[..count]);
        }
        let after = file.metadata().map_err(|error| format!("project input metadata: {error}"))?;
        if read != opened.len() || after.len() != opened.len() || after.modified().ok() != opened.modified().ok() {
            return Err(format!("project input changed while hashing: {portable}"));
        }
    }
    hash.update((versions.len() as u64).to_le_bytes());
    for (name, version) in versions {
        if name.is_empty() || version.is_empty() {
            return Err("toolchain name and version must be nonempty".into());
        }
        field(&mut hash, name.as_bytes());
        field(&mut hash, version.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
