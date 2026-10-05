//! Shared chrome for the plugin panel; native schema and editor own mutations.
use crate::animation_plugins::{
    PluginControl, PluginControlKind, PluginPanelModel, PluginPanelRow, PluginPanelState,
};
use crate::app::App;
use crate::value_control::{ControlKind, ControlSpec, ValueControl};
use ratatui::layout::Rect;

pub(crate) fn schema_control(area: Rect, control: &PluginControl) -> Option<ValueControl> {
    let value = control.value.as_ref().map_or_else(String::new, |value| {
        value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned)
    });
    let (kind, previous_enabled, next_enabled) = match &control.kind {
        PluginControlKind::Choice { options } => {
            (ControlKind::Choice, options.len() > 1, options.len() > 1)
        }
        PluginControlKind::Number {
            minimum, maximum, ..
        } => {
            let current = control.value.as_ref().and_then(serde_json::Value::as_f64);
            (
                ControlKind::Number,
                current.is_none_or(|value| minimum.is_none_or(|minimum| value > minimum)),
                current.is_none_or(|value| maximum.is_none_or(|maximum| value < maximum)),
            )
        }
        _ => return None,
    };
    Some(ValueControl::new(
        area,
        ControlSpec {
            kind,
            label: &control.label,
            value: &value,
            label_width: 24,
            previous_enabled,
            next_enabled,
            open_enabled: true,
        },
    ))
}

/// Prepare schema and common-row metadata once for this paint/input snapshot.
pub(crate) struct PanelValues {
    schema: Vec<PluginControl>,
    playback: Option<(
        Vec<ilium_animation_js::manifest::AnimationMode>,
        ilium_animation_js::manifest::AnimationMode,
    )>,
    native: crate::animation_rows::RowModel,
}
impl PanelValues {
    pub(crate) fn new(app: &App) -> Self {
        let schema = app
            .animation_settings
            .plugin
            .selected
            .as_ref()
            .and_then(|selection| {
                let descriptor = app
                    .plugin_catalogue
                    .as_ref()?
                    .view()
                    .find(&selection.package_id)?;
                crate::animation_plugins::project_controls(
                    &descriptor.manifest.settings,
                    &selection.settings,
                )
                .ok()
            })
            .unwrap_or_default();
        let playback = app
            .animation_settings
            .plugin
            .selected
            .as_ref()
            .and_then(|selection| {
                let descriptor = app
                    .plugin_catalogue
                    .as_ref()?
                    .view()
                    .find(&selection.package_id)?;
                Some((descriptor.manifest.modes.clone(), selection.mode.clone()))
            });
        Self {
            schema,
            playback,
            native: app.animation_row_model(),
        }
    }
    pub(crate) fn control(
        &self,
        area: Rect,
        model: &PluginPanelModel,
        state: &PluginPanelState,
        row: usize,
    ) -> Option<ValueControl> {
        let rectangle = crate::animation_plugins::plugin_row_rect(area, state.scroll, row)?;
        match model.rows.get(row)? {
            PluginPanelRow::Playback => {
                let (modes, current) = self.playback.as_ref()?;
                Some(ValueControl::new(
                    rectangle,
                    ControlSpec {
                        kind: ControlKind::Choice,
                        label: "Playback",
                        value: mode_label(current),
                        label_width: 24,
                        previous_enabled: modes.len() > 1,
                        next_enabled: modes.len() > 1,
                        open_enabled: true,
                    },
                ))
            }
            PluginPanelRow::Control(id) => schema_control(
                rectangle,
                self.schema.iter().find(|control| &control.id == id)?,
            ),
            PluginPanelRow::Common(index) => {
                let view = self.native.view(*index)?;
                let metadata = self.native.control(*index)?;
                let (kind, previous_enabled, next_enabled) = match view.kind {
                    crate::animation_rows::RowKind::Slider(spec) => (
                        ControlKind::Number,
                        spec.value > spec.minimum,
                        spec.value < spec.maximum,
                    ),
                    crate::animation_rows::RowKind::Choice => (
                        ControlKind::Choice,
                        metadata
                            .stepped(-1)
                            .is_some_and(|value| value != metadata.value),
                        metadata
                            .stepped(1)
                            .is_some_and(|value| value != metadata.value),
                    ),
                    _ => return None,
                };
                Some(ValueControl::new(
                    rectangle,
                    ControlSpec {
                        kind,
                        label: &view.label,
                        value: &view.value,
                        label_width: 24,
                        previous_enabled,
                        next_enabled,
                        open_enabled: true,
                    },
                ))
            }
            _ => None,
        }
    }
}

pub(crate) fn panel_control(
    app: &App,
    area: Rect,
    model: &PluginPanelModel,
    state: &PluginPanelState,
    row: usize,
) -> Option<ValueControl> {
    PanelValues::new(app).control(area, model, state, row)
}

#[derive(Clone, Debug, PartialEq)]
pub enum PluginField {
    Playback(Vec<ilium_animation_js::manifest::AnimationMode>),
    Control(String),
}

pub struct PluginTarget {
    pub project: std::path::PathBuf,
    pub fence: crate::animation_plugins::PluginControlFence,
    pub field: PluginField,
    /// Only this child's admitted authored selection may rebase a manual retry.
    pub pending: Option<crate::animation_plugins::PluginSelection>,
}
impl PluginTarget {
    pub(crate) fn binding_is_current(&self, app: &App) -> bool {
        let Some(crate::app::Mode::Settings(parent)) = app.modal_stack.last() else {
            return false;
        };
        let schema_matches = app
            .plugin_catalogue
            .as_ref()
            .and_then(|catalogue| catalogue.view().find(&self.fence.selection.package_id))
            .is_some_and(|descriptor| {
                descriptor.manifest.settings == self.fence.schema
                    && match &self.field {
                        PluginField::Playback(modes) => &descriptor.manifest.modes == modes,
                        PluginField::Control(_) => true,
                    }
            });
        parent.tab == crate::app::SettingsTab::Animations
            && parent.animation_source_tab == crate::animation_plugins::AnimationSourceTab::Plugin
            && app.animation_write_path().as_ref() == Ok(&self.project)
            && app
                .plugin_panel_model()
                .rows
                .get(parent.plugin_panel.cursor)
                .is_some_and(|row| self.row_matches(row))
            && schema_matches
            && app
                .animation_settings
                .plugin
                .selected
                .as_ref()
                .is_some_and(|selection| {
                    selection == &self.fence.selection || self.pending.as_ref() == Some(selection)
                })
    }
    fn row_matches(&self, row: &PluginPanelRow) -> bool {
        match (&self.field, row) {
            (PluginField::Playback(_), PluginPanelRow::Playback) => true,
            (PluginField::Control(id), PluginPanelRow::Control(current)) => id == current,
            _ => false,
        }
    }
    pub(crate) fn dialog(&self) -> Result<crate::value_dialog::ValueDialogState, String> {
        use crate::value_dialog::{
            ChoiceDialogState, ChoiceOption, NumberDialogState, ValueDialogState,
        };
        match &self.field {
            PluginField::Playback(modes) => Ok(ValueDialogState::Choice(ChoiceDialogState::new(
                "Plugin playback",
                modes
                    .iter()
                    .map(|mode| ChoiceOption {
                        id: format!("{mode:?}"),
                        label: mode_label(mode).into(),
                        disabled_reason: None,
                    })
                    .collect(),
                Some(format!("{:?}", self.fence.selection.mode)),
            )?)),
            PluginField::Control(id) => {
                let editor = crate::animation_plugins::PluginEditor::new(self.fence.clone(), id)?;
                match editor.kind {
                    crate::animation_plugins::PluginEditorKind::Choice { options, cursor } => {
                        let selected = options.get(cursor).map(|option| option.id.clone());
                        Ok(ValueDialogState::Choice(ChoiceDialogState::new(
                            editor.label,
                            options
                                .into_iter()
                                .map(|option| ChoiceOption {
                                    id: option.id,
                                    label: option.label,
                                    disabled_reason: None,
                                })
                                .collect(),
                            selected,
                        )?))
                    }
                    crate::animation_plugins::PluginEditorKind::Number => {
                        Ok(ValueDialogState::Number(NumberDialogState::new(
                            editor.label,
                            editor.input,
                        )))
                    }
                    _ => Err("This field uses its native text editor".into()),
                }
            }
        }
    }
    pub(crate) fn prepare(
        &self,
        project: &std::path::Path,
        digest: &str,
        descriptor: &crate::animation_plugins::PluginDescriptor,
        preferences: &crate::animation_plugins::PluginPreferences,
        outcome: &crate::value_dialog::DialogOutcome,
    ) -> Result<crate::animation_plugins::PluginPreferences, String> {
        use crate::value_dialog::DialogOutcome;
        if project != self.project
            || digest != self.fence.package_digest
            || descriptor.manifest.id != self.fence.selection.package_id
            || descriptor.manifest.settings != self.fence.schema
        {
            return Err("Plugin project, package or schema changed; reopen this dialog".into());
        }
        let current = preferences
            .selected
            .as_ref()
            .ok_or("Plugin selection disappeared")?;
        if current != &self.fence.selection && self.pending.as_ref() != Some(current) {
            return Err("Plugin settings changed; reopen this dialog".into());
        }
        let mut fence = self.fence.clone();
        fence.selection = current.clone();
        let mut desired = preferences.clone();
        match (&self.field, outcome) {
            (PluginField::Playback(modes), DialogOutcome::Choose(id)) => {
                if &descriptor.manifest.modes != modes {
                    return Err("Playback catalog changed; reopen this dialog".into());
                }
                let mode = modes
                    .iter()
                    .find(|mode| format!("{mode:?}") == *id)
                    .ok_or("Playback mode disappeared")?
                    .clone();
                let selection = crate::animation_plugins::PluginSelection::new(
                    descriptor,
                    mode,
                    current.settings.clone(),
                )?;
                desired.activate_selection(descriptor, selection)?;
            }
            (PluginField::Control(control_id), DialogOutcome::Choose(id)) => {
                let mut editor =
                    crate::animation_plugins::PluginEditor::new(fence.clone(), control_id)?;
                let crate::animation_plugins::PluginEditorKind::Choice { options, cursor } =
                    &mut editor.kind
                else {
                    return Err("Plugin control type changed".into());
                };
                *cursor = options
                    .iter()
                    .position(|option| &option.id == id)
                    .ok_or("Plugin option disappeared")?;
                fence.commit(
                    digest,
                    descriptor,
                    &mut desired,
                    control_id,
                    editor.value()?,
                )?;
            }
            (PluginField::Control(control_id), DialogOutcome::CommitNumber(text)) => {
                let mut editor =
                    crate::animation_plugins::PluginEditor::new(fence.clone(), control_id)?;
                if !matches!(
                    editor.kind,
                    crate::animation_plugins::PluginEditorKind::Number
                ) {
                    return Err("Plugin control type changed".into());
                }
                editor.input = text.clone();
                fence.commit(
                    digest,
                    descriptor,
                    &mut desired,
                    control_id,
                    editor.value()?,
                )?;
            }
            _ => return Err("Choose a value for this plugin control".into()),
        }
        Ok(desired)
    }
}
fn mode_label(mode: &ilium_animation_js::manifest::AnimationMode) -> &'static str {
    match mode {
        ilium_animation_js::manifest::AnimationMode::Live => "Live",
        ilium_animation_js::manifest::AnimationMode::PreRendered => "Pre-rendered",
    }
}

impl App {
    /// Called after the durable receipt arrives, before native acknowledgement
    /// advances the desired revision. The next frame may still be pending.
    pub(crate) fn validate_plugin_value_receipt(&self) -> Result<(), String> {
        let crate::app::Mode::ValueDialog(host) = &self.mode else {
            return Ok(());
        };
        let crate::value_dialog_host::ValueTarget::Plugin(target) = &host.target else {
            return Ok(());
        };
        if !target.binding_is_current(self)
            || self
                .plugin_value_package_digest(&target.fence.selection.package_id)
                .as_deref()
                != Some(target.fence.package_digest.as_str())
        {
            return Err("Plugin binding changed while saving; reopen this dialog".into());
        }
        Ok(())
    }

    pub(crate) fn begin_plugin_value_dialog(&mut self, row: usize) {
        use crate::value_dialog::ValueDialogState;
        use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
        let result = (|| {
            if !self.prepare_animation_edit() {
                return Err("Animation settings are not ready to edit".into());
            }
            let selected = self
                .animation_settings
                .plugin
                .selected
                .as_ref()
                .ok_or("No plugin selected")?;
            let descriptor = self
                .plugin_catalogue
                .as_ref()
                .and_then(|catalogue| catalogue.view().find(&selected.package_id))
                .ok_or("Plugin package disappeared")?;
            let field = match self.plugin_panel_model().rows.get(row) {
                Some(PluginPanelRow::Playback) => {
                    PluginField::Playback(descriptor.manifest.modes.clone())
                }
                Some(PluginPanelRow::Control(id)) => PluginField::Control(id.clone()),
                _ => return Err("This is not a plugin value control".into()),
            };
            let digest = self
                .plugin_value_package_digest(&selected.package_id)
                .ok_or("Plugin identity is still loading; retry when ready")?;
            let target = PluginTarget {
                project: self.animation_write_path()?,
                fence: crate::animation_plugins::PluginControlFence::new(
                    digest,
                    descriptor,
                    &self.animation_settings.plugin,
                )?,
                field,
                pending: None,
            };
            let dialog = target.dialog()?;
            Ok(match dialog {
                ValueDialogState::Choice(dialog) => {
                    ValueDialogHost::choice_host(ValueTarget::Plugin(target), dialog)
                }
                ValueDialogState::Number(dialog) => {
                    ValueDialogHost::number_host(ValueTarget::Plugin(target), dialog)
                }
            })
        })();
        match result {
            Ok(host) => self.push_modal(crate::app::Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }
    pub(crate) fn commit_plugin_value_dialog(
        &mut self,
        host: &mut crate::value_dialog_host::ValueDialogHost,
        outcome: &crate::value_dialog::DialogOutcome,
    ) -> Result<(), String> {
        use crate::app::{Mode, SettingsTab};
        if host.is_saving() {
            return Err("The previous plugin write is still pending".into());
        }
        let crate::value_dialog_host::ValueTarget::Plugin(target) = &mut host.target else {
            return Err("This is not a plugin dialog".into());
        };
        let Some(Mode::Settings(parent)) = self.modal_stack.last() else {
            return Err("Plugin settings parent changed".into());
        };
        if parent.tab != SettingsTab::Animations
            || parent.animation_source_tab != crate::animation_plugins::AnimationSourceTab::Plugin
            || !self
                .plugin_panel_model()
                .rows
                .get(parent.plugin_panel.cursor)
                .is_some_and(|row| target.row_matches(row))
        {
            return Err("Plugin control binding changed; reopen this dialog".into());
        }
        if !self.prepare_animation_edit() {
            return Err("Animation settings are not ready to edit".into());
        }
        let descriptor = self
            .plugin_catalogue
            .as_ref()
            .and_then(|catalogue| catalogue.view().find(&target.fence.selection.package_id))
            .ok_or("Plugin package disappeared")?;
        let digest = self
            .plugin_value_package_digest(&target.fence.selection.package_id)
            .ok_or("Plugin execution identity is unavailable")?;
        let desired = target.prepare(
            &self.animation_write_path()?,
            &digest,
            descriptor,
            &self.animation_settings.plugin,
            outcome,
        )?;
        let selection = desired.selected.clone();
        let mut settings = self.animation_settings.clone();
        settings.plugin = desired;
        let token = std::sync::Arc::new(());
        self.enqueue_animation_settings_with_value(settings, None, Some(token.clone()))?;
        target.pending = selection;
        host.begin_save(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_control::{ControlAction, ControlStyles, PointerButton};
    use ratatui::{backend::TestBackend, layout::Position, Terminal};
    use serde_json::json;
    #[test]
    fn schema_choice_and_number_paint_exact_shared_targets_with_native_bounds() {
        let schema = json!({"type":"object", "properties": {
            "palette": {"type":"string", "enum":["海", "λ", "Sunset"]},
            "speed": {"type":"number", "minimum":0.25, "maximum":4.5, "multipleOf":0.25}
        }});
        let controls = crate::animation_plugins::project_controls(
            &schema,
            &json!({"palette":"海", "speed":0.25}),
        )
        .unwrap();
        for width in [32, 60, 100] {
            let mut terminal = Terminal::new(TestBackend::new(width, 3)).unwrap();
            for (row, metadata) in controls.iter().enumerate() {
                let control = schema_control(Rect::new(0, row as u16, width, 1), metadata).unwrap();
                let geometry = control.geometry();
                terminal
                    .draw(|frame| control.render(frame, ControlStyles::default()))
                    .unwrap();
                let glyphs = if metadata.id == "palette" {
                    ["←", "→", "+"]
                } else {
                    ["−", "+", "*"]
                };
                for (rect, glyph) in [
                    (geometry.previous, glyphs[0]),
                    (geometry.next, glyphs[1]),
                    (geometry.open, glyphs[2]),
                ] {
                    assert_eq!(
                        terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                        glyph
                    );
                }
                if metadata.id == "palette" {
                    let value = Position::new(geometry.value.x, geometry.value.y);
                    assert_eq!(
                        control.hit(value, PointerButton::Left),
                        Some(ControlAction::NextChoice)
                    );
                    assert_eq!(
                        control.hit(value, PointerButton::Right),
                        Some(ControlAction::PreviousChoice)
                    );
                    assert_eq!(
                        control.hit(
                            Position::new(geometry.open.x, geometry.open.y),
                            PointerButton::Left
                        ),
                        Some(ControlAction::OpenChoices)
                    );
                } else {
                    assert_eq!(
                        control.hit(
                            Position::new(geometry.previous.x, geometry.previous.y),
                            PointerButton::Left
                        ),
                        None
                    );
                    assert_eq!(
                        control.hit(
                            Position::new(geometry.next.x, geometry.next.y),
                            PointerButton::Left
                        ),
                        Some(ControlAction::Increment)
                    );
                    assert_eq!(
                        control.hit(
                            Position::new(geometry.open.x, geometry.open.y),
                            PointerButton::Left
                        ),
                        Some(ControlAction::EditNumber)
                    );
                    assert_eq!(
                        geometry.value.x - geometry.value_slot.x,
                        (geometry.value_slot.width - geometry.value.width) / 2
                    );
                }
            }
        }
    }
    #[test]
    fn text_and_toggle_controls_keep_native_editors_without_fake_numeric_chrome() {
        let schema = json!({"type":"object","properties":{"name":{"type":"string"},"enabled":{"type":"boolean"}}});
        for control in crate::animation_plugins::project_controls(
            &schema,
            &json!({"name":"authored", "enabled":true}),
        )
        .unwrap()
        {
            assert!(schema_control(Rect::new(0, 0, 80, 1), &control).is_none());
        }
    }
    fn plugin_fixture(
        field: PluginField,
    ) -> (
        PluginTarget,
        crate::animation_plugins::PluginDescriptor,
        crate::animation_plugins::PluginPreferences,
    ) {
        let descriptor = crate::animation_plugins::PluginDescriptor {
            archive_path: std::path::PathBuf::from("synthetic-not-a-runtime-package.iliumanim"),
            manifest: serde_json::from_value(json!({"api_version":1,"id":"synthetic-controls","name":"Synthetic controls","version":"1.0.0","entry":"entry.mjs","modes":["live","pre_rendered"],"files":[],"settings":{"type":"object","properties":{
                "speed":{"type":"integer","minimum":1,"maximum":10,"default":3},
                "palette":{"type":"string","enum":["海","λ","Sunset"],"default":"海"},
                "authored":{"type":"string","default":"keep"}
            }}})).unwrap(),
        };
        let mut preferences = crate::animation_plugins::PluginPreferences::default();
        let selection = crate::animation_plugins::PluginSelection::new(
            &descriptor,
            ilium_animation_js::manifest::AnimationMode::Live,
            json!({"speed":3,"palette":"海","authored":"Unicode λ retained"}),
        )
        .unwrap();
        preferences
            .activate_selection(&descriptor, selection)
            .unwrap();
        let fence = crate::animation_plugins::PluginControlFence::new(
            "a".repeat(64),
            &descriptor,
            &preferences,
        )
        .unwrap();
        (
            PluginTarget {
                project: std::path::PathBuf::from("/synthetic/project"),
                fence,
                field,
                pending: None,
            },
            descriptor,
            preferences,
        )
    }
    #[test]
    fn playback_catalog_lists_all_declared_modes_and_preserves_authored_settings() {
        let (mut target, descriptor, preferences) = plugin_fixture(PluginField::Playback(vec![]));
        target.field = PluginField::Playback(descriptor.manifest.modes.clone());
        let crate::value_dialog::ValueDialogState::Choice(dialog) = target.dialog().unwrap() else {
            panic!("playback catalog");
        };
        assert_eq!(dialog.options().len(), descriptor.manifest.modes.len());
        assert_eq!(dialog.selected_id.as_deref(), Some("Live"));
        let desired = target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &preferences,
                &crate::value_dialog::DialogOutcome::Choose("PreRendered".into()),
            )
            .unwrap();
        assert_eq!(
            desired.selected.as_ref().unwrap().settings,
            preferences.selected.as_ref().unwrap().settings
        );
        assert_eq!(
            desired.selected.as_ref().unwrap().mode,
            ilium_animation_js::manifest::AnimationMode::PreRendered
        );
        assert!(target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &preferences,
                &crate::value_dialog::DialogOutcome::Choose("not-declared".into())
            )
            .is_err());
        let mut changed = descriptor.clone();
        changed.manifest.modes = vec![ilium_animation_js::manifest::AnimationMode::Live];
        assert!(target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &changed,
                &preferences,
                &crate::value_dialog::DialogOutcome::Choose("Live".into())
            )
            .is_err());
    }
    #[test]
    fn plugin_fractional_entry_preserves_exact_values_and_native_schema_validation() {
        let (mut target, mut descriptor, mut preferences) =
            plugin_fixture(PluginField::Control("speed".into()));
        descriptor.manifest.settings["properties"]["speed"] = json!({
            "type":"number", "minimum":0.25, "maximum":4.5,
            "multipleOf":0.25, "default":0.25
        });
        let mut authored = preferences.selected.as_ref().unwrap().settings.clone();
        authored["speed"] = json!(0.25);
        let selection = crate::animation_plugins::PluginSelection::new(
            &descriptor,
            ilium_animation_js::manifest::AnimationMode::Live,
            authored.clone(),
        )
        .unwrap();
        preferences
            .activate_selection(&descriptor, selection)
            .unwrap();
        target.fence = crate::animation_plugins::PluginControlFence::new(
            "a".repeat(64),
            &descriptor,
            &preferences,
        )
        .unwrap();
        let crate::value_dialog::ValueDialogState::Number(dialog) = target.dialog().unwrap() else {
            panic!("fractional number dialog");
        };
        assert_eq!(dialog.draft.buf, "0.25");
        for invalid in ["0", "4.75", "NaN", "infinity", "not a number"] {
            assert!(
                target
                    .prepare(
                        &target.project,
                        &target.fence.package_digest,
                        &descriptor,
                        &preferences,
                        &crate::value_dialog::DialogOutcome::CommitNumber(invalid.into()),
                    )
                    .is_err(),
                "{invalid}"
            );
        }
        // The existing native validator treats multipleOf as arrow-step metadata,
        // so direct entry must preserve an in-range value between those steps.
        for valid in ["0.25", "0.3", "0.75", "4.5"] {
            let desired = target
                .prepare(
                    &target.project,
                    &target.fence.package_digest,
                    &descriptor,
                    &preferences,
                    &crate::value_dialog::DialogOutcome::CommitNumber(valid.into()),
                )
                .unwrap();
            let mut expected = authored.clone();
            expected["speed"] = serde_json::from_str(valid).unwrap();
            assert_eq!(desired.selected.unwrap().settings, expected);
        }
        assert_eq!(preferences.selected.unwrap().settings, authored);
    }

    #[test]
    fn plugin_number_preserves_native_integer_validation_and_all_other_authored_fields() {
        let (target, descriptor, preferences) =
            plugin_fixture(PluginField::Control("speed".into()));
        for invalid in ["0", "11", "1.5", "NaN", "infinity", "not a number"] {
            assert!(
                target
                    .prepare(
                        &target.project,
                        &target.fence.package_digest,
                        &descriptor,
                        &preferences,
                        &crate::value_dialog::DialogOutcome::CommitNumber(invalid.into())
                    )
                    .is_err(),
                "{invalid}"
            );
        }
        let desired = target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &preferences,
                &crate::value_dialog::DialogOutcome::CommitNumber("7".into()),
            )
            .unwrap();
        let settings = &desired.selected.unwrap().settings;
        assert_eq!(settings["speed"], json!(7));
        assert_eq!(settings["palette"], json!("海"));
        assert_eq!(settings["authored"], json!("Unicode λ retained"));
    }
    #[test]
    fn plugin_option_ids_are_native_values_and_retry_rebases_only_own_admitted_selection() {
        let (mut target, descriptor, preferences) =
            plugin_fixture(PluginField::Control("palette".into()));
        let crate::value_dialog::ValueDialogState::Choice(dialog) = target.dialog().unwrap() else {
            panic!("choice");
        };
        assert_eq!(
            dialog
                .options()
                .iter()
                .map(|option| &option.id)
                .collect::<Vec<_>>(),
            vec!["\"海\"", "\"λ\"", "\"Sunset\""]
        );
        let outcome = crate::value_dialog::DialogOutcome::Choose("\"λ\"".into());
        let desired = target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &preferences,
                &outcome,
            )
            .unwrap();
        assert!(target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &desired,
                &outcome
            )
            .is_err());
        target.pending = desired.selected.clone();
        assert!(target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &desired,
                &outcome
            )
            .is_ok());
        let mut foreign = desired.clone();
        foreign.selected.as_mut().unwrap().settings["authored"] = json!("foreign change");
        assert!(target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &descriptor,
                &foreign,
                &outcome
            )
            .is_err());
        assert!(target
            .prepare(
                std::path::Path::new("/synthetic/other"),
                &target.fence.package_digest,
                &descriptor,
                &preferences,
                &outcome
            )
            .is_err());
        assert!(target
            .prepare(
                &target.project,
                &"b".repeat(64),
                &descriptor,
                &preferences,
                &outcome
            )
            .is_err());
        let mut changed = descriptor.clone();
        changed.manifest.settings["properties"]["palette"]["enum"] = json!(["New"]);
        assert!(target
            .prepare(
                &target.project,
                &target.fence.package_digest,
                &changed,
                &preferences,
                &outcome
            )
            .is_err());
    }
    #[test]
    fn plugin_number_receipts_keep_draft_on_failure_and_ignore_superseded_tokens() {
        use crate::value_dialog::ValueDialogState;
        use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
        use std::sync::Arc;
        let (target, _, _) = plugin_fixture(PluginField::Control("speed".into()));
        let ValueDialogState::Number(dialog) = target.dialog().unwrap() else {
            panic!("numeric editor");
        };
        let draft = dialog.draft.clone();
        let mut host = ValueDialogHost::number_host(ValueTarget::Plugin(target), dialog);
        let first = Arc::new(());
        let unrelated = Arc::new(());
        host.begin_save(first.clone());
        assert!(!host.finish_save(&unrelated, Ok(())));
        assert!(host.is_saving());
        assert!(host.finish_save(&first, Err("durable write rejected".into())));
        assert!(!host.is_saving());
        let ValueDialogState::Number(dialog) = &host.dialog else {
            panic!("numeric editor");
        };
        assert_eq!(dialog.draft, draft);
        assert_eq!(dialog.error.as_deref(), Some("durable write rejected"));
        let retry = Arc::new(());
        host.begin_save(retry.clone());
        assert!(!host.finish_save(&first, Ok(())));
        assert!(host.is_saving());
        assert!(host.finish_save(&retry, Err("worker reply lost".into())));
        let ValueDialogState::Number(dialog) = &host.dialog else {
            panic!("numeric editor");
        };
        assert_eq!(dialog.draft, draft);
        assert_eq!(dialog.error.as_deref(), Some("worker reply lost"));
        let final_token = Arc::new(());
        host.begin_save(final_token.clone());
        assert!(!host.finish_save(&retry, Ok(())));
        assert!(host.finish_save(&final_token, Ok(())));
        assert!(!host.is_saving());
    }
    #[test]
    fn app_plugin_receipts_retain_child_and_parent_and_cannot_settle_reopened_dialog() {
        use crate::app::{Mode, SettingsState, SettingsTab};
        use crate::value_dialog::ValueDialogState;
        use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
        use std::sync::Arc;
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("synthetic-plugin-receipts".into(), directory.path().into());
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            animation_source_tab: crate::animation_plugins::AnimationSourceTab::Plugin,
            ..SettingsState::default()
        });
        let host = || {
            let (target, _, _) = plugin_fixture(PluginField::Control("speed".into()));
            let ValueDialogState::Number(mut number) = target.dialog().unwrap() else {
                panic!("number");
            };
            number.draft.buf = "7".into();
            number.draft.cursor = 1;
            ValueDialogHost::number_host(ValueTarget::Plugin(target), number)
        };
        let first = Arc::new(());
        let mut child = host();
        child.begin_save(first.clone());
        app.push_modal(Mode::ValueDialog(Box::new(child)));
        app.finish_value_dialog_save(&first, Err("durable disk failure".into()));
        assert_eq!(app.modal_stack.len(), 1);
        assert!(
            matches!(&app.mode, Mode::ValueDialog(child) if !child.is_saving()
            && matches!(&child.dialog, ValueDialogState::Number(number)
                if number.draft.buf == "7" && number.error.as_deref() == Some("durable disk failure")))
        );
        app.pop_modal();
        let newer = Arc::new(());
        let mut replacement = host();
        replacement.begin_save(newer.clone());
        app.push_modal(Mode::ValueDialog(Box::new(replacement)));
        assert!(app.validate_plugin_value_receipt().is_err());
        app.finish_value_dialog_save(&first, Ok(()));
        assert!(matches!(&app.mode, Mode::ValueDialog(child) if child.is_saving()));
        assert_eq!(app.modal_stack.len(), 1);
        // Synthetic package has no authenticated runtime binding: even an owned
        // success token must retain this child rather than claim activation.
        app.finish_value_dialog_save(&newer, Ok(()));
        assert!(
            matches!(&app.mode, Mode::ValueDialog(child) if !child.is_saving()
            && matches!(&child.dialog, ValueDialogState::Number(number)
                if number.draft.buf == "7" && number.error.as_deref().is_some_and(|error| error.contains("binding changed"))))
        );
        assert_eq!(app.modal_stack.len(), 1);
        assert!(
            matches!(app.modal_stack.last(), Some(Mode::Settings(parent))
            if parent.tab == SettingsTab::Animations
                && parent.animation_source_tab == crate::animation_plugins::AnimationSourceTab::Plugin)
        );
    }
}
