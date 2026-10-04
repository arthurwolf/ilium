//! Captured identity and fresh validation for animation value dialogs.
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, NumberDialogState, ValueDialogState};
use crate::value_scene;
use ilium_ambient::{Control, ControlKind, ControlValue};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationControlScope {
    Common,
    Scene,
}

/// A stable scene identity is supplied by the scene registry, never a row index.
pub struct AnimationControlTarget {
    project: PathBuf,
    scene_id: String,
    scope: AnimationControlScope,
    opened: Control,
}

impl AnimationControlTarget {
    pub fn control_id(&self) -> &'static str {
        self.opened.id
    }

    pub fn scope(&self) -> AnimationControlScope {
        self.scope
    }

    pub fn new(
        project: PathBuf,
        scene_id: String,
        scope: AnimationControlScope,
        opened: Control,
    ) -> Result<Self, String> {
        if !project.is_absolute() || scene_id.is_empty() || opened.id.is_empty() {
            return Err("Animation dialog requires a project, scene and control identity".into());
        }
        Ok(Self {
            project,
            scene_id,
            scope,
            opened,
        })
    }

    pub fn dialog(&self) -> Result<ValueDialogState, String> {
        match (&self.opened.kind, &self.opened.value) {
            (ControlKind::Choice { .. }, ControlValue::Index(index)) => {
                let options = value_scene::choices(&self.opened)
                    .ok_or("This animation control no longer offers options")?
                    .into_iter()
                    .map(|option| ChoiceOption {
                        id: option.index.to_string(),
                        label: option.label,
                        disabled_reason: option.disabled_reason,
                    })
                    .collect();
                Ok(ValueDialogState::Choice(ChoiceDialogState::new(
                    self.opened.label,
                    options,
                    Some(index.to_string()),
                )?))
            }
            (ControlKind::Slider { .. }, ControlValue::Number(number)) => {
                Ok(ValueDialogState::Number(NumberDialogState::new(
                    self.opened.label,
                    number.to_string(),
                )))
            }
            _ => Err("This animation control does not use a value dialog".into()),
        }
    }

    fn validate_identity(
        &self,
        project: &Path,
        scene_id: &str,
        scope: AnimationControlScope,
        current: &Control,
    ) -> Result<(), String> {
        if project != self.project
            || scene_id != self.scene_id
            || scope != self.scope
            || current.id != self.opened.id
        {
            return Err("The animation destination changed; reopen this value dialog".into());
        }
        Ok(())
    }

    pub fn choice(
        &self,
        project: &Path,
        scene_id: &str,
        scope: AnimationControlScope,
        current: &Control,
        id: &str,
    ) -> Result<ControlValue, String> {
        self.validate_identity(project, scene_id, scope, current)?;
        let index = id
            .parse::<usize>()
            .map_err(|_| "Unknown animation option")?;
        if index.to_string() != id {
            return Err("Unknown animation option".into());
        }
        value_scene::checked_choice(&self.opened, current, index)
    }

    pub fn number(
        &self,
        project: &Path,
        scene_id: &str,
        scope: AnimationControlScope,
        current: &Control,
        text: &str,
    ) -> Result<ControlValue, String> {
        self.validate_identity(project, scene_id, scope, current)?;
        if !matches!(self.opened.kind, ControlKind::Slider { .. }) {
            return Err("The opening control did not accept a number".into());
        }
        value_scene::checked_number(current, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(control: Control) -> AnimationControlTarget {
        AnimationControlTarget::new(
            "/project".into(),
            "rain".into(),
            AnimationControlScope::Scene,
            control,
        )
        .unwrap()
    }
    fn choice() -> Control {
        Control::choice("palette", "Palette", 2, &["A", "B", "C"], "")
            .with_disabled_option(1, "device missing")
    }
    #[test]
    fn list_preserves_full_order_current_and_disabled_reason() {
        let ValueDialogState::Choice(dialog) = target(choice()).dialog().unwrap() else {
            panic!("choice dialog");
        };
        assert_eq!(dialog.options().len(), 3);
        assert_eq!(dialog.selected_id.as_deref(), Some("2"));
        assert_eq!(
            dialog.options()[1].disabled_reason.as_deref(),
            Some("device missing")
        );
    }
    #[test]
    fn stale_destinations_and_reordered_or_disabled_choices_are_rejected() {
        let opened = choice();
        let target = target(opened.clone());
        let validate = |project, scene, scope, current: &Control, id| {
            target.choice(Path::new(project), scene, scope, current, id)
        };
        assert_eq!(
            validate(
                "/project",
                "rain",
                AnimationControlScope::Scene,
                &opened,
                "2"
            ),
            Ok(ControlValue::Index(2))
        );
        assert!(validate("/other", "rain", AnimationControlScope::Scene, &opened, "2").is_err());
        assert!(validate(
            "/project",
            "snow",
            AnimationControlScope::Scene,
            &opened,
            "2"
        )
        .is_err());
        assert!(validate(
            "/project",
            "rain",
            AnimationControlScope::Common,
            &opened,
            "2"
        )
        .is_err());
        for id in ["1", "02", "-1", "99"] {
            assert!(validate(
                "/project",
                "rain",
                AnimationControlScope::Scene,
                &opened,
                id
            )
            .is_err());
        }
        let changed = Control::choice("palette", "Palette", 0, &["A", "C", "B"], "");
        assert!(validate(
            "/project",
            "rain",
            AnimationControlScope::Scene,
            &changed,
            "2"
        )
        .is_err());
    }
    #[test]
    fn exact_number_uses_fresh_bounds_without_clamping() {
        let opened = Control::slider("speed", "Speed", 10, (0, 100, 10), "%", "");
        let target = target(opened.clone());
        let ValueDialogState::Number(dialog) = target.dialog().unwrap() else {
            panic!("number");
        };
        assert_eq!(dialog.draft.buf, "10");
        assert_eq!(
            target.number(
                Path::new("/project"),
                "rain",
                AnimationControlScope::Scene,
                &opened,
                "17"
            ),
            Ok(ControlValue::Number(17))
        );
        let changed = Control::slider("speed", "Speed", 10, (0, 10, 1), "%", "");
        for text in ["17", "NaN", "1.5", "", "-1"] {
            assert!(target
                .number(
                    Path::new("/project"),
                    "rain",
                    AnimationControlScope::Scene,
                    &changed,
                    text
                )
                .is_err());
        }
    }
    #[test]
    fn missing_identity_and_non_numeric_controls_cannot_open_number_dialogs() {
        assert!(AnimationControlTarget::new(
            "relative".into(),
            "rain".into(),
            AnimationControlScope::Common,
            choice()
        )
        .is_err());
        assert!(target(Control::toggle("enabled", "Enabled", true, ""))
            .dialog()
            .is_err());
    }
}
