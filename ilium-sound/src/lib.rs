//! Cross-platform sound discovery, event selection, and playback for ilium.
//!
//! This crate is the narrow operating-system adapter shared by the client
//! settings screen and the detached server. Discovery is read-only and pure
//! from the caller's perspective: it searches the common sound-theme folders
//! that actually exist on the current platform and returns frozen paths. The
//! server owns playback because it also owns agent-state transitions and must
//! continue alerting while no TUI client is attached.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ilium_core::{AgentActivity, PaneStatus};
use serde::{Deserialize, Serialize};

/// Upper bound protecting startup from an unexpectedly enormous mounted
/// sound tree while still comfortably covering normal desktop installations.
const MAX_DISCOVERED_SOUNDS: usize = 4_096;
/// Sound themes are shallow in practice. This avoids recursively exploring a
/// misplaced or cyclic mount even though directory symlinks are not followed.
const MAX_DISCOVERY_DEPTH: usize = 8;
const PLAYBACK_TIMEOUT: Duration = Duration::from_secs(15);

/// Which playback source the user selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundSourceKind {
    #[default]
    SystemBeep,
    SoundFile,
}

impl SoundSourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SystemBeep => "System beep",
            Self::SoundFile => "Sound file",
        }
    }
}

/// User-selectable events that can independently trigger the configured
/// sound. These are semantic transitions, not raw polling states, so a pane
/// remaining idle across many detection ticks never repeats an alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    AgentFinished,
    ApprovalRequired,
    AgentStarted,
    WaitingBackground,
}

impl SoundEvent {
    pub const ALL: [Self; 4] = [
        Self::AgentFinished,
        Self::ApprovalRequired,
        Self::AgentStarted,
        Self::WaitingBackground,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AgentFinished => "Agent finished",
            Self::ApprovalRequired => "Agent needs approval",
            Self::AgentStarted => "Agent started working",
            Self::WaitingBackground => "Agent is waiting for background work",
        }
    }
}

/// Per-event checkboxes persisted under `[sound.events]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundEventSettings {
    pub agent_finished: bool,
    pub approval_required: bool,
    pub agent_started: bool,
    pub waiting_background: bool,
}

impl Default for SoundEventSettings {
    fn default() -> Self {
        Self {
            agent_finished: true,
            approval_required: false,
            agent_started: false,
            waiting_background: false,
        }
    }
}

impl SoundEventSettings {
    pub const fn is_enabled(self, event: SoundEvent) -> bool {
        match event {
            SoundEvent::AgentFinished => self.agent_finished,
            SoundEvent::ApprovalRequired => self.approval_required,
            SoundEvent::AgentStarted => self.agent_started,
            SoundEvent::WaitingBackground => self.waiting_background,
        }
    }

    pub fn toggle(&mut self, event: SoundEvent) {
        match event {
            SoundEvent::AgentFinished => self.agent_finished = !self.agent_finished,
            SoundEvent::ApprovalRequired => {
                self.approval_required = !self.approval_required;
            }
            SoundEvent::AgentStarted => self.agent_started = !self.agent_started,
            SoundEvent::WaitingBackground => {
                self.waiting_background = !self.waiting_background;
            }
        }
    }
}

/// The complete, transport-safe `[sound]` configuration shared by the client
/// and server. A missing `file` while `source == SoundFile` is a valid
/// temporarily-unconfigured state: the UI can show that no system sound was
/// found and playback simply returns a useful error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub source: SoundSourceKind,
    pub file: Option<PathBuf>,
    pub events: SoundEventSettings,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            source: SoundSourceKind::SystemBeep,
            file: None,
            events: SoundEventSettings::default(),
        }
    }
}

/// One common sound folder that exists on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundDirectory {
    pub path: PathBuf,
    pub origin: String,
}

/// One playable file found beneath a [`SoundDirectory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSound {
    pub path: PathBuf,
    pub display_name: String,
    pub collection: String,
}

/// Full discovery evidence shown by the settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundDiscovery {
    pub platform: &'static str,
    pub directories: Vec<SoundDirectory>,
    pub sounds: Vec<SystemSound>,
    pub was_truncated: bool,
}

impl Default for SoundDiscovery {
    fn default() -> Self {
        Self {
            platform: platform_label(),
            directories: Vec::new(),
            sounds: Vec::new(),
            was_truncated: false,
        }
    }
}

/// Playback failures never affect detection or server lifetime; callers log
/// or surface them while the agent transition itself remains authoritative.
#[derive(Debug, thiserror::Error)]
pub enum SoundError {
    #[error("no sound file is selected")]
    MissingSelectedFile,
    #[error("selected sound file does not exist: {0}")]
    MissingFile(PathBuf),
    #[error("no supported sound player was found on this system")]
    NoPlaybackBackend,
    #[error("failed to launch {program}: {source}")]
    Launch {
        program: String,
        source: std::io::Error,
    },
    #[error("{program} exited unsuccessfully ({status})")]
    Unsuccessful { program: String, status: ExitStatus },
    #[error("{program} did not finish within {PLAYBACK_TIMEOUT:?}")]
    TimedOut { program: String },
}

/// Maps a real status change to at most one semantic sound event.
///
/// `None -> Working` is deliberately silent: it is merely the first poll of
/// an already-running agent, not evidence that a new turn just started.
pub fn event_for_transition(previous: Option<&PaneStatus>, new: &PaneStatus) -> Option<SoundEvent> {
    let previous = previous?;

    if matches!(
        previous,
        PaneStatus::Agent(_, AgentActivity::Working | AgentActivity::WaitingBackground)
    ) && matches!(
        new,
        PaneStatus::Agent(_, AgentActivity::Idle | AgentActivity::Done)
    ) {
        return Some(SoundEvent::AgentFinished);
    }

    if !matches!(
        previous,
        PaneStatus::Agent(_, AgentActivity::WaitingApproval)
    ) && matches!(new, PaneStatus::Agent(_, AgentActivity::WaitingApproval))
    {
        return Some(SoundEvent::ApprovalRequired);
    }

    if matches!(
        previous,
        PaneStatus::Agent(
            _,
            AgentActivity::Idle | AgentActivity::Done | AgentActivity::WaitingApproval
        )
    ) && matches!(new, PaneStatus::Agent(_, AgentActivity::Working))
    {
        return Some(SoundEvent::AgentStarted);
    }

    if !matches!(
        previous,
        PaneStatus::Agent(_, AgentActivity::WaitingBackground)
    ) && matches!(new, PaneStatus::Agent(_, AgentActivity::WaitingBackground))
    {
        return Some(SoundEvent::WaitingBackground);
    }

    None
}

/// Finds existing platform-appropriate system sound folders and playable
/// files. Missing folders are expected and omitted, so the UI presents only
/// options proven to exist on the current machine.
pub fn discover_system_sounds() -> SoundDiscovery {
    let directories = existing_sound_directories();
    let mut sounds = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut was_truncated = false;

    for directory in &directories {
        collect_sounds(
            directory,
            &directory.path,
            0,
            &mut sounds,
            &mut seen_paths,
            &mut was_truncated,
        );
        if was_truncated {
            break;
        }
    }

    sounds.sort_by(|left, right| {
        left.collection
            .to_lowercase()
            .cmp(&right.collection.to_lowercase())
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| left.path.cmp(&right.path))
    });

    SoundDiscovery {
        platform: platform_label(),
        directories,
        sounds,
        was_truncated,
    }
}

/// Plays one configured source synchronously. Server call sites move this
/// blocking process wait to `tokio::task::spawn_blocking`.
pub fn play(settings: &SoundSettings) -> Result<(), SoundError> {
    match settings.source {
        SoundSourceKind::SystemBeep => play_system_beep(),
        SoundSourceKind::SoundFile => {
            let path = settings
                .file
                .as_deref()
                .ok_or(SoundError::MissingSelectedFile)?;
            play_file(path)
        }
    }
}

fn collect_sounds(
    directory: &SoundDirectory,
    current: &Path,
    depth: usize,
    sounds: &mut Vec<SystemSound>,
    seen_paths: &mut HashSet<PathBuf>,
    was_truncated: &mut bool,
) {
    if *was_truncated || depth > MAX_DISCOVERY_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        if sounds.len() >= MAX_DISCOVERED_SOUNDS {
            *was_truncated = true;
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_sounds(
                directory,
                &path,
                depth + 1,
                sounds,
                seen_paths,
                was_truncated,
            );
            continue;
        }
        if !file_type.is_file() || !is_supported_sound_file(&path) {
            continue;
        }

        let identity = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen_paths.insert(identity) {
            continue;
        }
        let relative = path.strip_prefix(&directory.path).unwrap_or(&path);
        sounds.push(SystemSound {
            display_name: relative
                .with_extension("")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, " / "),
            collection: directory.origin.clone(),
            path,
        });
    }
}

fn is_supported_sound_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    #[cfg(target_os = "windows")]
    return extension == "wav";

    #[cfg(not(target_os = "windows"))]
    matches!(
        extension.as_str(),
        "wav" | "oga" | "ogg" | "mp3" | "flac" | "aiff" | "aif" | "m4a"
    )
}

fn existing_sound_directories() -> Vec<SoundDirectory> {
    let mut candidates = platform_sound_directories();
    let mut seen = HashSet::new();
    candidates.retain(|candidate| candidate.path.is_dir() && seen.insert(candidate.path.clone()));
    candidates
}

#[cfg(target_os = "linux")]
fn platform_sound_directories() -> Vec<SoundDirectory> {
    let mut directories = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        directories.push(sound_directory(
            PathBuf::from(data_home).join("sounds"),
            "User sounds",
        ));
    } else if let Some(home) = std::env::var_os("HOME") {
        directories.push(sound_directory(
            PathBuf::from(&home).join(".local/share/sounds"),
            "User sounds",
        ));
    }

    let xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .unwrap_or_else(|| OsStr::new("/usr/local/share:/usr/share").to_os_string());
    for base in std::env::split_paths(&xdg_data_dirs) {
        directories.push(sound_directory(base.join("sounds"), "XDG sound themes"));
    }

    directories.extend([
        sound_directory("/usr/share/gnome/sounds", "GNOME sounds"),
        sound_directory("/usr/share/kde4/apps/kdeui/sounds", "KDE sounds"),
        sound_directory("/usr/share/plasma/desktoptheme", "Plasma sounds"),
        sound_directory("/usr/share/ubuntu/sounds", "Ubuntu sounds"),
        sound_directory("/usr/share/mint-artwork/sounds", "Linux Mint sounds"),
        sound_directory("/usr/share/elementary/sounds", "elementary OS sounds"),
        sound_directory(
            "/usr/share/deepin/deepin-sound-theme/stereo",
            "Deepin sounds",
        ),
    ]);
    directories
}

#[cfg(target_os = "macos")]
fn platform_sound_directories() -> Vec<SoundDirectory> {
    let mut directories = vec![
        sound_directory("/System/Library/Sounds", "macOS system sounds"),
        sound_directory("/Library/Sounds", "Shared macOS sounds"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        directories.push(sound_directory(
            PathBuf::from(home).join("Library/Sounds"),
            "User sounds",
        ));
    }
    directories
}

#[cfg(target_os = "windows")]
fn platform_sound_directories() -> Vec<SoundDirectory> {
    let mut directories = Vec::new();
    if let Some(windows) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
        directories.push(sound_directory(
            PathBuf::from(windows).join("Media"),
            "Windows system sounds",
        ));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        directories.push(sound_directory(
            PathBuf::from(&local_app_data).join("Microsoft/Windows/Sounds"),
            "User Windows sounds",
        ));
        directories.push(sound_directory(
            PathBuf::from(local_app_data).join("Microsoft/Windows/Themes"),
            "Local Windows themes",
        ));
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        directories.push(sound_directory(
            PathBuf::from(app_data).join("Microsoft/Windows/Themes"),
            "Roaming Windows themes",
        ));
    }
    directories
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_sound_directories() -> Vec<SoundDirectory> {
    Vec::new()
}

fn sound_directory(path: impl Into<PathBuf>, origin: &str) -> SoundDirectory {
    SoundDirectory {
        path: path.into(),
        origin: origin.to_string(),
    }
}

const fn platform_label() -> &'static str {
    #[cfg(target_os = "linux")]
    return "Linux";
    #[cfg(target_os = "macos")]
    return "macOS";
    #[cfg(target_os = "windows")]
    return "Windows";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return std::env::consts::OS;
}

#[cfg(target_os = "linux")]
fn play_system_beep() -> Result<(), SoundError> {
    run_first_available(&[
        CommandSpec::new("canberra-gtk-play", &["--id", "bell"]),
        CommandSpec::new("beep", &[]),
    ])
}

#[cfg(target_os = "macos")]
fn play_system_beep() -> Result<(), SoundError> {
    run_first_available(&[CommandSpec::new("/usr/bin/osascript", &["-e", "beep"])])
}

#[cfg(target_os = "windows")]
fn play_system_beep() -> Result<(), SoundError> {
    run_first_available(&[CommandSpec::new(
        "rundll32.exe",
        &["user32.dll,MessageBeep"],
    )])
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn play_system_beep() -> Result<(), SoundError> {
    Err(SoundError::NoPlaybackBackend)
}

#[cfg(target_os = "linux")]
fn play_file(path: &Path) -> Result<(), SoundError> {
    ensure_file_exists(path)?;
    let path_argument = path.as_os_str();
    let mut commands = vec![
        OwnedCommandSpec::new("pw-play", vec![path_argument]),
        OwnedCommandSpec::new("paplay", vec![path_argument]),
        OwnedCommandSpec::new(
            "ffplay",
            vec![
                OsStr::new("-nodisp"),
                OsStr::new("-autoexit"),
                OsStr::new("-loglevel"),
                OsStr::new("quiet"),
                path_argument,
            ],
        ),
        OwnedCommandSpec::new(
            "mpv",
            vec![
                OsStr::new("--no-video"),
                OsStr::new("--really-quiet"),
                path_argument,
            ],
        ),
    ];
    if path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        commands.push(OwnedCommandSpec::new(
            "aplay",
            vec![OsStr::new("-q"), path_argument],
        ));
    }
    run_first_available_owned(&commands)
}

#[cfg(target_os = "macos")]
fn play_file(path: &Path) -> Result<(), SoundError> {
    ensure_file_exists(path)?;
    run_command("/usr/bin/afplay", [path.as_os_str()])
}

#[cfg(target_os = "windows")]
fn play_file(path: &Path) -> Result<(), SoundError> {
    ensure_file_exists(path)?;
    run_command(
        "powershell.exe",
        [
            OsStr::new("-NoProfile"),
            OsStr::new("-NonInteractive"),
            OsStr::new("-Command"),
            OsStr::new(
                "$player = New-Object System.Media.SoundPlayer $args[0]; $player.PlaySync()",
            ),
            path.as_os_str(),
        ],
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn play_file(path: &Path) -> Result<(), SoundError> {
    ensure_file_exists(path)?;
    Err(SoundError::NoPlaybackBackend)
}

fn ensure_file_exists(path: &Path) -> Result<(), SoundError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(SoundError::MissingFile(path.to_path_buf()))
    }
}

struct CommandSpec<'a> {
    program: &'a str,
    arguments: &'a [&'a str],
}

impl<'a> CommandSpec<'a> {
    const fn new(program: &'a str, arguments: &'a [&'a str]) -> Self {
        Self { program, arguments }
    }
}

struct OwnedCommandSpec<'a> {
    program: &'a str,
    arguments: Vec<&'a OsStr>,
}

impl<'a> OwnedCommandSpec<'a> {
    fn new(program: &'a str, arguments: Vec<&'a OsStr>) -> Self {
        Self { program, arguments }
    }
}

fn run_first_available(commands: &[CommandSpec<'_>]) -> Result<(), SoundError> {
    let mut last_error = None;
    for command in commands {
        match run_command(command.program, command.arguments.iter().map(OsStr::new)) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or(SoundError::NoPlaybackBackend))
}

fn run_first_available_owned(commands: &[OwnedCommandSpec<'_>]) -> Result<(), SoundError> {
    let mut last_error = None;
    for command in commands {
        match run_command(command.program, command.arguments.iter().copied()) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or(SoundError::NoPlaybackBackend))
}

fn run_command<I, S>(program: &str, arguments: I) -> Result<(), SoundError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| SoundError::Launch {
            program: program.to_string(),
            source,
        })?;
    let deadline = Instant::now() + PLAYBACK_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SoundError::TimedOut {
                    program: program.to_string(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(source) => {
                // `try_wait` failed but the child may still be alive and
                // unreaped; `Child` does not wait-on-drop, so an early
                // return here without reaping would leak a zombie process.
                let _ = child.kill();
                let _ = child.wait();
                return Err(SoundError::Launch {
                    program: program.to_string(),
                    source,
                });
            }
        }
    };
    if status.success() {
        Ok(())
    } else {
        Err(SoundError::Unsuccessful {
            program: program.to_string(),
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::AgentClass;

    fn status(activity: AgentActivity) -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, activity)
    }

    #[test]
    fn defaults_alert_only_when_an_agent_finishes() {
        let settings = SoundSettings::default();
        assert!(settings.events.agent_finished);
        assert!(!settings.events.approval_required);
        assert!(!settings.events.agent_started);
        assert!(!settings.events.waiting_background);
    }

    #[test]
    fn maps_each_supported_transition_once() {
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::Working)),
                &status(AgentActivity::Done)
            ),
            Some(SoundEvent::AgentFinished)
        );
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::Working)),
                &status(AgentActivity::WaitingApproval)
            ),
            Some(SoundEvent::ApprovalRequired)
        );
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::Idle)),
                &status(AgentActivity::Working)
            ),
            Some(SoundEvent::AgentStarted)
        );
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::Working)),
                &status(AgentActivity::WaitingBackground)
            ),
            Some(SoundEvent::WaitingBackground)
        );
    }

    #[test]
    fn ignores_first_poll_and_stable_states() {
        assert_eq!(
            event_for_transition(None, &status(AgentActivity::Done)),
            None
        );
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::Done)),
                &status(AgentActivity::Done)
            ),
            None
        );
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::WaitingApproval)),
                &status(AgentActivity::WaitingApproval)
            ),
            None
        );
    }

    #[test]
    fn discovery_returns_only_existing_supported_files() {
        let discovery = discover_system_sounds();
        assert!(discovery
            .directories
            .iter()
            .all(|entry| entry.path.is_dir()));
        assert!(discovery.sounds.iter().all(|sound| sound.path.is_file()));
        assert!(discovery
            .sounds
            .iter()
            .all(|sound| is_supported_sound_file(&sound.path)));
    }

    #[test]
    fn missing_selected_file_is_a_typed_error() {
        let settings = SoundSettings {
            source: SoundSourceKind::SoundFile,
            file: None,
            ..SoundSettings::default()
        };
        assert!(matches!(
            play(&settings),
            Err(SoundError::MissingSelectedFile)
        ));
    }
}
