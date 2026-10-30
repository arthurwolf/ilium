use ilium_core::{resource_queue::*, NodeId, Tree};
use ratatui::text::Line;
use std::collections::{HashMap, HashSet};
use tui_tree_widget::TreeItem;
fn single_line(text: &str) -> String {
    super::terminal_text(text)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRowTarget {
    Resource {
        project_id: NodeId,
        resource_id: ResourceId,
    },
    Participant {
        project_id: NodeId,
        resource_id: ResourceId,
        reservation_id: ReservationId,
        pane_id: Option<NodeId>,
    },
}
#[derive(Debug, Default)]
pub struct ResourceRows {
    pub targets: HashMap<NodeId, ResourceRowTarget>,
    pub projects: HashMap<NodeId, Vec<NodeId>>,
    next: u64,
}
impl ResourceRows {
    pub fn rebuild(
        &mut self,
        tree: &Tree,
        catalog: &ResourceCatalog,
        session: &str,
        local_invocations: &[AgentIdentity],
    ) {
        let previous = std::mem::take(&mut self.targets);
        self.projects.clear();
        for project_id in tree.all_ids() {
            let Some(path) = tree.get(project_id).and_then(|node| node.project_path()) else {
                continue;
            };
            let mut links: Vec<_> = catalog
                .links
                .iter()
                .filter(|link| std::path::Path::new(&link.project_path) == path)
                .collect();
            links.sort_by(|left, right| {
                catalog
                    .resources
                    .get(&left.resource_id)
                    .map(|record| &record.definition.title)
                    .cmp(
                        &catalog
                            .resources
                            .get(&right.resource_id)
                            .map(|record| &record.definition.title),
                    )
                    .then(left.resource_id.cmp(&right.resource_id))
            });
            for link in links {
                let Some(record) = catalog
                    .resources
                    .get(&link.resource_id)
                    .filter(|record| !record.definition.archived)
                else {
                    continue;
                };
                let target = ResourceRowTarget::Resource {
                    project_id,
                    resource_id: link.resource_id.clone(),
                };
                let id = self.allocate(tree, &previous, target);
                self.projects.entry(project_id).or_default().push(id);
                for reservation in ordered(record) {
                    let local = local_invocations
                        .iter()
                        .any(|live| live.same_invocation(&reservation.agent))
                        && reservation.agent.session_id == session
                        && catalog.links.iter().any(|candidate| {
                            candidate.project_id == reservation.agent.project_id
                                && std::path::Path::new(&candidate.project_path) == path
                        });
                    let pane_id = local
                        .then_some(reservation.agent.pane_id)
                        .filter(|pane| tree.get(*pane).is_some_and(|node| node.is_pane()));
                    self.allocate(
                        tree,
                        &previous,
                        ResourceRowTarget::Participant {
                            project_id,
                            resource_id: link.resource_id.clone(),
                            reservation_id: reservation.id.clone(),
                            pane_id,
                        },
                    );
                }
            }
        }
    }
    fn allocate(
        &mut self,
        tree: &Tree,
        previous: &HashMap<NodeId, ResourceRowTarget>,
        target: ResourceRowTarget,
    ) -> NodeId {
        if let Some((id, _)) = previous.iter().find(|(_, old)| match (*old, &target) {
            (
                ResourceRowTarget::Participant {
                    project_id: left_project,
                    resource_id: left_resource,
                    reservation_id: left_reservation,
                    ..
                },
                ResourceRowTarget::Participant {
                    project_id: right_project,
                    resource_id: right_resource,
                    reservation_id: right_reservation,
                    ..
                },
            ) => {
                left_project == right_project
                    && left_resource == right_resource
                    && left_reservation == right_reservation
            }
            _ => **old == target,
        }) {
            self.targets.insert(*id, target);
            return *id;
        }
        loop {
            self.next = self.next.saturating_add(1);
            let id = NodeId((1_u64 << 61) | self.next);
            if tree.get(id).is_none() && !self.targets.contains_key(&id) {
                self.targets.insert(id, target);
                return id;
            }
        }
    }
    pub fn items(
        &self,
        project: NodeId,
        catalog: &ResourceCatalog,
        group: bool,
        icons: &crate::icon_settings::IconSettings,
        stable: bool,
        visible_nodes: Option<&HashSet<NodeId>>,
    ) -> Vec<TreeItem<'static, NodeId>> {
        self.projects.get(&project).into_iter().flatten().filter_map(|id|{
   let ResourceRowTarget::Resource{resource_id,..}=self.targets.get(id)? else {return None;};
   let record=catalog.resources.get(resource_id)?;
   let queue=ordered(record);
   let waiters=queue.iter().filter(|reservation|matches!(reservation.state,ReservationState::Queued)).count();
   let state=configured_state_label(record,icons,stable);
   let mut children=Vec::new();
   if group {
    for reservation in queue {
     let row_id=self.targets.iter().find_map(|(id,target)|matches!(target,ResourceRowTarget::Participant{project_id,resource_id:resource,reservation_id,..} if *project_id==project&&resource==&record.definition.id&&reservation_id==&reservation.id).then_some(*id))?;
     if matches!(self.targets.get(&row_id), Some(ResourceRowTarget::Participant { pane_id: Some(pane_id), .. }) if visible_nodes.is_some_and(|visible| !visible.contains(pane_id))) { continue; }
     let prefix=if matches!(reservation.state,ReservationState::Queued) {let position=record.reservations.iter().filter(|other|matches!(other.state,ReservationState::Queued)&&other.ticket<=reservation.ticket).count(); if waiters>1 {format!("{} {position}. ",icons.glyph_for_display(crate::icon_settings::IconTarget::ResourceQueued,stable))}else{format!("{} ",icons.glyph_for_display(crate::icon_settings::IconTarget::ResourceQueued,stable))}}else{format!("{} ",icons.glyph_for_display(crate::icon_settings::IconTarget::ResourceOwner,stable))};
     let local=matches!(self.targets.get(&row_id),Some(ResourceRowTarget::Participant{pane_id:Some(_),..}));
     children.push(TreeItem::new_leaf(row_id,Line::from(format!("{prefix}{}{}",single_line(&reservation.agent.label),if local {""}else{" ↗ referenced invocation"}))));
    }
   }
   let label=Line::from(format!("{state} {}  {} {waiters}",single_line(&record.definition.title),icons.glyph_for_display(crate::icon_settings::IconTarget::ResourceQueue,stable)));
   TreeItem::new(*id,label,children).ok()
  }).collect()
    }
}
pub fn ordered(record: &ResourceRecord) -> Vec<&Reservation> {
    let mut reservations: Vec<_> = record
        .reservations
        .iter()
        .filter(|reservation| reservation.state.active())
        .collect();
    reservations.sort_by_key(|reservation| (!reservation.state.occupied(), reservation.ticket));
    reservations
}
pub fn configured_state_label(
    record: &ResourceRecord,
    icons: &crate::icon_settings::IconSettings,
    stable: bool,
) -> String {
    use crate::icon_settings::IconTarget;
    let occupied = record
        .reservations
        .iter()
        .any(|claim| claim.state.occupied());
    let attention = record
        .reservations
        .iter()
        .any(|claim| matches!(claim.state, ReservationState::NeedsAttention { .. }));
    let mut parts = Vec::new();
    if record.clearance_required {
        parts.push(format!(
            "{} Clearance required",
            icons.glyph_for_display(IconTarget::ResourceAttention, stable)
        ));
    } else if attention {
        parts.push(format!(
            "{} Needs attention",
            icons.glyph_for_display(IconTarget::ResourceAttention, stable)
        ));
    } else if occupied {
        parts.push(format!(
            "{} Taken",
            icons.glyph_for_display(IconTarget::ResourceTaken, stable)
        ));
    }
    match record.gate {
        ResourceGate::Paused { .. } => parts.push(format!(
            "{} Paused",
            icons.glyph_for_display(IconTarget::ResourcePaused, stable)
        )),
        ResourceGate::Unavailable { .. } => parts.push(format!(
            "{} Unavailable",
            icons.glyph_for_display(IconTarget::ResourceUnavailable, stable)
        )),
        ResourceGate::Open if parts.is_empty() => parts.push(format!(
            "{} Free",
            icons.glyph_for_display(IconTarget::ResourceFree, stable)
        )),
        ResourceGate::Open => {}
    }
    parts.join(" · ")
}

#[cfg(test)]
pub fn state_label(record: &ResourceRecord) -> &'static str {
    if record.clearance_required {
        return "⚠ Clearance required";
    }
    if record
        .reservations
        .iter()
        .any(|reservation| matches!(reservation.state, ReservationState::NeedsAttention { .. }))
    {
        return "⚠ Needs attention";
    }
    match &record.gate {
        ResourceGate::Paused { .. } => "⏸ Paused",
        ResourceGate::Unavailable { .. } => "⊘ Unavailable",
        ResourceGate::Open
            if record
                .reservations
                .iter()
                .any(|reservation| reservation.state.occupied()) =>
        {
            "🔒 Taken"
        }
        ResourceGate::Open => "✓ Free",
    }
}
/// Only a top-level, ordinary local pane is suppressed; split members remain references.
pub fn grouped_local_panes(rows: &ResourceRows, tree: &Tree, project: NodeId) -> HashSet<NodeId> {
    rows.targets
        .values()
        .filter_map(|target| match target {
            ResourceRowTarget::Participant {
                project_id,
                pane_id: Some(pane),
                ..
            } if *project_id == project
                && tree.project_ancestor(*pane) == Some(project)
                && !tree
                    .parent_of(*pane)
                    .and_then(|parent| tree.get(parent))
                    .is_some_and(|node| node.is_split_view()) =>
            {
                Some(*pane)
            }
            _ => None,
        })
        .collect()
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> (Tree, ResourceCatalog, NodeId, NodeId) {
        let mut tree = Tree::new();
        let project = tree
            .add_project(std::path::PathBuf::from("/synthetic-resource-project"))
            .unwrap();
        let pane = tree
            .add_pane(
                project,
                "waiting agent",
                ilium_core::PaneContentKind::Terminal,
            )
            .unwrap();
        let resource_id = ResourceId::from("gpu");
        let clock = ResourceClock {
            unix_millis: 1000,
            monotonic_millis: 1000,
            boot_id: "boot".into(),
        };
        let reservation = Reservation {
            id: ReservationId::from("reservation"),
            request_id: RequestId::from("request"),
            resource_id: resource_id.clone(),
            agent: AgentIdentity {
                project_id: ResourceProjectId::from("project"),
                session_id: "test".into(),
                pane_id: pane,
                runtime_generation: "runtime".into(),
                invocation_id: InvocationId::from("invocation"),
                label: "Waiting agent".into(),
            },
            ticket: 2,
            estimate_seconds: 60,
            requested_at: clock,
            acquired_at: None,
            terminal_at: None,
            state: ReservationState::Queued,
            ownership_generation: 0,
            acquisition_pending: false,
        };
        let record = ResourceRecord {
            definition: ResourceDefinition {
                id: resource_id.clone(),
                title: "GPU".into(),
                policy: ResourcePolicy::ExclusiveFifo,
                created_at_unix_millis: 0,
                updated_at_unix_millis: 0,
                kind: "gpu".into(),
                physical_identity: None,
                description: String::new(),
                rules: String::new(),
                definition_revision: 1,
                archived: false,
            },
            gate: ResourceGate::Open,
            clearance_required: false,
            next_ticket: 3,
            next_generation: 1,
            reservations: vec![reservation],
        };
        let mut catalog = ResourceCatalog::default();
        catalog.resources.insert(resource_id.clone(), record);
        catalog.links.push(ProjectResourceLink {
            project_id: ResourceProjectId::from("project"),
            resource_id,
            slug: "gpu".into(),
            project_path: "/synthetic-resource-project".into(),
            desired_instruction_revision: 1,
            created_at_unix_millis: 0,
            sync_debt: None,
        });
        (tree, catalog, project, pane)
    }
    fn live(catalog: &ResourceCatalog) -> Vec<AgentIdentity> {
        catalog
            .resources
            .values()
            .flat_map(|record| {
                record
                    .reservations
                    .iter()
                    .map(|reservation| reservation.agent.clone())
            })
            .collect()
    }
    #[test]
    fn grouping_is_projection_and_does_not_change_core_tree() {
        let (tree, catalog, project, pane) = fixture();
        let before = serde_json::to_vec(&tree).unwrap();
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "test", &live(&catalog));
        let grouped = rows.items(project, &catalog, true, &Default::default(), false, None);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].children().len(), 1);
        assert!(grouped_local_panes(&rows, &tree, project).contains(&pane));
        assert!(
            rows.items(project, &catalog, false, &Default::default(), false, None)[0]
                .children()
                .is_empty()
        );
        assert_eq!(before, serde_json::to_vec(&tree).unwrap());
    }
    #[test]
    fn activity_filter_keeps_resource_row_but_hides_inactive_local_participant() {
        let (tree, catalog, project, pane) = fixture();
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "test", &live(&catalog));
        let hidden = rows.items(
            project,
            &catalog,
            true,
            &Default::default(),
            false,
            Some(&HashSet::new()),
        );
        assert_eq!(hidden.len(), 1);
        assert!(hidden[0].children().is_empty());
        let visible = rows.items(
            project,
            &catalog,
            true,
            &Default::default(),
            false,
            Some(&HashSet::from([pane])),
        );
        assert_eq!(visible[0].children().len(), 1);
    }
    #[test]
    fn row_identity_survives_snapshot_and_queue_reordering() {
        let (tree, mut catalog, project, _) = fixture();
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "test", &live(&catalog));
        let old = rows.projects[&project][0];
        catalog.revision += 1;
        rows.rebuild(&tree, &catalog, "test", &live(&catalog));
        assert_eq!(old, rows.projects[&project][0]);
    }
    #[test]
    fn foreign_session_pane_id_never_resolves_to_local_terminal() {
        let (tree, catalog, _, _) = fixture();
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "another-session", &live(&catalog));
        assert!(rows
            .targets
            .values()
            .filter(|target| matches!(target, ResourceRowTarget::Participant { .. }))
            .all(|target| matches!(target, ResourceRowTarget::Participant { pane_id: None, .. })));
    }
    #[test]
    fn participant_row_identity_survives_invocation_becoming_stale() {
        let (tree, catalog, _, _) = fixture();
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "test", &live(&catalog));
        let old = rows
            .targets
            .iter()
            .find_map(|(id, target)| {
                matches!(target, ResourceRowTarget::Participant { .. }).then_some(*id)
            })
            .unwrap();
        rows.rebuild(&tree, &catalog, "test", &[]);
        assert!(matches!(
            rows.targets.get(&old),
            Some(ResourceRowTarget::Participant { pane_id: None, .. })
        ));
    }
    #[test]
    fn stale_invocation_never_aliases_restored_local_pane() {
        let (tree, catalog, _, _) = fixture();
        let mut proofs = live(&catalog);
        proofs[0].invocation_id = InvocationId::from("replacement");
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "test", &proofs);
        assert!(rows.targets.values().all(|target| !matches!(
            target,
            ResourceRowTarget::Participant {
                pane_id: Some(_),
                ..
            }
        )));
    }
    #[test]
    fn resource_rows_sort_by_title_then_identity_instead_of_local_slug() {
        let (tree, mut catalog, project, _) = fixture();
        let mut second = catalog.resources.values().next().unwrap().clone();
        second.definition.id = ResourceId::from("aaa");
        second.definition.title = "AAA board".into();
        second.reservations.clear();
        let mut link = catalog.links[0].clone();
        link.resource_id = second.definition.id.clone();
        link.slug = "zzz".into();
        catalog
            .resources
            .insert(second.definition.id.clone(), second);
        catalog.links.push(link);
        let mut rows = ResourceRows::default();
        rows.rebuild(&tree, &catalog, "test", &live(&catalog));
        assert!(
            matches!(rows.targets.get(&rows.projects[&project][0]),Some(ResourceRowTarget::Resource{resource_id,..}) if resource_id==&ResourceId::from("aaa"))
        );
    }
    #[test]
    fn participants_order_owner_before_fifo_tickets() {
        let (_, mut catalog, _, _) = fixture();
        let record = catalog.resources.values_mut().next().unwrap();
        let mut owner = record.reservations[0].clone();
        owner.id = ReservationId::from("owner");
        owner.ticket = 1;
        owner.state = ReservationState::Owned;
        let mut last = record.reservations[0].clone();
        last.id = ReservationId::from("last");
        last.ticket = 9;
        record.reservations.insert(0, last);
        record.reservations.push(owner);
        assert_eq!(
            ordered(record)
                .iter()
                .map(|reservation| reservation.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["owner", "reservation", "last"]
        );
    }
    #[test]
    fn resource_status_uses_configured_semantic_icons() {
        let (_, catalog, _, _) = fixture();
        let record = catalog.resources.values().next().unwrap();
        let mut icons = crate::icon_settings::IconSettings::default();
        icons.set(crate::icon_settings::IconTarget::ResourceFree, "◎".into());
        assert_eq!(configured_state_label(record, &icons, false), "◎ Free");
    }
    #[test]
    fn blocked_gate_never_looks_free_and_clearance_is_distinct() {
        let (_, mut catalog, _, _) = fixture();
        let record = catalog.resources.values_mut().next().unwrap();
        record.reservations.clear();
        record.gate = ResourceGate::Unavailable {
            reason: "unplugged".into(),
        };
        assert!(state_label(record).contains("Unavailable"));
        record.gate = ResourceGate::Open;
        record.clearance_required = true;
        assert!(state_label(record).contains("Clearance"));
    }
}
