//! Windows implementation of [`crate::secure_fs::NoFollowDirectory`].
//!
//! The Unix implementation leans on `openat(dirfd, name, O_NOFOLLOW)`: every
//! step is relative to a directory handle that is already held, so renaming or
//! replacing a parent after it was opened cannot redirect a later open. Windows
//! has exactly one primitive with those semantics, `NtCreateFile` with
//! `OBJECT_ATTRIBUTES::RootDirectory` set to the parent handle and a
//! single-component relative name. Re-opening a child by absolute path would
//! reintroduce the parent-substitution race this type exists to close.
//!
//! Symbolic links, junctions and every other reparse point are refused:
//!
//! - `FILE_OPEN_REPARSE_POINT` makes each open stop at the final component
//!   instead of following it, so a link is opened *as* a link, never through.
//! - The opened handle is then asked for its own attributes, and any
//!   `FILE_ATTRIBUTE_REPARSE_POINT` fails the call. Refusing all reparse points
//!   (not only name surrogates) is deliberately fail-closed: a cloud
//!   placeholder or deduplicated file opened with `FILE_OPEN_REPARSE_POINT`
//!   does not reliably read back its real contents, so an honest error beats a
//!   silently truncated copy.
//!
//! Every handle is opened with `FILE_SHARE_READ | WRITE | DELETE`, for two
//! reasons. Other code (including this crate's own callers) opens the same
//! entries through ordinary `std::fs` while a handle here is alive, which a
//! narrower share mode would turn into sharing violations. And deleting an
//! entry through [`NoFollowDirectory::remove_regular`] requires every other
//! open handle to the file, including the caller's retained identity handle,
//! to permit delete sharing.

use std::ffi::{c_void, OsStr};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::Path;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
    FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows_sys::Win32::Foundation::{
    RtlNtStatusToDosError, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED,
    HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    FileDispositionInfo, FileDispositionInfoEx, FileIdInfo, GetFileInformationByHandle,
    GetFileInformationByHandleEx, SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Enough to list, traverse and use as the root of relative opens. Creating a
/// child is authorised by the directory's ACL, not by the handle's rights.
const DIRECTORY_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

/// Includes `FILE_WRITE_ATTRIBUTES` (for `File::set_permissions`) and the
/// write right `FlushFileBuffers` needs for `File::sync_all`.
const CREATE_FILE_ACCESS: u32 = FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES;

const DELETE_ACCESS: u32 = DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

/// A directory handle used to walk one child at a time without following
/// symbolic links, junctions, or any other reparse point.
pub struct NoFollowDirectory {
    file: File,
}

/// Identity of an open file: the volume serial plus the 128-bit file ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume: u64,
    file_id: [u8; 16],
}

impl NoFollowDirectory {
    /// Opens `path` itself, refusing it when its final component is a link.
    pub fn open_root(path: &Path) -> io::Result<Self> {
        let before = std::fs::symlink_metadata(path)?;
        if !before.file_type().is_dir() || before.file_type().is_symlink() {
            return Err(io::Error::other("root must be a real directory"));
        }
        let file = OpenOptions::new()
            .access_mode(DIRECTORY_ACCESS)
            .share_mode(SHARE_ALL)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        // The pre-check above is advisory; this one inspects the object the
        // handle actually refers to, so a swap in between cannot pass.
        require_plain_entry(&file, true)?;
        Ok(Self { file })
    }

    /// NTFS journals directory metadata and Windows has no API that flushes a
    /// directory, so there is nothing further to make durable here. Callers
    /// flush the file they created through `File::sync_all`, which commits
    /// that file's own data and metadata.
    pub fn sync_all(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn open_directory(&self, name: &OsStr) -> io::Result<Self> {
        let file = self.open_relative(name, DIRECTORY_ACCESS, FILE_OPEN, FILE_DIRECTORY_FILE)?;
        require_plain_entry(&file, true)?;
        Ok(Self { file })
    }

    /// Create a child directory, returning whether this call created it.
    /// Callers should record a newly created path before opening the child,
    /// since opening can itself fail after mkdir succeeds.
    pub fn create_directory_if_missing(&self, name: &OsStr) -> io::Result<bool> {
        match self.open_relative(
            name,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_CREATE,
            FILE_DIRECTORY_FILE,
        ) {
            // The handle only proved creation; dropping it closes it.
            Ok(_created) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn open_regular(&self, name: &OsStr) -> io::Result<File> {
        let file =
            self.open_relative(name, FILE_GENERIC_READ, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?;
        require_plain_entry(&file, false)?;
        Ok(file)
    }

    /// Creates a new file. `FILE_CREATE` refuses any existing entry, including
    /// a link, with `ErrorKind::AlreadyExists`.
    pub fn create_regular(&self, name: &OsStr) -> io::Result<File> {
        self.open_relative(
            name,
            CREATE_FILE_ACCESS,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
        )
    }

    /// Remove a regular child only when it still names the supplied open file.
    ///
    /// Unlike the Unix compare-then-unlink, the identity check and the delete
    /// act on the same freshly opened handle, so a substitution after the
    /// check cannot redirect the delete.
    pub fn remove_regular(&self, name: &OsStr, expected: &File) -> io::Result<()> {
        if !expected.metadata()?.file_type().is_file() {
            return Err(io::Error::other("expected handle is not a regular file"));
        }
        let expected = file_identity(expected)?;
        let child = self.open_relative(name, DELETE_ACCESS, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?;
        require_plain_entry(&child, false)?;
        if file_identity(&child)? != expected {
            return Err(io::Error::other("regular child changed before removal"));
        }
        mark_for_deletion(&child)
    }

    /// `(volume, file)` folded into two words for the generation fence.
    ///
    /// NTFS file IDs occupy only the low 64 bits, so the fold is exact there;
    /// on a filesystem with wider IDs the high half is mixed in, which can only
    /// make two distinct directories look alike with negligible probability.
    pub(crate) fn generation(&self) -> io::Result<(u64, u64)> {
        let identity = file_identity(&self.file)?;
        let (low, high) = identity.file_id.split_at(8);
        let low = u64::from_le_bytes(low.try_into().expect("split at eight bytes"));
        let high = u64::from_le_bytes(high.try_into().expect("remaining eight bytes"));
        Ok((identity.volume, low ^ high.rotate_left(32)))
    }

    /// One handle-relative open of a single directory entry.
    fn open_relative(
        &self,
        name: &OsStr,
        access: u32,
        disposition: u32,
        options: u32,
    ) -> io::Result<File> {
        let name = checked_entry_name(name)?;
        let byte_length = u16::try_from(name.len() * 2).map_err(|_| invalid_name())?;
        let object_name = UNICODE_STRING {
            Length: byte_length,
            MaximumLength: byte_length,
            Buffer: name.as_ptr().cast_mut(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.file.as_raw_handle() as HANDLE,
            ObjectName: &object_name,
            // Win32 callers get case-insensitive lookups; matching that keeps
            // `Config` and `config` the same entry, as every other tool sees it.
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut handle: HANDLE = std::ptr::null_mut();
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: `attributes` and the `UNICODE_STRING` it points at outlive
        // the call, and the string's buffer is `name`, which is alive for the
        // whole function. `RootDirectory` is a handle this struct owns. On
        // success the call hands back a new handle that is wrapped in `File`
        // immediately below, which takes ownership of closing it.
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status_block,
                std::ptr::null(),
                FILE_ATTRIBUTE_NORMAL,
                SHARE_ALL,
                disposition,
                options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            return Err(nt_status_error(status));
        }
        // SAFETY: a successful `NtCreateFile` returned a new handle owned by
        // this process and not yet wrapped by anything else.
        Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
    }
}

fn invalid_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid child path component")
}

/// Validates one directory-entry name and returns its UTF-16 form.
fn checked_entry_name(name: &OsStr) -> io::Result<Vec<u16>> {
    let as_text = name.to_str().ok_or_else(invalid_name)?;
    if !crate::paths::is_single_entry_name(as_text) {
        return Err(invalid_name());
    }
    Ok(name.encode_wide().collect())
}

fn nt_status_error(status: NTSTATUS) -> io::Error {
    // SAFETY: `RtlNtStatusToDosError` only maps the integer it is given.
    let dos_error = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(dos_error as i32)
}

/// Fails unless the open handle is a plain directory (or plain file) and not
/// a reparse point of any kind.
fn require_plain_entry(file: &File, expect_directory: bool) -> io::Result<()> {
    let attributes = file.metadata()?.file_attributes();
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(
            "refusing to follow a symbolic link, junction, or other reparse point",
        ));
    }
    let is_directory = attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if is_directory != expect_directory {
        return Err(io::Error::other(if expect_directory {
            "child is not a directory"
        } else {
            "child is not a regular file"
        }));
    }
    Ok(())
}

fn file_identity(file: &File) -> io::Result<FileIdentity> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut information = FILE_ID_INFO::default();
    // SAFETY: `information` is a live `FILE_ID_INFO` of exactly the size
    // passed, which is the layout `FileIdInfo` returns; the handle is open.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            std::ptr::addr_of_mut!(information).cast::<c_void>(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded != 0 {
        return Ok(FileIdentity {
            volume: information.VolumeSerialNumber,
            file_id: information.FileId.Identifier,
        });
    }

    // Filesystems without 128-bit IDs: the classic 64-bit index still names
    // the file uniquely within its volume.
    let mut classic = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `classic` is a live, correctly typed output buffer.
    if unsafe { GetFileInformationByHandle(handle, &mut classic) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(classic.nFileIndexHigh) << 32) | u64::from(classic.nFileIndexLow);
    let mut file_id = [0_u8; 16];
    file_id[..8].copy_from_slice(&index.to_le_bytes());
    Ok(FileIdentity {
        volume: u64::from(classic.dwVolumeSerialNumber),
        file_id,
    })
}

/// Unlinks the file `file` names, immediately and even if read-only.
///
/// POSIX delete semantics remove the name at once, while other handles (the
/// caller's retained identity handle) keep the object alive, which is what
/// lets a later `git worktree remove` and a re-creation of the same name
/// succeed. Older systems reject the extended class, so the classic
/// delete-on-close disposition is the fallback.
fn mark_for_deletion(file: &File) -> io::Result<()> {
    let handle = file.as_raw_handle() as HANDLE;
    let extended = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    // SAFETY: the pointer and size describe a live `FILE_DISPOSITION_INFO_EX`.
    let succeeded = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfoEx,
            std::ptr::addr_of!(extended).cast::<c_void>(),
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if succeeded != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    let is_unsupported = error.raw_os_error().is_some_and(|code| {
        matches!(
            code as u32,
            ERROR_INVALID_PARAMETER | ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED
        )
    });
    if !is_unsupported {
        return Err(error);
    }
    let classic = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the pointer and size describe a live `FILE_DISPOSITION_INFO`.
    let succeeded = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            std::ptr::addr_of!(classic).cast::<c_void>(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn create_refuses_an_existing_entry_and_reports_already_exists() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");
        root.create_regular("entry".as_ref()).expect("first create");

        let error = root
            .create_regular("entry".as_ref())
            .expect_err("second create");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!root
            .create_directory_if_missing("entry".as_ref())
            .expect("existing file is reported, not replaced"));
        assert!(root.open_directory("entry".as_ref()).is_err());
    }

    #[test]
    fn child_names_cannot_walk_or_address_streams() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");
        for name in ["..", "a\\b", "a/b", "file:stream", "nul", "trailing."] {
            assert!(root.create_regular(name.as_ref()).is_err(), "{name:?}");
            assert!(root.open_regular(name.as_ref()).is_err(), "{name:?}");
        }
    }

    #[test]
    fn a_missing_child_reports_not_found() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");

        let error = root.open_regular("absent".as_ref()).expect_err("absent");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_junction_is_refused_as_root_and_as_child() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root_path = temp.path().join("root");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&root_path).expect("root");
        std::fs::create_dir(&outside).expect("outside");
        let junction = root_path.join("child");
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&junction)
            .arg(&outside)
            .output()
            .expect("run mklink");
        assert!(
            status.status.success(),
            "mklink /J: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let root = NoFollowDirectory::open_root(&root_path).expect("root handle");

        assert!(root.open_directory("child".as_ref()).is_err());
        assert!(NoFollowDirectory::open_root(&junction).is_err());
        assert!(!outside.join("safe").exists());
    }

    #[test]
    fn remove_rejects_a_replaced_leaf_and_removes_the_exact_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");
        let marker = temp.path().join("marker");
        let moved = temp.path().join("moved");
        std::fs::write(&marker, "owned").expect("marker");
        let owned = root.open_regular("marker".as_ref()).expect("owned handle");
        std::fs::rename(&marker, &moved).expect("rename");
        std::fs::write(&marker, "other").expect("replacement");

        assert!(root.remove_regular("marker".as_ref(), &owned).is_err());
        assert_eq!(
            std::fs::read(&marker).expect("replacement survives"),
            b"other"
        );

        let replacement = root
            .open_regular("marker".as_ref())
            .expect("replacement handle");
        root.remove_regular("marker".as_ref(), &replacement)
            .expect("remove exact replacement");
        assert!(!marker.exists());
    }

    #[test]
    fn a_read_only_copy_can_still_be_removed_while_its_handle_is_retained() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");
        let mut created = root.create_regular("copied".as_ref()).expect("create");
        created.write_all(b"data").expect("write");
        let mut permissions = created.metadata().expect("metadata").permissions();
        permissions.set_readonly(true);
        created.set_permissions(permissions).expect("read-only");
        let retained = created.try_clone().expect("retain identity");
        drop(created);

        root.remove_regular("copied".as_ref(), &retained)
            .expect("remove");

        assert!(!temp.path().join("copied").exists());
    }

    #[test]
    fn generation_is_stable_for_a_directory_and_differs_between_directories() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(temp.path().join("one")).expect("one");
        std::fs::create_dir(temp.path().join("two")).expect("two");
        let one = NoFollowDirectory::open_root(&temp.path().join("one")).expect("one");
        let again = NoFollowDirectory::open_root(&temp.path().join("one")).expect("again");
        let two = NoFollowDirectory::open_root(&temp.path().join("two")).expect("two");

        assert_eq!(one.generation().unwrap(), again.generation().unwrap());
        assert_ne!(one.generation().unwrap(), two.generation().unwrap());
    }
}
