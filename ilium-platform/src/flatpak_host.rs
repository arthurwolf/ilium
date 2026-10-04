//! Flatpak's distribution wrapper hands the *whole trusted CLI* to the host.
//!
//! This avoids trying to create nested namespaces inside Flatpak. The host CLI
//! then owns its normal server, animation helper, and native resource domains.
//! The host command permission is a full sandbox escape for the trusted CLI;
//! unverified animation code must still enter only the existing sealed helper.

use std::{io, process::ExitStatus};

#[cfg(target_os = "linux")]
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};

#[cfg(target_os = "linux")]
const INFO_PATH: &str = "/.flatpak-info";
#[cfg(target_os = "linux")]
const EXPECTED_APP_ID: &str = "io.github.arthurwolf.Ilium";
#[cfg(target_os = "linux")]
const HOST_IDENTITY_ENV: &str = "ILIUM_FLATPAK_HOST_IDENTITY";
#[cfg(target_os = "linux")]
const PAYLOAD_DIRECTORY: &str = "lib/ilium";
#[cfg(target_os = "linux")]
const MAX_INFO_BYTES: u64 = 64 * 1024;
#[cfg(target_os = "linux")]
const MAX_EXECUTABLE_BYTES: u64 = 1024 * 1024 * 1024;

#[cfg(target_os = "linux")]
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// A process either runs the installed Flatpak payload on the host, verifies
/// that handoff, or runs normally outside Flatpak. Call before CLI parsing or
/// starting threads so the identity marker cannot leak to panes/agents.
#[cfg(target_os = "linux")]
pub fn maybe_handoff() -> io::Result<Option<ExitStatus>> {
    let expected = std::env::var(HOST_IDENTITY_ENV).ok();
    let info = match File::open(INFO_PATH) {
        Ok(file) => Some(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(expected) = expected {
        if info.is_some() {
            return Err(invalid("Flatpak host handoff remained inside a sandbox"));
        }
        verify_host_identity(&expected)?;
        // This runs before any worker or runtime starts; the marker must not
        // reach detached servers or pane commands through the environment.
        std::env::remove_var(HOST_IDENTITY_ENV);
        return Ok(None);
    }
    let Some(info) = info else {
        return Ok(None);
    };
    let app_path = running_app_path(info)?;
    let own_executable = std::env::current_exe()?;
    if own_executable.canonicalize()? != Path::new("/app/lib/ilium/ilium") {
        return Err(invalid("Flatpak CLI is outside its expected payload path"));
    }
    let payload = Path::new("/app").join(PAYLOAD_DIRECTORY);
    let identity = [
        hash_executable(Path::new("/proc/self/exe"))?,
        hash_executable(&payload.join("ilium-server"))?,
        hash_executable(&payload.join("ilium-animation-helper"))?,
    ]
    .join(":");
    let host_executable = app_path.join(PAYLOAD_DIRECTORY).join("ilium");
    let cwd = std::env::current_dir()?;
    if !cwd.is_absolute() || cwd.to_str().is_none() {
        return Err(invalid("Flatpak working directory is not a host path"));
    }
    if !Path::new("/usr/bin/flatpak-spawn").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Flatpak runtime does not provide /usr/bin/flatpak-spawn",
        ));
    }
    let mut directory_argument = std::ffi::OsString::from("--directory=");
    directory_argument.push(&cwd);
    let mut command = Command::new("/usr/bin/flatpak-spawn");
    command
        .arg("--host")
        .arg("--watch-bus")
        .arg(directory_argument)
        .arg(format!("--env={HOST_IDENTITY_ENV}={identity}"));
    // HostCommand starts from the Flatpak session helper, not this launcher.
    // Carry only state paths and terminal presentation, especially the private
    // XDG directories set by the installed-package smoke. Do not forward the
    // sandbox's PATH or loader variables to host processes.
    for key in [
        "HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
        "TMPDIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            let value = value
                .to_str()
                .ok_or_else(|| invalid("Flatpak state path is not UTF-8"))?;
            if !Path::new(value).is_absolute() || value.len() > 4096 {
                return Err(invalid("Flatpak state path is invalid"));
            }
            command.arg(format!("--env={key}={value}"));
        }
    }
    for key in ["TERM", "COLORTERM", "LANG", "LC_ALL"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value
                .to_str()
                .ok_or_else(|| invalid("Flatpak terminal setting is not UTF-8"))?;
            if value.len() > 128 || value.chars().any(char::is_control) {
                return Err(invalid("Flatpak terminal setting is invalid"));
            }
            command.arg(format!("--env={key}={value}"));
        }
    }
    let status = command
        .arg(host_executable)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(Some(status))
}

#[cfg(not(target_os = "linux"))]
pub fn maybe_handoff() -> io::Result<Option<ExitStatus>> {
    Ok(None)
}

#[cfg(target_os = "linux")]
fn read_limited(mut file: File, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("Flatpak metadata or executable exceeds its limit"));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn hash_executable(path: &Path) -> io::Result<String> {
    if path != Path::new("/proc/self/exe") && !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(invalid("Flatpak payload executable is not a regular file"));
    }
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("Flatpak payload executable is not a file"));
    }
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| invalid("Flatpak executable size overflow"))?;
        if bytes > MAX_EXECUTABLE_BYTES {
            return Err(invalid("Flatpak executable exceeds its limit"));
        }
        hasher.update(&chunk[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(target_os = "linux")]
fn verify_host_identity(expected: &str) -> io::Result<()> {
    let digests: Vec<&str> = expected.split(':').collect();
    if digests.len() != 3
        || digests.iter().any(|digest| {
            digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err(invalid("invalid Flatpak host identity marker"));
    }
    let executable = std::env::current_exe()?;
    if executable.file_name().is_none_or(|name| name != "ilium") {
        return Err(invalid(
            "Flatpak host command is not the installed Ilium CLI",
        ));
    }
    let sibling = executable
        .parent()
        .ok_or_else(|| invalid("Flatpak host payload directory is absent"))?;
    let actual = [
        hash_executable(Path::new("/proc/self/exe"))?,
        hash_executable(&sibling.join("ilium-server"))?,
        hash_executable(&sibling.join("ilium-animation-helper"))?,
    ];
    if actual
        .iter()
        .zip(digests)
        .any(|(actual, expected)| actual != expected)
    {
        return Err(invalid(
            "Flatpak host payload differs from the running deployment",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn running_app_path(info: File) -> io::Result<PathBuf> {
    let data = read_limited(info, MAX_INFO_BYTES)?;
    let contents = std::str::from_utf8(&data)
        .map_err(|_| invalid("Flatpak instance metadata is not UTF-8"))?;
    parse_running_app_path(contents)
}

#[cfg(target_os = "linux")]
fn parse_running_app_path(contents: &str) -> io::Result<PathBuf> {
    let mut section = "";
    let mut app_id = None;
    let mut app_path = None;
    let mut app_commit = None;
    let mut branch = None;
    let mut original_app_path = None;
    for raw_line in contents.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']'))
        {
            section = name;
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("malformed Flatpak instance metadata"))?;
        let slot = match (section, key) {
            ("Application", "name") => &mut app_id,
            ("Instance", "app-path") => &mut app_path,
            ("Instance", "app-commit") => &mut app_commit,
            ("Instance", "branch") => &mut branch,
            ("Instance", "original-app-path") => &mut original_app_path,
            _ => continue,
        };
        if slot.replace(value).is_some() {
            return Err(invalid("duplicate Flatpak instance identity field"));
        }
    }
    if app_id != Some(EXPECTED_APP_ID) || branch != Some("master") {
        return Err(invalid(
            "Flatpak instance does not match the Ilium release ref",
        ));
    }
    if original_app_path.is_some() {
        return Err(invalid(
            "Flatpak app-path override cannot be a release payload",
        ));
    }
    let commit = app_commit.ok_or_else(|| invalid("Flatpak running commit is absent"))?;
    if commit.len() != 64 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Flatpak running commit is invalid"));
    }
    let path =
        PathBuf::from(app_path.ok_or_else(|| invalid("Flatpak running app path is absent"))?);
    if path == Path::new("/")
        || !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(invalid("Flatpak running app path is invalid"));
    }
    Ok(path)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_running_app_path;
    use std::path::Path;

    fn info(app_path: &str) -> String {
        format!("[Application]\nname=io.github.arthurwolf.Ilium\n[Instance]\nbranch=master\napp-commit={}\napp-path={app_path}\n", "a".repeat(64))
    }

    #[test]
    fn instance_path_names_exact_running_deployment() {
        let value =
            info("/home/user/private-flatpak/app/io.github.arthurwolf.Ilium/current/active/files");
        let parsed = parse_running_app_path(&value).expect("valid running deployment");
        assert_eq!(
            parsed,
            Path::new(
                "/home/user/private-flatpak/app/io.github.arthurwolf.Ilium/current/active/files"
            )
        );
    }

    #[test]
    fn reject_ambiguous_or_overridden_deployments() {
        let duplicate = format!("{}app-path=/another/files\n", info("/first/files"));
        assert!(parse_running_app_path(&duplicate).is_err());
        let overridden = format!(
            "{}original-app-path=/original/files\n",
            info("/override/files")
        );
        assert!(parse_running_app_path(&overridden).is_err());
        assert!(parse_running_app_path(&info("/first/../other/files")).is_err());
        assert!(parse_running_app_path(&info("relative/files")).is_err());
        assert!(parse_running_app_path(&info("")).is_err());
    }
}
