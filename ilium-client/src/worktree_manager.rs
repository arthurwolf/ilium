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
use ratatui::widgets::{Clear, Paragraph, Wrap};
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
    pub selected: usize,
    pub inventory: Option<WorkspaceInventory>,
    pub view: ManagerView,
    pending_inventory_request: Option<u64>,
    pending_prune: Option<(u64, WorkspacePruneTarget)>,
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
    pub list: Rect,
    pub actions: Rect,
    pub detail: Rect,
}

pub fn layout(screen: Rect) -> ManagerLayout {
    let popup = modal::centered_fixed_rect(100, 24, screen);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let list_height = inner.height.saturating_sub(8);
    ManagerLayout {
        popup,
        list: Rect::new(inner.x, inner.y.saturating_add(2), inner.width, list_height),
        detail: Rect::new(
            inner.x,
            inner.y.saturating_add(2).saturating_add(list_height),
            inner.width,
            5.min(inner.height),
        ),
        actions: Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    }
}

fn visible_window(selected: usize, count: usize, height: usize) -> (usize, usize) {
    if count <= height {
        return (0, count);
    }
    let start = selected.saturating_sub(height / 2).min(count - height);
    (start, start + height)
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
            selected: 0,
            inventory: None,
            view: ManagerView::Loading,
            pending_inventory_request: Some(request_id),
            pending_prune: None,
        }
    }

    pub fn begin_refresh(&mut self, request_id: u64) {
        self.pending_inventory_request = Some(request_id);
        self.pending_prune = None;
        self.view = ManagerView::Loading;
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
                self.view = ManagerView::Browsing;
            }
            Err(error) => self.view = ManagerView::Error(error),
        }
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
        self.view = ManagerView::ConfirmDiscard {
            target,
            exact_path,
            typed_path: String::new(),
            branch_policy: WorkspacePruneBranchPolicy::Keep,
        };
        Ok(())
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
            || self.pending_prune.as_ref() != Some(&(request_id, target.clone()))
        {
            return false;
        }
        self.pending_prune = None;
        self.view = ManagerView::Result { result };
        true
    }

    pub fn hit_test(&self, screen: Rect, position: Position) -> Option<ManagerHit> {
        let layout = layout(screen);
        if !layout.popup.contains(position) {
            return None;
        }
        if matches!(self.view, ManagerView::Browsing) && layout.list.contains(position) {
            let count = self
                .inventory
                .as_ref()
                .map_or(0, |value| value.entries.len());
            let (start, end) = visible_window(self.selected, count, layout.list.height as usize);
            let index = start + usize::from(position.y.saturating_sub(layout.list.y));
            return (index < end).then_some(ManagerHit::Row(index));
        }
        if position.y != layout.actions.y {
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
        Rect::new(
            areas.list.x,
            areas.list.y.saturating_sub(2),
            areas.list.width,
            1,
        ),
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
        frame.render_widget(Paragraph::new(rows), areas.list);
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
}
