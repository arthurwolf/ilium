//! Two native-account finite-provider slots; invoke only inside bounded IO work.
//! This namespace never depends on settings, project, environment, provider, or key.
//! Files remain permanently named: neither release nor recovery unlinks a slot.
use crate::file_lock::ExclusiveFileLock;
use crate::secure_fs::NoFollowDirectory;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::PathBuf;
pub fn try_acquire_finite_provider_slot() -> io::Result<Option<ExclusiveFileLock>> {
    let account_home = crate::paths::canonicalize(&native_account_home()?)?;
    let home_directory = NoFollowDirectory::open_root(&account_home)?;
    validate_directory(&home_directory, false)?;
    let namespace_name = OsStr::new(".ilium-provider-admission");
    home_directory.create_directory_if_missing(namespace_name)?;
    let slot_directory = home_directory.open_directory(namespace_name)?;
    validate_directory(&slot_directory, true)?;
    try_acquire_in_directory(&slot_directory)
}
fn try_acquire_in_directory(
    directory: &NoFollowDirectory,
) -> io::Result<Option<ExclusiveFileLock>> {
    for slot_name in ["finite-0.lock", "finite-1.lock"] {
        let file = match directory.create_regular(OsStr::new(slot_name)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                directory.open_regular(OsStr::new(slot_name))?
            }
            Err(error) => return Err(error),
        };
        validate_slot_file(&file)?;
        if let Some(lease) = ExclusiveFileLock::try_acquire_opened(file)? {
            return Ok(Some(lease));
        }
    }
    Ok(None)
}
fn validate_directory(directory: &NoFollowDirectory, private: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = directory.try_clone_file()?.metadata()?;
        let forbidden_mode = if private { 0o077 } else { 0o022 };
        let user_id = unsafe { libc::geteuid() }; // SAFETY: geteuid takes no pointers and reports current filesystem authority.
        if !metadata.is_dir() || metadata.uid() != user_id || metadata.mode() & forbidden_mode != 0
        {
            return Err(refused(
                "finite provider admission requires an owned private namespace",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (directory, private);
    Ok(())
}
fn validate_slot_file(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(refused(
            "finite provider admission slot is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let user_id = unsafe { libc::geteuid() }; // SAFETY: this pointer-free call identifies current filesystem authority.
        if metadata.uid() != user_id || metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            return Err(refused(
                "finite provider admission slot ownership or links are unsafe",
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) }
            == 0
        {
            // SAFETY: the live file handle and writable result storage remain valid.
            return Err(io::Error::last_os_error());
        }
        let information = unsafe { information.assume_init() }; // SAFETY: GetFileInformationByHandle succeeded above.
        if information.nNumberOfLinks != 1
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(refused("finite provider admission slot links are unsafe"));
        }
    }
    Ok(())
}
fn refused(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, reason)
}
#[cfg(unix)]
fn native_account_home() -> io::Result<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    let user_id = unsafe { libc::geteuid() }; // SAFETY: geteuid takes no pointers and returns the effective native UID.
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = vec![0_u8; 65_536];
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            user_id,
            account.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    }; // SAFETY: result, record, and byte buffer are writable for this complete call.
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() {
        return Err(refused("finite provider admission has no native account"));
    }
    let account = unsafe { account.assume_init() }; // SAFETY: getpwuid_r succeeded and returned a nonnull account result.
    if account.pw_uid != user_id || account.pw_dir.is_null() {
        return Err(refused(
            "finite provider admission native account is invalid",
        ));
    }
    let directory = unsafe { CStr::from_ptr(account.pw_dir) }; // SAFETY: successful getpwuid_r supplies a valid NUL-terminated field.
    let path = PathBuf::from(OsStr::from_bytes(directory.to_bytes()));
    if !path.is_absolute() {
        return Err(refused(
            "finite provider admission native home is not absolute",
        ));
    }
    Ok(path)
}
#[cfg(windows)]
fn native_account_home() -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;
    let mut raw_token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0 {
        // SAFETY: current-process pseudohandle and writable token output are valid.
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(raw_token) }; // SAFETY: the freshly returned token is valid and uniquely owned.
    let mut buffer = vec![0_u16; 32_768];
    let mut length = 32_768_u32;
    if unsafe { GetUserProfileDirectoryW(token.as_raw_handle(), buffer.as_mut_ptr(), &mut length) }
        == 0
    {
        // SAFETY: token and the declared writable buffer remain alive for this call.
        return Err(io::Error::last_os_error());
    }
    let length = length as usize;
    if length < 2 || length > buffer.len() || buffer[length - 1] != 0 {
        return Err(refused(
            "finite provider admission native profile is invalid",
        ));
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..length - 1]));
    if !path.is_absolute() {
        return Err(refused(
            "finite provider admission native profile is not absolute",
        ));
    }
    Ok(path)
}
#[cfg(not(any(unix, windows)))]
fn native_account_home() -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native finite provider admission is unavailable",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    fn isolated_probe(namespace: &Path) -> io::Result<Option<ExclusiveFileLock>> {
        let directory = NoFollowDirectory::open_root(namespace)?;
        validate_directory(&directory, true)?;
        try_acquire_in_directory(&directory)
    }

    struct OwnedChild(Child);

    impl OwnedChild {
        fn wait_for_marker(&mut self, marker: &Path) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if marker.is_file() {
                    return;
                }
                if let Some(status) = self.0.try_wait().expect("child status") {
                    panic!("slot child exited before acquiring: {status}");
                }
                assert!(
                    Instant::now() < deadline,
                    "slot child acquisition timed out"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn release_and_wait(&mut self) {
            drop(self.0.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = self.0.try_wait().expect("child status") {
                    assert!(status.success(), "slot child failed: {status}");
                    return;
                }
                assert!(Instant::now() < deadline, "slot child exit timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    fn child_holding_slot(root: &Path, namespace: &Path, index: usize) -> OwnedChild {
        let home = root.join(format!("home-{index}"));
        let project = root.join(format!("project-{index}"));
        std::fs::create_dir(&home).expect("isolated home");
        std::fs::create_dir(&project).expect("isolated project");
        let marker = root.join(format!("ready-{index}"));
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "--nocapture",
                "--ignored",
                "provider_admission::tests::host_slot_child_helper",
            ])
            .current_dir(project)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_RUNTIME_DIR", home.join("runtime"))
            .env("ILIUM_TEST_SLOT_NAMESPACE", namespace)
            .env("ILIUM_TEST_SLOT_MARKER", &marker)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn isolated slot child");
        let mut owned = OwnedChild(child);
        owned.wait_for_marker(&marker);
        owned
    }

    #[test]
    fn two_children_share_exact_slots_and_exit_releases_one() {
        let root = tempfile::tempdir().expect("test root");
        let namespace = root.path().join("isolated-slots");
        crate::secure_fs::create_private_directory(&namespace).expect("private namespace");
        let mut first = child_holding_slot(root.path(), &namespace, 0);
        let second = child_holding_slot(root.path(), &namespace, 1);
        assert!(isolated_probe(&namespace).expect("third probe").is_none());

        let mut entries = std::fs::read_dir(&namespace)
            .expect("slot directory")
            .map(|entry| {
                entry
                    .expect("slot entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(
            entries,
            vec!["finite-0.lock".to_owned(), "finite-1.lock".to_owned()]
        );

        first.release_and_wait();
        let replacement = isolated_probe(&namespace)
            .expect("probe after real child exit")
            .expect("one exited child releases one slot");
        assert!(isolated_probe(&namespace)
            .expect("other child still owns slot")
            .is_none());
        drop(replacement);
        drop(second);
    }

    #[test]
    fn malformed_symlink_hardlink_and_public_namespace_fail_closed() {
        let root = tempfile::tempdir().expect("test root");
        for (name, make_bad_slot) in [("directory", 0_u8), ("symlink", 1_u8), ("hardlink", 2_u8)] {
            let namespace = root.path().join(name);
            crate::secure_fs::create_private_directory(&namespace).expect("private namespace");
            let slot = namespace.join("finite-0.lock");
            match make_bad_slot {
                0 => std::fs::create_dir(&slot).expect("malformed directory slot"),
                1 => symlink(root.path().join("outside"), &slot).expect("slot symlink"),
                _ => {
                    std::fs::write(&slot, b"").expect("regular slot");
                    std::fs::set_permissions(&slot, std::fs::Permissions::from_mode(0o600))
                        .expect("private slot mode isolates hardlink check");
                    std::fs::hard_link(&slot, root.path().join("other-link"))
                        .expect("second hard link");
                }
            }
            assert!(
                isolated_probe(&namespace).is_err(),
                "{name} must fail closed"
            );
            assert!(!namespace.join("finite-1.lock").exists());
        }

        let public = root.path().join("public");
        crate::secure_fs::create_private_directory(&public).expect("private first");
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755))
            .expect("make existing namespace public");
        assert_eq!(
            isolated_probe(&public)
                .expect_err("public namespace refused")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    #[ignore = "invoked only by the isolated multiprocess admission test"]
    fn host_slot_child_helper() {
        let namespace = PathBuf::from(
            std::env::var_os("ILIUM_TEST_SLOT_NAMESPACE").expect("namespace from parent"),
        );
        let marker =
            PathBuf::from(std::env::var_os("ILIUM_TEST_SLOT_MARKER").expect("marker from parent"));
        let altered_home = PathBuf::from(std::env::var_os("HOME").expect("altered home"));
        assert_ne!(
            native_account_home().expect("native account home"),
            altered_home
        );
        let _lease = isolated_probe(&namespace)
            .expect("isolated probe")
            .expect("one of the two slots");
        std::fs::write(marker, b"held").expect("signal acquired slot");
        let mut release = [0_u8; 1];
        let _ = std::io::stdin()
            .read(&mut release)
            .expect("wait for release");
    }
}
