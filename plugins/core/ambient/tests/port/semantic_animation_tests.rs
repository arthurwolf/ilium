//! Twins of the tests of ilium-client/src/semantic_animation_tests.rs, run against the plugin's
//! `semantic` admission module over the synthetic registry of `semantic_fixture` (the kernel runs
//! them against the real scene registry, which stays in the kernel; see that file's header).

use crate::common::manifest_row;
use crate::semantic_fixture::{carpet, paris, proposal, selected, Fixture, Settings};
use ilium_plugin_core_ambient::control::ControlValue::{Bool, Index, Number, Text};
use ilium_plugin_core_ambient::control::{Control, ControlKind};
use ilium_plugin_core_ambient::semantic::{
    adaptable, exposable, resolve_recommendation, validate_recommendation,
    validate_recommendations, AnimationRecommendation, AnimationValue, NamedRecommendation,
    ProposedParameter, ProposedRecommendation, ProposedRecommendations, ResourcePolicy,
    SemanticScenes,
};
use serde_json::json;

fn base() -> Settings {
    Settings::default()
}

fn validate(
    proposal: &ProposedRecommendation,
    authored: &Settings,
) -> Result<AnimationRecommendation, String> {
    validate_recommendation(&Fixture, proposal, authored)
}

fn resolve(
    recommendation: &AnimationRecommendation,
    authored: &Settings,
) -> Result<Settings, String> {
    resolve_recommendation(&Fixture, recommendation, authored)
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::all_concrete_ids_and_descriptions_are_present_once_without_aliases
#[test]
fn all_concrete_ids_and_descriptions_are_present_once_without_aliases_twin() {
    for kind in ["waves", "carpet", "spectrum", "clouds", "openstreetmap"] {
        assert!(Fixture.is_known_kind(kind), "{kind}");
    }
    for alias in [
        "semantic",
        "Carpet",
        "carpet ",
        "",
        "../carpet",
        "dithered_waves_old",
        "carpet\n",
    ] {
        assert!(
            validate(&proposal(alias, vec![]), &base()).is_err(),
            "{alias:?} must be rejected"
        );
    }
    let carpet_rec = validate(&carpet(2, vec![]), &base()).unwrap();
    assert_eq!(carpet_rec.kind, "carpet");
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::conditional_inventory_is_not_default_only
#[test]
fn conditional_inventory_is_not_default_only_twin() {
    let mut spectrum = Settings {
        kind: "spectrum".into(),
        ..base()
    };
    let ceiling = |settings: &Settings| {
        let mut row = Fixture.scene_control(settings, "ceiling_db").unwrap();
        Fixture.narrow(settings, &mut row);
        match row.kind {
            ControlKind::Slider { min, .. } => min,
            _ => panic!("slider"),
        }
    };
    assert_eq!(ceiling(&spectrum), -50);
    spectrum.values.insert("floor_db".into(), Number(-30));
    assert_eq!(ceiling(&spectrum), -20, "the inventory follows the state");
    let osm = Settings {
        kind: "openstreetmap".into(),
        ..base()
    };
    let mut source = Fixture.scene_control(&osm, "source").unwrap();
    Fixture.narrow(&osm, &mut source);
    assert!(source.disabled_reason(1).is_some() && source.disabled_reason(2).is_some());
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::malformed_types_bounds_duplicates_and_inactive_controls_are_rejected
#[test]
fn malformed_types_bounds_duplicates_and_inactive_controls_are_rejected_twin() {
    let authored = base();
    for bad in [
        carpet(9, vec![]),
        carpet(1, vec![("carpet_spacing", Number(25))]),
        carpet(1, vec![("carpet_spacing", Bool(true))]),
        carpet(1, vec![("carpet_snake_grid", Number(5))]),
        carpet(1, vec![("carpet_easing_ms", Number(3001))]),
        carpet(1, vec![("carpet_dvd_speed", Number(20))]),
        carpet(
            1,
            vec![("carpet_seed", Number(1)), ("carpet_seed", Number(2))],
        ),
        carpet(1, vec![("carpet_seed", Text("untrusted".into()))]),
        proposal("carpet", vec![]),
        proposal("openstreetmap", vec![("place", Index(0))]),
        proposal("wikipedia", vec![("topic", Text("Paris".into()))]),
    ] {
        assert!(validate(&bad, &authored).is_err(), "{bad:?}");
    }
    assert!(serde_json::from_str::<ProposedRecommendation>(
        r#"{"kind":"carpet","resources":"catalog","parameters":[],"extra":1}"#
    )
    .is_err());
    assert!(serde_json::from_str::<ProposedRecommendation>(
        r#"{"kind":"carpet","resources":"catalog","parameters":[]}"#
    )
    .is_ok());
    assert!(serde_json::from_str::<ProposedRecommendations>(r#"{"definitions":[]}"#).is_err());
    assert!(serde_json::from_str::<ProposedParameter>(
        r#"{"id":"carpet_spacing","value":{"Number":2.5}}"#
    )
    .is_err());
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::dependency_order_and_every_carpet_mode_work_without_enabling_background
#[test]
fn dependency_order_and_every_carpet_mode_work_without_enabling_background_twin() {
    let authored = base();
    for mode in 0..9 {
        let rec = validate(&carpet(mode, vec![]), &authored).unwrap();
        let resolved = resolve(&rec, &authored).unwrap();
        assert_eq!(selected(&resolved, "carpet_mode"), Some(mode));
        assert!(!resolved.enabled);
    }
    let raw = carpet(
        1,
        vec![
            ("carpet_easing_ms", Number(2800)),
            ("carpet_snake_step_ms", Number(3000)),
        ],
    );
    let rec = validate(&raw, &authored).unwrap();
    let resolved = resolve(&rec, &authored).unwrap();
    assert_eq!(resolved.values["carpet_easing_ms"], Number(2800));
    assert_eq!(resolved.values["carpet_snake_step_ms"], Number(3000));
    assert_eq!(resolved.folders, authored.folders);
    assert_eq!(authored, base(), "the authored settings are never mutated");
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::paris_is_offline_even_when_authored_osm_was_custom
#[test]
fn paris_is_offline_even_when_authored_osm_was_custom_twin() {
    let mut authored = Settings {
        kind: "openstreetmap".into(),
        folders: "custom-saved-maps".into(),
        ..base()
    };
    authored.values.insert("source".into(), Index(2));
    let rec = validate(&paris(), &authored).unwrap();
    let resolved = resolve(&rec, &authored).unwrap();
    assert_eq!(selected(&resolved, "source"), Some(0), "offline maps");
    assert_eq!(
        resolved.folders, "custom-saved-maps",
        "authored input untouched"
    );
    for online in [1, 2] {
        let mut raw = paris();
        raw.parameters[0].value = Index(online);
        assert!(validate(&raw, &authored).is_err(), "source {online}");
    }
    for named in ["selection"] {
        let raw = proposal(
            "openstreetmap",
            vec![
                ("source", Index(0)),
                ("place", Index(0)),
                ("tour", Index(0)),
                (named, Index(1)),
            ],
        );
        assert!(
            validate(&raw, &authored).is_err(),
            "{named} stays an explicit user choice"
        );
    }
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::graph_source_precedes_candles_and_saved_choices_cannot_silently_change
#[test]
fn graph_source_precedes_candles_and_saved_choices_cannot_silently_change_twin() {
    let authored = base();
    let rec = validate(&carpet(3, vec![]), &authored).unwrap();
    let AnimationValue::Choice { index, label } = &rec.parameters[0].value else {
        panic!("choice");
    };
    assert_eq!(
        (*index, label.as_str()),
        (3, "d"),
        "the label is captured at validation"
    );
    let mut renamed = rec.clone();
    renamed.parameters[0].value = AnimationValue::Choice {
        index: 3,
        label: "Renamed".into(),
    };
    assert!(
        resolve(&renamed, &authored).is_err(),
        "a changed label is refused"
    );
    let mut shifted = rec.clone();
    shifted.parameters[0].value = AnimationValue::Choice {
        index: 4,
        label: "d".into(),
    };
    assert!(
        resolve(&shifted, &authored).is_err(),
        "a moved index is refused"
    );
    assert!(resolve(&rec, &authored).is_ok());
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::images_reuse_authored_inputs_or_explicitly_choose_curated_builtins
#[test]
fn images_reuse_authored_inputs_or_explicitly_choose_curated_builtins_twin() {
    let mut authored = Settings {
        kind: "images".into(),
        folders: "fixture-input-never-opened".into(),
        ..base()
    };
    authored.values.insert("mode".into(), Index(1));
    let mut folder = proposal("images", vec![("mode", Index(1))]);
    folder.resources = ResourcePolicy::Authored;
    let rec = validate(&folder, &authored).unwrap();
    assert_eq!(resolve(&rec, &authored).unwrap().folders, authored.folders);
    let builtin = proposal("images", vec![("mode", Index(0))]);
    assert!(
        validate(&builtin, &authored).is_ok(),
        "curated built-ins are explicit"
    );
    let catalog_folder = proposal("images", vec![("mode", Index(1))]);
    assert!(validate(&catalog_folder, &authored).is_err());
    let mut mismatch = proposal("images", vec![("mode", Index(0))]);
    mismatch.resources = ResourcePolicy::Authored;
    assert!(validate(&mismatch, &authored).is_err());
    let mut missing = folder;
    missing.resources = ResourcePolicy::Authored;
    let empty = Settings {
        folders: String::new(),
        ..authored
    };
    assert!(
        validate(&missing, &empty).is_err(),
        "authored images need their folder"
    );
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::video_and_fixed_time_require_existing_authored_resources
#[test]
fn video_and_fixed_time_require_existing_authored_resources_twin() {
    let raw = proposal("video", vec![("shuffle", Bool(true))]);
    assert!(validate(&raw, &base()).is_err(), "no authored clip");
    let authored = Settings {
        folders: "clip.mp4".into(),
        ..base()
    };
    let rec = validate(&raw, &authored).unwrap();
    let resolved = resolve(&rec, &authored).unwrap();
    assert_eq!(resolved.values["shuffle"], Bool(true));
    assert_eq!(resolved.folders, "clip.mp4");
    // The authored clip disappears later: the saved recommendation is refused, not rendered.
    assert!(resolve(&rec, &base()).is_err());
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::capture_and_renderer_selection_are_not_semantic_parameters
#[test]
fn capture_and_renderer_selection_are_not_semantic_parameters_twin() {
    let mut authored = Settings {
        kind: "spectrum".into(),
        ..base()
    };
    authored.values.insert("input".into(), Index(1));
    let rec = validate(&proposal("spectrum", vec![("style", Index(6))]), &authored).unwrap();
    let resolved = resolve(&rec, &authored).unwrap();
    assert_eq!(
        selected(&resolved, "input"),
        Some(1),
        "the capture input stays authored"
    );
    for raw in [
        proposal("spectrum", vec![("input", Index(0))]),
        proposal("images", vec![("recursive", Bool(true))]),
        proposal("voxel-landscape", vec![("pack_profile", Index(1))]),
        proposal("voxel-landscape", vec![("pack_mount_depth", Number(1))]),
    ] {
        assert!(validate(&raw, &authored).is_err(), "{raw:?}");
    }
    for id in [
        "input",
        "device_name",
        "render_backend",
        "backend",
        "renderer",
        "gpu",
        "compute_backend",
    ] {
        let row = Control::toggle(id, id, false, "");
        assert!(!exposable(&row), "{id}");
    }
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::pointer_table_is_required_total_and_round_trips_durable_values
#[test]
fn pointer_table_is_required_total_and_round_trips_durable_values_twin() {
    let authored = base();
    let definition = NamedRecommendation {
        key: "paris".into(),
        recommendation: paris(),
    };
    let mut table = ProposedRecommendations {
        definitions: vec![definition.clone()],
        project: "paris".into(),
    };
    let check = |table: &ProposedRecommendations, pointers: &[&str]| {
        let pointers: Vec<String> = pointers.iter().map(|pointer| pointer.to_string()).collect();
        validate_recommendations(&Fixture, table, &pointers, &authored)
    };
    let good = check(&table, &["paris", "paris"]).unwrap();
    assert_eq!(
        good.entries,
        vec![good.project.clone(), good.project.clone()]
    );
    let encoded = serde_json::to_string(&good.project).unwrap();
    let restored: AnimationRecommendation = serde_json::from_str(&encoded).unwrap();
    assert_eq!(restored, good.project);
    assert!(resolve(&restored, &authored).is_ok());
    assert!(check(&table, &["missing"]).is_err());
    assert!(check(&table, &[""]).is_err());
    table.definitions.push(definition);
    assert!(check(&table, &["paris"]).is_err(), "duplicate key");
    table.definitions[1].key = "unused".into();
    assert!(check(&table, &["paris"]).is_err(), "unused definition");
    table.project.clear();
    assert!(check(&table, &["paris"]).is_err(), "empty project pointer");
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values
#[test]
fn numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values_twin() {
    let authored = base();
    let spectrum = |parameters| proposal("spectrum", parameters);
    let cases = [
        spectrum(vec![("ceiling_db", Number(-20)), ("floor_db", Number(-30))]),
        carpet(
            1,
            vec![
                ("carpet_easing_ms", Number(600)),
                ("carpet_snake_step_ms", Number(800)),
                ("carpet_snake_grid", Number(4)),
            ],
        ),
    ];
    for raw in cases {
        let forward = validate(&raw, &authored).unwrap();
        let mut reverse = raw;
        reverse.parameters.reverse();
        assert_eq!(validate(&reverse, &authored).unwrap(), forward);
        assert!(!resolve(&forward, &authored).unwrap().enabled);
    }
    for raw in [
        spectrum(vec![("ceiling_db", Number(-25)), ("floor_db", Number(-30))]),
        carpet(
            1,
            vec![
                ("carpet_easing_ms", Number(801)),
                ("carpet_snake_step_ms", Number(800)),
            ],
        ),
    ] {
        assert!(validate(&raw, &authored).is_err(), "{raw:?}");
    }
    let mut images = Settings {
        kind: "images".into(),
        folders: "fixture-input-never-opened".into(),
        ..base()
    };
    images.values.insert("mode".into(), Index(1));
    let mut raw = proposal(
        "images",
        vec![
            ("transition_seconds", Number(30)),
            ("display_seconds", Number(120)),
        ],
    );
    raw.resources = ResourcePolicy::Authored;
    let forward = validate(&raw, &images).unwrap();
    raw.parameters.reverse();
    assert_eq!(validate(&raw, &images).unwrap(), forward);
    raw.parameters
        .iter_mut()
        .find(|parameter| parameter.id == "display_seconds")
        .unwrap()
        .value = Number(3);
    assert!(validate(&raw, &images).is_err());
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work
#[test]
fn provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work_twin() {
    let authored = base();
    assert!(validate(
        &proposal("clouds", vec![("refresh_minutes", Number(5))]),
        &authored
    )
    .is_err());
    let coverage = validate(
        &proposal("clouds", vec![("coverage", Number(70))]),
        &authored,
    )
    .unwrap();
    assert_eq!(
        resolve(&coverage, &authored).unwrap().values["coverage"],
        Number(70)
    );
    let rec = validate(
        &proposal("spectrum", vec![("refresh_rate", Number(15))]),
        &authored,
    )
    .unwrap();
    assert_eq!(rec.parameters.len(), 1);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::provider_polling_remains_authored_while_render_controls_remain_available
#[test]
fn provider_polling_remains_authored_while_render_controls_remain_available_twin() {
    for (kind, id) in [("clouds", "refresh_minutes"), ("clouds", "refresh_hours")] {
        let settings = Settings {
            kind: kind.into(),
            ..base()
        };
        if let Some(row) = Fixture.scene_control(&settings, id) {
            assert!(!exposable(&row), "{id}");
        }
        assert!(
            validate(&proposal(kind, vec![(id, Number(5))]), &base()).is_err(),
            "{id}"
        );
    }
    let spectrum = Settings {
        kind: "spectrum".into(),
        ..base()
    };
    let refresh = Fixture.scene_control(&spectrum, "refresh_rate").unwrap();
    assert!(exposable(&refresh) && adaptable("spectrum", &refresh));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::semantic_selection_is_opt_in_and_scope_defaults_to_project
#[test]
fn semantic_selection_is_opt_in_and_scope_defaults_to_project_twin() {
    // The plugin's semantic row is an opt-in switch, off by default; the scope enum itself is
    // kernel state (KERNEL-STAYS) and resolution never turns the background on.
    assert_eq!(manifest_row("semantic")["default"], json!(false));
    let authored = base();
    assert!(!authored.enabled);
    let rec = validate(&carpet(1, vec![]), &authored).unwrap();
    let resolved = resolve(&rec, &authored).unwrap();
    assert!(!resolved.enabled);
    assert_ne!(resolved.kind, "semantic");
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::unresolved_semantic_policy_does_not_render_a_stale_scene
#[test]
fn unresolved_semantic_policy_does_not_render_a_stale_scene_twin() {
    let authored = base();
    let rec = validate(&carpet(1, vec![]), &authored).unwrap();
    for broken in [
        AnimationRecommendation {
            kind: "semantic".into(),
            ..rec.clone()
        },
        AnimationRecommendation {
            kind: "unknown_scene".into(),
            ..rec.clone()
        },
        AnimationRecommendation {
            version: rec.version + 1,
            ..rec.clone()
        },
    ] {
        assert!(
            resolve(&broken, &authored).is_err(),
            "an unresolved policy yields no settings to render: {broken:?}"
        );
    }
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::voxel_visual_choices_preserve_available_authored_resource_inventory
#[test]
fn voxel_visual_choices_preserve_available_authored_resource_inventory_twin() {
    let authored = Settings {
        kind: "voxel-landscape".into(),
        folders: "saved-world-folder".into(),
        ..base()
    };
    let rec = validate(
        &proposal(
            "voxel-landscape",
            vec![("zoom", Number(7)), ("caves", Number(1))],
        ),
        &authored,
    )
    .unwrap();
    let resolved = resolve(&rec, &authored).unwrap();
    assert_eq!(resolved.folders, "saved-world-folder");
    assert_eq!(resolved.values["zoom"], Number(7));
    for id in ["pack_profile", "pack_mount_depth"] {
        assert!(
            validate(
                &proposal("voxel-landscape", vec![(id, Index(1))]),
                &authored
            )
            .is_err(),
            "{id}"
        );
    }
}
