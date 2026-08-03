//! Resolving a path to its real location without making it unreadable.
//!
//! `std::fs::canonicalize` is the right tool for deciding whether two paths
//! name the same file, but on Windows it returns an *extended-length* path:
//! `\\?\C:\Users\me\project` rather than `C:\Users\me\project`. That prefix is
//! a real Win32 escape hatch, not cosmetic noise -- it lifts the legacy 260
//! character limit -- but it should never reach a person. ilium stores
//! canonical paths in board definitions and prints them in status messages, so
//! left alone a Windows user sees `\\?\C:\...` in the interface and in files
//! they may hand-edit later.
//!
//! Stripping it is only safe for ordinary drive-letter paths. A UNC share
//! canonicalizes to `\\?\UNC\server\share`, where removing the prefix would
//! produce something the OS no longer resolves, so those keep it.

use std::io;
use std::path::{Path, PathBuf};

/// Environment override for the configuration directory.
///
/// Exists because there is no portable way to redirect the platform default.
/// `directories` honours `XDG_CONFIG_HOME` on Linux only -- macOS resolves
/// `~/Library/Application Support` and Windows `%APPDATA%`, both ignoring it --
/// so anything wanting to point ilium at a specific configuration, a test
/// pinning an isolated one most of all, has no single lever without this.
pub const CONFIG_DIR_ENV: &str = "ILIUM_CONFIG_DIR";

/// The user-wide configuration directory.
///
/// `~/.config/ilium` on Linux, `~/Library/Application Support/ilium` on macOS,
/// `%APPDATA%\ilium` on Windows -- each platform's own convention, unless
/// [`CONFIG_DIR_ENV`] names one explicitly.
///
/// Defined once here because the CLI, the client, and the server each need it
/// and must agree: three independent resolutions would eventually disagree
/// about where a user's settings live, and only one of them would honour an
/// override.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(override_dir) = std::env::var_os(CONFIG_DIR_ENV) {
        if !override_dir.is_empty() {
            return Some(PathBuf::from(override_dir));
        }
    }
    directories::ProjectDirs::from("", "", "ilium")
        .map(|project_dirs| project_dirs.config_dir().to_path_buf())
}

/// Resolves symlinks and relative components, keeping the result in the form a
/// person would recognise and type.
///
/// Identical to [`std::fs::canonicalize`] everywhere except Windows, where the
/// extended-length prefix is removed when the remainder stands on its own.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(simplify(canonical))
}

/// Removes a Windows extended-length prefix when the rest is a complete path
/// in its own right. A no-op on every other platform.
#[cfg(windows)]
pub fn simplify(path: PathBuf) -> PathBuf {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    match prefix.kind() {
        // `\\?\C:\...` -> `C:\...`. Rebuilding from the disk letter rather
        // than trimming the string keeps this correct for any separator or
        // casing the OS returned.
        Prefix::VerbatimDisk(letter) => {
            let mut simplified = PathBuf::from(format!("{}:\\", letter as char));
            simplified.extend(components);
            simplified
        }
        // `\\?\UNC\server\share` and device paths must keep their prefix:
        // without it the OS resolves something different, or nothing.
        _ => path,
    }
}

#[cfg(not(windows))]
pub fn simplify(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizing_resolves_relative_components() {
        let root = tempfile::tempdir().expect("temp dir");
        let nested = root.path().join("outer").join("inner");
        std::fs::create_dir_all(&nested).expect("create nested");

        let resolved = canonicalize(&nested.join("..")).expect("canonicalize");

        assert!(resolved.ends_with("outer"));
    }

    /// The property that matters on every platform: the result is a path the
    /// OS still resolves, and it names the same directory.
    #[test]
    fn a_simplified_path_still_resolves_to_the_same_place() {
        let root = tempfile::tempdir().expect("temp dir");
        let nested = root.path().join("resolvable");
        std::fs::create_dir_all(&nested).expect("create");

        let resolved = canonicalize(&nested).expect("canonicalize");

        assert!(resolved.is_dir(), "{resolved:?} should still resolve");
        assert_eq!(
            std::fs::canonicalize(&resolved).expect("re-canonicalize"),
            std::fs::canonicalize(&nested).expect("canonicalize original"),
            "simplifying must not change which file is named"
        );
    }

    /// The reason this module exists: no extended-length prefix reaches a
    /// person or a stored board definition.
    #[cfg(windows)]
    #[test]
    fn a_drive_letter_path_loses_its_extended_length_prefix() {
        let root = tempfile::tempdir().expect("temp dir");

        let resolved = canonicalize(root.path()).expect("canonicalize");

        assert!(
            !resolved.to_string_lossy().starts_with(r"\\?\"),
            "{resolved:?} should not carry an extended-length prefix"
        );
    }

    /// A share path is not a drive-letter path, and dropping its prefix would
    /// name something the OS cannot resolve, so it keeps it.
    #[cfg(windows)]
    #[test]
    fn a_unc_share_keeps_its_prefix() {
        let share = PathBuf::from(r"\\?\UNC\server\share\file.txt");

        assert_eq!(simplify(share.clone()), share);
    }
}
