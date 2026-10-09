//! Client-local worktree inventory and exact-target prune flow.

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ilium_core::NodeId;
use ilium_ipc::{
    WorkspaceInventory, WorkspaceInventoryEntry, WorkspaceInventoryOwner,
    WorkspacePruneBranchPolicy, WorkspacePruneMode, WorkspacePruneOutcome, WorkspacePruneResult,
    WorkspacePruneTarget,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use crate::{modal, theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerView {
    Loading,
    Browsing,
    ConfirmSafe {
        target: WorkspacePruneTarget,
        branch_policy: WorkspacePruneBranchPolicy,
    },
    ConfirmDiscard {
        target: WorkspacePruneTarget,
        exact_path: String,
        typed_path: String,
        branch_policy: WorkspacePruneBranchPolicy,
    },
    Pruning,
    Result {
        result: WorkspacePruneResult,
    },
    Error(String),
}

pub struct WorktreeManagerState {
    pub project: NodeId,
    pub(crate) confirmation_identity: std::sync::Arc<()>,
    pub selected: usize,
    pub inventory: Option<WorkspaceInventory>,
    pub view: ManagerView,
    pending_inventory_request: Option<u64>,
    pending_prune: Option<(u64, WorkspacePruneTarget)>,
    pub(crate) inventory_retention: Option<crate::connection::EventRetention>,
    pub(crate) view_retention: Option<crate::connection::EventRetention>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerHit {
    Row(usize),
    Safe,
    Discard,
    ToggleBranch,
    Refresh,
    Close,
    Confirm,
    Cancel,
}

#[derive(Clone, Copy)]
pub struct ManagerLayout {
    pub popup: Rect,
    pub header: Rect,
    pub branch: Rect,
    pub list: Rect,
    pub actions: Rect,
    pub detail: Rect,
}

pub fn layout(screen: Rect) -> ManagerLayout {
    let popup = modal::centered_fixed_rect(100, 24, screen);
    let inner = theme::block(true).inner(popup);
    let action_height = inner.height.min(1);
    let header_height = inner.height.saturating_sub(action_height).min(2);
    let remaining = inner.height.saturating_sub(action_height + header_height);
    let detail_height = remaining.min(5);
    let list_height = remaining.saturating_sub(detail_height);
    let list_y = inner.y.saturating_add(header_height);
    ManagerLayout {
        popup,
        header: Rect::new(inner.x, inner.y, inner.width, header_height.min(1)),
        branch: Rect::new(
            inner.x,
            inner.y + header_height.min(1),
            inner.width,
            header_height.saturating_sub(1),
        ),
        list: Rect::new(inner.x, list_y, inner.width, list_height),
        detail: Rect::new(
            inner.x,
            list_y.saturating_add(list_height),
            inner.width,
            detail_height,
        ),
        actions: Rect::new(
            inner.x,
            inner.bottom().saturating_sub(action_height),
            inner.width,
            action_height,
        ),
    }
}

fn visible_window(selected: usize, count: usize, height: usize) -> (usize, usize) {
    if count <= height {
        return (0, count);
    }
    let start = selected.saturating_sub(height / 2).min(count - height);
    (start, start + height)
}

fn inventory_content_area(area: Rect, count: usize) -> Rect {
    if area.width >= 2 && count > usize::from(area.height) {
        Rect::new(area.x, area.y, area.width - 1, area.height)
    } else {
        area
    }
}

fn branch_toggle(policy: WorkspacePruneBranchPolicy) -> WorkspacePruneBranchPolicy {
    match policy {
        WorkspacePruneBranchPolicy::Keep => WorkspacePruneBranchPolicy::DeleteIfSafe,
        WorkspacePruneBranchPolicy::DeleteIfSafe => WorkspacePruneBranchPolicy::Keep,
    }
}

impl WorktreeManagerState {
    pub fn new(project: NodeId, request_id: u64) -> Self {
        Self {
            project,
            confirmation_identity: std::sync::Arc::new(()),
            selected: 0,
            inventory: None,
            view: ManagerView::Loading,
            pending_inventory_request: Some(request_id),
            pending_prune: None,
            inventory_retention: None,
            view_retention: None,
        }
    }

    pub fn begin_refresh(&mut self, request_id: u64) {
        self.pending_inventory_request = Some(request_id);
        self.pending_prune = None;
        self.view = ManagerView::Loading;
        self.view_retention = None;
    }

    pub fn receive_inventory(
        &mut self,
        request_id: u64,
        project: NodeId,
        result: Result<WorkspaceInventory, String>,
    ) -> bool {
        if self.project != project || self.pending_inventory_request != Some(request_id) {
            return false;
        }
        self.pending_inventory_request = None;
        match result {
            Ok(inventory) => {
                self.selected = self.selected.min(inventory.entries.len().saturating_sub(1));
                self.inventory = Some(inventory);
                self.inventory_retention = None;
                self.view = ManagerView::Browsing;
            }
            Err(error) => self.view = ManagerView::Error(error),
        }
        self.view_retention = None;
        true
    }

    pub fn selected_entry(&self) -> Option<&WorkspaceInventoryEntry> {
        self.inventory.as_ref()?.entries.get(self.selected)
    }

    pub fn select_delta(&mut self, delta: isize) {
        let count = self
            .inventory
            .as_ref()
            .map_or(0, |value| value.entries.len());
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
    }

    pub fn select(&mut self, index: usize) {
        if self
            .inventory
            .as_ref()
            .is_some_and(|inventory| index < inventory.entries.len())
        {
            self.selected = index;
        }
    }

    pub fn begin_safe_confirmation(&mut self) -> Result<(), String> {
        if !matches!(self.view, ManagerView::Browsing) {
            return Err("worktree inventory is not ready".into());
        }
        let row = self.selected_entry().ok_or("select a worktree first")?;
        if !matches!(row.owner, WorkspaceInventoryOwner::Owned) {
            return Err("only Ilium-owned worktrees can be removed".into());
        }
        if !row.safe_blockers.is_empty() {
            return Err(row.safe_blockers.join("; "));
        }
        let target = row.target.clone().ok_or("worktree target is unavailable")?;
        self.confirmation_identity = std::sync::Arc::new(());
        self.view = ManagerView::ConfirmSafe {
            target,
            branch_policy: WorkspacePruneBranchPolicy::Keep,
        };
        Ok(())
    }

    pub fn begin_discard_confirmation(&mut self) -> Result<(), String> {
        if !matches!(self.view, ManagerView::Browsing) {
            return Err("worktree inventory is not ready".into());
        }
        let row = self.selected_entry().ok_or("select a worktree first")?;
        if !matches!(row.owner, WorkspaceInventoryOwner::Owned) {
            return Err("only Ilium-owned worktrees can be removed".into());
        }
        if !row.discard_blockers.is_empty() {
            return Err(row.discard_blockers.join("; "));
        }
        let target = row.target.clone().ok_or("worktree target is unavailable")?;
        let exact_path = target
            .worktree_root
            .to_str()
            .filter(|path| !path.chars().any(char::is_control))
            .ok_or("this path cannot be confirmed as UTF-8 text")?
            .to_owned();
        self.confirmation_identity = std::sync::Arc::new(());
        self.view = ManagerView::ConfirmDiscard {
            target,
            exact_path,
            typed_path: String::new(),
            branch_policy: WorkspacePruneBranchPolicy::Keep,
        };
        Ok(())
    }

    pub(crate) fn confirmation_branch(
        &self,
    ) -> Option<(&WorkspacePruneTarget, WorkspacePruneBranchPolicy)> {
        match &self.view {
            ManagerView::ConfirmSafe {
                target,
                branch_policy,
            }
            | ManagerView::ConfirmDiscard {
                target,
                branch_policy,
                ..
            } => Some((target, *branch_policy)),
            _ => None,
        }
    }

    pub(crate) fn set_confirmation_branch(
        &mut self,
        policy: WorkspacePruneBranchPolicy,
    ) -> Result<(), String> {
        match &mut self.view {
            ManagerView::ConfirmSafe { branch_policy, .. }
            | ManagerView::ConfirmDiscard { branch_policy, .. } => {
                *branch_policy = policy;
                Ok(())
            }
            _ => Err("The removal confirmation changed; reopen branch choices".into()),
        }
    }

    pub fn toggle_branch_policy(&mut self) {
        match &mut self.view {
            ManagerView::ConfirmSafe { branch_policy, .. }
            | ManagerView::ConfirmDiscard { branch_policy, .. } => {
                *branch_policy = branch_toggle(*branch_policy);
            }
            _ => {}
        }
    }

    pub fn edit_discard_path(&mut self, key: KeyCode) {
        let ManagerView::ConfirmDiscard { typed_path, .. } = &mut self.view else {
            return;
        };
        match key {
            KeyCode::Backspace => {
                typed_path.pop();
            }
            KeyCode::Char(character) if !character.is_control() => {
                typed_path.push(character);
            }
            _ => {}
        }
    }

    pub fn cancel_confirmation(&mut self) {
        if matches!(
            self.view,
            ManagerView::ConfirmSafe { .. } | ManagerView::ConfirmDiscard { .. }
        ) {
            self.view = ManagerView::Browsing;
        }
    }

    pub fn confirmed_prune(
        &mut self,
        request_id: u64,
    ) -> Result<
        (
            WorkspacePruneTarget,
            WorkspacePruneMode,
            WorkspacePruneBranchPolicy,
        ),
        String,
    > {
        let (target, mode, branch_policy) = match &self.view {
            ManagerView::ConfirmSafe {
                target,
                branch_policy,
            } => (target.clone(), WorkspacePruneMode::Safe, *branch_policy),
            ManagerView::ConfirmDiscard {
                target,
                exact_path,
                typed_path,
                branch_policy,
            } if typed_path == exact_path => (
                target.clone(),
                WorkspacePruneMode::DiscardFiles {
                    confirmed_path: PathBuf::from(exact_path),
                },
                *branch_policy,
            ),
            ManagerView::ConfirmDiscard { .. } => {
                return Err("type the exact displayed path to discard files".into());
            }
            _ => return Err("no worktree removal is awaiting confirmation".into()),
        };
        self.pending_prune = Some((request_id, target.clone()));
        self.view = ManagerView::Pruning;
        Ok((target, mode, branch_policy))
    }

    pub fn receive_prune(
        &mut self,
        request_id: u64,
        project: NodeId,
        target: WorkspacePruneTarget,
        result: WorkspacePruneResult,
    ) -> bool {
        if self.project != project
            || !self
                .pending_prune
                .as_ref()
                .is_some_and(|(pending_id, pending_target)| {
                    *pending_id == request_id && pending_target == &target
                })
        {
            return false;
        }
        self.pending_prune = None;
        self.view = ManagerView::Result { result };
        self.view_retention = None;
        true
    }

    pub fn hit_test(&self, screen: Rect, position: Position) -> Option<ManagerHit> {
        let layout = layout(screen);
        if !layout.popup.contains(position) {
            return None;
        }
        let count = self
            .inventory
            .as_ref()
            .map_or(0, |value| value.entries.len());
        let content = inventory_content_area(layout.list, count);
        if matches!(self.view, ManagerView::Browsing) && content.contains(position) {
            let (start, end) = visible_window(self.selected, count, layout.list.height as usize);
            let index = start + usize::from(position.y.saturating_sub(layout.list.y));
            return (index < end).then_some(ManagerHit::Row(index));
        }
        if !layout.actions.contains(position) {
            return None;
        }
        let offset = position.x.saturating_sub(layout.actions.x);
        match &self.view {
            ManagerView::Browsing => match offset {
                0..=16 => Some(ManagerHit::Safe),
                17..=35 => Some(ManagerHit::Discard),
                36..=48 => Some(ManagerHit::Refresh),
                49..=59 => Some(ManagerHit::Close),
                _ => None,
            },
            ManagerView::ConfirmSafe { .. } => match offset {
                0..=18 => Some(ManagerHit::ToggleBranch),
                19..=31 => Some(ManagerHit::Confirm),
                32..=43 => Some(ManagerHit::Cancel),
                _ => None,
            },
            ManagerView::ConfirmDiscard { .. } => match offset {
                0..=20 => Some(ManagerHit::ToggleBranch),
                21..=37 => Some(ManagerHit::Confirm),
                38..=49 => Some(ManagerHit::Cancel),
                _ => None,
            },
            ManagerView::Result { .. } | ManagerView::Error(_) => match offset {
                0..=12 => Some(ManagerHit::Refresh),
                13..=23 => Some(ManagerHit::Close),
                _ => None,
            },
            ManagerView::Loading | ManagerView::Pruning => None,
        }
    }
}

fn row_label(row: &WorkspaceInventoryEntry) -> String {
    let owner = match &row.owner {
        WorkspaceInventoryOwner::Owned => "owned",
        WorkspaceInventoryOwner::Foreign => "foreign",
        WorkspaceInventoryOwner::Unavailable { .. } => "unavailable",
    };
    let branch = row.branch.as_deref().unwrap_or("detached");
    format!("[{owner}] {branch}  {}", row.path.display())
}

pub(crate) fn branch_control(
    screen: Rect,
    state: &WorktreeManagerState,
) -> Option<crate::value_control::ValueControl> {
    let (_, policy) = state.confirmation_branch()?;
    let areas = layout(screen);
    let area = areas.branch;
    if area.height == 0 || area.width == 0 {
        return None;
    }
    Some(crate::value_control::ValueControl::new(
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
        crate::value_control::ControlSpec {
            kind: crate::value_control::ControlKind::Choice,
            label: "Branch",
            value: branch_label(policy),
            label_width: 8,
            previous_enabled: true,
            next_enabled: true,
            open_enabled: true,
        },
    ))
}

fn branch_label(policy: WorkspacePruneBranchPolicy) -> &'static str {
    match policy {
        WorkspacePruneBranchPolicy::Keep => "keep branch",
        WorkspacePruneBranchPolicy::DeleteIfSafe => "delete merged branch safely (-d)",
    }
}

pub fn render(frame: &mut Frame, screen: Rect, state: &WorktreeManagerState) {
    let areas = layout(screen);
    frame.render_widget(Clear, areas.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Worktrees")),
        areas.popup,
    );
    let header = state.inventory.as_ref().map_or_else(
        || "Repository inventory pending".to_owned(),
        |inventory| {
            format!(
                "{} registered worktrees{}",
                inventory.total_worktrees,
                if inventory.truncated {
                    " (list truncated; refine manually)"
                } else {
                    ""
                }
            )
        },
    );
    frame.render_widget(
        Paragraph::new(header).style(Style::new().fg(Color::Cyan)),
        areas.header,
    );
    if let Some(inventory) = &state.inventory {
        let (start, end) = visible_window(
            state.selected,
            inventory.entries.len(),
            areas.list.height as usize,
        );
        let rows = inventory.entries[start..end]
            .iter()
            .enumerate()
            .map(|(offset, row)| {
                let selected = start + offset == state.selected;
                let style = if selected {
                    Style::new().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::new()
                };
                Line::from(Span::styled(row_label(row), style))
            })
            .collect::<Vec<_>>();
        let content = inventory_content_area(areas.list, inventory.entries.len());
        frame.render_widget(Paragraph::new(rows), content);
        if content.width < areas.list.width && content.height > 0 {
            let track = Rect::new(content.right(), content.y, 1, content.height);
            let mut scrollbar = ScrollbarState::new(inventory.entries.len())
                .position(start)
                .viewport_content_length(usize::from(content.height));
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│")),
                track,
                &mut scrollbar,
            );
        }
    }
    let detail = match &state.view {
        ManagerView::Loading => "Loading inventory…".to_owned(),
        ManagerView::Browsing => state.selected_entry().map_or_else(
            || "No worktrees in this repository".to_owned(),
            |row| {
                let mut parts = vec![format!(
                    "Safe blockers: {}. Discard blockers: {}.",
                    row.safe_blockers.len(),
                    row.discard_blockers.len()
                )];
                if let WorkspaceInventoryOwner::Unavailable { reason } = &row.owner {
                    parts.push(format!("Ownership: {reason}"));
                }
                if !row.safe_blockers.is_empty() {
                    parts.push(format!("Safe: {}", row.safe_blockers.join("; ")));
                }
                if !row.discard_blockers.is_empty() {
                    parts.push(format!("Discard: {}", row.discard_blockers.join("; ")));
                }
                if let Some(merge_target) = &row.merge_target {
                    parts.push(format!("Merge target: {merge_target}"));
                }
                parts.join(" ")
            },
        ),
        ManagerView::ConfirmSafe {
            target,
            branch_policy,
        } => format!(
            "Remove exact clean, merged worktree {}? Branch: {}. Default is Cancel.",
            target.worktree_root.display(),
            branch_label(*branch_policy)
        ),
        ManagerView::ConfirmDiscard {
            exact_path,
            typed_path,
            branch_policy,
            ..
        } => format!(
            "Discard files in {exact_path}. Type the full path exactly: {typed_path}. Branch: {}. Default is Cancel.",
            branch_label(*branch_policy)
        ),
        ManagerView::Pruning => "Removal in progress. Wait for the correlated result.".to_owned(),
        ManagerView::Result { result } => format!(
            "{:?}; mutation attempted: {}; path: {:?}; registration: {:?}; metadata: {:?}; branch: {:?}. {}",
            result.outcome,
            result.mutation_attempted,
            result.path_present,
            result.registration_present,
            result.metadata_present,
            result.branch_outcome,
            result.reasons.join("; ")
        ),
        ManagerView::Error(error) => format!("Inventory error: {error}"),
    };
    frame.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: false }),
        areas.detail,
    );
    if let Some(control) = branch_control(screen, state) {
        control.render(
            frame,
            crate::value_control::ControlStyles {
                value: theme::selected_style(),
                button: theme::selected_style(),
                ..Default::default()
            },
        );
    }
    let actions = match &state.view {
        ManagerView::Browsing => "[S] Safe remove  [D] Discard files  [R] Refresh  [Esc] Close",
        ManagerView::ConfirmSafe { .. } => "[B] Branch choice  [Y] Confirm  [Esc] Cancel",
        ManagerView::ConfirmDiscard { .. } => "[Tab] Branch choice  [Enter] Confirm  [Esc] Cancel",
        ManagerView::Result { result } if result.outcome == WorkspacePruneOutcome::Uncertain => {
            "Outcome uncertain. Inspect exact path and Git state. [R] Refresh  [Esc] Close"
        }
        ManagerView::Result { .. } | ManagerView::Error(_) => "[R] Refresh  [Esc] Close",
        ManagerView::Pruning => "Removal is running. [Esc] Close; result appears in status",
        ManagerView::Loading => "Please wait…",
    };
    frame.render_widget(
        Paragraph::new(actions).style(Style::new().add_modifier(Modifier::BOLD)),
        areas.actions,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> WorkspacePruneTarget {
        WorkspacePruneTarget {
            repo_common_dir: PathBuf::from("/tmp/repo/.git"),
            worktree_root: PathBuf::from("/tmp/repo.agent"),
            workspace_id: "00000000-0000-4000-8000-000000000001".into(),
            creation_branch: "agent/test".into(),
            base_ref: "refs/heads/main".into(),
            base_commit: "a".repeat(40),
            created_at_unix: 1_700_000_000,
            metadata_directory: PathBuf::from("/tmp/repo/.git/worktrees/agent"),
            root_device: 1,
            root_inode: 2,
            metadata_device: 1,
            metadata_inode: 3,
            expected_head: "a".repeat(40),
        }
    }

    fn ready() -> WorktreeManagerState {
        let mut state = WorktreeManagerState::new(NodeId(7), 10);
        let inventory = WorkspaceInventory {
            repo_common_dir: PathBuf::from("/tmp/repo/.git"),
            control_directory: PathBuf::from("/tmp/repo"),
            total_worktrees: 2,
            truncated: false,
            entries: vec![WorkspaceInventoryEntry {
                path: target().worktree_root.clone(),
                branch: Some("agent/test".into()),
                head: Some("a".repeat(40)),
                is_main: false,
                is_locked: false,
                is_prunable: false,
                owner: WorkspaceInventoryOwner::Owned,
                target: Some(target()),
                occupied_pane_ids: Vec::new(),
                protected_paths: Vec::new(),
                safe_blockers: Vec::new(),
                discard_blockers: Vec::new(),
                merge_target: Some("refs/heads/main".into()),
            }],
        };
        assert!(!state.receive_inventory(11, NodeId(7), Ok(inventory.clone())));
        assert!(state.receive_inventory(10, NodeId(7), Ok(inventory)));
        state
    }

    #[test]
    fn compact_layout_keeps_header_details_and_actions_inside_the_frame() {
        let state = ready();
        for (width, height) in [(120, 35), (80, 24), (40, 12), (12, 6), (2, 2), (1, 1)] {
            let screen = Rect::new(0, 0, width, height);
            let areas = layout(screen);
            for area in [
                areas.popup,
                areas.header,
                areas.branch,
                areas.list,
                areas.detail,
                areas.actions,
            ] {
                assert!(area.right() <= screen.right(), "{area:?} at {screen:?}");
                assert!(area.bottom() <= screen.bottom(), "{area:?} at {screen:?}");
            }
            assert!(areas.header.bottom() <= areas.branch.y);
            assert!(areas.branch.bottom() <= areas.list.y);
            assert!(areas.list.bottom() <= areas.detail.y);
            assert!(areas.detail.bottom() <= areas.actions.y);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, screen, &state))
                .unwrap();
            crate::ui_capture::save(&format!("worktrees-compact-{width}x{height}"), &terminal);
            if areas.actions.height == 0 {
                assert_eq!(state.hit_test(screen, Position::new(0, 0)), None);
            }
        }
        let regular = layout(Rect::new(0, 0, 120, 35));
        assert_eq!(regular.list.height, 14);
        assert_eq!(regular.detail.height, 5);
        assert_eq!(regular.branch.height, 1);
    }

    #[test]
    fn inventory_overflow_track_is_visible_and_does_not_select_a_worktree() {
        let mut state = ready();
        let entry = state.inventory.as_ref().unwrap().entries[0].clone();
        state.inventory.as_mut().unwrap().entries = vec![entry; 30];
        state.selected = 29;
        let screen = Rect::new(0, 0, 120, 35);
        let areas = layout(screen);
        let content = inventory_content_area(areas.list, 30);
        assert_eq!(content.right() + 1, areas.list.right());
        let (start, end) = visible_window(29, 30, usize::from(content.height));
        assert_eq!(end, 30);
        assert_eq!(
            state.hit_test(screen, Position::new(content.x, content.y)),
            Some(ManagerHit::Row(start)),
        );
        assert_eq!(
            state.hit_test(screen, Position::new(content.right(), content.y)),
            None
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 35)).unwrap();
        terminal
            .draw(|frame| render(frame, screen, &state))
            .unwrap();
        crate::ui_capture::save("worktrees-overflow", &terminal);
        let buffer = terminal.backend().buffer();
        assert!((content.y..content.bottom())
            .any(|y| !buffer[(content.right(), y)].symbol().trim().is_empty()));
        assert_eq!(inventory_content_area(areas.list, 1), areas.list);
        assert_eq!(inventory_content_area(Rect::new(0, 0, 1, 1), 30).width, 1);
    }

    #[test]
    fn safe_prune_defaults_to_keep_branch_and_requires_exact_result_identity() {
        let mut state = ready();
        state.begin_safe_confirmation().unwrap();
        let (chosen, mode, branch) = state.confirmed_prune(12).unwrap();
        assert_eq!(chosen, target());
        assert_eq!(mode, WorkspacePruneMode::Safe);
        assert_eq!(branch, WorkspacePruneBranchPolicy::Keep);
        let result = WorkspacePruneResult {
            outcome: WorkspacePruneOutcome::Blocked,
            mutation_attempted: false,
            path_present: None,
            registration_present: None,
            metadata_present: None,
            branch_outcome: ilium_ipc::WorkspacePruneBranchOutcome::Kept,
            reasons: vec!["process still using worktree".into()],
        };
        assert!(!state.receive_prune(13, NodeId(7), target(), result.clone()));
        assert!(!state.receive_prune(12, NodeId(8), target(), result.clone()));
        assert!(state.receive_prune(12, NodeId(7), target(), result.clone()));
        assert_eq!(state.view, ManagerView::Result { result });
    }

    #[test]
    fn discard_requires_the_exact_displayed_path() {
        let mut state = ready();
        state.begin_discard_confirmation().unwrap();
        for character in "/tmp/repo.agen".chars() {
            state.edit_discard_path(KeyCode::Char(character));
        }
        assert!(state.confirmed_prune(12).is_err());
        state.edit_discard_path(KeyCode::Char('t'));
        let (_, mode, branch) = state.confirmed_prune(12).unwrap();
        assert_eq!(
            mode,
            WorkspacePruneMode::DiscardFiles {
                confirmed_path: PathBuf::from("/tmp/repo.agent")
            }
        );
        assert_eq!(branch, WorkspacePruneBranchPolicy::Keep);
    }

    #[test]
    fn action_hits_follow_the_rendered_ascii_labels() {
        let mut state = ready();
        let screen = Rect::new(0, 0, 120, 35);
        let actions = layout(screen).actions;
        let hit = |offset| state.hit_test(screen, Position::new(actions.x + offset, actions.y));
        assert_eq!(hit(0), Some(ManagerHit::Safe));
        assert_eq!(hit(17), Some(ManagerHit::Discard));
        assert_eq!(hit(36), Some(ManagerHit::Refresh));
        assert_eq!(hit(49), Some(ManagerHit::Close));
        state.begin_discard_confirmation().unwrap();
        let hit = |offset| state.hit_test(screen, Position::new(actions.x + offset, actions.y));
        assert_eq!(hit(20), Some(ManagerHit::ToggleBranch));
        assert_eq!(hit(21), Some(ManagerHit::Confirm));
        assert_eq!(hit(38), Some(ManagerHit::Cancel));
    }
    #[test]
    fn branch_catalog_changes_only_policy_and_rejects_reopened_confirmation() {
        use crate::value_dialog::ValueDialogState;
        use crate::value_dialog_host::ValueDialogHost;
        let mut state = ready();
        state.begin_discard_confirmation().unwrap();
        for character in "/tmp/repo.agent".chars() {
            state.edit_discard_path(KeyCode::Char(character));
        }
        let before = state.view.clone();
        let host = ValueDialogHost::prune_branch(&state).unwrap();
        let ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("full branch catalog")
        };
        assert_eq!(dialog.options().len(), 2);
        host.apply_prune_branch_choice(&mut state, "delete-if-safe")
            .unwrap();
        let ManagerView::ConfirmDiscard {
            branch_policy,
            typed_path,
            target: current,
            ..
        } = &state.view
        else {
            panic!("confirmation retained")
        };
        assert_eq!(*branch_policy, WorkspacePruneBranchPolicy::DeleteIfSafe);
        assert_eq!(typed_path, "/tmp/repo.agent");
        assert_eq!(*current, target());
        assert!(state.pending_prune.is_none());
        assert!(host
            .apply_prune_branch_choice(&mut state, "invented")
            .is_err());
        state.cancel_confirmation();
        state.begin_discard_confirmation().unwrap();
        assert!(host
            .apply_prune_branch_choice(&mut state, "delete-if-safe")
            .is_err());
        let ManagerView::ConfirmDiscard { branch_policy, .. } = &state.view else {
            panic!("reopened")
        };
        assert_eq!(*branch_policy, WorkspacePruneBranchPolicy::Keep);
        assert_ne!(state.view, before);
    }

    #[test]
    fn branch_chrome_matches_actual_render_without_reducing_path_detail() {
        use crate::value_control::{ControlAction, PointerButton};
        use ratatui::{backend::TestBackend, Terminal};
        let mut state = ready();
        state.begin_safe_confirmation().unwrap();
        for width in [40, 80, 120] {
            let screen = Rect::new(0, 0, width, 35);
            let control = branch_control(screen, &state).unwrap();
            let g = control.geometry();
            let mut terminal = Terminal::new(TestBackend::new(width, 35)).unwrap();
            terminal
                .draw(|frame| render(frame, screen, &state))
                .unwrap();
            for (rect, glyph) in [(g.previous, "←"), (g.open, "+"), (g.next, "→")] {
                assert_eq!(
                    terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                    glyph
                );
            }
            assert_eq!(
                control.hit(Position::new(g.value.x, g.value.y), PointerButton::Left),
                Some(ControlAction::NextChoice)
            );
            assert_eq!(
                control.hit(Position::new(g.value.x, g.value.y), PointerButton::Right),
                Some(ControlAction::PreviousChoice)
            );
            assert_eq!(
                control.hit(Position::new(g.label.x, g.label.y), PointerButton::Left),
                None
            );
            assert!(g.row.y < layout(screen).list.y);
        }
    }
}
