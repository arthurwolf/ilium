//! Twins of the tests of ilium-client/src/app_semantic_animation_tests.rs.
//!
//! The kernel tests drive the whole `App`: tree snapshots, host and timer lifecycle, composed
//! frames. Those stay with the kernel (client composition and the project tree are not plugin
//! API). The plugin owns the data boundary they all rest on: admission and revalidation of a
//! saved recommendation (`semantic`) and the effective-kind and status rows (`rows`). Each twin
//! asserts that boundary over the synthetic registry of `semantic_fixture`.

use crate::semantic_fixture::{carpet, paris, proposal, selected, Fixture, Settings};
use ilium_plugin_core_ambient::control::Control;
use ilium_plugin_core_ambient::control::ControlValue::Number;
use ilium_plugin_core_ambient::rows::{
    AnimationRow, RowContext, RowInputs, RowKind, RowModel, SceneEntry, OPENSTREETMAP_ID,
    SEMANTIC_ID,
};
use ilium_plugin_core_ambient::semantic::{
    resolve_recommendation, validate_recommendation, validate_recommendations,
    AnimationRecommendation, AnimationValue, NamedRecommendation, ProposedRecommendations,
};

fn validated(
    raw: &ilium_plugin_core_ambient::semantic::ProposedRecommendation,
) -> AnimationRecommendation {
    validate_recommendation(&Fixture, raw, &Settings::default()).expect("valid recommendation")
}

fn semantic_inputs(selected: &str) -> RowInputs {
    RowInputs {
        scenes: [SEMANTIC_ID, OPENSTREETMAP_ID, "carpet"]
            .into_iter()
            .map(|id| SceneEntry {
                id: id.to_owned(),
                label: id.to_owned(),
                description: String::new(),
            })
            .collect(),
        selected: selected.to_owned(),
        common: vec![
            Control::toggle("panels", "Panels", true, ""),
            Control::toggle("background", "Background", false, ""),
            Control::choice("playback", "Playback", 0, &["Live", "Loop"], ""),
        ],
        observer: "Lisbon".to_owned(),
        ..RowInputs::default()
    }
}

fn status_of(model: &RowModel) -> (String, String) {
    let index = model
        .rows()
        .iter()
        .position(|row| *row == AnimationRow::SceneStatus)
        .expect("status row");
    let view = model.view(index).expect("status view");
    assert_eq!(view.kind, RowKind::Status);
    (view.value.clone(), view.help.clone())
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::animation_settings_are_global_and_selecting_projects_never_rebinds_them
#[test]
fn animation_settings_are_global_and_selecting_projects_never_rebinds_them_twin() {
    let authored = Settings::default();
    let first =
        resolve_recommendation(&Fixture, &validated(&carpet(1, vec![])), &authored).unwrap();
    let second = resolve_recommendation(&Fixture, &validated(&paris()), &authored).unwrap();
    // Two projects resolve against the same authored settings without touching them.
    assert_eq!(selected(&first, "carpet_mode"), Some(1));
    assert_eq!(selected(&second, "source"), Some(0));
    assert_eq!(authored, Settings::default());
    assert!(!first.enabled && !second.enabled);
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_authored_video_reuse_and_missing_input_are_revalidated
#[test]
fn semantic_authored_video_reuse_and_missing_input_are_revalidated_twin() {
    let raw = proposal(
        "video",
        vec![(
            "shuffle",
            ilium_plugin_core_ambient::control::ControlValue::Bool(true),
        )],
    );
    let with_clip = Settings {
        folders: "clip.mp4".into(),
        ..Settings::default()
    };
    let recommendation = validate_recommendation(&Fixture, &raw, &with_clip).unwrap();
    assert!(resolve_recommendation(&Fixture, &recommendation, &with_clip).is_ok());
    // The clip is removed from the authored settings: the same saved recommendation is refused.
    let without = Settings::default();
    assert!(resolve_recommendation(&Fixture, &recommendation, &without).is_err());
    // Restoring it makes the recommendation resolve again (nothing is cached across checks).
    assert!(resolve_recommendation(&Fixture, &recommendation, &with_clip).is_ok());
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts
#[test]
fn semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts_twin() {
    let authored = Settings::default();
    let recommendation = validated(&carpet(4, vec![("carpet_spacing", Number(7))]));
    let once = resolve_recommendation(&Fixture, &recommendation, &authored).unwrap();
    let twice = resolve_recommendation(&Fixture, &recommendation, &authored).unwrap();
    // An equivalent snapshot resolves to an equal state: the host keyed on it is not rebuilt.
    assert_eq!(once, twice);
    // A genuinely different snapshot resolves differently: the host is rebuilt.
    let other = validated(&carpet(5, vec![("carpet_spacing", Number(7))]));
    assert_ne!(
        resolve_recommendation(&Fixture, &other, &authored).unwrap(),
        once
    );
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_disabled_background_preview_and_concrete_cadence_are_consistent
#[test]
fn semantic_disabled_background_preview_and_concrete_cadence_are_consistent_twin() {
    let recommendation = validated(&carpet(1, vec![]));
    let disabled = Settings::default();
    let enabled = Settings {
        enabled: true,
        ..Settings::default()
    };
    // Resolution follows the authored background switch and never changes it.
    assert!(
        !resolve_recommendation(&Fixture, &recommendation, &disabled)
            .unwrap()
            .enabled
    );
    assert!(
        resolve_recommendation(&Fixture, &recommendation, &enabled)
            .unwrap()
            .enabled
    );
    // The settings page previews the concrete scene even while the background is off: the row
    // model reports the effective kind the registry resolved, not the word "semantic".
    let context = RowContext {
        effective_kind: Some("carpet".into()),
        ..RowContext::default()
    };
    let model = RowModel::new(&semantic_inputs(SEMANTIC_ID), &context);
    assert_eq!(model.effective_kind(), Some("carpet"));
    assert_eq!(
        RowModel::new(&semantic_inputs(SEMANTIC_ID), &RowContext::default()).effective_kind(),
        None
    );
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_invalid_or_missing_recommendation_releases_field_host_and_timer
#[test]
fn semantic_invalid_or_missing_recommendation_releases_field_host_and_timer_twin() {
    let authored = Settings::default();
    let mut invalid = validated(&carpet(1, vec![]));
    let AnimationValue::Choice { label, .. } = &mut invalid.parameters[0].value else {
        panic!("choice fixture");
    };
    label.push_str(" changed");
    // No settings resolve, so the caller has nothing to host or time.
    assert!(resolve_recommendation(&Fixture, &invalid, &authored).is_err());
    // A missing project pointer is refused as a whole table.
    let table = ProposedRecommendations {
        definitions: vec![NamedRecommendation {
            key: "paris".into(),
            recommendation: paris(),
        }],
        project: "missing".into(),
    };
    let error = validate_recommendations(&Fixture, &table, &["paris".to_owned()], &authored)
        .map(|_| ())
        .unwrap_err();
    assert!(error.contains("missing"), "{error}");
    assert_eq!(authored, Settings::default());
    // With no recommendation the status row says so instead of rendering a stale scene.
    let model = RowModel::new(&semantic_inputs(SEMANTIC_ID), &RowContext::default());
    assert_eq!(status_of(&model).0, "Awaiting tree recommendation");
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_paris_attribution_and_wikipedia_policy_use_effective_kind
#[test]
fn semantic_paris_attribution_and_wikipedia_policy_use_effective_kind_twin() {
    let recommendation = validated(&paris());
    assert_eq!(recommendation.kind, "openstreetmap");
    // Rows that depend on the scene (location, wikipedia exclusions) follow the effective kind.
    let context = RowContext {
        effective_kind: Some(recommendation.kind.clone()),
        ..RowContext::default()
    };
    let semantic = RowModel::new(&semantic_inputs(SEMANTIC_ID), &context);
    assert_eq!(semantic.effective_kind(), Some("openstreetmap"));
    let wikipedia = RowModel::new(&semantic_inputs("wikipedia"), &RowContext::default());
    assert_eq!(wikipedia.effective_kind(), Some("wikipedia"));
    assert!(
        !wikipedia.rows().iter().any(|row| matches!(
            row,
            AnimationRow::Common("speed" | "fps_limit" | "density" | "dither")
        )),
        "Wikipedia hides the motion rows"
    );
    assert!(
        semantic
            .rows()
            .iter()
            .any(|row| matches!(row, AnimationRow::Common("panels"))),
        "the shared rows stay"
    );
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_project_entry_group_and_pane_fallback_are_local
#[test]
fn semantic_project_entry_group_and_pane_fallback_are_local_twin() {
    let authored = Settings::default();
    let table = ProposedRecommendations {
        definitions: vec![
            NamedRecommendation {
                key: "paris".into(),
                recommendation: paris(),
            },
            NamedRecommendation {
                key: "spin".into(),
                recommendation: carpet(1, vec![]),
            },
        ],
        project: "paris".into(),
    };
    // One pointer per output node, in preorder: a group, a pane, a pane.
    let pointers: Vec<String> = ["paris", "spin", "paris"].map(str::to_owned).into();
    let validated = validate_recommendations(&Fixture, &table, &pointers, &authored).unwrap();
    assert_eq!(validated.entries.len(), 3);
    let kinds: Vec<&str> = validated
        .entries
        .iter()
        .map(|entry| entry.kind.as_str())
        .collect();
    assert_eq!(kinds, ["openstreetmap", "carpet", "openstreetmap"]);
    assert_eq!(validated.project.kind, "openstreetmap");
    // Each entry resolves on its own; the project one is the fallback for unmapped nodes.
    let entry = resolve_recommendation(&Fixture, &validated.entries[1], &authored).unwrap();
    let project = resolve_recommendation(&Fixture, &validated.project, &authored).unwrap();
    assert_eq!(selected(&entry, "carpet_mode"), Some(1));
    assert_eq!(selected(&project, "source"), Some(0));
    assert_eq!(authored, Settings::default());
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_status_help_preserves_full_report_after_host_release
#[test]
fn semantic_status_help_preserves_full_report_after_host_release_twin() {
    let report = format!(
        "{}\nUNIQUE_REPORT_TAIL",
        "complete fixture status\n".repeat(80)
    );
    let mut context = RowContext {
        scene_status: Some(report.clone()),
        ..RowContext::default()
    };
    let model = RowModel::new(&semantic_inputs(SEMANTIC_ID), &context);
    let (value, help) = status_of(&model);
    assert_eq!(
        value, report,
        "the full report reaches the row, nothing is cut"
    );
    assert!(value.contains("UNIQUE_REPORT_TAIL"));
    assert!(help.contains("validated recommendation"));
    // The scene host is released: the report is gone and the row says what it waits for.
    context.scene_status = None;
    let released = RowModel::new(&semantic_inputs(SEMANTIC_ID), &context);
    assert_eq!(status_of(&released).0, "Awaiting tree recommendation");
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_undo_restores_effective_choice_and_stale_apply_preserves_it
#[test]
fn semantic_undo_restores_effective_choice_and_stale_apply_preserves_it_twin() {
    let authored = Settings::default();
    let previous = validated(&paris());
    let applied = validated(&carpet(7, vec![]));
    let resolved = resolve_recommendation(&Fixture, &applied, &authored).unwrap();
    assert_eq!(selected(&resolved, "carpet_mode"), Some(7));
    // Applying the same proposal again (a stale apply) yields the same canonical recommendation.
    assert_eq!(validated(&carpet(7, vec![])), applied);
    // Undo restores the previous recommendation, which resolves to its own effective choice.
    let restored = resolve_recommendation(&Fixture, &previous, &authored).unwrap();
    assert_eq!(restored.kind, "openstreetmap");
    assert_eq!(selected(&restored, "source"), Some(0));
    assert_eq!(authored, Settings::default());
    assert!(validate_recommendation(&Fixture, &proposal("carpet", vec![]), &authored).is_err());
}
