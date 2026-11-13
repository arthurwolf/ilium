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
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::Path;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FileRenameInformation, FileRenameInformationEx, NtCreateFile, NtSetInformationFile,
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows_sys::Win32::Foundation::{
    RtlNtStatusToDosError, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED,
    HANDLE, INVALID_HANDLE_VALUE, NTSTATUS, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    FileDispositionInfo, FileDispositionInfoEx, FileIdInfo, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetFinalPathNameByHandleW, ReOpenFile,
    SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ADD_FILE,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DELETE_CHILD, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO,
    FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES,
    SYNCHRONIZE, VOLUME_NAME_DOS,
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
    /// Clone the admitted directory handle without resolving its pathname again.
    pub fn try_clone_file(&self) -> io::Result<File> {
        self.file.try_clone()
    }

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

    /// Opens the physical parent of this already pinned directory.
    ///
    /// NT has no `..` lookup, so the parent is found through this handle's
    /// own final path and then proven: it must contain an entry that opens to
    /// exactly this directory's identity, otherwise a concurrent rename made
    /// the path stale and the walk fails closed. A volume root is its own
    /// parent, matching the Unix `..` convention the ancestry walk relies on.
    pub(crate) fn open_parent_directory(&self) -> io::Result<Self> {
        let path = final_path(&self.file)?;
        let (Some(parent_path), Some(leaf)) = (path.parent(), path.file_name()) else {
            return Ok(Self {
                file: self.file.try_clone()?,
            });
        };
        let parent = Self::open_root(parent_path)?;
        let child = parent.open_directory(leaf)?;
        if file_identity(&child.file)? != file_identity(&self.file)? {
            return Err(io::Error::other(
                "pinned directory moved while opening its parent",
            ));
        }
        Ok(parent)
    }

    /// Removes an empty child directory only when it still has the identity
    /// the caller pinned. The identity check and the delete act on one
    /// freshly opened handle, so a substitution after the check cannot
    /// redirect the delete. A non-empty directory is refused by the kernel.
    pub(crate) fn remove_empty_directory(
        &self,
        name: &OsStr,
        expected_generation: (u64, u64),
    ) -> io::Result<()> {
        let child = Self {
            file: self.open_relative(name, DELETE_ACCESS, FILE_OPEN, FILE_DIRECTORY_FILE)?,
        };
        require_plain_entry(&child.file, true)?;
        if child.generation()? != expected_generation {
            return Err(io::Error::other("clip directory changed before removal"));
        }
        mark_for_deletion(&child.file)
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

    /// Creates a regular file with the delete right required to rename its
    /// already-open handle into another name under a pinned directory.
    pub(crate) fn create_regular_for_rename(&self, name: &OsStr) -> io::Result<File> {
        self.open_relative(
            name,
            CREATE_FILE_ACCESS | DELETE,
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

    /// Atomically renames a plain file to one child name of this pinned
    /// directory. `replace_existing = false` preserves create-new semantics;
    /// the kernel rejects an occupied destination with `AlreadyExists`.
    ///
    /// The rename is first issued relative to the pinned handle, so no path
    /// is resolved again. Which handle rights the kernel wants on that
    /// `RootDirectory` has differed between Windows builds, so a refusal
    /// retries once through a handle reopened with explicit mutation rights.
    /// If both relative forms are refused, the last resort names the
    /// directory by its current final path. That form is the one `std::fs::rename`
    /// uses and is the only part that re-resolves a path; `publish_entry`
    /// proves afterwards, through the pinned handle, that the published
    /// entry is the staged file, and a substitution surfaces as an error.
    /// A name collision is final and never retried.
    pub(crate) fn rename_regular(
        &self,
        source: &File,
        name: &OsStr,
        replace_existing: bool,
    ) -> io::Result<()> {
        require_plain_entry(source, false)?;
        let name = checked_entry_name(name)?;

        let pinned = rename_by_information(
            source,
            self.file.as_raw_handle() as HANDLE,
            &name,
            replace_existing,
        );
        let Err(pinned_error) = pinned else {
            return Ok(());
        };
        if pinned_error.kind() == io::ErrorKind::AlreadyExists {
            return Err(pinned_error);
        }

        let reopened = self.reopen_child_mutation_handle().and_then(|directory| {
            rename_by_information(
                source,
                directory.as_raw_handle() as HANDLE,
                &name,
                replace_existing,
            )
        });
        let Err(reopened_error) = reopened else {
            return Ok(());
        };
        if reopened_error.kind() == io::ErrorKind::AlreadyExists {
            return Err(reopened_error);
        }

        let mut target: Vec<u16> = final_path(&self.file)?.as_os_str().encode_wide().collect();
        // `\\?\C:\dir` (Win32 extended-length) is `\??\C:\dir` in the NT namespace.
        if target.starts_with(&[92, 92, 63, 92]) {
            target[1] = 63;
        }
        if target.last() != Some(&92) {
            target.push(92);
        }
        target.extend_from_slice(&name);
        rename_by_information(source, std::ptr::null_mut(), &target, replace_existing)
    }

    fn reopen_child_mutation_handle(&self) -> io::Result<File> {
        let handle = unsafe {
            ReOpenFile(
                self.file.as_raw_handle() as HANDLE,
                DIRECTORY_ACCESS | FILE_ADD_FILE | FILE_DELETE_CHILD,
                SHARE_ALL,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let directory = unsafe { File::from_raw_handle(handle as RawHandle) };
        require_plain_entry(&directory, true)?;
        if file_identity(&directory)? != file_identity(&self.file)? {
            return Err(io::Error::other(
                "pinned directory changed while reopening mutation handle",
            ));
        }
        Ok(directory)
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

/// Issues one native rename of `source` to `name`, which is a leaf relative to
/// `root_directory`, or a full NT path when `root_directory` is null.
///
/// Prefers `FileRenameInformationEx` with POSIX semantics: the legacy rename
/// refuses with `ERROR_ACCESS_DENIED` to replace a file that any handle still
/// has open, even one opened with `FILE_SHARE_DELETE`. The caller's staged
/// and published entries are routinely held open by pinned readers, so the
/// replace must unlink the old name immediately instead. Filesystems that
/// lack the extended class fall back to the classic record.
fn rename_by_information(
    source: &File,
    root_directory: HANDLE,
    name: &[u16],
    replace_existing: bool,
) -> io::Result<()> {
    const RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
    const RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;

    let name_bytes = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(invalid_name)?;
    let name_offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let required_bytes = name_offset
        .checked_add(std::mem::size_of_val(name))
        .and_then(|length| length.checked_add(std::mem::size_of::<u16>()))
        .ok_or_else(invalid_name)?;
    let allocation_bytes = required_bytes.max(std::mem::size_of::<FILE_RENAME_INFO>());
    let allocation_length = u32::try_from(allocation_bytes).map_err(|_| invalid_name())?;
    let mut buffer = vec![0_u64; allocation_bytes.div_ceil(std::mem::size_of::<u64>())];
    let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: the zeroed u64-backed allocation has FILE_RENAME_INFO alignment
    // and room for the fixed record, the UTF-16 name and a trailing NUL (left
    // zero). `FILE_RENAME_INFO` and the native `FILE_RENAME_INFORMATION(_EX)`
    // share one layout: a leading 32-bit flags word (whose low byte is
    // `ReplaceIfExists` for the classic record), the root handle, the byte
    // length, then the name.
    unsafe {
        std::ptr::addr_of_mut!((*information).RootDirectory).write(root_directory);
        std::ptr::addr_of_mut!((*information).FileNameLength).write(name_bytes);
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            information.cast::<u8>().add(name_offset).cast::<u16>(),
            name.len(),
        );
    }
    let issue = |class, flags: u32| -> io::Result<()> {
        // SAFETY: the first four bytes of the allocation are the flags word.
        unsafe { information.cast::<u32>().write(flags) };
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: `source` is a live handle with DELETE access and the buffer
        // holds a complete record of `allocation_length` bytes that outlives
        // the call. The native call is used, not `SetFileInformationByHandle`,
        // so the name is passed through unrewritten.
        let status = unsafe {
            NtSetInformationFile(
                source.as_raw_handle() as HANDLE,
                &mut status_block,
                information.cast::<c_void>(),
                allocation_length,
                class,
            )
        };
        if status < 0 {
            return Err(nt_status_error(status));
        }
        Ok(())
    };

    let extended_flags = if replace_existing {
        RENAME_FLAG_REPLACE_IF_EXISTS | RENAME_FLAG_POSIX_SEMANTICS
    } else {
        0
    };
    let error = match issue(FileRenameInformationEx, extended_flags) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    let is_unsupported = error.raw_os_error().is_some_and(|code| {
        matches!(
            code as u32,
            ERROR_INVALID_PARAMETER | ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED
        )
    });
    if !is_unsupported {
        return Err(error);
    }
    issue(FileRenameInformation, u32::from(replace_existing))
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

pub(crate) fn file_generation(file: &File) -> io::Result<(u64, u64)> {
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("file generation requires a regular file"));
    }
    let identity = file_identity(file)?;
    let (low, high) = identity.file_id.split_at(8);
    let low = u64::from_le_bytes(low.try_into().expect("split at eight bytes"));
    let high = u64::from_le_bytes(high.try_into().expect("remaining eight bytes"));
    Ok((identity.volume, low ^ high.rotate_left(32)))
}

/// Reopens this already pinned regular file as a distinct read handle. The
/// returned handle has its own file pointer, so `seek_read` cannot alter the
/// selected handle's position. Identity is checked across the reopen.
pub(crate) fn reopen_regular_for_read(file: &File) -> io::Result<File> {
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("read handle is not a regular file"));
    }
    let expected = file_generation(file)?;
    // SAFETY: `file` is a live handle to an already validated regular file.
    // ReOpenFile returns a new owned handle to that same filesystem object.
    let handle = unsafe {
        ReOpenFile(
            file.as_raw_handle() as HANDLE,
            FILE_GENERIC_READ,
            SHARE_ALL,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: success returned a new handle owned by this process.
    let reopened = unsafe { File::from_raw_handle(handle as RawHandle) };
    if file_generation(&reopened)? != expected {
        return Err(io::Error::other(
            "selected file changed during read-handle reopen",
        ));
    }
    Ok(reopened)
}

/// The extended-length DOS path the handle currently names.
fn final_path(file: &File) -> io::Result<std::path::PathBuf> {
    let handle = file.as_raw_handle() as HANDLE;
    // SAFETY: a null buffer of length zero asks for the required size; the
    // handle is live for the whole call.
    let required = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            std::ptr::null_mut(),
            0,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if required == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0_u16; required as usize];
    // SAFETY: `buffer` is writable for exactly `buffer.len()` UTF-16 units.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if written == 0 || written as usize >= buffer.len() {
        return Err(io::Error::other("final path changed while reading it"));
    }
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..written as usize],
    )))
}

/// Reopens this pinned regular file with only the right to change its
/// timestamps. The selected handle is read-only by design, so this is the one
/// place a recency mark can be written. Identity is checked across the reopen.
pub(crate) fn reopen_regular_for_attribute_write(file: &File) -> io::Result<File> {
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("attribute handle is not a regular file"));
    }
    let expected = file_generation(file)?;
    // SAFETY: `file` is a live handle to an already validated regular file.
    let handle = unsafe {
        ReOpenFile(
            file.as_raw_handle() as HANDLE,
            FILE_WRITE_ATTRIBUTES | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            SHARE_ALL,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: success returned a new handle owned by this process.
    let reopened = unsafe { File::from_raw_handle(handle as RawHandle) };
    if file_generation(&reopened)? != expected {
        return Err(io::Error::other(
            "selected file changed during attribute-handle reopen",
        ));
    }
    Ok(reopened)
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
