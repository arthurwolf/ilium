//! Twins of the tests of ilium-client/src/value_animation.rs, run against the plugin's own
//! `values` module. The kernel keys the wide-number rule on `AnimationKind::VoxelLandscape`;
//! the plugin takes the scene id string (`voxel-landscape`) the registry supplies.

use ilium_plugin_core_ambient::control::{Control, ControlValue};
use ilium_plugin_core_ambient::values::{
    stepped_animation_number, AnimationControlScope, AnimationControlTarget, ValueDialog,
    VOXEL_LANDSCAPE_ID,
};
use std::path::{Path, PathBuf};

/// An absolute project directory in the platform's own shape.
fn project_dir() -> PathBuf {
    std::env::temp_dir().join("project")
}

fn target(control: Control) -> AnimationControlTarget {
    AnimationControlTarget::new(
        project_dir(),
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
        "0-4294967295",
        "The same seed recreates the same world.",
    )
}

const VOXEL_SCENE: &str = "VoxelLandscape/Some(VoxelLandscape)";

fn voxel_target(value: u32) -> AnimationControlTarget {
    AnimationControlTarget::new_for_animation(
        project_dir(),
        VOXEL_SCENE.into(),
        AnimationControlScope::Scene,
        VOXEL_LANDSCAPE_ID,
        voxel_seed(value),
    )
    .unwrap()
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_exact_number_preserves_the_full_unsigned_domain
#[test]
fn voxel_seed_exact_number_preserves_the_full_unsigned_domain_twin() {
    let target = voxel_target(71_839);
    let ValueDialog::Number { text, .. } = target.dialog().unwrap() else {
        panic!("voxel seed number dialog");
    };
    assert_eq!(text, "71839");
    for value in [0_u32, i32::MAX as u32, i32::MAX as u32 + 1, u32::MAX] {
        let text = value.to_string();
        assert_eq!(
            target.number(
                project_dir().as_path(),
                VOXEL_SCENE,
                AnimationControlScope::Scene,
                &voxel_seed(71_839),
                &text,
            ),
            Ok(ControlValue::Text(text)),
        );
    }
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_exact_number_rejects_fraction_negative_and_overflow
#[test]
fn voxel_seed_exact_number_rejects_fraction_negative_and_overflow_twin() {
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
                    project_dir().as_path(),
                    VOXEL_SCENE,
                    AnimationControlScope::Scene,
                    &current,
                    text,
                )
                .is_err(),
            "accepted {text:?}",
        );
    }
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds
#[test]
fn voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds_twin() {
    for (value, direction, expected) in [
        (0_u32, -1, 0_u32),
        (0_u32, 1, 1_u32),
        (i32::MAX as u32, 1, i32::MAX as u32 + 1),
        (i32::MAX as u32 + 1, -1, i32::MAX as u32),
        (u32::MAX, 1, u32::MAX),
        (u32::MAX, -1, u32::MAX - 1),
    ] {
        assert_eq!(
            stepped_animation_number(VOXEL_LANDSCAPE_ID, &voxel_seed(value), direction),
            Ok(Some(ControlValue::Text(expected.to_string()))),
        );
    }
    assert_eq!(
        stepped_animation_number("shoreline", &voxel_seed(10), 1),
        Ok(None),
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_dialog_rejects_changed_native_control_shape
#[test]
fn voxel_seed_dialog_rejects_changed_native_control_shape_twin() {
    let target = voxel_target(71_839);
    let current = voxel_seed(71_839);
    for (project, scene, scope) in [
        (
            PathBuf::from("/other"),
            VOXEL_SCENE,
            AnimationControlScope::Scene,
        ),
        (
            project_dir(),
            "Rain/Some(Rain)",
            AnimationControlScope::Scene,
        ),
        (project_dir(), VOXEL_SCENE, AnimationControlScope::Common),
    ] {
        assert!(target
            .number(project.as_path(), scene, scope, &current, "4294967295")
            .is_err());
    }
    let changed = Control::slider("seed", "World seed", 10, (0, 100, 1), "", "");
    assert!(target
        .number(
            project_dir().as_path(),
            VOXEL_SCENE,
            AnimationControlScope::Scene,
            &changed,
            "17",
        )
        .is_err());
}

/// REQ: PORT:ilium-client/src/value_animation.rs::list_preserves_full_order_current_and_disabled_reason
#[test]
fn list_preserves_full_order_current_and_disabled_reason_twin() {
    let ValueDialog::Choice {
        options,
        selected_id,
        ..
    } = target(choice()).dialog().unwrap()
    else {
        panic!("choice dialog");
    };
    assert_eq!(options.len(), 3);
    assert_eq!(selected_id.as_deref(), Some("2"));
    assert_eq!(
        options[1].disabled_reason.as_deref(),
        Some("device missing")
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::stale_destinations_and_reordered_or_disabled_choices_are_rejected
#[test]
fn stale_destinations_and_reordered_or_disabled_choices_are_rejected_twin() {
    let opened = choice();
    let target = target(opened.clone());
    let project = project_dir();
    let validate = |project: &Path, scene, scope, current: &Control, id| {
        target.choice(project, scene, scope, current, id)
    };
    let scene = AnimationControlScope::Scene;
    assert_eq!(
        validate(project.as_path(), "rain", scene, &opened, "2"),
        Ok(ControlValue::Index(2))
    );
    assert!(validate(Path::new("/other"), "rain", scene, &opened, "2").is_err());
    assert!(validate(project.as_path(), "snow", scene, &opened, "2").is_err());
    assert!(validate(
        project.as_path(),
        "rain",
        AnimationControlScope::Common,
        &opened,
        "2"
    )
    .is_err());
    for id in ["1", "02", "-1", "99"] {
        assert!(validate(project.as_path(), "rain", scene, &opened, id).is_err());
    }
    let changed = Control::choice("palette", "Palette", 0, &["A", "C", "B"], "");
    assert!(validate(project.as_path(), "rain", scene, &changed, "2").is_err());
}

/// REQ: PORT:ilium-client/src/value_animation.rs::exact_number_uses_fresh_bounds_without_clamping
#[test]
fn exact_number_uses_fresh_bounds_without_clamping_twin() {
    let opened = Control::slider("speed", "Speed", 10, (0, 100, 10), "%", "");
    let target = target(opened.clone());
    let ValueDialog::Number { text, .. } = target.dialog().unwrap() else {
        panic!("number");
    };
    assert_eq!(text, "10");
    let project = project_dir();
    let scene = AnimationControlScope::Scene;
    assert_eq!(
        target.number(project.as_path(), "rain", scene, &opened, "17"),
        Ok(ControlValue::Number(17))
    );
    let changed = Control::slider("speed", "Speed", 10, (0, 10, 1), "%", "");
    for text in ["17", "NaN", "1.5", "", "-1"] {
        assert!(target
            .number(project.as_path(), "rain", scene, &changed, text)
            .is_err());
    }
}

/// REQ: PORT:ilium-client/src/value_animation.rs::missing_identity_and_non_numeric_controls_cannot_open_number_dialogs
#[test]
fn missing_identity_and_non_numeric_controls_cannot_open_number_dialogs_twin() {
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
