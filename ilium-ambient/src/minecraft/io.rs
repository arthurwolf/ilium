//! Shared read-only file admission for local save metadata and chunk payloads.
use std::{fs::File, io, path::Path};

pub(super) fn open_regular(path: &Path) -> io::Result<File> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "save payload needs a file name",
        )
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // Reuse the platform adapter's handle-relative, read-only admission. On
    // Unix its nonblocking open also rejects a raced-in FIFO without waiting
    // for a writer. This does not bound reads from a stalled filesystem.
    ilium_platform::secure_fs::NoFollowDirectory::open_root(parent)?.open_regular(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn admitted_file_is_read_only_and_preserves_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload");
        std::fs::write(&path, b"saved bytes").unwrap();
        let mut file = open_regular(&path).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"saved bytes");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn directories_are_rejected_before_returning_a_file_handle() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload");
        std::fs::create_dir(&path).unwrap();
        assert!(open_regular(&path).is_err());
    }
}
