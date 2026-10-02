//! Java Edition installation paths. Discovery does not create directories.
use std::path::{Path, PathBuf};

/// A pure platform selector also lets tests cover all three layouts on one OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinecraftPlatform {
    Linux,
    MacOs,
    Windows,
}

/// Resolves the official launcher's Java game directory, without probing disk.
pub fn java_directory_for(
    platform: MinecraftPlatform,
    home: &Path,
    roaming: Option<&Path>,
) -> Option<PathBuf> {
    match platform {
        MinecraftPlatform::Linux => Some(home.join(".minecraft")),
        MinecraftPlatform::MacOs => Some(home.join("Library/Application Support/minecraft")),
        MinecraftPlatform::Windows => roaming.map(|root| root.join(".minecraft")),
    }
}

/// Resolves the current user's official Java installation via OS directory APIs.
/// Alternate launcher folders are supplied explicitly by the scene settings.
pub fn java_directory() -> Option<PathBuf> {
    let directories = directories::BaseDirs::new()?;
    #[cfg(target_os = "linux")]
    let platform = MinecraftPlatform::Linux;
    #[cfg(target_os = "macos")]
    let platform = MinecraftPlatform::MacOs;
    #[cfg(target_os = "windows")]
    let platform = MinecraftPlatform::Windows;
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return None;
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    java_directory_for(
        platform,
        directories.home_dir(),
        Some(directories.data_dir()),
    )
}

/// Lossless, platform-tagged directory binding key; never use display text as identity.
pub fn native_path_key(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut key = std::env::consts::OS.as_bytes().to_vec();
    key.push(0);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        key.extend_from_slice(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            key.extend_from_slice(&unit.to_le_bytes());
        }
    }
    #[cfg(not(any(unix, windows)))]
    return Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Lossless native path encoding is unavailable",
    ));
    #[cfg(any(unix, windows))]
    Ok(key)
}

/// A fresh opaque map identifier from the operating system random source.
pub fn random_map_identifier() -> std::io::Result<[u8; 16]> {
    let mut identifier = [0; 16];
    getrandom::fill(&mut identifier).map_err(|error| {
        std::io::Error::other(format!(
            "Could not allocate Minecraft map identity: {error}"
        ))
    })?;
    Ok(identifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_binding_preserves_unicode_and_significant_whitespace() {
        let first = Path::new("saved worlds 雪");
        let trailing = Path::new("saved worlds 雪 ");
        assert_eq!(
            native_path_key(first).unwrap(),
            native_path_key(first).unwrap()
        );
        assert_ne!(
            native_path_key(first).unwrap(),
            native_path_key(trailing).unwrap()
        );
    }

    #[test]
    fn opaque_identifiers_do_not_reuse_a_constant_or_mutable_metadata() {
        let first = random_map_identifier().unwrap();
        let second = random_map_identifier().unwrap();
        assert_ne!(first, [0; 16]);
        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn unix_path_binding_does_not_collapse_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let first = Path::new(std::ffi::OsStr::from_bytes(b"world\xff"));
        let second = Path::new(std::ffi::OsStr::from_bytes(b"world\xfe"));
        assert_ne!(
            native_path_key(first).unwrap(),
            native_path_key(second).unwrap()
        );
    }
    #[test]
    fn official_paths_cover_three_platforms_and_windows_requires_roaming() {
        let home = Path::new("users/player");
        assert_eq!(
            java_directory_for(MinecraftPlatform::Linux, home, None),
            Some(home.join(".minecraft"))
        );
        assert_eq!(
            java_directory_for(MinecraftPlatform::MacOs, home, None),
            Some(home.join("Library/Application Support/minecraft"))
        );
        let roaming = home.join("AppData/Roaming");
        assert_eq!(
            java_directory_for(MinecraftPlatform::Windows, home, Some(&roaming)),
            Some(roaming.join(".minecraft"))
        );
        assert_eq!(
            java_directory_for(MinecraftPlatform::Windows, home, None),
            None
        );
    }
}
