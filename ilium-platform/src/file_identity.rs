//! Stable identity of an already-open file, for detecting replacement.

use std::fs::File;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}

#[cfg(unix)]
pub fn identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
pub fn identity(file: &File) -> io::Result<FileIdentity> {
    let (device, inode) = crate::secure_fs::file_generation(file)?;
    Ok(FileIdentity { device, inode })
}

#[cfg(not(any(unix, windows)))]
pub fn identity(_file: &File) -> io::Result<FileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file identity unavailable",
    ))
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_replaced_file_from_open_original() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transcript");
        let old_path = directory.path().join("original");
        let original = File::create(&path).unwrap();
        let first = identity(&original).unwrap();
        std::fs::rename(&path, old_path).unwrap();
        let replacement = File::create(&path).unwrap();
        assert_ne!(first, identity(&replacement).unwrap());
        assert_eq!(first, identity(&original).unwrap());
    }
}
