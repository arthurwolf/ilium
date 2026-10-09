//! Semantic recommendation admission and concrete settings resolution.
//!
//! Catalog construction belongs on the existing inference worker. Resolution
//! belongs at a settings/selection boundary, not once per row or rendered cell.
//! Nothing here constructs a Scene, starts a host, opens a file or makes a request.
use crate::background_animation::{AnimationKind, AnimationSettings};
use ilium_ambient::{Control, ControlKind, ControlValue};
use ilium_core::animation_recommendation::{
    valid_animation_identifier, AnimationParameter, AnimationRecommendation, AnimationValue,
    ResourcePolicy, ANIMATION_RECOMMENDATION_VERSION, MAX_ANIMATION_PARAMETERS,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedParameter {
    pub id: String,
    pub value: ControlValue,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedRecommendation {
    pub kind: String,
    pub resources: ResourcePolicy,
    pub parameters: Vec<ProposedParameter>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedRecommendation {
    pub key: String,
    pub recommendation: ProposedRecommendation,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedRecommendations {
    pub definitions: Vec<NamedRecommendation>,
    pub project: String,
}

#[derive(Debug, Clone)]
pub struct ValidatedRecommendations {
    pub project: AnimationRecommendation,
    /// Same order as the caller's complete output-node pointer list.
    pub entries: Vec<AnimationRecommendation>,
}

fn canonical_id(kind: AnimationKind) -> Result<String, String> {
    match serde_json::to_value(kind).map_err(|error| error.to_string())? {
        serde_json::Value::String(id) => Ok(id),
        _ => Err("AnimationKind must serialize as a string identifier".into()),
    }
}

fn kind_for(id: &str) -> Result<AnimationKind, String> {
    if !valid_animation_identifier(id) || id == "semantic" {
        return Err("Choose a canonical concrete animation identifier".into());
    }
    let kind: AnimationKind = serde_json::from_value(serde_json::Value::String(id.to_owned()))
        .map_err(|_| "Unknown concrete animation identifier".to_owned())?;
    if !AnimationKind::ALL.contains(&kind) || canonical_id(kind)? != id {
        return Err("Animation aliases are not canonical recommendation identifiers".into());
    }
    Ok(kind)
}

/// Dependency selectors, in application order. Values, labels and bounds still
/// come exclusively from scene_controls(). Numeric axes are sampled at bounds.
fn axes(kind: AnimationKind) -> &'static [&'static str] {
    match kind {
        AnimationKind::Shoreline => &["shoreline_style"],
        AnimationKind::Wikipedia => &["wiki_render_mode"],
        AnimationKind::Stars => &["projection", "star_style", "start_from"],
        AnimationKind::TopographicMaps => &["tide_range_m"],
        AnimationKind::Graph => &["source", "mode"],
        AnimationKind::Images => &["mode", "source_kind", "order", "motion", "display_seconds"],
        AnimationKind::Video => &["mode", "shuffle"],
        AnimationKind::Spectrum => &["style", "floor_db"],
        AnimationKind::Clouds => &["coverage", "projection", "history_hours", "land_underlay"],
        AnimationKind::NightLights => &["projection", "terminator", "coastline"],
        AnimationKind::Carpet => &[
            "carpet_mode",
            "carpet_snake_grid",
            "carpet_snake_step_ms",
            "carpet_life_generation_ms",
        ],
        _ => &[],
    }
}

fn exposable(row: &Control) -> bool {
    !matches!(&row.kind, ControlKind::Text { .. })
        // Profiles can retain custom paths; switching a profile selects an
        // authored resource even when its path is not a writable parameter.
        && !row.id.starts_with("pack_")
        && !row.id.contains("mount")
        && !matches!(
            row.id,
            "input" | "device_name" | "render_backend" | "backend" | "poll" | "recursive"
                | "world_source" | "renderer" | "compute_backend" | "gpu"
                | "refresh_minutes" | "refresh_hours"
                | "saved_maps_folder"
        )
}

fn adaptable(kind: AnimationKind, row: &Control) -> bool {
    if !exposable(row) {
        return false;
    }
    // Named destinations and lookup providers remain explicit user choices.
    // Their conditional indices are not a stable semantic catalogue schema.
    if kind == AnimationKind::OpenStreetMap
        && matches!(
            row.id,
            "selection" | "place_list" | "destination" | "address_provider"
        )
    {
        return false;
    }
    kind != AnimationKind::VoxelLandscape
        || matches!(
            row.id,
            "zoom"
                | "detail"
                | "pan_speed"
                | "pan_direction"
                | "color_mode"
                | "palette"
                | "hue"
                | "saturation"
                | "lightness"
                | "vegetation"
                | "structures"
                | "rivers"
                | "ravines"
                | "caves"
        )
}

/// Check indirect setter/normalization effects as well as writable controls.
fn same_protected_inputs(before: &AnimationSettings, after: &AnimationSettings) -> bool {
    match after.kind {
        AnimationKind::VoxelLandscape => {
            // Compare every authored field except the explicitly adaptable
            // appearance controls. Newly added resource fields are protected
            // automatically, without depending on another scene's private API.
            let protected = |settings: &ilium_ambient::VoxelLandscapeSettings| {
                let mut value = serde_json::to_value(settings).ok()?;
                let fields = value.as_object_mut()?;
                for name in [
                    "zoom_percent",
                    "detail",
                    "pan_speed_percent",
                    "pan_direction",
                    "color_mode",
                    "palette",
                    "hue_degrees",
                    "saturation_percent",
                    "lightness_percent",
                    "vegetation_percent",
                    "structures_percent",
                    "rivers",
                    "ravines",
                    "caves",
                ] {
                    fields.remove(name);
                }
                Some(value)
            };
            match (
                protected(&before.ambient.voxel_landscape),
                protected(&after.ambient.voxel_landscape),
            ) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        AnimationKind::OpenStreetMap => {
            let left = &before.ambient.openstreetmap;
            let right = &after.ambient.openstreetmap;
            left.search == right.search
                && left.endpoint == right.endpoint
                && left.local_path == right.local_path
                && left.coordinates == right.coordinates
                && left.location_label == right.location_label
                && left.place_list == right.place_list
                && left.destination_id == right.destination_id
        }
        AnimationKind::Clouds => {
            before.ambient.clouds.refresh_minutes == after.ambient.clouds.refresh_minutes
        }
        AnimationKind::NightLights => {
            before.ambient.night_lights.refresh_hours == after.ambient.night_lights.refresh_hours
        }
        AnimationKind::Graph => {
            before.ambient.graph.poll_seconds == after.ambient.graph.poll_seconds
        }
        AnimationKind::Earthquakes => {
            before.ambient.earthquakes.poll_seconds == after.ambient.earthquakes.poll_seconds
        }
        AnimationKind::Aircraft => {
            before.ambient.aircraft.poll_seconds == after.ambient.aircraft.poll_seconds
        }
        AnimationKind::Boats => {
            before.ambient.boats.poll_seconds == after.ambient.boats.poll_seconds
        }
        AnimationKind::Spectrum => {
            before.ambient.spectrum.input == after.ambient.spectrum.input
                && before.ambient.spectrum.device_name == after.ambient.spectrum.device_name
        }
        _ => true,
    }
}

/// Additional semantic constraints not fully expressed by the UI's ranges.
/// These narrow the real control, rather than defining a second control table.
fn narrow(settings: &AnimationSettings, row: &mut Control) {
    if let ControlKind::Slider { min, max, .. } = &mut row.kind {
        match (settings.kind, row.id) {
            (AnimationKind::Stars, "look_azimuth") => {
                // The setter wraps 360 to 0; recommendations use canonical bearings.
                *max = (*max).min(359);
            }
            (AnimationKind::Spectrum, "ceiling_db") => {
                *min = (*min).max(settings.ambient.spectrum.floor_db.saturating_add(10));
            }
            (AnimationKind::Images, "transition_seconds") => {
                *max = (*max).min(i32::from(settings.ambient.images.display_seconds / 2));
            }
            _ => {}
        }
    }
    if settings.kind == AnimationKind::OpenStreetMap && row.id == "source" {
        row.disabled_options.extend([
            (1, "Semantic uses offline maps only".into()),
            (2, "Semantic uses offline maps only".into()),
        ]);
    }
}

fn control(settings: &AnimationSettings, id: &str) -> Result<Control, String> {
    let mut row = settings
        .scene_control(id)
        .ok_or_else(|| format!("Unknown or inactive animation parameter: {id}"))?;
    if !adaptable(settings.kind, &row) {
        return Err(format!(
            "Animation parameter is not semantically writable: {id}"
        ));
    }
    narrow(settings, &mut row);
    Ok(row)
}

fn validate_value(row: &Control, value: &ControlValue) -> Result<(), String> {
    let valid = match (&row.kind, value) {
        (ControlKind::Slider { min, max, .. }, ControlValue::Number(number)) => {
            min <= max && number >= min && number <= max
        }
        (ControlKind::Choice { options }, ControlValue::Index(index)) => {
            *index < options.len() && row.disabled_reason(*index).is_none()
        }
        (ControlKind::Toggle, ControlValue::Bool(_)) => true,
        _ => false,
    };
    if !valid {
        return Err(format!(
            "Wrong type, bounds or disabled choice for {}",
            row.id
        ));
    }
    if row.id == "carpet_snake_grid"
        && matches!(value, ControlValue::Number(number) if number % 2 != 0)
    {
        return Err("carpet_snake_grid must be even".into());
    }
    Ok(())
}

fn write(settings: &mut AnimationSettings, id: &str, value: &ControlValue) -> Result<(), String> {
    validate_value(&control(settings, id)?, value)?;
    let before = settings.clone();
    settings.set_scene_control(id, value.clone())?;
    if !same_protected_inputs(&before, settings) {
        return Err(format!(
            "Parameter {id} would change authored resource or provider cadence settings"
        ));
    }
    let after = control(settings, id)?;
    if &after.value != value {
        return Err(format!(
            "Parameter {id} was normalized instead of accepted exactly"
        ));
    }
    Ok(())
}

fn selected(settings: &AnimationSettings, id: &str) -> Option<usize> {
    match settings.scene_control(id)?.value {
        ControlValue::Index(index) => Some(index),
        _ => None,
    }
}

fn has_text(settings: &AnimationSettings, id: &str) -> bool {
    settings
        .scene_control(id)
        .is_some_and(|row| match row.value {
            ControlValue::Text(value) => {
                value.split(';').any(|part| !part.trim().is_empty())
                    && !value.chars().any(char::is_control)
            }
            _ => false,
        })
}

fn usable_image_source(settings: &AnimationSettings) -> bool {
    match (
        selected(settings, "mode"),
        selected(settings, "source_kind"),
    ) {
        (Some(0), Some(0)) => control(settings, "builtin_image")
            .is_ok_and(|row| validate_value(&row, &row.value).is_ok()),
        (Some(0), Some(1)) => has_text(settings, "file"),
        (Some(0), Some(2)) => has_text(settings, "url"),
        (Some(1), _) => has_text(settings, "folders"),
        (Some(2), _) => has_text(settings, "urls"),
        _ => false,
    }
}

fn verify_resources(
    settings: &AnimationSettings,
    authored: &AnimationSettings,
    resources: ResourcePolicy,
) -> Result<(), String> {
    match settings.kind {
        AnimationKind::VoxelLandscape => {
            if selected(settings, "world_source") == Some(1)
                && !has_text(settings, "saved_maps_folder")
            {
                return Err("Saved-world Voxel requires an existing authored maps folder".into());
            }
        }
        AnimationKind::Images => {
            if resources == ResourcePolicy::Catalog {
                if selected(settings, "mode") != Some(0)
                    || selected(settings, "source_kind") != Some(0)
                {
                    return Err("Catalog Images requires single-image built-in mode".into());
                }
            } else if settings.ambient.images.mode != authored.ambient.images.mode
                || settings.ambient.images.source != authored.ambient.images.source
            {
                return Err("Authored Images must retain its existing resource selection".into());
            }
            if !usable_image_source(settings) {
                return Err("The selected authored image input is missing or malformed".into());
            }
        }
        AnimationKind::Video => {
            if resources != ResourcePolicy::Authored || !has_text(settings, "source") {
                return Err("Video requires an existing authored source".into());
            }
            if settings.ambient.video.source.trim() != authored.ambient.video.source.trim() {
                return Err("Semantic Video cannot replace its authored source".into());
            }
        }
        AnimationKind::Stars if selected(settings, "start_from") == Some(1) => {
            if resources != ResourcePolicy::Authored
                || settings.ambient.stars.fixed_start_unix().is_none()
            {
                return Err("Fixed Stars time requires a valid existing authored timestamp".into());
            }
        }
        AnimationKind::Spectrum
            if settings.scene_control("device_name").is_some()
                && !has_text(settings, "device_name") =>
        {
            return Err("The authored named audio device is empty".into());
        }
        _ => {}
    }
    Ok(())
}

fn prepare(
    kind: AnimationKind,
    resources: ResourcePolicy,
    authored: &AnimationSettings,
) -> Result<AnimationSettings, String> {
    if resources == ResourcePolicy::Authored
        && !matches!(
            kind,
            AnimationKind::Images | AnimationKind::Video | AnimationKind::Stars
        )
    {
        return Err("Authored resource policy applies only to Images, Video or Stars".into());
    }
    let mut settings = authored.clone();
    settings.kind = kind;
    match kind {
        AnimationKind::OpenStreetMap => {
            write(&mut settings, "source", &ControlValue::Index(0))?;
            // Semantic recommendations retain the original offline place contract.
            // Initialize the mode without exposing authored lists to recommendations.
            settings.set_scene_control("selection", ControlValue::Index(0))?;
        }
        AnimationKind::Images if resources == ResourcePolicy::Catalog => {
            write(&mut settings, "mode", &ControlValue::Index(0))?;
            write(&mut settings, "source_kind", &ControlValue::Index(0))?;
            write(&mut settings, "builtin_image", &ControlValue::Index(0))?;
        }
        AnimationKind::Stars if resources == ResourcePolicy::Catalog => {
            write(&mut settings, "start_from", &ControlValue::Index(0))?;
        }
        _ => {}
    }
    Ok(settings)
}

fn checked(
    proposal: &ProposedRecommendation,
    authored: &AnimationSettings,
) -> Result<(AnimationRecommendation, AnimationSettings), String> {
    let kind = kind_for(&proposal.kind)?;
    if proposal.parameters.len() > MAX_ANIMATION_PARAMETERS {
        return Err("Too many animation parameters".into());
    }
    let mut ids = BTreeSet::new();
    for parameter in &proposal.parameters {
        if !valid_animation_identifier(&parameter.id) || !ids.insert(parameter.id.as_str()) {
            return Err("Invalid or duplicate animation parameter identifier".into());
        }
        if matches!(&parameter.value, ControlValue::Text(_)) {
            return Err("Text parameters are not permitted in recommendations".into());
        }
    }
    let required: &[&str] = match kind {
        AnimationKind::OpenStreetMap => &["source", "place", "tour"],
        AnimationKind::Carpet => &["carpet_mode"],
        _ => &[],
    };
    for id in required {
        if !ids.contains(id) {
            return Err(format!("Recommendation requires parameter {id}"));
        }
    }

    let mut original = authored.clone();
    original.kind = kind;
    let mut next = prepare(kind, proposal.resources, authored)?;
    let mut parameters = proposal.parameters.clone();
    let drivers = axes(kind);
    parameters.sort_by(|left, right| {
        let rank = |id: &str| {
            drivers
                .iter()
                .position(|driver| *driver == id)
                .unwrap_or(drivers.len())
        };
        rank(&left.id)
            .cmp(&rank(&right.id))
            .then_with(|| left.id.cmp(&right.id))
    });

    for parameter in &parameters {
        if kind == AnimationKind::Images && matches!(parameter.id.as_str(), "mode" | "source_kind")
        {
            let permitted = match proposal.resources {
                ResourcePolicy::Catalog => parameter.value == ControlValue::Index(0),
                ResourcePolicy::Authored => original
                    .scene_control(&parameter.id)
                    .is_some_and(|row| row.value == parameter.value),
            };
            if !permitted {
                return Err("Image resource selectors require the declared resource policy".into());
            }
        }
        if kind == AnimationKind::Stars
            && proposal.resources == ResourcePolicy::Catalog
            && parameter.id == "start_from"
            && parameter.value != ControlValue::Index(0)
        {
            return Err("Catalog Stars starts from Now, not an invented timestamp".into());
        }
        write(&mut next, &parameter.id, &parameter.value)?;
    }
    verify_resources(&next, &original, proposal.resources)?;
    if !same_protected_inputs(&original, &next) || !same_protected_inputs(&next, &next.normalized())
    {
        return Err(
            "Semantic rendering would change authored resource or provider-cadence settings".into(),
        );
    }

    // Recheck the final state: a later setter may normalize an earlier field.
    let parameters = parameters
        .into_iter()
        .map(|parameter| {
            let row = control(&next, &parameter.id)?;
            validate_value(&row, &parameter.value)?;
            if row.value != parameter.value {
                return Err(format!("Final settings changed parameter {}", parameter.id));
            }
            let value = match parameter.value {
                ControlValue::Number(number) => AnimationValue::Number(number),
                ControlValue::Bool(on) => AnimationValue::Bool(on),
                ControlValue::Index(index) => {
                    let ControlKind::Choice { options } = &row.kind else {
                        return Err("Choice parameter lost its choice schema".into());
                    };
                    AnimationValue::Choice {
                        index: u32::try_from(index)
                            .map_err(|_| "Choice index exceeds transport range")?,
                        label: options.get(index).ok_or("Choice disappeared")?.to_string(),
                    }
                }
                ControlValue::Text(_) => return Err("Text parameters are forbidden".into()),
            };
            Ok(AnimationParameter {
                id: parameter.id,
                value,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let recommendation = AnimationRecommendation {
        version: ANIMATION_RECOMMENDATION_VERSION,
        kind: canonical_id(kind)?,
        resources: proposal.resources,
        parameters,
    };
    recommendation.validate_shape()?;
    Ok((recommendation, next))
}

pub fn validate_recommendation(
    proposal: &ProposedRecommendation,
    authored: &AnimationSettings,
) -> Result<AnimationRecommendation, String> {
    checked(proposal, authored).map(|(recommendation, _)| recommendation)
}

/// Revalidation is mandatory after attach or authored-settings changes.
/// The caller decides whether Semantic is selected and which node owns the
/// recommendation. This function never turns on the background switch.
pub fn resolve_recommendation(
    recommendation: &AnimationRecommendation,
    authored: &AnimationSettings,
) -> Result<AnimationSettings, String> {
    recommendation.validate_shape()?;
    let parameters = recommendation
        .parameters
        .iter()
        .map(|parameter| {
            let value = match &parameter.value {
                AnimationValue::Number(number) => ControlValue::Number(*number),
                AnimationValue::Bool(on) => ControlValue::Bool(*on),
                AnimationValue::Choice { index, .. } => ControlValue::Index(
                    usize::try_from(*index).map_err(|_| "Choice index is not supported here")?,
                ),
            };
            Ok(ProposedParameter {
                id: parameter.id.clone(),
                value,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let proposal = ProposedRecommendation {
        kind: recommendation.kind.clone(),
        resources: recommendation.resources,
        parameters,
    };
    let (verified, settings) = checked(&proposal, authored)?;
    if &verified != recommendation {
        return Err(
            "Saved recommendation no longer has the same canonical choice identities".into(),
        );
    }
    Ok(settings)
}

/// The caller supplies one required pointer for EVERY output node in preorder.
/// No enabled/disabled setting is consulted here.
pub fn validate_recommendations(
    proposed: &ProposedRecommendations,
    entry_pointers: &[String],
    authored: &AnimationSettings,
) -> Result<ValidatedRecommendations, String> {
    if proposed.definitions.is_empty()
        || proposed.definitions.len() > entry_pointers.len().saturating_add(1)
    {
        return Err("Recommendation definitions must be nonempty and all referenced".into());
    }
    let mut definitions = BTreeMap::new();
    for definition in &proposed.definitions {
        if !valid_animation_identifier(&definition.key) || definitions.contains_key(&definition.key)
        {
            return Err("Invalid or duplicate recommendation key".into());
        }
        definitions.insert(
            definition.key.clone(),
            validate_recommendation(&definition.recommendation, authored)?,
        );
    }
    let mut used = BTreeSet::new();
    let mut lookup = |key: &str| -> Result<AnimationRecommendation, String> {
        if !valid_animation_identifier(key) {
            return Err("A project or entry recommendation pointer is missing or invalid".into());
        }
        let recommendation = definitions
            .get(key)
            .ok_or_else(|| format!("Unknown recommendation pointer: {key}"))?;
        used.insert(key.to_owned());
        Ok(recommendation.clone())
    };
    let project = lookup(&proposed.project)?;
    let entries = entry_pointers
        .iter()
        .map(|key| lookup(key))
        .collect::<Result<Vec<_>, _>>()?;
    if used.len() != definitions.len() {
        return Err("Unused recommendation definitions are not permitted".into());
    }
    Ok(ValidatedRecommendations { project, entries })
}

/// Resource capability metadata contains no paths, URLs, device names or dates.
pub fn authored_capabilities(authored: &AnimationSettings) -> serde_json::Value {
    let mut images = authored.clone();
    images.kind = AnimationKind::Images;
    let images_usable = usable_image_source(&images);
    let mut video = authored.clone();
    video.kind = AnimationKind::Video;
    let mut stars = authored.clone();
    stars.kind = AnimationKind::Stars;
    let fixed_star_time = write(&mut stars, "start_from", &ControlValue::Index(1)).is_ok()
        && stars.ambient.stars.fixed_start_unix().is_some();
    serde_json::json!({
        "authored_images_mode": images_usable.then(|| selected(&images, "mode")).flatten(),
        "authored_image_source_kind":
            images_usable.then(|| selected(&images, "source_kind")).flatten(),
        "authored_video": has_text(&video, "source"),
        "authored_fixed_star_time": fixed_star_time
    })
}

fn probe_values(row: &Control) -> Vec<ControlValue> {
    match &row.kind {
        ControlKind::Choice { options } => (0..options.len())
            .filter(|index| row.disabled_reason(*index).is_none())
            .map(ControlValue::Index)
            .collect(),
        ControlKind::Toggle => vec![ControlValue::Bool(false), ControlValue::Bool(true)],
        ControlKind::Slider { min, max, .. } => {
            let mut values = vec![ControlValue::Number(*min)];
            for value in [ControlValue::Number(*max), row.value.clone()] {
                if !values.contains(&value) {
                    values.push(value);
                }
            }
            values
        }
        ControlKind::Text { .. } => Vec::new(),
    }
}

fn probe_states(kind: AnimationKind) -> Result<Vec<AnimationSettings>, String> {
    let base = AnimationSettings {
        kind,
        ..Default::default()
    };
    let mut states = vec![base];
    for id in axes(kind) {
        let mut expanded = Vec::new();
        for state in states {
            if state.scene_control(id).is_none() {
                expanded.push(state);
                continue;
            }
            let row = control(&state, id)?;
            for value in probe_values(&row) {
                let mut next = state.clone();
                write(&mut next, id, &value)?;
                expanded.push(next);
                if expanded.len() > 4096 {
                    return Err("Animation metadata dependency expansion exceeded its bound".into());
                }
            }
        }
        states = expanded;
    }
    Ok(states)
}

fn observe(
    inventory: &mut BTreeMap<&'static str, Control>,
    state: &AnimationSettings,
) -> Result<(), String> {
    for row in state
        .scene_controls()
        .into_iter()
        .filter(|row| adaptable(state.kind, row))
    {
        let row = control(state, row.id)?;
        if let Some(previous) = inventory.get_mut(row.id) {
            match (&mut previous.kind, &row.kind) {
                (
                    ControlKind::Slider { min: a, max: b, .. },
                    ControlKind::Slider { min: c, max: d, .. },
                ) => {
                    *a = (*a).min(*c);
                    *b = (*b).max(*d);
                }
                (ControlKind::Choice { options: a }, ControlKind::Choice { options: b })
                    if a.as_slice() == b.as_slice() => {}
                (ControlKind::Toggle, ControlKind::Toggle) => {}
                _ => {
                    return Err(format!(
                        "Incompatible conditional control schemas: {}",
                        row.id
                    ));
                }
            }
            // Catalog choices are the union of available choices across modes.
            // Admission still checks disabled choices against the final source.
            previous
                .disabled_options
                .retain(|(index, _)| row.disabled_reason(*index).is_some());
        } else {
            inventory.insert(row.id, row);
        }
    }
    Ok(())
}

fn inventory(kind: AnimationKind) -> Result<BTreeMap<&'static str, Control>, String> {
    let mut result = BTreeMap::new();
    let mut probes = 0usize;
    for state in probe_states(kind)? {
        observe(&mut result, &state)?;
        // Also inspect all other immediate choice/toggle branches. The explicit
        // axes above supply the interacting dependencies verified in the packet.
        for row in state
            .scene_controls()
            .into_iter()
            .filter(|row| adaptable(state.kind, row))
        {
            if axes(kind).contains(&row.id)
                || !matches!(&row.kind, ControlKind::Choice { .. } | ControlKind::Toggle)
            {
                continue;
            }
            let row = control(&state, row.id)?;
            for value in probe_values(&row) {
                probes += 1;
                if probes > 16_384 {
                    return Err("Animation metadata probing exceeded its bound".into());
                }
                let mut next = state.clone();
                write(&mut next, row.id, &value)?;
                observe(&mut result, &next)?;
            }
        }
    }
    Ok(result)
}

fn notes(kind: AnimationKind) -> &'static str {
    match kind {
        AnimationKind::VoxelLandscape => {
            "World source, saved-map folder, seed, pack profiles, custom overrides, mounts, edition, addon and duplicate policy remain authored. Only listed visual controls adapt. Saved-world mode hides generated detail, direction, vegetation, structures and terrain toggles; controls must be active for the authored source."
        }
        AnimationKind::Clouds => {
            "Coverage precedes projection; rotation requires global globe projection. history_hours greater than zero exposes playback_fps and smoothing. land_underlay exposes underlay_brightness. refresh_minutes is authored provider cadence, not rendering FPS."
        }
        AnimationKind::NightLights => {
            "Projection precedes rotation; terminator and coastline expose their dependent visual controls. refresh_hours is authored provider cadence."
        }
        AnimationKind::OpenStreetMap => {
            "Require source, place and tour. source=0 only. Paris uses place=0,tour=0. Local/custom map inputs are never selected."
        }
        AnimationKind::Carpet => {
            "Require carpet_mode. Mode-specific controls: hunters=0; snake=1; life=2; AI chess=3; piece heights/chess_easing=3 or 4; dvd=5; orbit=6; UTC offset/seconds=7 or 8; 24h=7; tubes=8. Snake grid must be even; initial length <= min(32,grid*grid-1). Easing <= snake step in mode 1, <= min(3000,Life generation) in mode 2, <=900 in mode 7; otherwise <=3000."
        }
        AnimationKind::Graph => {
            "Apply source before mode. OHLC candles require that source's enabled candle choice; rising/falling hues apply only to candles. Poll settings remain authored."
        }
        AnimationKind::Wikipedia => {
            "wiki_zoom applies only to Braille. This is today's English Main Page feed, not a selectable topic."
        }
        AnimationKind::Shoreline => {
            "shoreline_* controls other than shoreline_style require Rich; the four scene_control_* sliders apply to both styles."
        }
        AnimationKind::TopographicMaps => {
            "tide_seconds requires tide_range_m > 0. The listed envelope is not permission to use inactive controls."
        }
        AnimationKind::Stars => {
            "Panorama hides lens; Patch exposes look_altitude. Realistic star_style exposes star_size/star_colors. Fixed start requires resources=authored and a usable saved timestamp; start_datetime is never writable."
        }
        AnimationKind::Images => {
            "resources=catalog selects Single/Built-in, default picture 0. resources=authored keeps the current authored mode and source. order/transition require slideshow; shuffle_seed also requires Shuffle. display_seconds requires slideshow or motion. motion_strength/easing require motion. transition_seconds <= display_seconds/2."
        }
        AnimationKind::Video => {
            "Requires resources=authored and existing video input. Slowed exposes slowed_percent; Random scenes exposes scene_seconds and hides shuffle/repeat_one. seed requires Random scenes or shuffle."
        }
        AnimationKind::Spectrum => {
            "Input/device are unchanged. Waveform hides band/frequency/FFT/tilt/floor/ceiling controls. Radial/Pulse rings hide orientation. Bars expose bar_width/bar_gap. Spectrogram exposes spectrogram_speed. Peak markers apply to Bars/Mirrored bars/Line/Area/Radial; mirror excludes Waveform/Pulse rings. ceiling_db >= floor_db+10."
        }
        AnimationKind::Boats => {
            "The supplied description is older than the boat_source control: that control offers broader OpenSeaFeed or Finnish-water Digitraffic. Selection and coverage follow the actual control, not the older description."
        }
        _ => "",
    }
}

/// Compact complete catalog, not a clipped summary. The restructure prompt
/// builder must retain this intact and keep its existing total prompt limit.
fn build_catalog() -> Result<String, String> {
    let mut text = String::from(
        "Catalog notation: N=Number, I=Index, B=Bool; N[min,max;step]unit. \
         Values: Number(integer), Index(zero-based listed choice), Bool(true/false). \
         Numeric ranges are inclusive envelopes; dependency rules and final active \
         controls apply. step is a keyboard increment, not universal divisibility. \
         Text, capture input/device, renderer backend, polling and recursive \
         discovery are not writable. resources is catalog or authored; authored \
         is permitted only for Images, Video or Stars. Emit required project and \
         every-entry pointers even when animations are disabled.\n",
    );
    for kind in AnimationKind::ALL {
        let id = canonical_id(kind)?;
        if id == "semantic" {
            continue;
        }
        text.push_str(&format!("{id:?} | {}\n", kind.description()));
        for (id, row) in inventory(kind)? {
            if id.starts_with("scene_control_") {
                text.push_str(&format!("{id}({})=", row.label));
            } else {
                text.push_str(&format!("{id}="));
            }
            match &row.kind {
                ControlKind::Slider {
                    min,
                    max,
                    step,
                    unit,
                } => {
                    text.push_str(&format!("N[{min},{max};{step}]{unit}"));
                }
                ControlKind::Choice { options } => {
                    text.push_str("I[");
                    let choices = options
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| row.disabled_reason(*index).is_none())
                        .map(|(index, label)| format!("{index}:{label}"))
                        .collect::<Vec<_>>();
                    text.push_str(&choices.join("|"));
                    text.push(']');
                }
                ControlKind::Toggle => text.push('B'),
                ControlKind::Text { .. } => return Err("Text leaked into semantic catalog".into()),
            }
            text.push(';');
        }
        text.push('\n');
        if !notes(kind).is_empty() {
            text.push_str(notes(kind));
            text.push('\n');
        }
    }
    Ok(text)
}

pub fn catalog() -> Result<&'static str, String> {
    static CATALOG: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    CATALOG
        .get_or_init(build_catalog)
        .as_ref()
        .map(String::as_str)
        .map_err(Clone::clone)
}

#[cfg(test)]
#[path = "semantic_animation_tests.rs"]
mod tests;
