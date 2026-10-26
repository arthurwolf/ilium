//! Client projection only. The shared server catalog remains authoritative.
mod detail;
mod dialog;
mod help;
#[cfg(test)]
mod near_limit_tests;
mod presentation;
pub mod segments;
pub(crate) mod tree;
use crate::app::{App, FocusTarget, RightPanelTarget};
pub use dialog::ResourceDialog;
use ilium_core::{resource_queue::*, NodeId};
use std::collections::HashMap;
pub use tree::{ResourceRowTarget, ResourceRows};
// Terminal presentation must never interpret catalog text as control sequences.
pub(super) fn terminal_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}
pub(super) fn sanitized_lines(
    mut lines: Vec<ratatui::text::Line<'static>>,
) -> Vec<ratatui::text::Line<'static>> {
    for line in &mut lines {
        for span in &mut line.spans {
            span.content = terminal_text(&span.content).into();
        }
    }
    lines
}
pub(super) fn new_mutation_key() -> String {
    format!(
        "{}:{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4()
    )
}
#[derive(Debug, Default)]
pub struct ResourceUi {
    pub help_open: bool,
    pub help_scroll: u16,
    pub catalog: Option<ResourceCatalog>,
    pub local_invocations: Vec<AgentIdentity>,
    pub clock: Option<ResourceClock>,
    pub observed_at: Option<std::time::Instant>,
    pub grouping: bool,
    pub rows: ResourceRows,
    pub dialog: Option<ResourceDialog>,
    pub waits: HashMap<NodeId, ReservationId>,
    pub wait_revision: Option<u64>,
    pub next_request: u64,
    pub pending: Option<u64>,
    pub retry_request: Option<ilium_ipc::resource::ResourceRequest>,
    pub error: Option<String>,
    pub observation_error: Option<String>,
    pub scroll: u16,
    pub previous_target: Option<RightPanelTarget>,
    pub recovery_intent: Option<(NodeId, ResourceCommand)>,
    pub selected_reservation: Option<ReservationId>,
}
impl ResourceUi {
    pub fn record(&self, id: &ResourceId) -> Option<&ResourceRecord> {
        self.catalog.as_ref()?.resources.get(id)
    }
    pub fn clock_now(&self) -> Option<ResourceClock> {
        if self.observation_error.is_some() {
            return None;
        }
        let mut clock = self.clock.clone()?;
        let elapsed = u64::try_from(self.observed_at?.elapsed().as_millis()).ok()?;
        clock.monotonic_millis = clock.monotonic_millis.checked_add(elapsed)?;
        clock.unix_millis = clock
            .unix_millis
            .checked_add(i64::try_from(elapsed).ok()?)?;
        Some(clock)
    }
    pub fn waiting(&self, pane: NodeId) -> bool {
        self.rows.targets.values().any(|target| match target {
            ResourceRowTarget::Participant {
                pane_id: Some(local),
                resource_id,
                reservation_id,
                ..
            } if *local == pane => self.record(resource_id).is_some_and(|record| {
                record.reservations.iter().any(|reservation| {
                    &reservation.id == reservation_id
                        && matches!(reservation.state, ReservationState::Queued)
                })
            }),
            _ => false,
        })
    }
}
impl App {
    pub fn receive_resource_catalog(
        &mut self,
        catalog: ResourceCatalog,
        clock: ResourceClock,
        local_invocations: Vec<AgentIdentity>,
        active_waits: Vec<(NodeId, ReservationId)>,
        wait_revision: u64,
    ) -> bool {
        if let Some(old) = self
            .resources
            .catalog
            .as_ref()
            .filter(|old| old.revision > catalog.revision)
        {
            self.resources.observation_error=Some(format!("Resource authority revision regressed from {} to {}; ETA unknown until fresh state arrives",old.revision,catalog.revision));
            self.bump_tree_version();
            return false;
        }
        if self
            .resources
            .wait_revision
            .is_none_or(|revision| wait_revision >= revision)
        {
            self.resources.waits = active_waits.into_iter().collect();
            self.resources.wait_revision = Some(wait_revision);
        }
        self.resources.observation_error = None;
        self.resources.clock = Some(clock);
        self.resources.observed_at = Some(std::time::Instant::now());
        let previous_selection = self
            .tree_state
            .selected()
            .last()
            .and_then(|id| self.resources.rows.targets.get(id))
            .cloned();
        self.resources.grouping = self.ui_settings.resource_queue_grouping;
        self.resources
            .rows
            .rebuild(&self.tree, &catalog, &self.session_name, &local_invocations);
        self.resources.local_invocations = local_invocations;
        self.resources.catalog = Some(catalog);
        if let Some(ResourceRowTarget::Participant {
            pane_id,
            project_id,
            resource_id,
            reservation_id,
        }) = previous_selection
        {
            if !self.resources.rows.targets.values().any(|target|matches!(target,ResourceRowTarget::Participant{project_id:project,reservation_id:id,..} if *project==project_id&&id==&reservation_id)) {
    if let Some(pane)=pane_id.filter(|pane|self.tree.get(*pane).is_some()){self.select_node(pane);}else if let Some(row)=self.resources.rows.targets.iter().find_map(|(id,target)|matches!(target,ResourceRowTarget::Resource{project_id:project,resource_id:resource} if *project==project_id&&resource==&resource_id).then_some(*id)){let mut path=self.path_to(project_id);path.push(row);self.select_tree_path(path);}else{self.select_node(project_id);}
   }
        }
        if self.tree_state.selected().last().is_some_and(|id| {
            self.tree.get(*id).is_none() && !self.resources.rows.targets.contains_key(id)
        }) {
            let project = self
                .tree_state
                .selected()
                .iter()
                .copied()
                .find(|id| self.tree.get(*id).is_some_and(|node| node.is_project()));
            if let Some(project) = project {
                self.select_node(project);
            }
        }
        self.bump_tree_version();
        true
    }
    pub fn resource_request(
        &mut self,
        project: NodeId,
        operation: ilium_ipc::resource::ResourceOperation,
    ) {
        self.resource_request_with_token(project, operation, None, None);
    }
    pub fn resource_request_with_token(
        &mut self,
        project: NodeId,
        operation: ilium_ipc::resource::ResourceOperation,
        token: Option<String>,
        expected_revision: Option<u64>,
    ) {
        self.resource_request_full(project, operation, token, expected_revision, None, None);
    }
    pub fn resource_request_full(
        &mut self,
        project: NodeId,
        operation: ilium_ipc::resource::ResourceOperation,
        token: Option<String>,
        expected_revision: Option<u64>,
        request_key: Option<String>,
        pane_id: Option<NodeId>,
    ) {
        use ilium_ipc::{resource::ResourceRequest, ClientRequest};
        if self.resources.observation_error.is_some()
            && !matches!(&operation, ilium_ipc::resource::ResourceOperation::Snapshot)
        {
            self.resources.error=Some("Resource state cannot be observed; wait for a fresh catalog before management actions".into());
            return;
        }
        if self.resources.pending.is_some() {
            return;
        }
        self.resources.next_request = self.resources.next_request.saturating_add(1);
        let request_id = self.resources.next_request;
        let mut request = ResourceRequest::new(Some(project), operation);
        request.pane_id = pane_id;
        if matches!(
            &request.operation,
            ilium_ipc::resource::ResourceOperation::Mutate(_)
        ) {
            request.request_key = Some(request_key.unwrap_or_else(new_mutation_key));
        }
        request.recovery_token = token;
        request.expected_revision = expected_revision.or_else(|| {
            self.resources
                .catalog
                .as_ref()
                .map(|catalog| catalog.revision)
        });
        if self.queue_request(ClientRequest::Resource {
            request_id,
            request: request.clone(),
        }) {
            if matches!(
                request.operation,
                ilium_ipc::resource::ResourceOperation::Mutate(_)
            ) {
                self.resources.retry_request = Some(request);
            }
            self.resources.pending = Some(request_id);
            self.resources.error = None;
        }
    }
    pub fn retry_resource_request(&mut self) {
        if self.resources.observation_error.is_some() {
            self.resources.error =
                Some("Cannot retry while resource authority observation is stale".into());
            return;
        }
        let Some(request) = self.resources.retry_request.clone() else {
            self.resources.error = Some("No unresolved resource mutation to retry".into());
            return;
        };
        if let Some(pane) = request.pane_id {
            let valid = self.resources.local_invocations.iter().any(|agent| {
                agent.pane_id == pane && self.resource_request_actor_matches(&request, agent)
            });
            if !valid {
                self.resources.error =
                    Some("The original invocation is no longer verified; refusing retry".into());
                return;
            }
        }
        self.resources.next_request = self.resources.next_request.saturating_add(1);
        let request_id = self.resources.next_request;
        if self.queue_request(ilium_ipc::ClientRequest::Resource {
            request_id,
            request,
        }) {
            self.resources.pending = Some(request_id);
            if let Some(dialog) = &mut self.resources.dialog {
                dialog.submitted = true;
            }
            self.resources.error = None;
        }
    }
    pub fn resource_request_actor_matches(
        &self,
        request: &ilium_ipc::resource::ResourceRequest,
        agent: &AgentIdentity,
    ) -> bool {
        let ilium_ipc::resource::ResourceOperation::Mutate(command) = &request.operation else {
            return false;
        };
        let id = match command {
            ResourceCommand::Release { reservation_id, .. }
            | ResourceCommand::Cancel { reservation_id }
            | ResourceCommand::Estimate { reservation_id, .. } => reservation_id,
            _ => return false,
        };
        self.resources.catalog.as_ref().is_some_and(|catalog| {
            catalog
                .resources
                .values()
                .flat_map(|record| record.reservations.iter())
                .any(|reservation| {
                    &reservation.id == id
                        && reservation.agent.same_invocation(agent)
                        && match command {
                            ResourceCommand::Release {
                                ownership_generation,
                                ..
                            } => {
                                matches!(reservation.state, ReservationState::Owned)
                                    && reservation.ownership_generation == *ownership_generation
                            }
                            ResourceCommand::Cancel { .. } => {
                                matches!(reservation.state, ReservationState::Queued)
                            }
                            ResourceCommand::Estimate { .. } => reservation.state.active(),
                            _ => false,
                        }
                })
        })
    }
    pub fn verified_resource_actor(&self, reservation: &Reservation) -> Option<NodeId> {
        if self.resources.observation_error.is_some() {
            return None;
        }
        self.resources
            .local_invocations
            .iter()
            .any(|agent| agent.same_invocation(&reservation.agent))
            .then_some(reservation.agent.pane_id)
            .filter(|pane| self.tree.get(*pane).is_some_and(|node| node.is_pane()))
    }
    pub fn show_resource(&mut self, project_id: NodeId, resource_id: ResourceId) {
        if !matches!(self.right_panel_target, RightPanelTarget::Resource { .. }) {
            self.resources.previous_target = Some(self.right_panel_target.clone());
        }
        self.right_panel_target = RightPanelTarget::Resource {
            project_id,
            resource_id,
        };
        self.resources.scroll = 0;
        self.focus = FocusTarget::Pane;
    }
    pub fn open_resource_dialog(&mut self, project_id: NodeId, link: bool) {
        if self.resources.pending.is_some() {
            self.status_message = Some(
                "Wait for the pending resource request before opening another resource form".into(),
            );
            return;
        }
        self.resources.dialog = Some(if link {
            ResourceDialog::link(project_id)
        } else {
            ResourceDialog::create(project_id)
        });
        if let Some(dialog) = &mut self.resources.dialog {
            dialog.expected_revision = self
                .resources
                .catalog
                .as_ref()
                .map(|catalog| catalog.revision);
        }
        self.resources.error = None;
        self.resource_request(project_id, ilium_ipc::resource::ResourceOperation::Snapshot);
    }
    pub fn settings_toggle_resource_grouping(&mut self) {
        let mut ui = self.ui_settings.clone();
        ui.resource_queue_grouping = !ui.resource_queue_grouping;
        self.resources.grouping = ui.resource_queue_grouping;
        self.apply_and_persist_ui_settings(ui);
        self.bump_tree_version();
        // Restore canonical selection when a participant stops being grouped.
        if !self.resources.grouping {
            if let Some(ResourceRowTarget::Participant {
                pane_id: Some(pane),
                ..
            }) = self
                .tree_state
                .selected()
                .last()
                .and_then(|id| self.resources.rows.targets.get(id))
                .cloned()
            {
                self.select_node(pane);
            }
        }
        self.reconcile_selected_tree_path();
    }
    pub fn resource_select_target(&mut self, id: NodeId, target: ResourceRowTarget) {
        match target {
            ResourceRowTarget::Resource {
                project_id,
                resource_id,
            } => {
                let mut path = self.path_to(project_id);
                path.push(id);
                self.select_tree_path(path.clone());
                self.tree_state.open(path);
                self.show_resource(project_id, resource_id);
            }
            ResourceRowTarget::Participant {
                pane_id: Some(pane),
                project_id,
                resource_id,
                ..
            } => {
                self.focus_pane(pane);
                if let Some(resource_row)=self.resources.rows.targets.iter().find_map(|(id,target)|matches!(target,ResourceRowTarget::Resource{project_id:project,resource_id:resource} if *project==project_id&&resource==&resource_id).then_some(*id)){let mut path=self.path_to(project_id);path.push(resource_row);path.push(id);self.select_tree_path(path);}
            }
            ResourceRowTarget::Participant {
                project_id,
                resource_id,
                ..
            } => {
                self.show_resource(project_id, resource_id);
                self.status_message=Some("This participant is not a verified local invocation; inspect its identity in the resource queue".into());
            }
        }
    }
    pub fn receive_resource_reply(
        &mut self,
        request_id: u64,
        result: Result<ilium_ipc::resource::ResourceReply, ilium_ipc::resource::ResourceError>,
    ) {
        let correlated = self.resources.pending == Some(request_id);
        if correlated {
            self.resources.pending = None;
        }
        match result {
            Ok(reply) => {
                // A stale success is not confirmation: keep the exact retry envelope and form.
                let accepted = self.receive_resource_catalog(
                    reply.catalog,
                    reply.clock,
                    reply.local_invocations,
                    reply.active_waits,
                    reply.wait_revision,
                );
                if !accepted {
                    if correlated {
                        if let Some(dialog) = &mut self.resources.dialog {
                            dialog.submitted = false;
                        }
                    }
                    return;
                }
                if let Some(token) = reply.recovery_token {
                    if correlated {
                        if let Some((project_id, command)) = self.resources.recovery_intent.take() {
                            self.resources.dialog =
                                Some(ResourceDialog::confirm(project_id, command, Some(token)));
                        }
                    }
                } else if correlated && reply.result.is_some() {
                    self.resources.retry_request = None;
                    if let Some(dialog) = self
                        .resources
                        .dialog
                        .as_mut()
                        .filter(|dialog| dialog.submitted)
                    {
                        if matches!(dialog.kind, dialog::DialogKind::Edit(_)) {
                            dialog.submitted = false;
                            dialog.dirty_at = None;
                            dialog.expected_revision = self
                                .resources
                                .catalog
                                .as_ref()
                                .map(|catalog| catalog.revision);
                            dialog.request_key = new_mutation_key();
                        } else {
                            self.resources.dialog = None;
                        }
                    }
                }
                if let Some(dialog) = &mut self.resources.dialog {
                    if dialog.expected_revision.is_none() {
                        dialog.expected_revision = self
                            .resources
                            .catalog
                            .as_ref()
                            .map(|catalog| catalog.revision);
                    }
                }
                self.resources.error = None;
                let debt=self.resources.catalog.as_ref().and_then(|catalog|catalog.links.iter().find_map(|link|link.sync_debt.as_ref().map(|debt|format!("Resource committed; instruction/settings repair required: {debt}. Open the resource and press i to repair."))));
                if debt.is_some() {
                    self.status_message = debt;
                } else if self
                    .status_message
                    .as_ref()
                    .is_some_and(|message| message.starts_with("Resource committed;"))
                {
                    self.status_message = None;
                }
                if let Some(message) = reply.message {
                    self.status_message = Some(
                        match self
                            .status_message
                            .take()
                            .filter(|message| message.starts_with("Resource committed;"))
                        {
                            Some(warning) => format!("{message} · {warning}"),
                            None => message,
                        },
                    );
                }
            }
            Err(error) => {
                if correlated {
                    self.resources.recovery_intent = None;
                }
                self.resources.error = Some(format!("{}: {}", error.code, error.message));
                if let Some(dialog) = &mut self.resources.dialog {
                    dialog.submitted = false;
                }
            }
        }
    }
    pub fn resource_key(&mut self, event: &crossterm::event::Event) -> bool {
        if help::handle_event(self, event) {
            return true;
        }
        if dialog::handle_event(self, event) {
            return true;
        }
        if !matches!(self.mode, crate::app::Mode::Normal) || !self.modal_stack.is_empty() {
            return false;
        }
        if detail::handle_event(self, event) {
            return true;
        }
        if self.focus == FocusTarget::Tree {
            if let crossterm::event::Event::Key(key) = event {
                if matches!(key.code, crossterm::event::KeyCode::Enter) {
                    if let Some(id) = self.selected_node_id() {
                        if let Some(target) = self.resources.rows.targets.get(&id).cloned() {
                            self.resource_select_target(id, target);
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
    pub fn prepare_resource_recovery(
        &mut self,
        project: NodeId,
        resource: ResourceId,
        command: ResourceCommand,
    ) {
        if self.resources.pending.is_some() {
            return;
        }
        self.resources.recovery_intent = Some((project, command));
        self.resource_request(
            project,
            ilium_ipc::resource::ResourceOperation::PrepareRecovery {
                resource_id: resource,
            },
        );
    }
    pub fn open_resource_from_wait_footer(&mut self, position: ratatui::layout::Position) -> bool {
        if !self.ui_settings.progress_monitor_enabled {
            return false;
        }
        let target = self.pane_viewports().into_iter().find_map(|viewport| {
            let area = viewport.progress_area?;
            let strip = ratatui::layout::Rect::new(
                area.x,
                area.y,
                area.width,
                self.resource_wait_footer_rows(viewport.pane_id, area.height),
            );
            if !strip.contains(position) {
                return None;
            }
            let reservation = self.resources.waits.get(&viewport.pane_id)?;
            self.resources
                .rows
                .targets
                .values()
                .find_map(|target| match target {
                    ResourceRowTarget::Participant {
                        project_id,
                        resource_id,
                        reservation_id,
                        ..
                    } if reservation_id == reservation => {
                        Some((*project_id, resource_id.clone(), reservation_id.clone()))
                    }
                    _ => None,
                })
        });
        if let Some((project, resource, reservation)) = target {
            self.show_resource(project, resource);
            self.resources.selected_reservation = Some(reservation);
            true
        } else {
            false
        }
    }
    pub fn tick_resource_dialog(&mut self, now: std::time::Instant) -> bool {
        dialog::autosave(self, now)
    }
    pub fn resource_detail(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        detail::render(self, frame, area);
    }
    pub fn resource_overlay(&self, frame: &mut ratatui::Frame) {
        dialog::render(self, frame);
        help::render(self, frame);
    }
    pub fn resource_wait_footer(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        pane: NodeId,
    ) {
        detail::render_wait(self, frame, area, pane);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_grouping_is_enabled_and_progress_disable_does_not_mutate_reservations() {
        let app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        assert!(app.ui_settings.resource_queue_grouping);
        assert!(app.resources.catalog.is_none());
    }
    #[test]
    fn recovery_confirmation_keeps_original_project_and_revision() {
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        let project = NodeId(7);
        let resource = ResourceId::from("gpu");
        let command = ResourceCommand::ConfirmClear {
            resource_id: resource.clone(),
            reason: "synthetic safety confirmation".into(),
        };
        app.resources.pending = Some(9);
        app.resources.recovery_intent = Some((project, command));
        app.show_resource(NodeId(99), ResourceId::from("other-resource"));
        let mut catalog = ResourceCatalog::default();
        catalog.revision = 18;
        app.receive_resource_reply(
            9,
            Ok(ilium_ipc::resource::ResourceReply {
                catalog,
                wait_revision: 0,
                active_waits: Vec::new(),
                local_invocations: Vec::new(),
                clock: ResourceClock {
                    unix_millis: 1,
                    monotonic_millis: 1,
                    boot_id: "test".into(),
                },
                result: None,
                reservation: None,
                recovery_token: Some("synthetic-token".into()),
                message: None,
            }),
        );
        let dialog = app.resources.dialog.as_ref().unwrap();
        assert_eq!(dialog.project, project);
        assert_eq!(dialog.expected_revision, Some(18));
        assert!(
            matches!(&dialog.kind,dialog::DialogKind::Confirm{command,token:Some(token)} if token=="synthetic-token"&&matches!(command.as_ref(),ResourceCommand::ConfirmClear{resource_id,..} if resource_id==&resource))
        );
    }
    #[test]
    fn failed_observation_suppresses_estimates_without_releasing_wait() {
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        let clock = ResourceClock {
            unix_millis: 1,
            monotonic_millis: 1,
            boot_id: "test".into(),
        };
        app.receive_resource_catalog(
            ResourceCatalog::default(),
            clock.clone(),
            Vec::new(),
            Vec::new(),
            0,
        );
        app.resources
            .waits
            .insert(NodeId(8), ReservationId::from("wait"));
        crate::render_cache::apply(
            &mut app,
            ilium_ipc::ServerEvent::ResourceObservationFailed {
                message: "synthetic catalog unreadable".into(),
            },
        );
        assert!(app.resources.clock_now().is_none());
        assert!(app.resources.waits.contains_key(&NodeId(8)));
        app.receive_resource_catalog(ResourceCatalog::default(), clock, Vec::new(), Vec::new(), 0);
        assert!(app.resources.clock_now().is_some());
    }
    #[test]
    fn resource_view_is_nonterminal() {
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        app.show_resource(NodeId(1), ResourceId::from("gpu"));
        assert_eq!(app.right_panel_target.active_pane_id(), None);
        assert!(app.pane_viewports().is_empty());
    }
}

#[cfg(test)]
mod review_regressions {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ilium_ipc::{
        resource::{ResourceOperation, ResourceReply, ResourceRequest},
        ClientRequest,
    };
    fn setup() -> (App, NodeId, NodeId) {
        let (tree, mut catalog, project, pane) = tree::tests::fixture();
        catalog.revision = 7;
        let proof = catalog.resources.values().next().unwrap().reservations[0]
            .agent
            .clone();
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        app.tree = tree;
        app.receive_resource_catalog(
            catalog,
            ResourceClock {
                unix_millis: 1000,
                monotonic_millis: 1000,
                boot_id: "boot".into(),
            },
            vec![proof],
            vec![(pane, ReservationId::from("reservation"))],
            4,
        );
        (app, project, pane)
    }
    fn capture(app: &mut App) -> (u64, ResourceRequest) {
        app.take_outbound_requests()
            .into_iter()
            .find_map(|request| match request {
                ClientRequest::Resource {
                    request_id,
                    request,
                } => Some((request_id, request)),
                _ => None,
            })
            .expect("resource request was admitted")
    }
    fn reply(app: &App, revision: u64) -> ResourceReply {
        let mut catalog = app.resources.catalog.clone().unwrap();
        catalog.revision = revision;
        ResourceReply {
            catalog,
            local_invocations: app.resources.local_invocations.clone(),
            active_waits: Vec::new(),
            wait_revision: 5,
            clock: app.resources.clock.clone().unwrap(),
            result: Some(ResourceCommandResult {
                resource_id: Some(ResourceId::from("gpu")),
                reservation_id: None,
                revision,
                reused: false,
            }),
            reservation: None,
            recovery_token: None,
            message: None,
        }
    }
    #[test]
    fn final_create_envelope_has_timestamped_key_and_stable_nonempty_identity() {
        let (mut app, project, _) = setup();
        let mut form = ResourceDialog::create(project);
        form.fields[0] = "Second synthetic GPU".into();
        form.expected_revision = Some(7);
        let expected_id = form.create_id.clone();
        app.resources.dialog = Some(form);
        dialog::handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        let (_, request) = capture(&mut app);
        assert!(request_created_at(&RequestId(request.request_key.clone().unwrap())).is_ok());
        assert_eq!(request.expected_revision, Some(7));
        assert!(
            matches!(request.operation,ResourceOperation::Mutate(ResourceCommand::Create{definition,link}) if !definition.id.0.is_empty()&&definition.id==expected_id&&link.resource_id==expected_id)
        );
    }
    #[test]
    fn ambiguous_retry_resends_exact_envelope_despite_edited_form() {
        let (mut app, project, _) = setup();
        let mut form = ResourceDialog::create(project);
        form.fields[0] = "Synthetic second queue".into();
        form.expected_revision = Some(7);
        app.resources.dialog = Some(form);
        dialog::handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        let (first_id, first) = capture(&mut app);
        app.resources.dialog.as_mut().unwrap().fields[0] = "Edited after an unknown result".into();
        app.retry_resource_request();
        let (retry_id, retry) = capture(&mut app);
        assert_ne!(retry_id, first_id);
        assert_eq!(first, retry);
    }
    #[test]
    fn stale_success_preserves_form_and_disables_eta_until_fresh_snapshot() {
        let (mut app, project, _) = setup();
        let mut form = ResourceDialog::create(project);
        form.fields[0] = "Synthetic queue".into();
        form.submitted = true;
        app.resources.dialog = Some(form);
        app.resources.pending = Some(8);
        let stale = reply(&app, 6);
        app.receive_resource_reply(8, Ok(stale));
        assert!(app.resources.dialog.is_some());
        assert!(!app.resources.dialog.as_ref().unwrap().submitted);
        assert!(app.resources.clock_now().is_none());
        assert_eq!(app.resources.catalog.as_ref().unwrap().revision, 7);
        let catalog = app.resources.catalog.clone().unwrap();
        let clock = app.resources.clock.clone().unwrap();
        app.receive_resource_catalog(catalog, clock, Vec::new(), Vec::new(), 6);
        assert!(app.resources.clock_now().is_some());
    }
    #[test]
    fn late_attach_reconciles_waits_and_old_registry_snapshot_cannot_revert_them() {
        let (mut app, _, pane) = setup();
        assert_eq!(
            app.resources.waits.get(&pane),
            Some(&ReservationId::from("reservation"))
        );
        let catalog = app.resources.catalog.clone().unwrap();
        let clock = app.resources.clock.clone().unwrap();
        app.receive_resource_catalog(catalog.clone(), clock.clone(), Vec::new(), Vec::new(), 3);
        assert!(app.resources.waits.contains_key(&pane));
        crate::render_cache::apply(
            &mut app,
            ilium_ipc::ServerEvent::ResourceWaitChanged {
                pane_id: pane,
                reservation_id: None,
                wait_revision: 5,
            },
        );
        assert!(!app.resources.waits.contains_key(&pane));
        app.receive_resource_catalog(
            catalog,
            clock,
            Vec::new(),
            vec![(pane, ReservationId::from("reservation"))],
            4,
        );
        assert!(!app.resources.waits.contains_key(&pane));
    }
    #[test]
    fn own_release_captures_fenced_pane_and_generation_and_rejects_replacement() {
        let (mut app, project, pane) = setup();
        let record = app
            .resources
            .catalog
            .as_mut()
            .unwrap()
            .resources
            .values_mut()
            .next()
            .unwrap();
        let reservation = &mut record.reservations[0];
        reservation.state = ReservationState::Owned;
        reservation.ownership_generation = 13;
        let reservation = reservation.clone();
        let mut form = ResourceDialog::own_action(
            project,
            &reservation,
            ResourceCommand::Release {
                reservation_id: reservation.id.clone(),
                ownership_generation: 13,
            },
        );
        form.fields[0] = "CONFIRM".into();
        form.expected_revision = Some(7);
        app.resources.dialog = Some(form);
        dialog::handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        let (_, request) = capture(&mut app);
        assert_eq!(request.pane_id, Some(pane));
        assert!(matches!(
            request.operation,
            ResourceOperation::Mutate(ResourceCommand::Release {
                ownership_generation: 13,
                ..
            })
        ));
        app.resources.pending = None;
        app.resources.local_invocations[0].invocation_id = InvocationId::from("replacement");
        app.retry_resource_request();
        assert!(app.resources.pending.is_none());
        assert!(app.resources.error.as_ref().unwrap().contains("invocation"));
    }
    #[test]
    fn own_estimate_and_cancel_require_verified_invocation_and_current_state() {
        let (mut app, project, _) = setup();
        let reservation = app
            .resources
            .catalog
            .as_ref()
            .unwrap()
            .resources
            .values()
            .next()
            .unwrap()
            .reservations[0]
            .clone();
        let mut estimate = ResourceDialog::estimate(project, &reservation);
        estimate.fields[0] = "180".into();
        assert!(matches!(
            dialog::command(&app, &estimate),
            Ok(ResourceCommand::Estimate {
                estimate_seconds: 180,
                ..
            })
        ));
        let mut cancel = ResourceDialog::own_action(
            project,
            &reservation,
            ResourceCommand::Cancel {
                reservation_id: reservation.id.clone(),
            },
        );
        cancel.fields[0] = "CONFIRM".into();
        assert!(matches!(
            dialog::command(&app, &cancel),
            Ok(ResourceCommand::Cancel { .. })
        ));
        app.resources.local_invocations.clear();
        assert!(dialog::command(&app, &estimate).is_err());
        assert!(dialog::command(&app, &cancel).is_err());
    }
    #[test]
    fn committed_sync_debt_is_immediate_and_repair_action_is_project_scoped() {
        let (mut app, project, _) = setup();
        let mut response = reply(&app, 8);
        response.catalog.links[0].sync_debt = Some("synthetic denied instruction file".into());
        app.resources.pending = Some(3);
        app.receive_resource_reply(3, Ok(response));
        assert!(app
            .status_message
            .as_ref()
            .unwrap()
            .contains("repair required"));
        app.show_resource(project, ResourceId::from("gpu"));
        detail::handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)),
        );
        let (_, request) = capture(&mut app);
        assert_eq!(request.project_id, Some(project));
        assert!(
            matches!(request.operation,ResourceOperation::RepairInstructions{resource_id} if resource_id==ResourceId::from("gpu"))
        );
    }
}
