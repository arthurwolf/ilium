//! Local selection, admission caching and presentation identity for Semantic animations.
use super::{App, Mode, RightPanelTarget};
use crate::background_animation::{AnimationKind, AnimationSettings, SemanticScope};
use ilium_core::animation_recommendation::{AnimationRecommendation, AnimationValue};
use ilium_core::NodeId;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
#[derive(Debug, Clone, PartialEq, Eq)]
struct Selection {
    project_id: NodeId,
    entry_id: NodeId,
    path: PathBuf,
}
struct Resolution {
    settings: Option<Rc<AnimationSettings>>,
    error: Option<String>,
    label: String,
    semantic: bool,
}
struct CachedResolution {
    selection: Option<Selection>,
    tree_version: u64,
    generation: u64,
    recommendation: Option<AnimationRecommendation>,
    authored: AnimationSettings,
    bound_path: PathBuf,
    load_error: Option<String>,
    resolution: Rc<Resolution>,
}
pub(super) struct SemanticPresentation {
    bound_path: PathBuf,
    load_error: Option<String>,
    cache: RefCell<Option<CachedResolution>>,
    presented: Option<Rc<Resolution>>,
    field_settings: Option<Rc<AnimationSettings>>,
}
impl SemanticPresentation {
    pub(super) fn new(bound_path: PathBuf) -> Self {
        Self {
            bound_path,
            load_error: None,
            cache: RefCell::new(None),
            presented: None,
            field_settings: None,
        }
    }
}
impl App {
    fn animation_selection(&self) -> Option<Selection> {
        let resolve = |entry_id: NodeId| -> Option<Selection> {
            let project_id = self.tree.project_ancestor(entry_id)?;
            let path = self.tree.get(project_id)?.project_path()?.to_path_buf();
            Some(Selection {
                project_id,
                entry_id,
                path,
            })
        };
        for entry_id in self.tree_state.selected().iter().rev().copied() {
            if let Some(selection) = resolve(entry_id) {
                return Some(selection);
            }
        }
        let fallback = self.active_pane_id().or(match &self.right_panel_target {
            RightPanelTarget::SplitView { split_id, .. } => Some(*split_id),
            RightPanelTarget::Chatroom { project_id } => Some(*project_id),
            _ => None,
        });
        if let Some(selection) = fallback.and_then(resolve) {
            return Some(selection);
        }
        let projects = self.tree.project_ids();
        if projects.len() != 1 {
            return None;
        }
        resolve(projects[0])
    }
    fn animation_project_path(&self) -> PathBuf {
        self.animation_selection()
            .map_or_else(|| self.session_cwd.clone(), |selection| selection.path)
    }
    pub(crate) fn install_animation_project_settings(
        &mut self,
        path: PathBuf,
        result: Result<AnimationSettings, String>,
    ) {
        self.semantic_presentation.bound_path = path;
        self.semantic_presentation.load_error = match result {
            Ok(settings) => {
                self.animation_settings = settings;
                self.committed_animation_settings = None;
                None
            }
            Err(error) => Some(error),
        };
        self.semantic_presentation.cache.get_mut().take();
    }
    pub(crate) fn synchronize_animation_project_settings(&mut self) -> bool {
        let path = self.animation_project_path();
        let mut changed = false;
        if path != self.semantic_presentation.bound_path {
            if matches!(
                self.mode,
                Mode::AnimationTextPrompt(..) | Mode::LocationPicker(_)
            ) {
                return self.reconcile_animation_presentation();
            }
            self.semantic_presentation.bound_path = path.clone();
            self.semantic_presentation.load_error =
                Some("Project animation settings are loading".into());
            self.semantic_presentation.cache.get_mut().take();
            let result = self
                .configuration_files
                .as_mut()
                .ok_or_else(|| "Project file worker unavailable".to_owned())
                .and_then(|files| files.request_project(path));
            if let Err(error) = result {
                self.semantic_presentation.load_error = Some(error);
            }
            changed = true;
        }
        self.reconcile_animation_presentation() || changed
    }
    pub(crate) fn animation_project_binding(&self) -> PathBuf {
        self.semantic_presentation.bound_path.clone()
    }
    pub(crate) fn animation_write_path(&self) -> Result<PathBuf, String> {
        let path = self.animation_project_path();
        if path != self.semantic_presentation.bound_path {
            return Err(
                "Animation edit target changed; reopen the edit for the selected project".into(),
            );
        }
        if let Some(error) = &self.semantic_presentation.load_error {
            return Err(error.clone());
        }
        Ok(path)
    }
    pub(crate) fn prepare_animation_edit(&mut self) -> bool {
        self.synchronize_animation_project_settings();
        if let Err(error) = self.animation_write_path() {
            self.status_message = Some(error);
            return false;
        }
        true
    }
    fn animation_resolution(&self) -> Rc<Resolution> {
        let authored = self
            .committed_animation_settings
            .as_ref()
            .unwrap_or(&self.animation_settings);
        let selection = self.animation_selection();
        let owner = selection
            .as_ref()
            .map(|selection| match authored.semantic_scope {
                SemanticScope::Project => selection.project_id,
                SemanticScope::Entry => selection.entry_id,
            });
        let recommendation = owner
            .and_then(|id| self.tree.get(id))
            .and_then(|node| node.inferred_animation.as_ref());
        let generation = selection
            .as_ref()
            .and_then(|selection| self.tree.get(selection.project_id))
            .map_or(0, |node| node.animation_generation);
        let mut cache = self.semantic_presentation.cache.borrow_mut();
        if let Some(cached) = cache.as_ref() {
            if cached.selection == selection
                && cached.tree_version == self.tree_version
                && cached.generation == generation
                && cached.recommendation.as_ref() == recommendation
                && cached.authored == *authored
                && cached.bound_path == self.semantic_presentation.bound_path
                && cached.load_error == self.semantic_presentation.load_error
            {
                return Rc::clone(&cached.resolution);
            }
        }
        let semantic = authored.kind == AnimationKind::Semantic;
        let result = if let Some(error) = &self.semantic_presentation.load_error {
            Err(error.clone())
        } else if !semantic {
            Ok(authored.clone())
        } else if selection
            .as_ref()
            .is_some_and(|selection| selection.path != self.semantic_presentation.bound_path)
        {
            Err("Semantic: selected project animation settings are not loaded".into())
        } else if selection.is_none() {
            Err("Semantic: select a project or entry".into())
        } else if let Some(recommendation) = recommendation {
            crate::semantic_animation::resolve_recommendation(recommendation, authored)
                .map_err(|error| format!("Semantic: {error}"))
        } else {
            Err(format!(
                "Semantic: no {} recommendation; reorganize this project to create one",
                if authored.semantic_scope == SemanticScope::Project {
                    "project"
                } else {
                    "entry"
                }
            ))
        };
        let (settings, error) = match result {
            Ok(settings) => (Some(Rc::new(settings)), None),
            Err(error) => (None, Some(error)),
        };
        let mut label = String::new();
        if semantic {
            if let Some(settings) = &settings {
                let scope = if authored.semantic_scope == SemanticScope::Project {
                    "project"
                } else {
                    "entry"
                };
                label = format!(
                    "Semantic {scope} #{}: {}",
                    owner.map_or(0, |id| id.0),
                    settings.kind.label()
                );
                if let Some(recommendation) = recommendation {
                    for parameter in &recommendation.parameters {
                        let value = match &parameter.value {
                            AnimationValue::Number(value) => value.to_string(),
                            AnimationValue::Bool(value) => value.to_string(),
                            AnimationValue::Choice { label, .. } => label.clone(),
                        };
                        label.push_str(&format!("; {}={value}", parameter.id));
                    }
                }
            }
        }
        let resolution = Rc::new(Resolution {
            settings,
            error,
            label,
            semantic,
        });
        *cache = Some(CachedResolution {
            selection,
            tree_version: self.tree_version,
            generation,
            recommendation: recommendation.cloned(),
            authored: authored.clone(),
            bound_path: self.semantic_presentation.bound_path.clone(),
            load_error: self.semantic_presentation.load_error.clone(),
            resolution: Rc::clone(&resolution),
        });
        resolution
    }
    pub(crate) fn effective_animation_settings(&self) -> Option<Rc<AnimationSettings>> {
        self.animation_resolution().settings.clone()
    }
    pub(crate) fn effective_animation_kind(&self) -> Option<AnimationKind> {
        self.animation_resolution()
            .settings
            .as_ref()
            .map(|settings| settings.kind)
    }
    pub(crate) fn semantic_animation_error(&self) -> Option<String> {
        let resolution = self.animation_resolution();
        if let Some(error) = &resolution.error {
            return Some(error.clone());
        }
        if resolution.semantic
            && self.effective_animation_kind() == Some(AnimationKind::OpenStreetMap)
            && !self.layout.screen_area.is_empty()
            && crate::layout::osm_attribution_area(self.layout.screen_area).is_empty()
        {
            return Some(
                "Semantic: terminal is too small for the map and its OpenStreetMap attribution"
                    .into(),
            );
        }
        None
    }
    fn animation_field_matches(&self, settings: &AnimationSettings) -> bool {
        self.animation_settings.kind != AnimationKind::Semantic
            || self.semantic_presentation.field_settings.as_deref() == Some(settings)
    }
    pub(crate) fn note_animation_field_settings(&mut self) {
        let settings = self.effective_animation_settings();
        self.semantic_presentation.field_settings = settings;
    }
    pub(super) fn animation_wants_attribution(&self) -> bool {
        self.effective_animation_settings().is_some_and(|settings| {
            settings.enabled && settings.kind == AnimationKind::OpenStreetMap
        })
    }
    pub(crate) fn reconcile_animation_presentation(&mut self) -> bool {
        let next = self.animation_resolution();
        let previous = self.semantic_presentation.presented.clone();
        let changed = previous
            .as_ref()
            .is_none_or(|previous| !Rc::ptr_eq(previous, &next));
        let semantic = next.semantic || previous.as_ref().is_some_and(|previous| previous.semantic);
        let settings_changed = previous
            .as_ref()
            .and_then(|previous| previous.settings.as_deref())
            != next.settings.as_deref();
        let old_kind = previous
            .as_ref()
            .and_then(|previous| previous.settings.as_ref())
            .map(|settings| settings.kind);
        let new_kind = next.settings.as_ref().map(|settings| settings.kind);
        if changed && (next.settings.is_none() || (semantic && settings_changed)) {
            self.semantic_presentation.field_settings = None;
            if next.settings.is_none() || old_kind != new_kind {
                self.animation_frame.release_hosts();
            }
        }
        if semantic
            && next
                .settings
                .as_ref()
                .is_some_and(|settings| !settings.enabled)
            && !self.is_animation_preview_visible()
        {
            self.animation_frame.release_hosts();
            self.semantic_presentation.field_settings = None;
        }
        if settings_changed && self.animation_frame.has_requested() {
            if let Some(settings) = next
                .settings
                .as_ref()
                .filter(|settings| settings.enabled || self.is_animation_preview_visible())
            {
                if let Err(error) = self.animation_frame.request(
                    settings,
                    self.layout.screen_area.width,
                    self.layout.screen_area.height,
                    crate::background_composition::quantized_elapsed_at(
                        self.started_at.elapsed(),
                        self.animation_frames_per_second(),
                    ),
                    None,
                ) {
                    self.status_message = Some(format!("Animation transition pending: {error:?}"));
                }
            }
        }
        self.semantic_presentation.presented = Some(next);
        let layout = self.layout_for_animation(
            self.layout.screen_area,
            self.tree_width_animation.current_width(),
        );
        let layout_changed = layout != self.layout;
        if layout_changed {
            self.set_layout(layout, ilium_ipc::PaneResizeCause::UserInterfaceSettings);
        }
        changed || layout_changed
    }
    pub(super) fn effective_animation_frames_per_second(&self) -> u32 {
        let Some(settings) = self.effective_animation_settings() else {
            return 1;
        };
        let scene = if settings.kind == AnimationKind::Wikipedia {
            4
        } else if settings.kind.is_ambient() && self.animation_field_matches(&settings) {
            self.animation_frame
                .frames_per_second()
                .unwrap_or(crate::background_composition::DEFAULT_FRAMES_PER_SECOND)
        } else {
            crate::background_composition::DEFAULT_FRAMES_PER_SECOND
        };
        if settings.fps_limit == 0 {
            return scene;
        }
        scene.min(u32::from(settings.fps_limit.clamp(1, 30)))
    }
    pub(super) fn effective_animation_row_context(&self) -> crate::animation_rows::RowContext {
        let resolution = self.animation_resolution();
        let settings = resolution.settings.as_deref();
        let kind = settings.map(|settings| settings.kind);
        let current = settings.is_some_and(|settings| self.animation_field_matches(settings));
        let runtime_status =
            if kind == Some(AnimationKind::Wikipedia) && !self.animation_frame.is_wikipedia() {
                Some("Loading Wikipedia".to_owned())
            } else if current {
                self.animation_frame.status()
            } else {
                None
            };
        let scene_status = if let Some(error) = self.semantic_animation_error() {
            Some(error)
        } else if resolution.semantic {
            let mut text = resolution.label.clone();
            text.push_str(if self.animation_settings.enabled {
                "; background enabled"
            } else {
                "; background disabled"
            });
            if self.is_animation_preview_visible() {
                text.push_str("; preview active");
            }
            if let Some(status) = runtime_status {
                text.push_str("; ");
                text.push_str(&status);
            }
            Some(text)
        } else {
            runtime_status
        };
        let uses_loop = settings.is_some_and(AnimationSettings::uses_loop_cache);
        crate::animation_rows::RowContext {
            effective_kind: kind,
            scene_uses_cell_colors: kind == Some(AnimationKind::Wikipedia)
                || (current && self.animation_frame.has_cell_colors()),
            scene_status,
            cache: if uses_loop {
                self.animation_frame.cache_status()
            } else {
                Default::default()
            },
            loop_bytes: settings.filter(|_| uses_loop).map_or(0, |settings| {
                settings.estimated_loop_bytes(
                    self.layout.screen_area.width,
                    self.layout.screen_area.height,
                )
            }),
        }
    }
}
#[cfg(test)]
#[path = "app_semantic_animation_tests.rs"]
mod tests;
