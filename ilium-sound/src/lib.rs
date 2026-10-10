//! Cross-platform sound discovery, event selection, and playback for ilium.
//!
//! This crate is the narrow operating-system adapter shared by the client
//! settings screen and the detached server. Discovery is read-only and pure
//! from the caller's perspective: it searches the common sound-theme folders
//! that actually exist on the current platform and returns frozen paths. The
//! server owns playback because it also owns agent-state transitions and must
//! continue alerting while no TUI client is attached.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ilium_core::{NowSignal, PaneSignals, PaneStatus};
use serde::{Deserialize, Serialize};

pub mod synthesis;
pub use synthesis::{
    render_pcm, render_wav, try_waveform_preview, waveform_preview, SoundDesign, Waveform,
    WaveformColumn,
};

/// The attributed GNOME chirping sample is embedded so detached playback
/// works without a source checkout or a system sound package.
pub const BUNDLED_CHIRPING_WAV: &[u8] = include_bytes!("../assets/chirping.wav");

/// Upper bound protecting startup from an unexpectedly enormous mounted
/// sound tree while still comfortably covering normal desktop installations.
const MAX_DISCOVERED_SOUNDS: usize = 4_096;
/// Sound themes are shallow in practice. This avoids recursively exploring a
/// misplaced or cyclic mount even though directory symlinks are not followed.
const MAX_DISCOVERY_DEPTH: usize = 8;
/// Count every filesystem entry, including unsupported files and directories.
/// The playable-file limit alone cannot bound traversal of an empty theme.
const MAX_DISCOVERY_ENTRIES: usize = 65_536;
/// Retained dynamic text, including canonical identities used for deduplication.
/// Container slots remain separately bounded by MAX_DISCOVERED_SOUNDS.
const MAX_DISCOVERY_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOUND_ROOTS: usize = 64;
const MAX_SOUND_ROOT_TEXT_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct SoundRoots {
    directories: Vec<SoundDirectory>,
    text_bytes: usize,
    was_truncated: bool,
}

impl SoundRoots {
    fn push(&mut self, directory: SoundDirectory) -> bool {
        let bytes = directory
            .path
            .capacity()
            .checked_add(directory.origin.capacity())
            .and_then(|bytes| self.text_bytes.checked_add(bytes));
        let Some(bytes) = bytes.filter(|bytes| *bytes <= MAX_SOUND_ROOT_TEXT_BYTES) else {
            self.was_truncated = true;
            return false;
        };
        if self.directories.len() >= MAX_SOUND_ROOTS {
            self.was_truncated = true;
            return false;
        }
        self.text_bytes = bytes;
        self.directories.push(directory);
        true
    }
}

struct DiscoveryTraversal {
    remaining_entries: usize,
    remaining_text_bytes: usize,
    was_truncated: bool,
}

impl DiscoveryTraversal {
    fn visit(&mut self) -> bool {
        if self.remaining_entries == 0 {
            self.was_truncated = true;
            return false;
        }
        self.remaining_entries -= 1;
        true
    }

    fn retain_text(&mut self, bytes: Option<usize>) -> bool {
        let remaining = bytes.and_then(|bytes| self.remaining_text_bytes.checked_sub(bytes));
        let Some(remaining) = remaining else {
            self.was_truncated = true;
            return false;
        };
        self.remaining_text_bytes = remaining;
        true
    }
}
const PLAYBACK_TIMEOUT: Duration = Duration::from_secs(15);

/// Which playback source the user selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundSourceKind {
    #[default]
    SystemBeep,
    SoundFile,
    BundledChirping,
    Generated,
    Muted,
}

impl SoundSourceKind {
    /// Settings cycles every source while preserving the old first toggle.
    pub const ALL: [Self; 5] = [
        Self::SystemBeep,
        Self::SoundFile,
        Self::BundledChirping,
        Self::Generated,
        Self::Muted,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SystemBeep => "System beep",
            Self::SoundFile => "Sound file",
            Self::BundledChirping => "Bundled chirping",
            Self::Generated => "Custom sound",
            Self::Muted => "Muted",
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
    TaskSucceeded,
    TaskFailed,
}

impl SoundEvent {
    pub const ALL: [Self; 6] = [
        Self::AgentFinished,
        Self::ApprovalRequired,
        Self::AgentStarted,
        Self::WaitingBackground,
        Self::TaskSucceeded,
        Self::TaskFailed,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AgentFinished => "Agent finished",
            Self::ApprovalRequired => "Agent needs approval",
            Self::AgentStarted => "Agent started working",
            Self::WaitingBackground => "Agent is waiting for background work",
            Self::TaskSucceeded => "Task succeeded",
            Self::TaskFailed => "Task failed or monitor lost",
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
    pub task_succeeded: bool,
    pub task_failed: bool,
}

impl Default for SoundEventSettings {
    fn default() -> Self {
        Self {
            agent_finished: true,
            approval_required: false,
            agent_started: false,
            waiting_background: false,
            // A monitored task finishing while its agent keeps working is
            // routine progress, not something worth interrupting for.
            task_succeeded: false,
            task_failed: true,
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
            SoundEvent::TaskSucceeded => self.task_succeeded,
            SoundEvent::TaskFailed => self.task_failed,
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
            SoundEvent::TaskSucceeded => self.task_succeeded = !self.task_succeeded,
            SoundEvent::TaskFailed => self.task_failed = !self.task_failed,
        }
    }
}

/// User-selectable events that can independently raise a desktop
/// notification. Names match [`SoundEvent`] so Sound and Notifications read as
/// two outputs of one event vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationEvent {
    AgentFinished,
    ApprovalRequired,
    TaskSucceeded,
    TaskFailed,
}

impl NotificationEvent {
    pub const ALL: [Self; 4] = [
        Self::AgentFinished,
        Self::ApprovalRequired,
        Self::TaskSucceeded,
        Self::TaskFailed,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AgentFinished => "Agent finished",
            Self::ApprovalRequired => "Agent needs approval",
            Self::TaskSucceeded => "Task succeeded",
            Self::TaskFailed => "Task failed or monitor lost",
        }
    }
}

/// The `[notifications]` table. `enabled` is the master switch; every event
/// is additionally gated by its own flag. Flat keys keep hand-written config
/// simple (`task_succeeded = false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub agent_finished: bool,
    pub approval_required: bool,
    pub task_succeeded: bool,
    pub task_failed: bool,
    /// Suppress task-outcome notifications and sounds while the agent in the
    /// pane is idle or parked: it is about to receive the result and will
    /// raise its own "agent finished" alert, so the outcome is redundant.
    pub suppress_redundant_task_outcomes: bool,
    /// Task-outcome alerts of the same kind on the same pane closer together
    /// than this are collapsed into the first. Zero disables coalescing.
    pub task_coalesce_seconds: u32,
}

impl NotificationSettings {
    pub const MAX_TASK_COALESCE_SECONDS: u32 = 600;
    pub const TASK_COALESCE_STEP_SECONDS: u32 = 10;

    pub const fn is_enabled(self, event: NotificationEvent) -> bool {
        self.enabled
            && match event {
                NotificationEvent::AgentFinished => self.agent_finished,
                NotificationEvent::ApprovalRequired => self.approval_required,
                NotificationEvent::TaskSucceeded => self.task_succeeded,
                NotificationEvent::TaskFailed => self.task_failed,
            }
    }

    /// The event's own flag, ignoring the master switch (for Settings rows).
    pub const fn event_flag(self, event: NotificationEvent) -> bool {
        match event {
            NotificationEvent::AgentFinished => self.agent_finished,
            NotificationEvent::ApprovalRequired => self.approval_required,
            NotificationEvent::TaskSucceeded => self.task_succeeded,
            NotificationEvent::TaskFailed => self.task_failed,
        }
    }

    pub fn toggle(&mut self, event: NotificationEvent) {
        let flag = match event {
            NotificationEvent::AgentFinished => &mut self.agent_finished,
            NotificationEvent::ApprovalRequired => &mut self.approval_required,
            NotificationEvent::TaskSucceeded => &mut self.task_succeeded,
            NotificationEvent::TaskFailed => &mut self.task_failed,
        };
        *flag = !*flag;
    }

    /// Clamps hand-edited values into their supported range.
    pub fn normalized(mut self) -> Self {
        self.task_coalesce_seconds = self
            .task_coalesce_seconds
            .min(Self::MAX_TASK_COALESCE_SECONDS);
        self
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_finished: true,
            approval_required: true,
            task_succeeded: false,
            task_failed: true,
            suppress_redundant_task_outcomes: true,
            task_coalesce_seconds: 30,
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
    /// Kept when another source is selected so reopening the studio restores
    /// the authored design. Generated playback normalizes it before rendering.
    pub design: SoundDesign,
    pub events: SoundEventSettings,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            source: SoundSourceKind::SystemBeep,
            file: None,
            design: SoundDesign::default(),
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
    #[error("could not prepare private temporary WAV for playback: {0}")]
    TemporaryWav(std::io::Error),
    #[error("failed to launch {program}: {source}")]
    Launch {
        program: String,
        source: std::io::Error,
    },
    #[error("failed while waiting for {program} to finish: {source}")]
    WaitFailed {
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
    let previous =
        previous.map(|status| ilium_core::project_pane_signals(status, &[], false, None));
    let new = ilium_core::project_pane_signals(new, &[], false, None);
    event_for_signals(previous.as_ref(), &new)
}

/// Selects an alert from the shared tree projection so sound transitions
/// follow the same precedence and attention state as every client's icons.
pub fn event_for_signals(previous: Option<&PaneSignals>, new: &PaneSignals) -> Option<SoundEvent> {
    let previous = previous.map(|signals| signals.now);
    let new = new.now;

    if matches!(
        previous,
        Some(NowSignal::Working | NowSignal::WaitingSubagents | NowSignal::Settling)
    ) && new == NowSignal::FinishedUnread
    {
        return Some(SoundEvent::AgentFinished);
    }

    if previous != Some(NowSignal::NeedsApproval) && new == NowSignal::NeedsApproval {
        return Some(SoundEvent::ApprovalRequired);
    }

    if matches!(
        previous,
        Some(
            NowSignal::Idle
                | NowSignal::NeedsApproval
                | NowSignal::Parked
                | NowSignal::FinishedUnread
        )
    ) && new == NowSignal::Working
    {
        return Some(SoundEvent::AgentStarted);
    }

    // `BackgroundTaskStillRunning` reuses the same sound event as
    // `WaitingBackground` rather than getting its own -- both mean "the
    // agent is now blocked on something running in the background" from a
    // notification-sound perspective, and moving directly between the two
    // (e.g. dispatched subagents finish just as a leftover shell is still
    // reported running) must not refire the chime.
    if !matches!(
        previous,
        Some(NowSignal::WaitingSubagents | NowSignal::Settling)
    ) && matches!(new, NowSignal::WaitingSubagents | NowSignal::Settling)
    {
        return Some(SoundEvent::WaitingBackground);
    }

    None
}

/// Finds existing platform-appropriate system sound folders and playable
/// files. Missing folders are expected and omitted, so the UI presents only
/// options proven to exist on the current machine.
pub fn discover_system_sounds() -> SoundDiscovery {
    discover_sound_roots(existing_sound_directories())
}

fn discover_sound_roots(roots: SoundRoots) -> SoundDiscovery {
    let directories = roots.directories;
    let mut sounds = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut traversal = DiscoveryTraversal {
        remaining_entries: MAX_DISCOVERY_ENTRIES,
        remaining_text_bytes: MAX_DISCOVERY_TEXT_BYTES,
        was_truncated: false,
    };

    for directory in &directories {
        collect_sounds(
            directory,
            &directory.path,
            0,
            &mut sounds,
            &mut seen_paths,
            &mut traversal,
        );
        if traversal.was_truncated {
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
        was_truncated: roots.was_truncated || traversal.was_truncated,
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
        SoundSourceKind::BundledChirping => play_wav_bytes(BUNDLED_CHIRPING_WAV),
        SoundSourceKind::Generated => play_wav_bytes(&render_wav(&settings.design)),
        SoundSourceKind::Muted => Ok(()),
    }
}

/// A private, short-lived WAV path is needed by the existing platform
/// players, especially Windows SoundPlayer. The temp directory outlives the
/// synchronous player process and is removed after that process exits.
fn play_wav_bytes(bytes: &[u8]) -> Result<(), SoundError> {
    let directory = tempfile::Builder::new()
        .prefix("ilium-sound-")
        .tempdir()
        .map_err(SoundError::TemporaryWav)?;
    let path = directory.path().join("sound.wav");
    std::fs::write(&path, bytes).map_err(SoundError::TemporaryWav)?;
    play_file(&path)
}

/// Plays a previously rendered WAV without performing synthesis on the
/// playback thread. Callers should obtain bytes from [`render_wav`] so the
/// duration and allocation remain within the generated-sound limits.
pub fn play_prepared_wav(bytes: &[u8]) -> Result<(), SoundError> {
    play_wav_bytes(bytes)
}

fn collect_sounds(
    directory: &SoundDirectory,
    current: &Path,
    depth: usize,
    sounds: &mut Vec<SystemSound>,
    seen_paths: &mut HashSet<PathBuf>,
    traversal: &mut DiscoveryTraversal,
) {
    if traversal.was_truncated || depth > MAX_DISCOVERY_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries {
        if !traversal.visit() {
            return;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if sounds.len() >= MAX_DISCOVERED_SOUNDS {
            traversal.was_truncated = true;
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        // Bound recursion paths independently of remaining retained text.
        if path.as_os_str().len() > MAX_DISCOVERY_TEXT_BYTES {
            traversal.was_truncated = true;
            return;
        }
        if file_type.is_dir() {
            collect_sounds(directory, &path, depth + 1, sounds, seen_paths, traversal);
            if traversal.was_truncated {
                return;
            }
            continue;
        }
        // `DirEntry::file_type()` reports the entry itself, not the symlink
        // target, so a symlinked sound file (common in freedesktop/GNOME/
        // Ubuntu/elementary theme packages that alias files this way) must
        // be confirmed with a metadata lookup that *does* follow the link.
        // Directory symlinks are deliberately excluded from that fallback:
        // they are the cyclic/misplaced-mount case `MAX_DISCOVERY_DEPTH`
        // guards against, so only a non-directory symlink target counts.
        let is_playable_file = file_type.is_file() || (file_type.is_symlink() && path.is_file());
        if !is_playable_file || !is_supported_sound_file(&path) {
            continue;
        }

        let identity = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen_paths.contains(&identity) {
            continue;
        }
        // Lossy decoding and separator replacement can each expand the path
        // threefold. Check only new playable identities, so skipped entries
        // and duplicates do not falsely exhaust retained text admission.
        if path
            .as_os_str()
            .len()
            .checked_mul(9)
            .and_then(|bytes| bytes.checked_add(directory.origin.len()))
            .and_then(|bytes| bytes.checked_add(identity.as_os_str().len()))
            .is_none_or(|bytes| bytes > traversal.remaining_text_bytes)
        {
            traversal.was_truncated = true;
            return;
        }
        let relative = path.strip_prefix(&directory.path).unwrap_or(&path);
        let sound = SystemSound {
            display_name: relative
                .with_extension("")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, " / "),
            collection: directory.origin.clone(),
            path,
        };
        let bytes = sound
            .path
            .capacity()
            .checked_add(identity.capacity())
            .and_then(|bytes| bytes.checked_add(sound.display_name.capacity()))
            .and_then(|bytes| bytes.checked_add(sound.collection.capacity()));
        if !traversal.retain_text(bytes) {
            return;
        }
        seen_paths.insert(identity);
        sounds.push(sound);
    }
}

fn is_supported_sound_file(path: &Path) -> bool {
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or_default();

    #[cfg(target_os = "windows")]
    return extension.eq_ignore_ascii_case("wav");

    #[cfg(not(target_os = "windows"))]
    ["wav", "oga", "ogg", "mp3", "flac", "aiff", "aif", "m4a"]
        .iter()
        .any(|supported| extension.eq_ignore_ascii_case(supported))
}

fn existing_sound_directories() -> SoundRoots {
    let mut candidates = platform_sound_directories();
    let mut seen = HashSet::new();
    candidates
        .directories
        .retain(|candidate| candidate.path.is_dir() && seen.insert(candidate.path.clone()));
    candidates
}

#[cfg(target_os = "linux")]
fn collect_xdg_sound_roots(directories: &mut SoundRoots, list: &OsStr) {
    if list.len() > MAX_SOUND_ROOT_TEXT_BYTES {
        directories.was_truncated = true;
        return;
    }
    for base in std::env::split_paths(list).filter(|base| base.is_absolute()) {
        if !directories.push(sound_directory(base.join("sounds"), "XDG sound themes")) {
            break;
        }
    }
}

#[cfg(target_os = "linux")]
fn platform_sound_directories() -> SoundRoots {
    let mut directories = SoundRoots::default();
    // One fall-through chain rather than `if let ... else if let ...`: an
    // `XDG_DATA_HOME` that is set but unusable (empty or relative) must still
    // fall back to the spec's `$HOME/.local/share` default, whereas the
    // `else if` form treats "set" as "usable" and would drop user sounds
    // entirely.
    let user_data_home = absolute_directory(std::env::var_os("XDG_DATA_HOME")).or_else(|| {
        absolute_directory(std::env::var_os("HOME")).map(|home| home.join(".local/share"))
    });
    if let Some(data_home) = user_data_home {
        directories.push(sound_directory(data_home.join("sounds"), "User sounds"));
    }

    let xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsStr::new("/usr/local/share:/usr/share").to_os_string());
    // Every entry of the list must be absolute for the same reason a single
    // directory variable must be, and an empty segment is the realistic case:
    // `XDG_DATA_DIRS="$XDG_DATA_DIRS:/opt/share"` written while the variable
    // was unset yields a leading empty component, and `PathBuf::from("")
    // .join("sounds")` resolves against ilium's working directory -- which
    // would present a checked-out repository's own `sounds/` folder as a
    // legitimate system sound theme.
    collect_xdg_sound_roots(&mut directories, &xdg_data_dirs);

    for directory in [
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
    ] {
        directories.push(directory);
    }
    directories
}

#[cfg(target_os = "macos")]
fn platform_sound_directories() -> SoundRoots {
    let mut directories = SoundRoots::default();
    directories.push(sound_directory(
        "/System/Library/Sounds",
        "macOS system sounds",
    ));
    directories.push(sound_directory("/Library/Sounds", "Shared macOS sounds"));
    if let Some(home) = absolute_directory(std::env::var_os("HOME")) {
        directories.push(sound_directory(home.join("Library/Sounds"), "User sounds"));
    }
    directories
}

#[cfg(target_os = "windows")]
fn platform_sound_directories() -> SoundRoots {
    let mut directories = SoundRoots::default();
    if let Some(windows) = absolute_directory(std::env::var_os("WINDIR"))
        .or_else(|| absolute_directory(std::env::var_os("SystemRoot")))
    {
        directories.push(sound_directory(
            windows.join("Media"),
            "Windows system sounds",
        ));
    }
    if let Some(local_app_data) = absolute_directory(std::env::var_os("LOCALAPPDATA")) {
        directories.push(sound_directory(
            local_app_data.join("Microsoft/Windows/Sounds"),
            "User Windows sounds",
        ));
        directories.push(sound_directory(
            local_app_data.join("Microsoft/Windows/Themes"),
            "Local Windows themes",
        ));
    }
    if let Some(app_data) = absolute_directory(std::env::var_os("APPDATA")) {
        directories.push(sound_directory(
            app_data.join("Microsoft/Windows/Themes"),
            "Roaming Windows themes",
        ));
    }
    directories
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_sound_directories() -> SoundRoots {
    SoundRoots::default()
}

fn sound_directory(path: impl Into<PathBuf>, origin: &str) -> SoundDirectory {
    SoundDirectory {
        path: path.into(),
        origin: origin.to_string(),
    }
}

/// Resolves an environment variable that names exactly one directory,
/// rejecting anything that is not an absolute path.
///
/// This covers both invalid forms the XDG Base Directory spec calls out for
/// `XDG_DATA_HOME`/`XDG_DATA_DIRS` -- set-but-empty (which must be treated as
/// unset) and relative (which "must be ignored") -- and is applied to every
/// platform's discovery variables for the same reason. A relative value,
/// empty included, resolves against the process working directory, which for
/// ilium is the user's project directory; without this check a checked-out
/// repository containing its own `sounds/` (or `Media/`, `Themes/`, ...)
/// folder would be presented as a legitimate system sound theme. An empty
/// `OsString` needs no separate case: `PathBuf::from("")` is not absolute.
fn absolute_directory(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    if path.is_absolute() {
        Some(path)
    } else {
        None
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
    // The configured filename is data, never options. A `[sound] file` path
    // beginning with `-` (hand-editable in `config.toml`) would otherwise be
    // parsed as a flag by every one of these players, so each candidate is
    // given an explicit end-of-options marker: `--` for the getopt/getopt_long
    // parsers (pw-play, paplay, mpv, aplay), and `-i` for ffplay, whose own
    // FFmpeg option parser does not honour `--` but does take an explicit
    // input flag.
    let mut commands = vec![
        OwnedCommandSpec::new("pw-play", vec![OsStr::new("--"), path_argument]),
        OwnedCommandSpec::new("paplay", vec![OsStr::new("--"), path_argument]),
        OwnedCommandSpec::new(
            "ffplay",
            vec![
                OsStr::new("-nodisp"),
                OsStr::new("-autoexit"),
                OsStr::new("-loglevel"),
                OsStr::new("quiet"),
                OsStr::new("-i"),
                path_argument,
            ],
        ),
        OwnedCommandSpec::new(
            "mpv",
            vec![
                OsStr::new("--no-video"),
                OsStr::new("--really-quiet"),
                OsStr::new("--"),
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
            vec![OsStr::new("-q"), OsStr::new("--"), path_argument],
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

    // The path travels through an environment variable, never through the
    // command text: `powershell.exe -Command "<string>" <arg>` appends any
    // trailing argument to the command string and re-parses it as code (it
    // does not populate `$args`), so interpolating the path there both
    // breaks playback outright and executes PowerShell metacharacters
    // (`;`, `$(...)`, quotes) found in a configured filename.
    let program = "powershell.exe";
    let mut command = Command::new(program);
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$player = New-Object System.Media.SoundPlayer $env:ILIUM_SOUND_FILE; \
             $player.PlaySync()",
        ])
        .env("ILIUM_SOUND_FILE", path.as_os_str());
    run_prepared_command(program, command)
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

/// Only Linux needs to try a list of players in turn: macOS and Windows each
/// ship exactly one built-in command, so their `play_file` calls it directly.
#[cfg(target_os = "linux")]
struct OwnedCommandSpec<'a> {
    program: &'a str,
    arguments: Vec<&'a OsStr>,
}

#[cfg(target_os = "linux")]
impl<'a> OwnedCommandSpec<'a> {
    fn new(program: &'a str, arguments: Vec<&'a OsStr>) -> Self {
        Self { program, arguments }
    }
}

fn run_first_available(commands: &[CommandSpec<'_>]) -> Result<(), SoundError> {
    resolve_playback_attempts(
        commands
            .iter()
            .map(|command| run_command(command.program, command.arguments.iter().map(OsStr::new))),
    )
}

#[cfg(target_os = "linux")]
fn run_first_available_owned(commands: &[OwnedCommandSpec<'_>]) -> Result<(), SoundError> {
    resolve_playback_attempts(
        commands
            .iter()
            .map(|command| run_command(command.program, command.arguments.iter().copied())),
    )
}

/// Runs each candidate player attempt in order (the iterator is lazy, so a
/// player later in the list never launches once an earlier one succeeds) and
/// picks the most useful outcome.
///
/// A `Launch` failure whose underlying error is `NotFound` just means that
/// particular binary is not installed -- expected on most systems and not
/// worth reporting. It must never shadow a more diagnostic failure from a
/// player that *did* launch (unsupported codec, no audio device, timed out,
/// ...), so we keep the first such error rather than the last "not found"
/// one, and only fall back to [`SoundError::NoPlaybackBackend`] when every
/// candidate was missing.
fn resolve_playback_attempts(
    attempts: impl Iterator<Item = Result<(), SoundError>>,
) -> Result<(), SoundError> {
    let mut diagnostic_error = None;
    for attempt in attempts {
        match attempt {
            Ok(()) => return Ok(()),
            Err(error) if is_backend_missing(&error) => {}
            Err(error) => {
                diagnostic_error.get_or_insert(error);
            }
        }
    }
    Err(diagnostic_error.unwrap_or(SoundError::NoPlaybackBackend))
}

/// True when a launch failure means the player binary is simply absent
/// (`ErrorKind::NotFound`), as opposed to a permissions error, a launch
/// failure for another reason, or a failure after the process started.
fn is_backend_missing(error: &SoundError) -> bool {
    matches!(
        error,
        SoundError::Launch { source, .. } if source.kind() == std::io::ErrorKind::NotFound
    )
}

fn run_command<I, S>(program: &str, arguments: I) -> Result<(), SoundError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(program);
    command.args(arguments);
    run_prepared_command(program, command)
}

/// Spawns an already-configured player command with quiet stdio, waits for it
/// with the shared playback timeout, and always reaps the child on every exit
/// path so no zombie process is left behind.
fn run_prepared_command(program: &str, mut command: Command) -> Result<(), SoundError> {
    let mut child = command
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
                return Err(SoundError::WaitFailed {
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
    use ilium_core::{AgentActivity, AgentClass};

    fn status(activity: AgentActivity) -> PaneStatus {
        PaneStatus::from_activity(AgentClass::Claude, activity, None)
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
    fn background_task_still_running_is_treated_symmetrically_with_waiting_background() {
        // Entering from a non-background-wait state reuses the WaitingBackground chime.
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::Working)),
                &status(AgentActivity::BackgroundTaskStillRunning)
            ),
            Some(SoundEvent::WaitingBackground)
        );
        // Moving directly between the two background-wait flavors must not refire it.
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::WaitingBackground)),
                &status(AgentActivity::BackgroundTaskStillRunning)
            ),
            None
        );
        // Leaving it for Idle/Done still plays the finished chime.
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::BackgroundTaskStillRunning)),
                &status(AgentActivity::Done)
            ),
            Some(SoundEvent::AgentFinished)
        );
        // Resuming Working from it must not refire AgentStarted -- it was
        // already a busy state, same as WaitingBackground.
        assert_eq!(
            event_for_transition(
                Some(&status(AgentActivity::BackgroundTaskStillRunning)),
                &status(AgentActivity::Working)
            ),
            None
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
    fn discovery_entry_budget_counts_unsupported_files_and_nested_directories() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        for name in ["first.txt", "second.txt", "third.txt"] {
            std::fs::write(nested.join(name), []).unwrap();
        }
        let directory = sound_directory(root.path(), "Fixture");
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 2,
            remaining_text_bytes: MAX_DISCOVERY_TEXT_BYTES,
            was_truncated: false,
        };
        let mut sounds = Vec::new();
        collect_sounds(
            &directory,
            &directory.path,
            0,
            &mut sounds,
            &mut HashSet::new(),
            &mut traversal,
        );
        assert!(sounds.is_empty());
        assert_eq!(traversal.remaining_entries, 0);
        assert!(traversal.was_truncated);
    }

    #[test]
    fn discovery_entry_budget_is_shared_across_roots() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("first.wav"), []).unwrap();
        std::fs::write(second.path().join("second.wav"), []).unwrap();
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 1,
            remaining_text_bytes: MAX_DISCOVERY_TEXT_BYTES,
            was_truncated: false,
        };
        let mut sounds = Vec::new();
        let mut seen_paths = HashSet::new();
        for root in [first.path(), second.path()] {
            let directory = sound_directory(root, "Fixture");
            collect_sounds(
                &directory,
                &directory.path,
                0,
                &mut sounds,
                &mut seen_paths,
                &mut traversal,
            );
        }
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].path, first.path().join("first.wav"));
        assert!(traversal.was_truncated);
    }

    #[test]
    fn discovery_entry_budget_exactly_complete_tree_is_not_truncated() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("only.wav"), []).unwrap();
        let directory = sound_directory(root.path(), "Fixture");
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 1,
            remaining_text_bytes: MAX_DISCOVERY_TEXT_BYTES,
            was_truncated: false,
        };
        let mut sounds = Vec::new();
        collect_sounds(
            &directory,
            &directory.path,
            0,
            &mut sounds,
            &mut HashSet::new(),
            &mut traversal,
        );
        assert_eq!(sounds.len(), 1);
        assert_eq!(traversal.remaining_entries, 0);
        assert!(!traversal.was_truncated);
    }

    #[test]
    fn discovery_text_budget_rejects_before_retaining_sound_or_identity() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sound.wav"), []).unwrap();
        let directory = sound_directory(root.path(), "Fixture");
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 10,
            remaining_text_bytes: 1,
            was_truncated: false,
        };
        let mut sounds = Vec::new();
        let mut identities = HashSet::new();
        collect_sounds(
            &directory,
            &directory.path,
            0,
            &mut sounds,
            &mut identities,
            &mut traversal,
        );
        assert!(sounds.is_empty());
        assert!(identities.is_empty());
        assert_eq!(traversal.remaining_text_bytes, 1);
        assert!(traversal.was_truncated);
    }

    #[test]
    fn discovery_text_budget_charges_actual_capacities_and_not_duplicates() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sound.wav"), []).unwrap();
        let directory = sound_directory(root.path(), "Fixture");
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 10,
            remaining_text_bytes: MAX_DISCOVERY_TEXT_BYTES,
            was_truncated: false,
        };
        let mut sounds = Vec::new();
        let mut identities = HashSet::new();
        for _ in 0..2 {
            collect_sounds(
                &directory,
                &directory.path,
                0,
                &mut sounds,
                &mut identities,
                &mut traversal,
            );
        }
        assert_eq!(sounds.len(), 1);
        assert_eq!(identities.len(), 1);
        let sound = &sounds[0];
        let retained = sound.path.capacity()
            + sound.display_name.capacity()
            + sound.collection.capacity()
            + identities.iter().next().unwrap().capacity();
        assert_eq!(
            traversal.remaining_text_bytes,
            MAX_DISCOVERY_TEXT_BYTES - retained
        );
        assert!(!traversal.was_truncated);
    }

    #[test]
    fn discovery_text_exhaustion_does_not_refuse_duplicate_or_unsupported_entries() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sound.WAV"), []).unwrap();
        let directory = sound_directory(root.path(), "Fixture");
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 10,
            remaining_text_bytes: MAX_DISCOVERY_TEXT_BYTES,
            was_truncated: false,
        };
        let mut sounds = Vec::new();
        let mut identities = HashSet::new();
        collect_sounds(
            &directory,
            &directory.path,
            0,
            &mut sounds,
            &mut identities,
            &mut traversal,
        );
        traversal.remaining_text_bytes = 0;
        std::fs::write(root.path().join("unsupported.txt"), []).unwrap();
        collect_sounds(
            &directory,
            &directory.path,
            0,
            &mut sounds,
            &mut identities,
            &mut traversal,
        );
        assert_eq!(sounds.len(), 1);
        assert_eq!(identities.len(), 1);
        assert!(!traversal.was_truncated);
    }

    #[test]
    fn discovery_text_budget_exact_capacity_and_overflow_are_truthful() {
        let mut traversal = DiscoveryTraversal {
            remaining_entries: 1,
            remaining_text_bytes: 5,
            was_truncated: false,
        };
        assert!(traversal.retain_text(Some(5)));
        assert!(!traversal.was_truncated);
        assert!(!traversal.retain_text(None));
        assert!(traversal.was_truncated);
        assert_eq!(traversal.remaining_text_bytes, 0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn sound_roots_xdg_oversized_list_is_refused_without_component_retention() {
        let mut roots = SoundRoots::default();
        assert!(roots.push(sound_directory("/user/sounds", "User sounds")));
        let bytes = roots.text_bytes;
        collect_xdg_sound_roots(
            &mut roots,
            OsStr::new(&"/".repeat(MAX_SOUND_ROOT_TEXT_BYTES + 1)),
        );
        assert!(roots.was_truncated);
        assert_eq!(roots.directories.len(), 1);
        assert_eq!(roots.text_bytes, bytes);
        assert!(roots.push(sound_directory("/builtin/sounds", "Built-in sounds")));
        assert_eq!(roots.directories.len(), 2);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn sound_roots_xdg_preserves_order_and_ignores_invalid_components() {
        let mut roots = SoundRoots::default();
        collect_xdg_sound_roots(&mut roots, OsStr::new("/first:relative::/second"));
        assert!(!roots.was_truncated);
        assert_eq!(roots.directories.len(), 2);
        assert_eq!(roots.directories[0].path, PathBuf::from("/first/sounds"));
        assert_eq!(roots.directories[1].path, PathBuf::from("/second/sounds"));
    }

    #[test]
    fn sound_roots_count_limit_keeps_original_prefix_and_reports_refusal() {
        let mut roots = SoundRoots::default();
        for index in 0..MAX_SOUND_ROOTS {
            assert!(roots.push(sound_directory(format!("/fixture/{index}"), "Fixture")));
        }
        assert!(!roots.was_truncated);
        let bytes = roots.text_bytes;
        assert!(!roots.push(sound_directory("/refused", "Fixture")));
        assert!(roots.was_truncated);
        assert_eq!(roots.directories.len(), MAX_SOUND_ROOTS);
        assert_eq!(roots.directories[0].path, PathBuf::from("/fixture/0"));
        assert_eq!(roots.text_bytes, bytes);
    }

    #[test]
    fn sound_roots_byte_limit_counts_spare_capacity_before_insertion() {
        let mut roots = SoundRoots::default();
        let mut path = PathBuf::with_capacity(MAX_SOUND_ROOT_TEXT_BYTES + 1);
        path.push("/fixture");
        assert!(!roots.push(SoundDirectory {
            path,
            origin: "Fixture".into()
        }));
        assert!(roots.was_truncated);
        assert!(roots.directories.is_empty());
        assert_eq!(roots.text_bytes, 0);
        assert!(roots.push(sound_directory("/small", "Fixture")));
        assert_eq!(roots.directories.len(), 1);
        assert!(roots.was_truncated);
    }

    #[test]
    fn sound_roots_capacity_arithmetic_overflow_refuses_without_losing_prefix() {
        let mut roots = SoundRoots::default();
        assert!(roots.push(sound_directory("/original", "Fixture")));
        roots.text_bytes = usize::MAX;
        assert!(!roots.push(sound_directory("/refused", "Fixture")));
        assert_eq!(roots.directories.len(), 1);
        assert_eq!(roots.directories[0].path, PathBuf::from("/original"));
        assert!(roots.was_truncated);
    }

    #[test]
    fn sound_roots_refusal_still_scans_accepted_roots() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sound.wav");
        std::fs::write(&path, []).unwrap();
        let mut roots = SoundRoots::default();
        assert!(roots.push(sound_directory(root.path(), "Fixture")));
        roots.was_truncated = true;
        let discovery = discover_sound_roots(roots);
        assert!(discovery.was_truncated);
        assert_eq!(discovery.sounds.len(), 1);
        assert_eq!(discovery.sounds[0].path, path);
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
    fn environment_directories_reject_unset_empty_and_relative_values() {
        assert_eq!(absolute_directory(None), None);
        assert_eq!(absolute_directory(Some(OsString::new())), None);
        assert_eq!(
            absolute_directory(Some(OsString::from("relative/share"))),
            None
        );

        // An absolute-path literal is necessarily platform-specific: on
        // Windows a merely rooted path is absolute only once it also carries
        // a drive or UNC prefix.
        #[cfg(windows)]
        let absolute = OsString::from("C:\\ProgramData");
        #[cfg(not(windows))]
        let absolute = OsString::from("/usr/local/share");
        assert_eq!(
            absolute_directory(Some(absolute.clone())),
            Some(PathBuf::from(absolute))
        );
    }

    #[test]
    fn bundled_sample_is_an_embedded_pcm_wave() {
        assert_eq!(&BUNDLED_CHIRPING_WAV[..4], b"RIFF");
        assert_eq!(&BUNDLED_CHIRPING_WAV[8..12], b"WAVE");
        assert_eq!(
            u16::from_le_bytes([BUNDLED_CHIRPING_WAV[20], BUNDLED_CHIRPING_WAV[21]]),
            1
        );
        assert_eq!(
            u16::from_le_bytes([BUNDLED_CHIRPING_WAV[22], BUNDLED_CHIRPING_WAV[23]]),
            2
        );
        assert_eq!(
            u32::from_le_bytes(BUNDLED_CHIRPING_WAV[24..28].try_into().unwrap()),
            44_100
        );
    }

    #[test]
    fn muted_source_has_no_playback_side_effect() {
        let settings = SoundSettings {
            source: SoundSourceKind::Muted,
            ..SoundSettings::default()
        };
        assert!(play(&settings).is_ok());
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

    #[test]
    fn task_success_sound_and_notification_default_off_and_failure_on() {
        assert!(!SoundEventSettings::default().task_succeeded);
        assert!(SoundEventSettings::default().task_failed);
        let notifications = NotificationSettings::default();
        assert!(notifications.is_enabled(NotificationEvent::AgentFinished));
        assert!(notifications.is_enabled(NotificationEvent::ApprovalRequired));
        assert!(!notifications.is_enabled(NotificationEvent::TaskSucceeded));
        assert!(notifications.is_enabled(NotificationEvent::TaskFailed));
        assert!(notifications.suppress_redundant_task_outcomes);
        assert_eq!(notifications.task_coalesce_seconds, 30);
    }

    #[test]
    fn master_notification_switch_overrides_every_event() {
        let mut notifications = NotificationSettings {
            enabled: false,
            ..NotificationSettings::default()
        };
        for event in NotificationEvent::ALL {
            assert!(!notifications.is_enabled(event));
        }
        notifications.enabled = true;
        notifications.toggle(NotificationEvent::TaskSucceeded);
        assert!(notifications.is_enabled(NotificationEvent::TaskSucceeded));
        assert!(notifications.event_flag(NotificationEvent::TaskSucceeded));
    }

    #[test]
    fn notification_coalescing_is_clamped() {
        let notifications = NotificationSettings {
            task_coalesce_seconds: u32::MAX,
            ..NotificationSettings::default()
        }
        .normalized();
        assert_eq!(
            notifications.task_coalesce_seconds,
            NotificationSettings::MAX_TASK_COALESCE_SECONDS
        );
    }
}
