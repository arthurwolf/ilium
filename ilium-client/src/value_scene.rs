//! Pure scene-control option inventory and fresh-domain validation for shared dialogs.
use crate::value_number::{NumberSpec, NumberValue};
use ilium_ambient::{Control, ControlKind, ControlValue};
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
#[cfg(test)]
mod tests {
    use super::*;
    fn choice() -> Control {
        Control::choice("palette", "Palette", 0, &["A", "B", "C"], "")
            .with_disabled_option(1, "missing device")
    }
    #[test]
    fn full_list_keeps_disabled_choices_in_original_order() {
        let options = choices(&choice()).unwrap();
        assert_eq!(
            options.iter().map(|o| o.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            options[1].disabled_reason.as_deref(),
            Some("missing device")
        );
        assert_eq!(options[2].label, "C");
    }
    #[test]
    fn direct_choice_revalidates_live_disabled_state() {
        let opened = choice();
        let current = opened.clone().with_disabled_option(2, "removed");
        assert_eq!(
            checked_choice(&opened, &opened, 2),
            Ok(ControlValue::Index(2))
        );
        assert!(checked_choice(&opened, &current, 2).is_err());
        assert!(checked_choice(&opened, &opened, 1).is_err());
        assert!(checked_choice(&opened, &opened, 3).is_err());
    }
    #[test]
    fn changed_option_identity_is_rejected_instead_of_using_stale_index() {
        let opened = choice();
        let current = Control::choice("palette", "Palette", 0, &["A", "C", "B"], "");
        assert!(checked_choice(&opened, &current, 2).is_err());
        let foreign = Control::choice("city", "City", 0, &["A", "B", "C"], "");
        assert!(checked_choice(&opened, &foreign, 2).is_err());
    }
    #[test]
    fn typed_numbers_keep_intermediates_and_reject_bounds_before_domain_clamping() {
        let control = Control::slider("rate", "Rate", 10, (0, 100, 10), "%", "");
        assert_eq!(checked_number(&control, "17"), Ok(ControlValue::Number(17)));
        for invalid in ["17.5", "101", "-1", "10%", "", "2147483648"] {
            assert!(checked_number(&control, invalid).is_err());
        }
        let changed = Control::slider("rate", "Rate", 10, (0, 10, 1), "%", "");
        assert!(checked_number(&changed, "17").is_err());
    }
    #[test]
    fn numeric_entry_does_not_apply_to_boolean_or_text_controls() {
        assert!(choices(&Control::toggle("b", "B", true, "")).is_none());
        assert!(checked_number(&Control::text("t", "T", "", "", ""), "17").is_err());
    }
}
