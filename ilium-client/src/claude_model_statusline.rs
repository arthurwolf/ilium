//! Captures Claude Code's selected model from its status-line JSON input.
//!
//! The bridge stores only the session ID, project path, and model ID. It
//! temporarily overlays each active project's ignored `settings.local.json`,
//! chains the status-line command that was previously effective, and restores
//! only the `statusLine` value that Ilium installed.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: u64 = 16 * 1024;
const MAX_SETTINGS_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 1024 * 1024;
const MAX_PROJECTS: usize = 128;
const MAX_OBSERVATIONS: usize = 128;
const MAX_MODEL_BYTES: usize = 256;
const REGISTRY_FILE: &str = "claude-model-statusline-projects.json";
const RECONCILE_RETRY: Duration = Duration::from_secs(3);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
static APP_RECONCILER: OnceLock<Mutex<AppReconciler>> = OnceLock::new();

#[derive(Default)]
struct AppReconciler {
    initialized: bool,
    enabled: bool,
    projects: BTreeSet<PathBuf>,
    generation: Option<String>,
    retry_after: Option<Instant>,
}

/// Reconciles the process-wide bridge only when its setting or live Claude
/// project set changes, with a bounded retry interval after filesystem errors.
pub fn reconcile_app_setting(
    enabled: bool,
    projects: &[PathBuf],
    executable: Option<&Path>,
) -> Result<(), String> {
    let projects = projects.iter().cloned().collect::<BTreeSet<_>>();
    let reconciler = APP_RECONCILER.get_or_init(|| Mutex::new(AppReconciler::default()));
    let now = Instant::now();
    let generation = {
        let state = reconciler
            .lock()
            .map_err(|_| "Claude model bridge state is unavailable".to_owned())?;
        if state
            .retry_after
            .is_some_and(|retry_after| retry_after > now)
        {
            return Ok(());
        }
        let unchanged = state.initialized && state.enabled == enabled && state.projects == projects;
        if unchanged {
            return Ok(());
        }
        if enabled {
            state.generation.clone().or_else(|| {
                let sequence = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                Some(format!("{}-{timestamp}-{sequence}", std::process::id()))
            })
        } else {
            None
        }
    };

    let project_paths = projects.iter().cloned().collect::<Vec<_>>();
    let result = reconcile_setting(
        enabled,
        &project_paths,
        executable,
        generation.as_deref().unwrap_or("disabled"),
    );
    let mut state = reconciler
        .lock()
        .map_err(|_| "Claude model bridge state is unavailable".to_owned())?;
    match result {
        Ok(()) => {
            state.initialized = true;
            state.enabled = enabled;
            state.projects = projects;
            state.generation = generation;
            state.retry_after = None;
            Ok(())
        }
        Err(error) => {
            state.retry_after = Some(Instant::now() + RECONCILE_RETRY);
            Err(error)
        }
    }
}

/// Fences cached observations immediately when the user changes the setting.
pub fn reset_app_generation() {
    if let Some(reconciler) = APP_RECONCILER.get() {
        if let Ok(mut state) = reconciler.lock() {
            *state = AppReconciler::default();
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Observation {
    session_id: String,
    project: PathBuf,
    model: String,
    generation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProjectState {
    project: PathBuf,
    claude_directory_existed: bool,
    local_settings_existed: bool,
    previous_local_raw: Option<String>,
    previous_effective: Option<serde_json::Value>,
    installed: serde_json::Value,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Registry {
    generation: Option<String>,
    projects: BTreeMap<String, ProjectState>,
}

/// Reconciles live Claude projects with the model-capture status-line bridge.
/// Projects are canonicalized and bounded before any settings file is read.
pub fn reconcile_setting(
    enabled: bool,
    projects: &[PathBuf],
    executable: Option<&Path>,
    generation: &str,
) -> Result<(), String> {
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| "home directory is unavailable".to_owned())?;
    let config_dir = ilium_platform::paths::config_dir()
        .ok_or_else(|| "Ilium configuration directory is unavailable".to_owned())?;
    reconcile_setting_in(
        &home,
        &config_dir,
        enabled,
        projects,
        executable,
        generation,
    )
}

fn reconcile_setting_in(
    home: &Path,
    config_dir: &Path,
    enabled: bool,
    projects: &[PathBuf],
    executable: Option<&Path>,
    generation: &str,
) -> Result<(), String> {
    if projects.len() > MAX_PROJECTS {
        return Err("too many active Claude projects for model capture".into());
    }
    let state_dir = config_dir.join("claude-model-statusline");
    if fs::symlink_metadata(&state_dir).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Claude model status-line state directory must not be a symlink".into());
    }
    ilium_platform::secure_fs::create_private_directory(&state_dir)
        .map_err(|error| format!("cannot create private Claude model state: {error}"))?;
    let registry_path = state_dir.join(REGISTRY_FILE);
    let lock_path = config_dir.join("locks/claude-model-statusline.lock");
    fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;
    let _lock = ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&lock_path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "Claude model status-line settings are being updated by another Ilium process"
                .to_owned()
        })?;

    let mut registry = read_registry(&registry_path)?;
    if enabled {
        if generation.is_empty() || generation.len() > 128 {
            return Err("invalid Claude model capture generation".into());
        }
        registry.generation = Some(generation.to_owned());
        let executable = executable.ok_or("Ilium executable path is unavailable")?;
        for project in projects {
            let project = fs::canonicalize(project).map_err(|error| {
                format!(
                    "cannot resolve Claude project {}: {error}",
                    project.display()
                )
            })?;
            let key = project_key(&project);
            if let Some(state) = registry.projects.get(&key) {
                if state.project != project {
                    return Err("Claude model status-line project key collision".into());
                }
                reconcile_existing_project(&project, state, executable)?;
            } else {
                if registry.projects.len() >= MAX_PROJECTS {
                    return Err("too many retained Claude projects for model capture".into());
                }
                let (state, settings_path, current, edited) =
                    prepare_project(&project, home, executable)?;
                registry.projects.insert(key.clone(), state);
                // Persist the restore record before writing project settings so
                // a crash cannot leave an untracked Ilium command installed.
                write_registry(&registry_path, &registry)?;
                write_settings_if_unchanged(&settings_path, &current, &edited)?;
            }
        }
        if registry.projects.len() > MAX_PROJECTS {
            return Err("too many retained Claude projects for model capture".into());
        }
        write_registry(&registry_path, &registry)
    } else {
        registry.generation = None;
        let keys = registry.projects.keys().cloned().collect::<Vec<_>>();
        let mut failures = Vec::new();
        for key in keys {
            let Some(state) = registry.projects.get(&key) else {
                continue;
            };
            match restore_project(&state.project, state) {
                Ok(()) => {
                    registry.projects.remove(&key);
                }
                Err(error) => failures.push(error),
            }
        }
        write_registry(&registry_path, &registry)?;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

fn reconcile_existing_project(
    project: &Path,
    state: &ProjectState,
    executable: &Path,
) -> Result<(), String> {
    let settings_path = local_settings_path(project);
    let current = read_settings(&settings_path)?;
    let current_line = current_statusline(&current)?;
    if current_line.as_ref() == Some(&state.installed) {
        return Ok(());
    }
    if current_line
        == state
            .previous_local_raw
            .as_deref()
            .and_then(parse_optional_json)
    {
        let installed = install_statusline_value(
            state
                .previous_local_raw
                .as_deref()
                .and_then(parse_optional_json),
            state.previous_effective.clone(),
            executable,
        );
        let edited = crate::agent_config_writer::edit_top_level_json_value(
            &current,
            "statusLine",
            Some(installed),
        )
        .map_err(|error| error.to_string())?;
        return write_settings_if_unchanged(&settings_path, &current, &edited);
    }
    Err(format!(
        "Claude project settings changed after Ilium enabled model capture: {}",
        project.display()
    ))
}

fn prepare_project(
    project: &Path,
    home: &Path,
    executable: &Path,
) -> Result<(ProjectState, PathBuf, String, String), String> {
    let claude_dir = project.join(".claude");
    if fs::symlink_metadata(&claude_dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(format!(
            "Claude project settings directory is a symlink: {}",
            claude_dir.display()
        ));
    }
    let claude_directory_existed = claude_dir.exists();
    fs::create_dir_all(&claude_dir).map_err(|error| error.to_string())?;
    let settings_path = local_settings_path(project);
    let local_settings_existed = settings_path.exists();
    let current = read_settings(&settings_path)?;
    let previous_local_raw =
        crate::agent_config_writer::top_level_json_value_text(&current, "statusLine")
            .map_err(|error| error.to_string())?;
    let previous_local = previous_local_raw.as_deref().and_then(parse_optional_json);
    let previous_effective = match previous_local.clone() {
        Some(value) => Some(value),
        None => effective_statusline(project, home)?,
    };
    if let Some(value) = previous_local.as_ref().or(previous_effective.as_ref()) {
        if !value.is_object() {
            return Err("Claude statusLine must be a JSON object to chain safely".into());
        }
        if value
            .get("command")
            .is_some_and(|command| !command.is_string())
        {
            return Err("Claude statusLine.command must be a string".into());
        }
    }
    let installed =
        install_statusline_value(previous_local, previous_effective.clone(), executable);
    let edited = crate::agent_config_writer::edit_top_level_json_value(
        &current,
        "statusLine",
        Some(installed.clone()),
    )
    .map_err(|error| format!("cannot configure Claude status-line capture: {error}"))?;
    Ok((
        ProjectState {
            project: project.to_path_buf(),
            claude_directory_existed,
            local_settings_existed,
            previous_local_raw,
            previous_effective,
            installed,
        },
        settings_path,
        current,
        edited,
    ))
}

fn install_statusline_value(
    previous_local: Option<serde_json::Value>,
    previous_effective: Option<serde_json::Value>,
    executable: &Path,
) -> serde_json::Value {
    let mut installed = previous_local
        .or(previous_effective)
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    installed.as_object_mut().expect("object ensured").insert(
        "command".into(),
        serde_json::Value::String(statusline_command(executable)),
    );
    installed
        .as_object_mut()
        .expect("object ensured")
        .insert("type".into(), serde_json::Value::String("command".into()));
    installed
}

fn restore_project(project: &Path, state: &ProjectState) -> Result<(), String> {
    let settings_path = local_settings_path(project);
    let current = read_settings(&settings_path)?;
    let current_line = current_statusline(&current)?;
    if current_line.as_ref() == Some(&state.installed)
        || (state.previous_local_raw.is_none() && current_line == state.previous_effective)
    {
        let edited = crate::agent_config_writer::restore_top_level_json_value_text(
            &current,
            "statusLine",
            state.previous_local_raw.as_deref(),
        )
        .map_err(|error| format!("cannot restore Claude status-line setting: {error}"))?;
        write_settings_if_unchanged(&settings_path, &current, &edited)?;
    } else if let (Some(current_line), Some(installed)) =
        (current_line, state.installed.as_object())
    {
        let Some(mut current_line) = current_line.as_object().cloned() else {
            return Ok(());
        };
        let previous = state
            .previous_local_raw
            .as_deref()
            .and_then(parse_optional_json);
        let previous = previous.as_ref().and_then(serde_json::Value::as_object);
        let mut changed_owned_field = false;
        for field in ["command", "type", "padding"] {
            if current_line.get(field) == installed.get(field) {
                changed_owned_field = true;
                if let Some(value) = previous.and_then(|object| object.get(field)) {
                    current_line.insert(field.to_owned(), value.clone());
                } else {
                    current_line.remove(field);
                }
            }
        }
        if changed_owned_field {
            let restored = if current_line.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(current_line))
            };
            let edited = crate::agent_config_writer::edit_top_level_json_value(
                &current,
                "statusLine",
                restored,
            )
            .map_err(|error| format!("cannot restore Claude status-line fields: {error}"))?;
            write_settings_if_unchanged(&settings_path, &current, &edited)?;
        }
    }
    if !state.local_settings_existed
        && fs::read_to_string(&settings_path).is_ok_and(|text| text.trim() == "{}")
    {
        fs::remove_file(&settings_path).map_err(|error| error.to_string())?;
    }
    if !state.claude_directory_existed {
        match fs::remove_dir(project.join(".claude")) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn effective_statusline(project: &Path, home: &Path) -> Result<Option<serde_json::Value>, String> {
    for path in [
        project.join(".claude/settings.json"),
        home.join(".claude/settings.json"),
    ] {
        let text = read_settings(&path)?;
        if current_statusline(&text)?.is_some() {
            return current_statusline(&text);
        }
    }
    Ok(None)
}

fn read_settings(path: &Path) -> Result<String, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok("{}".into()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.len() > MAX_SETTINGS_BYTES {
        return Err(format!(
            "Claude settings file is not a bounded regular file: {}",
            path.display()
        ));
    }
    if fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(format!(
            "Claude settings symlinks are not modified by Ilium: {}",
            path.display()
        ));
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("invalid Claude settings JSON: {error}"))?;
    if !value.is_object() {
        return Err("Claude settings top level is not an object".into());
    }
    Ok(text)
}

fn current_statusline(text: &str) -> Result<Option<serde_json::Value>, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("invalid Claude settings JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "Claude settings top level is not an object".to_owned())?;
    Ok(object.get("statusLine").cloned())
}

fn parse_optional_json(raw: &str) -> Option<serde_json::Value> {
    serde_json::from_str(raw).ok()
}

fn local_settings_path(project: &Path) -> PathBuf {
    project.join(".claude/settings.local.json")
}

fn write_settings_if_unchanged(
    path: &Path,
    expected: &str,
    replacement: &str,
) -> Result<(), String> {
    if read_settings(path)? != expected {
        return Err(format!(
            "Claude settings changed while Ilium was updating {}",
            path.display()
        ));
    }
    write_atomic(path, replacement.as_bytes()).map_err(|error| error.to_string())
}

fn read_registry(path: &Path) -> Result<Registry, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Registry::default()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.len() > MAX_STATE_BYTES {
        return Err("Claude model status-line registry is not a bounded regular file".into());
    }
    if fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Claude model status-line registry must not be a symlink".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("invalid Claude model status-line registry: {error}"))
}

fn write_registry(path: &Path, registry: &Registry) -> Result<(), String> {
    let bytes = serde_json::to_vec(registry).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err("Claude model status-line registry exceeds its size limit".into());
    }
    if registry.projects.is_empty() {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    }
    write_atomic(path, &bytes).map_err(|error| error.to_string())
}

fn statusline_command(executable: &Path) -> String {
    #[cfg(unix)]
    {
        format!(
            "{} __claude-model-statusline",
            shell_quote(&executable.to_string_lossy())
        )
    }
    #[cfg(windows)]
    {
        format!("\"{}\" __claude-model-statusline", executable.display())
    }
    #[cfg(not(any(unix, windows)))]
    {
        format!("{} __claude-model-statusline", executable.display())
    }
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn project_key(project: &Path) -> String {
    let digest = Sha256::digest(project.as_os_str().as_encoded_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn session_key(session_id: &str) -> String {
    let digest = Sha256::digest(session_id.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Loads the last verified observation for exactly this session and project.
pub fn model_for_verified_session(
    home: &Path,
    project: &Path,
    session_id: &str,
) -> io::Result<Option<String>> {
    let config_dir = ilium_platform::paths::config_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Ilium configuration directory is unavailable",
        )
    })?;
    model_for_verified_session_in_config(home, project, session_id, &config_dir)
}

pub(crate) fn model_for_verified_session_in_config(
    home: &Path,
    project: &Path,
    session_id: &str,
    config_dir: &Path,
) -> io::Result<Option<String>> {
    let project = fs::canonicalize(project)?;
    let registry_path = config_dir
        .join("claude-model-statusline")
        .join(REGISTRY_FILE);
    if fs::symlink_metadata(&registry_path)
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let registry = read_registry(&registry_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(generation) = registry.generation.as_deref() else {
        return Ok(None);
    };
    if !registry.projects.contains_key(&project_key(&project)) {
        return Ok(None);
    }
    let path = observation_path(home, session_id)?;
    let observation = match read_bounded_json::<Observation>(&path, MAX_STATE_BYTES) {
        Ok(observation) => observation,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(model_for_generation(
        observation,
        session_id,
        &project,
        generation,
    ))
}

fn model_for_generation(
    observation: Observation,
    session_id: &str,
    project: &Path,
    generation: &str,
) -> Option<String> {
    (observation.session_id == session_id
        && observation.project == project
        && observation.generation == generation)
        .then_some(observation.model)
}

/// Reads Claude Code's status-line input, records minimal model identity, and
/// runs the previously effective status-line command with the original JSON.
pub fn run_statusline_helper() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut input)?;
    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude status-line payload exceeds the size limit",
        ));
    }
    let observation = parse_observation(&input)?;
    let value: serde_json::Value = serde_json::from_slice(&input)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let display_name = value
        .get("model")
        .and_then(|model| model.get("display_name"))
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| observation.model.clone());
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable"))?;
    let config_dir = ilium_platform::paths::config_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Ilium configuration directory is unavailable",
        )
    })?;
    let registry_path = config_dir
        .join("claude-model-statusline")
        .join(REGISTRY_FILE);
    let registry = read_registry(&registry_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut observation = observation;
    let previous_result = if let Some(generation) = registry.generation.as_deref().filter(|_| {
        registry
            .projects
            .contains_key(&project_key(&observation.project))
    }) {
        observation.generation = generation.to_owned();
        write_observation(&home, &observation)?;
        run_previous_statusline(&registry, &observation.project, &input)?
    } else {
        false
    };
    if !previous_result {
        println!("{display_name}");
    }
    Ok(())
}

fn parse_observation(input: &[u8]) -> io::Result<Observation> {
    #[derive(Deserialize)]
    struct Input {
        session_id: String,
        cwd: PathBuf,
        model: Model,
    }
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    let input: Input = serde_json::from_slice(input)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if input.session_id.is_empty() || input.session_id.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude session ID is missing or too long",
        ));
    }
    if input.model.id.is_empty() || input.model.id.len() > MAX_MODEL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude model ID is missing or too long",
        ));
    }
    let project = fs::canonicalize(input.cwd)?;
    Ok(Observation {
        session_id: input.session_id,
        project,
        model: input.model.id,
        generation: String::new(),
    })
}

fn write_observation(home: &Path, observation: &Observation) -> io::Result<()> {
    let path = observation_path(home, &observation.session_id)?;
    let directory = path
        .parent()
        .expect("Claude observation path has a parent directory");
    ilium_platform::secure_fs::create_private_directory(directory)?;
    if fs::symlink_metadata(directory).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude model observations directory must not be a symlink",
        ));
    }
    let lock_path = directory.join(".lock");
    let _lock = ilium_platform::file_lock::ExclusiveFileLock::acquire(&lock_path)?;
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude model observation must not be a symlink",
        ));
    }
    if let Ok(current) = read_bounded_json::<Observation>(&path, MAX_STATE_BYTES) {
        if current.session_id == observation.session_id
            && current.project == observation.project
            && current.model == observation.model
            && current.generation == observation.generation
        {
            return Ok(());
        }
    }
    let bytes = serde_json::to_vec(observation)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_atomic(&path, &bytes)?;
    prune_observations(directory, &path)
}

fn prune_observations(directory: &Path, current: &Path) -> io::Result<()> {
    let mut observations = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if !name.ends_with(".json") || !entry.file_type().ok()?.is_file() || path == current {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    let retained_existing = MAX_OBSERVATIONS.saturating_sub(1);
    if observations.len() > retained_existing {
        let remove_count = observations.len() - retained_existing;
        observations.sort_by(|(left_time, left_path), (right_time, right_path)| {
            left_time
                .cmp(right_time)
                .then_with(|| left_path.cmp(right_path))
        });
        for (_, path) in observations.into_iter().take(remove_count) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn observation_path(home: &Path, session_id: &str) -> io::Result<PathBuf> {
    if session_id.is_empty() || session_id.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Claude session ID",
        ));
    }
    let claude_directory = home.join(".claude");
    if fs::symlink_metadata(&claude_directory)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude home directory must not be a symlink",
        ));
    }
    let observations = claude_directory.join("ilium-model-observations");
    if fs::symlink_metadata(&observations).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude model observations directory must not be a symlink",
        ));
    }
    Ok(observations.join(format!("{}.json", session_key(session_id))))
}

fn run_previous_statusline(registry: &Registry, project: &Path, input: &[u8]) -> io::Result<bool> {
    let Some(state) = registry.projects.get(&project_key(project)) else {
        return Ok(false);
    };
    let Some(command) = state
        .previous_effective
        .as_ref()
        .and_then(|value| value.get("command"))
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty())
    else {
        return Ok(false);
    };
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
    let mut child = return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "status-line chaining is unsupported",
    ));
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input) {
            if error.kind() != io::ErrorKind::BrokenPipe {
                return Err(error);
            }
        }
    }
    let status = child.wait()?;
    if status.success() {
        Ok(true)
    } else {
        Err(io::Error::other(format!(
            "previous Claude status-line command exited with {status}"
        )))
    }
}

fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> io::Result<T> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude model observation is not a bounded regular file",
        ));
    }
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude model observation must not be a symlink",
        ));
    }
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
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
        ".ilium-claude-statusline-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut file = ilium_platform::secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
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

#[cfg(test)]
pub(crate) fn record_model_observation_for_test(
    config_dir: &Path,
    home: &Path,
    session_id: &str,
    project: &Path,
    model: &str,
) -> io::Result<()> {
    let project = fs::canonicalize(project)?;
    let registry_path = config_dir
        .join("claude-model-statusline")
        .join(REGISTRY_FILE);
    let mut registry = read_registry(&registry_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let generation = "mixed-provider-model-test";
    registry.generation = Some(generation.to_owned());
    registry.projects.insert(
        project_key(&project),
        ProjectState {
            project: project.clone(),
            claude_directory_existed: false,
            local_settings_existed: false,
            previous_local_raw: None,
            previous_effective: None,
            installed: serde_json::json!({}),
        },
    );
    write_registry(&registry_path, &registry)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_observation(
        home,
        &Observation {
            session_id: session_id.to_owned(),
            project,
            model: model.to_owned(),
            generation: generation.to_owned(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statusline_input_retains_only_session_project_and_model() {
        let project = tempfile::tempdir().unwrap();
        let input = serde_json::json!({
            "session_id": "session-1",
            "cwd": project.path(),
            "model": {"id": "claude-opus-4-1"},
            "transcript_path": "/private/transcript.jsonl",
            "prompt": "private prompt"
        });
        let observation = parse_observation(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(observation.session_id, "session-1");
        assert_eq!(
            observation.project,
            fs::canonicalize(project.path()).unwrap()
        );
        assert_eq!(observation.model, "claude-opus-4-1");
        let encoded = serde_json::to_string(&observation).unwrap();
        assert!(!encoded.contains("private prompt"));
        assert!(!encoded.contains("transcript_path"));
    }

    #[test]
    fn stale_model_observation_from_a_previous_activation_is_ignored() {
        let project = tempfile::tempdir().unwrap();
        let project_path = fs::canonicalize(project.path()).unwrap();
        let observation = Observation {
            session_id: "session-1".into(),
            project: project_path.clone(),
            model: "claude-opus-4-1".into(),
            generation: "previous-activation".into(),
        };

        assert_eq!(
            model_for_generation(
                observation.clone(),
                "session-1",
                &project_path,
                "current-activation",
            ),
            None
        );
        assert_eq!(
            model_for_generation(
                observation,
                "session-1",
                &project_path,
                "previous-activation",
            )
            .as_deref(),
            Some("claude-opus-4-1")
        );
    }

    #[test]
    fn stored_model_observations_remain_bounded_as_sessions_accumulate() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let project = fs::canonicalize(project.path()).unwrap();

        for index in 0..(MAX_OBSERVATIONS + 3) {
            write_observation(
                home.path(),
                &Observation {
                    session_id: format!("session-{index}"),
                    project: project.clone(),
                    model: "claude-sonnet-5".into(),
                    generation: "generation-1".into(),
                },
            )
            .unwrap();
            if index == 0 {
                fs::write(
                    observation_path(home.path(), "session-0")
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("unrelated.txt"),
                    "preserve",
                )
                .unwrap();
            }
        }

        let observations = observation_path(home.path(), "session-0")
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let stored_count = fs::read_dir(&observations)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
            .count();
        assert_eq!(stored_count, MAX_OBSERVATIONS);
        assert_eq!(
            fs::read_to_string(observations.join("unrelated.txt")).unwrap(),
            "preserve"
        );
        assert!(
            observation_path(home.path(), &format!("session-{}", MAX_OBSERVATIONS + 2))
                .unwrap()
                .is_file()
        );
    }

    #[test]
    fn project_statusline_setting_chains_user_command_and_restores_only_owned_value() {
        let home = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let user_settings = home.path().join(".claude/settings.json");
        fs::create_dir_all(user_settings.parent().unwrap()).unwrap();
        fs::write(
            &user_settings,
            r#"{"statusLine":{"type":"command","command":"my-statusline"}}"#,
        )
        .unwrap();
        let executable = PathBuf::from("/tmp/ilium cli");

        reconcile_setting_in(
            home.path(),
            config.path(),
            true,
            &[project.path().into()],
            Some(&executable),
            "generation-1",
        )
        .unwrap();
        let local = local_settings_path(project.path());
        let installed = current_statusline(&fs::read_to_string(&local).unwrap())
            .unwrap()
            .unwrap();
        assert!(installed["command"]
            .as_str()
            .unwrap()
            .contains("__claude-model-statusline"));
        assert_eq!(
            read_registry(
                &config
                    .path()
                    .join("claude-model-statusline")
                    .join(REGISTRY_FILE)
            )
            .unwrap()
            .projects
            .len(),
            1
        );

        fs::write(
            &local,
            r#"{"statusLine":{"type":"command","command":"my-statusline"},"theme":"dark"}"#,
        )
        .unwrap();
        reconcile_setting_in(home.path(), config.path(), false, &[], None, "generation-1").unwrap();
        let restored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(local).unwrap()).unwrap();
        assert!(restored.get("statusLine").is_none());
        assert_eq!(restored["theme"], "dark");
        let effective: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(user_settings).unwrap()).unwrap();
        assert_eq!(effective["statusLine"]["command"], "my-statusline");
    }
}
