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

#[cfg(test)]
mod tests {
    use super::*;
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
