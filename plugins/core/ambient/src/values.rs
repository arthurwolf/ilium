//! Captured identity and fresh validation for animation value dialogs.
//!
//! Ported from `ilium-client/src/value_scene.rs` and `ilium-client/src/value_animation.rs`. A
//! value dialog is opened against one control of one scene of one project. When the user
//! confirms, the dialog's answer is checked against the control as it is now, never against the
//! snapshot it opened with, so a stale index or a changed option list can never write a value.
//!
//! Pure: no host call, no I/O. The dialog chrome (drawing, keys) stays with the kernel; the
//! plugin owns the rules of what may be confirmed.

use crate::control::{Control, ControlKind, ControlValue};
use crate::number::{NumberSpec, NumberValue};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The scene whose world seed is a wide unsigned number instead of a text row.
pub const VOXEL_LANDSCAPE_ID: &str = "voxel-landscape";

const VOXEL_SEED_MINIMUM: i128 = 0;
const VOXEL_SEED_MAXIMUM: i128 = u32::MAX as i128;
const VOXEL_SEED_STEP: i128 = 1;

/// One option of a scene choice control, disabled ones included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneChoice {
    pub index: usize,
    pub label: String,
    pub disabled_reason: Option<String>,
}

/// Preserve every option, including disabled entries, without cycling live settings.
pub fn choices(control: &Control) -> Option<Vec<SceneChoice>> {
    let ControlKind::Choice { options } = &control.kind else {
        return None;
    };
    Some(
        options
            .iter()
            .enumerate()
            .map(|(index, label)| SceneChoice {
                index,
                label: (*label).to_owned(),
                disabled_reason: control.disabled_reason(index).map(str::to_owned),
            })
            .collect(),
    )
}

/// A dialog index belongs to its opening snapshot; resolve it against fresh metadata.
pub fn checked_choice(
    opened: &Control,
    current: &Control,
    index: usize,
) -> Result<ControlValue, String> {
    if opened.id != current.id {
        return Err("This control changed while the options dialog was open".into());
    }
    let (ControlKind::Choice { options: before }, ControlKind::Choice { options: now }) =
        (&opened.kind, &current.kind)
    else {
        return Err("This control no longer offers a choice list".into());
    };
    let Some(label) = before.get(index) else {
        return Err("The selected option is no longer available".into());
    };
    if now.get(index) != Some(label) {
        return Err("The option list changed; reopen it to select a current option".into());
    }
    if let Some(reason) = current.disabled_reason(index) {
        return Err(reason.to_owned());
    }
    Ok(ControlValue::Index(index))
}

/// Validate exact stored-domain input before setters that might otherwise silently clamp.
pub fn checked_number(current: &Control, text: &str) -> Result<ControlValue, String> {
    let ControlKind::Slider { min, max, .. } = current.kind else {
        return Err("This control does not accept a number".into());
    };
    let spec = NumberSpec::Integer {
        minimum: i128::from(min),
        maximum: i128::from(max),
    };
    let NumberValue::Integer(number) = spec.parse(text)? else {
        return Err("Enter a whole number".into());
    };
    i32::try_from(number)
        .map(ControlValue::Number)
        .map_err(|_| "Number exceeds this control's storage range".into())
}

fn voxel_seed_number_spec(control: &Control) -> Option<(i128, i128, i128, i128)> {
    if control.id != "seed" {
        return None;
    }
    let (ControlKind::Text { .. }, ControlValue::Text(text)) = (&control.kind, &control.value)
    else {
        return None;
    };
    let value = text.trim().parse::<u32>().ok()?;
    Some((
        VOXEL_SEED_MINIMUM,
        VOXEL_SEED_MAXIMUM,
        VOXEL_SEED_STEP,
        i128::from(value),
    ))
}

/// `(minimum, maximum, step, value)` of a text control that is really a wide number, or `None`.
/// Only the voxel landscape world seed is one.
pub fn animation_number_spec(
    animation_kind: &str,
    control: &Control,
) -> Option<(i128, i128, i128, i128)> {
    if animation_kind != VOXEL_LANDSCAPE_ID {
        return None;
    }
    voxel_seed_number_spec(control)
}

/// One keyboard step of a wide-number text control, or `Ok(None)` when the control is not one.
pub fn stepped_animation_number(
    animation_kind: &str,
    control: &Control,
    direction: i32,
) -> Result<Option<ControlValue>, String> {
    let Some((minimum, maximum, step, value)) = animation_number_spec(animation_kind, control)
    else {
        return Ok(None);
    };
    let NumberValue::Integer(next) = (NumberSpec::Integer { minimum, maximum }).stepped(
        NumberValue::Integer(value),
        NumberValue::Integer(step),
        direction,
    )?
    else {
        return Err("World seed numeric metadata changed unexpectedly".into());
    };
    let next = u32::try_from(next)
        .map_err(|_| "World seed must be an integer from 0 through 4294967295")?;
    Ok(Some(ControlValue::Text(next.to_string())))
}

fn checked_voxel_seed_number(current: &Control, text: &str) -> Result<ControlValue, String> {
    let Some((minimum, maximum, _, _)) = voxel_seed_number_spec(current) else {
        return Err("The world seed control changed; reopen this value dialog".into());
    };
    let NumberValue::Integer(value) = (NumberSpec::Integer { minimum, maximum }).parse(text)?
    else {
        return Err("Enter a whole-number world seed".into());
    };
    let seed = u32::try_from(value)
        .map_err(|_| "World seed must be an integer from 0 through 4294967295")?;
    Ok(ControlValue::Text(seed.to_string()))
}

/// Whether a control belongs to the shared look rows or to the selected scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationControlScope {
    Common,
    Scene,
}

/// One option of a dialog choice list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
    pub disabled_reason: Option<String>,
}

/// The dialog a control opens: a list of options or one exact number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueDialog {
    Choice {
        title: String,
        options: Vec<ChoiceOption>,
        selected_id: Option<String>,
    },
    Number {
        title: String,
        text: String,
    },
}

fn validate_ids(options: &[ChoiceOption]) -> Result<(), String> {
    let mut seen = HashSet::with_capacity(options.len());
    for option in options {
        if option.id.is_empty() {
            return Err("Choice IDs must not be empty".into());
        }
        if !seen.insert(option.id.as_str()) {
            return Err(format!("Duplicate choice ID: {}", option.id));
        }
    }
    Ok(())
}

/// A stable scene identity is supplied by the scene registry, never a row index.
pub struct AnimationControlTarget {
    project: PathBuf,
    scene_id: String,
    scope: AnimationControlScope,
    opened: Control,
    wide_text_number: bool,
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
            wide_text_number: false,
        })
    }

    pub fn new_for_animation(
        project: PathBuf,
        scene_id: String,
        scope: AnimationControlScope,
        animation_kind: &str,
        opened: Control,
    ) -> Result<Self, String> {
        let wide_text_number = scope == AnimationControlScope::Scene
            && animation_number_spec(animation_kind, &opened).is_some();
        let mut target = Self::new(project, scene_id, scope, opened)?;
        target.wide_text_number = wide_text_number;
        Ok(target)
    }

    pub fn dialog(&self) -> Result<ValueDialog, String> {
        if self.wide_text_number {
            let (_, _, _, value) = voxel_seed_number_spec(&self.opened)
                .ok_or("The opening world seed control is no longer numeric")?;
            return Ok(ValueDialog::Number {
                title: self.opened.label.to_owned(),
                text: value.to_string(),
            });
        }
        match (&self.opened.kind, &self.opened.value) {
            (ControlKind::Choice { .. }, ControlValue::Index(index)) => {
                let options: Vec<ChoiceOption> = choices(&self.opened)
                    .ok_or("This animation control no longer offers options")?
                    .into_iter()
                    .map(|option| ChoiceOption {
                        id: option.index.to_string(),
                        label: option.label,
                        disabled_reason: option.disabled_reason,
                    })
                    .collect();
                validate_ids(&options)?;
                Ok(ValueDialog::Choice {
                    title: self.opened.label.to_owned(),
                    options,
                    selected_id: Some(index.to_string()),
                })
            }
            (ControlKind::Slider { .. }, ControlValue::Number(number)) => Ok(ValueDialog::Number {
                title: self.opened.label.to_owned(),
                text: number.to_string(),
            }),
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
        checked_choice(&self.opened, current, index)
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
        if self.wide_text_number {
            return checked_voxel_seed_number(current, text);
        }
        if !matches!(self.opened.kind, ControlKind::Slider { .. }) {
            return Err("The opening control did not accept a number".into());
        }
        checked_number(current, text)
    }
}
