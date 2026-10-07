//! Metadata-only plugin catalogue and owned settings/permission UI contracts.
//!
//! Browsing never evaluates JavaScript or expands assets. Runtime activation must
//! reopen and fully verify the selected archive; these descriptors confer no trust.

#[cfg(test)]
mod integration_tests;
pub mod permissions;
pub mod preparation;
pub(crate) mod review_bridge;
pub(crate) mod review_controller;

use directories::ProjectDirs;
use ilium_animation_js::{
    manifest::{AnimationMode, Manifest},
    package::{inspect_manifest_reader, PackageLimits},
    release,
    settings::{validate_schema, validate_settings},
};
use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_CATALOGUE_ENTRIES: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationSourceTab {
    #[default]
    Native,
    Plugin,
}

impl AnimationSourceTab {
    /// Browsing changes only UI state, never the active animation.
    pub fn adjacent(self) -> Self {
        match self {
            Self::Native => Self::Plugin,
            Self::Plugin => Self::Native,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SourceTabs {
    pub native: Rect,
    pub plugin: Rect,
    pub body: Rect,
}

pub fn source_tabs(area: Rect) -> SourceTabs {
    let height = area.height.min(1);
    let tabs_width = area.width.min(20);
    let native_width = tabs_width.div_ceil(2);
    let plugin_width = tabs_width.saturating_sub(native_width);
    SourceTabs {
        native: Rect::new(area.x, area.y, native_width, height),
        plugin: Rect::new(
            area.x.saturating_add(native_width),
            area.y,
            plugin_width,
            height,
        ),
        body: Rect::new(
            area.x,
            area.y.saturating_add(area.height.min(2)),
            area.width,
            area.height.saturating_sub(2),
        ),
    }
}

impl SourceTabs {
    pub fn hit_test(self, position: Position) -> Option<AnimationSourceTab> {
        if self.native.contains(position) {
            Some(AnimationSourceTab::Native)
        } else if self.plugin.contains(position) {
            Some(AnimationSourceTab::Plugin)
        } else {
            None
        }
    }
}

pub fn draw_source_tabs(
    frame: &mut Frame<'_>,
    area: Rect,
    selected: AnimationSourceTab,
    style: Style,
) -> Rect {
    let geometry = source_tabs(area);
    for (tab, rectangle, label) in [
        (AnimationSourceTab::Native, geometry.native, "Native"),
        (AnimationSourceTab::Plugin, geometry.plugin, "Plugin"),
    ] {
        let tab_style = if selected == tab {
            style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            style
        };
        frame.render_widget(Paragraph::new(label).style(tab_style), rectangle);
    }
    geometry.body
}

#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub archive_path: PathBuf,
    pub manifest: Manifest,
}

#[derive(Debug, Clone)]
pub struct CatalogueIssue {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginCatalogue {
    pub entries: Vec<PluginDescriptor>,
    pub issues: Vec<CatalogueIssue>,
}

/// The installer owns these directories; discovery does not create them.
pub fn package_directories() -> Result<Vec<PathBuf>, String> {
    let project = ProjectDirs::from("", "", "ilium")
        .ok_or("Ilium data/config directories are unavailable")?;
    let bundled = bundled_package_directory()?;
    let mut paths = vec![bundled];
    let data = project.data_dir().join("animation-plugins");
    if data != paths[0] {
        paths.push(data);
    }
    let configuration = project.config_dir().join("animation-plugins");
    if !paths.contains(&configuration) {
        paths.push(configuration);
    }
    Ok(paths)
}

fn bundled_package_directory() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let helper = ilium_platform::animation_sandbox::helper_executable_path(&current)
        .map_err(|error| error.to_string())?;
    Ok(helper
        .parent()
        .ok_or_else(|| "Animation helper directory unavailable".to_owned())?
        .to_path_buf())
}

fn archive_sha256(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("Animation package must be a regular file, not a symlink".into());
    }
    let limit = PackageLimits::default().archive_bytes;
    if metadata.len() > limit {
        return Err("Animation archive exceeds its byte limit".into());
    }
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut reader = file.take(limit + 1);
    let mut hasher = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let bytes = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if bytes == 0 {
            break;
        }
        count += bytes as u64;
        if count > limit {
            return Err("Animation archive exceeds its byte limit".into());
        }
        hasher.update(&buffer[..bytes]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

impl PluginCatalogue {
    pub fn discover_default() -> Result<Self, String> {
        Ok(Self::discover(&package_directories()?))
    }

    /// Run on an owned I/O worker, rather than on every render/input event.
    /// Symlinks and nested directories are not followed. Conflicting package
    /// IDs are all rejected, making activation independent of directory order.
    pub fn discover(directories: &[PathBuf]) -> Self {
        Self::discover_with_stop(directories, || false)
    }

    pub(crate) fn discover_with_stop(
        directories: &[PathBuf],
        should_stop: impl Fn() -> bool,
    ) -> Self {
        let bundled = bundled_package_directory().ok();
        Self::discover_with_bundled_and_stop(directories, bundled.as_deref(), should_stop)
    }

    fn discover_with_bundled_and_stop(
        directories: &[PathBuf],
        bundled: Option<&Path>,
        should_stop: impl Fn() -> bool,
    ) -> Self {
        let mut catalogue = Self::default();
        let mut paths = BTreeSet::new();
        let mut bundled_paths = BTreeSet::new();
        for directory in directories {
            if should_stop() {
                return catalogue;
            }
            if Some(directory.as_path()) == bundled {
                // Executable directories can contain hundreds of unrelated files.
                // Only these two compiled release identities may enter from there.
                for &(_, filename, expected_digest) in release::PACKAGES {
                    if should_stop() {
                        return catalogue;
                    }
                    let path = directory.join(filename);
                    match fs::symlink_metadata(&path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(error) => catalogue.issues.push(CatalogueIssue {
                            path,
                            message: error.to_string(),
                        }),
                        Ok(_) => match archive_sha256(&path) {
                            Ok(digest) if digest == expected_digest => {
                                bundled_paths.insert(path.clone());
                                paths.insert(path);
                            }
                            Ok(_) => catalogue.issues.push(CatalogueIssue {
                                path,
                                message: "Bundled animation archive does not match the compiled release digest".into(),
                            }),
                            Err(message) => catalogue.issues.push(CatalogueIssue { path, message }),
                        },
                    }
                }
                continue;
            }
            let entries = match fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    catalogue.issues.push(CatalogueIssue {
                        path: directory.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            for (index, entry) in entries.take(MAX_CATALOGUE_ENTRIES + 1).enumerate() {
                if should_stop() {
                    return catalogue;
                }
                if index == MAX_CATALOGUE_ENTRIES {
                    catalogue.issues.push(CatalogueIssue {
                        path: directory.clone(),
                        message: "Animation directory inspection is limited to 256 entries".into(),
                    });
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        catalogue.issues.push(CatalogueIssue {
                            path: directory.clone(),
                            message: error.to_string(),
                        });
                        continue;
                    }
                };
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "iliumanim")
                {
                    paths.insert(path);
                }
            }
            if paths.len() > MAX_CATALOGUE_ENTRIES {
                catalogue.issues.push(CatalogueIssue {
                    path: directory.clone(),
                    message: "Animation catalogue exceeds 256 packages".into(),
                });
                break;
            }
        }
        let mut metadata_bytes = 0usize;
        // The total inspection bound also applies when user directories are full.
        // Reserve the verified release archives before selecting user paths;
        // lexicographic path order must not make an installed package vanish.
        let selected_paths = bundled_paths.iter().cloned().chain(
            paths
                .into_iter()
                .filter(|path| !bundled_paths.contains(path)),
        );
        for path in selected_paths.take(MAX_CATALOGUE_ENTRIES) {
            if should_stop() {
                return catalogue;
            }
            let inspected = (|| -> Result<Manifest, String> {
                let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
                if !metadata.file_type().is_file() {
                    return Err("Animation package must be a regular file, not a symlink".into());
                }
                let file = fs::File::open(&path).map_err(|error| error.to_string())?;
                let length = file.metadata().map_err(|error| error.to_string())?.len();
                inspect_manifest_reader(file, length, PackageLimits::default())
                    .map_err(|error| error.to_string())
            })();
            match inspected {
                Ok(manifest) => {
                    let bytes =
                        serde_json::to_vec(&manifest).map_or(usize::MAX, |bytes| bytes.len());
                    if metadata_bytes.saturating_add(bytes) > 1024 * 1024 {
                        catalogue.issues.push(CatalogueIssue {
                            path,
                            message: "Animation catalogue metadata exceeds 1 MiB".into(),
                        });
                        break;
                    }
                    metadata_bytes += bytes;
                    catalogue.entries.push(PluginDescriptor {
                        archive_path: path,
                        manifest,
                    });
                }
                Err(message) => catalogue.issues.push(CatalogueIssue { path, message }),
            }
        }
        let mut by_id: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, entry) in catalogue.entries.iter().enumerate() {
            by_id
                .entry(entry.manifest.id.clone())
                .or_default()
                .push(index);
        }
        let mut keep = vec![true; catalogue.entries.len()];
        for (id, indices) in by_id {
            if indices.len() < 2 {
                continue;
            }
            let official_digest = release::PACKAGES
                .iter()
                .find_map(|&(official_id, _, digest)| {
                    (official_id == id.as_str()).then_some(digest)
                });
            let identical_official = official_digest.is_some_and(|expected| {
                indices.iter().all(|&index| {
                    archive_sha256(&catalogue.entries[index].archive_path)
                        .is_ok_and(|digest| digest == expected)
                })
            });
            if identical_official {
                // Preserve an existing user-owned copy on disk, but present the
                // bundled path as the one canonical descriptor when available.
                let canonical = indices
                    .iter()
                    .copied()
                    .find(|&index| catalogue.entries[index].archive_path.parent() == bundled)
                    .unwrap_or(indices[0]);
                for index in indices {
                    if index != canonical {
                        keep[index] = false;
                    }
                }
            } else {
                for index in indices {
                    let entry = &catalogue.entries[index];
                    catalogue.issues.push(CatalogueIssue {
                        path: entry.archive_path.clone(),
                        message: format!("Duplicate animation ID: {}", entry.manifest.id),
                    });
                    keep[index] = false;
                }
            }
        }
        catalogue.entries = catalogue
            .entries
            .into_iter()
            .enumerate()
            .filter_map(|(index, entry)| keep[index].then_some(entry))
            .collect();
        catalogue.entries.sort_by(|left, right| {
            left.manifest
                .name
                .cmp(&right.manifest.name)
                .then_with(|| left.manifest.id.cmp(&right.manifest.id))
        });
        catalogue
    }

    pub fn find(&self, id: &str) -> Option<&PluginDescriptor> {
        self.entries.iter().find(|entry| entry.manifest.id == id)
    }
}

/// Persisted intent only. A stored ID/setting does not grant permission or trust.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StoredPluginPreferences")]
pub struct PluginPreferences {
    pub selected: Option<PluginSelection>,
    /// Switching packages preserves each package's authored settings and mode.
    pub remembered: BTreeMap<String, PluginSelection>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StoredPluginPreferences {
    selected: Option<PluginSelection>,
    remembered: BTreeMap<String, PluginSelection>,
}

impl TryFrom<StoredPluginPreferences> for PluginPreferences {
    type Error = String;

    fn try_from(stored: StoredPluginPreferences) -> Result<Self, Self::Error> {
        let preferences = Self {
            selected: stored.selected,
            remembered: stored.remembered,
        };
        preferences.validate_persistence()?;
        Ok(preferences)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSelection {
    pub package_id: String,
    pub mode: AnimationMode,
    pub settings: Value,
}

impl PluginPreferences {
    pub fn validate_persistence(&self) -> Result<(), String> {
        if self.remembered.len() > 128
            || serde_json::to_vec(self)
                .map_err(|error| error.to_string())?
                .len()
                > 512 * 1024
        {
            return Err("Stored plugin preferences exceed their budget".into());
        }
        for (id, selection) in &self.remembered {
            if id != &selection.package_id {
                return Err("Stored plugin identity mismatch".into());
            }
        }
        for selection in self.remembered.values().chain(self.selected.iter()) {
            if selection.package_id.is_empty()
                || selection.package_id.len() > 80
                || !selection
                    .package_id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                || !selection.settings.is_object()
                || serde_json::to_vec(&selection.settings)
                    .map_err(|error| error.to_string())?
                    .len()
                    > 64 * 1024
            {
                return Err("Invalid stored plugin preferences".into());
            }
        }
        Ok(())
    }

    /// Returns validated remembered values, or new defaults. A changed schema
    /// reports its incompatibility rather than silently discarding authored data.
    pub fn selection_for(&self, descriptor: &PluginDescriptor) -> Result<PluginSelection, String> {
        self.validate_persistence()?;
        if let Some(selection) = self
            .selected
            .as_ref()
            .filter(|selection| selection.package_id == descriptor.manifest.id)
            .or_else(|| self.remembered.get(&descriptor.manifest.id))
        {
            return selection.validate(descriptor);
        }
        let mode = if descriptor.manifest.modes.contains(&AnimationMode::Live) {
            AnimationMode::Live
        } else {
            descriptor
                .manifest
                .modes
                .first()
                .cloned()
                .ok_or("Plugin has no playback mode")?
        };
        PluginSelection::new(descriptor, mode, Value::Object(Map::new()))
    }

    pub fn activate_selection(
        &mut self,
        descriptor: &PluginDescriptor,
        selection: PluginSelection,
    ) -> Result<(), String> {
        let selection = selection.validate(descriptor)?;
        let mut candidate = self.clone();
        if let Some(previous) = &candidate.selected {
            candidate
                .remembered
                .insert(previous.package_id.clone(), previous.clone());
        }
        candidate
            .remembered
            .insert(selection.package_id.clone(), selection.clone());
        candidate.selected = Some(selection);
        candidate.validate_persistence()?;
        *self = candidate;
        Ok(())
    }
}

/// Retained prompt/dropdown target. The runtime supplies full package digests;
/// descriptor metadata never authenticates a package by itself.
#[derive(Debug, Clone)]
pub struct PluginControlFence {
    pub package_digest: String,
    pub selection: PluginSelection,
    pub schema: Value,
}

#[derive(Debug, Clone)]
pub enum PluginEditorKind {
    Choice {
        options: Vec<PluginChoice>,
        cursor: usize,
    },
    Number,
    Text {
        max_length: usize,
    },
}

#[derive(Debug, Clone)]
pub struct PluginEditor {
    pub fence: PluginControlFence,
    pub control_id: String,
    pub label: String,
    pub kind: PluginEditorKind,
    pub input: String,
    pub error: Option<String>,
}

impl PluginEditor {
    pub fn new(fence: PluginControlFence, control_id: &str) -> Result<Self, String> {
        let controls = project_controls(&fence.schema, &fence.selection.settings)?;
        let control = controls
            .into_iter()
            .find(|control| control.id == control_id)
            .ok_or("Plugin control changed")?;
        let input = control.value.as_ref().map_or_else(String::new, |value| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        });
        let kind = match control.kind {
            PluginControlKind::Choice { options } => {
                let cursor = options
                    .iter()
                    .position(|option| control.value.as_ref() == Some(&option.value))
                    .unwrap_or(0);
                PluginEditorKind::Choice { options, cursor }
            }
            PluginControlKind::Number { .. } => PluginEditorKind::Number,
            PluginControlKind::Text { max_length } => PluginEditorKind::Text { max_length },
            PluginControlKind::Toggle => return Err("Toggle has no edit dialog".into()),
        };
        Ok(Self {
            fence,
            control_id: control.id,
            label: control.label,
            kind,
            input,
            error: None,
        })
    }
    pub fn value(&self) -> Result<Value, String> {
        match &self.kind {
            PluginEditorKind::Choice { options, cursor } => options
                .get(*cursor)
                .map(|option| option.value.clone())
                .ok_or_else(|| "Plugin option disappeared".into()),
            PluginEditorKind::Text { .. } => Ok(Value::String(self.input.clone())),
            PluginEditorKind::Number => serde_json::from_str::<Value>(&self.input)
                .ok()
                .filter(Value::is_number)
                .ok_or_else(|| "Enter a finite number".into()),
        }
    }
}

impl PluginControlFence {
    pub fn new(
        package_digest: String,
        descriptor: &PluginDescriptor,
        preferences: &PluginPreferences,
    ) -> Result<Self, String> {
        if package_digest.len() != 64
            || !package_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Plugin control requires a verified package identity".into());
        }
        let selection = preferences.selected.as_ref().ok_or("No plugin selected")?;
        selection.validate(descriptor)?;
        let selection = selection.clone();
        Ok(Self {
            package_digest,
            selection,
            schema: descriptor.manifest.settings.clone(),
        })
    }

    pub fn commit(
        &self,
        current_digest: &str,
        descriptor: &PluginDescriptor,
        preferences: &mut PluginPreferences,
        control_id: &str,
        value: Value,
    ) -> Result<(), String> {
        if current_digest != self.package_digest
            || descriptor.manifest.settings != self.schema
            || preferences.selected.as_ref() != Some(&self.selection)
        {
            return Err("Plugin package, settings or selection changed; reopen the control".into());
        }
        let settings =
            apply_control_value(&self.schema, &self.selection.settings, control_id, value)?;
        let selection = PluginSelection::new(descriptor, self.selection.mode.clone(), settings)?;
        preferences.activate_selection(descriptor, selection)
    }
}

impl PluginSelection {
    pub fn new(
        descriptor: &PluginDescriptor,
        mode: AnimationMode,
        settings: Value,
    ) -> Result<Self, String> {
        if !descriptor.manifest.modes.contains(&mode) {
            return Err("This package does not support that playback mode".into());
        }
        let settings = validate_settings(&descriptor.manifest.settings, &settings)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            package_id: descriptor.manifest.id.clone(),
            mode,
            settings,
        })
    }

    pub fn validate(&self, descriptor: &PluginDescriptor) -> Result<Self, String> {
        if self.package_id != descriptor.manifest.id {
            return Err("The selected plugin identity changed".into());
        }
        Self::new(descriptor, self.mode.clone(), self.settings.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginChoice {
    /// Serialized primitive value, stable across reordering the enum array.
    pub id: String,
    pub label: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginControlKind {
    Toggle,
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
        step: f64,
    },
    Choice {
        options: Vec<PluginChoice>,
    },
    Text {
        max_length: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginControl {
    pub id: String,
    pub label: String,
    pub help: String,
    pub kind: PluginControlKind,
    pub value: Option<Value>,
}

fn display_text(text: &str, maximum: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(maximum)
        .collect()
}

pub fn project_controls(schema: &Value, authored: &Value) -> Result<Vec<PluginControl>, String> {
    validate_schema(schema).map_err(|error| error.to_string())?;
    let values = authored
        .as_object()
        .ok_or("Plugin settings must be an object")?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("Plugin schema has no properties")?;
    properties
        .iter()
        .map(|(id, definition)| {
            let kind = if let Some(options) = definition.get("enum").and_then(Value::as_array) {
                let mut ids = BTreeSet::new();
                let options = options
                    .iter()
                    .map(|value| {
                        let id = serde_json::to_string(value).map_err(|error| error.to_string())?;
                        if !ids.insert(id.clone()) {
                            return Err("Duplicate plugin option identity".into());
                        }
                        let label = value
                            .as_str()
                            .map_or_else(|| value.to_string(), str::to_owned);
                        Ok(PluginChoice {
                            id,
                            label: display_text(&label, 160),
                            value: value.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                PluginControlKind::Choice { options }
            } else {
                match definition.get("type").and_then(Value::as_str) {
                    Some("boolean") => PluginControlKind::Toggle,
                    Some("integer" | "number") => {
                        let integer =
                            definition.get("type").and_then(Value::as_str) == Some("integer");
                        let step = definition
                            .get("multipleOf")
                            .and_then(Value::as_f64)
                            .filter(|step| step.is_finite() && *step > 0.0)
                            .unwrap_or(1.0);
                        PluginControlKind::Number {
                            integer,
                            minimum: definition.get("minimum").and_then(Value::as_f64),
                            maximum: definition.get("maximum").and_then(Value::as_f64),
                            step,
                        }
                    }
                    Some("string") => PluginControlKind::Text {
                        max_length: definition
                            .get("maxLength")
                            .and_then(Value::as_u64)
                            .unwrap_or(4096)
                            .min(4096) as usize,
                    },
                    _ => return Err("Unsupported plugin control".into()),
                }
            };
            Ok(PluginControl {
                id: id.clone(),
                label: display_text(
                    definition
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or(id),
                    160,
                ),
                help: display_text(
                    definition
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    4096,
                ),
                kind,
                value: values
                    .get(id)
                    .or_else(|| definition.get("default"))
                    .cloned(),
            })
        })
        .collect()
}

/// Changes are atomic and validated against the complete current schema.
pub fn apply_control_value(
    schema: &Value,
    authored: &Value,
    id: &str,
    value: Value,
) -> Result<Value, String> {
    let mut values: Map<String, Value> = authored
        .as_object()
        .cloned()
        .ok_or("Plugin settings must be an object")?;
    if !schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| properties.contains_key(id))
    {
        return Err("The plugin control no longer exists".into());
    }
    values.insert(id.to_owned(), value);
    validate_settings(schema, &Value::Object(values)).map_err(|error| error.to_string())
}

pub fn apply_choice(
    schema: &Value,
    authored: &Value,
    control_id: &str,
    choice_id: &str,
) -> Result<Value, String> {
    let controls = project_controls(schema, authored)?;
    let control = controls
        .iter()
        .find(|control| control.id == control_id)
        .ok_or("Plugin control changed")?;
    let PluginControlKind::Choice { options } = &control.kind else {
        return Err("Control is not a choice".into());
    };
    let option = options
        .iter()
        .find(|option| option.id == choice_id)
        .ok_or("Plugin option changed")?;
    apply_control_value(schema, authored, control_id, option.value.clone())
}

#[derive(Debug, Clone)]
pub enum PluginControlEdit {
    Values(Value),
    TextPrompt {
        id: String,
        label: String,
        current: String,
    },
}

/// Keyboard arrows and pointer steppers use the same bounded calculation.
pub fn adjust_control(
    schema: &Value,
    authored: &Value,
    control_id: &str,
    delta: i32,
) -> Result<PluginControlEdit, String> {
    let controls = project_controls(schema, authored)?;
    let control = controls
        .iter()
        .find(|control| control.id == control_id)
        .ok_or("Plugin control changed")?;
    let value = match &control.kind {
        PluginControlKind::Toggle => Value::Bool(
            !control
                .value
                .as_ref()
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        PluginControlKind::Choice { options } => {
            let index = options
                .iter()
                .position(|option| control.value.as_ref() == Some(&option.value))
                .unwrap_or(0);
            let next = (index as i64 + i64::from(delta)).rem_euclid(options.len() as i64) as usize;
            options[next].value.clone()
        }
        PluginControlKind::Number {
            integer,
            minimum,
            maximum,
            step,
        } => {
            let current = control
                .value
                .as_ref()
                .and_then(Value::as_f64)
                .unwrap_or(minimum.unwrap_or(0.0));
            let step = if *integer {
                step.round().max(1.0)
            } else {
                *step
            };
            let mut next = current + step * f64::from(delta);
            if let Some(minimum) = minimum {
                next = next.max(*minimum);
            }
            if let Some(maximum) = maximum {
                next = next.min(*maximum);
            }
            if *integer {
                next = next.round();
            }
            let number = serde_json::Number::from_f64(next)
                .ok_or("Plugin number exceeds supported range")?;
            Value::Number(number)
        }
        PluginControlKind::Text { .. } => {
            return Ok(PluginControlEdit::TextPrompt {
                id: control.id.clone(),
                label: control.label.clone(),
                current: control
                    .value
                    .as_ref()
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
    };
    Ok(PluginControlEdit::Values(apply_control_value(
        schema, authored, control_id, value,
    )?))
}

pub fn adjacent_mode(
    descriptor: &PluginDescriptor,
    selection: &PluginSelection,
    delta: i32,
) -> Result<PluginSelection, String> {
    let current = selection.validate(descriptor)?;
    let modes = &descriptor.manifest.modes;
    let index = modes
        .iter()
        .position(|mode| mode == &current.mode)
        .ok_or("Playback mode changed")?;
    let next = (index as i64 + i64::from(delta)).rem_euclid(modes.len() as i64) as usize;
    PluginSelection::new(descriptor, modes[next].clone(), current.settings)
}

#[derive(Debug, Clone, Default)]
pub struct PluginPanelState {
    pub cursor: usize,
    pub scroll: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginPanelRow {
    Package(String),
    Playback,
    Control(String),
    /// Index into the same native RowModel; only Common rows are projected.
    Common(usize),
    Permissions,
    Issues,
    Status,
}

#[derive(Debug, Clone)]
pub struct PluginPanelModel {
    pub rows: Vec<PluginPanelRow>,
    pub labels: Vec<String>,
    pub issue_details: Vec<String>,
}

impl PluginPanelModel {
    pub fn new(catalogue: &PluginCatalogue, preferences: &PluginPreferences) -> Self {
        Self::with_active(catalogue, preferences, None)
    }

    pub fn with_active(
        catalogue: &PluginCatalogue,
        preferences: &PluginPreferences,
        active_package_id: Option<&str>,
    ) -> Self {
        let mut model = Self {
            rows: Vec::new(),
            labels: Vec::new(),
            issue_details: catalogue
                .issues
                .iter()
                .map(|issue| {
                    format!(
                        "{}: {}",
                        issue.path.display(),
                        display_text(&issue.message, 512)
                    )
                })
                .collect(),
        };
        for entry in &catalogue.entries {
            let active = active_package_id == Some(entry.manifest.id.as_str());
            model
                .rows
                .push(PluginPanelRow::Package(entry.manifest.id.clone()));
            model.labels.push(format!(
                "{} {}  {}",
                if active { "●" } else { "○" },
                entry.manifest.name,
                entry.manifest.version
            ));
        }
        if let Some(selection) = &preferences.selected {
            if let Some(entry) = catalogue.find(&selection.package_id) {
                model.rows.push(PluginPanelRow::Playback);
                model.labels.push(format!(
                    "Playback: {}",
                    match selection.mode {
                        AnimationMode::Live => "Live",
                        AnimationMode::PreRendered => "Pre-rendered",
                    }
                ));
                match project_controls(&entry.manifest.settings, &selection.settings) {
                    Ok(controls) => {
                        for control in controls {
                            model.rows.push(PluginPanelRow::Control(control.id));
                            let value = control.value.map_or_else(
                                || "Required value missing".into(),
                                |value| {
                                    value
                                        .as_str()
                                        .map_or_else(|| value.to_string(), str::to_owned)
                                },
                            );
                            model.labels.push(format!(
                                "{}: {}",
                                control.label,
                                display_text(&value, 160)
                            ));
                        }
                    }
                    Err(error) => {
                        model.rows.push(PluginPanelRow::Status);
                        model.labels.push(error);
                    }
                }
                model.rows.push(PluginPanelRow::Permissions);
                model.labels.push("Permissions · review or revoke".into());
            } else {
                model.rows.push(PluginPanelRow::Status);
                model.labels.push(format!(
                    "Selected package unavailable: {}",
                    display_text(&selection.package_id, 80)
                ));
            }
        }
        if model.rows.is_empty() {
            model.rows.push(PluginPanelRow::Status);
            model.labels.push("No animation packages installed".into());
        }
        if !catalogue.issues.is_empty() {
            model.rows.push(PluginPanelRow::Issues);
            model.labels.push(format!(
                "{} package inspection issue(s)",
                catalogue.issues.len()
            ));
        }
        model
    }
}

impl PluginPanelState {
    pub fn move_cursor(&mut self, delta: i32, count: usize, visible: usize) {
        if count == 0 {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        self.cursor = self
            .cursor
            .saturating_add_signed(delta as isize)
            .min(count - 1);
        self.follow(count, visible);
    }

    pub fn follow(&mut self, count: usize, visible: usize) {
        self.cursor = self.cursor.min(count.saturating_sub(1));
        let visible = visible.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        if self.cursor >= self.scroll.saturating_add(visible) {
            self.scroll = self.cursor + 1 - visible;
        }
        self.scroll = self.scroll.min(count.saturating_sub(visible));
    }
}

pub fn plugin_row_rect(area: Rect, scroll: usize, row: usize) -> Option<Rect> {
    let relative = row.checked_sub(scroll)?;
    if relative >= usize::from(area.height) || area.width == 0 {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y.saturating_add(relative as u16),
        area.width,
        1,
    ))
}

pub fn plugin_row_at(area: Rect, scroll: usize, count: usize, position: Position) -> Option<usize> {
    if !area.contains(position) {
        return None;
    }
    let row = scroll.checked_add(usize::from(position.y - area.y))?;
    (row < count).then_some(row)
}

pub fn draw_plugin_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &PluginPanelModel,
    state: &PluginPanelState,
    style: Style,
) {
    for (index, label) in model
        .labels
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(usize::from(area.height))
    {
        let Some(rectangle) = plugin_row_rect(area, state.scroll, index) else {
            continue;
        };
        let row_style = if state.cursor == index {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(label.as_str(), row_style))),
            rectangle,
        );
    }
}

pub fn draw_plugin_issue_popover(
    frame: &mut Frame<'_>,
    content_area: Rect,
    app: &crate::app::App,
    model: &PluginPanelModel,
    state: &PluginPanelState,
) {
    let Some(hover) = app.plugin_issue_hover.filter(|hover| hover.is_shown) else {
        return;
    };
    let Some(row) = model
        .rows
        .iter()
        .position(|row| *row == PluginPanelRow::Issues)
    else {
        return;
    };
    if hover.row != row {
        return;
    }
    let area = crate::animation_settings_ui::plugin_panel_area(content_area);
    let Some(anchor) = plugin_row_rect(area, state.scroll, row) else {
        return;
    };
    let body = model.issue_details.join("\n\n");
    let Some(geometry) = crate::animation_hover::popover_geometry(
        content_area,
        crate::animation_settings_ui::layout(content_area).panel,
        anchor.y,
        &body,
    ) else {
        return;
    };
    let ink = crate::animation_settings_ui::control_ink(app);
    frame.render_widget(ratatui::widgets::Clear, geometry.rectangle);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(geometry.lines.join("\n"))
            .style(ink)
            .block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title(" Package inspection details ")
                    .style(ink),
            ),
        geometry.rectangle,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use serde_json::json;

    fn descriptor() -> PluginDescriptor {
        let manifest: Manifest = serde_json::from_value(json!({
            "api_version":1,"id":"carpet","name":"Carpet","version":"1.0.0",
            "entry":"entry.mjs","modes":["live","pre_rendered"],"files":[],
            "settings":{"type":"object","properties":{
                "mode":{"type":"string","title":"Pattern","enum":["snake","life"],"default":"snake"},
                "speed":{"type":"integer","minimum":1,"maximum":10,"default":3},
                "enabled":{"type":"boolean","default":true}
            }}
        })).expect("valid test manifest");
        PluginDescriptor {
            archive_path: PathBuf::from("carpet.iliumanim"),
            manifest,
        }
    }

    #[test]
    fn choice_identity_survives_reordering_and_rejects_removed_options() {
        let entry = descriptor();
        let mut schema = entry.manifest.settings;
        let controls = project_controls(&schema, &json!({})).expect("controls");
        let control = controls
            .iter()
            .find(|control| control.id == "mode")
            .expect("mode");
        let PluginControlKind::Choice { options } = &control.kind else {
            panic!("expected choice");
        };
        let life = options
            .iter()
            .find(|option| option.value == "life")
            .expect("life");
        schema["properties"]["mode"]["enum"] = json!(["life", "snake"]);
        assert_eq!(
            apply_choice(&schema, &json!({}), "mode", &life.id).expect("reordered")["mode"],
            "life"
        );
        schema["properties"]["mode"]["enum"] = json!(["snake"]);
        assert!(apply_choice(&schema, &json!({}), "mode", &life.id).is_err());
    }

    #[test]
    fn plugin_panel_retains_catalogue_issue_details_for_hover_report() {
        let catalogue = PluginCatalogue {
            entries: Vec::new(),
            issues: vec![CatalogueIssue {
                path: PathBuf::from("/tmp/broken.iliumanim"),
                message: "manifest is missing entry.mjs".into(),
            }],
        };
        let model = PluginPanelModel::new(&catalogue, &PluginPreferences::default());
        assert!(model
            .rows
            .iter()
            .any(|row| matches!(row, PluginPanelRow::Issues)));
        assert_eq!(
            model.issue_details,
            vec!["/tmp/broken.iliumanim: manifest is missing entry.mjs"]
        );
    }

    #[test]
    fn plugin_issue_popover_renders_both_package_paths_and_failure_reasons() {
        let project = tempfile::tempdir().expect("project");
        let mut app = crate::app::App::new("plugin-issues-test".into(), project.path().into());
        let catalogue = PluginCatalogue {
            entries: Vec::new(),
            issues: vec![
                CatalogueIssue {
                    path: PathBuf::from("beach.iliumanim"),
                    message: "release digest mismatch".into(),
                },
                CatalogueIssue {
                    path: PathBuf::from("carpet.iliumanim"),
                    message: "entry.mjs is missing".into(),
                },
            ],
        };
        let model = PluginPanelModel::new(&catalogue, &PluginPreferences::default());
        let row = model
            .rows
            .iter()
            .position(|row| *row == PluginPanelRow::Issues)
            .unwrap();
        app.plugin_issue_hover = Some(crate::animation_hover::AnimationHover {
            row,
            since: std::time::Instant::now(),
            is_shown: true,
        });
        let mut terminal = Terminal::new(TestBackend::new(160, 30)).expect("terminal");
        terminal
            .draw(|frame| {
                draw_plugin_issue_popover(
                    frame,
                    frame.area(),
                    &app,
                    &model,
                    &PluginPanelState::default(),
                );
            })
            .expect("draw");
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for detail in [
            "Package inspection details",
            "beach.iliumanim",
            "release digest mismatch",
            "carpet.iliumanim",
            "entry.mjs is missing",
        ] {
            assert!(
                rendered.contains(detail),
                "missing rendered detail: {detail}"
            );
        }

        // Pending hover and stale row identity must not expose unrelated errors.
        let shown = app.plugin_issue_hover.unwrap();
        for hover in [
            crate::animation_hover::AnimationHover {
                is_shown: false,
                ..shown
            },
            crate::animation_hover::AnimationHover {
                row: row + 1,
                ..shown
            },
        ] {
            app.plugin_issue_hover = Some(hover);
            terminal
                .draw(|frame| {
                    draw_plugin_issue_popover(
                        frame,
                        frame.area(),
                        &app,
                        &model,
                        &PluginPanelState::default(),
                    );
                })
                .expect("hidden draw");
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(!rendered.contains("beach.iliumanim"));
            assert!(!rendered.contains("carpet.iliumanim"));
        }
    }

    #[test]
    fn edits_validate_the_entire_schema_and_do_not_mutate_authored_values() {
        let entry = descriptor();
        let authored = json!({"mode":"life","speed":3});
        assert!(
            apply_control_value(&entry.manifest.settings, &authored, "speed", json!(11)).is_err()
        );
        assert!(
            apply_control_value(&entry.manifest.settings, &authored, "unknown", json!(1)).is_err()
        );
        assert_eq!(authored, json!({"mode":"life","speed":3}));
        assert_eq!(
            apply_control_value(&entry.manifest.settings, &authored, "speed", json!(5))
                .expect("valid")["enabled"],
            true
        );
    }

    #[test]
    fn persisted_selection_is_intent_and_revalidates_mode_identity_and_values() {
        let entry = descriptor();
        let selected =
            PluginSelection::new(&entry, AnimationMode::Live, json!({})).expect("defaults");
        let text = serde_json::to_string(&PluginPreferences {
            selected: Some(selected),
            ..PluginPreferences::default()
        })
        .expect("serialize");
        let restored: PluginPreferences = serde_json::from_str(&text).expect("deserialize");
        let mut selected = restored.selected.expect("selection");
        assert!(selected.validate(&entry).is_ok());
        selected.package_id = "replacement".into();
        assert!(selected.validate(&entry).is_err());
        assert!(!text.contains("allow"));
    }

    #[test]
    fn switching_packages_preserves_settings_and_stale_prompt_cannot_commit() {
        let carpet = descriptor();
        let mut beach = descriptor();
        beach.manifest.id = "beach".into();
        let mut preferences = PluginPreferences::default();
        preferences
            .activate_selection(
                &carpet,
                PluginSelection::new(&carpet, AnimationMode::Live, json!({"speed":7}))
                    .expect("carpet"),
            )
            .expect("activate carpet");
        let fence = PluginControlFence::new("a".repeat(64), &carpet, &preferences).expect("fence");
        preferences
            .activate_selection(
                &beach,
                preferences.selection_for(&beach).expect("beach defaults"),
            )
            .expect("activate beach");
        assert!(fence
            .commit(
                &"a".repeat(64),
                &carpet,
                &mut preferences,
                "speed",
                json!(4)
            )
            .is_err());
        let remembered = preferences.selection_for(&carpet).expect("restore carpet");
        assert_eq!(remembered.settings["speed"], 7);
        preferences
            .activate_selection(&carpet, remembered)
            .expect("restore");
        assert!(fence
            .commit(
                &"b".repeat(64),
                &carpet,
                &mut preferences,
                "speed",
                json!(4)
            )
            .is_err());
        fence
            .commit(
                &"a".repeat(64),
                &carpet,
                &mut preferences,
                "speed",
                json!(4),
            )
            .expect("current exact fence");
        assert_eq!(
            preferences.selected.as_ref().expect("selected").settings["speed"],
            4
        );
        assert!(fence
            .commit(
                &"a".repeat(64),
                &carpet,
                &mut preferences,
                "speed",
                json!(8)
            )
            .is_err());
    }

    #[test]
    fn persisted_preferences_reject_mismatched_keys_and_large_setting_blobs() {
        let entry = descriptor();
        let selection =
            PluginSelection::new(&entry, AnimationMode::Live, json!({})).expect("selection");
        let mismatched = json!({"remembered":{"beach":selection}});
        assert!(serde_json::from_value::<PluginPreferences>(mismatched).is_err());
        let oversized = json!({"selected":{"package_id":"carpet","mode":"live","settings":{"text":"x".repeat(65536)}}});
        assert!(serde_json::from_value::<PluginPreferences>(oversized).is_err());
    }

    #[test]
    fn control_steppers_preserve_bounds_and_playback_modes_are_manifest_driven() {
        let entry = descriptor();
        let PluginControlEdit::Values(values) =
            adjust_control(&entry.manifest.settings, &json!({}), "speed", 100).expect("step")
        else {
            panic!("numeric edit");
        };
        assert_eq!(values["speed"].as_f64(), Some(10.0));
        let selected =
            PluginSelection::new(&entry, AnimationMode::Live, values).expect("selection");
        assert_eq!(
            adjacent_mode(&entry, &selected, -1).expect("mode").mode,
            AnimationMode::PreRendered
        );
        let mut unsupported = entry.clone();
        unsupported.manifest.modes = vec![AnimationMode::Live];
        assert_eq!(
            adjacent_mode(&unsupported, &selected, 1)
                .expect("only supported mode")
                .mode,
            AnimationMode::Live
        );
    }

    #[test]
    fn narrow_tabs_and_scrolled_rows_share_render_and_pointer_geometry() {
        for width in [2, 8, 20, 40, 80, 120] {
            let area = Rect::new(3, 2, width, 8);
            let geometry = source_tabs(area);
            assert_eq!(
                geometry.hit_test(Position::new(geometry.native.x, geometry.native.y)),
                Some(AnimationSourceTab::Native)
            );
            assert_eq!(
                geometry.hit_test(Position::new(geometry.plugin.x, geometry.plugin.y)),
                Some(AnimationSourceTab::Plugin)
            );
            assert_eq!(
                geometry.hit_test(Position::new(area.x, geometry.body.y)),
                None
            );
            for row in 4..8 {
                let rectangle = plugin_row_rect(geometry.body, 4, row).expect("visible");
                assert_eq!(
                    plugin_row_at(geometry.body, 4, 8, Position::new(rectangle.x, rectangle.y)),
                    Some(row)
                );
            }
        }
    }

    #[test]
    fn browsing_plugin_tab_does_not_change_selection_and_actual_widgets_render() {
        let catalogue = PluginCatalogue {
            entries: vec![descriptor()],
            issues: vec![],
        };
        let preferences = PluginPreferences::default();
        let model = PluginPanelModel::new(&catalogue, &preferences);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("terminal");
        terminal
            .draw(|frame| {
                let body = draw_source_tabs(
                    frame,
                    frame.area(),
                    AnimationSourceTab::Plugin,
                    Style::default(),
                );
                draw_plugin_panel(
                    frame,
                    body,
                    &model,
                    &PluginPanelState::default(),
                    Style::default(),
                );
            })
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(rendered.contains("Native"));
        assert!(rendered.contains("Plugin"));
        assert!(rendered.contains("Carpet"));
        assert!(preferences.selected.is_none());
    }

    #[test]
    fn catalogue_reports_invalid_archives_without_evaluating_them() {
        let temporary = std::env::temp_dir().join(format!(
            "ilium-plugin-catalogue-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&temporary).expect("unique test directory");
        let path = temporary.join("broken.iliumanim");
        fs::write(&path, b"throw new Error('must never execute')").expect("invalid package");
        let catalogue = PluginCatalogue::discover(std::slice::from_ref(&temporary));
        assert!(catalogue.entries.is_empty());
        assert_eq!(catalogue.issues.len(), 1);
        assert_eq!(catalogue.issues[0].path, path);
        fs::remove_file(&path).expect("remove exact owned fixture");
        fs::remove_dir(&temporary).expect("remove empty owned directory");
    }

    #[test]
    fn installed_catalogue_searches_bundled_payload_before_user_packages() {
        let directories = package_directories().expect("release catalogue directories");
        let current = std::env::current_exe().expect("client executable location");
        let helper = ilium_platform::animation_sandbox::helper_executable_path(&current)
            .expect("platform helper location");
        let bundled = helper.parent().expect("helper directory").to_path_buf();
        assert_eq!(directories.first(), Some(&bundled));
        let project = ProjectDirs::from("", "", "ilium").expect("project directories");
        assert!(directories.contains(&project.data_dir().join("animation-plugins")));
        assert!(directories.contains(&project.config_dir().join("animation-plugins")));
    }

    fn copy_approved_archives(directory: &Path) {
        fs::create_dir_all(directory).expect("create owned catalogue fixture");
        for &(_, filename, _) in release::PACKAGES {
            let original = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ilium-animation-js/assets/packages")
                .join(filename);
            fs::copy(original, directory.join(filename)).expect("copy exact approved archive");
        }
    }

    #[test]
    fn bundled_lookup_ignores_more_than_256_unrelated_executable_directory_entries() {
        let temporary = tempfile::tempdir().expect("owned catalogue fixture");
        let bundled = temporary.path().join("bin");
        copy_approved_archives(&bundled);
        for index in 0..300 {
            fs::write(bundled.join(format!("unrelated-{index}")), b"unrelated")
                .expect("create irrelevant executable-directory entry");
        }
        let catalogue = PluginCatalogue::discover_with_bundled_and_stop(
            std::slice::from_ref(&bundled),
            Some(&bundled),
            || false,
        );
        assert_eq!(catalogue.entries.len(), release::PACKAGES.len());
        assert!(catalogue.issues.is_empty(), "{:?}", catalogue.issues);
        for &(id, filename, _) in release::PACKAGES {
            assert_eq!(
                catalogue.find(id).map(|entry| entry.archive_path.as_path()),
                Some(bundled.join(filename).as_path())
            );
        }
    }

    #[test]
    fn bundled_packages_keep_slots_when_256_valid_user_archives_sort_first() {
        let temporary = tempfile::tempdir().expect("owned catalogue fixture");
        let user = temporary.path().join("a-user-animation-plugins");
        let bundled = temporary.path().join("z-bin");
        assert!(user < bundled);
        copy_approved_archives(&bundled);
        fs::create_dir(&user).expect("create owned user catalogue fixture");
        for index in 0..MAX_CATALOGUE_ENTRIES {
            let filename = release::PACKAGES[index % release::PACKAGES.len()].1;
            let approved = bundled.join(filename);
            fs::copy(approved, user.join(format!("user-{index:03}.iliumanim")))
                .expect("copy valid approved user archive");
        }
        assert_eq!(
            fs::read_dir(&user).expect("user entries").count(),
            MAX_CATALOGUE_ENTRIES
        );

        // Production searches the bundled directory first, but its absolute
        // path sorts after the user directory in the combined path set.
        let directories = [bundled.clone(), user];
        let catalogue =
            PluginCatalogue::discover_with_bundled_and_stop(&directories, Some(&bundled), || false);
        assert!(catalogue.entries.len() <= MAX_CATALOGUE_ENTRIES);
        assert!(catalogue
            .issues
            .iter()
            .any(|issue| issue.message == "Animation catalogue exceeds 256 packages"));
        for &(id, filename, _) in release::PACKAGES {
            assert_eq!(
                catalogue.find(id).map(|entry| entry.archive_path.as_path()),
                Some(bundled.join(filename).as_path()),
                "verified bundled {id} must survive a full user catalogue"
            );
        }

        let conflicting_path = directories[1].join("user-000.iliumanim");
        let mut conflicting = fs::read(&conflicting_path).expect("owned user archive");
        assert_eq!(&conflicting[..4], b"PK\x03\x04");
        conflicting[10] ^= 1;
        fs::write(&conflicting_path, &conflicting).expect("replace owned fixture copy");
        let manifest = inspect_manifest_reader(
            std::io::Cursor::new(&conflicting),
            conflicting.len() as u64,
            PackageLimits::default(),
        )
        .expect("conflicting user archive remains valid");
        assert_eq!(manifest.id, release::PACKAGES[0].0);
        let conflicted =
            PluginCatalogue::discover_with_bundled_and_stop(&directories, Some(&bundled), || false);
        assert!(conflicted.find(release::PACKAGES[0].0).is_none());
        assert_eq!(
            conflicted
                .find(release::PACKAGES[1].0)
                .map(|entry| entry.archive_path.as_path()),
            Some(bundled.join(release::PACKAGES[1].1).as_path())
        );
        assert_eq!(
            fs::read(conflicting_path).expect("preserved conflicting user archive"),
            conflicting
        );
    }

    #[test]
    fn exact_official_user_copies_coalesce_but_conflicting_id_is_rejected() {
        let temporary = tempfile::tempdir().expect("owned catalogue fixture");
        let bundled = temporary.path().join("bin");
        let user = temporary.path().join("user-animation-plugins");
        copy_approved_archives(&bundled);
        copy_approved_archives(&user);
        let directories = [bundled.clone(), user.clone()];
        let catalogue =
            PluginCatalogue::discover_with_bundled_and_stop(&directories, Some(&bundled), || false);
        assert_eq!(catalogue.entries.len(), release::PACKAGES.len());
        assert!(catalogue.issues.is_empty(), "{:?}", catalogue.issues);
        for &(id, filename, _) in release::PACKAGES {
            assert_eq!(
                catalogue.find(id).map(|entry| entry.archive_path.as_path()),
                Some(bundled.join(filename).as_path())
            );
        }

        // Change only the local ZIP header timestamp. The manifest remains a
        // valid same-ID package, while its archive digest no longer has trust.
        let beach_name = release::PACKAGES[0].1;
        let user_beach = user.join(beach_name);
        let mut conflicting = fs::read(&user_beach).expect("read owned user copy");
        assert_eq!(&conflicting[..4], b"PK\x03\x04");
        conflicting[10] ^= 1;
        fs::write(&user_beach, &conflicting).expect("replace owned fixture copy");
        let manifest = inspect_manifest_reader(
            std::io::Cursor::new(&conflicting),
            conflicting.len() as u64,
            PackageLimits::default(),
        )
        .expect("conflicting archive remains a valid same-ID package");
        assert_eq!(manifest.id, "beach");
        let conflicted =
            PluginCatalogue::discover_with_bundled_and_stop(&directories, Some(&bundled), || false);
        assert!(conflicted.find("beach").is_none());
        assert!(conflicted.find("carpet").is_some());
        assert_eq!(
            conflicted
                .issues
                .iter()
                .filter(|issue| issue.message == "Duplicate animation ID: beach")
                .count(),
            2
        );
        assert_eq!(
            fs::read(user_beach).expect("preserved user copy"),
            conflicting
        );
    }

    #[test]
    #[ignore = "must be explicitly run against a real installed native payload"]
    fn installed_official_catalogue_resolves_both_compiled_packages() {
        let root = PathBuf::from(
            std::env::var_os("ILIUM_INSTALLED_ANIMATION_ROOT").expect("installed root"),
        );
        assert!(root.is_absolute());
        let client = root.join(format!("ilium{}", std::env::consts::EXE_SUFFIX));
        let helper = ilium_platform::animation_sandbox::helper_executable_path(&client)
            .expect("production helper path");
        assert_eq!(helper.parent(), Some(root.as_path()));
        assert!(fs::symlink_metadata(helper)
            .expect("installed helper")
            .file_type()
            .is_file());
        let catalogue = PluginCatalogue::discover_with_bundled_and_stop(
            std::slice::from_ref(&root),
            Some(&root),
            || false,
        );
        assert!(catalogue.issues.is_empty(), "{:?}", catalogue.issues);
        assert_eq!(catalogue.entries.len(), release::PACKAGES.len());
        for &(id, filename, digest) in release::PACKAGES {
            let descriptor = catalogue.find(id).expect("approved installed animation");
            assert_eq!(descriptor.archive_path, root.join(filename));
            assert_eq!(
                archive_sha256(&descriptor.archive_path).expect("archive hash"),
                digest
            );
        }
        println!(
            "{}",
            serde_json::json!({"type":"artifact","gate":"installed_catalogue",
                "packages": release::PACKAGES.iter().map(|entry| entry.0).collect::<Vec<_>>()})
        );
    }
}
