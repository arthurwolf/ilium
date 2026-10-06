//! Captured identity and fresh validation for animation value dialogs.
use crate::background_animation::AnimationKind;
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, NumberDialogState, ValueDialogState};
use crate::value_number::{NumberSpec, NumberValue};
use crate::value_scene;
use ilium_ambient::{Control, ControlKind, ControlValue};
use std::path::{Path, PathBuf};

const VOXEL_SEED_MINIMUM: i128 = 0;
const VOXEL_SEED_MAXIMUM: i128 = u32::MAX as i128;
const VOXEL_SEED_STEP: i128 = 1;

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

pub fn animation_number_spec(
    animation_kind: AnimationKind,
    control: &Control,
) -> Option<(i128, i128, i128, i128)> {
    if animation_kind != AnimationKind::VoxelLandscape {
        return None;
    }
    voxel_seed_number_spec(control)
}

pub fn stepped_animation_number(
    animation_kind: AnimationKind,
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
        animation_kind: AnimationKind,
        opened: Control,
    ) -> Result<Self, String> {
        let wide_text_number = scope == AnimationControlScope::Scene
            && animation_number_spec(animation_kind, &opened).is_some();
        let mut target = Self::new(project, scene_id, scope, opened)?;
        target.wide_text_number = wide_text_number;
        Ok(target)
    }

    pub fn dialog(&self) -> Result<ValueDialogState, String> {
        if self.wide_text_number {
            let (_, _, _, value) = voxel_seed_number_spec(&self.opened)
                .ok_or("The opening world seed control is no longer numeric")?;
            return Ok(ValueDialogState::Number(NumberDialogState::new(
                self.opened.label,
                value.to_string(),
            )));
        }
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
        if self.wide_text_number {
            return checked_voxel_seed_number(current, text);
        }
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

    fn voxel_seed(value: u32) -> Control {
        Control::text(
            "seed",
            "World seed",
            &value.to_string(),
            "0–4294967295",
            "The same seed recreates the same world.",
        )
    }

    fn voxel_target(value: u32) -> AnimationControlTarget {
        AnimationControlTarget::new_for_animation(
            "/project".into(),
            "VoxelLandscape/Some(VoxelLandscape)".into(),
            AnimationControlScope::Scene,
            AnimationKind::VoxelLandscape,
            voxel_seed(value),
        )
        .unwrap()
    }

    #[test]
    fn voxel_seed_exact_number_preserves_the_full_unsigned_domain() {
        let target = voxel_target(71_839);
        let ValueDialogState::Number(dialog) = target.dialog().unwrap() else {
            panic!("voxel seed number dialog");
        };
        assert_eq!(dialog.draft.buf, "71839");
        for value in [0_u32, i32::MAX as u32, i32::MAX as u32 + 1, u32::MAX] {
            let text = value.to_string();
            assert_eq!(
                target.number(
                    Path::new("/project"),
                    "VoxelLandscape/Some(VoxelLandscape)",
                    AnimationControlScope::Scene,
                    &voxel_seed(71_839),
                    &text,
                ),
                Ok(ControlValue::Text(text)),
            );
        }
    }

    #[test]
    fn voxel_seed_exact_number_rejects_fraction_negative_and_overflow() {
        let target = voxel_target(71_839);
        let current = voxel_seed(71_839);
        for text in [
            "1.5",
            "-1",
            "4294967296",
            "170141183460469231731687303715884105728",
            "",
            "NaN",
            "inf",
        ] {
            assert!(
                target
                    .number(
                        Path::new("/project"),
                        "VoxelLandscape/Some(VoxelLandscape)",
                        AnimationControlScope::Scene,
                        &current,
                        text,
                    )
                    .is_err(),
                "accepted {text:?}",
            );
        }
    }

    #[test]
    fn voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds() {
        for (value, direction, expected) in [
            (0_u32, -1, 0_u32),
            (0_u32, 1, 1_u32),
            (i32::MAX as u32, 1, i32::MAX as u32 + 1),
            (i32::MAX as u32 + 1, -1, i32::MAX as u32),
            (u32::MAX, 1, u32::MAX),
            (u32::MAX, -1, u32::MAX - 1),
        ] {
            assert_eq!(
                stepped_animation_number(
                    AnimationKind::VoxelLandscape,
                    &voxel_seed(value),
                    direction,
                ),
                Ok(Some(ControlValue::Text(expected.to_string()))),
            );
        }
        assert_eq!(
            stepped_animation_number(AnimationKind::Shoreline, &voxel_seed(10), 1,),
            Ok(None),
        );
    }

    #[test]
    fn voxel_seed_dialog_rejects_changed_native_control_shape() {
        let target = voxel_target(71_839);
        let current = voxel_seed(71_839);
        for (project, scene, scope) in [
            (
                "/other",
                "VoxelLandscape/Some(VoxelLandscape)",
                AnimationControlScope::Scene,
            ),
            ("/project", "Rain/Some(Rain)", AnimationControlScope::Scene),
            (
                "/project",
                "VoxelLandscape/Some(VoxelLandscape)",
                AnimationControlScope::Common,
            ),
        ] {
            assert!(target
                .number(Path::new(project), scene, scope, &current, "4294967295")
                .is_err());
        }
        let changed = Control::slider("seed", "World seed", 10, (0, 100, 1), "", "");
        assert!(target
            .number(
                Path::new("/project"),
                "VoxelLandscape/Some(VoxelLandscape)",
                AnimationControlScope::Scene,
                &changed,
                "17",
            )
            .is_err(),);
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
