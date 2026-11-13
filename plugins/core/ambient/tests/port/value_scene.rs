//! Twins of the tests of ilium-client/src/value_scene.rs, run against the plugin's own
//! `values` module (the rules moved out of the kernel file).

use ilium_plugin_core_ambient::control::{Control, ControlValue};
use ilium_plugin_core_ambient::values::{checked_choice, checked_number, choices};

fn choice() -> Control {
    Control::choice("palette", "Palette", 0, &["A", "B", "C"], "")
        .with_disabled_option(1, "missing device")
}

/// REQ: PORT:ilium-client/src/value_scene.rs::full_list_keeps_disabled_choices_in_original_order
#[test]
fn full_list_keeps_disabled_choices_in_original_order_twin() {
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

/// REQ: PORT:ilium-client/src/value_scene.rs::direct_choice_revalidates_live_disabled_state
#[test]
fn direct_choice_revalidates_live_disabled_state_twin() {
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

/// REQ: PORT:ilium-client/src/value_scene.rs::changed_option_identity_is_rejected_instead_of_using_stale_index
#[test]
fn changed_option_identity_is_rejected_instead_of_using_stale_index_twin() {
    let opened = choice();
    let current = Control::choice("palette", "Palette", 0, &["A", "C", "B"], "");
    assert!(checked_choice(&opened, &current, 2).is_err());
    let foreign = Control::choice("city", "City", 0, &["A", "B", "C"], "");
    assert!(checked_choice(&opened, &foreign, 2).is_err());
}

/// REQ: PORT:ilium-client/src/value_scene.rs::typed_numbers_keep_intermediates_and_reject_bounds_before_domain_clamping
#[test]
fn typed_numbers_keep_intermediates_and_reject_bounds_before_domain_clamping_twin() {
    let control = Control::slider("rate", "Rate", 10, (0, 100, 10), "%", "");
    assert_eq!(checked_number(&control, "17"), Ok(ControlValue::Number(17)));
    for invalid in ["17.5", "101", "-1", "10%", "", "2147483648"] {
        assert!(checked_number(&control, invalid).is_err());
    }
    let changed = Control::slider("rate", "Rate", 10, (0, 10, 1), "%", "");
    assert!(checked_number(&changed, "17").is_err());
}

/// REQ: PORT:ilium-client/src/value_scene.rs::numeric_entry_does_not_apply_to_boolean_or_text_controls
#[test]
fn numeric_entry_does_not_apply_to_boolean_or_text_controls_twin() {
    assert!(choices(&Control::toggle("b", "B", true, "")).is_none());
    assert!(checked_number(&Control::text("t", "T", "", "", ""), "17").is_err());
}
