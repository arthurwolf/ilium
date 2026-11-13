//! Semantic recommendation admission and concrete settings resolution.
//!
//! Ported from `ilium-client/src/semantic_animation.rs` and the recommendation data of
//! `ilium-core/src/animation_recommendation.rs`. Admission is pure: the scene registry and the
//! authored settings are supplied through the [`SemanticScenes`] trait, so this module never
//! constructs a scene, opens a file or makes a request. The kernel keeps the registry
//! (`AnimationSettings`, `scene_control`, `set_scene_control`); the plugin owns the rules that
//! decide which proposals are admitted.

use crate::control::{Control, ControlKind, ControlValue};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const ANIMATION_RECOMMENDATION_VERSION: u16 = 1;
pub const MAX_ANIMATION_PARAMETERS: usize = 128;

/// Identifiers are protocol tokens, not display labels or resource addresses.
pub fn valid_animation_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePolicy {
    Catalog,
    Authored,
}

/// Externally tagged for binary transport compatibility.
/// Choice labels are captured by validation, never supplied by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationValue {
    Number(i32),
    Choice { index: u32, label: String },
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationParameter {
    pub id: String,
    pub value: AnimationValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationRecommendation {
    pub version: u16,
    pub kind: String,
    pub resources: ResourcePolicy,
    pub parameters: Vec<AnimationParameter>,
}

impl AnimationRecommendation {
    /// Domain-envelope validation, not knowledge of a client's scene registry.
    /// Concrete kind, control and resource validation remains mandatory at use.
    pub fn validate_shape(&self) -> Result<(), String> {
        if self.version != ANIMATION_RECOMMENDATION_VERSION {
            return Err("Unsupported animation recommendation version".into());
        }
        if !valid_animation_identifier(&self.kind) || self.kind == "semantic" {
            return Err("Invalid concrete animation identifier".into());
        }
        if self.parameters.len() > MAX_ANIMATION_PARAMETERS {
            return Err("Too many animation parameters".into());
        }
        let mut seen = BTreeSet::new();
        for parameter in &self.parameters {
            if !valid_animation_identifier(&parameter.id) || !seen.insert(parameter.id.as_str()) {
                return Err("Invalid or duplicate animation parameter identifier".into());
            }
            if let AnimationValue::Choice { label, .. } = &parameter.value {
                if label.is_empty() || label.len() > 512 || label.chars().any(char::is_control) {
                    return Err("Invalid animation choice identity".into());
                }
            }
        }
        Ok(())
    }
}

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

/// What the kernel's animation settings and scene registry supply to admission.
pub trait SemanticScenes {
    /// The authored settings value (cloned freely; admission never mutates the caller's copy).
    type Settings: Clone;

    /// Whether `id` is a canonical concrete animation identifier of the registry.
    fn is_known_kind(&self, id: &str) -> bool;
    /// Dependency selectors of a kind, in application order.
    fn axes(&self, kind: &str) -> Vec<String>;
    /// Parameters a kind's recommendation must carry.
    fn required(&self, kind: &str) -> Vec<String>;
    /// The authored settings re-aimed at `kind` under `resources` (catalog defaults or authored
    /// inputs); an error names the missing authored resource.
    fn prepare(
        &self,
        kind: &str,
        resources: ResourcePolicy,
        authored: &Self::Settings,
    ) -> Result<Self::Settings, String>;
    /// The active scene's control row `id`, or `None` for an unknown or inactive control.
    fn scene_control(&self, settings: &Self::Settings, id: &str) -> Option<Control>;
    /// Narrows a row to what a recommendation may write (a bound tighter than the setter's).
    fn narrow(&self, _settings: &Self::Settings, _row: &mut Control) {}
    /// Applies one edit through the scene's own setter.
    fn set_scene_control(
        &self,
        settings: &mut Self::Settings,
        id: &str,
        value: ControlValue,
    ) -> Result<(), String>;
    /// Whether the two states hold the same authored resource and provider-cadence inputs.
    fn same_protected_inputs(&self, before: &Self::Settings, after: &Self::Settings) -> bool;
    /// The settings after the scene's own normalization.
    fn normalized(&self, settings: &Self::Settings) -> Self::Settings;
    /// Whether a resource-selector parameter value is allowed under the policy.
    fn selector_permitted(
        &self,
        _kind: &str,
        _original: &Self::Settings,
        _resources: ResourcePolicy,
        _parameter: &ProposedParameter,
    ) -> bool {
        true
    }
    /// Checks that the final state honours the resource policy.
    fn verify_resources(
        &self,
        _next: &Self::Settings,
        _original: &Self::Settings,
        _resources: ResourcePolicy,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Rows that may never be written by a recommendation (paths, devices, renderers, cadence).
pub fn exposable(row: &Control) -> bool {
    !matches!(&row.kind, ControlKind::Text { .. })
        && !row.id.starts_with("pack_")
        && !row.id.contains("mount")
        && !matches!(
            row.id,
            "input"
                | "device_name"
                | "render_backend"
                | "backend"
                | "poll"
                | "recursive"
                | "world_source"
                | "renderer"
                | "compute_backend"
                | "gpu"
                | "refresh_minutes"
                | "refresh_hours"
                | "saved_maps_folder"
        )
}

/// [`exposable`] plus the per-scene exclusions (named OSM destinations, voxel resource rows).
pub fn adaptable(kind: &str, row: &Control) -> bool {
    if !exposable(row) {
        return false;
    }
    if kind == crate::rows::OPENSTREETMAP_ID
        && matches!(
            row.id,
            "selection" | "place_list" | "destination" | "address_provider"
        )
    {
        return false;
    }
    kind != crate::values::VOXEL_LANDSCAPE_ID
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

fn kind_for<S: SemanticScenes>(scenes: &S, id: &str) -> Result<String, String> {
    if !valid_animation_identifier(id) || id == "semantic" {
        return Err("Choose a canonical concrete animation identifier".into());
    }
    if !scenes.is_known_kind(id) {
        return Err("Unknown concrete animation identifier".into());
    }
    Ok(id.to_owned())
}

fn control<S: SemanticScenes>(
    scenes: &S,
    kind: &str,
    settings: &S::Settings,
    id: &str,
) -> Result<Control, String> {
    let mut row = scenes
        .scene_control(settings, id)
        .ok_or_else(|| format!("Unknown or inactive animation parameter: {id}"))?;
    if !adaptable(kind, &row) {
        return Err(format!(
            "Animation parameter is not semantically writable: {id}"
        ));
    }
    scenes.narrow(settings, &mut row);
    Ok(row)
}

/// Checks a value against the row it targets: type, bounds, disabled choices, parity.
pub fn validate_value(row: &Control, value: &ControlValue) -> Result<(), String> {
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

fn write<S: SemanticScenes>(
    scenes: &S,
    kind: &str,
    settings: &mut S::Settings,
    id: &str,
    value: &ControlValue,
) -> Result<(), String> {
    validate_value(&control(scenes, kind, settings, id)?, value)?;
    let before = settings.clone();
    scenes.set_scene_control(settings, id, value.clone())?;
    if !scenes.same_protected_inputs(&before, settings) {
        return Err(format!(
            "Parameter {id} would change authored resource or provider cadence settings"
        ));
    }
    let after = control(scenes, kind, settings, id)?;
    if &after.value != value {
        return Err(format!(
            "Parameter {id} was normalized instead of accepted exactly"
        ));
    }
    Ok(())
}

fn checked<S: SemanticScenes>(
    scenes: &S,
    proposal: &ProposedRecommendation,
    authored: &S::Settings,
) -> Result<(AnimationRecommendation, S::Settings), String> {
    let kind = kind_for(scenes, &proposal.kind)?;
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
    for id in scenes.required(&kind) {
        if !ids.contains(id.as_str()) {
            return Err(format!("Recommendation requires parameter {id}"));
        }
    }

    let original = scenes.prepare(&kind, ResourcePolicy::Authored, authored)?;
    let mut next = scenes.prepare(&kind, proposal.resources, authored)?;
    let mut parameters = proposal.parameters.clone();
    let drivers = scenes.axes(&kind);
    parameters.sort_by(|left, right| {
        let rank = |id: &str| {
            drivers
                .iter()
                .position(|driver| driver == id)
                .unwrap_or(drivers.len())
        };
        rank(&left.id)
            .cmp(&rank(&right.id))
            .then_with(|| left.id.cmp(&right.id))
    });

    for parameter in &parameters {
        if !scenes.selector_permitted(&kind, &original, proposal.resources, parameter) {
            return Err("Resource selectors require the declared resource policy".into());
        }
        write(scenes, &kind, &mut next, &parameter.id, &parameter.value)?;
    }
    scenes.verify_resources(&next, &original, proposal.resources)?;
    if !scenes.same_protected_inputs(&original, &next)
        || !scenes.same_protected_inputs(&next, &scenes.normalized(&next))
    {
        return Err(
            "Semantic rendering would change authored resource or provider-cadence settings".into(),
        );
    }

    // Recheck the final state: a later setter may normalize an earlier field.
    let parameters = parameters
        .into_iter()
        .map(|parameter| {
            let row = control(scenes, &kind, &next, &parameter.id)?;
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
        kind,
        resources: proposal.resources,
        parameters,
    };
    recommendation.validate_shape()?;
    Ok((recommendation, next))
}

pub fn validate_recommendation<S: SemanticScenes>(
    scenes: &S,
    proposal: &ProposedRecommendation,
    authored: &S::Settings,
) -> Result<AnimationRecommendation, String> {
    checked(scenes, proposal, authored).map(|(recommendation, _)| recommendation)
}

/// Revalidation is mandatory after attach or authored-settings changes. The caller decides whether
/// Semantic is selected and which node owns the recommendation; this never turns the background
/// switch on.
pub fn resolve_recommendation<S: SemanticScenes>(
    scenes: &S,
    recommendation: &AnimationRecommendation,
    authored: &S::Settings,
) -> Result<S::Settings, String> {
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
    let (verified, settings) = checked(scenes, &proposal, authored)?;
    if &verified != recommendation {
        return Err(
            "Saved recommendation no longer has the same canonical choice identities".into(),
        );
    }
    Ok(settings)
}

/// The caller supplies one required pointer for EVERY output node in preorder. No
/// enabled/disabled setting is consulted here.
pub fn validate_recommendations<S: SemanticScenes>(
    scenes: &S,
    proposed: &ProposedRecommendations,
    entry_pointers: &[String],
    authored: &S::Settings,
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
            validate_recommendation(scenes, &definition.recommendation, authored)?,
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
