use super::*;
use ControlValue::{Bool, Index, Number, Text};

#[test]
fn provider_polling_remains_authored_while_render_controls_remain_available() {
    for (kind, id) in [
        (AnimationKind::Clouds, "refresh_minutes"),
        (AnimationKind::NightLights, "refresh_hours"),
    ] {
        assert!(!inventory(kind).unwrap().contains_key(id));
        assert!(validate_recommendation(
            &proposal(kind, vec![(id, Number(5))]),
            &AnimationSettings::default(),
        )
        .is_err());
    }
    assert!(inventory(AnimationKind::Spectrum)
        .unwrap()
        .contains_key("refresh_rate"));
}

#[test]
fn semantic_selection_is_opt_in_and_scope_defaults_to_project() {
    use crate::background_animation::SemanticScope;
    let mut settings = AnimationSettings::default();
    assert!(!settings.enabled);
    assert_ne!(settings.kind, AnimationKind::Semantic);
    assert_eq!(settings.semantic_scope, SemanticScope::Project);
    settings.kind = AnimationKind::Semantic;
    assert!(settings.scene_control("semantic_scope").is_some());
    settings
        .set_scene_control("semantic_scope", Index(1))
        .unwrap();
    assert_eq!(settings.semantic_scope, SemanticScope::Entry);
    let encoded = serde_json::to_string(&settings).unwrap();
    let reloaded: AnimationSettings = serde_json::from_str(&encoded).unwrap();
    assert_eq!(reloaded, settings);
    assert!(!reloaded.enabled);
    assert!(settings
        .set_scene_control("semantic_scope", Index(2))
        .is_err());
    assert_eq!(settings.semantic_scope, SemanticScope::Entry);
}

#[test]
fn unresolved_semantic_policy_does_not_render_a_stale_scene() {
    use crate::background_animation::AnimationFrame;
    use std::time::Duration;
    let mut frame = AnimationFrame::default();
    let mut settings = AnimationSettings::default();
    frame.render(&settings, 20, 8, Duration::from_secs(1));
    settings.kind = AnimationKind::Semantic;
    assert!(!settings.uses_loop_cache());
    frame.render(&settings, 20, 8, Duration::from_secs(2));
    for y in 0..8 {
        for x in 0..20 {
            assert_eq!(frame.glyph(x, y), ' ');
        }
    }
}

fn proposal(kind: AnimationKind, parameters: Vec<(&str, ControlValue)>) -> ProposedRecommendation {
    ProposedRecommendation {
        kind: canonical_id(kind).unwrap(),
        resources: ResourcePolicy::Catalog,
        parameters: parameters
            .into_iter()
            .map(|(id, value)| ProposedParameter {
                id: id.into(),
                value,
            })
            .collect(),
    }
}

fn carpet(mode: usize, parameters: Vec<(&str, ControlValue)>) -> ProposedRecommendation {
    let mut result = proposal(AnimationKind::Carpet, parameters);
    result.parameters.push(ProposedParameter {
        id: "carpet_mode".into(),
        value: Index(mode),
    });
    result
}

fn paris() -> ProposedRecommendation {
    proposal(
        AnimationKind::OpenStreetMap,
        vec![
            ("source", Index(0)),
            ("place", Index(0)),
            ("tour", Index(0)),
        ],
    )
}

#[test]
fn all_concrete_ids_and_descriptions_are_present_once_without_aliases() {
    let text = catalog().unwrap();
    println!(
        "{}",
        serde_json::json!({
            "type": "artifact",
            "catalog_characters": text.chars().count(),
            "catalog_bytes": text.len(),
            "scene_sections": text.split('\n').filter(|line| line.starts_with('"')).count(),
        })
    );
    let mut count = 0;
    for kind in AnimationKind::ALL {
        let id = canonical_id(kind).unwrap();
        if id == "semantic" {
            continue;
        }
        count += 1;
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with(&format!("{id:?} | ")))
                .count(),
            1
        );
        assert!(text.contains(kind.description()));
        assert_eq!(kind_for(&id).unwrap(), kind);
    }
    assert_eq!(
        count, 40,
        "Review the frozen inventory when concrete scenes change"
    );
    assert!(
        text.chars().count() < 32_000,
        "The catalog alone cannot exceed the prompt ceiling"
    );
    assert!(!text.contains("semantic | "));
    assert!(kind_for("semantic").is_err());
    assert!(kind_for("breathing_mountain").is_err());
    assert!(kind_for("not_a_scene").is_err());
}

#[test]
fn conditional_inventory_is_not_default_only() {
    for (kind, ids) in [
        (
            AnimationKind::Carpet,
            vec![
                "carpet_hunters_count",
                "carpet_snake_food_count",
                "carpet_life_wrap",
                "carpet_chess_ai_depth",
                "carpet_king_height",
                "carpet_dvd_speed",
                "carpet_orbit_scale",
                "carpet_clock_24h",
                "carpet_clock_tubes",
            ],
        ),
        (AnimationKind::Graph, vec!["rising_hue", "falling_hue"]),
        (AnimationKind::Wikipedia, vec!["wiki_zoom"]),
        (
            AnimationKind::Shoreline,
            vec!["shoreline_lace", "shoreline_wet_memory"],
        ),
        (AnimationKind::TopographicMaps, vec!["tide_seconds"]),
        (
            AnimationKind::Stars,
            vec!["lens", "look_altitude", "star_colors", "star_size"],
        ),
        (
            AnimationKind::Images,
            vec!["order", "shuffle_seed", "transition_seconds", "easing"],
        ),
        (
            AnimationKind::Video,
            vec!["slowed_percent", "scene_seconds", "seed"],
        ),
        (
            AnimationKind::Spectrum,
            vec!["bar_gap", "spectrogram_speed", "orientation"],
        ),
    ] {
        let rows = inventory(kind).unwrap();
        for id in ids {
            assert!(rows.contains_key(id), "{kind:?}: {id}");
        }
        assert!(rows.values().all(exposable));
    }
    let rows = inventory(AnimationKind::Carpet).unwrap();
    assert_eq!(rows.len(), 45);
    assert_eq!(rows["carpet_infinite_lines"].value, Bool(true));
    let ControlKind::Choice { options } = &rows["carpet_mode"].kind else {
        panic!("mode")
    };
    assert_eq!(options.len(), 9);
    assert_eq!(options[1], "Autonomous Snake");
    let graph = inventory(AnimationKind::Graph).unwrap();
    let ControlKind::Choice { options } = &graph["source"].kind else {
        panic!("source")
    };
    assert_eq!(options.len(), 32);
}

#[test]
fn malformed_types_bounds_duplicates_and_inactive_controls_are_rejected() {
    let base = AnimationSettings::default();
    for bad in [
        carpet(9, vec![]),
        carpet(1, vec![("carpet_spacing", Number(25))]),
        carpet(1, vec![("carpet_spacing", Bool(true))]),
        carpet(1, vec![("carpet_snake_grid", Number(5))]),
        carpet(
            1,
            vec![
                ("carpet_snake_grid", Number(4)),
                ("carpet_snake_initial_length", Number(16)),
            ],
        ),
        carpet(1, vec![("carpet_easing_ms", Number(201))]),
        carpet(7, vec![("carpet_easing_ms", Number(901))]),
        carpet(1, vec![("carpet_dvd_speed", Number(20))]),
        carpet(
            1,
            vec![("carpet_seed", Number(1)), ("carpet_seed", Number(2))],
        ),
        carpet(1, vec![("carpet_seed", Text("untrusted".into()))]),
        proposal(AnimationKind::Carpet, vec![]),
        proposal(AnimationKind::OpenStreetMap, vec![("place", Index(0))]),
        proposal(AnimationKind::Graph, vec![("poll", Number(5))]),
        proposal(
            AnimationKind::Wikipedia,
            vec![("topic", Text("Paris".into()))],
        ),
    ] {
        assert!(validate_recommendation(&bad, &base).is_err(), "{bad:?}");
    }
    assert!(serde_json::from_str::<ProposedRecommendation>(
        r#"{"kind":"shoreline","parameters":[]}"#
    )
    .is_err());
    assert!(serde_json::from_str::<ProposedRecommendations>(r#"{"definitions":[]}"#).is_err());
    assert!(serde_json::from_str::<ProposedParameter>(
        r#"{"id":"carpet_spacing","value":{"Number":2.5}}"#
    )
    .is_err());
}

#[test]
fn dependency_order_and_every_carpet_mode_work_without_enabling_background() {
    let base = AnimationSettings::default();
    for mode in 0..9 {
        let rec = validate_recommendation(&carpet(mode, vec![]), &base).unwrap();
        let resolved = resolve_recommendation(&rec, &base).unwrap();
        assert_eq!(resolved.ambient.carpet.mode, mode as i32);
        assert!(!resolved.enabled);
    }
    let raw = carpet(
        1,
        vec![
            ("carpet_easing_ms", Number(2800)),
            ("carpet_snake_step_ms", Number(3000)),
        ],
    );
    let rec = validate_recommendation(&raw, &base).unwrap();
    let resolved = resolve_recommendation(&rec, &base).unwrap();
    assert_eq!(resolved.ambient.carpet.easing_ms, 2800);
    assert_eq!(resolved.ambient.carpet.snake_step_ms, 3000);
    assert_eq!(resolved.appearance, base.appearance);
    assert_eq!(resolved.fps_limit, base.fps_limit);
    assert_eq!(base, AnimationSettings::default());
}

#[test]
fn paris_is_offline_even_when_authored_osm_was_custom() {
    let mut base = AnimationSettings::default();
    base.ambient.openstreetmap.source = 2;
    base.ambient.openstreetmap.endpoint = "authored endpoint sentinel".into();
    base.ambient.openstreetmap.place = 9;
    base.ambient.openstreetmap.tour = 2;
    let before = base.clone();
    let rec = validate_recommendation(&paris(), &base).unwrap();
    let resolved = resolve_recommendation(&rec, &base).unwrap();
    assert_eq!(resolved.ambient.openstreetmap.source, 0);
    assert_eq!(resolved.ambient.openstreetmap.place, 0);
    assert_eq!(resolved.ambient.openstreetmap.tour, 0);
    assert_eq!(base, before);
    assert!(!resolved.enabled);
    for source in [1, 2, 3] {
        let mut raw = paris();
        raw.parameters[0].value = Index(source);
        assert!(validate_recommendation(&raw, &base).is_err());
    }
}

#[test]
fn graph_source_precedes_candles_and_saved_choices_cannot_silently_change() {
    let base = AnimationSettings::default();
    let raw = proposal(
        AnimationKind::Graph,
        vec![
            ("rising_hue", Number(100)),
            ("mode", Index(2)),
            ("source", Index(0)),
        ],
    );
    let mut rec = validate_recommendation(&raw, &base).unwrap();
    assert!(resolve_recommendation(&rec, &base).is_ok());
    let mut bad = raw;
    bad.parameters[2].value = Index(8);
    assert!(validate_recommendation(&bad, &base).is_err());
    let choice = rec
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == "source")
        .unwrap();
    let AnimationValue::Choice { label, .. } = &mut choice.value else {
        panic!("choice")
    };
    label.push_str(" changed");
    assert!(resolve_recommendation(&rec, &base).is_err());
}

#[test]
fn images_reuse_authored_inputs_or_explicitly_choose_curated_builtins() {
    let mut base = AnimationSettings {
        kind: AnimationKind::Images,
        ..Default::default()
    };
    base.set_scene_control("mode", Index(1)).unwrap();
    base.ambient.images.folders = "synthetic-folder-not-opened".into();
    let before = base.clone();
    let mut reused = proposal(AnimationKind::Images, vec![("motion", Index(5))]);
    reused.resources = ResourcePolicy::Authored;
    let rec = validate_recommendation(&reused, &base).unwrap();
    let resolved = resolve_recommendation(&rec, &base).unwrap();
    assert_eq!(selected(&resolved, "mode"), Some(1));
    assert_eq!(resolved.ambient.images.folders, base.ambient.images.folders);
    let builtin = proposal(AnimationKind::Images, vec![("builtin_image", Index(2))]);
    let rec = validate_recommendation(&builtin, &base).unwrap();
    let resolved = resolve_recommendation(&rec, &base).unwrap();
    assert_eq!(selected(&resolved, "mode"), Some(0));
    assert_eq!(selected(&resolved, "source_kind"), Some(0));
    assert_eq!(selected(&resolved, "builtin_image"), Some(2));
    assert_eq!(base, before);
    assert!(!authored_capabilities(&base)
        .to_string()
        .contains("synthetic-folder"));
    let bad = proposal(AnimationKind::Images, vec![("mode", Index(1))]);
    assert!(validate_recommendation(&bad, &base).is_err());
    base.ambient.images.folders.clear();
    assert!(validate_recommendation(&reused, &base).is_err());
}

#[test]
fn video_and_fixed_time_require_existing_authored_resources() {
    let mut base = AnimationSettings::default();
    let mut video = proposal(
        AnimationKind::Video,
        vec![("scene_seconds", Number(10)), ("mode", Index(2))],
    );
    video.resources = ResourcePolicy::Authored;
    assert!(validate_recommendation(&video, &base).is_err());
    base.ambient.video.source = "synthetic-video-not-opened.mp4".into();
    let rec = validate_recommendation(&video, &base).unwrap();
    assert_eq!(
        resolve_recommendation(&rec, &base)
            .unwrap()
            .ambient
            .video
            .source,
        base.ambient.video.source
    );
    let mut stars = proposal(AnimationKind::Stars, vec![("start_from", Index(1))]);
    stars.resources = ResourcePolicy::Authored;
    assert!(validate_recommendation(&stars, &base).is_err());
    base.ambient.stars.start_datetime = "2026-12-21 22:00".into();
    let rec = validate_recommendation(&stars, &base).unwrap();
    assert!(resolve_recommendation(&rec, &base)
        .unwrap()
        .ambient
        .stars
        .fixed_start_unix()
        .is_some());
    stars.resources = ResourcePolicy::Catalog;
    assert!(validate_recommendation(&stars, &base).is_err());
}

#[test]
fn capture_and_renderer_selection_are_not_semantic_parameters() {
    let mut base = AnimationSettings {
        kind: AnimationKind::Spectrum,
        ..Default::default()
    };
    base.set_scene_control("input", Index(1)).unwrap();
    let raw = proposal(AnimationKind::Spectrum, vec![("style", Index(6))]);
    let rec = validate_recommendation(&raw, &base).unwrap();
    let resolved = resolve_recommendation(&rec, &base).unwrap();
    assert_eq!(selected(&resolved, "input"), Some(1));
    assert_eq!(
        resolved.ambient.spectrum.device_name,
        base.ambient.spectrum.device_name
    );
    for raw in [
        proposal(AnimationKind::Spectrum, vec![("input", Index(0))]),
        proposal(AnimationKind::FbmClouds, vec![("render_backend", Index(1))]),
        proposal(AnimationKind::Images, vec![("recursive", Bool(true))]),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_profile", Index(1))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_mount", Index(1))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_duplicate_last_wins", Bool(true))],
        ),
    ] {
        assert!(validate_recommendation(&raw, &base).is_err());
    }
}

#[test]
fn pointer_table_is_required_total_and_round_trips_durable_values() {
    let base = AnimationSettings::default();
    let definition = NamedRecommendation {
        key: "paris".into(),
        recommendation: paris(),
    };
    let mut table = ProposedRecommendations {
        definitions: vec![definition.clone()],
        project: "paris".into(),
    };
    let good = validate_recommendations(&table, &["paris".into(), "paris".into()], &base).unwrap();
    assert_eq!(
        good.entries,
        vec![good.project.clone(), good.project.clone()]
    );
    let encoded = serde_json::to_string(&good.project).unwrap();
    let restored: AnimationRecommendation = serde_json::from_str(&encoded).unwrap();
    assert_eq!(restored, good.project);
    assert!(resolve_recommendation(&restored, &base).is_ok());
    assert!(validate_recommendations(&table, &["missing".into()], &base).is_err());
    assert!(validate_recommendations(&table, &["".into()], &base).is_err());
    table.definitions.push(definition);
    assert!(validate_recommendations(&table, &["paris".into()], &base).is_err());
    table.definitions[1].key = "unused".into();
    assert!(validate_recommendations(&table, &["paris".into()], &base).is_err());
    table.project.clear();
    assert!(validate_recommendations(&table, &["paris".into()], &base).is_err());
}

#[test]
fn numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values() {
    let base = AnimationSettings::default();
    let cases = [
        proposal(
            AnimationKind::TopographicMaps,
            vec![("tide_seconds", Number(120)), ("tide_range_m", Number(50))],
        ),
        proposal(
            AnimationKind::Spectrum,
            vec![("ceiling_db", Number(-20)), ("floor_db", Number(-30))],
        ),
        carpet(
            1,
            vec![
                ("carpet_easing_ms", Number(600)),
                ("carpet_snake_step_ms", Number(800)),
                ("carpet_snake_initial_length", Number(15)),
                ("carpet_snake_grid", Number(4)),
            ],
        ),
    ];
    for raw in cases {
        let forward = validate_recommendation(&raw, &base).unwrap();
        let mut reverse = raw;
        reverse.parameters.reverse();
        assert_eq!(validate_recommendation(&reverse, &base).unwrap(), forward);
        assert!(!resolve_recommendation(&forward, &base).unwrap().enabled);
    }
    for raw in [
        proposal(
            AnimationKind::TopographicMaps,
            vec![("tide_seconds", Number(120)), ("tide_range_m", Number(0))],
        ),
        proposal(
            AnimationKind::Spectrum,
            vec![("ceiling_db", Number(-25)), ("floor_db", Number(-30))],
        ),
        carpet(
            1,
            vec![
                ("carpet_easing_ms", Number(801)),
                ("carpet_snake_step_ms", Number(800)),
            ],
        ),
    ] {
        assert!(validate_recommendation(&raw, &base).is_err());
    }
    let mut authored = base.clone();
    authored.kind = AnimationKind::Images;
    authored.set_scene_control("mode", Index(1)).unwrap();
    authored.ambient.images.folders = "fixture-input-never-opened".into();
    let mut raw = proposal(
        AnimationKind::Images,
        vec![
            ("transition_seconds", Number(30)),
            ("display_seconds", Number(120)),
        ],
    );
    raw.resources = ResourcePolicy::Authored;
    let forward = validate_recommendation(&raw, &authored).unwrap();
    raw.parameters.reverse();
    assert_eq!(validate_recommendation(&raw, &authored).unwrap(), forward);
    raw.parameters
        .iter_mut()
        .find(|parameter| parameter.id == "display_seconds")
        .unwrap()
        .value = Number(3);
    assert!(validate_recommendation(&raw, &authored).is_err());
}

#[test]
fn provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work() {
    // No provider scene is constructed.
    let authored = AnimationSettings::default();
    for raw in [
        proposal(AnimationKind::Clouds, vec![("refresh_minutes", Number(5))]),
        proposal(
            AnimationKind::NightLights,
            vec![("refresh_hours", Number(1))],
        ),
    ] {
        // Try both newly identified polling controls.
        assert!(validate_recommendation(&raw, &authored).is_err());
    }
    let raw = proposal(
        AnimationKind::Clouds,
        vec![
            ("playback_fps", Number(8)),
            ("rotation_deg_per_min", Number(5)),
            ("land_underlay_percent", Number(20)),
            ("history_hours", Index(1)),
            ("coverage", Index(1)),
            ("projection", Index(1)),
            ("land_underlay", Bool(true)),
        ],
    );
    let first = validate_recommendation(&raw, &authored).unwrap();
    let mut reversed = raw;
    reversed.parameters.reverse();
    assert_eq!(
        validate_recommendation(&reversed, &authored).unwrap(),
        first
    );
    let effective = resolve_recommendation(&first, &authored).unwrap();
    assert_eq!(effective.ambient.clouds.playback_fps, 8);
    assert_eq!(
        effective.ambient.clouds.refresh_minutes,
        authored.ambient.clouds.refresh_minutes
    );
    let clouds = inventory(AnimationKind::Clouds).unwrap();
    for id in [
        "projection",
        "rotation_deg_per_min",
        "zoom_level",
        "playback_fps",
        "smoothing_percent",
        "land_underlay_percent",
    ] {
        assert!(clouds.contains_key(id), "{id}");
    }
    assert!(!clouds.contains_key("refresh_minutes"));
    let lights = inventory(AnimationKind::NightLights).unwrap();
    for id in [
        "rotation_deg_per_min",
        "terminator_strength_percent",
        "coastline_strength_percent",
    ] {
        assert!(lights.contains_key(id), "{id}");
    }
    assert!(!lights.contains_key("refresh_hours"));
}
#[test]
fn voxel_visual_choices_preserve_available_authored_resource_inventory() {
    // Exercise actual settings setters, never the resource loader.
    let mut authored = AnimationSettings {
        kind: AnimationKind::VoxelLandscape,
        ..Default::default()
    };
    if authored.scene_control("world_source").is_some() {
        authored
            .set_scene_control("world_source", Index(1))
            .unwrap();
        authored
            .set_scene_control(
                "saved_maps_folder",
                Text(
                    std::env::temp_dir()
                        .join("semantic-world-fixture-not-opened")
                        .to_string_lossy()
                        .into_owned(),
                ),
            )
            .unwrap();
    }
    // Exercise every resource interface present in this scene version without
    // importing its loader or depending on unreleased private scene methods.
    let mut resource_fixture = serde_json::to_value(&authored.ambient.voxel_landscape).unwrap();
    let fields = resource_fixture.as_object_mut().unwrap();
    for name in ["pack_path", "pack_addon_path"] {
        if fields.contains_key(name) {
            fields.insert(
                name.into(),
                serde_json::json!(format!("fixture-{name}-not-opened.zip")),
            );
        }
    }
    if fields.contains_key("pack_custom_sources") {
        fields.insert(
            "pack_custom_sources".into(),
            serde_json::json!({
                "fixture-profile": {
                    "path": "fixture-hidden-not-opened.zip", "root": "", "mount": 0,
                    "format_major": 999, "format_minor": 0, "edition": 0,
                    "addon_path": "", "addon_mount": 0, "duplicate_last_wins": false
                }
            }),
        );
    }
    authored.ambient.voxel_landscape = serde_json::from_value(resource_fixture).unwrap();
    let before = authored.clone();
    let raw = proposal(
        AnimationKind::VoxelLandscape,
        vec![
            ("palette", Index(1)),
            ("color_mode", Index(1)),
            ("zoom", Number(150)),
        ],
    );
    let recommendation = validate_recommendation(&raw, &authored).unwrap();
    let effective = resolve_recommendation(&recommendation, &authored).unwrap();
    assert!(same_protected_inputs(&authored, &effective));
    assert_eq!(effective.ambient.voxel_landscape.palette, 1);
    assert_eq!(effective.ambient.voxel_landscape.zoom_percent, 150);
    assert_eq!(authored, before);
    for raw in [
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("world_source", Index(0))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_profile", Index(1))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_mount", Index(1))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_edition", Index(1))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_addon_mount", Index(1))],
        ),
        proposal(
            AnimationKind::VoxelLandscape,
            vec![("pack_duplicate_last_wins", Bool(true))],
        ),
    ] {
        // Cover all identified indirect resource selectors.
        assert!(validate_recommendation(&raw, &authored).is_err());
    }
    let mut invalid_resource = serde_json::to_value(&authored.ambient.voxel_landscape).unwrap();
    if invalid_resource.get("pack_profile").is_some() {
        invalid_resource["pack_profile"] = serde_json::json!(usize::MAX);
        authored.ambient.voxel_landscape = serde_json::from_value(invalid_resource).unwrap();
        assert!(validate_recommendation(
            &proposal(AnimationKind::VoxelLandscape, vec![]),
            &authored
        )
        .is_err());
    }
    let mut changed_identity = before.clone();
    changed_identity.ambient.voxel_landscape.seed = changed_identity
        .ambient
        .voxel_landscape
        .seed
        .wrapping_add(1);
    assert!(!same_protected_inputs(&before, &changed_identity));
}
