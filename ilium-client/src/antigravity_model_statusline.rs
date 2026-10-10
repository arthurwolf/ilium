//! Small local bridge from Antigravity CLI status-line payloads to the model
//! cache used by the agent tree. The status-line payload contains no prompt
//! text; the bridge retains only project, conversation, and model identity.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ilium_platform::file_lock::ExclusiveFileLock;
use serde::{Deserialize, Serialize};

const MAX_INPUT_BYTES: u64 = 16 * 1024;
const MAX_MODEL_BYTES: usize = 256;
const MAX_SETTINGS_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 64 * 1024;
const STATE_FILE: &str = "ilium-model-statusline-state.json";
const MAX_OBSERVATIONS: usize = 128;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
struct StatuslineInput {
    conversation_id: Option<String>,
    session_id: Option<String>,
    cwd: PathBuf,
    model: ModelInput,
}

#[derive(Deserialize)]
struct ModelInput {
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelObservation {
    conversation_id: String,
    project: PathBuf,
    model: String,
    #[serde(default)]
    generation: String,
}

#[derive(Serialize, Deserialize)]
struct StatuslineState {
    previous: Option<serde_json::Value>,
    previous_raw: Option<String>,
    installed: serde_json::Value,
    #[serde(default)]
    generation: String,
}

/// Returns the command Ilium installed for enabled model capture. The caller
/// must first reconcile the persisted settings so this is the command that
/// new Antigravity sessions will use.
pub fn enabled_runtime_action() -> Result<ilium_ipc::AntigravityStatuslineAction, String> {
    #[cfg(test)]
    {
        return Ok(ilium_ipc::AntigravityStatuslineAction::SetCommand {
            command: "ilium __antigravity-model-statusline".to_owned(),
        });
    }
    #[cfg(not(test))]
    {
        let (state, still_owned) = read_global_state_and_ownership()?.ok_or_else(|| {
            "Antigravity status-line bridge state is missing after enable".to_owned()
        })?;
        if !still_owned {
            return Err(
                "Antigravity statusLine.command changed after model capture was enabled".to_owned(),
            );
        }
        enabled_action(&state)
    }
}

/// Selects the live command to restore before removing Ilium's saved state.
/// An explicitly disabled prior command stays disabled and is never activated
/// as a side effect of turning model capture off.
pub fn disabled_runtime_action() -> Result<ilium_ipc::AntigravityStatuslineAction, String> {
    #[cfg(test)]
    {
        return Ok(ilium_ipc::AntigravityStatuslineAction::CancelPending);
    }
    #[cfg(not(test))]
    {
        let Some((state, still_owned)) = read_global_state_and_ownership()? else {
            return Ok(ilium_ipc::AntigravityStatuslineAction::CancelPending);
        };
        Ok(disabled_action_for_ownership(Some(&state), still_owned))
    }
}

fn enabled_action(
    state: &StatuslineState,
) -> Result<ilium_ipc::AntigravityStatuslineAction, String> {
    let command = state
        .installed
        .get("command")
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| "installed Antigravity status-line command is missing".to_owned())?;
    Ok(ilium_ipc::AntigravityStatuslineAction::SetCommand {
        command: command.to_owned(),
    })
}

fn disabled_action(state: Option<&StatuslineState>) -> ilium_ipc::AntigravityStatuslineAction {
    let Some(state) = state else {
        return ilium_ipc::AntigravityStatuslineAction::CancelPending;
    };
    let previous = state.previous.as_ref();
    let command = previous
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get("command"))
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty());
    let was_enabled = previous
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        != Some(false);
    match (command, was_enabled) {
        (Some(command), true) => ilium_ipc::AntigravityStatuslineAction::SetCommand {
            command: command.to_owned(),
        },
        (Some(_), false) => ilium_ipc::AntigravityStatuslineAction::DisableCommand,
        (None, _) => ilium_ipc::AntigravityStatuslineAction::DeleteCommand,
    }
}

fn disabled_action_for_ownership(
    state: Option<&StatuslineState>,
    still_owned: bool,
) -> ilium_ipc::AntigravityStatuslineAction {
    if still_owned {
        disabled_action(state)
    } else {
        ilium_ipc::AntigravityStatuslineAction::CancelPending
    }
}

/// Completes the persisted restore only after the server confirms that every
/// live Antigravity pane processed its requested status-line command.
pub fn restore_after_live_deactivation() -> Result<(), String> {
    reconcile_setting(false, None)
}

#[cfg(not(test))]
fn read_global_state_and_ownership() -> Result<Option<(StatuslineState, bool)>, String> {
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| "home directory is unavailable".to_owned())?;
    let config_directory = home.join(".gemini/antigravity-cli");
    let state_path = config_directory.join(STATE_FILE);
    if !state_path.exists() {
        return Ok(None);
    }
    let lock_path = ilium_platform::paths::config_dir()
        .ok_or_else(|| "Ilium configuration directory is unavailable".to_owned())?
        .join("locks/antigravity-model-statusline.lock");
    let _lock = ExclusiveFileLock::try_acquire(&lock_path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "Antigravity status-line settings are being updated by another Ilium process".to_owned()
        })?;
    let Some(state) = read_state(&state_path)? else {
        return Ok(None);
    };
    let current = read_settings(&config_directory.join("settings.json"))?;
    let installed_command = state.installed.get("command").cloned();
    let current_command =
        current_statusline(&current)?.and_then(|statusline| statusline.get("command").cloned());
    Ok(Some((
        state,
        installed_command.as_ref() == current_command.as_ref(),
    )))
}

/// Reconciles the Antigravity CLI status-line command with the model-icon
/// setting. Existing status-line commands are chained through and restored
/// only while the managed configuration remains unchanged.
pub fn reconcile_setting(enabled: bool, executable: Option<&Path>) -> Result<(), String> {
    #[cfg(test)]
    {
        let _ = (enabled, executable);
        return Ok(());
    }
    #[cfg(not(test))]
    {
        let home = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .ok_or_else(|| "home directory is unavailable".to_owned())?;
        let antigravity_config = home.join(".gemini/antigravity-cli");
        if !enabled && !antigravity_config.join(STATE_FILE).exists() {
            return Ok(());
        }
        let lock_path = ilium_platform::paths::config_dir()
            .ok_or_else(|| "Ilium configuration directory is unavailable".to_owned())?
            .join("locks/antigravity-model-statusline.lock");
        reconcile_setting_in(&antigravity_config, &lock_path, enabled, executable)
    }
}

fn reconcile_setting_in(
    config_directory: &Path,
    lock_path: &Path,
    enabled: bool,
    executable: Option<&Path>,
) -> Result<(), String> {
    let settings_path = config_directory.join("settings.json");
    let state_path = config_directory.join(STATE_FILE);
    if !enabled && !state_path.exists() {
        return Ok(());
    }
    fs::create_dir_all(&config_directory).map_err(|error| error.to_string())?;
    let _lock = ExclusiveFileLock::try_acquire(lock_path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "Antigravity status-line settings are being updated by another Ilium process".to_owned()
        })?;
    let state = read_state(&state_path)?;
    if enabled {
        if let Some(mut state) = state {
            if state.generation.is_empty() {
                state.generation = new_generation();
                write_state(&state_path, &state)?;
            }
            let current = read_settings(&settings_path)?;
            if current_statusline(&current)?.as_ref() == Some(&state.installed) {
                return Ok(());
            }
            if current_statusline(&current)? == state.previous
                && crate::agent_config_writer::top_level_json_value_text(&current, "statusLine")
                    .map_err(|error| error.to_string())?
                    == state.previous_raw
            {
                let edited = crate::agent_config_writer::edit_top_level_json_value(
                    &current,
                    "statusLine",
                    Some(state.installed),
                )
                .map_err(|error| error.to_string())?;
                return write_settings_if_unchanged(&settings_path, &current, &edited);
            }
            return Err(
                "Antigravity status-line settings changed after Ilium enabled model capture".into(),
            );
        }
        install_statusline(
            &settings_path,
            &state_path,
            executable.ok_or("Ilium executable path is unavailable")?,
        )
    } else {
        let Some(state) = state else {
            return Ok(());
        };
        let current = read_settings(&settings_path)?;
        let current_line = current_statusline(&current)?;
        let installed_command = state.installed.get("command");
        let current_command = current_line.as_ref().and_then(|line| line.get("command"));
        if installed_command.is_some() && current_command == installed_command {
            let current_line = current_line.expect("command comparison requires statusLine");
            let normalized_disabled_line = current_line.as_object().is_some_and(|line| {
                line.get("type") == state.installed.get("type")
                    && line.get("command") == installed_command
                    && line.get("enabled") == Some(&serde_json::Value::Bool(false))
            });
            let exact_normalized_disabled_line = normalized_disabled_line
                && current_line.as_object().is_some_and(|line| line.len() == 3);
            let edited = if current_line == state.installed || exact_normalized_disabled_line {
                crate::agent_config_writer::restore_top_level_json_value_text(
                    &current,
                    "statusLine",
                    state.previous_raw.as_deref(),
                )
            } else if normalized_disabled_line {
                let restored = restore_normalized_disabled_statusline_fields(
                    current_line,
                    &state.installed,
                    state.previous.as_ref(),
                );
                crate::agent_config_writer::edit_top_level_json_value(
                    &current,
                    "statusLine",
                    restored,
                )
            } else {
                let restored = restore_unmodified_statusline_fields(
                    current_line,
                    &state.installed,
                    state.previous.as_ref(),
                );
                crate::agent_config_writer::edit_top_level_json_value(
                    &current,
                    "statusLine",
                    restored,
                )
            }
            .map_err(|error| format!("cannot restore Antigravity status-line settings: {error}"))?;
            write_settings_if_unchanged(&settings_path, &current, &edited)?;
        } else if current_line == state.previous
            || current_line
                .as_ref()
                .is_some_and(|line| is_normalized_saved_statusline(line, state.previous.as_ref()))
        {
            // The live `/statusline <saved command>` action or its normalized
            // readback already switched Antigravity back to the prior renderer.
            // Restore the saved JSON
            // representation only while that same semantic value is present,
            // preserving unrelated settings changed since installation.
            let edited = crate::agent_config_writer::restore_top_level_json_value_text(
                &current,
                "statusLine",
                state.previous_raw.as_deref(),
            )
            .map_err(|error| format!("cannot restore Antigravity status-line settings: {error}"))?;
            write_settings_if_unchanged(&settings_path, &current, &edited)?;
        }
        remove_owned_file(
            &state_path,
            &serde_json::to_vec(&state).map_err(|e| e.to_string())?,
        )?;
        Ok(())
    }
}

fn restore_unmodified_statusline_fields(
    mut current: serde_json::Value,
    installed: &serde_json::Value,
    previous: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    let Some(object) = current.as_object_mut() else {
        return previous.cloned();
    };
    let previous_object = previous.and_then(serde_json::Value::as_object);
    for key in ["command", "type", "enabled", "stack_with_default"] {
        if object.get(key) == installed.get(key) {
            if let Some(value) = previous_object.and_then(|value| value.get(key)) {
                object.insert(key.to_owned(), value.clone());
            } else {
                object.remove(key);
            }
        }
    }
    if object.is_empty() {
        None
    } else {
        Some(current)
    }
}

fn restore_normalized_disabled_statusline_fields(
    mut current: serde_json::Value,
    installed: &serde_json::Value,
    previous: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    let Some(object) = current.as_object_mut() else {
        return previous.cloned();
    };
    let installed_object = installed.as_object();
    let previous_object = previous.and_then(serde_json::Value::as_object);
    if let Some(installed_object) = installed_object {
        for (key, installed_value) in installed_object {
            let current_value = object.get(key);
            let statusline_was_disabled = key == "enabled"
                && installed_value == &serde_json::Value::Bool(true)
                && current_value == Some(&serde_json::Value::Bool(false));
            if current_value == Some(installed_value)
                || current_value.is_none()
                || statusline_was_disabled
            {
                if let Some(value) = previous_object.and_then(|previous| previous.get(key)) {
                    object.insert(key.clone(), value.clone());
                } else {
                    object.remove(key);
                }
            }
        }
    }
    if object.is_empty() {
        None
    } else {
        Some(current)
    }
}

fn is_normalized_saved_statusline(
    current: &serde_json::Value,
    previous: Option<&serde_json::Value>,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    let Some(command) = previous
        .get("command")
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty())
    else {
        return false;
    };
    let was_enabled = previous.get("enabled").and_then(serde_json::Value::as_bool) != Some(false);
    *current
        == serde_json::json!({
            "type": "command",
            "command": command,
            "enabled": was_enabled,
        })
}

fn install_statusline(
    settings_path: &Path,
    state_path: &Path,
    executable: &Path,
) -> Result<(), String> {
    let current = read_settings(settings_path)?;
    let _: serde_json::Value = serde_json::from_str(&current)
        .map_err(|error| format!("invalid Antigravity settings JSON: {error}"))?;
    let previous = current_statusline(&current)?;
    let previous_command = previous
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.get("command"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let previous_raw =
        crate::agent_config_writer::top_level_json_value_text(&current, "statusLine")
            .map_err(|error| format!("cannot read Antigravity statusLine: {error}"))?;
    if let Some(previous) = &previous {
        let Some(object) = previous.as_object() else {
            return Err("Antigravity statusLine must be a JSON object".into());
        };
        if object
            .get("command")
            .is_some_and(|command| !command.is_string())
        {
            return Err("Antigravity statusLine.command must be a string".into());
        }
    }
    if previous
        .as_ref()
        .and_then(|value| serde_json::to_vec(value).ok())
        .is_some_and(|value| value.len() > MAX_INPUT_BYTES as usize / 2)
        || previous_raw
            .as_ref()
            .is_some_and(|value| value.len() > MAX_STATE_BYTES as usize / 2)
    {
        return Err("Antigravity status-line setting exceeds the bridge size limit".into());
    }
    let mut installed = previous
        .clone()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let object = installed.as_object_mut().expect("installed object ensured");
    object.insert("type".into(), serde_json::Value::String("command".into()));
    object.insert(
        "command".into(),
        serde_json::Value::String(statusline_command(executable)),
    );
    object.insert("enabled".into(), serde_json::Value::Bool(true));
    if previous_command.is_empty() {
        object
            .entry("stack_with_default")
            .or_insert(serde_json::Value::Bool(true));
    }
    let installed = installed;
    let edited = crate::agent_config_writer::edit_top_level_json_value(
        &current,
        "statusLine",
        Some(installed.clone()),
    )
    .map_err(|error| format!("cannot configure Antigravity status line: {error}"))?;
    let state = StatuslineState {
        previous,
        previous_raw,
        installed,
        generation: new_generation(),
    };
    let encoded = encode_state(&state)?;
    write_atomic(state_path, &encoded)?;
    if let Err(error) = write_settings_if_unchanged(settings_path, &current, &edited) {
        let _ = remove_owned_file(state_path, &encoded);
        return Err(error);
    }
    Ok(())
}

fn encode_state(state: &StatuslineState) -> Result<Vec<u8>, String> {
    serde_json::to_vec(state).map_err(|error| error.to_string())
}

fn write_state(path: &Path, state: &StatuslineState) -> Result<(), String> {
    write_atomic(path, &encode_state(state)?)
}

fn new_generation() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    format!("{}-{timestamp}-{sequence}", std::process::id())
}

fn statusline_command(executable: &Path) -> String {
    #[cfg(unix)]
    {
        format!(
            "{} __antigravity-model-statusline",
            shell_quote(&executable.to_string_lossy())
        )
    }
    #[cfg(windows)]
    {
        format!(
            "\"{}\" __antigravity-model-statusline",
            executable.display()
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        format!("{} __antigravity-model-statusline", executable.display())
    }
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn current_statusline(text: &str) -> Result<Option<serde_json::Value>, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("invalid Antigravity settings JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "Antigravity settings top level is not an object".to_owned())?;
    Ok(object.get("statusLine").cloned())
}

fn read_settings(path: &Path) -> Result<String, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok("{}".into()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Antigravity settings file exceeds the size limit".into());
    }
    if !metadata.is_file() {
        return Err("Antigravity settings path is not a regular file".into());
    }
    if fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Antigravity settings symlinks are not modified by Ilium".into());
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}

fn write_settings_if_unchanged(
    path: &Path,
    expected: &str,
    replacement: &str,
) -> Result<(), String> {
    if read_settings(path)? != expected {
        return Err(
            "Antigravity settings changed while Ilium was updating status-line capture".into(),
        );
    }
    write_atomic(path, replacement.as_bytes())
}

fn read_state(path: &Path) -> Result<Option<StatuslineState>, String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Ilium Antigravity status-line state must not be a symlink".into());
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Err("Ilium Antigravity status-line state is not a regular file".into());
    }
    if metadata.len() > MAX_STATE_BYTES {
        return Err("Ilium Antigravity status-line state exceeds the size limit".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map(Some)
        .map_err(|error| format!("invalid Ilium Antigravity status-line state: {error}"))
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_atomic_io(path, contents).map_err(|error| error.to_string())
}

fn write_atomic_io(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target path has no parent"))?;
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to replace a symlink",
        ));
    }
    let temporary = parent.join(format!(
        ".ilium-write-{}-{}.tmp",
        std::process::id(),
        TEMPORARY_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = ilium_platform::secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        if let Err(error) = file.set_permissions(fs::Permissions::from_mode(mode)) {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        ilium_platform::secure_fs::replace_file_durably(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

static TEMPORARY_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn remove_owned_file(path: &Path, expected: &[u8]) -> Result<(), String> {
    match fs::read(path) {
        Ok(contents) if contents == expected => {
            fs::remove_file(path).map_err(|error| error.to_string())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Reads one Antigravity CLI status-line payload and atomically records its
/// minimal model observation. Intended for the hidden `ilium` CLI helper.
pub fn run_statusline_helper() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut input)?;
    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Antigravity status-line payload exceeds the size limit",
        ));
    }
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable"))?;
    let capture_result = capture_statusline_input(&home, &input);
    let previous_result = run_previous_statusline(&home, &input);
    capture_result?;
    previous_result
}

fn capture_statusline_input(home: &Path, input: &[u8]) -> io::Result<()> {
    let Some(generation) = active_generation(home)? else {
        return Ok(());
    };
    let mut observation = parse_statusline_input(input)?;
    observation.generation = generation;
    write_observation(home, &observation)
}

fn run_previous_statusline(home: &Path, input: &[u8]) -> io::Result<()> {
    let state_path = home.join(".gemini/antigravity-cli").join(STATE_FILE);
    let state = read_state(&state_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(previous) = state.and_then(|state| state.previous) else {
        return Ok(());
    };
    if previous.get("enabled").and_then(serde_json::Value::as_bool) == Some(false) {
        return Ok(());
    }
    let Some(command) = previous.get("command").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    if command.trim().is_empty() {
        return Ok(());
    }
    #[cfg(unix)]
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    #[cfg(windows)]
    let mut child = Command::new("cmd.exe")
        .args(["/d", "/s", "/c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    #[cfg(not(any(unix, windows)))]
    let mut child = {
        let _ = command;
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported shell for the existing Antigravity status-line command",
        ));
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input) {
            if error.kind() != io::ErrorKind::BrokenPipe {
                return Err(error);
            }
        }
    }
    child.wait()?;
    Ok(())
}

fn parse_statusline_input(input: &[u8]) -> io::Result<ModelObservation> {
    let payload: StatuslineInput = serde_json::from_slice(input)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let conversation_id = match (payload.conversation_id, payload.session_id) {
        (Some(conversation_id), Some(session_id)) if conversation_id != session_id => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Antigravity conversation id fields do not match",
            ));
        }
        (Some(conversation_id), _) => conversation_id,
        (_, Some(session_id)) => session_id,
        (None, None) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Antigravity conversation id is missing",
            ));
        }
    };
    if !valid_session_id(&conversation_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Antigravity conversation id is invalid",
        ));
    }
    let project = payload.cwd.canonicalize()?;
    let model = payload
        .model
        .id
        .filter(|value| !value.trim().is_empty())
        .or(payload.model.display_name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "model id is missing"))?;
    if model.len() > MAX_MODEL_BYTES || model.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Antigravity model id is invalid",
        ));
    }
    Ok(ModelObservation {
        conversation_id,
        project,
        model,
        generation: String::new(),
    })
}

/// Returns a recorded model only when the status-line event matches the live
/// project and Antigravity's authoritative history binds that conversation to
/// the same project.
pub(crate) fn model_for_verified_session(
    home: &Path,
    project: &Path,
    conversation_id: &str,
) -> io::Result<Option<String>> {
    let state_path = home.join(".gemini/antigravity-cli").join(STATE_FILE);
    let state = read_state(&state_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(generation) = state
        .as_ref()
        .map(|state| state.generation.as_str())
        .filter(|generation| !generation.is_empty())
    else {
        return Ok(None);
    };
    let path = observation_path(home, conversation_id)?;
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Antigravity model observation must not be a symlink",
        ));
    }
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Antigravity model observation is not a regular file",
        ));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Antigravity model observation exceeds the size limit",
        ));
    }
    let observation: ModelObservation = serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let canonical_project = project.canonicalize()?;
    if observation.conversation_id != conversation_id
        || observation.project != canonical_project
        || observation.generation != generation
    {
        return Ok(None);
    }
    Ok(Some(observation.model))
}

fn active_generation(home: &Path) -> io::Result<Option<String>> {
    let state_path = home.join(".gemini/antigravity-cli").join(STATE_FILE);
    let state = read_state(&state_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(state
        .map(|state| state.generation)
        .filter(|generation| !generation.is_empty()))
}

fn write_observation(home: &Path, observation: &ModelObservation) -> io::Result<()> {
    let path = observation_path(home, &observation.conversation_id)?;
    let parent = path.parent().expect("observation path has a parent");
    fs::create_dir_all(parent)?;
    let lock_path = parent.join(".model-observations.lock");
    let Some(_lock) = ExclusiveFileLock::try_acquire(&lock_path)
        .map_err(|error| io::Error::other(error.to_string()))?
    else {
        return Ok(());
    };
    if active_generation(home)?.as_deref() != Some(observation.generation.as_str()) {
        return Ok(());
    }
    let encoded = serde_json::to_vec(observation)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_atomic_io(&path, &encoded)?;
    prune_observations(parent, &observation.conversation_id)?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn enable_capture_for_test(home: &Path) -> io::Result<()> {
    let config_directory = home.join(".gemini/antigravity-cli");
    let lock_path = home.join("locks/antigravity-model-statusline.lock");
    fs::create_dir_all(&config_directory)?;
    let executable = std::env::current_exe()?;
    reconcile_setting_in(&config_directory, &lock_path, true, Some(&executable))
        .map_err(|error| io::Error::other(error))
}

#[cfg(test)]
pub(crate) fn record_observation_for_test(
    home: &Path,
    conversation_id: &str,
    project: &Path,
    model: &str,
) -> io::Result<()> {
    let generation = active_generation(home)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "model capture is disabled"))?;
    write_observation(
        home,
        &ModelObservation {
            conversation_id: conversation_id.to_owned(),
            project: project.canonicalize()?,
            model: model.to_owned(),
            generation,
        },
    )
}

fn prune_observations(directory: &Path, current_id: &str) -> io::Result<()> {
    let mut observations = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if stem == current_id
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || !valid_session_id(stem)
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.len() > MAX_INPUT_BYTES || !metadata.is_file() {
            continue;
        }
        let observation = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ModelObservation>(&bytes).ok());
        let Some(observation) = observation else {
            continue;
        };
        if observation.conversation_id != stem {
            continue;
        }
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        observations.push((modified, path));
    }
    observations.sort_by_key(|(modified, _)| *modified);
    for (_, path) in observations
        .into_iter()
        .rev()
        .skip(MAX_OBSERVATIONS.saturating_sub(1))
    {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn observation_path(home: &Path, conversation_id: &str) -> io::Result<PathBuf> {
    if !valid_session_id(conversation_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Antigravity conversation id is invalid",
        ));
    }
    Ok(home
        .join(".gemini/antigravity-cli/ilium-model-observations")
        .join(format!("{conversation_id}.json")))
}

fn valid_session_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn live_statusline_actions_preserve_enabled_prior_commands_without_reactivating_disabled_ones()
    {
        let enabled_previous = StatuslineState {
            previous: Some(serde_json::json!({
                "type": "command",
                "command": "user-statusline --format full",
                "enabled": true,
                "extra": "preserved"
            })),
            previous_raw: None,
            installed: serde_json::json!({
                "command": "/usr/bin/ilium __antigravity-model-statusline",
                "enabled": true
            }),
            generation: "test-generation".to_owned(),
        };
        assert_eq!(
            enabled_action(&enabled_previous).unwrap(),
            ilium_ipc::AntigravityStatuslineAction::SetCommand {
                command: "/usr/bin/ilium __antigravity-model-statusline".to_owned(),
            }
        );
        assert_eq!(
            disabled_action(Some(&enabled_previous)),
            ilium_ipc::AntigravityStatuslineAction::SetCommand {
                command: "user-statusline --format full".to_owned(),
            }
        );
        assert_eq!(
            disabled_action_for_ownership(Some(&enabled_previous), false),
            ilium_ipc::AntigravityStatuslineAction::CancelPending
        );

        let disabled_previous = StatuslineState {
            previous: Some(serde_json::json!({
                "type": "command",
                "command": "disabled-user-statusline",
                "enabled": false
            })),
            previous_raw: None,
            installed: serde_json::json!({
                "command": "/usr/bin/ilium __antigravity-model-statusline",
                "enabled": true
            }),
            generation: "test-generation".to_owned(),
        };
        assert_eq!(
            disabled_action(Some(&disabled_previous)),
            ilium_ipc::AntigravityStatuslineAction::DisableCommand
        );
        assert_eq!(
            disabled_action(None),
            ilium_ipc::AntigravityStatuslineAction::CancelPending
        );
        assert_eq!(
            disabled_action_for_ownership(None, false),
            ilium_ipc::AntigravityStatuslineAction::CancelPending
        );
    }

    fn reconcile(directory: &Path, enabled: bool, executable: Option<&Path>) -> Result<(), String> {
        reconcile_setting_in(
            directory,
            &directory.join("locks/antigravity-model-statusline.lock"),
            enabled,
            executable,
        )
    }

    #[test]
    fn observations_are_scoped_to_conversation_and_project() {
        let home = tempdir().unwrap();
        let project = tempdir().unwrap();
        let other_project = tempdir().unwrap();
        reconcile_setting_in(
            &home.path().join(".gemini/antigravity-cli"),
            &home.path().join("locks/antigravity-model-statusline.lock"),
            true,
            Some(Path::new("/usr/bin/ilium")),
        )
        .unwrap();
        let id = "12345678-abcd-ef01-2345-6789abcdef01";
        record_observation_for_test(home.path(), id, project.path(), "Gemini 3.5 Flash (High)")
            .unwrap();

        assert_eq!(
            model_for_verified_session(home.path(), project.path(), id).unwrap(),
            Some("Gemini 3.5 Flash (High)".to_owned())
        );
        assert_eq!(
            model_for_verified_session(home.path(), other_project.path(), id).unwrap(),
            None
        );
        assert_eq!(
            model_for_verified_session(
                home.path(),
                project.path(),
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn statusline_helper_does_not_capture_a_model_while_setting_is_disabled() {
        let home = tempdir().unwrap();
        let project = tempdir().unwrap();
        let id = "12345678-abcd-ef01-2345-6789abcdef01";
        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": project.path(),
            "conversation_id": id,
            "model": {"id": "gemini-3-pro"}
        }))
        .unwrap();

        capture_statusline_input(home.path(), &payload).unwrap();

        assert!(!observation_path(home.path(), id).unwrap().exists());
        assert_eq!(
            model_for_verified_session(home.path(), project.path(), id).unwrap(),
            None
        );
    }

    #[test]
    fn old_observation_is_rejected_after_model_icons_are_disabled_and_reenabled() {
        let home = tempdir().unwrap();
        let project = tempdir().unwrap();
        let config_directory = home.path().join(".gemini/antigravity-cli");
        let lock_path = config_directory.join("locks/antigravity-model-statusline.lock");
        let id = "12345678-abcd-ef01-2345-6789abcdef01";
        reconcile_setting_in(
            &config_directory,
            &lock_path,
            true,
            Some(Path::new("/usr/bin/ilium")),
        )
        .unwrap();
        let payload = |model: &str| {
            serde_json::to_vec(&serde_json::json!({
                "cwd": project.path(),
                "conversation_id": id,
                "model": {"id": model}
            }))
            .unwrap()
        };
        capture_statusline_input(home.path(), &payload("gemini-3-pro")).unwrap();
        assert_eq!(
            model_for_verified_session(home.path(), project.path(), id).unwrap(),
            Some("gemini-3-pro".to_owned())
        );

        reconcile_setting_in(&config_directory, &lock_path, false, None).unwrap();
        reconcile_setting_in(
            &config_directory,
            &lock_path,
            true,
            Some(Path::new("/usr/bin/ilium")),
        )
        .unwrap();

        assert_eq!(
            model_for_verified_session(home.path(), project.path(), id).unwrap(),
            None,
            "the setting must not reuse a model observed during an earlier enabled period"
        );

        capture_statusline_input(home.path(), &payload("gemini-3-flash")).unwrap();
        assert_eq!(
            model_for_verified_session(home.path(), project.path(), id).unwrap(),
            Some("gemini-3-flash".to_owned()),
            "a fresh model observation must be accepted in the new enabled period"
        );
    }

    #[test]
    fn disable_restores_saved_statusline_json_after_live_command_switch() {
        let home = tempdir().unwrap();
        let config_directory = home.path().join(".gemini/antigravity-cli");
        let lock_path = home.path().join("locks/antigravity-model-statusline.lock");
        fs::create_dir_all(&config_directory).unwrap();
        let original = "{\n  \"other\": 7,\n  \"statusLine\": { \"type\": \"command\", \"command\": \"user-renderer --format 'wide'\", \"enabled\": true }\n}\n";
        fs::write(config_directory.join("settings.json"), original).unwrap();

        reconcile_setting_in(
            &config_directory,
            &lock_path,
            true,
            Some(Path::new("/usr/bin/ilium")),
        )
        .unwrap();
        let state_path = config_directory.join(STATE_FILE);
        let state = read_state(&state_path).unwrap().unwrap();
        fs::write(
            config_directory.join("settings.json"),
            r#"{"other":7,"statusLine":{"type":"command","command":"user-renderer --format 'wide'","enabled":true}}"#,
        )
        .unwrap();

        reconcile_setting_in(&config_directory, &lock_path, false, None).unwrap();

        let restored = fs::read_to_string(config_directory.join("settings.json")).unwrap();
        assert_eq!(current_statusline(&restored).unwrap(), state.previous);
        assert_eq!(
            crate::agent_config_writer::top_level_json_value_text(&restored, "statusLine").unwrap(),
            state.previous_raw
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&restored).unwrap()["other"],
            7
        );
        assert!(!state_path.exists());
        assert_eq!(
            state.previous,
            Some(serde_json::json!({
                "type": "command",
                "command": "user-renderer --format 'wide'",
                "enabled": true
            }))
        );
    }

    #[test]
    fn disable_restores_enabled_saved_statusline_after_cli_normalizes_it() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        let original = "{\n  \"theme\": \"dark\",\n  \"statusLine\": { \"type\" : \"command\",\n    \"command\": \"user-statusline --format full\", \"enabled\": true, \"stack_with_default\": false, \"customField\": \"preserved\" }\n}\n";
        fs::write(&settings, original).unwrap();

        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let current = fs::read_to_string(&settings).unwrap();
        let cli_normalized_saved_line = serde_json::json!({
            "type": "command",
            "command": "user-statusline --format full",
            "enabled": true
        });
        let current = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "statusLine",
            Some(cli_normalized_saved_line),
        )
        .unwrap();
        let current = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "theme",
            Some(serde_json::Value::String("light".into())),
        )
        .unwrap();
        fs::write(&settings, current).unwrap();

        reconcile(config.path(), false, None).unwrap();

        let restored = fs::read_to_string(&settings).unwrap();
        assert_eq!(
            crate::agent_config_writer::top_level_json_value_text(&restored, "statusLine").unwrap(),
            crate::agent_config_writer::top_level_json_value_text(original, "statusLine").unwrap(),
            "the saved statusLine JSON, including CLI-normalized custom fields, must be restored byte-for-byte"
        );
        let restored_line = current_statusline(&restored).unwrap().unwrap();
        assert_eq!(restored_line["customField"], "preserved");
        assert_eq!(restored_line["stack_with_default"], false);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&restored).unwrap()["theme"],
            "light",
            "restoration must preserve unrelated settings edited while the bridge was installed"
        );
        assert!(!config.path().join(STATE_FILE).exists());
    }

    #[test]
    fn disable_restores_saved_fields_after_statusline_off_drops_them() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        let original = "{\n  \"theme\": \"dark\",\n  \"statusLine\": { \"type\" : \"command\",\n    \"command\": \"user-statusline --format full\", \"enabled\": true, \"stack_with_default\": false, \"customField\": \"preserved\" }\n}\n";
        fs::write(&settings, original).unwrap();

        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let state_path = config.path().join(STATE_FILE);
        let state = read_state(&state_path).unwrap().unwrap();
        let normalized_off = serde_json::json!({
            "type": "command",
            "command": state.installed["command"],
            "enabled": false
        });
        let current = fs::read_to_string(&settings).unwrap();
        let current = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "statusLine",
            Some(normalized_off),
        )
        .unwrap();
        let current = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "theme",
            Some(serde_json::Value::String("light".into())),
        )
        .unwrap();
        fs::write(&settings, current).unwrap();

        reconcile(config.path(), false, None).unwrap();

        let restored = fs::read_to_string(&settings).unwrap();
        assert_eq!(
            crate::agent_config_writer::top_level_json_value_text(&restored, "statusLine").unwrap(),
            crate::agent_config_writer::top_level_json_value_text(original, "statusLine").unwrap(),
            "the statusline-off readback must not discard saved stack_with_default or custom fields"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&restored).unwrap()["theme"],
            "light",
            "restoring statusLine must preserve unrelated settings changed concurrently"
        );
        assert!(!state_path.exists());
    }

    #[test]
    fn disable_restores_dropped_fields_when_statusline_off_keeps_extra_metadata() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        let original = "{\n  \"theme\": \"dark\",\n  \"statusLine\": { \"type\" : \"command\",\n    \"command\": \"user-statusline --format full\", \"enabled\": true, \"stack_with_default\": false, \"customField\": \"preserved\" }\n}\n";
        fs::write(&settings, original).unwrap();

        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let state_path = config.path().join(STATE_FILE);
        let state = read_state(&state_path).unwrap().unwrap();
        let normalized_off_with_metadata = serde_json::json!({
            "type": "command",
            "command": state.installed["command"],
            "enabled": false,
            "customField": "edited while capture was enabled",
            "cliMetadata": "preserve this readback field"
        });
        let current = fs::read_to_string(&settings).unwrap();
        let current = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "statusLine",
            Some(normalized_off_with_metadata),
        )
        .unwrap();
        let current = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "theme",
            Some(serde_json::Value::String("light".into())),
        )
        .unwrap();
        fs::write(&settings, current).unwrap();

        reconcile(config.path(), false, None).unwrap();

        let restored = fs::read_to_string(&settings).unwrap();
        assert_eq!(
            current_statusline(&restored).unwrap().unwrap(),
            serde_json::json!({
                "type": "command",
                "command": "user-statusline --format full",
                "enabled": true,
                "stack_with_default": false,
                "customField": "edited while capture was enabled",
                "cliMetadata": "preserve this readback field"
            }),
            "statusline-off normalization must restore omitted fields while retaining explicit changes and new metadata"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&restored).unwrap()["theme"],
            "light",
            "restoring statusLine must preserve unrelated settings edited while the bridge was installed"
        );
        assert!(!state_path.exists());
    }

    #[test]
    fn conversation_ids_cannot_escape_the_observation_directory() {
        assert!(!valid_session_id("../../settings.json"));
        assert!(observation_path(Path::new("/home/test"), "../../settings.json").is_err());
    }

    #[test]
    fn statusline_payload_retains_only_project_conversation_and_model() {
        let project = tempdir().unwrap();
        let payload = serde_json::json!({
            "cwd": project.path(),
            "conversation_id": "12345678-abcd-ef01-2345-6789abcdef01",
            "session_id": "12345678-abcd-ef01-2345-6789abcdef01",
            "model": {
                "id": "Gemini 3.5 Flash (High)",
                "display_name": "Gemini Flash"
            },
            "email": "must-not-be-retained@example.invalid",
            "prompt": "must not be retained"
        });
        let observation = parse_statusline_input(&serde_json::to_vec(&payload).unwrap()).unwrap();
        assert_eq!(
            observation.conversation_id,
            "12345678-abcd-ef01-2345-6789abcdef01"
        );
        assert_eq!(observation.project, project.path().canonicalize().unwrap());
        assert_eq!(observation.model, "Gemini 3.5 Flash (High)");
        let encoded = serde_json::to_string(&observation).unwrap();
        assert!(!encoded.contains("must not be retained"));
        assert!(!encoded.contains("must-not-be-retained"));
    }

    #[test]
    fn statusline_payload_rejects_conflicting_session_aliases() {
        let project = tempdir().unwrap();
        let payload = serde_json::json!({
            "cwd": project.path(),
            "conversation_id": "12345678-abcd-ef01-2345-6789abcdef01",
            "session_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "model": {"id": "gemini-3-pro"}
        });
        assert!(parse_statusline_input(&serde_json::to_vec(&payload).unwrap()).is_err());
    }

    #[test]
    fn statusline_configuration_chains_and_restores_an_existing_command() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        let original = "{\n  \"theme\": \"dark\",\n  \"statusLine\": { \"type\" : \"command\",\n    \"command\": \"my-statusline --format compact\", \"stack_with_default\": true }\n}\n";
        fs::write(&settings, original).unwrap();
        let previous = current_statusline(&fs::read_to_string(&settings).unwrap())
            .unwrap()
            .unwrap();

        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let installed = fs::read_to_string(&settings).unwrap();
        let installed_line = current_statusline(&installed).unwrap().unwrap();
        assert_eq!(installed_line["type"], "command");
        assert_eq!(installed_line["enabled"], true);
        assert_eq!(installed_line["stack_with_default"], true);
        assert!(installed_line["command"]
            .as_str()
            .unwrap()
            .contains("__antigravity-model-statusline"));
        let state: StatuslineState =
            serde_json::from_slice(&fs::read(config.path().join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(
            state.previous.as_ref().unwrap()["command"],
            "my-statusline --format compact"
        );

        reconcile(config.path(), false, None).unwrap();
        let restored = fs::read_to_string(&settings).unwrap();
        assert_eq!(current_statusline(&restored).unwrap(), Some(previous));
        assert_eq!(restored, original);
        assert!(restored.contains("\"theme\": \"dark\""));
        assert!(!config.path().join(STATE_FILE).exists());
    }

    #[test]
    fn statusline_configuration_stacks_with_default_and_removes_its_own_key() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        fs::write(&settings, "{\n  \"theme\": \"dark\"\n}\n").unwrap();

        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let enabled = fs::read_to_string(&settings).unwrap();
        let line = current_statusline(&enabled).unwrap().unwrap();
        assert_eq!(line["stack_with_default"], true);

        reconcile(config.path(), false, None).unwrap();
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "{\n  \"theme\": \"dark\"\n}\n"
        );
    }

    #[test]
    fn statusline_configuration_restores_previously_disabled_command_byte_for_byte() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        let original = "{\n  \"theme\": \"dark\",\n  \"statusLine\": { \"type\" : \"command\", \"command\": \"user-statusline --format full\", \"enabled\": false, \"extra\": \"preserved\" }\n}\n";
        fs::write(&settings, original).unwrap();

        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let installed = fs::read_to_string(&settings).unwrap();
        let installed_line = current_statusline(&installed).unwrap().unwrap();
        assert_eq!(installed_line["enabled"], true);
        assert!(installed_line["command"]
            .as_str()
            .unwrap()
            .contains("__antigravity-model-statusline"));

        // Antigravity 1.3.1 normalizes `/statusline off` to these three
        // fields, dropping the prior command's custom fields.
        let disabled_line = serde_json::json!({
            "type": "command",
            "command": installed_line["command"].clone(),
            "enabled": false,
        });
        let settings_with_disabled_bridge = crate::agent_config_writer::edit_top_level_json_value(
            &fs::read_to_string(&settings).unwrap(),
            "statusLine",
            Some(disabled_line),
        )
        .unwrap();
        write_settings_if_unchanged(
            &settings,
            &fs::read_to_string(&settings).unwrap(),
            &settings_with_disabled_bridge,
        )
        .unwrap();

        reconcile(config.path(), false, None).unwrap();
        assert_eq!(fs::read_to_string(&settings).unwrap(), original);
        assert!(!config.path().join(STATE_FILE).exists());
    }

    #[test]
    fn disabling_capture_preserves_statusline_changes_made_by_the_user() {
        let config = tempdir().unwrap();
        let settings = config.path().join("settings.json");
        fs::write(
            &settings,
            "{\"statusLine\": {\"type\": \"command\", \"command\": \"old\"}}",
        )
        .unwrap();
        reconcile(config.path(), true, Some(Path::new("/usr/bin/ilium"))).unwrap();
        let user_settings =
            "{\"statusLine\": {\"type\": \"command\", \"command\": \"user-change\"}}";
        fs::write(&settings, user_settings).unwrap();

        reconcile(config.path(), false, None).unwrap();
        assert_eq!(fs::read_to_string(&settings).unwrap(), user_settings);
    }
}
