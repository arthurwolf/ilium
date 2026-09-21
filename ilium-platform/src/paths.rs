//! Resolving a path to its real location without making it unreadable.
//!
//! `std::fs::canonicalize` is the right tool for deciding whether two paths
//! name the same file, but on Windows it returns an *extended-length* path:
//! `\\?\C:\Users\me\project` rather than `C:\Users\me\project`. That prefix is
//! a real Win32 escape hatch, not cosmetic noise -- it lifts the legacy 260
//! character limit and hands the rest of the name to the filesystem verbatim,
//! with none of Win32's own rewriting -- but it should never reach a person.
//! ilium stores canonical paths in board definitions and prints them in status
//! messages, so left alone a Windows user sees `\\?\C:\...` in the interface
//! and in files they may hand-edit later.
//!
//! Stripping it is only safe when Win32, normalizing the remainder itself,
//! still lands on the same file. A UNC share canonicalizes to
//! `\\?\UNC\server\share`, where removing the prefix produces something the OS
//! no longer resolves. A name past the legacy limit, or one whose spelling
//! only survives because the prefix suppressed normalization, would resolve to
//! nothing or to a different file. All of those keep their prefix: an ugly
//! path is a much smaller problem than a path that names the wrong thing.

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

/// The user-wide configuration directory, always absolute.
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
            return Some(anchored_to_working_directory(PathBuf::from(override_dir)));
        }
    }
    directories::ProjectDirs::from("", "", "ilium")
        .map(|project_dirs| project_dirs.config_dir().to_path_buf())
}

/// Resolves a relative path against the working directory, once, where it is
/// read.
///
/// A relative override only means something next to a particular working
/// directory, and ilium's processes do not share one: the CLI spawns the
/// server, and re-execs the client, with `current_dir` set to the project
/// root. Left relative, the same override would silently name a different
/// directory depending on which process opened it and when. Resolving it here
/// also makes the value safe to store, log, and print, which the platform
/// defaults already are.
fn anchored_to_working_directory(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    // Purely lexical: the directory is allowed not to exist yet, since the
    // first run is expected to create it.
    std::path::absolute(&path).unwrap_or(path)
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

/// Removes a Windows extended-length prefix when Win32 resolves the remainder
/// to the very same file on its own. A no-op on every other platform.
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

            // The prefix carries meaning beyond the drive letter, so the
            // shorter spelling is only an improvement while it still opens the
            // same file.
            if resolves_the_same_without_prefix(&simplified) {
                simplified
            } else {
                path
            }
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

/// `MAX_PATH`: the limit Win32 re-imposes on any path that is not
/// extended-length, counting the terminating NUL, so 259 usable characters.
/// Machines that opted into long paths do better, but a path is stored and
/// re-opened by processes that cannot check that per machine, so the
/// conservative limit is the one worth honouring.
#[cfg(windows)]
const LEGACY_PATH_LIMIT: usize = 260;

/// Names Win32 hands to a device rather than to the filesystem, whatever
/// extension follows them. A directory really called `nul` can exist behind an
/// extended-length prefix, and only there.
#[cfg(windows)]
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether Win32's own normalization of `path` reaches the same file the
/// extended-length spelling does.
#[cfg(windows)]
fn resolves_the_same_without_prefix(path: &Path) -> bool {
    use std::path::Component;

    // `len` counts UTF-8 bytes where Windows counts UTF-16 units, so this only
    // ever errs towards keeping a prefix that could have been dropped.
    if path.as_os_str().len() >= LEGACY_PATH_LIMIT {
        return false;
    }

    path.components().all(|component| match component {
        Component::Prefix(_) | Component::RootDir => true,
        // A verbatim path is passed down literally, so a `.` or `..` in one is
        // a directory with that name. Win32 would resolve it as a traversal
        // instead and arrive somewhere else entirely.
        Component::CurDir | Component::ParentDir => false,
        Component::Normal(name) => is_ordinary_file_name(&name.to_string_lossy()),
    })
}

/// Whether Win32 would leave this file name alone. Trailing dots and spaces
/// are trimmed away by the Win32 layer, and a device name is intercepted
/// before the filesystem sees it -- in both cases the extended-length spelling
/// names a file the plain one cannot.
#[cfg(windows)]
fn is_ordinary_file_name(name: &str) -> bool {
    if name.ends_with('.') || name.ends_with(' ') {
        return false;
    }

    // `nul.txt` is the `nul` device too: the extension is not part of the
    // decision, and trailing blanks in the stem are trimmed before it is made.
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    !RESERVED_DEVICE_NAMES
        .iter()
        .copied()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
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

    /// The prefix is what lifts the legacy length limit, so a path long enough
    /// to need it keeps it rather than becoming prettier and unopenable.
    #[cfg(windows)]
    #[test]
    fn a_path_past_the_legacy_limit_keeps_its_prefix() {
        let long_component = "d".repeat(130);
        let long = PathBuf::from(format!(
            r"\\?\C:\{long_component}\{long_component}\file.txt"
        ));

        assert_eq!(simplify(long.clone()), long);
    }

    /// A name only the verbatim spelling can address stays verbatim: Win32
    /// would trim the trailing dot and open a different file.
    #[cfg(windows)]
    #[test]
    fn a_name_win32_would_rewrite_keeps_its_prefix() {
        let trailing_dot = PathBuf::from(r"\\?\C:\project\odd.");

        assert_eq!(simplify(trailing_dot.clone()), trailing_dot);
    }

    /// The same rule covers device names, which Win32 intercepts before the
    /// filesystem ever sees them.
    #[cfg(windows)]
    #[test]
    fn a_device_name_keeps_its_prefix() {
        let device = PathBuf::from(r"\\?\C:\project\nul.txt");

        assert_eq!(simplify(device.clone()), device);
    }
}
