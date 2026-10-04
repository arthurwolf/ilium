//! Bounded, read-only discovery. Decoding determines version and usable coverage.
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Finds direct Java save children, preserving a stable path order.
/// This is only a cheap first pass: level.dat versions and completed chunks
/// must be validated before a folder becomes a renderable map candidate.
pub fn save_folders(saves_root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(saves_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut folders = BTreeSet::new();
    for (index, entry) in entries.enumerate() {
        if index >= 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Minecraft saves folder has too many entries; choose a smaller folder",
            ));
        }
        let entry = entry?;
        let path = entry.path();
        if !metadata_if_present(&path)?.is_some_and(|metadata| metadata.is_dir()) {
            continue;
        }
        let level = metadata_if_present(&path.join("level.dat"))?;
        let region = metadata_if_present(&path.join("region"))?;
        if level.is_some_and(|metadata| metadata.is_file())
            && region.is_some_and(|metadata| metadata.is_dir())
        {
            // Match the canonical spelling used by session admission and history.
            // On Windows std canonicalization may retain a removable \\?\ prefix.
            folders.insert(ilium_platform::paths::canonicalize(&path)?);
        }
        if folders.len() > 512 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Too many Minecraft maps; choose a smaller saves folder",
            ));
        }
    }
    Ok(folders.into_iter().collect())
}

fn metadata_if_present(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn finds_only_save_roots_without_changing_map_bytes() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["new world", "older", "not a map", "missing terrain"] {
            fs::create_dir(directory.path().join(name)).unwrap();
        }
        for name in ["new world", "older", "missing terrain"] {
            fs::write(directory.path().join(name).join("level.dat"), [1, 2, 3, 4]).unwrap();
        }
        for name in ["new world", "older", "not a map"] {
            fs::create_dir(directory.path().join(name).join("region")).unwrap();
        }
        let maps = save_folders(directory.path()).unwrap();
        assert_eq!(
            maps,
            [
                ilium_platform::paths::canonicalize(&directory.path().join("new world")).unwrap(),
                ilium_platform::paths::canonicalize(&directory.path().join("older")).unwrap()
            ]
        );
        for map in maps {
            assert_eq!(fs::read(map.join("level.dat")).unwrap(), [1, 2, 3, 4]);
        }
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 4);
    }
    #[test]
    fn absent_root_is_empty_but_a_file_root_reports_the_problem() {
        let directory = tempfile::tempdir().unwrap();
        assert!(save_folders(&directory.path().join("missing"))
            .unwrap()
            .is_empty());
        let file = directory.path().join("file");
        fs::write(&file, b"x").unwrap();
        assert!(save_folders(&file).is_err());
    }
}
