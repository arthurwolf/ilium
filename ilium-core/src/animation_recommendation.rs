//! Pure recommendation data. This module has no renderer, client or I/O dependency.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

/// Child ordinals from the proposed project's children; never fabricated NodeIds.
/// Coverage must equal every output node path, including groups and split views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanAnimationEntry {
    pub path: Vec<u32>,
    pub recommendation: AnimationRecommendation,
}

/// The generation is captured by the client, not returned by the language model.
/// Keeping structure separate preserves the existing structural enum contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecommendedRestructurePlan {
    pub structure: crate::RestructurePlan,
    pub expected_animation_generation: u64,
    pub project: AnimationRecommendation,
    pub entries: Vec<PlanAnimationEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recommendation() -> AnimationRecommendation {
        AnimationRecommendation {
            version: ANIMATION_RECOMMENDATION_VERSION,
            kind: "carpet".into(),
            resources: ResourcePolicy::Catalog,
            parameters: vec![AnimationParameter {
                id: "carpet_mode".into(),
                value: AnimationValue::Choice {
                    index: 1,
                    label: "Autonomous Snake".into(),
                },
            }],
        }
    }

    #[test]
    fn untrusted_envelope_rejects_versions_recursive_kinds_and_duplicate_parameters() {
        let valid = recommendation();
        assert!(valid.validate_shape().is_ok());
        let mut invalid = valid.clone();
        invalid.version += 1;
        assert!(invalid.validate_shape().is_err());
        for kind in ["semantic", "", "Carpet", "../carpet", "carpet\n"] {
            invalid = valid.clone();
            invalid.kind = kind.into();
            assert!(invalid.validate_shape().is_err(), "{kind:?}");
        }
        invalid = valid.clone();
        invalid.parameters.push(valid.parameters[0].clone());
        assert!(invalid.validate_shape().is_err());
    }

    #[test]
    fn saved_choice_identity_is_bounded_and_single_line() {
        for label in [String::new(), "x".repeat(513), "Snake\nInjected".into()] {
            let mut invalid = recommendation();
            invalid.parameters[0].value = AnimationValue::Choice { index: 1, label };
            assert!(invalid.validate_shape().is_err());
        }
        let mut maximum = recommendation();
        maximum.parameters[0].value = AnimationValue::Choice {
            index: 1,
            label: "x".repeat(512),
        };
        assert!(maximum.validate_shape().is_ok());
    }
}

use crate::{NodeActivityRevision, NodeId, RestructureNode, Tree, TreeError};
pub const MAXIMUM_RECOMMENDATION_NODES: usize = 4096;
pub const MAXIMUM_RECOMMENDATION_DEPTH: usize = 64;
pub const MAXIMUM_RECOMMENDATION_PARAMETERS: usize = 65_536;
fn recommendation_error(message: impl Into<String>) -> TreeError {
    TreeError::InvalidStructure(message.into())
}
pub fn restructure_animation_paths(nodes: &[RestructureNode]) -> Result<Vec<Vec<u32>>, TreeError> {
    fn visit(
        nodes: &[RestructureNode],
        prefix: &[u32],
        output: &mut Vec<Vec<u32>>,
    ) -> Result<(), TreeError> {
        for (index, node) in nodes.iter().enumerate() {
            if output.len() >= MAXIMUM_RECOMMENDATION_NODES
                || prefix.len() >= MAXIMUM_RECOMMENDATION_DEPTH
            {
                return Err(recommendation_error(
                    "Recommendation tree exceeds its node or depth bound",
                ));
            }
            let mut path = prefix.to_vec();
            path.push(
                u32::try_from(index)
                    .map_err(|_| recommendation_error("Recommendation child index is too large"))?,
            );
            output.push(path.clone());
            let children = match node {
                RestructureNode::Group { children, .. }
                | RestructureNode::ExistingGroup { children, .. }
                | RestructureNode::ExistingSplitView { children, .. } => children.as_slice(),
                RestructureNode::Pane { .. } | RestructureNode::Folder { .. } => &[],
            };
            visit(children, &path, output)?;
        }
        Ok(())
    }
    let mut output = Vec::new();
    visit(nodes, &[], &mut output)?;
    Ok(output)
}
impl RecommendedRestructurePlan {
    pub fn validate_assignments(&self) -> Result<(), TreeError> {
        self.project
            .validate_shape()
            .map_err(recommendation_error)?;
        let expected: BTreeSet<Vec<u32>> = restructure_animation_paths(&self.structure.children)?
            .into_iter()
            .collect();
        let mut actual = BTreeSet::new();
        let mut parameter_count = self.project.parameters.len();
        for entry in &self.entries {
            entry
                .recommendation
                .validate_shape()
                .map_err(recommendation_error)?;
            if !actual.insert(entry.path.clone()) {
                return Err(recommendation_error("Duplicate recommendation output path"));
            }
            parameter_count = parameter_count.saturating_add(entry.recommendation.parameters.len());
            if parameter_count > MAXIMUM_RECOMMENDATION_PARAMETERS {
                return Err(recommendation_error(
                    "Expanded recommendation parameters exceed the admission bound",
                ));
            }
        }
        if actual != expected {
            return Err(recommendation_error(
                "Recommendations must cover exactly every output-node path",
            ));
        }
        Ok(())
    }
}
impl Tree {
    pub fn project_animation_generation(&self, project_id: NodeId) -> Result<u64, TreeError> {
        self.get(project_id)
            .filter(|node| node.is_project())
            .map(|node| node.animation_generation)
            .ok_or(TreeError::NotAProject(project_id))
    }
    pub(crate) fn next_project_animation_generation(
        &self,
        project_id: NodeId,
    ) -> Result<u64, TreeError> {
        self.project_animation_generation(project_id)?
            .checked_add(1)
            .ok_or_else(|| recommendation_error("Project animation generation is exhausted"))
    }
    pub fn project_has_complete_animation_recommendations(
        &self,
        project_id: NodeId,
    ) -> Result<bool, TreeError> {
        self.project_animation_generation(project_id)?;
        Ok(self
            .all_ids()
            .filter(|id| *id == project_id || self.project_ancestor(*id) == Some(project_id))
            .all(|id| {
                self.get(id)
                    .and_then(|node| node.inferred_animation.as_ref())
                    .is_some_and(|value| value.validate_shape().is_ok())
            }))
    }
    pub fn apply_recommended_project_restructure(
        &mut self,
        project_id: NodeId,
        plan: RecommendedRestructurePlan,
        inference_activity_revisions: &[NodeActivityRevision],
    ) -> Result<Vec<NodeActivityRevision>, TreeError> {
        if self.project_animation_generation(project_id)? != plan.expected_animation_generation {
            return Err(recommendation_error("Stale project animation generation"));
        }
        plan.validate_assignments()?;
        let mut updated = self.clone();
        let checkpoints = updated.apply_project_restructure_with_activity_checkpoint(
            project_id,
            plan.structure,
            inference_activity_revisions,
        )?;
        for entry in plan.entries {
            let mut node_id = project_id;
            for ordinal in entry.path {
                let index = usize::try_from(ordinal)
                    .map_err(|_| recommendation_error("Unsupported recommendation child index"))?;
                node_id = *updated.children_of(node_id)?.get(index).ok_or_else(|| {
                    recommendation_error("Recommendation path does not match the rebuilt tree")
                })?;
            }
            updated.get_mut(node_id)?.inferred_animation = Some(entry.recommendation);
        }
        updated.get_mut(project_id)?.inferred_animation = Some(plan.project);
        updated.validate()?;
        *self = updated;
        Ok(checkpoints)
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use crate::{PaneContentKind, SplitOrientation};
    fn recommendation() -> AnimationRecommendation {
        AnimationRecommendation {
            version: 1,
            kind: "carpet".into(),
            resources: ResourcePolicy::Catalog,
            parameters: vec![AnimationParameter {
                id: "carpet_mode".into(),
                value: AnimationValue::Choice {
                    index: 1,
                    label: "Autonomous Snake".into(),
                },
            }],
        }
    }
    fn fixture() -> (Tree, NodeId, NodeId, NodeId, RecommendedRestructurePlan) {
        let mut tree = Tree::new();
        let path = std::env::temp_dir().join("ilium-semantic-domain-fixture");
        let project = tree.add_project(path.clone()).unwrap();
        let group = tree.add_group(project, "Fixed group").unwrap();
        tree.rename_node(group, "Fixed group", None, None).unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "second", PaneContentKind::Editor)
            .unwrap();
        let split = tree
            .create_split_view(
                group,
                "Protected",
                SplitOrientation::Horizontal,
                &[first, second],
            )
            .unwrap();
        let folder = tree.add_folder(project, path.join("assets")).unwrap();
        let pane = |id| RestructureNode::Pane {
            id,
            title: "Model title".into(),
            short_title: None,
            icon: None,
        };
        let structure = crate::RestructurePlan {
            children: vec![
                RestructureNode::ExistingGroup {
                    id: group,
                    children: vec![RestructureNode::ExistingSplitView {
                        id: split,
                        children: vec![pane(first), pane(second)],
                    }],
                },
                RestructureNode::Folder {
                    id: folder,
                    title: "Assets".into(),
                    short_title: None,
                    icon: None,
                },
                RestructureNode::Group {
                    title: "New group".into(),
                    short_title: None,
                    icon: None,
                    children: vec![],
                },
            ],
        };
        let entries = restructure_animation_paths(&structure.children)
            .unwrap()
            .into_iter()
            .map(|path| PlanAnimationEntry {
                path,
                recommendation: recommendation(),
            })
            .collect();
        let plan = RecommendedRestructurePlan {
            structure,
            expected_animation_generation: 0,
            project: recommendation(),
            entries,
        };
        (tree, project, first, split, plan)
    }
    #[test]
    fn exact_path_coverage_and_envelope_errors_are_atomic() {
        let (mut tree, project, _, _, plan) = fixture();
        assert_eq!(
            restructure_animation_paths(&plan.structure.children).unwrap(),
            vec![
                vec![0],
                vec![0, 0],
                vec![0, 0, 0],
                vec![0, 0, 1],
                vec![1],
                vec![2]
            ]
        );
        let mut missing = plan.clone();
        missing.entries.remove(1);
        let mut duplicate = plan.clone();
        duplicate.entries.push(duplicate.entries[0].clone());
        let mut extra = plan.clone();
        extra.entries[0].path = vec![99];
        let mut malformed = plan.clone();
        malformed.project.kind = "semantic".into();
        let revisions = tree.project_activity_revisions(project).unwrap();
        for candidate in [missing, duplicate, extra, malformed] {
            let before = tree.clone();
            assert!(tree
                .apply_recommended_project_restructure(project, candidate, &revisions)
                .is_err());
            assert_eq!(tree, before);
        }
    }
    #[test]
    fn accepted_plan_preserves_authored_names_splits_and_newer_activity() {
        let (mut tree, project, first, split, plan) = fixture();
        let revisions = tree.project_activity_revisions(project).unwrap();
        let old_revision = tree.get(first).unwrap().activity_revision;
        tree.rename_node(first, "Authored during inference", None, None)
            .unwrap();
        tree.record_node_activity(first).unwrap();
        tree.apply_recommended_project_restructure(project, plan, &revisions)
            .unwrap();
        assert_eq!(tree.get(first).unwrap().name, "Authored during inference");
        assert_eq!(
            tree.get(first).unwrap().last_restructure_activity_revision,
            Some(old_revision)
        );
        assert!(tree.get(first).unwrap().has_unrestructured_activity());
        assert_eq!(tree.get(split).unwrap().name, "Protected");
        assert_eq!(
            tree.split_orientation(split),
            Some(SplitOrientation::Horizontal)
        );
        assert!(tree
            .project_has_complete_animation_recommendations(project)
            .unwrap());
        assert_eq!(tree.project_animation_generation(project).unwrap(), 1);
    }
    #[test]
    fn changed_protected_order_is_rejected_atomically() {
        let (mut tree, project, _, _, mut plan) = fixture();
        let RestructureNode::ExistingGroup { children, .. } = &mut plan.structure.children[0]
        else {
            panic!("fixture group")
        };
        let RestructureNode::ExistingSplitView { children, .. } = &mut children[0] else {
            panic!("fixture split")
        };
        children.reverse();
        let before = tree.clone();
        let revisions = tree.project_activity_revisions(project).unwrap();
        assert!(tree
            .apply_recommended_project_restructure(project, plan, &revisions)
            .is_err());
        assert_eq!(tree, before);
    }
    #[test]
    fn stale_generation_and_undo_are_fenced() {
        let (mut tree, project, first, _, plan) = fixture();
        let previous = tree.clone();
        let revisions = tree.project_activity_revisions(project).unwrap();
        tree.apply_recommended_project_restructure(project, plan.clone(), &revisions)
            .unwrap();
        let accepted = tree.clone();
        assert!(tree
            .apply_recommended_project_restructure(project, plan.clone(), &revisions)
            .is_err());
        assert_eq!(tree, accepted);
        tree.restore_project_from(project, &previous).unwrap();
        assert!(tree.get(project).unwrap().inferred_animation.is_none());
        assert!(tree.get(first).unwrap().inferred_animation.is_none());
        assert_eq!(tree.project_animation_generation(project).unwrap(), 2);
        assert!(tree
            .apply_recommended_project_restructure(project, plan, &revisions)
            .is_err());
    }
    #[test]
    fn generation_exhaustion_preserves_the_tree() {
        let (mut tree, project, _, _, mut plan) = fixture();
        let previous = tree.clone();
        tree.get_mut(project).unwrap().animation_generation = u64::MAX;
        plan.expected_animation_generation = u64::MAX;
        let before = tree.clone();
        let revisions = tree.project_activity_revisions(project).unwrap();
        assert!(tree
            .apply_recommended_project_restructure(project, plan, &revisions)
            .is_err());
        assert_eq!(tree, before);
        assert!(tree.restore_project_from(project, &previous).is_err());
        assert_eq!(tree, before);
    }
}
