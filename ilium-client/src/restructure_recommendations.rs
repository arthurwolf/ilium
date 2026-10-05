//! Required recommendation pointers and immutable inference context for restructuring.
use super::{LeafContext, ProtectedSplitViewContext, RestructureCompletionClient};
use crate::background_animation::AnimationSettings;
use crate::semantic_animation::{validate_recommendations, ProposedRecommendations};
use ilium_core::animation_recommendation::{
    AnimationRecommendation, PlanAnimationEntry, RecommendedRestructurePlan,
    MAXIMUM_RECOMMENDATION_DEPTH, MAXIMUM_RECOMMENDATION_NODES,
};
use ilium_core::{NodeId, Tree};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
#[derive(Debug, Clone, Default)]
pub struct RecommendationSnapshot {
    pub expected_animation_generation: u64,
    pub fixed_groups: Vec<(NodeId, String)>,
}
impl RecommendationSnapshot {
    pub fn capture(tree: &Tree, project_id: NodeId) -> anyhow::Result<Self> {
        let expected_animation_generation = tree.project_animation_generation(project_id)?;
        let mut fixed_groups = tree
            .all_ids()
            .filter_map(|id| {
                let node = tree.get(id)?;
                (node.is_group()
                    && node.is_name_fixed
                    && tree.project_ancestor(id) == Some(project_id))
                .then(|| (id, node.name.clone()))
            })
            .collect::<Vec<_>>();
        fixed_groups.sort_by_key(|(id, _)| *id);
        Ok(Self {
            expected_animation_generation,
            fixed_groups,
        })
    }
    pub fn input_fingerprint(&self, contexts: &[LeafContext], structure: &str) -> u64 {
        let mut fingerprint = super::project_restructure_input_fingerprint(contexts, structure);
        super::hash_restructure_value(
            &mut fingerprint,
            &self.expected_animation_generation.to_string(),
        );
        for (id, title) in &self.fixed_groups {
            super::hash_restructure_value(&mut fingerprint, &id.0.to_string());
            super::hash_restructure_value(&mut fingerprint, title);
        }
        fingerprint
    }
    pub(super) fn prompt_groups(&self) -> String {
        self.fixed_groups
            .iter()
            .map(|(id, title)| {
                format!(
                    "<fixed-group id=\"{}\" current-title={} />",
                    id.0,
                    crate::naming::encode_untrusted_context(title)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
#[derive(Debug, Clone, Default)]
pub struct RecommendationContext {
    pub snapshot: RecommendationSnapshot,
    pub authored: AnimationSettings,
}
pub fn infer_project_restructure<G: RestructureCompletionClient>(
    generator: &G,
    contexts: &[LeafContext],
    structure: &str,
    splits: &[ProtectedSplitViewContext],
    snapshot: &RecommendationSnapshot,
    animation_home: &Path,
) -> anyhow::Result<RecommendedRestructurePlan> {
    let authored = crate::project_config::load(animation_home)?.animation;
    let recommendation_context = RecommendationContext {
        snapshot: snapshot.clone(),
        authored,
    };
    super::infer_restructure_plan_with_protected_splits(
        generator,
        contexts,
        structure,
        splits,
        &recommendation_context,
    )
}
fn collect_pointers(
    nodes: &[Value],
    prefix: &[u32],
    output: &mut Vec<(Vec<u32>, String)>,
    fixed_groups: &mut HashSet<NodeId>,
) -> anyhow::Result<()> {
    for (index, node) in nodes.iter().enumerate() {
        anyhow::ensure!(
            output.len() < MAXIMUM_RECOMMENDATION_NODES
                && prefix.len() < MAXIMUM_RECOMMENDATION_DEPTH,
            "Recommendation tree exceeds its node or depth bound"
        );
        let object = node
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Every output node must be an object"))?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Every output node requires kind"))?;
        let allowed: &[&str] = match kind {
            "pane" => &[
                "kind",
                "id",
                "title",
                "short_title",
                "icon",
                "command_hint",
                "animation",
            ],
            "folder" => &["kind", "id", "title", "short_title", "icon", "animation"],
            "group" => &[
                "kind",
                "title",
                "short_title",
                "icon",
                "children",
                "animation",
            ],
            "existing_group" | "split_view" => &["kind", "id", "children", "animation"],
            _ => anyhow::bail!("Unknown restructure node kind"),
        };
        anyhow::ensure!(
            object.keys().all(|key| allowed.contains(&key.as_str())),
            "Unknown or immutable field in {kind} output"
        );
        let pointer = object
            .get("animation")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Every output node requires an animation pointer"))?;
        let mut path = prefix.to_vec();
        path.push(u32::try_from(index)?);
        output.push((path.clone(), pointer.to_owned()));
        if kind == "existing_group" {
            let id: NodeId = serde_json::from_value(
                object
                    .get("id")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Existing group requires id"))?,
            )?;
            anyhow::ensure!(fixed_groups.insert(id), "Duplicate fixed-group reference");
        }
        if matches!(kind, "group" | "existing_group" | "split_view") {
            match object.get("children") {
                Some(Value::Array(children)) => {
                    collect_pointers(children, &path, output, fixed_groups)?
                }
                None => {}
                _ => anyhow::bail!("Container children must be an array"),
            }
        }
    }
    Ok(())
}
pub(super) fn parse_pointers(
    candidate: &Value,
    table: &ProposedRecommendations,
    context: &RecommendationContext,
) -> anyhow::Result<(AnimationRecommendation, Vec<PlanAnimationEntry>)> {
    let nodes = candidate
        .get("children")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Restructure requires a children array"))?;
    let mut pointers = Vec::new();
    let mut actual_groups = HashSet::new();
    collect_pointers(nodes, &[], &mut pointers, &mut actual_groups)?;
    let expected_groups: HashSet<NodeId> = context
        .snapshot
        .fixed_groups
        .iter()
        .map(|(id, _)| *id)
        .collect();
    anyhow::ensure!(
        expected_groups.len() == context.snapshot.fixed_groups.len(),
        "Duplicate captured fixed-group identity"
    );
    anyhow::ensure!(
        actual_groups == expected_groups,
        "Restructure changed the required fixed-group set"
    );
    let keys = pointers
        .iter()
        .map(|(_, key)| key.clone())
        .collect::<Vec<_>>();
    let validated =
        validate_recommendations(table, &keys, &context.authored).map_err(anyhow::Error::msg)?;
    let entries = pointers
        .into_iter()
        .zip(validated.entries)
        .map(|((path, _), recommendation)| PlanAnimationEntry {
            path,
            recommendation,
        })
        .collect();
    Ok((validated.project, entries))
}
