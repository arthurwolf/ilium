//! Focus hand-over when the server replaces a pane (unfreeze gives the resumed agent a new pane id).

use ilium_core::NodeId;

use crate::app::{App, UNFREEZING_AGENT_STATUS};

/// Where a replaced pane sat, for handing it focus to the new pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplacementFocus {
    pub old_pane_id: NodeId,
    pub parent: NodeId,
    pub index: usize,
    pub was_focused: bool,
}

impl App {
    pub(crate) fn remember_replacement_focus(&mut self, pane_id: NodeId) {
        let Some(parent) = self.tree.parent_of(pane_id) else {
            return;
        };
        let Some(index) = self
            .tree
            .children_of(parent)
            .ok()
            .and_then(|children| children.iter().position(|child| *child == pane_id))
        else {
            return;
        };
        self.pending_replacement_focus = Some(ReplacementFocus {
            old_pane_id: pane_id,
            parent,
            index,
            was_focused: self.active_pane_id() == Some(pane_id),
        });
    }

    /// Called after every tree snapshot: once the replaced pane is gone, the
    /// pane now at its position takes over focus when the old one had it.
    pub fn apply_pending_replacement_focus(&mut self) {
        let Some(pending) = self.pending_replacement_focus else {
            return;
        };
        if self.tree.get(pending.old_pane_id).is_some() {
            return;
        }
        self.pending_replacement_focus = None;
        if self.status_message.as_deref() == Some(UNFREEZING_AGENT_STATUS) {
            self.status_message = None;
        }
        if !pending.was_focused {
            return;
        }
        let replacement = self
            .tree
            .children_of(pending.parent)
            .ok()
            .and_then(|children| children.get(pending.index).copied())
            .filter(|id| self.tree.get(*id).is_some_and(|node| node.is_pane()));
        if let Some(replacement) = replacement {
            self.focus_pane(replacement);
        }
    }
}
