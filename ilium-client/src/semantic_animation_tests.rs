use super::*;
use ControlValue::{Bool, Index, Number, Text};

#[test]
fn capture_inputs_remain_authored_while_render_controls_remain_available() {
    for (kind, id) in [(AnimationKind::Spectrum, "input")] {
        assert!(!inventory(kind).unwrap().contains_key(id));
        assert!(validate_recommendation(
            &proposal(kind, vec![(id, Index(0))]),
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
        if id == "semantic" || kind == AnimationKind::PluginScene {
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
        count, 21,
        "Review the frozen inventory when concrete scenes change"
    );
    assert!(
        text.chars().count() < 32_000,
        "The catalog alone cannot exceed the prompt ceiling"
    );
    assert!(!text.contains("semantic | "));
    assert!(kind_for("semantic").is_err());
    assert!(kind_for("plugin_scene").is_err());
    assert!(kind_for("breathing_mountain").is_err());
    // Scenes that moved into the Scenes plugin load as plugin scenes, so they are aliases here.
    assert!(kind_for("carpet").is_err());
    assert!(kind_for("chess").is_err());
    assert!(kind_for("not_a_scene").is_err());
}

#[test]
fn conditional_inventory_is_not_default_only() {
    for (kind, ids) in [
        (AnimationKind::Wikipedia, vec!["wiki_zoom"]),
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
}

#[test]
fn malformed_types_bounds_duplicates_and_inactive_controls_are_rejected() {
    let base = AnimationSettings::default();
    for bad in [
        proposal(AnimationKind::OpenStreetMap, vec![("place", Index(0))]),
        proposal(AnimationKind::Spectrum, vec![("input", Index(0))]),
        proposal(
            AnimationKind::Wikipedia,
            vec![("topic", Text("Paris".into()))],
        ),
    ] {
        assert!(validate_recommendation(&bad, &base).is_err(), "{bad:?}");
    }
    assert!(
        serde_json::from_str::<ProposedRecommendation>(r#"{"kind":"images","parameters":[]}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<ProposedRecommendations>(r#"{"definitions":[]}"#).is_err());
    assert!(serde_json::from_str::<ProposedParameter>(
        r#"{"id":"tide_seconds","value":{"Number":2.5}}"#
    )
    .is_err());
}

#[test]
fn paris_is_offline_even_when_authored_osm_was_custom() {
    let mut base = AnimationSettings::default();
    base.ambient.openstreetmap.source = 2;
    base.ambient.openstreetmap.endpoint = "authored endpoint sentinel".into();
    base.ambient.openstreetmap.place = 9;
    base.ambient.openstreetmap.tour = 2;
    base.kind = AnimationKind::OpenStreetMap;
    base.set_scene_control("selection", Index(1)).unwrap();
    base.ambient.openstreetmap.place_list = "authored list sentinel".into();
    base.ambient.openstreetmap.destination_id = "authored destination sentinel".into();
    base.ambient.openstreetmap.coordinates = "10,20".into();
    let before = base.clone();
    let rec = validate_recommendation(&paris(), &base).unwrap();
    let resolved = resolve_recommendation(&rec, &base).unwrap();
    assert_eq!(resolved.ambient.openstreetmap.source, 0);
    assert_eq!(resolved.ambient.openstreetmap.place, 0);
    assert_eq!(resolved.ambient.openstreetmap.tour, 0);
    assert_eq!(
        resolved.ambient.openstreetmap.search,
        before.ambient.openstreetmap.search
    );
    assert_eq!(
        resolved.ambient.openstreetmap.place_list,
        before.ambient.openstreetmap.place_list
    );
    assert_eq!(
        resolved.ambient.openstreetmap.destination_id,
        before.ambient.openstreetmap.destination_id
    );
    assert_eq!(resolved.ambient.openstreetmap.coordinates, "10,20");
    for id in ["selection", "place_list", "destination", "address_provider"] {
        let mut raw = paris();
        raw.parameters.push(ProposedParameter {
            id: id.into(),
            value: Index(0),
        });
        assert!(validate_recommendation(&raw, &base).is_err(), "{id}");
    }
    assert_eq!(base, before);
    assert!(!resolved.enabled);
    for source in [1, 2, 3] {
        let mut raw = paris();
        raw.parameters[0].value = Index(source);
        assert!(validate_recommendation(&raw, &base).is_err());
    }
}

#[test]
fn saved_choices_cannot_silently_change() {
    let base = AnimationSettings::default();
    let raw = proposal(AnimationKind::Spectrum, vec![("style", Index(0))]);
    let mut rec = validate_recommendation(&raw, &base).unwrap();
    assert!(resolve_recommendation(&rec, &base).is_ok());
    let mut bad = raw;
    bad.parameters[0].value = Index(99);
    assert!(validate_recommendation(&bad, &base).is_err());
    let choice = rec
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == "style")
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
        proposal(AnimationKind::Images, vec![("recursive", Bool(true))]),
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
    let cases = [proposal(
        AnimationKind::Spectrum,
        vec![("ceiling_db", Number(-20)), ("floor_db", Number(-30))],
    )];
    for raw in cases {
        let forward = validate_recommendation(&raw, &base).unwrap();
        let mut reverse = raw;
        reverse.parameters.reverse();
        assert_eq!(validate_recommendation(&reverse, &base).unwrap(), forward);
        assert!(!resolve_recommendation(&forward, &base).unwrap().enabled);
    }
    for raw in [proposal(
        AnimationKind::Spectrum,
        vec![("ceiling_db", Number(-25)), ("floor_db", Number(-30))],
    )] {
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
fn conditional_controls_are_exposed_and_capture_inputs_are_not() {
    let spectrum = inventory(AnimationKind::Spectrum).unwrap();
    for id in ["bar_width", "bar_gap", "spectrogram_speed"] {
        assert!(spectrum.contains_key(id), "{id}");
    }
    assert!(!spectrum.contains_key("input"));
    assert!(!spectrum.contains_key("device_name"));
}
